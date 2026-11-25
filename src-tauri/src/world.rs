//! WORLD on the other screens: one frameless window per screen the member switched on, filling
//! it, showing that screen's world with that screen's camera. The WORLD room sends the whole list
//! (world_apply); open windows are told their new settings without a reload. Windows are created
//! only from async commands (a window built from a sync command deadlocks on Windows), and they
//! hide and show with Castle. Castle's own screen never gets a world window.

use crate::screen;
use crate::settings::{self, Settings, SettingsStore, WorldScreen};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Monitor, State, WebviewUrl, WebviewWindowBuilder};

const PREFIX: &str = "world-";
const IDENTIFY: &str = "identify-";
const IDENTIFY_MS: u64 = 2200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub primary: bool,
    /// the screen Castle itself is on
    pub is_main: bool,
    /// a WORLD window is open on it
    pub open: bool,
}

/// A screen as the matching sees it (pure, so it can be tested without monitors).
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenBox {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

/// What a world window is told: its settings and whether it plays the music.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldConfig {
    pub view: &'static str,
    pub screen: usize,
    pub lead: bool,
    pub music: bool,
    pub volume: f32,
    pub config: WorldScreen,
}

fn label(index: usize) -> String {
    format!("{PREFIX}{index}")
}

/// The saved entry for a screen: the same name first, else the same size and place.
pub fn matches<'a>(entries: &'a [WorldScreen], screen: &ScreenBox) -> Option<&'a WorldScreen> {
    entries
        .iter()
        .find(|e| !e.screen.is_empty() && e.screen == screen.name)
        .or_else(|| entries.iter().find(|e| e.width == screen.width && e.height == screen.height && e.x == screen.x && e.y == screen.y))
}

/// Which screens get a world window, and with what: (screen index, entry). Castle's own screen
/// (`main`) never does; a screen with no entry, or one switched off, stays empty.
pub fn wanted(entries: &[WorldScreen], screens: &[ScreenBox], main: Option<usize>) -> Vec<(usize, WorldScreen)> {
    screens
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != main)
        .filter_map(|(i, s)| matches(entries, s).filter(|e| e.on).map(|e| (i, e.clone())))
        .collect()
}

/// Should the world windows be brought in line with these settings? Only while the WORLD room is
/// on and at least one screen is switched on: with the room off, nothing may open a world window.
pub fn windows_wanted(settings: &Settings) -> bool {
    settings.room_on("world") && settings.world.screens.iter().any(|e| e.on)
}

fn boxes(monitors: &[Monitor]) -> Vec<ScreenBox> {
    monitors
        .iter()
        .enumerate()
        .map(|(i, m)| ScreenBox {
            name: m.name().cloned().unwrap_or_else(|| format!("Screen {}", i + 1)),
            width: m.size().width,
            height: m.size().height,
            x: m.position().x,
            y: m.position().y,
        })
        .collect()
}

fn main_index(app: &AppHandle, monitors: &[Monitor]) -> Option<usize> {
    let main = app.get_window("main")?.current_monitor().ok().flatten()?;
    monitors.iter().position(|m| m.position() == main.position())
}

#[tauri::command]
pub fn world_screens(app: AppHandle) -> Vec<ScreenInfo> {
    let monitors = app.available_monitors().unwrap_or_default();
    let primary = app.primary_monitor().ok().flatten().map(|m| *m.position());
    let main = main_index(&app, &monitors);
    boxes(&monitors)
        .into_iter()
        .enumerate()
        .map(|(index, b)| ScreenInfo {
            index,
            primary: primary.is_some_and(|p| p.x == b.x && p.y == b.y),
            is_main: Some(index) == main,
            open: app.get_webview_window(&label(index)).is_some(),
            name: b.name,
            width: b.width,
            height: b.height,
            x: b.x,
            y: b.y,
        })
        .collect()
}

/// Opens, retunes or closes every world window to match the saved screens.
async fn apply(app: &AppHandle, entries: &[WorldScreen]) -> Result<(), String> {
    let monitors = app.available_monitors().map_err(|_| "The screens could not be read.".to_string())?;
    let want = wanted(entries, &boxes(&monitors), main_index(app, &monitors));
    let lead = want.first().map(|(i, _)| *i);
    let sound = app.state::<SettingsStore>().get().world;
    for (name, window) in app.webview_windows() {
        let Some(index) = name.strip_prefix(PREFIX).and_then(|n| n.parse::<usize>().ok()) else { continue };
        if !want.iter().any(|(i, _)| *i == index) {
            let _ = window.close();
        }
    }
    for (index, entry) in want {
        let payload = WorldConfig { view: "world", screen: index, lead: Some(index) == lead, music: sound.music, volume: sound.volume, config: entry };
        if let Some(existing) = app.get_webview_window(&label(index)) {
            let _ = app.emit_to(label(index), "castle-world-config", &payload);
            // re-fit every time, not just at creation: an older window kept open across settings
            // changes must not keep whatever bounds it had before, or it stays mis-sized/mis-placed
            let _ = screen::fill_monitor(&existing.as_ref().window(), &monitors[index]);
            let _ = existing.show();
            continue;
        }
        let init = serde_json::to_string(&payload).map_err(|_| "The world settings could not be read.".to_string())?;
        let window = WebviewWindowBuilder::new(app, label(index), WebviewUrl::App("index.html".into()))
            .title("MemoryCore Castle - World")
            .decorations(false)
            // like Castle's own window: no resize border or shadow, or Windows grows the window past the screen
            .resizable(false)
            .shadow(false)
            .skip_taskbar(true)
            .visible(false)
            .initialization_script(&format!("window.__CASTLE_VIEW__ = {init};{NO_START_SCREEN}"))
            .build()
            .map_err(|_| "The world window could not be opened.".to_string())?;
        screen::fill_monitor(&window.as_ref().window(), &monitors[index]).map_err(|_| "The world window could not be placed.".to_string())?;
        window.show().map_err(|_| "The world window could not be shown.".to_string())?;
    }
    Ok(())
}

/// A window made for another screen has no start-up screen: this marks the page, before it first
/// paints, so index.html's CSS keeps the screen from flashing there.
const NO_START_SCREEN: &str = "(function(){var m=function(){var d=document.documentElement;if(d){d.setAttribute('data-castle-bare','');}};m();new MutationObserver(m).observe(document,{childList:true});})();";

/// The WORLD room's whole list of screens: saved, then every window made to match.
#[tauri::command]
pub async fn world_apply(app: AppHandle, screens: Vec<WorldScreen>, store: State<'_, SettingsStore>) -> Result<Vec<WorldScreen>, String> {
    crate::rooms::need(&store, "world")?;
    let mut next = store.get();
    next.world.screens = screens;
    let next = next.normalized();
    settings::save(&store.path, &next).map_err(|_| "The screens could not be saved.".to_string())?;
    let saved = next.world.screens.clone();
    *store.current.lock().unwrap() = next;
    apply(&app, &saved).await?;
    Ok(saved)
}

/// Moves Castle itself to another screen and keeps it there (the next start too). The screen it
/// left may now show a world; the one it joined cannot, so any world window there closes. With the
/// WORLD room off Castle just moves: no world window opens.
#[tauri::command]
pub async fn castle_move_to_screen(app: AppHandle, index: usize, store: State<'_, SettingsStore>) -> Result<(), String> {
    let monitors = app.available_monitors().map_err(|_| "The screens could not be read.".to_string())?;
    let monitor = monitors.get(index).ok_or("That screen is not connected.")?.clone();
    let window = app.get_window("main").ok_or("Castle's window is not open.")?;
    let mut next = store.get();
    next.castle_screen = monitor.name().cloned().unwrap_or_default();
    settings::save(&store.path, &next).map_err(|_| "The screen could not be saved.".to_string())?;
    let entries = next.world.screens.clone();
    let worlds = windows_wanted(&next);
    *store.current.lock().unwrap() = next;
    screen::fill_work_area(&window, &monitor).map_err(|_| "Castle could not be moved.".to_string())?;
    if worlds {
        apply(&app, &entries).await?;
    }
    Ok(())
}

/// At start: puts Castle on the screen the member chose, if it is still connected. False when
/// there is none (the caller then fills the screen it opened on).
pub fn place_saved(app: &AppHandle, window: &tauri::Window) -> bool {
    let chosen = app.state::<SettingsStore>().get().castle_screen;
    if chosen.is_empty() {
        return false;
    }
    let Ok(monitors) = app.available_monitors() else { return false };
    monitors.iter().find(|m| m.name().is_some_and(|n| *n == chosen)).is_some_and(|m| screen::fill_work_area(window, m).is_ok())
}

/// The WORLD music: on / off and how loud. Saves only these two, so it can never write an older
/// copy of the screens back; the open world windows hear it at once. With the WORLD room off the
/// choice is still saved, but no world window opens for it.
#[tauri::command]
pub async fn world_music(app: AppHandle, music: Option<bool>, volume: Option<f32>, store: State<'_, SettingsStore>) -> Result<(), String> {
    let mut next = store.get();
    if let Some(music) = music {
        next.world.music = music;
    }
    if let Some(volume) = volume {
        next.world.volume = volume.clamp(0.0, 1.0);
    }
    settings::save(&store.path, &next).map_err(|_| "The music setting could not be saved.".to_string())?;
    let entries = next.world.screens.clone();
    let worlds = windows_wanted(&next);
    *store.current.lock().unwrap() = next;
    if worlds {
        apply(&app, &entries).await?;
    }
    Ok(())
}

/// Shows a big number on one screen for a moment, so the member knows which is which.
#[tauri::command]
pub async fn world_identify(app: AppHandle, index: usize) -> Result<(), String> {
    let monitors = app.available_monitors().map_err(|_| "The screens could not be read.".to_string())?;
    let monitor = monitors.get(index).ok_or("That screen is not connected.")?.clone();
    let name = format!("{IDENTIFY}{index}");
    if let Some(old) = app.get_webview_window(&name) {
        let _ = old.close();
    }
    let init = format!("window.__CASTLE_VIEW__ = {{ \"view\": \"identify\", \"number\": {} }};", index + 1);
    let builder = WebviewWindowBuilder::new(&app, &name, WebviewUrl::App("index.html".into()));
    // see-through needs the private API on macOS; there the number sits on a dark sheet instead
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true);
    let window = builder
        .title("MemoryCore Castle - Identify")
        .decorations(false)
        .resizable(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .initialization_script(&format!("{init}{NO_START_SCREEN}"))
        .build()
        .map_err(|_| "The screen number could not be shown.".to_string())?;
    screen::fill_monitor(&window.as_ref().window(), &monitor).map_err(|_| "The screen number could not be placed.".to_string())?;
    let _ = window.set_ignore_cursor_events(true);
    window.show().map_err(|_| "The screen number could not be shown.".to_string())?;
    tauri::async_runtime::spawn(async move {
        let _ = tauri::async_runtime::spawn_blocking(|| std::thread::sleep(std::time::Duration::from_millis(IDENTIFY_MS))).await;
        let _ = window.close();
    });
    Ok(())
}

/// The points a covered-check looks at: the middle and four points near the corners.
pub fn probe_points(x: i32, y: i32, width: u32, height: u32) -> [(i32, i32); 5] {
    let at = |fx: f64, fy: f64| (x + (width as f64 * fx) as i32, y + (height as f64 * fy) as i32);
    [at(0.5, 0.5), at(0.2, 0.2), at(0.8, 0.2), at(0.2, 0.8), at(0.8, 0.8)]
}

/// Can the member see any of this world window? Hidden, minimised, or covered at every probe
/// point by another window means no.
#[cfg(windows)]
fn seen(window: &tauri::WebviewWindow) -> bool {
    use windows_sys::Win32::Foundation::{HWND, POINT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetAncestor, WindowFromPoint, GA_ROOT};
    if !window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
        return false;
    }
    let (Ok(own), Ok(pos), Ok(size)) = (window.hwnd(), window.outer_position(), window.outer_size()) else { return true };
    let own = own.0 as HWND;
    probe_points(pos.x, pos.y, size.width, size.height).iter().any(|&(x, y)| {
        // SAFETY: plain Win32 queries with a point; no memory is handed over
        let top = unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) };
        top == own
    })
}

#[cfg(not(windows))]
fn seen(window: &tauri::WebviewWindow) -> bool {
    window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false)
}

/// Every 1.5 s each world window learns whether it can be seen (`world-visible`), so a covered
/// world stops drawing.
pub fn watch_covered(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut last: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let windows: Vec<_> = app.webview_windows().into_iter().filter(|(name, _)| name.starts_with(PREFIX)).collect();
            last.retain(|name, _| windows.iter().any(|(n, _)| n == name));
            for (name, window) in windows {
                let now = seen(&window);
                if last.get(&name) != Some(&now) {
                    let _ = app.emit_to(name.as_str(), "world-visible", now);
                    last.insert(name, now);
                }
            }
        }
    });
}

/// The WORLD room was switched off: every world window closes (the saved screens stay).
pub fn close_all(app: &AppHandle) {
    for (name, window) in app.webview_windows() {
        if name.starts_with(PREFIX) {
            let _ = window.close();
        }
    }
}

/// At start (or when the WORLD room is switched back on): the screens the member left on come back.
pub fn restore(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let settings = app.state::<SettingsStore>().get();
        if !windows_wanted(&settings) {
            return;
        }
        if let Err(error) = apply(&app, &settings.world.screens).await {
            eprintln!("MemoryCore Castle: the world screens could not be restored: {error}");
        }
    });
}

/// The world windows hide and show with Castle (the tray, the hotkey, the hide button).
pub fn set_visible(app: &AppHandle, visible: bool) {
    for (name, window) in app.webview_windows() {
        if name.starts_with(PREFIX) {
            let _ = if visible { window.show() } else { window.hide() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(name: &str, x: i32) -> ScreenBox {
        ScreenBox { name: name.into(), width: 1920, height: 1080, x, y: 0 }
    }

    fn entry(name: &str, x: i32, on: bool, world: &str) -> WorldScreen {
        WorldScreen { screen: name.into(), width: 1920, height: 1080, x, y: 0, on, world: world.into(), ..WorldScreen::default() }
    }

    #[test]
    fn a_screen_is_found_by_name_first_then_by_size_and_place() {
        let entries = vec![entry("\\\\.\\DISPLAY9", 1920, true, "3d/crystal-isle"), entry("\\\\.\\DISPLAY5", 0, true, "2d/mine")];
        // the same name wins even after the screen moved
        assert_eq!(matches(&entries, &screen("\\\\.\\DISPLAY5", 3840)).unwrap().world, "2d/mine");
        // a renamed screen in the same place keeps its world
        assert_eq!(matches(&entries, &screen("\\\\.\\DISPLAY2", 1920)).unwrap().world, "3d/crystal-isle");
        assert!(matches(&entries, &screen("\\\\.\\DISPLAY3", 7680)).is_none());
    }

    #[test]
    fn only_switched_on_screens_get_a_window_and_never_castles_own() {
        let screens = vec![screen("A", 0), screen("B", 1920), screen("C", 3840)];
        let entries = vec![entry("A", 0, true, "3d/floating-isles"), entry("B", 1920, false, "2d/mine"), entry("C", 3840, true, "2d/mine")];
        let want = wanted(&entries, &screens, Some(0));
        assert_eq!(want.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn the_covered_check_looks_at_the_middle_and_near_each_corner() {
        let points = probe_points(1920, 0, 1920, 1080);
        assert_eq!(points[0], (2880, 540));
        assert!(points.iter().all(|&(x, y)| (1920..3840).contains(&x) && (0..1080).contains(&y)));
        assert_eq!(points.len(), 5);
    }

    #[test]
    fn nothing_saved_means_every_screen_stays_off() {
        let screens = vec![screen("A", 0), screen("B", 1920)];
        assert!(wanted(&[], &screens, Some(0)).is_empty());
    }

    #[test]
    fn with_the_world_room_off_no_world_window_is_opened() {
        let mut settings = Settings::default();
        assert!(!windows_wanted(&settings), "nothing switched on");
        settings.world.screens = vec![entry("B", 1920, true, "2d/mine")];
        assert!(windows_wanted(&settings));
        settings.rooms.insert("world".into(), false);
        assert!(!windows_wanted(&settings), "the music switch or a Castle move must not reopen worlds while the room is off");
    }

    #[test]
    fn a_new_member_hears_no_music_until_they_turn_it_on_but_a_saved_choice_stays() {
        assert!(!Settings::default().world.music, "a fresh install starts quiet");
        let missing: Settings = serde_json::from_str(r#"{"world":{"volume":0.4}}"#).unwrap();
        assert!(!missing.world.music);
        let on: Settings = serde_json::from_str(r#"{"world":{"music":true,"volume":0.4}}"#).unwrap();
        assert!(on.world.music, "a settings file that saved the music on keeps it on");
        assert_eq!(on.world.volume, 0.4);
    }
}
