//! F9 while a game is attached: the game's picture goes into WORLDSMITH's chat. The key is held only
//! between an attach and a detach (or the room closing), so no other app loses F9 the rest of the time.

use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

/// The event the WORLDSMITH page hears with the picture: { path } or { error }.
pub const SHOT_EVENT: &str = "worldsmith-shot";
pub const F9_TAKEN: &str = "Another app holds F9 - use the Screenshot button instead.";

pub fn f9() -> Shortcut {
    Shortcut::new(None, Code::F9)
}

/// The game is attached: F9 is taken for it (once). Err with one line when another app holds F9.
pub fn hold_f9(app: &AppHandle) -> Result<(), String> {
    let serve = app.state::<super::pipe::Serve>();
    if serve.f9_held.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let taken = app.global_shortcut().on_shortcut(f9(), |app, _, event| {
        if event.state() == ShortcutState::Pressed {
            on_f9(app);
        }
    });
    if taken.is_err() {
        serve.f9_held.store(false, Ordering::SeqCst);
        return Err(F9_TAKEN.into());
    }
    Ok(())
}

/// The game is let go: F9 goes back.
pub fn release_f9(app: &AppHandle) {
    let serve = app.state::<super::pipe::Serve>();
    if serve.f9_held.swap(false, Ordering::SeqCst) {
        let _ = app.global_shortcut().unregister(f9());
    }
}

/// F9 pressed: the helper takes the game's picture and the page drops it into the chat.
pub fn on_f9(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let payload = match super::pipe::shot() {
            Ok(path) => serde_json::json!({ "path": path }),
            Err(error) => serde_json::json!({ "error": error }),
        };
        let _ = app.emit_to("main", SHOT_EVENT, payload);
    });
}
