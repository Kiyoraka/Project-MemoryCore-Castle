//! Chat: each session's own record, read only. The main session and the STORY room each have
//! their own reader, so the two Chats never mix.

use crate::files;
use crate::session::{MainHost, Room, StoryHost, WorldsmithHost, STORY_FOLDER, WORLDSMITH_FOLDER};
use crate::settings::{Cli, SettingsStore};
use crate::transcript::{self, claude, ChatEntry, Follower};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::State;

/// The record a Chat follows, and the session it belongs to.
#[derive(Default)]
pub struct ChatState(Mutex<Option<(u64, Follower)>>);

impl ChatState {
    /// Forgets the record it was following (a DEV tab closed): the next session starts a fresh Chat.
    pub fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }

    /// Whether this Chat's reader showed `path` (a picture or voice of its session, or a picture the
    /// member shared in a message it read).
    pub fn was_shown(&self, path: &str) -> bool {
        self.0.lock().unwrap().as_ref().is_some_and(|(_, follower)| follower.was_shown(path))
    }
}
#[derive(Default)]
pub struct MainChat(pub ChatState);
#[derive(Default)]
pub struct StoryChat(pub ChatState);
#[derive(Default)]
pub struct WorldsmithChat(pub ChatState);

/// Pictures the member dropped or pasted into Chat this run: the screen may show these as
/// thumbnails before they are sent (once sent, the record reader allows them as the member's).
#[derive(Default)]
pub struct MemberPictures(pub Mutex<HashSet<String>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPoll {
    entries: Vec<ChatEntry>,
    /// True when Chat started reading a new record: the screen clears and shows these.
    reset: bool,
    /// "asleep", "waiting" (no record yet), "ok" or "unreadable"
    status: &'static str,
}

fn pick<'a>(room: Room, main: &'a MainChat, story: &'a StoryChat, smith: &'a WorldsmithChat) -> &'a ChatState {
    match room {
        Room::Main => &main.0,
        Room::Story => &story.0,
        Room::Worldsmith => &smith.0,
    }
}

/// The newest record of `folder`'s session written since `started`, and how to follow it.
pub(crate) fn newest_record(cli: Cli, folder: &Path, started: SystemTime) -> Option<(PathBuf, fn(PathBuf, SystemTime) -> Follower)> {
    match cli {
        Cli::Claude => {
            let records = claude::projects_root()?.join(claude::project_folder_name(&folder.display().to_string()));
            claude::newest_record(&records, started).map(|path| (path, claude::follower as fn(PathBuf, SystemTime) -> Follower))
        }
        Cli::Codex => transcript::codex::newest_record(folder, started).map(|path| (path, transcript::codex::follower as fn(PathBuf, SystemTime) -> Follower)),
        // their readers come with Phase 6 (transcript/grok.rs, transcript/agy.rs): until then
        // Chat stays empty and the Log shows the talk
        Cli::Gemini | Cli::Grok => None,
    }
}

/// Claude Code's /clear goes on in a new record: moves Castle's pin there when this session's
/// own turn started it (Codex's Chat already follows the newest rollout).
fn follow_a_clear(cli: Cli, folder: &Path, host: &crate::pty::PtyHost) {
    match cli {
        Cli::Claude => {
            if let Some(records) = claude::projects_root().map(|root| root.join(claude::project_folder_name(&folder.display().to_string()))) {
                claude::follow_clear(&records, &host.recent_enters());
            }
        }
        Cli::Codex | Cli::Gemini | Cli::Grok => {}
    }
}

/// Whether a record Chat now follows is the main session's own to pick up again. Claude's is
/// pinned (it only moves after Castle's own turn); a Codex rollout may be a conversation the
/// member began in a terminal of their own, so it counts only when born right after a turn
/// Castle sent (a /new). A fresh Codex session is remembered when it appears (resume.rs).
fn started_by_castle(cli: Cli, record: &Path, enters: &[SystemTime]) -> bool {
    if cli.pins_id() {
        true
    } else {
        record.metadata().and_then(|m| m.created()).is_ok_and(|born| claude::born_after_a_turn(born, enters))
    }
}

/// A DEV tab's Chat: the newest record of that project's session, followed like the main room's
/// (a fresh record after /clear is picked up), without the main room's journal.
pub fn follow_folder(chat: &ChatState, host: &crate::pty::PtyHost, cli: Cli, folder: &Path) -> ChatPoll {
    let empty = |status| ChatPoll { entries: Vec::new(), reset: false, status };
    let Some((session, started)) = host.started() else {
        return empty("asleep");
    };
    let cli = host.provider().unwrap_or(cli);
    follow_a_clear(cli, folder, host);
    let Some((newest, follow)) = newest_record(cli, folder, started) else {
        return empty("waiting");
    };
    let mut slot = chat.0.lock().unwrap();
    let mut reset = false;
    if slot.as_ref().map(|(id, f)| (*id, f.path.clone())) != Some((session, newest.clone())) {
        *slot = Some((session, follow(newest, started)));
        reset = true;
    }
    let (_, follower) = slot.as_mut().expect("set above");
    match follower.read_new() {
        Ok(mut entries) => {
            entries.retain(|e| !crate::providers::castle_wrote(e));
            if reset && entries.len() > transcript::FIRST_READ_LIMIT {
                entries = entries.split_off(entries.len() - transcript::FIRST_READ_LIMIT);
            }
            ChatPoll { entries, reset, status: "ok" }
        }
        Err(_) => ChatPoll { entries: Vec::new(), reset, status: "unreadable" },
    }
}

/// New chat entries since the last poll. Follows the newest record of this session's folder,
/// so a fresh record (after /clear or /new) is picked up.
#[tauri::command]
pub fn chat_poll(
    room: Option<String>,
    store: State<SettingsStore>,
    main: State<MainHost>,
    story: State<StoryHost>,
    smith: State<WorldsmithHost>,
    main_chat: State<MainChat>,
    story_chat: State<StoryChat>,
    smith_chat: State<WorldsmithChat>,
) -> ChatPoll {
    let room = Room::named(room.as_deref());
    let host = match room {
        Room::Main => &main.0,
        Room::Story => &story.0,
        Room::Worldsmith => &smith.0,
    };
    let chat = pick(room, &main_chat, &story_chat, &smith_chat);
    let empty = |status| ChatPoll { entries: Vec::new(), reset: false, status };
    let Some((session, started)) = host.started() else {
        return empty("asleep");
    };
    let mut settings = store.get();
    settings.cli = host.provider().unwrap_or(settings.cli);
    let Some(folder) = room.folder(&settings) else {
        return empty("waiting");
    };
    follow_a_clear(settings.cli, &folder, host);
    let Some((newest, follow)) = newest_record(settings.cli, &folder, started) else {
        return empty("waiting");
    };

    let mut slot = chat.0.lock().unwrap();
    let mut reset = false;
    let current = slot.as_ref().map(|(id, f)| (*id, f.path.clone()));
    if current != Some((session, newest.clone())) {
        // the main session's talk now lives in this record: a restart picks this one up
        if room == Room::Main && started_by_castle(settings.cli, &newest, &host.recent_enters()) {
            crate::resume::remember_followed(&store, settings.cli, &newest, &folder);
        }
        let follower = follow(newest, started);
        // The main Chat leaves the STORY room's pictures and voices to the STORY room.
        let follower = match room {
            Room::Main => follower.skipping(&folder.join(STORY_FOLDER)).skipping(&folder.join(WORLDSMITH_FOLDER)),
            Room::Story | Room::Worldsmith => follower,
        };
        *slot = Some((session, follower));
        reset = true;
    }
    let (_, follower) = slot.as_mut().expect("set above");
    match follower.read_new() {
        Ok(mut entries) => {
            entries.retain(|e| !crate::providers::castle_wrote(e));
            if let Err(error) = crate::providers::remember(&store, room, settings.cli, session, &mut entries) {
                eprintln!("MemoryCore Castle: {error}");
            }
            if reset && entries.len() > transcript::FIRST_READ_LIMIT {
                entries = entries.split_off(entries.len() - transcript::FIRST_READ_LIMIT);
            }
            if reset { entries = crate::providers::history(&store, room); }
            ChatPoll { entries, reset, status: "ok" }
        }
        Err(_) => ChatPoll { entries: Vec::new(), reset, status: "unreadable" },
    }
}

#[tauri::command]
pub fn chat_history(room: Option<String>, store: State<SettingsStore>) -> Vec<ChatEntry> {
    crate::providers::history(&store, Room::named(room.as_deref()))
}

/// The plan Claude Code is asking about, read from its plan file when the record has not carried
/// it yet. Only a plain `.md` in this user's own plans folder is ever read.
#[tauri::command]
pub async fn chat_plan_file(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || claude::read_plan_file(&path))
        .await
        .map_err(|_| "The plan file could not be read.".to_string())?
}

/// A picture or voice file Chat already showed (the companion's this session, or one the member
/// shared), as a data URL - never any other path.
#[tauri::command]
pub fn chat_media(
    path: String,
    room: Option<String>,
    main_chat: State<MainChat>,
    story_chat: State<StoryChat>,
    smith_chat: State<WorldsmithChat>,
    member: State<MemberPictures>,
    store: State<SettingsStore>,
) -> Result<String, String> {
    let chat = pick(Room::named(room.as_deref()), &main_chat, &story_chat, &smith_chat);
    let allowed = chat.was_shown(&path)
        || member.0.lock().unwrap().contains(&path)
        || crate::providers::history(&store, Room::named(room.as_deref())).iter().any(|e| e.path.as_ref() == Some(&path) || e.pictures.contains(&path));
    if !allowed {
        return Err("That file is not one Chat has shown this session.".into());
    }
    files::data_url(Path::new(&path))
}

/// Lets the asset protocol serve one film or sound Chat showed, so the screen streams it with
/// `convertFileSrc` instead of reading it whole into a data URL. Only a file this chat showed, and
/// only a film or a sound.
fn allow_stream(app: &tauri::AppHandle, path: &str) -> Result<(), String> {
    if !matches!(transcript::media_kind(path), Some("video") | Some("voice")) {
        return Err("Only a film or a sound plays here.".into());
    }
    let file = Path::new(path);
    if !file.is_file() {
        return Err("The file is gone.".into());
    }
    tauri::Manager::asset_protocol_scope(app)
        .allow_file(file)
        .map_err(|_| "The file could not be opened for playing.".to_string())
}

/// A film or a long sound the main or STORY chat showed, made playable (see `allow_stream`).
#[tauri::command]
pub fn chat_media_stream(
    path: String,
    room: Option<String>,
    main_chat: State<MainChat>,
    story_chat: State<StoryChat>,
    smith_chat: State<WorldsmithChat>,
    store: State<SettingsStore>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let chat = pick(Room::named(room.as_deref()), &main_chat, &story_chat, &smith_chat);
    let allowed = chat.was_shown(&path)
        || crate::providers::history(&store, Room::named(room.as_deref())).iter().any(|e| e.path.as_ref() == Some(&path));
    if !allowed {
        return Err("That file is not one Chat has shown this session.".into());
    }
    allow_stream(&app, &path)
}

/// A film or a long sound a DEV tab's Chat showed, made playable (see `allow_stream`).
#[tauri::command]
pub fn dev_chat_media_stream(
    slot: usize,
    path: String,
    chats: State<crate::dev::DevChats>,
    member: State<MemberPictures>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    dev_allows(&chats.0, slot, &path, &member.0.lock().unwrap())?;
    allow_stream(&app, &path)
}

/// Takes the paths the member dropped on Chat; returns the pictures among them (existing png /
/// jpg / webp files up to 25 MB), which the screen may then show.
#[tauri::command]
pub fn chat_attach(paths: Vec<String>, member: State<MemberPictures>) -> Vec<String> {
    let pictures: Vec<String> = paths
        .into_iter()
        .filter(|p| transcript::media_kind(p) == Some("picture"))
        .filter(|p| std::fs::metadata(p).map(|m| m.is_file() && m.len() <= 25 * 1024 * 1024).unwrap_or(false))
        .collect();
    member.0.lock().unwrap().extend(pictures.iter().cloned());
    pictures
}

/// Whether a DEV tab's Chat may hand the screen `path`: that tab's reader showed it, or the member
/// dropped or pasted it this run. A slot that does not exist is refused.
pub(crate) fn dev_allows(chats: &[ChatState], slot: usize, path: &str, member: &HashSet<String>) -> Result<(), String> {
    let chat = chats.get(slot).ok_or("That DEV tab does not exist.")?;
    if chat.was_shown(path) || member.contains(path) {
        Ok(())
    } else {
        Err("That file is not one this chat has shown.".into())
    }
}

/// A picture or voice a DEV tab's Chat showed, as a data URL - never any other path.
#[tauri::command]
pub fn dev_chat_media(slot: usize, path: String, chats: State<crate::dev::DevChats>, member: State<MemberPictures>) -> Result<String, String> {
    dev_allows(&chats.0, slot, &path, &member.0.lock().unwrap())?;
    files::data_url(Path::new(&path))
}

/// Opens a file a DEV tab's Chat showed in its own app, or its folder with the file selected. A
/// project's files are outside the memory folder, so the rule is the chat's own, not the fence.
#[tauri::command]
pub fn dev_media_open(slot: usize, path: String, folder: bool, chats: State<crate::dev::DevChats>, member: State<MemberPictures>) -> Result<(), String> {
    dev_allows(&chats.0, slot, &path, &member.0.lock().unwrap())?;
    let file = Path::new(&path);
    if !file.is_file() {
        return Err("The file is gone.".into());
    }
    #[cfg(not(windows))]
    return crate::os::open_path(file, folder);
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("explorer.exe");
        if folder {
            command.arg(format!("/select,{}", file.display()));
        } else {
            command.arg(file);
        }
        crate::shell::let_it_come_forward();
        command.spawn().map(|_| ()).map_err(|_| "Windows could not open it.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_dev_chat_hands_over_only_what_its_own_reader_showed_or_the_member_dropped() {
        let folder = std::env::temp_dir().join(format!("castle-dev-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let shot = folder.join("shot.png");
        std::fs::write(&shot, b"png").unwrap();
        let shot = shot.display().to_string();
        // the member's message in slot 1's record names the picture, so slot 1's reader showed it
        let record = folder.join("r.jsonl");
        let line = format!(r#"{{"type":"user","uuid":"k","message":{{"content":{}}}}}"#, serde_json::to_string(&format!("look at {shot}")).unwrap());
        std::fs::write(&record, format!("{line}\n")).unwrap();
        let chats: [ChatState; 3] = Default::default();
        let mut follower = claude::follower(record, SystemTime::now() + Duration::from_secs(3600));
        follower.read_new().unwrap();
        *chats[1].0.lock().unwrap() = Some((7, follower));
        let none = HashSet::new();

        assert!(chats[1].was_shown(&shot));
        assert!(dev_allows(&chats, 1, &shot, &none).is_ok());
        // another tab's chat never showed it, and no other path passes
        assert!(dev_allows(&chats, 0, &shot, &none).is_err());
        assert!(dev_allows(&chats, 1, r"C:\Windows\win.ini", &none).is_err());
        // a picture the member dropped shows in any tab
        let dropped = HashSet::from([shot.clone()]);
        assert!(dev_allows(&chats, 2, &shot, &dropped).is_ok());
        // a slot that does not exist is refused, whatever the path
        assert_eq!(dev_allows(&chats, 3, &shot, &dropped), Err("That DEV tab does not exist.".to_string()));
        // a closed tab forgets what it showed
        chats[1].clear();
        assert!(dev_allows(&chats, 1, &shot, &none).is_err());
        let _ = std::fs::remove_dir_all(folder);
    }
}
