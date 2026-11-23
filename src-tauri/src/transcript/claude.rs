//! Claude Code's record: one jsonl per session in %USERPROFILE%\.claude\projects\<folder name>.

use super::{media_kind, media_paths, native, relative_media_paths, SEP, strip_insight, work_detail, ChatEntry, Follower};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The paths in a record line that may name a file the companion made: its tool results' text, its
/// commands' arguments (a Write's file path, a shell command), the files Claude Code lists as
/// changed by a shell command, and drive-less paths read against the record's own folder.
pub fn media_candidates(line: &str) -> Vec<String> {
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let mut found: Vec<String> = record
        .get("toolUseResult")
        .and_then(|r| r.get("bashEditDiff"))
        .and_then(|d| d.get("changedFiles"))
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .filter_map(Value::as_str)
                .filter(|path| media_kind(path).is_some())
                .map(native)
                .collect()
        })
        .unwrap_or_default();
    let cwd = record.get("cwd").and_then(Value::as_str).map(|c| native(c.trim_end_matches(['\\', '/'])));
    let Some(Value::Array(blocks)) = record.get("message").and_then(|m| m.get("content")) else {
        return found;
    };
    let mut texts = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("tool_result") => match block.get("content") {
                Some(Value::String(text)) => texts.push(text.clone()),
                Some(Value::Array(parts)) => texts.extend(
                    parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).map(str::to_string),
                ),
                _ => {}
            },
            Some("tool_use") => {
                let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                // Reading a picture is looking, not making.
                if name == "Read" {
                    continue;
                }
                if let Some(input) = block.get("input") {
                    for key in ["file_path", "command"] {
                        if let Some(text) = input.get(key).and_then(Value::as_str) {
                            texts.push(text.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for text in &texts {
        found.extend(media_paths(text));
        if let Some(cwd) = &cwd {
            found.extend(relative_media_paths(text).into_iter().map(|p| format!("{cwd}{SEP}{p}")));
        }
    }
    let mut seen = std::collections::HashSet::new();
    found.retain(|p| seen.insert(p.to_lowercase()));
    found
}

/// Claude Code's folder name for a working folder: every character that is not a letter or
/// digit becomes `-`.
pub fn project_folder_name(folder: &str) -> String {
    folder.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Where Claude Code keeps the records for this computer's user.
pub fn projects_root() -> Option<PathBuf> {
    crate::env::home_dir().map(|home| home.join(".claude").join("projects"))
}

/// Where Claude Code keeps every session's plan files.
pub fn plans_root() -> Option<PathBuf> {
    crate::env::home_dir().map(|home| home.join(".claude").join("plans"))
}

/// A path that is a plan file: `<anything>/.claude/plans/<name>.md` (either slash, any case), the
/// name plain letters, digits, dashes and underscores. Returns the file's name.
pub fn plan_file_name(path: &str) -> Option<&str> {
    let (folder, name) = path.rsplit_once(['\\', '/'])?;
    let stem = name.strip_suffix(".md")?;
    let plain = !stem.is_empty() && stem.len() <= 120 && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let folder = folder.replace('\\', "/").to_lowercase();
    (plain && folder.ends_with("/.claude/plans")).then_some(name)
}

/// The text of a plan file. Only a plain `.md` name is taken from `path`, and it is read from this
/// user's own plans folder, whatever folder the path claims.
pub fn read_plan_file(path: &str) -> Result<String, String> {
    const LIMIT: u64 = 512 * 1024;
    let name = plan_file_name(path).ok_or("That is not a plan file.")?;
    let file = plans_root().ok_or("The plans folder could not be found.")?.join(name);
    let size = std::fs::metadata(&file).map_err(|_| "The plan file is not there.".to_string())?.len();
    if size > LIMIT {
        return Err("That is too large to be a plan.".into());
    }
    let text = std::fs::read_to_string(&file).map_err(|_| "The plan file could not be read.".to_string())?;
    Ok(text.trim().to_string())
}

/// The record of the session Castle started at `since`. Castle launches a fresh Claude Code session
/// each time, so its own record is a file CREATED after that moment; a record the member keeps
/// writing from a terminal of their own in the same folder is older and must not be taken for it
/// (it would be followed in Chat and would hold a provider switch waiting for ever). With no file
/// born since then (the first moments of a session, or a resumed one), the newest record written
/// since `since` is used, a minute's grace for clocks - but never one over 64 MB: Chat reads a
/// record on the thread that keeps the window alive, and a terminal's long record read from its
/// start froze Castle for a minute and a half.
/// Castle's own session, when its id is known (Castle gave it to Claude Code, or resumed it): its
/// record is then that one file, and nothing is guessed among the folder's other records.
static OWN_SESSION: std::sync::Mutex<Option<(PathBuf, String)>> = std::sync::Mutex::new(None);

pub fn set_own_session(records_folder: PathBuf, id: String) {
    if let Ok(mut own) = OWN_SESSION.lock() {
        *own = Some((records_folder, id));
    }
}

/// How long after a turn Castle sent a record born in the folder counts as that turn's.
pub const CLEAR_GRACE: Duration = Duration::from_secs(20);

/// After a /clear Claude Code goes on under a new id, in a new record, and Chat pinned to the
/// old one would never see another line. The pin moves to a record Castle caused: born within
/// CLEAR_GRACE after a turn Castle sent (`enters`), and after the pinned record's last write (the
/// old talk went quiet). A conversation the member starts in a terminal of their own in the same
/// folder is not born of Castle's turn, and Castle's own record keeps growing beside it, so the
/// pin stays. Returns the new id when the pin moved.
pub fn follow_clear(folder: &Path, enters: &[SystemTime]) -> Option<String> {
    let (dir, id) = OWN_SESSION.lock().ok()?.clone()?;
    if dir != folder || enters.is_empty() {
        return None;
    }
    let pinned = dir.join(format!("{id}.jsonl"));
    let quiet_since = pinned.metadata().and_then(|m| m.modified()).ok()?;
    let candidates: Vec<(PathBuf, SystemTime, u64)> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .filter_map(|p| {
            let meta = p.metadata().ok()?;
            Some((p, meta.created().ok()?, meta.len()))
        })
        .collect();
    let found = pick_cleared(candidates, &pinned, quiet_since, enters)?;
    let new_id = found.file_stem()?.to_str()?.to_string();
    set_own_session(dir, new_id.clone());
    Some(new_id)
}

/// Whether a record was born right after one of the turns Castle sent (two seconds of clock grace
/// before, CLEAR_GRACE after).
pub fn born_after_a_turn(born: SystemTime, enters: &[SystemTime]) -> bool {
    enters.iter().any(|sent| {
        let early = sent.checked_sub(Duration::from_secs(2)).unwrap_or(*sent);
        born >= early && born <= *sent + CLEAR_GRACE
    })
}

fn pick_cleared(candidates: Vec<(PathBuf, SystemTime, u64)>, pinned: &Path, quiet_since: SystemTime, enters: &[SystemTime]) -> Option<PathBuf> {
    candidates
        .into_iter()
        .filter(|(path, born, len)| path != pinned && *born > quiet_since && *len <= FALLBACK_LIMIT && born_after_a_turn(*born, enters))
        .max_by_key(|(_, born, _)| *born)
        .map(|(path, _, _)| path)
}

pub fn newest_record(folder: &Path, since: SystemTime) -> Option<PathBuf> {
    let own = OWN_SESSION.lock().ok().and_then(|own| own.clone());
    if let Some((dir, id)) = own.filter(|(dir, _)| dir == folder) {
        // not there yet in a session's first moments: wait for it, do not take another record
        let record = dir.join(format!("{id}.jsonl"));
        return record.is_file().then_some(record);
    }
    let candidates: Vec<Candidate> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .filter_map(|p| {
            let meta = p.metadata().ok()?;
            Some((meta.modified().ok()?, meta.created().ok(), meta.len(), p))
        })
        .collect();
    pick_record(candidates, since)
}

/// A record file: when written, when created, its size, where it is.
type Candidate = (SystemTime, Option<SystemTime>, u64, PathBuf);

const FALLBACK_LIMIT: u64 = 64 * 1024 * 1024;

fn pick_record(candidates: Vec<Candidate>, since: SystemTime) -> Option<PathBuf> {
    let floor = since.checked_sub(Duration::from_secs(60)).unwrap_or(since);
    let born_floor = since.checked_sub(Duration::from_secs(5)).unwrap_or(since);
    let recent: Vec<_> = candidates.into_iter().filter(|(modified, _, _, _)| *modified >= floor).collect();
    let own = recent.iter().filter(|(_, created, _, _)| created.is_some_and(|c| c >= born_floor)).max_by_key(|(modified, _, _, _)| *modified);
    own.or_else(|| recent.iter().filter(|(_, _, len, _)| *len <= FALLBACK_LIMIT).max_by_key(|(modified, _, _, _)| *modified))
        .map(|(_, _, _, path)| path.clone())
}

/// Turns one record line into the chat entries it holds (often none).
pub fn parse_line(line: &str) -> Vec<ChatEntry> {
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let kind = record.get("type").and_then(Value::as_str).unwrap_or("");
    // isCompactSummary / isVisibleInTranscriptOnly: the summary a /compact writes as a user record,
    // which is not the member's words.
    let flag = |name: &str| record.get(name).and_then(Value::as_bool) == Some(true);
    if flag("isMeta") || flag("isSidechain") || flag("isCompactSummary") || flag("isVisibleInTranscriptOnly") {
        return Vec::new();
    }
    let id = record.get("uuid").and_then(Value::as_str).unwrap_or("").to_string();
    let at = record.get("timestamp").and_then(Value::as_str).map(str::to_string);
    let content = record.get("message").and_then(|m| m.get("content"));
    let entry = |n: usize, role: &str, text: String| ChatEntry {
        id: format!("{id}-{n}"),
        role: role.to_string(),
        text,
        at: at.clone(),
        path: None,
        pictures: Vec::new(),
        state: None,
    };
    let marker = |role: &str, path: &str| {
        let stamp = at.clone().unwrap_or_default();
        vec![ChatEntry {
            id: format!("{role}-{stamp}-{id}"),
            role: role.to_string(),
            text: path.to_string(),
            at: at.clone(),
            path: None,
            pictures: Vec::new(),
            state: None,
        }]
    };

    // Where the plan is being written: the record names the plan file when plan mode starts and each
    // time the file is written - before the "present the plan" step, which Claude Code may write only
    // after the member has answered. Chat reads the file itself when the question is already on
    // screen and no plan card came. Leaving plan mode is the plan's end, never a new write.
    let attachment = record.get("attachment");
    let attachment_type = attachment.and_then(|a| a.get("type")).and_then(Value::as_str);
    let attachment_plan = attachment
        .and_then(|a| a.get("planFilePath"))
        .and_then(Value::as_str)
        .filter(|p| plan_file_name(p).is_some());
    if kind == "attachment" && attachment_type == Some("plan_mode_exit") {
        return attachment_plan.map(|path| marker("plan-end", path)).unwrap_or_default();
    }
    let plan_file = match kind {
        "attachment" if attachment_type.is_some_and(|t| t.starts_with("plan_mode")) => attachment_plan,
        "file-history-delta" => record.get("trackingPath").and_then(Value::as_str).filter(|p| plan_file_name(p).is_some()),
        _ => None,
    };
    if let Some(path) = plan_file {
        return marker("plan-file", path);
    }

    match kind {
        // A message sent while the companion was busy: it waits in the queue, then leaves it (taken in
        // mid-turn, or as the next turn's words).
        "queue-operation" => {
            let text = record.get("content").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if text.is_empty() || text.starts_with('<') {
                return Vec::new();
            }
            let stamp = at.clone().unwrap_or_default();
            match record.get("operation").and_then(Value::as_str) {
                Some("enqueue") => vec![ChatEntry {
                    id: format!("queued-{stamp}"),
                    state: Some("queued".into()),
                    ..entry(0, "member", text)
                }],
                Some("remove") => vec![ChatEntry {
                    id: format!("receipt-{stamp}"),
                    state: Some(record.get("reason").and_then(Value::as_str).unwrap_or("removed").to_string()),
                    ..entry(0, "receipt", text)
                }],
                _ => Vec::new(),
            }
        }
        "user" => {
            if record.get("toolUseResult").is_some() {
                return Vec::new();
            }
            let text = match content {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Array(blocks)) => blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            let text = text.trim();
            // Slash commands, hook and system wrappers arrive as tags; they are not the member's words.
            if text.is_empty() || text.starts_with('<') {
                Vec::new()
            } else {
                vec![entry(0, "member", text.to_string())]
            }
        }
        "assistant" => {
            let Some(Value::Array(blocks)) = content else {
                return Vec::new();
            };
            blocks
                .iter()
                .enumerate()
                .filter_map(|(n, block)| match block.get("type").and_then(Value::as_str) {
                    Some("text") => block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(strip_insight)
                        .filter(|t| !t.is_empty())
                        .map(|t| entry(n, "companion", t)),
                    // A plan the companion presents becomes a plan card holding the whole plan.
                    Some("tool_use")
                        if block.get("name").and_then(Value::as_str) == Some("ExitPlanMode")
                            && block.get("input").and_then(|i| i.get("plan")).and_then(Value::as_str).is_some() =>
                    {
                        let plan = block["input"]["plan"].as_str().unwrap_or("").trim().to_string();
                        (!plan.is_empty()).then(|| entry(n, "plan", plan))
                    }
                    Some("tool_use") => {
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("a tool");
                        let detail = block.get("input").and_then(work_detail);
                        let text = match detail {
                            Some(detail) => format!("{name}: {detail}"),
                            None => name.to_string(),
                        };
                        Some(entry(n, "work", text))
                    }
                    _ => None,
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// A follower for one Claude Code record.
pub fn follower(path: PathBuf, since: SystemTime) -> Follower {
    Follower::new(path, since, parse_line, media_candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::*;
    use std::io::Write;

    #[test]
    fn a_memory_folder_maps_to_claude_codes_folder_name() {
        assert_eq!(
            project_folder_name("D:\\Users\\Rin\\My MemoryCore"),
            "D--Users-Rin-My-MemoryCore"
        );
    }

    #[test]
    fn his_words_and_hers_become_entries_and_the_rest_stays_out() {
        let member_line = r#"{"type":"user","uuid":"u1","timestamp":"t","message":{"role":"user","content":"hi Lyra"}}"#;
        assert_eq!(parse_line(member_line)[0].role, "member");
        assert_eq!(parse_line(member_line)[0].text, "hi Lyra");

        let companion_line = r#"{"type":"assistant","uuid":"a1","message":{"role":"assistant","content":[
            {"type":"thinking","thinking":"private"},
            {"type":"text","text":"Good evening, Rin."},
            {"type":"tool_use","name":"Read","input":{"file_path":"D:\\Castle\\src\\lib.rs"}}]}}"#
            .replace('\n', "");
        let entries = parse_line(&companion_line);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].role, "companion");
        assert_eq!(entries[1].role, "work");
        assert_eq!(entries[1].text, "Read: D:\\Castle\\src\\lib.rs");

        let tool_result = r#"{"type":"user","uuid":"u2","toolUseResult":{},"message":{"role":"user","content":[{"type":"tool_result","content":"x"}]}}"#;
        assert!(parse_line(tool_result).is_empty());
        let meta = r#"{"type":"user","uuid":"u3","isMeta":true,"message":{"role":"user","content":"caveat"}}"#;
        assert!(parse_line(meta).is_empty());
        let summary = r#"{"type":"user","uuid":"u5","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued from a previous conversation that ran out of context."}}"#;
        assert!(parse_line(summary).is_empty());
        let boundary = r#"{"type":"system","subtype":"compact_boundary","uuid":"u6","content":"Conversation compacted"}"#;
        assert!(parse_line(boundary).is_empty());
        let command = r#"{"type":"user","uuid":"u4","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#;
        assert!(parse_line(command).is_empty());
        assert!(parse_line(r#"{"type":"attachment","uuid":"x"}"#).is_empty());
        assert!(parse_line("{ broken").is_empty());
    }

    #[test]
    fn the_follower_holds_back_a_half_written_line() {
        let folder = std::env::temp_dir().join(format!("castle-transcript-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("session.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, r#"{{"type":"user","uuid":"u1","message":{{"content":"first"}}}}"#).unwrap();
        write!(file, r#"{{"type":"user","uuid":"u2","mess"#).unwrap();
        file.flush().unwrap();

        let mut follower = super::follower(path.clone(), SystemTime::now());
        let first = follower.read_new().unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].text, "first");

        writeln!(file, r#"age":{{"content":"second"}}}}"#).unwrap();
        file.flush().unwrap();
        let second = follower.read_new().unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].text, "second");
        assert!(follower.read_new().unwrap().is_empty());

        assert_eq!(newest_record(&folder, SystemTime::now()), Some(path));
        assert_eq!(newest_record(&folder, SystemTime::now() + Duration::from_secs(3600)), None);
    }

    #[test]
    fn the_pin_moves_only_to_a_record_a_sent_turn_started_after_the_old_talk_went_quiet() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let at = |s: u64| t0 + Duration::from_secs(s);
        let pinned = PathBuf::from("old.jsonl");
        let cleared = (PathBuf::from("new.jsonl"), at(105), 10);
        // Castle sent "/clear" at 100 and the next line at 104; the old record last grew at 90
        assert_eq!(pick_cleared(vec![cleared.clone()], &pinned, at(90), &[at(100), at(104)]), Some(PathBuf::from("new.jsonl")));
        // no turn of Castle's near its birth: the member's own terminal
        assert_eq!(pick_cleared(vec![cleared.clone()], &pinned, at(90), &[at(10)]), None);
        // the old record kept growing after it was born: both talks are alive, the pin stays
        assert_eq!(pick_cleared(vec![cleared.clone()], &pinned, at(110), &[at(100)]), None);
        // the pinned record itself is never "a new one", and an outsized record is never taken
        assert_eq!(pick_cleared(vec![(pinned.clone(), at(105), 10)], &pinned, at(90), &[at(100)]), None);
        assert_eq!(pick_cleared(vec![(PathBuf::from("big.jsonl"), at(105), FALLBACK_LIMIT + 1)], &pinned, at(90), &[at(100)]), None);
        // born past the grace after the last turn
        assert_eq!(pick_cleared(vec![(PathBuf::from("late.jsonl"), at(100 + 21), 10)], &pinned, at(90), &[at(100)]), None);
        // two cleared records (two /clear in a row): the newest is the talk now
        let second = (PathBuf::from("newer.jsonl"), at(130), 10);
        assert_eq!(pick_cleared(vec![cleared, second], &pinned, at(90), &[at(100), at(125)]), Some(PathBuf::from("newer.jsonl")));
    }

    #[test]
    fn the_record_names_the_plan_file_before_the_plan_is_presented() {
        let entered = r#"{"type":"attachment","uuid":"a1","timestamp":"2026-11-10T10:03:19.989Z","isSidechain":false,"attachment":{"type":"plan_mode","reminderType":"full","planFilePath":"C:\\Users\\Lyra\\.claude\\plans\\synchronous-cooking-magpie.md"}}"#;
        let wrote = r#"{"type":"file-history-delta","messageId":"m","trackingPath":"C:\\Users\\Lyra\\.claude\\plans\\synchronous-cooking-magpie.md","timestamp":"2026-11-10T10:13:35.198Z"}"#;
        for line in [entered, wrote] {
            let entries = parse_line(line);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].role, "plan-file");
            assert_eq!(entries[0].text, "C:\\Users\\Lyra\\.claude\\plans\\synchronous-cooking-magpie.md");
        }
        assert_ne!(parse_line(entered)[0].id, parse_line(wrote)[0].id);
        // coming back into plan mode is a plan file too; leaving it is the plan's end
        let reentry = r#"{"type":"attachment","uuid":"a2","timestamp":"t","attachment":{"type":"plan_mode_reentry","planFilePath":"/Users/lyra/.claude/plans/synchronous-cooking-magpie.md"}}"#;
        assert_eq!(parse_line(reentry)[0].role, "plan-file");
        let exit = r#"{"type":"attachment","uuid":"a3","timestamp":"2026-11-11T15:35:43.961Z","attachment":{"type":"plan_mode_exit","planFilePath":"C:\\Users\\Lyra\\.claude\\plans\\synchronous-cooking-magpie.md"}}"#;
        let ended = parse_line(exit);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].role, "plan-end");
        assert!(parse_line(r#"{"type":"attachment","uuid":"a4","attachment":{"type":"plan_mode_exit","planFilePath":"D:\\Work\\notes.md"}}"#).is_empty());
        // another file being edited, and another attachment, are not plan files
        assert!(parse_line(r#"{"type":"file-history-delta","trackingPath":"D:\\Work\\src\\main.rs","timestamp":"t"}"#).is_empty());
        assert!(parse_line(r#"{"type":"attachment","uuid":"x","attachment":{"type":"todo","planFilePath":"C:\\Users\\Lyra\\.claude\\plans\\a.md"}}"#).is_empty());
    }

    #[test]
    fn only_a_plain_md_in_the_plans_folder_is_a_plan_file() {
        assert_eq!(plan_file_name("C:\\Users\\Lyra\\.claude\\plans\\calm-river.md"), Some("calm-river.md"));
        assert_eq!(plan_file_name("C:/Users/Lyra/.claude/plans/calm-river.md"), Some("calm-river.md"));
        assert_eq!(plan_file_name("/Users/lyra/.claude/Plans/calm_river.md"), Some("calm_river.md"));
        assert_eq!(plan_file_name("C:\\Users\\Lyra\\.claude\\plans\\..\\settings.md"), None);
        assert_eq!(plan_file_name("C:\\Users\\Lyra\\.claude\\plans\\notes.txt"), None);
        assert_eq!(plan_file_name("C:\\Users\\Lyra\\.claude\\settings.md"), None);
        assert_eq!(plan_file_name("D:\\Work\\plans\\calm-river.md"), None);
        assert_eq!(plan_file_name("calm-river.md"), None);
        assert!(read_plan_file("D:\\secrets\\passwords.md").is_err());
    }

    #[test]
    fn a_terminal_session_in_the_same_folder_is_not_taken_for_the_castle_session() {
        let at = |s: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(s);
        let started = at(10_000);
        let terminal = PathBuf::from("terminal.jsonl");
        let own = PathBuf::from("own.jsonl");
        let small = 1024;
        let huge = 110 * 1024 * 1024;
        // the terminal's file is old but written to right now; Castle's own is born after the start
        let both = vec![(at(10_900), Some(at(1_000)), huge, terminal.clone()), (at(10_300), Some(at(10_001)), small, own.clone())];
        assert_eq!(pick_record(both, started), Some(own.clone()));
        // Castle's own file is not there yet: a huge terminal record is NOT followed (it froze the window)
        assert_eq!(pick_record(vec![(at(10_900), Some(at(1_000)), huge, terminal.clone())], started), None);
        // but a small one still is, as before (a resumed session, or no birth time at all)
        let small_old = vec![(at(10_900), Some(at(1_000)), small, terminal.clone()), (at(10_300), None, small, own.clone())];
        assert_eq!(pick_record(small_old, started), Some(terminal));
        // nothing written since the start: nothing
        assert_eq!(pick_record(vec![(at(100), Some(at(50)), small, own)], started), None);
    }

    #[test]
    fn a_presented_plan_becomes_one_plan_entry_with_the_whole_plan() {
        let line = r##"{"type":"assistant","uuid":"p1","message":{"content":[
            {"type":"text","text":"Here is the plan."},
            {"type":"tool_use","name":"ExitPlanMode","input":{"plan":"# Link Lv.20 - The Update Door\n\n### Phase 1\n- [ ] one","planFilePath":"x.md"}}]}}"##
            .replace('\n', "");
        let entries = parse_line(&line);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].role, "plan");
        assert!(entries[1].text.starts_with("# Link Lv.20 - The Update Door"));
        assert!(entries[1].text.contains("### Phase 1"));

        let empty = r#"{"type":"assistant","uuid":"p2","message":{"content":[{"type":"tool_use","name":"ExitPlanMode","input":{}}]}}"#;
        assert_eq!(parse_line(empty)[0].role, "work");
    }

    // these records carry Windows paths, which only a Windows Castle reads
    #[cfg(windows)]
    #[test]
    fn media_paths_are_found_in_wrapped_lines_commands_and_bash_paths() {
        let wrapped = "saved_path        : D:\\Users\\Rin\\Castle Work\\Project \n                    Resources\\sight\\castle-design-1.png\ncost_estimate_usd : 0.01";
        assert_eq!(
            media_paths(wrapped),
            ["D:\\Users\\Rin\\Castle Work\\Project Resources\\sight\\castle-design-1.png"]
        );
        let speak = "powershell -File castle-voice.ps1 -TextFile \"C:\\t\\line.txt\" -Lang ja -OutFile \"D:\\Memory\\voice\\day 203.mp3\"";
        assert_eq!(media_paths(speak), ["D:\\Memory\\voice\\day 203.mp3"]);
        assert_eq!(media_paths("saved to /d/My Files/art/x.webp done"), ["D:\\My Files\\art\\x.webp"]);
        assert!(media_paths("edited D:\\Castle\\src\\lib.rs and C:\\notes.txt").is_empty());
        assert_eq!(media_paths("a C:\\a.png then D:\\b.mp3"), ["C:\\a.png", "D:\\b.mp3"]);
        assert_eq!(media_kind("x.JPEG"), Some("picture"));
        assert_eq!(media_kind("x.m4a"), Some("voice"));
        assert_eq!(media_kind("x.MP4"), Some("video"));
        assert_eq!(media_kind("loop.webm"), Some("video"));
        assert_eq!(media_paths("the film is at D:\\Memory\\films\\shore loop.mp4 now"), ["D:\\Memory\\films\\shore loop.mp4"]);
    }

    /// Reads a real record and lists the pictures and voices it would show (names only).
    /// Run by hand: KEEP_RECORD_FOLDER=<working folder> cargo test real_record -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_record_media() {
        let folder = std::env::var("KEEP_RECORD_FOLDER").expect("KEEP_RECORD_FOLDER");
        let records = projects_root().unwrap().join(project_folder_name(&folder));
        let since = SystemTime::now() - Duration::from_secs(24 * 3600);
        let record = newest_record(&records, since).expect("a record from the last day");
        let mut follower = super::follower(record, since);
        let media: Vec<_> = follower.read_new().unwrap().into_iter().filter(|e| e.path.is_some()).collect();
        println!("media shown: {}", media.len());
        for entry in &media {
            println!("  {} {}", entry.role, entry.text);
        }
    }

    #[test]
    fn a_made_file_is_shown_once_and_a_read_or_old_one_never() {
        let folder = std::env::temp_dir().join(format!("castle-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let picture = folder.join("art.png");
        let later = folder.join("voice.mp3");
        std::fs::write(&picture, b"png").unwrap();
        let record = folder.join("session.jsonl");
        let result = |path: &Path| {
            format!(
                r#"{{"type":"user","uuid":"r","toolUseResult":{{}},"message":{{"content":[{{"type":"tool_result","content":"saved_path : {}"}}]}}}}"#,
                path.display().to_string().replace('\\', "\\\\")
            )
        };
        let read = format!(
            r#"{{"type":"assistant","uuid":"a","message":{{"content":[{{"type":"tool_use","name":"Read","input":{{"file_path":"{}"}}}}]}}}}"#,
            folder.join("other.png").display().to_string().replace('\\', "\\\\")
        );
        std::fs::write(folder.join("other.png"), b"png").unwrap();
        std::fs::write(&record, format!("{}\n{}\n{}\n", result(&picture), read, result(&later))).unwrap();

        let mut follower = super::follower(record.clone(), SystemTime::now());
        let first = follower.read_new().unwrap();
        let media: Vec<_> = first.iter().filter(|e| e.path.is_some()).collect();
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].role, "picture");
        assert!(follower.was_shown(&picture.display().to_string()));
        assert!(!follower.was_shown(&folder.join("other.png").display().to_string()));

        // The voice file is finished after its line was read: it appears on a later read.
        std::fs::write(&later, b"mp3").unwrap();
        let second = follower.read_new().unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].role, "voice");

        // A file written before the session woke is only mentioned, never shown.
        let mut fresh_session = super::follower(record, SystemTime::now() + Duration::from_secs(3600));
        assert!(fresh_session.read_new().unwrap().iter().all(|e| e.path.is_none()));
    }

    #[test]
    fn the_insight_block_leaves_chat_and_her_words_stay() {
        let rule = "`─────────────────────────────────────────────────`";
        let open = "`★ Insight ─────────────────────────────────────`";
        let start = format!("{open}\n- one\n- two\n{rule}\n\nCastle is ready~");
        assert_eq!(strip_insight(&start), "Castle is ready~");
        let middle = format!("Done.\n\n{open}\n- why\n{rule}\n\nSealed at abc 💜");
        assert_eq!(strip_insight(&middle), "Done.\n\nSealed at abc 💜");
        let two = format!("{open}\na\n{rule}\nhi\n{open}\nb\n{rule}\nbye");
        assert_eq!(strip_insight(&two), "hi\nbye");
        assert_eq!(strip_insight(&format!("{open}\n- only\n{rule}")), "");
        let unclosed = format!("{open}\n- no end\nstill the companion's");
        assert_eq!(strip_insight(&unclosed), unclosed);
        assert_eq!(strip_insight("  plain words  "), "plain words");
        // A reply that is only an Insight block makes no bubble.
        let line = format!(
            r#"{{"type":"assistant","uuid":"i","message":{{"content":[{{"type":"text","text":{}}}]}}}}"#,
            serde_json::to_string(&format!("{open}\n- x\n{rule}")).unwrap()
        );
        assert!(parse_line(&line).is_empty());
    }

    // these records carry Windows paths, which only a Windows Castle reads
    #[cfg(windows)]
    #[test]
    fn changed_files_and_relative_paths_name_what_she_made() {
        let changed = r#"{"type":"user","uuid":"c","cwd":"D:\\Memory","toolUseResult":{"bashEditDiff":{"changedFiles":["D:\\Memory\\daily-diary\\x-art.png","D:\\Memory\\notes.md"]}},"message":{"content":[{"type":"tool_result","content":"saved_path        : D:\\Memory\\daily-diary\\x-comm\ncost_estimate_usd : 0.01"}]}}"#;
        assert_eq!(media_candidates(changed), ["D:\\Memory\\daily-diary\\x-art.png"]);

        let spoke = r#"{"type":"user","uuid":"s","cwd":"D:\\Memory","message":{"content":[{"type":"tool_result","content":"SPOKE elevenlabs ja -> daily-diary\\current\\2026-11-07\\voice\\2047-how-i-feel-tonight.mp3 (306407 bytes)"}]}}"#;
        assert_eq!(media_candidates(spoke), ["D:\\Memory\\daily-diary\\current\\2026-11-07\\voice\\2047-how-i-feel-tonight.mp3"]);

        let joined = r#"{"type":"assistant","uuid":"j","cwd":"D:\\Memory","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"$out = Join-Path $root \"daily-diary\\current\\a.png\"; & castle-image.ps1 -OutFile $out"}}]}}"#;
        assert_eq!(media_candidates(joined), ["D:\\Memory\\daily-diary\\current\\a.png"]);

        // Without the record's folder a relative path cannot be placed; flags and full paths are not relative.
        let no_cwd = r#"{"type":"user","uuid":"n","message":{"content":[{"type":"tool_result","content":"wrote voice/x.mp3"}]}}"#;
        assert!(media_candidates(no_cwd).is_empty());
        assert!(relative_media_paths("-OutFile x.png C:\\a\\b.png /d/a/b.png").is_empty());
        assert_eq!(relative_media_paths("see ./art/x.webp."), ["art\\x.webp"]);
    }

    #[test]
    fn main_chat_leaves_the_story_rooms_pictures_to_the_story_room() {
        let memory = std::env::temp_dir().join(format!("castle-skip-{}", std::process::id()));
        let story_scene = memory.join("adventures").join("w").join("scenes");
        std::fs::create_dir_all(&story_scene).unwrap();
        let painting = story_scene.join("04-witch.png");
        let diary_art = memory.join("art.png");
        std::fs::write(&painting, b"png").unwrap();
        std::fs::write(&diary_art, b"png").unwrap();
        let record = memory.join("r.jsonl");
        let line = |p: &Path| {
            format!(
                r#"{{"type":"user","uuid":"u","toolUseResult":{{}},"message":{{"content":[{{"type":"tool_result","content":{}}}]}}}}"#,
                serde_json::to_string(&format!("saved_path : {}", p.display())).unwrap()
            )
        };
        std::fs::write(&record, format!("{}\n{}\n", line(&painting), line(&diary_art))).unwrap();

        let mut main_chat = super::follower(record.clone(), SystemTime::now() - Duration::from_secs(5)).skipping(&memory.join("adventures"));
        let shown: Vec<_> = main_chat.read_new().unwrap().into_iter().filter_map(|e| e.path).collect();
        assert_eq!(shown, [diary_art.display().to_string()]);

        let mut story_chat = super::follower(record, SystemTime::now() - Duration::from_secs(5));
        assert_eq!(story_chat.read_new().unwrap().iter().filter(|e| e.path.is_some()).count(), 2);
        let _ = std::fs::remove_dir_all(memory);
    }

    #[test]
    fn his_shared_pictures_ride_on_his_message_and_may_be_shown() {
        let folder = std::env::temp_dir().join(format!("castle-member-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let shot = folder.join("Screenshot 1.png");
        std::fs::write(&shot, b"png").unwrap();
        let text = format!("look & '{}' and this C:\\nowhere\\gone.png", shot.display());
        assert_eq!(member_pictures(&text), [shot.display().to_string()]);
        let record = folder.join("r.jsonl");
        let line = format!(
            r#"{{"type":"user","uuid":"k","message":{{"content":{}}}}}"#,
            serde_json::to_string(&text).unwrap()
        );
        std::fs::write(&record, format!("{line}\n")).unwrap();
        let mut follower = super::follower(record, SystemTime::now() + Duration::from_secs(3600));
        let entries = follower.read_new().unwrap();
        assert_eq!(entries[0].role, "member");
        assert_eq!(entries[0].pictures, [shot.display().to_string()]);
        // The member's picture is theirs even when older than the session.
        assert!(follower.was_shown(&shot.display().to_string()));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[cfg(not(windows))]
    #[test]
    fn mac_paths_are_found_in_tool_output_commands_and_changed_files() {
        let wrapped = "saved_path        : /Users/rin/Castle Work/Project \n                    Resources/sight/castle-design-1.png\ncost_estimate_usd : 0.01";
        assert_eq!(media_paths(wrapped), ["/Users/rin/Castle Work/Project Resources/sight/castle-design-1.png"]);
        assert_eq!(media_paths("node castle-voice.mjs --text-file \"/tmp/t.txt\" --out-file \"/Users/rin/Memory/voice/day 203.mp3\""), ["/Users/rin/Memory/voice/day 203.mp3"]);
        assert!(media_paths("edited /Users/rin/castle/src/lib.rs and /tmp/notes.txt").is_empty());
        assert!(media_paths("a url https://x.test/a.png is not a file").is_empty());
        let changed = r#"{"type":"user","uuid":"c","cwd":"/Users/rin/Memory","toolUseResult":{"bashEditDiff":{"changedFiles":["/Users/rin/Memory/daily-diary/x-art.png","/Users/rin/Memory/notes.md"]}},"message":{"content":[{"type":"tool_result","content":"SPOKE elevenlabs ja -> daily-diary/current/voice/2047.mp3 (306407 bytes)"}]}}"#;
        assert_eq!(media_candidates(changed), ["/Users/rin/Memory/daily-diary/x-art.png", "/Users/rin/Memory/daily-diary/current/voice/2047.mp3"]);
    }

    #[test]
    fn a_relative_path_that_does_not_exist_is_never_shown() {
        let folder = std::env::temp_dir().join(format!("castle-relative-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("voice")).unwrap();
        std::fs::write(folder.join("voice").join("here.mp3"), b"mp3").unwrap();
        let record = folder.join("r.jsonl");
        let line = format!(
            r#"{{"type":"user","uuid":"v","cwd":{},"message":{{"content":[{{"type":"tool_result","content":"SPOKE -> voice{sep}here.mp3 and voice{sep}missing.mp3"}}]}}}}"#,
            serde_json::to_string(&folder.display().to_string()).unwrap(),
            sep = if cfg!(windows) { "\\\\" } else { "/" }
        );
        std::fs::write(&record, format!("{line}\n")).unwrap();
        let mut follower = super::follower(record, SystemTime::now());
        let media: Vec<_> = follower.read_new().unwrap().into_iter().filter(|e| e.path.is_some()).collect();
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].role, "voice");
        assert!(media[0].path.as_deref().unwrap().ends_with(&format!("voice{SEP}here.mp3")));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_message_sent_while_she_works_is_queued_then_leaves_the_queue() {
        let queued = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-08T13:07:40.802Z","content":"also add upload and download below ping"}"#;
        let taken = r#"{"type":"queue-operation","operation":"remove","timestamp":"2026-11-08T13:07:43.194Z","content":"also add upload and download below ping","reason":"absorbed_mid_turn"}"#;
        let folded = r#"{"type":"attachment","uuid":"a","attachment":{"type":"queued_command","prompt":"also add upload and download below ping"}}"#;
        let q = parse_line(queued);
        assert_eq!((q[0].role.as_str(), q[0].state.as_deref(), q[0].text.as_str()), ("member", Some("queued"), "also add upload and download below ping"));
        let r = parse_line(taken);
        assert_eq!((r[0].role.as_str(), r[0].state.as_deref()), ("receipt", Some("absorbed_mid_turn")));
        assert_ne!(q[0].id, r[0].id);
        assert!(parse_line(folded).is_empty(), "the folded copy is not shown twice");
        let hook = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","content":"<task-notification>x</task-notification>"}"#;
        assert!(parse_line(hook).is_empty());
    }
}
