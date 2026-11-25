//! How much of the screen Castle takes. Every sizing decision lives here.

use crate::settings::{self, SettingsStore};
use tauri::{AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, Result, Window};

/// How much of its screen Castle takes (Settings' `window_layout`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Native full screen: on macOS its own Space with the menu bar and Dock hidden; on Windows
    /// the whole monitor, taskbar and all.
    FullScreen,
    /// The whole work area: the menu bar and Dock (or the taskbar) stay ("Full width").
    Full,
    Left,
    Right,
    /// The member sizes the window; Castle only remembers it.
    Free,
}

impl Layout {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "left" => Self::Left,
            "right" => Self::Right,
            "free" => Self::Free,
            "fullscreen" => Self::FullScreen,
            _ => Self::Full,
        }
    }

    pub fn as_setting(self) -> &'static str {
        match self {
            Self::FullScreen => "fullscreen",
            Self::Full => "full",
            Self::Left => "left",
            Self::Right => "right",
            Self::Free => "free",
        }
    }
}

/// What the layout menu, the keys and Esc ask for: a layout, or "toggle-fullscreen" (Ctrl+Cmd+F,
/// F11) and "leave-fullscreen" (Esc). Gives the new layout and the one to come back to after
/// full screen; leaving full screen returns to the layout the member had before it.
fn resolve(requested: &str, current: Layout, before: Layout) -> (Layout, Layout) {
    // full screen is never the layout to come back to
    let before = if before == Layout::FullScreen { Layout::Full } else { before };
    let wanted = match requested {
        "toggle-fullscreen" if current == Layout::FullScreen => before,
        "toggle-fullscreen" => Layout::FullScreen,
        "leave-fullscreen" if current == Layout::FullScreen => before,
        "leave-fullscreen" => current,
        other => Layout::from_setting(other),
    };
    let before = if wanted == Layout::FullScreen && current != Layout::FullScreen { current } else { before };
    (wanted, before)
}

fn before_of(app: &AppHandle) -> Layout {
    Layout::from_setting(&app.state::<SettingsStore>().get().window_layout_before)
}

fn save_layout(app: &AppHandle, layout: Layout, before: Layout) -> std::result::Result<(), String> {
    let store = app.state::<SettingsStore>();
    let mut next = store.get();
    next.window_layout = layout.as_setting().into();
    next.window_layout_before = before.as_setting().into();
    settings::save(&store.path, &next).map_err(|_| "The window layout could not be saved.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(())
}

fn layout_of(app: &AppHandle) -> (Layout, Option<[i32; 4]>) {
    let s = app.state::<SettingsStore>().get();
    (Layout::from_setting(&s.window_layout), s.window_bounds)
}

/// The part of a work area a layout takes; None for the free layout (the member's own size).
fn target(area: DesktopArea, layout: Layout) -> Option<DesktopArea> {
    let half = area.width / 2;
    match layout {
        Layout::FullScreen | Layout::Full => Some(area),
        Layout::Left => Some(DesktopArea { width: half, ..area }),
        Layout::Right => Some(DesktopArea { x: area.x + half as i32, width: area.width - half, ..area }),
        Layout::Free => None,
    }
}

/// The free layout's first size when nothing is saved: two thirds of the work area, centred.
fn first_free(area: DesktopArea) -> DesktopArea {
    let (width, height) = (area.width * 2 / 3, area.height * 5 / 6);
    DesktopArea { x: area.x + ((area.width - width) / 2) as i32, y: area.y + ((area.height - height) / 2) as i32, width, height }
}

/// Puts the main window where its layout says, on the monitor whose work area is `area`.
fn place(window: &Window, area: DesktopArea) -> Result<()> {
    let (layout, bounds) = layout_of(window.app_handle());
    // macOS only lets a resizable window into full screen
    window.set_resizable(matches!(layout, Layout::Free | Layout::FullScreen))?;
    if layout == Layout::FullScreen {
        // macOS gives a window that is not on screen yet only the frame of full screen, not its
        // Space: before the first show it takes the work area, and the watcher goes full screen
        if window.is_visible().unwrap_or(false) {
            return window.set_fullscreen(true);
        }
        return apply_desktop_area(window, area);
    }
    if window.is_fullscreen().unwrap_or(false) {
        // macOS leaves full screen with an animation; the watcher fits the window once it is out
        return window.set_fullscreen(false);
    }
    match target(area, layout) {
        Some(part) => apply_desktop_area(window, part),
        None => {
            let free = bounds
                .filter(|b| b[2] > 200 && b[3] > 200)
                .map(|b| DesktopArea { x: b[0], y: b[1], width: b[2] as u32, height: b[3] as u32 })
                .unwrap_or_else(|| first_free(area));
            apply_desktop_area(window, free)
        }
    }
}

/// The layout menu: saves the member's choice and applies it at once.
#[tauri::command]
pub fn castle_window_layout(app: AppHandle, layout: String) -> std::result::Result<String, String> {
    let (current, _) = layout_of(&app);
    let (next, before) = resolve(&layout, current, before_of(&app));
    save_layout(&app, next, before)?;
    let window = app.get_window("main").ok_or("Castle's window is not open.")?;
    fill_desktop(&window).map_err(|_| "Castle's window could not be resized.".to_string())?;
    Ok(next.as_setting().into())
}

/// The Window menu's window size items (macOS menu bar): menu id, label, key.
/// Full screen has no key here: Tauri's View > Enter Full Screen already holds Ctrl+Cmd+F, and the
/// watcher follows that one like a menu choice.
#[cfg(any(target_os = "macos", test))]
const MENU_LAYOUTS: [(&str, &str, Option<&str>); 5] = [
    ("layout:fullscreen", "Full Screen", None),
    ("layout:full", "Full Width", Some("Ctrl+Alt+Enter")),
    ("layout:left", "Left Half", Some("Ctrl+Alt+ArrowLeft")),
    ("layout:right", "Right Half", Some("Ctrl+Alt+ArrowRight")),
    ("layout:free", "Free: Drag the Edges", None),
];

/// Tauri's default macOS menu, with the window sizes at the top of its Window menu.
#[cfg(target_os = "macos")]
pub fn app_menu(app: &AppHandle) -> Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, WINDOW_SUBMENU_ID};
    let menu = Menu::default(app)?;
    if let Some(window_menu) = menu.get(WINDOW_SUBMENU_ID).and_then(|kind| kind.as_submenu().cloned()) {
        for (at, (id, label, key)) in MENU_LAYOUTS.iter().enumerate() {
            window_menu.insert(&MenuItem::with_id(app, *id, *label, true, *key)?, at)?;
        }
        window_menu.insert(&PredefinedMenuItem::separator(app)?, MENU_LAYOUTS.len())?;
    }
    Ok(menu)
}

/// A Window menu choice: the same as the top bar's window size menu.
pub fn on_menu_event(app: &AppHandle, id: &str) {
    let Some(layout) = id.strip_prefix("layout:") else { return };
    if let Err(error) = castle_window_layout(app.clone(), layout.to_string()) {
        eprintln!("MemoryCore Castle: {error}");
    }
    told(app);
}

/// Tells the top bar the layout changed outside it (its Esc needs to know about full screen).
fn told(app: &AppHandle) {
    let _ = tauri::Emitter::emit(app, "window-layout-changed", ());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DesktopArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn desktop_area(window: &Window) -> Result<Option<DesktopArea>> {
    let monitor = match window.current_monitor()? {
        Some(monitor) => Some(monitor),
        None => window.primary_monitor()?,
    };
    Ok(monitor.map(|monitor| {
        let area = monitor.work_area();
        DesktopArea { x: area.position.x, y: area.position.y, width: area.size.width, height: area.size.height }
    }))
}

fn apply_desktop_area(window: &Window, area: DesktopArea) -> Result<()> {
    window.set_position(PhysicalPosition::new(area.x, area.y))?;
    window.set_size(PhysicalSize::new(area.width, area.height))?;
    Ok(())
}

fn needs_fit(previous: Option<DesktopArea>, current: DesktopArea) -> bool {
    current.width > 0 && current.height > 0 && previous != Some(current)
}

/// Fills a whole monitor, taskbar and all: a world window on one of the extra screens.
pub fn fill_monitor(window: &Window, monitor: &Monitor) -> Result<()> {
    window.set_position(PhysicalPosition::new(monitor.position().x, monitor.position().y))?;
    window.set_size(PhysicalSize::new(monitor.size().width, monitor.size().height))?;
    Ok(())
}

/// Places Castle on one given monitor's work area (the screen the member chose), in its layout.
pub fn fill_work_area(window: &Window, monitor: &Monitor) -> Result<()> {
    let area = monitor.work_area();
    place(window, DesktopArea { x: area.position.x, y: area.position.y, width: area.size.width, height: area.size.height })
}

/// Places Castle on its monitor's work area in its layout: by default the whole desktop, with the
/// Windows taskbar still showing.
pub fn fill_desktop(window: &Window) -> Result<()> {
    if let Some(area) = desktop_area(window)? {
        place(window, area)?;
    }
    Ok(())
}

/// Windows can change its work area without resizing our manually placed, borderless window
/// (for example when taskbar auto-hide is toggled). Query fresh monitor bounds, but only move
/// the main window after a change. World windows intentionally keep their full-monitor bounds.
pub fn watch_desktop(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut fitted = None;
        // full screen asked for and seen: from then on, a window out of full screen means the
        // member left it another way (the Mac's Window menu), so Castle follows
        let mut seen_full_screen = false;
        // full screen asked of a window that was not on screen yet (Castle's start)
        let mut asked_full_screen = false;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let Some(window) = app.get_window("main") else { break };
            if !window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(true) {
                // Refit on return, even if the work area changed and then changed back.
                fitted = None;
                continue;
            }
            let (layout, bounds) = layout_of(&app);
            let in_full_screen = window.is_fullscreen().unwrap_or(false);
            if layout == Layout::FullScreen || in_full_screen {
                // the 1-second refit pauses: macOS owns the window while it is in full screen
                fitted = None;
                if layout != Layout::FullScreen {
                    // entered from the Mac's own menu: remember it like a menu choice
                    if save_layout(&app, Layout::FullScreen, layout).is_ok() {
                        told(&app);
                    }
                    seen_full_screen = true;
                } else if in_full_screen {
                    seen_full_screen = true;
                } else if !seen_full_screen && !asked_full_screen {
                    asked_full_screen = true;
                    let _ = window.set_resizable(true);
                    let _ = window.set_fullscreen(true);
                } else if seen_full_screen {
                    seen_full_screen = false;
                    if save_layout(&app, before_of(&app), before_of(&app)).is_ok() {
                        told(&app);
                    }
                    let _ = window.set_resizable(before_of(&app) == Layout::Free);
                }
                continue;
            }
            seen_full_screen = false;
            asked_full_screen = false;
            if layout == Layout::Free {
                // the member's own size: only remember where they put it
                fitted = None;
                remember_bounds(&app, &window, bounds);
                continue;
            }
            if let Ok(Some(area)) = desktop_area(&window) {
                let Some(part) = target(area, layout) else { continue };
                if needs_fit(fitted, part) {
                    match apply_desktop_area(&window, part) {
                        Ok(()) => fitted = Some(part),
                        Err(error) => eprintln!("MemoryCore Castle: desktop size could not be refreshed: {error}"),
                    }
                }
            }
        }
    });
}

/// The free layout: saves the window's place and size when the member has moved or resized it.
fn remember_bounds(app: &AppHandle, window: &Window, saved: Option<[i32; 4]>) {
    let (Ok(at), Ok(size)) = (window.outer_position(), window.outer_size()) else { return };
    let now = [at.x, at.y, size.width as i32, size.height as i32];
    if saved == Some(now) || window.is_maximized().unwrap_or(false) {
        return;
    }
    let store = app.state::<SettingsStore>();
    let mut next = store.get();
    next.window_bounds = Some(now);
    if settings::save(&store.path, &next).is_ok() {
        *store.current.lock().unwrap() = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_takes_its_part_of_the_work_area() {
        let wide = DesktopArea { x: 0, y: 25, width: 5120, height: 1415 };
        assert_eq!(target(wide, Layout::Full), Some(wide));
        assert_eq!(target(wide, Layout::Left), Some(DesktopArea { width: 2560, ..wide }));
        assert_eq!(target(wide, Layout::Right), Some(DesktopArea { x: 2560, width: 2560, ..wide }));
        assert_eq!(target(wide, Layout::Free), None);
        // an odd width leaves no gap between the halves
        let odd = DesktopArea { x: -1921, y: 0, width: 1921, height: 1080 };
        let (left, right) = (target(odd, Layout::Left).unwrap(), target(odd, Layout::Right).unwrap());
        assert_eq!(left.width + right.width, 1921);
        assert_eq!(right.x, left.x + left.width as i32);
        assert_eq!(Layout::from_setting(""), Layout::Full, "older settings files keep the whole screen");
        assert_eq!(Layout::from_setting("free"), Layout::Free);
        let first = first_free(wide);
        assert!(first.width < wide.width && first.x > 0);
    }

    #[test]
    fn full_screen_comes_back_to_the_layout_before_it() {
        use Layout::*;
        assert_eq!(Layout::from_setting("fullscreen"), FullScreen);
        for l in [FullScreen, Full, Left, Right, Free] {
            assert_eq!(Layout::from_setting(l.as_setting()), l);
        }
        // the menu item and Ctrl+Cmd+F go in, remembering the half the member was on
        assert_eq!(resolve("fullscreen", Left, Full), (FullScreen, Left));
        assert_eq!(resolve("toggle-fullscreen", Right, Full), (FullScreen, Right));
        // Ctrl+Cmd+F again or Esc comes back out to it
        assert_eq!(resolve("toggle-fullscreen", FullScreen, Right), (Right, Right));
        assert_eq!(resolve("leave-fullscreen", FullScreen, Free), (Free, Free));
        // Esc outside full screen changes nothing
        assert_eq!(resolve("leave-fullscreen", Left, Full), (Left, Full));
        // choosing full screen again keeps the layout to return to
        assert_eq!(resolve("fullscreen", FullScreen, Left), (FullScreen, Left));
        // an older file with nothing saved comes back to full width
        assert_eq!(resolve("leave-fullscreen", FullScreen, Layout::from_setting("")), (Full, Full));
        assert_eq!(resolve("toggle-fullscreen", FullScreen, FullScreen), (Full, Full));
    }

    #[test]
    fn the_mac_window_menu_offers_every_layout() {
        for (id, _, _) in MENU_LAYOUTS {
            let layout = id.strip_prefix("layout:").unwrap();
            assert_eq!(Layout::from_setting(layout).as_setting(), layout);
        }
        assert!(MENU_LAYOUTS.iter().all(|(_, _, key)| *key != Some("Ctrl+Cmd+F")), "View > Enter Full Screen holds that key");
    }

    #[test]
    fn taskbar_changes_refit_in_both_directions_without_repeating_unchanged_bounds() {
        let shown = DesktopArea { x: 0, y: 0, width: 1920, height: 1040 };
        let hidden = DesktopArea { height: 1080, ..shown };
        assert!(needs_fit(Some(shown), hidden));
        assert!(needs_fit(Some(hidden), shown));
        assert!(!needs_fit(Some(hidden), hidden));
        assert!(needs_fit(None, hidden));
        assert!(!needs_fit(None, DesktopArea { height: 0, ..shown }));
    }

    #[test]
    fn side_taskbars_and_secondary_monitor_origins_are_part_of_the_bounds() {
        let desktop = DesktopArea { x: -1920, y: 0, width: 1920, height: 1080 };
        assert!(needs_fit(Some(desktop), DesktopArea { x: -1880, width: 1880, ..desktop }));
        assert!(needs_fit(Some(desktop), DesktopArea { y: 40, height: 1040, ..desktop }));
        assert!(needs_fit(Some(desktop), DesktopArea { x: 0, ..desktop }));
    }
}
