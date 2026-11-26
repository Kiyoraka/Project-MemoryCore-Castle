//! Messages accepted during a provider switch or image job, separate from either CLI queue.
use crate::{
    providers::{Control, Images},
    session::{MainHost, ScriptoriumHost, StoryHost, WorldsmithHost},
    settings::SettingsStore,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Manager, State};
#[derive(Default)]
pub struct Pending(pub Mutex<Vec<Message>>);
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    id: String,
    text: String,
    memory: PathBuf,
    pub state: String,
}
#[derive(Serialize)]
pub struct Sent {
    queued: bool,
}
fn memory(store: &SettingsStore) -> Result<PathBuf, String> {
    PathBuf::from(
        store
            .get()
            .memory_folder
            .ok_or("Choose a MemoryCore folder first.")?,
    )
    .canonicalize()
    .map_err(|_| "The memory folder is gone.".into())
}
fn save(store: &SettingsStore, messages: &[Message]) -> Result<(), String> {
    let path = store.folder().join("pending-messages.json");
    let temp = path.with_extension("pending");
    std::fs::write(
        &temp,
        serde_json::to_vec(messages).map_err(|_| "Message queue could not be encoded.")?,
    )
    .map_err(|_| "Message queue could not be saved.")?;
    std::fs::rename(temp, path).map_err(|_| "Message queue could not be published.".into())
}
/// The text as one bracketed paste, so a new line never sends the message early.
fn paste(text: &str) -> String {
    format!("\u{1b}[200~{}\u{1b}[201~", text)
}

/// Codex reads an Enter that arrives together with text as part of a paste and leaves the text in
/// its box unsent, so Enter follows after a short pause (Codex needs more than 50 ms; this leaves
/// margin for a busy PC). The wait is on its own thread: commands must not hold the window.
const ENTER_DELAY: std::time::Duration = std::time::Duration::from_millis(300);
/// Claude Code turns each picture path in a paste into `[Image #N]`, which takes a moment; an Enter
/// that arrives meanwhile is swallowed and the words stay in its box, so each picture path adds this.
const PICTURE_DELAY: std::time::Duration = std::time::Duration::from_millis(700);

/// A picture path as typed into the box: a drive letter or a leading `/` or `\`, ending in a picture
/// extension (any case), in quotes when it holds spaces. It begins the text or follows a space, or
/// the `~` that opens a bracketed paste.
fn picture_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)(?:^|[\s~])(?:"(?:[a-z]:|[/\\])[^"]*\.(?:png|jpe?g|gif|webp)"|(?:[a-z]:|[/\\])[^\s"]*\.(?:png|jpe?g|gif|webp)\b)"#,
        )
        .expect("a fixed pattern")
    })
}

/// The pause between the text and its Enter: 300 ms, and 700 ms more for each picture path in it
/// (a bracketed paste counts the paths inside it). Pure.
pub fn enter_delay(text: &str) -> std::time::Duration {
    ENTER_DELAY + PICTURE_DELAY * picture_pattern().find_iter(text).count() as u32
}

/// The delayed Enter goes only to the session the text went to: if that session ended or was
/// replaced in the pause (a switch, a restart), a stray Enter could answer the new one's startup
/// question, so it is dropped.
fn enter_after_pause(delay: std::time::Duration, session: Option<u64>, now: impl Fn() -> Option<u64>, press: impl FnOnce()) {
    std::thread::sleep(delay);
    if session.is_some() && now() == session {
        press();
    }
}

fn send_main(app: &AppHandle, text: &str) -> Result<(), String> {
    let host = &app.state::<MainHost>().0;
    let session = host.started().map(|(id, _)| id);
    host.write(paste(text).as_bytes())?;
    let delay = enter_delay(text);
    let app = app.clone();
    std::thread::spawn(move || {
        let host = &app.state::<MainHost>().0;
        enter_after_pause(delay, session, || host.started().map(|(id, _)| id), || {
            let _ = host.write(b"\r");
        });
    });
    Ok(())
}

fn send_story(app: &AppHandle, text: &str) -> Result<(), String> {
    send_room::<StoryHost>(app, text, |h| &h.0)
}

/// A room of its own (STORY, WORLDSMITH, SCRIPTORIUM): the words pasted whole, Enter after the pictures land.
fn send_room<H: Send + Sync + 'static>(app: &AppHandle, text: &str, pty: fn(&H) -> &crate::pty::PtyHost) -> Result<(), String> {
    let state = app.state::<H>();
    let host = pty(&state);
    let session = host.started().map(|(id, _)| id);
    host.write(paste(text).as_bytes())?;
    let delay = enter_delay(text);
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<H>();
        let host = pty(&state);
        enter_after_pause(delay, session, || host.started().map(|(id, _)| id), || {
            let _ = host.write(b"\r");
        });
    });
    Ok(())
}

#[tauri::command]
pub fn send_message(text: String, room: Option<String>, app: AppHandle) -> Result<Sent, String> {
    if text.trim().is_empty() || text.len() > 128_000 {
        return Err("Use a nonempty message shorter than 128 KB.".into());
    }
    match crate::session::Room::known(room.as_deref()) {
        Some(crate::session::Room::Story) => {
            send_story(&app, &text)?;
            return Ok(Sent { queued: false });
        }
        Some(crate::session::Room::Worldsmith) => {
            send_room::<WorldsmithHost>(&app, &text, |h| &h.0)?;
            return Ok(Sent { queued: false });
        }
        Some(crate::session::Room::Scriptorium) => {
            send_room::<ScriptoriumHost>(&app, &text, |h| &h.0)?;
            return Ok(Sent { queued: false });
        }
        Some(crate::session::Room::Music) => {
            send_room::<crate::session::MusicHost>(&app, &text, |h| &h.0)?;
            return Ok(Sent { queued: false });
        }
        Some(crate::session::Room::Main) => {}
        None => return Err("Castle does not know that room. Your message has not been sent.".into()),
    }
    let store = app.state::<SettingsStore>();
    let control = app.state::<Control>();
    let guard = control.0.try_lock().ok();
    let blocked = guard
        .as_ref()
        .is_none_or(|s| s.target.is_some() || s.phase == "switching")
        || app
            .state::<Images>()
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|j| j.holds_session());
    let pending = app.state::<Pending>();
    let mut messages = pending.0.lock().unwrap();
    let selected = memory(&store)?;
    if blocked
        || messages
            .iter()
            .any(|m| m.memory == selected && m.state == "queued")
    {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_string();
        messages.push(Message {
            id,
            text,
            memory: selected,
            state: "queued".into(),
        });
        if let Err(e) = save(&store, &messages) {
            messages.pop();
            return Err(e);
        }
        return Ok(Sent { queued: true });
    }
    send_main(&app, &text)?;
    Ok(Sent { queued: false })
}

#[tauri::command]
pub fn queued_messages(store: State<SettingsStore>, pending: State<Pending>) -> Vec<Message> {
    let Ok(selected) = memory(&store) else {
        return Vec::new();
    };
    pending
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.memory == selected)
        .cloned()
        .collect()
}

#[tauri::command]
pub fn queued_message_action(
    id: String,
    retry: bool,
    store: State<SettingsStore>,
    pending: State<Pending>,
) -> Result<(), String> {
    let selected = memory(&store)?;
    let mut messages = pending.0.lock().unwrap();
    let at = messages
        .iter()
        .position(|m| m.id == id && m.memory == selected)
        .ok_or("Message not found.")?;
    if retry {
        messages[at].state = "queued".into();
    } else {
        messages.remove(at);
    }
    save(&store, &messages)
}

pub fn start(app: &AppHandle) {
    let store = app.state::<SettingsStore>();
    let mut saved: Vec<Message> = std::fs::read(store.folder().join("pending-messages.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    // A crash may have occurred after sending but before removing the queue entry. Never replay blindly.
    for m in &mut saved {
        m.state = "held-after-restart".into();
    }
    *app.state::<Pending>().0.lock().unwrap() = saved;
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let control = app.state::<Control>();
        let Ok(guard) = control.0.try_lock() else {
            continue;
        };
        if guard.target.is_some()
            || guard.phase == "switching"
            || app
                .state::<Images>()
                .0
                .lock()
                .unwrap()
                .iter()
                .any(|j| j.holds_session())
        {
            continue;
        }
        let store = app.state::<SettingsStore>();
        let main = app.state::<MainHost>();
        if !main.0.is_running() || !crate::providers::safe_boundary(&store, &main.0) {
            continue;
        }
        let Ok(selected) = memory(&store) else {
            continue;
        };
        let pending = app.state::<Pending>();
        let mut messages = pending.0.lock().unwrap();
        let Some(at) = messages
            .iter()
            .position(|m| m.memory == selected && m.state == "queued")
        else {
            continue;
        };
        if send_main(&app, &messages[at].text).is_ok() {
            // Retain as held if queue publication fails; do not resend on a subsequent tick.
            messages[at].state = "sent".into();
            let sent = messages.remove(at);
            if save(&store, &messages).is_err() {
                let mut held = sent;
                held.state = "held-check-chat".into();
                messages.insert(at, held);
            }
        }
    });
}

/// One border of Claude Code's input box: a line made mostly of `─`, rounded corners allowed.
fn is_rule(line: &str) -> bool {
    let marks = line.chars().filter(|c| !c.is_whitespace()).count();
    let dashes = line.chars().filter(|&c| c == '─').count();
    dashes >= 8 && dashes * 10 >= marks * 8
}

/// The words with every space and line break taken out, so a line the box wrapped still matches.
fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// How much of the start of the sent words must still sit in the box.
const HOLD_PREFIX: usize = 40;

/// Whether Claude Code's input box still holds the sent words, read from the plain screen: the lines
/// between its last two rules (the box's borders), without side borders, the prompt glyph or spaces,
/// must contain the start of the text. Picture paths are left out of the text and `[Image #N]` out of
/// the box, since Claude Code shows one for each picture it took in; a long paste it shows as
/// `[Pasted text #N ...]` counts as held too. Words echoed above the box never count. False when the
/// screen has no such box (another provider) or the box is empty. Pure.
pub fn prompt_holds(screen_plain: &str, text: &str) -> bool {
    let lines: Vec<&str> = screen_plain.lines().collect();
    let mut rules = lines.iter().enumerate().rev().filter(|(_, line)| is_rule(line)).map(|(i, _)| i);
    let (Some(bottom), Some(top)) = (rules.next(), rules.next()) else {
        return false;
    };
    let inside = lines[top + 1..bottom].iter().map(|line| line.replace('│', "")).collect::<Vec<_>>().join(" ");
    let pictures = inside.contains("[Image #");
    let without_pictures = inside.split("[Image #").enumerate().map(|(i, part)| if i == 0 { part } else { part.split_once(']').map_or("", |(_, rest)| rest) }).collect::<String>();
    let boxed = squeeze(&without_pictures);
    let boxed = boxed.strip_prefix('>').or_else(|| boxed.strip_prefix('❯')).unwrap_or(&boxed);
    let said = squeeze(&picture_pattern().replace_all(text, " "));
    if said.is_empty() {
        return pictures;
    }
    if boxed.is_empty() {
        return false;
    }
    let long = text.contains('\n') || text.chars().count() > 200;
    let head: String = said.chars().take(HOLD_PREFIX).collect();
    boxed.contains(&head) || (long && boxed.contains("[Pastedtext#"))
}

/// Whether the room's Claude Code session still holds the sent words in its input box. `room`: None
/// (or "main") for the home chat, "story" for the STORY room, `dev:<slot>` for a DEV tab. False for
/// any session that is not a running Claude Code. It only reads the screen.
#[tauri::command]
pub fn chat_prompt_holds(room: Option<String>, text: String, app: AppHandle) -> bool {
    use crate::unstick::claude_screen;
    let screen = match room.as_deref() {
        None | Some("") | Some("main") => claude_screen(&app.state::<MainHost>().0),
        Some("story") => claude_screen(&app.state::<StoryHost>().0),
        Some("worldsmith") => claude_screen(&app.state::<WorldsmithHost>().0),
        Some("scriptorium") => claude_screen(&app.state::<ScriptoriumHost>().0),
        Some("music") => claude_screen(&app.state::<crate::session::MusicHost>().0),
        Some(other) => other
            .strip_prefix("dev:")
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|slot| app.state::<crate::dev::DevHosts>().0.get(slot).and_then(claude_screen)),
    };
    screen.is_some_and(|screen| prompt_holds(&screen, &text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_enter_waits_longer_for_each_picture_path() {
        assert_eq!(enter_delay("save diary"), Duration::from_millis(300));
        assert_eq!(enter_delay("see 3.png and the docs/a.png page"), Duration::from_millis(300), "not paths");
        let two = r#"look C:\shots\one.PNG and "D:\My Pictures\two.jpeg" please"#;
        assert_eq!(enter_delay(two), Duration::from_millis(1700));
        assert_eq!(enter_delay(&paste("/home/me/a.webp\nwhat is it")), Duration::from_millis(1000), "inside a paste");
        assert_eq!(enter_delay(r"\\server\share\b.gif"), Duration::from_millis(1000));
    }

    const RULE: &str = "────────────────────────────────────────";

    fn screen(prompt: &[&str]) -> String {
        let mut lines = vec!["> fix the login page", "● Done, the login page works.", "", RULE];
        lines.extend_from_slice(prompt);
        lines.extend_from_slice(&[RULE, "  ? for shortcuts"]);
        lines.join("\n")
    }

    #[test]
    fn the_words_still_in_the_box_are_held() {
        assert!(prompt_holds(&screen(&["> please fix the login page again"]), "please fix the login page again"));
        // a long line the box wrapped, and the rounded box with side borders
        let long = "please look at the upload form and tell me why the button does not answer when clicked";
        assert!(prompt_holds(&screen(&["❯ please look at the upload form and tell me", "  why the button does not answer when clicked"]), long));
        let rounded = format!("╭{RULE}╮\n│ > please fix the login page again │\n╰{RULE}╯\n  ? for shortcuts");
        assert!(prompt_holds(&rounded, "please fix the login page again"));
    }

    #[test]
    fn an_empty_box_or_words_only_above_it_are_not_held() {
        assert!(!prompt_holds(&screen(&["> "]), "fix the login page"), "sent: the words are echoed above, the box is empty");
        assert!(!prompt_holds(&screen(&["> something else entirely"]), "fix the login page"));
    }

    #[test]
    fn a_screen_without_the_box_is_never_a_false_alarm() {
        assert!(!prompt_holds("› fix the login page\n  working...", "fix the login page"));
        assert!(!prompt_holds("", "fix the login page"));
    }

    #[test]
    fn pictures_show_as_image_placeholders() {
        let text = r"C:\shots\one.png C:\shots\two.png what is in these";
        assert!(prompt_holds(&screen(&["> [Image #1] [Image #2] what is in these"]), text));
        assert!(!prompt_holds(&screen(&["> "]), text));
        assert!(prompt_holds(&screen(&["> [Image #1] "]), r"C:\shots\one.png"), "a picture alone");
        assert!(!prompt_holds(&screen(&["> "]), r"C:\shots\one.png"));
        let long = "first line\nsecond line\nthird line";
        assert!(prompt_holds(&screen(&["> [Pasted text #1 +3 lines]"]), long));
    }

    #[test]
    fn sends_multiline_text_as_one_bracketed_paste() {
        // Enter is not part of it: it follows after ENTER_DELAY, because Codex ignores an Enter
        // that arrives with the text.
        assert_eq!(paste("one\ntwo"), "\u{1b}[200~one\ntwo\u{1b}[201~");
        assert!(ENTER_DELAY >= std::time::Duration::from_millis(100));
    }

    #[test]
    fn the_delayed_enter_reaches_only_the_session_the_text_went_to() {
        use std::cell::Cell;
        let pressed = Cell::new(0);
        enter_after_pause(Duration::ZERO, Some(7), || Some(7), || pressed.set(pressed.get() + 1));
        assert_eq!(pressed.get(), 1, "same session: Enter is pressed");
        enter_after_pause(Duration::ZERO, Some(7), || Some(8), || pressed.set(pressed.get() + 1));
        assert_eq!(pressed.get(), 1, "replaced in the pause: no stray Enter");
        enter_after_pause(Duration::ZERO, Some(7), || None, || pressed.set(pressed.get() + 1));
        enter_after_pause(Duration::ZERO, None, || None, || pressed.set(pressed.get() + 1));
        assert_eq!(pressed.get(), 1, "ended, or never running: nothing is pressed");
    }
}
