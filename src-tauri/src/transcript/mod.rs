//! Chat's view of a session: the record the CLI writes for it, read only.
//! Nothing here talks to the CLI; it reads the file the CLI already keeps.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub mod claude;
pub mod codex;
pub mod grok;

/// How many entries the first read hands the screen, so a long session opens quickly.
pub const FIRST_READ_LIMIT: usize = 300;
pub const WORK_DETAIL_LIMIT: usize = 70;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEntry {
    pub id: String,
    /// "member", "companion", "work" (a tool the companion ran), "plan", "receipt", "plan-file" / "plan-end"
    /// (where a plan is written, never drawn), "picture", "voice" or "video" (a file the companion made),
    /// "peer" (a message sent to another session, state "to", or heard from one, state "from"; never
    /// drawn, it only says which DEV tab the companion's words are about)
    pub role: String,
    pub text: String,
    pub at: Option<String>,
    /// For a picture or voice: the file, which the screen may then ask the core for.
    pub path: Option<String>,
    /// For the member's messages: the pictures shared in them (dropped, pasted or typed paths that
    /// exist), shown as thumbnails under the member's words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<String>,
    /// The member's message ticks: "queued" while it waits in the CLI's queue (one tick);
    /// a "receipt" entry carries the reason it left the queue, which the screen turns into "read".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

const PICTURE_TYPES: &[&str] = &["png", "jpg", "jpeg", "webp"];
const VOICE_TYPES: &[&str] = &["mp3", "wav", "ogg", "m4a"];
/// Films the companion makes or names: played in Chat, streamed, never read whole.
const VIDEO_TYPES: &[&str] = &["mp4", "webm", "mov", "m4v"];
/// How long a path named before its file exists is waited for.
const PENDING_FOR: Duration = Duration::from_secs(5 * 60);

/// "picture", "voice", "video", or None for any other file.
pub fn media_kind(path: &str) -> Option<&'static str> {
    let extension = path.rsplit('.').next()?.to_lowercase();
    if PICTURE_TYPES.contains(&extension.as_str()) {
        Some("picture")
    } else if VOICE_TYPES.contains(&extension.as_str()) {
        Some("voice")
    } else if VIDEO_TYPES.contains(&extension.as_str()) {
        Some("video")
    } else {
        None
    }
}

/// PowerShell's list output wraps a long value onto lines indented far to the right; join them.
fn unwrap_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        out.push_str(line.trim_end_matches('\r'));
        if let Some(next) = lines.peek() {
            let indent = next.len() - next.trim_start().len();
            if indent < 10 || next.trim().is_empty() {
                out.push('\n');
            } else {
                // Continuation: drop the line break and the indentation.
                lines.next().map(|n| out.push_str(n.trim()));
                while let Some(more) = lines.peek() {
                    let more_indent = more.len() - more.trim_start().len();
                    if more_indent >= 10 && !more.trim().is_empty() {
                        out.push_str(lines.next().unwrap().trim());
                    } else {
                        break;
                    }
                }
                out.push('\n');
            }
        }
    }
    out
}

/// The separator Castle writes media paths with: `\` on Windows, `/` on macOS and Linux.
pub const SEP: char = if cfg!(windows) { '\\' } else { '/' };

/// A path in this platform's own form (Windows turns `/` into `\`; elsewhere it stays as written).
pub fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// Every picture or voice path written in `text`, each cut at its first picture or voice extension.
/// Windows: drive paths (D:\.. or D:/..) and Git Bash paths (/d/..). macOS and Linux: absolute
/// paths (/Users/..).
pub fn media_paths(text: &str) -> Vec<String> {
    let text = unwrap_lines(text);
    let chars: Vec<char> = text.chars().collect();
    let stops = ['"', '\'', '<', '>', '|', '*', '?', '\n', '\r', '\t', '`'];
    let mut found = Vec::new();
    let mut i = 0;
    while i + 2 < chars.len() {
        let windows = cfg!(windows)
            && chars[i].is_ascii_alphabetic()
            && chars[i + 1] == ':'
            && (chars[i + 2] == '\\' || chars[i + 2] == '/')
            && (i == 0 || !chars[i - 1].is_ascii_alphanumeric());
        let bash = cfg!(windows)
            && chars[i] == '/'
            && chars[i + 1].is_ascii_alphabetic()
            && chars[i + 2] == '/'
            && (i == 0 || chars[i - 1].is_whitespace() || stops.contains(&chars[i - 1]));
        let unix = !cfg!(windows)
            && chars[i] == '/'
            && chars[i + 1] != '/'
            && !chars[i + 1].is_whitespace()
            && (i == 0 || chars[i - 1].is_whitespace() || stops.contains(&chars[i - 1]) || chars[i - 1] == '(');
        if !(windows || bash || unix) {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < chars.len() && !stops.contains(&chars[end]) {
            end += 1;
        }
        let candidate: String = chars[i..end].iter().collect();
        let mut cut = None;
        for (at, _) in candidate.match_indices('.') {
            let rest = &candidate[at + 1..];
            let extension: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if !extension.is_empty() && media_kind(&format!("x.{extension}")).is_some() {
                cut = Some(at + 1 + extension.len());
                break;
            }
        }
        match cut {
            Some(length) => {
                let mut path = candidate[..length].to_string();
                if bash {
                    // /d/My Files/x.png -> D:\My Files\x.png
                    let drive = path[1..2].to_uppercase();
                    path = format!("{drive}:{}", &path[2..]);
                }
                found.push(native(&path));
                i += path.chars().count().max(1);
            }
            None => i = end.max(i + 1),
        }
    }
    found
}

/// Picture or voice paths written without a drive (`daily-diary\current\x.png`, `voice/x.mp3`),
/// as the companion's tools print them relative to its folder. Paths with spaces are not read this way
/// (full paths and Claude Code's changed-files list carry those).
pub fn relative_media_paths(text: &str) -> Vec<String> {
    let text = unwrap_lines(text);
    let stops = ['"', '\'', '<', '>', '|', '*', '?', '`', '(', ')', '[', ']', '=', ','];
    text.split(|c: char| c.is_whitespace() || stops.contains(&c))
        .map(|token| token.trim_end_matches(['.', ';', ':']))
        .filter(|token| {
            !token.is_empty()
                && !token.contains(':')
                && !token.starts_with(['/', '\\', '$', '%', '~', '-'])
                && token.contains(['\\', '/'])
                && media_kind(token).is_some()
        })
        .map(|token| native(token.trim_start_matches(".\\").trim_start_matches("./")))
        .collect()
}

/// The companion's words for the phone: each DEV-tab marker (`[dev:shop|done]`, see
/// mirror::DEV_MARKER_RULE) becomes the tab's name in brackets, "(shop)". Inside a fenced code block
/// nothing changes; a marker that is not one of the three states stays as written.
pub fn plain_dev_markers(text: &str) -> String {
    static MARKER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let marker = MARKER.get_or_init(|| regex::Regex::new(r"\[dev:\s*([^\[\]|\n`]{1,80}?)\s*(?:\|\s*(?i:go|done|fyi)\s*)?\]").unwrap());
    let mut fenced = false;
    text.split_inclusive('\n')
        .map(|line| {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                return line.to_string();
            }
            if fenced {
                line.to_string()
            } else {
                marker.replace_all(line, "($1)").to_string()
            }
        })
        .collect()
}

/// The companion's reply without the Insight block (from the line holding "★ Insight" to the next line made
/// only of rule characters). Log keeps it; Chat reads like a conversation. A block with no
/// closing rule is left whole, so no words are lost to a guess.
pub fn strip_insight(text: &str) -> String {
    fn is_rule(line: &str) -> bool {
        let bare = line.trim().trim_matches('`').trim();
        bare.chars().count() >= 10 && bare.chars().all(|c| matches!(c, '─' | '━' | '—'))
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].contains("★ Insight") {
            if let Some(end) = (i + 1..lines.len()).find(|&j| is_rule(lines[j])) {
                i = end + 1;
                continue;
            }
        }
        kept.push(lines[i]);
        i += 1;
    }
    // The blank lines left behind fold to one.
    let mut out = String::new();
    let mut blanks = 0;
    for line in kept {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

pub fn short(text: &str, limit: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() > limit {
        format!("{}…", line.chars().take(limit).collect::<String>())
    } else {
        line.to_string()
    }
}

/// A few words on what a tool is doing, from its input, for the "working:" line.
pub fn work_detail(input: &Value) -> Option<String> {
    ["description", "file_path", "command", "pattern", "skill", "url", "query", "prompt"]
        .iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .map(|text| short(text, WORK_DETAIL_LIMIT))
        .filter(|text| !text.is_empty())
}

/// The pictures the member shared in a message: picture paths in their words whose files exist.
/// They may be older than the session (a screenshot from this morning is still theirs to show).
pub fn member_pictures(text: &str) -> Vec<String> {
    let mut found: Vec<String> = media_paths(text)
        .into_iter()
        .filter(|p| media_kind(p) == Some("picture") && Path::new(p).is_file())
        .collect();
    found.dedup();
    found
}

/// Reads a record as it grows, holding back a half-written last line.
pub struct Follower {
    pub path: PathBuf,
    offset: u64,
    partial: String,
    /// Files count as the companion's only when written since the session started.
    since: SystemTime,
    /// Paths named before their files existed, with when they were first seen.
    pending: Vec<(String, SystemTime)>,
    /// Every picture and voice file shown so far: the only files the screen may ask for.
    shown: std::collections::HashSet<String>,
    /// Folders whose files are other rooms': the main Chat leaves the STORY and WORLDSMITH rooms' pictures and
    /// voices to the STORY room.
    skip: Vec<String>,
    /// Turns one record line into chat entries (the CLI's own format).
    parse: fn(&str) -> Vec<ChatEntry>,
    /// The paths in a record line that may name a file the companion made.
    candidates: fn(&str) -> Vec<String>,
}

impl Follower {
    pub fn new(path: PathBuf, since: SystemTime, parse: fn(&str) -> Vec<ChatEntry>, candidates: fn(&str) -> Vec<String>) -> Self {
        Self {
            path,
            offset: 0,
            partial: String::new(),
            since,
            pending: Vec::new(),
            shown: std::collections::HashSet::new(),
            skip: Vec::new(),
            parse,
            candidates,
        }
    }

    /// The same follower, leaving files under `folder` to another room.
    pub fn skipping(mut self, folder: &Path) -> Self {
        let folder = folder.display().to_string().replace('/', "\\").trim_end_matches('\\').to_lowercase();
        self.skip.push(format!("{folder}\\"));
        self
    }

    fn skipped(&self, path: &str) -> bool {
        let path = path.replace('/', "\\").to_lowercase();
        self.skip.iter().any(|folder| path.starts_with(folder))
    }

    /// Whether `path` was shown as a picture or voice of this session.
    pub fn was_shown(&self, path: &str) -> bool {
        self.shown.contains(path)
    }

    /// A media entry for each pending path whose file now exists and was written this session.
    fn settle_pending(&mut self) -> Vec<ChatEntry> {
        let floor = self.since.checked_sub(Duration::from_secs(60)).unwrap_or(self.since);
        let now = SystemTime::now();
        let mut ready = Vec::new();
        let mut still = Vec::new();
        for (path, seen) in std::mem::take(&mut self.pending) {
            if self.shown.contains(&path) {
                continue;
            }
            let written = std::fs::metadata(&path).ok().filter(|m| m.is_file()).and_then(|m| m.modified().ok());
            match written {
                Some(time) if time >= floor => {
                    let kind = media_kind(&path).unwrap_or("picture");
                    let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    self.shown.insert(path.clone());
                    ready.push(ChatEntry {
                        id: format!("media-{}", self.shown.len()),
                        role: kind.to_string(),
                        text: name,
                        at: None,
                        path: Some(path),
                        pictures: Vec::new(),
                        state: None,
                    });
                }
                // An older file was only mentioned, not made: never shown.
                Some(_) => {}
                None if now.duration_since(seen).unwrap_or_default() < PENDING_FOR => still.push((path, seen)),
                None => {}
            }
        }
        self.pending = still;
        ready
    }

    /// The entries in lines written since the last read.
    pub fn read_new(&mut self) -> std::io::Result<Vec<ChatEntry>> {
        let mut file = File::open(&self.path)?;
        let length = file.metadata()?.len();
        if length < self.offset {
            // The file was replaced or cut: start again.
            self.offset = 0;
            self.partial.clear();
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));

        let mut entries = Vec::new();
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            let line = line.trim_end();
            for mut entry in (self.parse)(line) {
                if entry.role == "member" {
                    entry.pictures = member_pictures(&entry.text);
                    self.shown.extend(entry.pictures.iter().cloned());
                }
                entries.push(entry);
            }
            let now = SystemTime::now();
            for path in (self.candidates)(line) {
                if self.skipped(&path) {
                    continue;
                }
                if !self.shown.contains(&path) && !self.pending.iter().any(|(p, _)| *p == path) {
                    self.pending.push((path, now));
                }
            }
            entries.extend(self.settle_pending());
        }
        // Files that finished after their lines were read.
        entries.extend(self.settle_pending());
        Ok(entries)
    }
}

#[cfg(test)]
mod dev_marker_tests {
    use super::plain_dev_markers;

    #[test]
    fn the_phone_reads_a_dev_marker_as_the_tabs_name() {
        assert_eq!(plain_dev_markers("Montee finished #1276. [dev:montee · 1|done]"), "Montee finished #1276. (montee · 1)");
        assert_eq!(plain_dev_markers("It waits for you [dev:sales-engineering|GO]\nand more"), "It waits for you (sales-engineering)\nand more");
        // a marker in a code block, or one that is not a marker, stays as written
        let code = "```\n[dev:shop|go]\n```\n";
        assert_eq!(plain_dev_markers(code), code);
        assert_eq!(plain_dev_markers("[dev:shop|maybe] [dev:|go] [dev:shop"), "[dev:shop|maybe] [dev:|go] [dev:shop");
    }
}
