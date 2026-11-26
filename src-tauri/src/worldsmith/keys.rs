//! F9 and F10 while a game is attached: F9 puts the game's picture into WORLDSMITH's chat; F10, in
//! Translate mode, reads the game's words over it (or takes the words away again). The keys are held
//! only between an attach and a detach (or the room closing), so no other app loses them the rest of
//! the time.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

/// The event the WORLDSMITH page hears with the picture: { path } or { error }.
pub const SHOT_EVENT: &str = "worldsmith-shot";
pub const F9_TAKEN: &str = "Another app holds F9 - use the Screenshot button instead.";
pub const F10_TAKEN: &str = "Another app holds F10 - use the Translate button instead.";

pub fn f9() -> Shortcut {
    Shortcut::new(None, Code::F9)
}

pub fn f10() -> Shortcut {
    Shortcut::new(None, Code::F10)
}

/// Takes one key (once); false when another app holds it.
fn hold(app: &AppHandle, held: &AtomicBool, key: Shortcut, pressed: fn(&AppHandle)) -> bool {
    if held.swap(true, Ordering::SeqCst) {
        return true;
    }
    let taken = app.global_shortcut().on_shortcut(key, move |app, _, event| {
        if event.state() == ShortcutState::Pressed {
            pressed(app);
        }
    });
    if taken.is_err() {
        held.store(false, Ordering::SeqCst);
        return false;
    }
    true
}

/// The notes for the keys another app holds, as one line (None when both were taken). Pure.
pub fn taken_note(f9_free: bool, f10_free: bool) -> Option<String> {
    let notes: Vec<&str> = [(f9_free, F9_TAKEN), (f10_free, F10_TAKEN)].into_iter().filter(|(free, _)| !free).map(|(_, note)| note).collect();
    (!notes.is_empty()).then(|| notes.join(" "))
}

/// The game is attached: F9 and F10 are taken for it (once). Err with one line when another app holds
/// either (the other key still works).
pub fn hold_f9(app: &AppHandle) -> Result<(), String> {
    let serve = app.state::<super::pipe::Serve>();
    let f9_free = hold(app, &serve.f9_held, f9(), on_f9);
    let f10_free = hold(app, &serve.f10_held, f10(), on_f10);
    match taken_note(f9_free, f10_free) {
        Some(note) => Err(note),
        None => Ok(()),
    }
}

/// The game is let go: F9 and F10 go back.
pub fn release_f9(app: &AppHandle) {
    let serve = app.state::<super::pipe::Serve>();
    if serve.f9_held.swap(false, Ordering::SeqCst) {
        let _ = app.global_shortcut().unregister(f9());
    }
    if serve.f10_held.swap(false, Ordering::SeqCst) {
        let _ = app.global_shortcut().unregister(f10());
    }
}

/// F9 pressed: the helper takes the game's picture and the page drops it into the chat.
pub fn on_f9(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let payload = match super::pipe::shot(&app) {
            Ok(path) => serde_json::json!({ "path": path }),
            Err(error) => serde_json::json!({ "error": error }),
        };
        let _ = app.emit_to("main", SHOT_EVENT, payload);
    });
}

/// F10 pressed (Translate mode only): the words over the game go away, or the game's words are read.
pub fn on_f10(app: &AppHandle) {
    super::translate::toggle(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_key_is_named_in_one_line() {
        assert_eq!(taken_note(true, true), None);
        assert_eq!(taken_note(false, true).as_deref(), Some(F9_TAKEN));
        assert_eq!(taken_note(true, false).as_deref(), Some(F10_TAKEN));
        assert_eq!(taken_note(false, false), Some(format!("{F9_TAKEN} {F10_TAKEN}")));
    }
}
