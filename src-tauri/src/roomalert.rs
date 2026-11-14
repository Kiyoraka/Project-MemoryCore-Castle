//! A new line in the Foundation room: a sound and a desktop notification when the member is not
//! looking at the CHAT tab. The notification is on by default and the sound off (the sound ignores
//! Windows quiet hours); each has its own switch in Settings > Rooms.
//! The room reader (roomread.rs) runs from the moment Castle starts, also while Castle is hidden in
//! the tray, so this reaches the member then too. An alert is best effort: it never stops a read.

use crate::settings::SettingsStore;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

const PREVIEW: usize = 80;

/// The notification for a batch of new lines, leaving out lines the member posted from Castle
/// (`mine`, by id): (title, body), or None when nothing is left to tell. Pure, tested.
pub fn summary(fresh: &[Value], mine: &[u64]) -> Option<(String, String)> {
    let others: Vec<&Value> = fresh.iter().filter(|l| l.get("id").and_then(Value::as_u64).is_none_or(|id| !mine.contains(&id))).collect();
    let last = others.last()?;
    let who = last.pointer("/author/name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Someone");
    let said = last.get("text").and_then(Value::as_str).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    let said = if said.is_empty() { "sent a picture or a voice line".to_string() } else { preview(&said) };
    let body = if others.len() > 1 { format!("{} new messages. Latest, {who}: {said}", others.len()) } else { format!("{who}: {said}") };
    Some(("New in the room".to_string(), body))
}

fn preview(text: &str) -> String {
    let mut short: String = text.chars().take(PREVIEW).collect();
    if text.chars().count() > PREVIEW {
        short.push('…');
    }
    short
}

/// Windows' own notification sound, so it follows the member's sound scheme and volume.
#[cfg(windows)]
fn beep() -> bool {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
    let name: Vec<u16> = "SystemNotification\0".encode_utf16().collect();
    unsafe {
        PlaySoundW(name.as_ptr(), std::ptr::null_mut(), SND_ALIAS | SND_ASYNC | SND_NODEFAULT) != 0
    }
}

#[cfg(not(windows))]
fn beep() -> bool {
    false
}

/// The member is looking at the room when CHAT is the tab on screen in a visible, focused Castle.
fn looking(app: &AppHandle, chat_shown: bool) -> bool {
    chat_shown && app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
}

/// Called with the lines a read just found (never the first read after start or sign-in).
pub fn new_lines(app: &AppHandle, fresh: &[Value], chat_shown: bool) {
    let store = app.state::<SettingsStore>();
    let settings = store.get();
    if !(settings.alert_sound || settings.alert_notify) {
        return;
    }
    // the member's own companion speaking is not news to tell them about
    let fresh: Vec<Value> = fresh.iter().filter(|l| !crate::roomturn::companion_said(&settings, l)).cloned().collect();
    let Some((title, body)) = summary(&fresh, &crate::chatroom::my_line_ids(&store)) else { return };
    if looking(app, chat_shown) {
        return;
    }
    if settings.alert_sound {
        beep();
    }
    if settings.alert_notify {
        // No `.sound(..)`: with tauri-plugin-notification 2.8 / notify-rust 4.19 a Windows toast built
        // without one is `<audio silent="true"/>`, so the Sound switch is the only sound (checked in
        // the dependency source, not by ear).
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(id: u64, who: &str, text: &str) -> Value {
        json!({ "id": id, "text": text, "author": { "name": who, "kind": "member" } })
    }

    /// Plays the sound once, out loud: `cargo test plays_the_windows -- --ignored`.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn plays_the_windows_notification_sound() {
        assert!(beep(), "Windows refused to play its notification sound");
    }

    #[test]
    fn one_line_names_who_said_it() {
        let (title, body) = summary(&[line(5, "Kiyo", "Boleh   tengok\nPR tu")], &[]).unwrap();
        assert_eq!(title, "New in the room");
        assert_eq!(body, "Kiyo: Boleh tengok PR tu");
    }

    #[test]
    fn several_lines_are_counted_and_the_latest_is_shown() {
        let (_, body) = summary(&[line(1, "Kiyo", "a"), line(2, "Alice", "b")], &[]).unwrap();
        assert_eq!(body, "2 new messages. Latest, Alice: b");
    }

    #[test]
    fn the_members_own_lines_never_alert() {
        assert!(summary(&[line(7, "Bernard", "hi")], &[7]).is_none());
        let (_, body) = summary(&[line(7, "Bernard", "hi"), line(8, "Kiyo", "hey")], &[7]).unwrap();
        assert_eq!(body, "Kiyo: hey");
        assert!(summary(&[], &[]).is_none());
    }

    #[test]
    fn long_and_empty_lines_are_handled() {
        let long = "x".repeat(200);
        let (_, body) = summary(&[line(1, "Kiyo", &long)], &[]).unwrap();
        assert_eq!(body.chars().count(), "Kiyo: ".len() + PREVIEW + 1);
        assert!(body.ends_with('…'));
        let (_, body) = summary(&[json!({ "id": 2, "text": "", "author": { "name": "  " } })], &[]).unwrap();
        assert_eq!(body, "Someone: sent a picture or a voice line");
    }
}
