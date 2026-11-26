//! Translate's overlay: a see-through, click-through window laid exactly over the game, where the page
//! draws the translated words (`worldsmith-lines`). It never takes the focus or a click, so the game
//! plays on under it. Built hidden on demand (before a read, so it is listening by the time the words
//! come), shown over the game's place, hidden when the picture moves, gone with the room.

use super::pipe::Rect;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The overlay window's label (the page's view of the same name).
pub const LABEL: &str = "worldsmith-overlay";
/// The event the page and the overlay hear: a Translation, { hidden: true } or { error }.
pub const EVENT: &str = "worldsmith-lines";

/// Tells both the room's page and the overlay.
pub fn emit(app: &AppHandle, payload: &serde_json::Value) {
    let _ = app.emit_to("main", EVENT, payload);
    let _ = app.emit_to(LABEL, EVENT, payload);
}

/// The overlay, built hidden when it is not there yet. Never on the main thread from a sync command
/// (building a window there waits on itself on Windows).
pub fn ensure(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()));
    // see-through needs the private API on macOS; there the words sit on a dark sheet instead
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true);
    let window = builder
        .title("MemoryCore Castle - Translate")
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .initialization_script(format!("window.__CASTLE_VIEW__ = {{ view: 'worldsmith-overlay' }};{}", crate::world::NO_START_SCREEN))
        .build()
        .map_err(|_| "The translation could not be laid over the game.".to_string())?;
    let _ = window.set_ignore_cursor_events(true);
    Ok(window)
}

/// The overlay laid over the game's window (physical pixels) and shown, still click-through.
pub fn place(app: &AppHandle, rect: Rect) -> Result<(), String> {
    let window = ensure(app)?;
    let failed = |_| "The translation could not be laid over the game.".to_string();
    window.set_position(PhysicalPosition::new(rect.x, rect.y)).map_err(failed)?;
    window.set_size(PhysicalSize::new(rect.w, rect.h)).map_err(failed)?;
    window.show().map_err(failed)?;
    let _ = window.set_ignore_cursor_events(true);
    app.state::<super::translate::Translate>().shown.store(true, Ordering::SeqCst);
    Ok(())
}

/// The words taken away from over the game (the window stays, hidden, for the next read).
pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    app.state::<super::translate::Translate>().shown.store(false, Ordering::SeqCst);
    emit(app, &serde_json::json!({ "hidden": true }));
}

/// The room closed or Castle ends: the overlay is gone.
pub fn stop(app: &AppHandle) {
    let state = app.state::<super::translate::Translate>();
    let was = state.shown.swap(false, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
    if was {
        let _ = app.emit_to("main", EVENT, serde_json::json!({ "hidden": true }));
    }
}
