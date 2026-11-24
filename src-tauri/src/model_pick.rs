//! Which model a Claude Code session starts on when the member's model has used up its own weekly
//! share. Claude Code's `--fallback-model` only steps in when a model is overloaded, never when its
//! share is spent, and a running session cannot change its model, so the choice is made before the
//! session starts. Nothing here is tied to one plan's names: a model draws on a share when the
//! share's name is in the model's id (`claude-fable-5-2` and "Fable").

use crate::quota::SpentShare;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The aliases a session may fall back to, in order. Claude Code resolves each to the newest model
/// of its family.
const FALLBACKS: [&str; 2] = ["opus", "sonnet"];

/// How much of a record's end is read for its last model.
const TAIL: u64 = 256 * 1024;

/// A session that starts on another model, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Fallback {
    /// the alias given to `--model`
    pub model: &'static str,
    /// the share that is spent ("Fable")
    pub share: String,
    /// when it opens again, in milliseconds since 1970
    pub until_ms: i64,
}

/// The word of a share's name a model id carries: "Fable" -> "fable", "Opus 5.5" -> "opus".
fn share_word(share: &str) -> String {
    share.split(|c: char| !c.is_ascii_alphabetic()).find(|w| !w.is_empty()).unwrap_or("").to_ascii_lowercase()
}

fn draws_on(model: &str, share: &str) -> bool {
    let word = share_word(share);
    !word.is_empty() && model.to_ascii_lowercase().contains(&word)
}

/// The fallback for a session about to start on `asked`, given the spent shares: None when the
/// asked model's share is open, or when every fallback's share is spent too (the session then
/// starts as asked and Claude Code says why itself). A share whose reset has passed is open.
pub fn pick(asked: &str, spent: &[SpentShare], now_ms: i64) -> Option<Fallback> {
    let live: Vec<&SpentShare> = spent.iter().filter(|s| s.until_ms > now_ms).collect();
    let hit = live.iter().find(|s| draws_on(asked, &s.name))?;
    let model = FALLBACKS.into_iter().find(|alias| !live.iter().any(|s| draws_on(alias, &s.name)))?;
    Some(Fallback { model, share: hit.name.clone(), until_ms: hit.until_ms })
}

/// The model a Claude Code record last answered with - what the member's default really resolved
/// to. Read from the record's end; Claude Code's own `<synthetic>` lines are passed over.
pub fn last_model(record: &Path) -> Option<String> {
    let mut file = std::fs::File::open(record).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    last_model_in(&String::from_utf8_lossy(&tail))
}

fn last_model_in(text: &str) -> Option<String> {
    const KEY: &str = "\"model\":\"";
    let mut end = text.len();
    while let Some(at) = text[..end].rfind(KEY) {
        let value: String = text[at + KEY.len()..].chars().take_while(|c| *c != '"').collect();
        if value.starts_with("claude-") && crate::oneshot::clean_model(&value).as_deref() == Some(value.as_str()) {
            return Some(value);
        }
        end = at;
    }
    None
}

/// How long a wake waits for the usage answer when the shared one is old.
const WAKE_WAIT: std::time::Duration = std::time::Duration::from_secs(4);

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The newest record in a Claude Code records folder (by its last write), for a session that has
/// no remembered record: what that folder's sessions last ran on.
pub fn newest_record(records: &Path) -> Option<std::path::PathBuf> {
    std::fs::read_dir(records)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|path| Some((path.metadata().ok()?.modified().ok()?, path)))
        .max_by_key(|(at, _)| *at)
        .map(|(_, path)| path)
}

/// The fallback for a Claude Code session about to start in `records`' folder: the model is read
/// from `record` (the one being resumed) or else the folder's newest record. Nothing when the
/// member's switch is off, when no record says which model it is, or when the share is open.
pub fn for_session(on: bool, record: Option<&Path>, records: Option<&Path>) -> Option<Fallback> {
    if !on {
        return None;
    }
    let asked = record.and_then(last_model).or_else(|| records.and_then(newest_record).and_then(|r| last_model(&r)))?;
    remember_main(&asked);
    pick(&asked, &crate::quota::claude_spent_shares(WAKE_WAIT), now_ms())
}

/// The model the main session last asked for, so a background run on the member's default
/// (no model given) knows what that default is.
static MAIN_MODEL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn remember_main(model: &str) {
    *MAIN_MODEL.lock().unwrap_or_else(|p| p.into_inner()) = Some(model.to_string());
}

/// The fallback for a background run: on the model given, else on the member's default as the
/// last wake saw it. Only after a wake whose switch was on (a wake with it off forgets the model).
pub fn for_background(model: Option<&str>) -> Option<Fallback> {
    let known = MAIN_MODEL.lock().unwrap_or_else(|p| p.into_inner()).clone()?;
    let asked = model.map(str::to_string).unwrap_or(known);
    pick(&asked, &crate::quota::claude_spent_shares(WAKE_WAIT), now_ms())
}

/// A wake with the switch off: background runs keep their own model too.
pub fn forget_main() {
    *MAIN_MODEL.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const LATER: i64 = NOW + 3_600_000;

    fn spent(names: &[&str]) -> Vec<SpentShare> {
        names.iter().map(|n| SpentShare { name: n.to_string(), until_ms: LATER }).collect()
    }

    #[test]
    fn a_model_without_a_spent_share_is_never_changed() {
        assert_eq!(pick("claude-opus-5-5", &spent(&["Fable"]), NOW), None);
        assert_eq!(pick("claude-sonnet-5-5", &[], NOW), None);
    }

    #[test]
    fn a_spent_share_starts_the_fallback_until_its_reset() {
        assert_eq!(pick("claude-fable-5-2", &spent(&["Fable"]), NOW), Some(Fallback { model: "opus", share: "Fable".into(), until_ms: LATER }));
        assert_eq!(pick("claude-fable-5-2", &spent(&["Fable 5"]), NOW).map(|f| f.model), Some("opus"), "the share's first word is matched");
    }

    #[test]
    fn the_next_fallback_when_the_first_is_spent_too_and_none_when_all_are() {
        assert_eq!(pick("claude-fable-5-2", &spent(&["Fable", "Opus"]), NOW).map(|f| f.model), Some("sonnet"));
        assert_eq!(pick("claude-opus-5-5", &spent(&["Opus"]), NOW).map(|f| f.model), Some("sonnet"));
        assert_eq!(pick("claude-fable-5-2", &spent(&["Fable", "Opus", "Sonnet"]), NOW), None);
    }

    #[test]
    fn a_share_past_its_reset_is_open_again() {
        let old = vec![SpentShare { name: "Fable".into(), until_ms: NOW }];
        assert_eq!(pick("claude-fable-5-2", &old, NOW), None);
    }

    #[test]
    fn an_empty_share_name_matches_nothing() {
        assert_eq!(pick("claude-fable-5-2", &spent(&["", "5.5"]), NOW), None);
    }

    #[test]
    fn the_last_real_model_of_a_record_is_read() {
        let text = concat!(
            "{\"message\":{\"model\":\"claude-fable-5-2\"}}\n",
            "{\"message\":{\"model\":\"claude-opus-5-5\"}}\n",
            "{\"message\":{\"model\":\"<synthetic>\"}}\n",
        );
        assert_eq!(last_model_in(text).as_deref(), Some("claude-opus-5-5"));
        assert_eq!(last_model_in("{\"model\":\"<synthetic>\"}"), None);
        assert_eq!(last_model_in("{\"model\":\"claude-x --tools Bash\"}"), None, "never a value that could carry a flag");
        let file = std::env::temp_dir().join(format!("castle-last-model-{}.jsonl", std::process::id()));
        std::fs::write(&file, text).unwrap();
        assert_eq!(last_model(&file).as_deref(), Some("claude-opus-5-5"));
        let _ = std::fs::remove_file(&file);
        assert_eq!(last_model(Path::new("C:/castle/no-such-record.jsonl")), None);
    }

    #[test]
    fn the_newest_record_of_a_folder_is_found() {
        let dir = std::env::temp_dir().join(format!("castle-newest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("old.jsonl"), "{}").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("new.jsonl"), "{}").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert_eq!(newest_record(&dir), Some(dir.join("new.jsonl")));
        assert_eq!(for_session(false, Some(&dir.join("new.jsonl")), Some(&dir)), None, "the switch off asks nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
