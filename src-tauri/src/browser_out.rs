//! A BROWSER tab in a window of its own: the live page moves out of the browser's home into a window
//! `browser-out-<n>` (onto another monitor, say) and back again. The page itself never reloads: its
//! child webview `browser-tab-<id>` is moved with `Webview::reparent` (Tauri's `unstable` feature), so
//! a private tab stays the same private page and every sign-in, a playing video and a half-typed reply
//! survive the move.
//!
//! - The home is `browser::host`: the browser's own window, or Castle's main window when the browser
//!   is a room inside Castle. Each tab names the window it lives in (`Tab.window`).
//! - **Closing a window of its own closes its tabs for good** (its x, the OS close, or its last tab
//!   closing). Bringing it back moves the pages home first, so nothing is lost there.
//! - The pages keep every guard: the same webviews keep their navigation guard, the platform's
//!   phishing warning and the DANGER screen.
//! - Windows are built only in async commands (a window built in a synchronous one deadlocks, wry#583).
//! - The window Castle's own page runs in is named by `capabilities/browser-out.json`; the pages moved
//!   into it are named by no capability.

use crate::browser::{self, tab_label, Browser};
use crate::browser_tabs::{Tab, HOME, MAX_PANES};
use crate::settings::{OutPlace, SettingsStore};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

const OUT_PREFIX: &str = "browser-out-";
const GHOST_PREFIX: &str = "browser-ghost-";
/// Said to every browser surface after pages came home: their ids, shown again (as a group for 2-4).
pub const POPPED_IN_EVENT: &str = "browser-popped-in";
/// A window of its own's first size (logical pixels) until one is resized.
pub const DEFAULT_SIZE: (f64, f64) = (1100.0, 760.0);
/// The smallest a window of its own may be (logical pixels).
const MIN_SIZE: (f64, f64) = (480.0, 320.0);
/// How far up-left of the cursor a new window's corner lands (logical): the cursor ends on its header.
const GRAB: (f64, f64) = (110.0, 18.0);
/// The title bar's height a window adds over its page (logical), counted when it is kept on a monitor.
const FRAME_H: f64 = 32.0;
/// How long a window must rest after a move or resize before its place is kept.
const PLACE_REST: Duration = Duration::from_millis(400);

/// The windows' own counters: labels are never reused in a run, so an old event can never reach a
/// new window.
static OUTS: AtomicU32 = AtomicU32::new(0);
static GHOSTS: AtomicU32 = AtomicU32::new(0);

pub fn out_label(n: u32) -> String {
    format!("{OUT_PREFIX}{n}")
}

pub fn ghost_label(n: u32) -> String {
    format!("{GHOST_PREFIX}{n}")
}

/// The next window of its own's label, never one used before in this run.
fn next_out() -> String {
    out_label(OUTS.fetch_add(1, Ordering::Relaxed) + 1)
}

/// The next drag card's label, never one used before in this run (closing the last one is not
/// immediate, so a quick second drag gets a fresh name).
fn next_ghost() -> String {
    ghost_label(GHOSTS.fetch_add(1, Ordering::Relaxed) + 1)
}

/// Whether a window is one of the browser's windows of its own.
pub fn is_out(label: &str) -> bool {
    label.strip_prefix(OUT_PREFIX).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

// ---------- the pure parts ----------

/// A rectangle on the desktop, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Area {
    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Where a dragged tab goes when it is let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drop {
    /// Into a new window at the cursor.
    Out,
    /// Back into the browser's home.
    In,
    /// Nowhere: it stays where it was.
    Stay,
}

impl Drop {
    pub fn word(self) -> &'static str {
        match self {
            Drop::Out => "out",
            Drop::In => "in",
            Drop::Stay => "stay",
        }
    }
}

/// The drop decided (pure): a tab dragged from the home and let go outside the home's window opens in
/// a new window; a window of its own's tab let go over the home's window goes back in; anything else
/// stays. A hidden or minimized home (`home_area` None) has no window to drop onto.
pub fn drop_decision(from: &str, cursor: (f64, f64), home_area: Option<Area>) -> Drop {
    let over_home = home_area.is_some_and(|a| a.contains(cursor));
    if from == HOME {
        if over_home {
            Drop::Stay
        } else {
            Drop::Out
        }
    } else if is_out(from) && over_home {
        Drop::In
    } else {
        Drop::Stay
    }
}

/// A new window's corner when it opens at the cursor (pure): a little up-left of it (`grab`), kept
/// inside the monitor under the cursor; a window bigger than the monitor keeps its top-left corner on
/// it. All in physical pixels.
pub fn corner_at(cursor: (f64, f64), size: (f64, f64), grab: (f64, f64), monitor: Area) -> (i32, i32) {
    let fit = |want: f64, len: f64, start: f64, room: f64| want.min(start + room - len).max(start).round() as i32;
    (fit(cursor.0 - grab.0, size.0, monitor.x, monitor.w), fit(cursor.1 - grab.1, size.1, monitor.y, monitor.h))
}

/// A window of its own's title (pure): the first page's title and how many more pages ride with it,
/// a private one marked.
pub fn out_title(list: &[&Tab]) -> String {
    let Some(first) = list.first() else { return "MemoryCore · Browser".into() };
    let title: String = first.title.chars().take(80).collect();
    let more = if list.len() > 1 { format!(" +{}", list.len() - 1) } else { String::new() };
    let private = if list.iter().any(|t| t.private) { "Private · " } else { "" };
    format!("{private}{title}{more} · MemoryCore")
}

/// The view a window of its own opens with, set before Castle's page loads in it: which window it is
/// and the pages it took, plus the bare-page marker (no start-up screen).
pub fn out_script(label: &str, ids: &[u32]) -> String {
    let view = serde_json::json!({ "view": "browser-out", "window": label, "ids": ids });
    format!("window.__CASTLE_VIEW__ = {view};{}", browser::BARE_PAGE)
}

/// The view a drag's card opens with: the dragged tab's title and site (none for a private page),
/// how many pages, whether one is private, and the window they come from.
pub fn ghost_script(title: &str, site: &str, count: usize, private: bool, from: &str) -> String {
    let view = serde_json::json!({ "view": "browser-ghost", "title": title, "site": site, "count": count, "private": private, "from": from });
    format!("window.__CASTLE_VIEW__ = {view};{}", browser::BARE_PAGE)
}

/// The tabs that may leave a window together (pure): those named that live there, each once, four at
/// most (one tab, or a group of two to four side by side).
pub fn movable(list: &[Tab], ids: &[u32], from: &str) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for id in ids {
        if list.iter().any(|t| t.id == *id && t.window == from) && !out.contains(id) {
            out.push(*id);
        }
    }
    out.truncate(MAX_PANES);
    out
}

// ---------- the remembered place ----------

fn saved_place(app: &AppHandle) -> Option<OutPlace> {
    app.state::<SettingsStore>().get().browser_out_place
}

/// A window of its own's last place kept in the settings (only when it changed).
fn save_place(app: &AppHandle, place: OutPlace) {
    let store = app.state::<SettingsStore>();
    let snapshot = {
        let mut current = store.current.lock().unwrap();
        if current.browser_out_place == Some(place) {
            return;
        }
        current.browser_out_place = Some(place);
        current.clone()
    };
    let _ = crate::settings::save(&store.path, &snapshot);
}

/// Every move and resize bumps this; the place is written once the window has rested a moment.
static PLACE_ROUND: AtomicU64 = AtomicU64::new(0);

/// The window's place read and kept once it has rested after a move or resize: never while it is
/// minimized, maximized, full screen or hidden (none is where it was left), and only while its corner
/// is still on a connected monitor.
fn remember_soon(app: &AppHandle, label: &str) {
    let round = PLACE_ROUND.fetch_add(1, Ordering::Relaxed) + 1;
    let (app, label) = (app.clone(), label.to_string());
    std::thread::spawn(move || {
        std::thread::sleep(PLACE_REST);
        if PLACE_ROUND.load(Ordering::Relaxed) != round {
            return;
        }
        let Some(w) = app.get_window(&label) else { return };
        if w.is_minimized().unwrap_or(true) || w.is_maximized().unwrap_or(true) || w.is_fullscreen().unwrap_or(true) || !w.is_visible().unwrap_or(false) {
            return;
        }
        let (Ok(pos), Ok(size), Ok(k)) = (w.outer_position(), w.inner_size(), w.scale_factor()) else { return };
        let place = OutPlace { x: pos.x, y: pos.y, w: size.width as f64 / k, h: size.height as f64 / k };
        if k > 0.0 && size.width > 0 && size.height > 0 && on_a_monitor(&app, &place) {
            save_place(&app, place);
        }
    });
}

/// Whether a remembered corner is on a connected monitor.
fn on_a_monitor(app: &AppHandle, place: &OutPlace) -> bool {
    app.monitor_from_point(place.x as f64 + 40.0, place.y as f64 + 16.0).ok().flatten().is_some()
}

/// A new window put where it belongs: at the cursor (a drop), else where one was last left (still on
/// a monitor), else centred.
fn position(app: &AppHandle, window: &WebviewWindow, place: Option<OutPlace>, at_cursor: bool, size: (f64, f64)) {
    if at_cursor {
        if let Ok(c) = app.cursor_position() {
            if let Ok(Some(m)) = app.monitor_from_point(c.x, c.y) {
                let k = m.scale_factor();
                let area = m.work_area();
                let monitor = Area { x: area.position.x as f64, y: area.position.y as f64, w: area.size.width as f64, h: area.size.height as f64 };
                let (x, y) = corner_at((c.x, c.y), (size.0 * k, (size.1 + FRAME_H) * k), (GRAB.0 * k, GRAB.1 * k), monitor);
                let _ = window.set_position(PhysicalPosition::new(x, y));
                // a monitor of another scale keeps the size chosen
                let _ = window.set_size(LogicalSize::new(size.0, size.1));
                return;
            }
        }
    }
    match place.filter(|p| on_a_monitor(app, p)) {
        Some(p) => {
            let _ = window.set_position(PhysicalPosition::new(p.x, p.y));
            let _ = window.set_size(LogicalSize::new(size.0, size.1));
        }
        None => {
            let _ = window.center();
        }
    }
}

// ---------- out and back in ----------

/// The windows whose pages are being brought home right now: their closing never ends a tab.
static POPPING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A window of its own closed by its x or the OS: its tabs end for good, each as `browser_close` ends
/// one. A window whose pages are being brought home ends nothing.
fn end_tabs(app: &AppHandle, label: &str) {
    if POPPING.lock().unwrap().iter().any(|l| l == label) {
        return;
    }
    let ids = browser::tabs(app).in_window(label);
    if ids.is_empty() {
        return;
    }
    for id in ids {
        browser::close_page(app, id);
    }
    browser::forget_placed(label);
    let _ = browser::after_closing(app, false);
}

/// A window of its own gone, its tabs left alone (they were closed or brought home first).
pub(crate) fn destroy_out(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_window(label) {
        let _ = window.destroy();
    }
}

/// Every window of its own gone (Browser opens changed: their tabs were closed first).
pub(crate) fn destroy_all(app: &AppHandle) {
    for (label, window) in app.windows() {
        if is_out(&label) {
            let _ = window.destroy();
        }
    }
}

/// A window of its own closed for good: the sign-ins kept first (as the browser window does before it
/// closes), then its tabs ended, then the window. Blocks on WebKit: call it off the main thread.
fn close_for_good(app: &AppHandle, label: &str) {
    browser::keep_sign_ins_now(app);
    end_tabs(app, label);
    destroy_out(app, label);
}

/// A window of its own's events: a move or resize remembered; the OS close ends its tabs (off the
/// window's thread, the sign-ins kept first); destroyed any other way, its tabs end there and then.
fn watch(app: &AppHandle, window: &WebviewWindow) {
    let (app, label) = (app.clone(), window.label().to_string());
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => remember_soon(&app, &label),
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let (app, label) = (app.clone(), label.clone());
            std::thread::spawn(move || close_for_good(&app, &label));
        }
        WindowEvent::Destroyed => {
            end_tabs(&app, &label);
            browser::forget_placed(&label);
            POPPING.lock().unwrap().retain(|l| l != &label);
        }
        _ => {}
    });
}

/// Home tabs moved into a new window of their own: the same pages (never rebuilt), at the cursor or
/// the remembered place, its size the remembered one. A page with its phone layout is its desktop self
/// again (the phone layout is for three or four side by side at home). Returns the window's label.
fn pop_out(app: &AppHandle, ids: &[u32], at_cursor: bool) -> Result<String, String> {
    let now = browser::tabs(app);
    // only tabs with a page move: a new tab's page is Castle's own and stays at home
    let ids: Vec<u32> = movable(&now.list, ids, HOME).into_iter().filter(|id| app.get_webview(&tab_label(*id)).is_some()).collect();
    if ids.is_empty() {
        return Err("That tab has no page to move - open a page in it first.".into());
    }
    let moving: Vec<&Tab> = ids.iter().filter_map(|id| now.list.iter().find(|t| t.id == *id)).collect();
    let label = next_out();
    let place = saved_place(app);
    let size = place.map(|p| (p.w.max(MIN_SIZE.0), p.h.max(MIN_SIZE.1))).unwrap_or(DEFAULT_SIZE);
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(out_title(&moving))
        .inner_size(size.0, size.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .visible(false)
        .initialization_script(out_script(&label, &ids));
    // the title bar stays and the content starts below it, as in the browser's own window
    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Transparent);
    let window = builder.build().map_err(|_| "The new window could not be opened.".to_string())?;
    position(app, &window, place, at_cursor, size);
    let host = app.get_window(&label).ok_or("The new window could not be opened.")?;
    for tab in &moving {
        if let Some(view) = app.get_webview(&tab_label(tab.id)) {
            if tab.phone {
                crate::browser_native::set_user_agent(&view, tab_label(tab.id), None);
            }
            // hidden until the new window has measured its page area and placed it
            let _ = view.hide();
            let _ = view.reparent(&host);
        }
    }
    // moved first, then named: the window that places a page is always the one it sits in by then
    app.state::<Browser>().0.lock().unwrap().send_out(&ids, &label);
    watch(app, &window);
    let _ = window.show();
    let _ = window.set_focus();
    browser::layout(app);
    browser::tell(app);
    Ok(label)
}

/// A window of its own's pages moved back into the home (brought forward, opened first when it was
/// closed), hidden until the home places them; the window gone, its tabs kept; every browser surface
/// told which came home. Returns their ids.
fn pop_in(app: &AppHandle, label: &str) -> Result<Vec<u32>, String> {
    if !is_out(label) || app.get_window(label).is_none() {
        return Err("That window is closed.".into());
    }
    let home = browser::home_window(app)?;
    POPPING.lock().unwrap().push(label.to_string());
    let ids = browser::tabs(app).in_window(label);
    for id in &ids {
        if let Some(view) = app.get_webview(&tab_label(*id)) {
            let _ = view.hide();
            let _ = view.reparent(&home);
        }
    }
    app.state::<Browser>().0.lock().unwrap().come_home(&ids);
    destroy_out(app, label);
    browser::forget_placed(label);
    browser::layout(app);
    browser::tell(app);
    browser::emit_browsers(app, POPPED_IN_EVENT, &ids);
    Ok(ids)
}

/// Each window of its own's title follows its pages (called with every tab change).
pub(crate) fn retitle(app: &AppHandle, list: &[Tab]) {
    for (label, window) in app.windows() {
        if is_out(&label) {
            let mine: Vec<&Tab> = list.iter().filter(|t| t.window == label).collect();
            if !mine.is_empty() {
                let _ = window.set_title(&out_title(&mine));
            }
        }
    }
}

/// Move to a new window: the named home tabs (a group goes together, four at most) into a window of
/// their own, at the cursor (a drop) or at the remembered place. Async: it builds a window (wry#583).
#[tauri::command]
pub async fn browser_pop_out(app: AppHandle, ids: Vec<u32>, at_cursor: bool) -> Result<String, String> {
    pop_out(&app, &ids, at_cursor)
}

/// Back to the home: a window of its own's pages home again, the window gone.
#[tauri::command]
pub async fn browser_pop_in(app: AppHandle, window: String) -> Result<Vec<u32>, String> {
    pop_in(&app, &window)
}

/// A window of its own's x: its tabs closed for good, then the window (Castle's page there may not
/// close a window itself).
#[tauri::command]
pub async fn browser_out_close(app: AppHandle, window: String) -> Result<(), String> {
    if !is_out(&window) {
        return Err("That is not a browser window.".into());
    }
    tauri::async_runtime::spawn_blocking(move || close_for_good(&app, &window)).await.map_err(|_| "The window could not be closed.".to_string())
}

/// A window of its own full screen, or back. Returns whether it is full screen now.
#[tauri::command]
pub async fn browser_out_fullscreen(app: AppHandle, window: String, on: bool) -> Result<bool, String> {
    let w = app.get_window(&window).filter(|_| is_out(&window)).ok_or("That window is closed.")?;
    w.set_fullscreen(on).map_err(|_| "The window could not change.".to_string())?;
    Ok(w.is_fullscreen().unwrap_or(on))
}

/// A tab that is out pressed at home: its window brought to the front.
#[tauri::command]
pub async fn browser_out_focus(app: AppHandle, window: String) -> Result<(), String> {
    let w = app.get_window(&window).filter(|_| is_out(&window)).ok_or("That window is closed.")?;
    let _ = w.unminimize();
    let _ = w.show();
    w.set_focus().map_err(|_| "The window could not come forward.".to_string())
}

// ---------- the drag ----------

/// Said to the drag's card while it follows the cursor: "out", "in" or "stay" if let go now.
pub const GHOST_STATE_EVENT: &str = "browser-ghost-state";
/// Said to the drag's card when the page's little picture is ready (it asks for it then).
#[cfg_attr(not(windows), allow(dead_code))] // only Windows pictures the page
pub const GHOST_THUMB_EVENT: &str = "browser-ghost-thumb";
/// Said to every browser surface when the core made the drop itself (the release never reached the
/// page, or the drag ran too long): `{ from, decision }`, so the lifted chip settles.
pub const DRAG_ENDED_EVENT: &str = "browser-drag-ended";
/// The drag's card window (logical pixels): the card and the pill under it.
const GHOST_SIZE: (f64, f64) = (260.0, 190.0);
/// How far right and down of the cursor the card's corner sits (logical pixels).
const GHOST_OFFSET: (f64, f64) = (18.0, 14.0);
/// How often the card follows the cursor.
const FOLLOW_EVERY: Duration = Duration::from_millis(16);
/// The state is said again this often (in follow steps) even when unchanged, for a card that started
/// listening late.
const SAY_AGAIN: u32 = 12;
/// The longest a drag lasts when its end never comes.
const DRAG_MOST: Duration = Duration::from_secs(120);
/// How long the button may be up before the follow thread makes the drop itself (the release
/// normally reaches the page first).
const RELEASE_WAIT: Duration = Duration::from_millis(450);
/// How long the card waits for the page's picture.
#[cfg_attr(not(windows), allow(dead_code))] // only Windows pictures the page
const THUMB_WAIT: Duration = Duration::from_secs(4);

/// The drag under way: its round, the tabs dragged, the window they come from and the card's label.
struct Drag {
    round: u64,
    ids: Vec<u32>,
    from: String,
    ghost: String,
}

static DRAG: Mutex<Option<Drag>> = Mutex::new(None);
static DRAGGING: AtomicBool = AtomicBool::new(false);
static DRAG_ROUND: AtomicU64 = AtomicU64::new(0);
/// The dragged page's little picture (a data URL), for the card to ask for.
static THUMB: Mutex<Option<String>> = Mutex::new(None);

/// The card's title (pure): the first page's title and how many more go with it.
pub fn ghost_title(list: &[&Tab]) -> String {
    let Some(first) = list.first() else { return String::new() };
    let title: String = first.title.chars().take(80).collect();
    if list.len() > 1 {
        format!("{title} +{}", list.len() - 1)
    } else {
        title
    }
}

/// The site a page is on, for the card (pure): its host without `www.`, or nothing.
pub fn site_of(url: Option<&str>) -> String {
    url.and_then(|u| tauri::Url::parse(u).ok()).and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_default()
}

/// Whether the dragged pages may be pictured on the card (pure): never a private page, never one
/// under the DANGER screen.
pub fn may_picture(list: &[&Tab]) -> bool {
    !list.is_empty() && list.iter().all(|t| !t.private && t.danger.is_none())
}

/// The card's corner for a cursor (pure): a little right of and below it, in physical pixels at the
/// cursor's monitor scale.
pub fn ghost_corner(cursor: (f64, f64), scale: f64) -> (i32, i32) {
    ((cursor.0 + GHOST_OFFSET.0 * scale).round() as i32, (cursor.1 + GHOST_OFFSET.1 * scale).round() as i32)
}

/// Whether a release was lost (pure): the button has been up long enough and no end came.
pub fn release_lost(up_for: Option<Duration>) -> bool {
    up_for.is_some_and(|d| d >= RELEASE_WAIT)
}

/// The home's window on the desktop, or None while it is closed, hidden or minimized (nothing to
/// drop onto). Never opens or brings it forward.
fn home_area(app: &AppHandle) -> Option<Area> {
    let w = browser::host(app)?;
    if !w.is_visible().unwrap_or(false) || w.is_minimized().unwrap_or(false) {
        return None;
    }
    let (pos, size) = (w.outer_position().ok()?, w.outer_size().ok()?);
    Some(Area { x: pos.x as f64, y: pos.y as f64, w: size.width as f64, h: size.height as f64 })
}

fn destroy_ghost(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_window(label) {
        let _ = window.destroy();
    }
}

/// Whether the main mouse button is held (the left one, or the right when they are swapped).
#[cfg(windows)] // platform: windows-only
fn button_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
    // SAFETY: plain Win32 queries of the mouse's state; nothing is handed over.
    unsafe {
        let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { VK_RBUTTON } else { VK_LBUTTON };
        (GetAsyncKeyState(key as i32) as u16 & 0x8000) != 0
    }
}

/// Elsewhere the page's own release decides; the drag's time cap ends a stuck one.
#[cfg(not(windows))]
fn button_down() -> bool {
    true
}

/// The card follows the cursor about every 16 ms until the drag ends, saying what a drop would do
/// now (the home's window read once, as the drag began; the drop itself reads it again). On Windows a
/// release that never reached the page is noticed here and the drop made; a drag that runs past its
/// cap ends with nothing moved.
fn follow(app: AppHandle, round: u64, from: String, ghost: String) {
    std::thread::spawn(move || {
        let home = home_area(&app);
        let started = Instant::now();
        let mut shown = false;
        let mut said: Option<Drop> = None;
        let mut steps: u32 = 0;
        let mut up_since: Option<Instant> = None;
        while DRAGGING.load(Ordering::Relaxed) && DRAG_ROUND.load(Ordering::Relaxed) == round {
            if started.elapsed() >= DRAG_MOST {
                cancel(&app, round);
                return;
            }
            let Some(window) = app.get_window(&ghost) else { break };
            if let Ok(c) = app.cursor_position() {
                let k = app.monitor_from_point(c.x, c.y).ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
                let (x, y) = ghost_corner((c.x, c.y), k);
                let _ = window.set_position(PhysicalPosition::new(x, y));
                if !shown {
                    let _ = window.show();
                    shown = true;
                }
                let now = drop_decision(&from, (c.x, c.y), home);
                if said != Some(now) || steps % SAY_AGAIN == 0 {
                    said = Some(now);
                    let _ = app.emit_to(ghost.as_str(), GHOST_STATE_EVENT, now.word());
                }
            }
            steps = steps.wrapping_add(1);
            // the release went past every window and never reached the page: the drop is made here
            if button_down() {
                up_since = None;
            } else if release_lost(Some(up_since.get_or_insert_with(Instant::now).elapsed())) {
                let decision = finish(&app, &from).unwrap_or(Drop::Stay);
                browser::emit_browsers(&app, DRAG_ENDED_EVENT, serde_json::json!({ "from": from, "decision": decision.word() }));
                return;
            }
            std::thread::sleep(FOLLOW_EVERY);
        }
    });
}

/// A drag that ran past its cap: its card gone, nothing moved, the lifted chip told to settle.
fn cancel(app: &AppHandle, round: u64) {
    let drag = {
        let mut kept = DRAG.lock().unwrap();
        if kept.as_ref().is_some_and(|d| d.round == round) {
            kept.take()
        } else {
            None
        }
    };
    let Some(drag) = drag else { return };
    DRAGGING.store(false, Ordering::Relaxed);
    *THUMB.lock().unwrap() = None;
    destroy_ghost(app, &drag.ghost);
    browser::emit_browsers(app, DRAG_ENDED_EVENT, serde_json::json!({ "from": drag.from, "decision": Drop::Stay.word() }));
}

/// The dragged page's little picture, taken once the card is up (Windows: the page's own DevTools
/// screenshot, a small JPEG). Kept for the card to ask for, then the card is told it is ready.
#[cfg(windows)] // platform: windows-only
fn picture_soon(app: &AppHandle, round: u64, id: u32, ghost: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(view) = app.get_webview(&tab_label(id)) else { return };
        let Ok(data) = crate::browser_native::capture(&view, r#"{"format":"jpeg","quality":50}"#, THUMB_WAIT).await else { return };
        if data.is_empty() || DRAG_ROUND.load(Ordering::Relaxed) != round || !DRAGGING.load(Ordering::Relaxed) {
            return;
        }
        *THUMB.lock().unwrap() = Some(format!("data:image/jpeg;base64,{data}"));
        let _ = app.emit_to(ghost.as_str(), GHOST_THUMB_EVENT, ());
    });
}

/// macOS has no page picture: the card shows the title and the site.
#[cfg(not(windows))]
fn picture_soon(_app: &AppHandle, _round: u64, _id: u32, _ghost: String) {}

/// A chip dragged past 8 px (from the home's strip or a window of its own's header): a small glass
/// card with the page's title, site and little picture (never for a private page or one under the
/// DANGER screen) follows the cursor, see-through to clicks and never focused. Each drag gets a card
/// of its own (closing the last one is not immediate). Async: it builds a window (wry#583).
#[tauri::command]
pub async fn browser_drag_start(app: AppHandle, ids: Vec<u32>, from: String) -> Result<(), String> {
    if from != HOME && !is_out(&from) {
        return Err("That is not a browser window.".into());
    }
    let now = browser::tabs(&app);
    // only tabs with a page can move: a new tab's page is Castle's own
    let ids: Vec<u32> = movable(&now.list, &ids, &from).into_iter().filter(|id| app.get_webview(&tab_label(*id)).is_some()).collect();
    let moving: Vec<&Tab> = ids.iter().filter_map(|id| now.list.iter().find(|t| t.id == *id)).collect();
    let Some(first) = moving.first() else { return Err("That tab has no page to move - open a page in it first.".into()) };
    let round = DRAG_ROUND.fetch_add(1, Ordering::Relaxed) + 1;
    let ghost = next_ghost();
    let old = DRAG.lock().unwrap().replace(Drag { round, ids: ids.clone(), from: from.clone(), ghost: ghost.clone() });
    if let Some(old) = old {
        destroy_ghost(&app, &old.ghost);
    }
    DRAGGING.store(true, Ordering::Relaxed);
    *THUMB.lock().unwrap() = None;
    let private = moving.iter().any(|t| t.private);
    let site = if private { String::new() } else { site_of(first.url.as_deref()) };
    let script = ghost_script(&ghost_title(&moving), &site, moving.len(), private, &from);
    let builder = WebviewWindowBuilder::new(&app, &ghost, WebviewUrl::App("index.html".into()));
    // see-through needs the private API on macOS; there the card sits on the theme's ground instead
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true);
    let window = builder
        .title("MemoryCore · Moving a tab")
        .inner_size(GHOST_SIZE.0, GHOST_SIZE.1)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .initialization_script(script)
        .build()
        .map_err(|_| "The tab could not be lifted.".to_string())?;
    let _ = window.set_ignore_cursor_events(true);
    // let go while the card was being made
    if !DRAGGING.load(Ordering::Relaxed) || DRAG_ROUND.load(Ordering::Relaxed) != round {
        let _ = window.destroy();
        return Ok(());
    }
    let pictured = may_picture(&moving).then_some(first.id);
    follow(app.clone(), round, from, ghost.clone());
    if let Some(id) = pictured {
        picture_soon(&app, round, id, ghost);
    }
    Ok(())
}

/// The card's picture, when one was taken for this drag.
#[tauri::command]
pub fn browser_ghost_thumb() -> Option<String> {
    THUMB.lock().unwrap().clone()
}

/// Let go: the card gone, then the drop decided from where the cursor is - a new window at the cursor
/// ("out"), back into the home ("in"), or nothing ("stay"). Returns that word. Async: a drop out
/// builds a window (wry#583).
#[tauri::command]
pub async fn browser_drag_end(app: AppHandle, from: String) -> Result<String, String> {
    finish(&app, &from).map(|d| d.word().to_string())
}

/// The drag ended (the page's release, or the follow thread's own look at the button): whichever
/// takes the drag first makes the drop, so it is made once. The card always goes.
fn finish(app: &AppHandle, from: &str) -> Result<Drop, String> {
    let drag = DRAG.lock().unwrap().take();
    let Some(drag) = drag else { return Ok(Drop::Stay) };
    DRAGGING.store(false, Ordering::Relaxed);
    *THUMB.lock().unwrap() = None;
    destroy_ghost(app, &drag.ghost);
    if drag.from != from {
        return Ok(Drop::Stay);
    }
    let cursor = app.cursor_position().map(|c| (c.x, c.y)).map_err(|_| "The cursor could not be read.".to_string())?;
    let decision = drop_decision(from, cursor, home_area(app));
    match decision {
        Drop::Out => {
            pop_out(app, &drag.ids, true)?;
        }
        Drop::In => {
            pop_in(app, from)?;
        }
        Drop::Stay => {}
    }
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser_tabs::Tabs;

    #[test]
    fn a_window_label_is_never_used_twice() {
        let (a, b, c) = (next_out(), next_out(), next_out());
        assert!(a != b && b != c && a != c);
        assert!(is_out(&a) && is_out(&b) && is_out(&c));
        assert_eq!(out_label(7), "browser-out-7");
        assert!(is_out("browser-out-12"));
        for not in ["main", "browser", "browser-ui", "browser-out-", "browser-out-x", "browser-tab-3", "browser-ghost-1", "browser-popup-2"] {
            assert!(!is_out(not), "{not}");
        }
        let (g, h) = (next_ghost(), next_ghost());
        assert!(g != h && g.starts_with("browser-ghost-") && !is_out(&g));
        assert_eq!(ghost_label(2), "browser-ghost-2");
    }

    #[test]
    fn a_drop_outside_the_home_opens_a_window_and_over_it_comes_home() {
        let home = Some(Area { x: 0.0, y: 0.0, w: 1920.0, h: 1040.0 });
        // from the home: out past its edge, nothing inside it
        assert_eq!(drop_decision(HOME, (2500.0, 300.0), home), Drop::Out);
        assert_eq!(drop_decision(HOME, (-10.0, 300.0), home), Drop::Out);
        assert_eq!(drop_decision(HOME, (900.0, 300.0), home), Drop::Stay);
        assert_eq!(drop_decision(HOME, (1920.0, 300.0), home), Drop::Out, "the right edge is outside");
        // from a window of its own: home over the home's window, nothing elsewhere
        assert_eq!(drop_decision("browser-out-3", (900.0, 300.0), home), Drop::In);
        assert_eq!(drop_decision("browser-out-3", (2500.0, 300.0), home), Drop::Stay);
        // a hidden home has no window to drop onto
        assert_eq!(drop_decision("browser-out-3", (900.0, 300.0), None), Drop::Stay);
        assert_eq!(drop_decision(HOME, (900.0, 300.0), None), Drop::Out);
        // a stranger never moves anything
        assert_eq!(drop_decision("world-2", (900.0, 300.0), home), Drop::Stay);
        assert_eq!((Drop::Out.word(), Drop::In.word(), Drop::Stay.word()), ("out", "in", "stay"));
    }

    #[test]
    fn a_new_window_opens_up_left_of_the_cursor_and_stays_on_its_monitor() {
        let second = Area { x: 1920.0, y: 0.0, w: 2560.0, h: 1400.0 };
        // in the open: the grab offset
        assert_eq!(corner_at((3000.0, 500.0), (1100.0, 790.0), (110.0, 18.0), second), (2890, 482));
        // near the right and bottom edges: pulled back inside
        assert_eq!(corner_at((4400.0, 1350.0), (1100.0, 790.0), (110.0, 18.0), second), (4480 - 1100, 1400 - 790));
        // near the left and top edges: never onto the monitor before it
        assert_eq!(corner_at((1950.0, 5.0), (1100.0, 790.0), (110.0, 18.0), second), (1920, 0));
        // bigger than the monitor: its corner stays on it
        let small = Area { x: -1280.0, y: 0.0, w: 1280.0, h: 720.0 };
        assert_eq!(corner_at((-600.0, 300.0), (1650.0, 1185.0), (165.0, 27.0), small), (-1280, 0));
    }

    #[test]
    fn a_window_of_its_own_is_named_after_its_page_and_knows_its_pages() {
        let mut tabs = Tabs::default();
        let a = tabs.open(Some("https://threads.example/".into())).unwrap();
        let b = tabs.open(Some("https://video.example/".into())).unwrap();
        let p = tabs.open_with(Some("https://secret.example/".into()), true).unwrap();
        for (id, title) in [(a, "Threads"), (b, "Video"), (p, "Secret")] {
            tabs.get_mut(id).unwrap().title = title.into();
        }
        let tab = |id: u32| tabs.list.iter().find(|t| t.id == id).unwrap();
        assert_eq!(out_title(&[tab(a)]), "Threads · MemoryCore");
        assert_eq!(out_title(&[tab(a), tab(b)]), "Threads +1 · MemoryCore");
        assert_eq!(out_title(&[tab(p)]), "Private · Secret · MemoryCore");
        assert_eq!(out_title(&[]), "MemoryCore · Browser");
        let script = out_script("browser-out-4", &[1, 2]);
        assert!(script.starts_with("window.__CASTLE_VIEW__ = {"));
        assert!(script.contains("\"view\":\"browser-out\"") && script.contains("\"window\":\"browser-out-4\"") && script.contains("\"ids\":[1,2]"));
        assert!(script.contains("data-castle-bare"), "no start-up screen in the window");
        let ghost = ghost_script("Threads", "threads.example", 1, false, HOME);
        assert!(ghost.contains("\"view\":\"browser-ghost\"") && ghost.contains("\"title\":\"Threads\"") && ghost.contains("\"from\":\"home\""));
        assert!(ghost.contains("\"site\":\"threads.example\"") && ghost.contains("data-castle-bare"));
    }

    #[test]
    fn only_home_tabs_named_move_each_once_and_four_at_most() {
        let mut tabs = Tabs::default();
        let ids: Vec<u32> = (0..6).map(|_| tabs.open(Some("https://x.example/".into())).unwrap()).collect();
        tabs.get_mut(ids[5]).unwrap().window = "browser-out-1".into();
        assert_eq!(movable(&tabs.list, &[ids[1], ids[1], 99, ids[5]], HOME), vec![ids[1]]);
        assert_eq!(movable(&tabs.list, &ids, HOME), ids[..4].to_vec(), "a group is four at most");
        assert_eq!(movable(&tabs.list, &ids, "browser-out-1"), vec![ids[5]]);
    }

    #[test]
    fn the_drag_card_is_titled_placed_and_pictured_only_when_it_may_be() {
        let mut tabs = Tabs::default();
        let a = tabs.open(Some("https://www.threads.example/@castle".into())).unwrap();
        let b = tabs.open(Some("https://video.example/".into())).unwrap();
        let p = tabs.open_with(Some("https://secret.example/".into()), true).unwrap();
        for (id, title) in [(a, "Threads"), (b, "Video"), (p, "Secret")] {
            tabs.get_mut(id).unwrap().title = title.into();
        }
        let tab = |id: u32| tabs.list.iter().find(|t| t.id == id).unwrap();
        assert_eq!(ghost_title(&[tab(a)]), "Threads");
        assert_eq!(ghost_title(&[tab(a), tab(b)]), "Threads +1");
        assert_eq!(ghost_title(&[]), "");
        assert_eq!(site_of(tab(a).url.as_deref()), "threads.example");
        assert_eq!(site_of(None), "");
        assert_eq!(site_of(Some("not an address")), "");
        // a private page, or one under the DANGER screen, is never pictured
        assert!(may_picture(&[tab(a), tab(b)]));
        assert!(!may_picture(&[tab(a), tab(p)]));
        assert!(!may_picture(&[]));
        let mut held = tab(b).clone();
        held.danger = Some(crate::browser_tabs::Danger { url: "https://bad.example/".into(), threat: "phishing".into(), reason: "A copy of a sign-in page".into(), caught_by: vec!["sentinel".into()] });
        assert!(!may_picture(&[&held]));
        // right of and below the cursor, scaled with its monitor
        assert_eq!(ghost_corner((100.0, 50.0), 1.0), (118, 64));
        assert_eq!(ghost_corner((3000.0, 500.0), 1.5), (3027, 521));
    }

    #[test]
    fn a_lost_release_is_noticed_only_after_the_wait() {
        assert!(!release_lost(None), "the button still held");
        assert!(!release_lost(Some(Duration::from_millis(100))));
        assert!(release_lost(Some(RELEASE_WAIT)));
        assert!(release_lost(Some(Duration::from_secs(2))));
        assert_eq!(RELEASE_WAIT, Duration::from_millis(450));
        assert_eq!(DRAG_MOST, Duration::from_secs(120));
    }
}
