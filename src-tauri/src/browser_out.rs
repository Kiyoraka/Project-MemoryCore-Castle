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
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

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
#[allow(dead_code)] // used by the tab drag's card
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
#[allow(dead_code)] // used by the tab drag
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drop {
    /// Into a new window at the cursor.
    Out,
    /// Back into the browser's home.
    In,
    /// Nowhere: it stays where it was.
    Stay,
}

#[allow(dead_code)] // used by the tab drag
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
#[allow(dead_code)] // used by the tab drag
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

/// The view a drag's card opens with: the dragged tab's title, how many pages, whether one is
/// private, and the window they come from.
#[allow(dead_code)] // used by the tab drag's card
pub fn ghost_script(title: &str, count: usize, private: bool, from: &str) -> String {
    let view = serde_json::json!({ "view": "browser-ghost", "title": title, "count": count, "private": private, "from": from });
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
        let ghost = ghost_script("Threads", 1, false, HOME);
        assert!(ghost.contains("\"view\":\"browser-ghost\"") && ghost.contains("\"title\":\"Threads\"") && ghost.contains("\"from\":\"home\""));
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
}
