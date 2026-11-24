//! MemoryCore in your pocket: lets a platform (Telegram today, more later) talk to the member's
//! own companion - the real one, with its own memory and tools, not a sandboxed reply. Never the
//! shared Foundation chat room (see roomturn.rs/inboxbridge.rs for that, a different thing).
//! Each adapter only needs to turn its own transport's messages into calls to `send_and_await`.

pub mod discord;
pub mod guests;
pub mod reactions;
pub mod telegram;
pub mod whatsapp;
mod notify;
pub use notify::notify;
pub use notify::target as notification_target;
pub use notify::ready as notification_ready;

use crate::session::{MainHost, Room};
use crate::settings::SettingsStore;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// Types the member's words into the companion's real session (waking it first if it is asleep)
/// and waits for its reply, by following the session's own transcript - the same record the DEV
/// tab's Chat reads, just with this call's own independent follower so the two never race.
pub fn send_and_await(app: &AppHandle, store: &SettingsStore, text: &str, timeout: Duration) -> Result<String, String> {
    let host = &app.state::<MainHost>().0;
    let fresh = !host.is_running();
    if fresh {
        // A session that is not running yet can take the first message as its own launch
        // argument - no PTY write race against a CLI that has not started reading stdin yet.
        crate::session::wake_with_prompt(app, store, host, Room::Main, 120, 40, Some(text))?;
    }
    let (_, started) = host.started().ok_or("Your companion's session did not start.")?;
    let settings = store.get();
    let folder = Room::Main.folder(&settings).ok_or("No MemoryCore folder is chosen yet.")?;
    let cli = host.provider().unwrap_or(settings.cli);
    // A session that just woke takes a moment to write its first record; a running one already
    // has one. Either way, wait rather than fail the whole message over a one-second gap.
    let found_at = Instant::now() + Duration::from_secs(15).min(timeout);
    let (path, follow) = loop {
        if let Some(found) = crate::chat::newest_record(cli, &folder, started) {
            break found;
        }
        if Instant::now() >= found_at {
            return Err("Your companion's session has no record yet. Try again in a moment.".into());
        }
        std::thread::sleep(Duration::from_millis(300));
    };
    let mut follower = follow(path.clone(), started);
    // The reply is finished when the record gets its end-of-turn line after this point.
    let mut turn_from: u64 = 0;
    if !fresh {
        turn_from = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        // Already running: anything already said is not this message's reply, so it is skipped
        // before the word is typed in - never after, or a fast answer could be skipped too.
        let _ = follower.read_new();
        host.write(text.as_bytes())?;
        std::thread::sleep(Duration::from_millis(150));
        host.write(b"\r")?;
    }
    let deadline = Instant::now() + timeout;
    // A companion often says something first ("let me check...") and works before the real answer,
    // so the first words are not the reply. It is taken as done once it has been quiet for a while;
    // the answer is the last thing it said.
    let mut last_said: Option<String> = None;
    let mut quiet_since = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(entries) = follower.read_new() {
            if !entries.is_empty() {
                quiet_since = Instant::now();
            }
            if let Some(said) = entries.iter().rev().find(|e| e.role == "companion" && !e.text.trim().is_empty()) {
                last_said = Some(said.text.trim().to_string());
            }
        }
        if last_said.is_some() {
            let finished = turn_finished(&path, turn_from, cli);
            if finished || quiet_since.elapsed() >= QUIET_AFTER {
                if finished {
                    // anything written between the last read and the end-of-turn line
                    if let Ok(entries) = follower.read_new() {
                        if let Some(said) = entries.iter().rev().find(|e| e.role == "companion" && !e.text.trim().is_empty()) {
                            last_said = Some(said.text.trim().to_string());
                        }
                    }
                }
                return Ok(last_said.unwrap_or_default());
            }
        }
        if Instant::now() >= deadline {
            return match last_said {
                Some(said) => Ok(said),
                None => Err("Your companion took too long to answer.".into()),
            };
        }
    }
}

/// True once the session's record carries its end-of-turn line after `from` (a byte offset taken
/// before the member's words were typed): Codex writes `task_complete`, Claude Code a system line
/// of subtype `turn_duration`. Lets the answer go out the moment it is complete.
fn turn_finished(path: &std::path::Path, from: u64, cli: crate::settings::Cli) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    if file.seek(SeekFrom::Start(from)).is_err() {
        return false;
    }
    let mut tail = Vec::new();
    if file.take(16 * 1024 * 1024).read_to_end(&mut tail).is_err() {
        return false;
    }
    let text = String::from_utf8_lossy(&tail);
    match cli {
        crate::settings::Cli::Codex => text.contains("\"type\":\"task_complete\""),
        crate::settings::Cli::Claude => text.contains("\"subtype\":\"turn_duration\""),
        // their turn ends are read with their records (Phase 6)
        crate::settings::Cli::Gemini | crate::settings::Cli::Grok => false,
    }
}

/// A picture the member sent from the phone, for the companion: checked by its own bytes, moved
/// into the memory folder's `inbox/pictures/` by its fingerprint, and the words the companion
/// reads ("(sent a picture: <path>) <caption>"). The file the bridge saved is removed either way.
/// None when it is not a picture Castle takes (the companion then hears only the caption line).
pub fn take_picture(memory: &std::path::Path, saved: &std::path::Path, caption: &str) -> Option<String> {
    let picture = crate::finance::companion::Picture::open(saved);
    let _ = std::fs::remove_file(saved);
    let picture = picture.ok()?;
    let inbox = memory.join("inbox").join("pictures");
    let file = inbox.join(format!("{}.{}", &picture.sha256[..8], picture.ext));
    if !file.is_file() {
        std::fs::create_dir_all(&inbox).ok()?;
        std::fs::write(&file, &picture.bytes).ok()?;
    }
    let caption = caption.trim();
    Some(format!("(sent a picture: {}){}", file.display(), if caption.is_empty() { String::new() } else { format!(" {caption}") }))
}

/// How long the companion must stay silent before what it last said counts as its answer.
const QUIET_AFTER: Duration = Duration::from_secs(12);

#[cfg(test)]
mod picture_tests {
    use super::take_picture;

    #[test]
    fn a_sent_picture_moves_into_the_memory_folder_and_is_named_to_the_companion() {
        let root = std::env::temp_dir().join(format!("castle-phone-picture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (memory, saved) = (root.join("memory"), root.join("media").join("ABC.img"));
        std::fs::create_dir_all(saved.parent().unwrap()).unwrap();
        std::fs::write(&saved, [0xFF, 0xD8, 0xFF, 0xE0, 7]).unwrap();
        let words = take_picture(&memory, &saved, " lunch receipt ").unwrap();
        assert!(words.starts_with("(sent a picture: ") && words.ends_with(") lunch receipt"), "{words}");
        assert!(words.contains(&memory.join("inbox").join("pictures").display().to_string()));
        assert!(!saved.exists(), "the bridge's own file is removed");
        let pdf = root.join("media").join("D.img");
        std::fs::write(&pdf, b"%PDF").unwrap();
        assert_eq!(take_picture(&memory, &pdf, ""), None);
        assert!(!pdf.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Cli;
    use std::io::Write;

    #[test]
    fn a_reply_is_finished_only_by_a_turn_end_written_after_the_words_were_typed() {
        let path = std::env::temp_dir().join(format!("castle-turn-{}.jsonl", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        // an earlier turn already ended
        writeln!(file, r#"{{"type":"event_msg","payload":{{"type":"task_complete"}}}}"#).unwrap();
        file.flush().unwrap();
        let from = std::fs::metadata(&path).unwrap().len();
        assert!(!turn_finished(&path, from, Cli::Codex), "the earlier turn's end does not count");
        writeln!(file, r#"{{"type":"event_msg","payload":{{"type":"agent_message","message":"hi"}}}}"#).unwrap();
        file.flush().unwrap();
        assert!(!turn_finished(&path, from, Cli::Codex), "still working");
        writeln!(file, r#"{{"type":"event_msg","payload":{{"type":"task_complete"}}}}"#).unwrap();
        file.flush().unwrap();
        assert!(turn_finished(&path, from, Cli::Codex));
        assert!(!turn_finished(&path, from, Cli::Claude), "Claude Code ends a turn differently");
        writeln!(file, r#"{{"type":"system","subtype":"turn_duration","durationMs":5}}"#).unwrap();
        file.flush().unwrap();
        assert!(turn_finished(&path, from, Cli::Claude));
        assert!(!turn_finished(&std::env::temp_dir().join("castle-no-such-file.jsonl"), 0, Cli::Codex));
        let _ = std::fs::remove_file(path);
    }
}
