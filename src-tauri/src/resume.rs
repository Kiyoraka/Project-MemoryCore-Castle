//! Castle's own conversation, remembered across restarts. A fresh session each time Castle woke
//! meant a restart, a crash or a sleep lost what was being said (a phone chat counting to ten
//! began again at "Good evening"). Castle now keeps the id of its own session for each provider
//! and resumes THAT session - never "the newest one in the folder", which is the member's own
//! terminal conversation when they also use Claude Code or Codex by hand in the same folder.
//! A provider switch is always a fresh session (a conversation cannot move between providers).

use crate::settings::{Cli, SettingsStore, REST_HOURS};
use crate::transcript::{claude, codex};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Saved {
    claude: Option<Remembered>,
    codex: Option<Remembered>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Remembered {
    id: String,
    folder: String,
}

/// What the CLI is started with, and whether it picks an older conversation up.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub args: Vec<String>,
    pub resumed: bool,
}

fn file(store: &SettingsStore) -> PathBuf {
    store.folder().join("main-session.json")
}

fn load(store: &SettingsStore) -> Saved {
    std::fs::read_to_string(file(store)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn save(store: &SettingsStore, saved: &Saved) {
    if let Ok(text) = serde_json::to_string_pretty(saved) {
        let _ = std::fs::write(file(store), text);
    }
}

fn remember(store: &SettingsStore, cli: Cli, id: &str, folder: &Path) {
    let mut saved = load(store);
    let entry = Some(Remembered { id: id.to_string(), folder: folder.display().to_string() });
    match cli {
        Cli::Claude => saved.claude = entry,
        Cli::Codex => saved.codex = entry,
        // their places in main-session.json come with Task 8
        Cli::Gemini | Cli::Grok => return,
    }
    save(store, &saved);
}

/// The session id a record carries in its file name: Claude Code's `<id>.jsonl`, Codex's rollout.
pub fn id_of_record(cli: Cli, record: &Path) -> Option<String> {
    match cli {
        Cli::Claude => record.file_stem()?.to_str().filter(|id| is_id(id)).map(str::to_string),
        Cli::Codex => id_from_rollout(record),
        // their record layouts are read on this PC first (Phase 4)
        Cli::Gemini | Cli::Grok => None,
    }
}

/// Chat moved to another record of the main session (a /clear in Claude Code, a /new in Codex):
/// that conversation is the one to pick up again, not the one before it.
pub fn remember_followed(store: &SettingsStore, cli: Cli, record: &Path, folder: &Path) {
    if let Some(id) = id_of_record(cli, record) {
        let saved = load(store);
        let kept = match cli {
            Cli::Claude => saved.claude,
            Cli::Codex => saved.codex,
            Cli::Gemini | Cli::Grok => None,
        };
        if kept.as_ref().map(|r| (r.id.as_str(), r.folder.as_str())) != Some((id.as_str(), &folder.display().to_string())) {
            remember(store, cli, &id, folder);
        }
    }
}

/// A fresh random (version 4) id in the usual written form.
pub fn new_id() -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
        b.copy_from_slice(&nanos.to_le_bytes());
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Whether text is an id in the usual written form (36 characters: hex and four dashes).
pub fn is_id(text: &str) -> bool {
    text.len() == 36 && text.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// The session id inside a Codex rollout's file name (`rollout-<time>-<id>.jsonl`).
pub fn id_from_rollout(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let id = stem.get(stem.len().checked_sub(36)?..)?;
    is_id(id).then(|| id.to_string())
}

/// The CLI's own words for starting a conversation: Claude Code is told the id of a fresh one, or
/// to resume; Codex has `resume <id>` as a subcommand (so it comes first) and names a fresh one itself.
pub fn args_for(cli: Cli, resume: Option<&str>, fresh_id: &str) -> Vec<String> {
    match (cli, resume) {
        (Cli::Claude, Some(id)) => vec!["--resume".into(), id.into()],
        (Cli::Claude, None) => vec!["--session-id".into(), fresh_id.into()],
        (Cli::Codex, Some(id)) => vec!["resume".into(), id.into()],
        (Cli::Codex, None) => Vec::new(),
        // fresh every time until Task 8 gives them their own words
        (Cli::Gemini | Cli::Grok, _) => Vec::new(),
    }
}

fn claude_records(folder: &Path) -> Option<PathBuf> {
    Some(claude::projects_root()?.join(claude::project_folder_name(&folder.display().to_string())))
}

/// Whether the member's start setting (`main_start`, one of settings::MAIN_STARTS) picks a talk up
/// whose record was last written at `last_write`: `rested` only while it has been quiet less than
/// REST_HOURS - the morning after, a fresh session wakes from the memory instead of reading the
/// whole old talk back at full price.
pub fn resumes(start: &str, last_write: Option<SystemTime>, now: SystemTime) -> bool {
    match start {
        "fresh" => false,
        "continue" => true,
        _ => last_write.is_some_and(|at| now.duration_since(at).map_or(true, |quiet| quiet < Duration::from_secs(REST_HOURS * 3600))),
    }
}

/// How to start the main session. `allow_resume` is false for a provider switch and for the
/// member's one-off "Start fresh"; `start` is their setting.
pub fn plan(store: &SettingsStore, cli: Cli, folder: &Path, allow_resume: bool, start: &str) -> Plan {
    let saved = load(store);
    let remembered = match cli {
        Cli::Claude => saved.claude,
        Cli::Codex => saved.codex,
        Cli::Gemini | Cli::Grok => None,
    }
    .filter(|r| allow_resume && r.folder == folder.display().to_string() && is_id(&r.id));
    let record = |id: &str| -> Option<std::fs::Metadata> {
        match cli {
            Cli::Claude => claude_records(folder)?.join(format!("{id}.jsonl")).metadata().ok().filter(|m| m.len() > 0),
            Cli::Codex => codex::record_with_id(id)?.metadata().ok(),
            Cli::Gemini | Cli::Grok => None,
        }
    };
    let now = SystemTime::now();
    let resumable = remembered.map(|r| r.id).filter(|id| record(id).is_some_and(|m| resumes(start, m.modified().ok(), now)));
    let fresh = new_id();
    let args = args_for(cli, resumable.as_deref(), &fresh);
    match cli {
        Cli::Claude => {
            // Claude Code takes the id we give it, so its record is known exactly: Chat and the
            // provider switch follow that file and never guess among the folder's records.
            let id = resumable.clone().unwrap_or_else(|| fresh.clone());
            if resumable.is_none() {
                remember(store, cli, &id, folder);
            }
            if let Some(dir) = claude_records(folder) {
                claude::set_own_session(dir, id);
            }
        }
        // Grok's pin follows with its reader (Phase 6)
        Cli::Codex | Cli::Gemini | Cli::Grok => {}
    }
    Plan { args, resumed: resumable.is_some() }
}

/// Codex names its own sessions: once a fresh one's rollout appears, its id is kept.
pub fn remember_codex_when_it_appears(app: &AppHandle, folder: PathBuf) {
    let app = app.clone();
    let since = SystemTime::now();
    std::thread::spawn(move || {
        for _ in 0..45 {
            std::thread::sleep(Duration::from_secs(2));
            if let Some(id) = codex::newest_record(&folder, since).and_then(|p| id_from_rollout(&p)) {
                remember(&app.state::<SettingsStore>(), Cli::Codex, &id, &folder);
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_id_has_the_written_form_and_is_not_repeated() {
        let (a, b) = (new_id(), new_id());
        assert!(is_id(&a) && is_id(&b));
        assert_ne!(a, b);
        assert_eq!(a.as_bytes()[14], b'4');
        assert!(!is_id("not-an-id") && !is_id(&format!("{a}x")));
    }

    #[test]
    fn each_cli_is_started_in_its_own_words() {
        let id = "6fa92052-0000-4000-8000-000000000001";
        assert_eq!(args_for(Cli::Claude, Some(id), "new"), vec!["--resume", id]);
        assert_eq!(args_for(Cli::Claude, None, "new"), vec!["--session-id", "new"]);
        assert_eq!(args_for(Cli::Codex, Some(id), "new"), vec!["resume", id]);
        assert!(args_for(Cli::Codex, None, "new").is_empty());
    }

    #[test]
    fn a_codex_rollout_names_its_session() {
        let path = Path::new("C:/x/rollout-2026-11-21T12-00-00-019a1b2c-3d4e-7f80-9a1b-2c3d4e5f6a7b.jsonl");
        assert_eq!(id_from_rollout(path).as_deref(), Some("019a1b2c-3d4e-7f80-9a1b-2c3d4e5f6a7b"));
        assert_eq!(id_from_rollout(Path::new("rollout-short.jsonl")), None);
    }

    #[test]
    fn the_start_setting_decides_whether_a_quiet_talk_is_picked_up() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let quiet = |mins: u64| Some(now - Duration::from_secs(mins * 60));
        assert!(resumes("continue", quiet(60 * 24 * 3), now));
        assert!(!resumes("fresh", quiet(1), now));
        assert!(resumes("rested", quiet(5 * 60 + 59), now));
        assert!(!resumes("rested", quiet(6 * 60 + 1), now));
        assert!(!resumes("rested", None, now), "an unknown age is not picked up after a rest");
        // a clock that went backwards still picks a talk up
        assert!(resumes("rested", Some(now + Duration::from_secs(60)), now));
    }

    #[test]
    fn a_followed_record_names_its_session() {
        let id = "6fa92052-0000-4000-8000-000000000001";
        assert_eq!(id_of_record(Cli::Claude, Path::new(&format!("C:/r/{id}.jsonl"))).as_deref(), Some(id));
        assert_eq!(id_of_record(Cli::Claude, Path::new("C:/r/notes.jsonl")), None);
        let rollout = Path::new("C:/x/rollout-2026-11-21T12-00-00-019a1b2c-3d4e-7f80-9a1b-2c3d4e5f6a7b.jsonl");
        assert_eq!(id_of_record(Cli::Codex, rollout).as_deref(), Some("019a1b2c-3d4e-7f80-9a1b-2c3d4e5f6a7b"));
    }

    #[test]
    fn what_is_remembered_survives_being_written_and_read() {
        let saved = Saved { claude: Some(Remembered { id: new_id(), folder: "C:/m".into() }), codex: None };
        let text = serde_json::to_string(&saved).unwrap();
        assert_eq!(serde_json::from_str::<Saved>(&text).unwrap(), saved);
    }
}
