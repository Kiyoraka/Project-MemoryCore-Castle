//! MUSIC STUDIO: whole songs made with the companion, as a room of its own (taken from the Alice
//! Keep's studio). The room's session lives in `<memory>/music-studio`, taught by Castle's rules
//! between their markers in CLAUDE.md and AGENTS.md (WORLDSMITH's way). Every control on the page is
//! a named action, and the companion reaches the same actions through three files in that folder:
//!
//! - `state.json`: the page writes what is open on every change, swapped in whole, so the companion
//!   reads it (`castle-memory music look`);
//! - `actions.jsonl`: the companion appends one action per line (`castle-memory music act`); Castle
//!   follows the file from the moment it opens (an older line is never run) and hands each new line
//!   to the page as a `music-action` event, where the member sees it happen;
//! - `results/<id>.json`: the page's answer `{ id, ok, note }`. With no studio open Castle answers by
//!   itself after 8 s, so the door never waits on nothing.
//!
//! The files sit in the room's own folder because the door finds it there: the room's session is
//! given the memory folder (CASTLE_MEMORY) and starts inside it. Songs land where STUDIO keeps them,
//! `<memory>/music/<album>/`, so they show in STUDIO's gallery too. The paid steps run on the
//! member's own sunoapi.org key; the page never calls them.

use crate::session::MUSIC_FOLDER;
use crate::settings::SettingsStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

/// The room's id (Settings > Rooms, the launcher, `rooms::need`).
pub const ROOM: &str = "music";
pub const ACTION_EVENT: &str = "music-action";
const STATE_CAP: usize = 4 * 1024 * 1024;
const LINE_CAP: usize = 64 * 1024;
const NOTE_CAP: usize = 500;
const TICK: Duration = Duration::from_millis(400);
/// How long the page has to answer before Castle says it is not open (the door waits 10 s).
const ANSWER_WAIT: Duration = Duration::from_secs(8);
/// How long the door waits for an answer.
const DOOR_WAIT: Duration = Duration::from_secs(10);
const RESULTS_KEPT: Duration = Duration::from_secs(24 * 60 * 60);
/// The shelf STUDIO keeps songs on (studio.rs SHELVES).
pub const SONG_SHELF: &str = "music";

/// The room's folder: the session starts here and the bridge's files live here.
pub fn folder_of(memory: &Path) -> PathBuf {
    memory.join(MUSIC_FOLDER)
}

fn memory_of(store: &SettingsStore) -> Result<PathBuf, String> {
    store.get().memory_folder.map(PathBuf::from).ok_or_else(|| "No MemoryCore folder is chosen yet.".to_string())
}

// ---------------------------------------------------------------------------------------------------
// The room's folder and its rules
// ---------------------------------------------------------------------------------------------------

const RULES: &str = include_str!("../resources/music-rules.md");
const BEGIN: &str = "<!-- castle:music:begin";
const END: &str = "<!-- castle:music:end -->";

/// `text` with the rules put in place between their markers; everything else kept. Pure.
pub fn with_rules(text: &str, rules: &str) -> String {
    let rules = rules.trim_end();
    if let (Some(start), Some(end)) = (text.find(BEGIN), text.find(END)) {
        if end > start {
            return format!("{}{}{}", &text[..start], rules, &text[end + END.len()..]);
        }
    }
    if text.trim().is_empty() {
        format!("{rules}\n")
    } else {
        format!("{}\n\n{rules}\n", text.trim_end())
    }
}

/// Makes the room's folder and the song shelf (when missing) and writes the rules with the names,
/// only when they changed.
pub fn prepare(memory: &Path, companion: &str, member: &str) -> Result<PathBuf, String> {
    let folder = folder_of(memory);
    std::fs::create_dir_all(folder.join("results")).map_err(|_| "The music studio folder could not be created.".to_string())?;
    let _ = std::fs::create_dir_all(memory.join(SONG_SHELF));
    let rules = RULES.replace("{{AI_NAME}}", companion.trim()).replace("{{USER_NAME}}", member.trim());
    for name in crate::setup::INSTRUCTION_FILES {
        let path = folder.join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let next = with_rules(&text, &rules);
        if next != text {
            std::fs::write(&path, next).map_err(|_| format!("{name} could not be written in the music studio folder."))?;
        }
    }
    Ok(folder)
}

#[tauri::command]
pub fn music_prepare(app: AppHandle, store: State<SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, ROOM)?;
    let s = store.get();
    let memory = memory_of(&store)?;
    let member = s.member_name.clone().unwrap_or_else(|| "the member".into());
    let folder = prepare(&memory, &s.companion_label(), &member)?;
    // the takes, stems and seeds play through the asset protocol, scoped to the memory folder
    crate::studio::allow_memory_folder(&app);
    Ok(folder.display().to_string())
}

/// Ends the room's session (the room closed or switched off).
#[tauri::command]
pub fn music_close(music: State<crate::session::MusicHost>) {
    music.0.kill();
}

// ---------------------------------------------------------------------------------------------------
// The bridge
// ---------------------------------------------------------------------------------------------------

/// An action id as the door writes it (pure): letters, digits and dashes, 1 to 64.
pub fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// An action name (pure): `step.set`, `stem.volume`, `cover_art`: lower letters, '.' and '_', 1 to 40.
pub fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_')
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Action {
    pub id: String,
    pub name: String,
    pub values: Value,
    pub by: &'static str,
}

/// One line of `actions.jsonl` read (pure): `Ok` an action for the page, `Err((id, why))` a line with an id that is
/// answered at once, `None` a line too broken to answer.
pub fn parse_line(line: &str) -> Option<Result<Action, (String, String)>> {
    let line = line.trim();
    if line.is_empty() || line.len() > LINE_CAP {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let id = v["id"].as_str().filter(|id| plain_id(id))?.to_string();
    let Some(name) = v["name"].as_str().filter(|n| plain_name(n)) else {
        return Some(Err((id, "That action has no plain name.".into())));
    };
    let values = match &v["values"] {
        Value::Null => json!({}),
        Value::Object(_) => v["values"].clone(),
        _ => return Some(Err((id, "An action's values are an object.".into()))),
    };
    Some(Ok(Action { id, name: name.to_string(), values, by: "companion" }))
}

/// The lines finished since the last read (pure): the complete lines, and the tail still being written kept back.
pub fn split_lines(carry: &mut String, chunk: &str) -> Vec<String> {
    carry.push_str(chunk);
    let Some(end) = carry.rfind('\n') else { return Vec::new() };
    let done: String = carry.drain(..=end).collect();
    done.lines().map(str::to_string).filter(|l| !l.trim().is_empty()).collect()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Done {
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub note: String,
}

/// The page's answer written (pure apart from the disk), never outside `results/`.
pub fn write_result(folder: &Path, done: &Done) -> Result<(), String> {
    if !plain_id(&done.id) {
        return Err("That action id is not plain.".into());
    }
    let results = folder.join("results");
    std::fs::create_dir_all(&results).map_err(|_| "The results folder could not be made.".to_string())?;
    let note: String = done.note.chars().filter(|c| !c.is_control() || *c == '\n').take(NOTE_CAP).collect();
    let body = serde_json::to_vec_pretty(&json!({ "id": done.id, "ok": done.ok, "note": note })).map_err(|_| "The answer could not be written.".to_string())?;
    let tmp = results.join(format!("{}.tmp", done.id));
    std::fs::write(&tmp, body).map_err(|_| "The answer could not be written.".to_string())?;
    std::fs::rename(&tmp, results.join(format!("{}.json", done.id))).map_err(|_| "The answer could not be kept.".to_string())
}

/// The page's state written whole (pure apart from the disk): an object, at most 4 MB, swapped in at once so the
/// door never reads half a file.
pub fn write_state(folder: &Path, state: &Value) -> Result<(), String> {
    if !state.is_object() {
        return Err("The studio's state is an object.".into());
    }
    let body = serde_json::to_vec_pretty(state).map_err(|_| "The studio's state could not be written.".to_string())?;
    if body.len() > STATE_CAP {
        return Err("The studio's state is too big to write.".into());
    }
    std::fs::create_dir_all(folder).map_err(|_| "The studio's folder could not be made.".to_string())?;
    let tmp = folder.join("state.json.tmp");
    std::fs::write(&tmp, body).map_err(|_| "The studio's state could not be written.".to_string())?;
    std::fs::rename(&tmp, folder.join("state.json")).map_err(|_| "The studio's state could not be kept.".to_string())
}

#[tauri::command]
pub fn music_state_write(state: Value, store: State<SettingsStore>) -> Result<(), String> {
    crate::rooms::need(&store, ROOM)?;
    write_state(&folder_of(&memory_of(&store)?), &state)
}

#[tauri::command]
pub fn music_action_done(result: Done, store: State<SettingsStore>) -> Result<(), String> {
    crate::rooms::need(&store, ROOM)?;
    write_result(&folder_of(&memory_of(&store)?), &result)
}

/// The kinds of seed the studio takes, by extension.
const SEED_KINDS: [(&str, &[&str]); 4] = [
    ("picture", &["png", "jpg", "jpeg", "webp"]),
    ("audio", &["mp3", "wav", "m4a", "flac", "ogg"]),
    ("midi", &["mid", "midi"]),
    ("video", &["mp4", "mov", "webm"]),
];
const SEED_CAP: u64 = 200 * 1024 * 1024;

/// A dropped or pasted seed copied into the room's `seeds/` (pure apart from the disk), so the companion and the song
/// service read the studio's own copy and never the member's folders: a known kind, at most 200 MB, a plain unique name.
pub fn copy_seed(folder: &Path, source: &Path) -> Result<(String, PathBuf), String> {
    let ext = source.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let kind = SEED_KINDS.iter().find(|(_, exts)| exts.contains(&ext.as_str())).map(|(k, _)| *k).ok_or("That file is not a picture, audio, MIDI or video seed.")?;
    let size = std::fs::metadata(source).map_err(|_| "That seed could not be found.".to_string())?.len();
    if size > SEED_CAP {
        return Err("That seed is too big (over 200 MB).".into());
    }
    let seeds = folder.join("seeds");
    if source.parent().is_some_and(|p| p == seeds) {
        return Ok((kind.into(), source.to_path_buf()));
    }
    std::fs::create_dir_all(&seeds).map_err(|_| "The seeds folder could not be made.".to_string())?;
    let stem: String = source.file_stem().and_then(|s| s.to_str()).unwrap_or("seed").chars().filter(|c| c.is_alphanumeric() || " -_".contains(*c)).take(60).collect();
    let stem = if stem.trim().is_empty() { "seed".to_string() } else { stem.trim().to_string() };
    let mut target = seeds.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while target.exists() {
        target = seeds.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    std::fs::copy(source, &target).map_err(|_| "The seed could not be copied.".to_string())?;
    Ok((kind.into(), target))
}

#[tauri::command]
pub fn music_seed_copy(path: String, store: State<SettingsStore>) -> Result<Value, String> {
    crate::rooms::need(&store, ROOM)?;
    let (kind, copied) = copy_seed(&folder_of(&memory_of(&store)?), Path::new(path.trim()))?;
    Ok(json!({ "kind": kind, "path": copied.display().to_string() }))
}

/// The follower's own memory: where it is in the file, the half line, and the actions still waiting for the page.
pub struct Follower {
    folder: PathBuf,
    offset: u64,
    carry: String,
    waiting: HashMap<String, Instant>,
}

impl Follower {
    /// Starts at the file's end: an action written before this Castle opened is never run again.
    pub fn new(folder: PathBuf) -> Self {
        let offset = std::fs::metadata(folder.join("actions.jsonl")).map(|m| m.len()).unwrap_or(0);
        Self { folder, offset, carry: String::new(), waiting: HashMap::new() }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// One look at the file: the new actions for the page (a broken line with an id is answered here).
    pub fn poll(&mut self) -> Vec<Action> {
        let path = self.folder.join("actions.jsonl");
        let Ok(mut file) = std::fs::File::open(&path) else { return Vec::new() };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // the file was emptied or replaced: read it from the top
            self.offset = 0;
            self.carry.clear();
        }
        if len == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        if file.take(len - self.offset).read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        let mut out = Vec::new();
        for line in split_lines(&mut self.carry, &String::from_utf8_lossy(&bytes)) {
            match parse_line(&line) {
                Some(Ok(action)) => {
                    self.waiting.insert(action.id.clone(), Instant::now());
                    out.push(action);
                }
                Some(Err((id, why))) => {
                    let _ = write_result(&self.folder, &Done { id, ok: false, note: why });
                }
                None => {}
            }
        }
        if self.carry.len() > LINE_CAP {
            self.carry.clear();
        }
        out
    }

    /// Actions the page never answered are answered by Castle, so the door hears why.
    pub fn answer_the_unanswered(&mut self, now: Instant) {
        let results = self.folder.join("results");
        self.waiting.retain(|id, since| {
            if results.join(format!("{id}.json")).is_file() {
                return false;
            }
            if now.duration_since(*since) < ANSWER_WAIT {
                return true;
            }
            let note = "MUSIC STUDIO is not open in Castle, so nothing was done. Ask your person to open the MUSIC STUDIO room.";
            let _ = write_result(&self.folder, &Done { id: id.clone(), ok: false, note: note.into() });
            false
        });
    }

    /// The actions read while the room is switched off: each answered at once, never handed on.
    pub fn refuse(&mut self, actions: Vec<Action>, why: &str) {
        for action in actions {
            self.waiting.remove(&action.id);
            let _ = write_result(&self.folder, &Done { id: action.id, ok: false, note: why.into() });
        }
    }
}

/// Answers older than a day go (the door read them long ago).
pub fn clear_old_results(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder.join("results")) else { return };
    for entry in entries.flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > RESULTS_KEPT);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The follower's thread, started once at launch. It follows the memory folder the settings name
/// now (a folder chosen again starts a fresh follower at that file's end).
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("music-studio".into()).spawn(move || {
        let mut follower: Option<Follower> = None;
        loop {
            std::thread::sleep(TICK);
            let store = app.state::<SettingsStore>();
            let Some(memory) = store.get().memory_folder.map(PathBuf::from) else {
                follower = None;
                continue;
            };
            let folder = folder_of(&memory);
            if follower.as_ref().is_none_or(|f| f.folder() != folder) {
                clear_old_results(&folder);
                follower = Some(Follower::new(folder));
            }
            let Some(f) = follower.as_mut() else { continue };
            let actions = f.poll();
            if store.get().room_on(ROOM) {
                for action in actions {
                    let _ = app.emit(ACTION_EVENT, &action);
                }
            } else {
                f.refuse(actions, &crate::rooms::off_line(ROOM));
            }
            f.answer_the_unanswered(Instant::now());
        }
    });
}

// ---------------------------------------------------------------------------------------------------
// The door: `castle-memory music look` and `castle-memory music act <name> <json>`
// ---------------------------------------------------------------------------------------------------

/// The line `act` appends (pure): one action, its values an object, on one line.
pub fn action_line(id: &str, name: &str, values: &str) -> Result<String, String> {
    if !plain_name(name) {
        return Err(format!("`{name}` is not an action name (lower letters, '.' and '_', like stem.volume)."));
    }
    let values = values.trim().trim_start_matches('\u{feff}');
    let values: Value = if values.is_empty() { json!({}) } else { serde_json::from_str(values).map_err(|_| "The values are not JSON. Write them like {\"stem\":\"drums\",\"value\":0.6}, or put them in a file and pass --file <path.json>.".to_string())? };
    if !values.is_object() {
        return Err("The values are a JSON object, like {\"step\":\"fix\"}.".into());
    }
    Ok(json!({ "id": id, "name": name, "values": values }).to_string())
}

/// Runs the music words for the door (everything after `castle-memory`); the text to print.
pub fn door_cli(ask: &crate::door::Ask, memory: &Path) -> Result<String, String> {
    let folder = folder_of(memory);
    match ask.words.get(1).map(String::as_str) {
        Some("look") => match std::fs::read_to_string(folder.join("state.json")) {
            Ok(text) => Ok(text),
            Err(_) => Err("MUSIC STUDIO has not written its state yet. Ask your person to open the MUSIC STUDIO room.".into()),
        },
        Some("act") => {
            let name = ask.words.get(2).ok_or("Name the action: castle-memory music act <name> <json values>.")?;
            let values = match ask.text()? {
                Some(text) => text,
                None => ask.words.get(3..).map(|rest| rest.join(" ")).unwrap_or_default(),
            };
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
            let id = format!("d{}-{nanos}", std::process::id());
            let line = action_line(&id, name, &values)?;
            std::fs::create_dir_all(&folder).map_err(|_| "The music studio folder could not be made.".to_string())?;
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(folder.join("actions.jsonl")).map_err(|_| "The actions file could not be opened.".to_string())?;
            file.write_all(format!("{line}\n").as_bytes()).map_err(|_| "The action could not be written.".to_string())?;
            drop(file);
            wait_for_answer(&folder, &id, DOOR_WAIT)
        }
        _ => Err("Music words: look (the studio's state) and act <name> <json values> (one action; --file <path.json> for long values).".into()),
    }
}

/// Waits for `results/<id>.json` and prints it; a plain refusal when the page said no.
fn wait_for_answer(folder: &Path, id: &str, wait: Duration) -> Result<String, String> {
    let path = folder.join("results").join(format!("{id}.json"));
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                let note = value["note"].as_str().unwrap_or_default().to_string();
                return if value["ok"] == Value::Bool(true) { Ok(format!("MUSIC ok: {note}")) } else { Err(note) };
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("No answer came from MUSIC STUDIO in 10 seconds. Castle may be closed; ask your person to open the MUSIC STUDIO room.".into())
}

// ---------------------------------------------------------------------------------------------------
// The album shelf: <memory>/music/<album>/
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Take {
    pub take: u32,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub number: u32,
    /// the file names' shared part: `<NN>-<slug>.mp3`, `<slug>.song.json`
    pub slug: String,
    pub title: String,
    pub style: String,
    pub lyrics: String,
    pub negative: String,
    pub approved: Vec<u32>,
    pub takes: Vec<Take>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub folder: String,
    pub title: String,
    pub cover: Option<String>,
    pub tracks: Vec<Track>,
}

const TAKE_KINDS: [&str; 4] = ["mp3", "m4a", "ogg", "wav"];
const PICTURES: [&str; 3] = ["jpg", "png", "webp"];

/// `01-first-rain` -> (1, "first-rain"); a name without a number is take 1 of itself. Pure.
pub fn take_of(stem: &str) -> (u32, String) {
    if let Some((n, rest)) = stem.split_once('-') {
        if !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
            return (n.parse().unwrap_or(1), rest.to_string());
        }
    }
    (1, stem.to_string())
}

/// "first-rain" -> "First Rain". Pure.
pub fn title_of(slug: &str) -> String {
    slug.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn read_json(path: &Path) -> Value {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null)
}

fn modified(path: &Path) -> u64 {
    path.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

/// One album folder read: every take (`<NN>-<slug>.<mp3|m4a|ogg|wav>`, an mp3 before a WAV of the same
/// take) grouped into tracks, each track's words from `<slug>.song.json`, the cover from `cover.*` or
/// the first take's picture. Tracks keep the number their song.json gives, else come in the order
/// they were first sung. None for a folder with no take and no song.json.
pub fn album_of(dir: &Path) -> Option<Album> {
    let folder = dir.file_name()?.to_string_lossy().to_string();
    let mut takes: HashMap<String, HashMap<u32, (usize, PathBuf)>> = HashMap::new();
    let mut words: HashMap<String, Value> = HashMap::new();
    let mut pictures: Vec<(String, PathBuf)> = Vec::new();
    for path in std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_file()) {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        if let Some(slug) = lower.strip_suffix(".song.json") {
            words.insert(slug.to_string(), read_json(&path));
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        if let Some(rank) = TAKE_KINDS.iter().position(|k| *k == ext) {
            let (take, slug) = take_of(&stem.to_lowercase());
            let slot = takes.entry(slug).or_default();
            if slot.get(&take).is_none_or(|(r, _)| rank < *r) {
                slot.insert(take, (rank, path));
            }
        } else if PICTURES.contains(&ext.as_str()) && !lower.contains("-previous") {
            pictures.push((stem.to_lowercase(), path));
        }
    }
    if takes.is_empty() && words.is_empty() {
        return None;
    }
    let album_info = read_json(&dir.join("album.json"));
    let mut slugs: Vec<String> = takes.keys().chain(words.keys()).cloned().collect();
    slugs.sort();
    slugs.dedup();
    let first_sung = |slug: &str| takes.get(slug).and_then(|t| t.values().map(|(_, p)| modified(p)).min()).unwrap_or(u64::MAX);
    let given = |slug: &str| words.get(slug).and_then(|w| w["track"].as_u64()).unwrap_or(u64::MAX);
    slugs.sort_by_key(|s| (given(s), first_sung(s), s.clone()));
    let mut tracks = Vec::new();
    for (n, slug) in slugs.iter().enumerate() {
        let w = words.get(slug).cloned().unwrap_or(Value::Null);
        let text = |key: &str| w[key].as_str().unwrap_or_default().to_string();
        let mut list: Vec<Take> = takes.get(slug).map(|t| t.iter().map(|(take, (_, p))| Take { take: *take, path: p.display().to_string() }).collect()).unwrap_or_default();
        list.sort_by_key(|t| t.take);
        let approved = match &w["approved"] {
            Value::Array(a) => a.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect(),
            Value::Number(v) => v.as_u64().map(|v| vec![v as u32]).unwrap_or_default(),
            _ => Vec::new(),
        };
        let title = Some(text("title")).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| title_of(slug));
        tracks.push(Track { number: n as u32 + 1, slug: slug.clone(), title, style: text("style"), lyrics: text("lyrics"), negative: text("negative"), approved, takes: list });
    }
    pictures.sort();
    let cover = pictures
        .iter()
        .find(|(stem, _)| stem == "cover")
        .or_else(|| pictures.iter().find(|(stem, _)| tracks.first().is_some_and(|t| take_of(stem).1 == t.slug)))
        .or_else(|| pictures.first())
        .map(|(_, p)| p.display().to_string());
    let title = album_info["title"].as_str().filter(|t| !t.trim().is_empty()).map(str::to_string).unwrap_or_else(|| title_of(&folder));
    Some(Album { folder, title, cover, tracks })
}

/// Every album on the song shelf, the newest first.
pub fn albums_of(memory: &Path) -> Vec<Album> {
    let Ok(entries) = std::fs::read_dir(memory.join(SONG_SHELF)) else { return Vec::new() };
    let mut found: Vec<(u64, Album)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|dir| album_of(&dir).map(|album| (modified(&dir), album)))
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.folder.cmp(&b.1.folder)));
    found.into_iter().map(|(_, a)| a).collect()
}

#[tauri::command]
pub async fn music_albums(store: State<'_, SettingsStore>) -> Result<Vec<Album>, String> {
    crate::rooms::need(&store, ROOM)?;
    let memory = memory_of(&store)?;
    tauri::async_runtime::spawn_blocking(move || albums_of(&memory)).await.map_err(|_| "The albums could not be read.".to_string())
}

/// A plain folder or slug name (pure): letters, digits, '-', '_' and spaces; no path.
fn plain_part(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 120 && name.chars().all(|c| c.is_alphanumeric() || "-_ ".contains(c)) && name != "." && name != ".."
}

/// The member approves one take: added to the track's `approved` in its song.json (the other
/// words there kept). A take is never approved by a play.
pub fn approve(memory: &Path, album: &str, slug: &str, take: u32) -> Result<(), String> {
    if !plain_part(album) || !plain_part(slug) {
        return Err("That album or song name is not plain.".into());
    }
    let dir = memory.join(SONG_SHELF).join(album);
    if !dir.is_dir() {
        return Err("That album is not on the shelf.".into());
    }
    let file = dir.join(format!("{slug}.song.json"));
    let mut words = match read_json(&file) {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    let mut approved: Vec<u64> = match words.get("approved") {
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_u64()).collect(),
        Some(Value::Number(n)) => n.as_u64().into_iter().collect(),
        _ => Vec::new(),
    };
    if !approved.contains(&(take as u64)) {
        approved.push(take as u64);
        approved.sort();
    }
    words.insert("approved".into(), json!(approved));
    let body = serde_json::to_vec_pretty(&Value::Object(words)).map_err(|_| "The approval could not be written.".to_string())?;
    let tmp = dir.join(format!("{slug}.song.json.tmp"));
    std::fs::write(&tmp, body).map_err(|_| "The approval could not be written.".to_string())?;
    std::fs::rename(&tmp, &file).map_err(|_| "The approval could not be kept.".to_string())
}

#[tauri::command]
pub fn music_approve(album: String, slug: String, take: u32, store: State<SettingsStore>) -> Result<(), String> {
    crate::rooms::need(&store, ROOM)?;
    approve(&memory_of(&store)?, &album, &slug, take)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("castle-music-studio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn music_a_line_becomes_an_action_or_a_plain_refusal() {
        let a = parse_line(r#"{"id":"a-1","name":"stem.volume","values":{"stem":"drums","value":0.6}}"#).unwrap().unwrap();
        assert_eq!((a.id.as_str(), a.name.as_str(), a.by), ("a-1", "stem.volume", "companion"));
        assert_eq!(a.values["value"], 0.6);
        assert_eq!(parse_line(r#"{"id":"b2","name":"pause"}"#).unwrap().unwrap().values, json!({}));
        assert!(matches!(parse_line(r#"{"id":"c3","name":"Rm -rf"}"#), Some(Err(_))));
        assert!(matches!(parse_line(r#"{"id":"c4","name":"play","values":[1]}"#), Some(Err(_))));
        assert!(parse_line(r#"{"id":"../x","name":"play"}"#).is_none(), "an id that is not plain is never answered into a path");
        assert!(parse_line("not json").is_none());
    }

    #[test]
    fn music_half_a_line_waits_for_the_rest() {
        let mut carry = String::new();
        assert!(split_lines(&mut carry, "{\"id\":\"a\"").is_empty());
        assert_eq!(split_lines(&mut carry, ",\"name\":\"pause\"}\n{\"id\":\"b\"").len(), 1);
        assert_eq!(carry, "{\"id\":\"b\"");
    }

    #[test]
    fn music_the_follower_reads_only_new_lines_and_answers_for_a_closed_page() {
        let root = temp("follow");
        std::fs::write(root.join("actions.jsonl"), "{\"id\":\"old\",\"name\":\"pause\"}\n").unwrap();
        let mut f = Follower::new(root.clone());
        assert!(f.poll().is_empty(), "a line from before Castle opened is never run");
        let mut more = std::fs::OpenOptions::new().append(true).open(root.join("actions.jsonl")).unwrap();
        more.write_all(b"{\"id\":\"n1\",\"name\":\"step.set\",\"values\":{\"step\":\"fix\"}}\n{\"id\":\"n2\",\"name\":\"BAD\"}\n").unwrap();
        let got = f.poll();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].values["step"], "fix");
        let bad: Value = serde_json::from_slice(&std::fs::read(root.join("results").join("n2.json")).unwrap()).unwrap();
        assert_eq!(bad["ok"], false);
        write_result(&root, &Done { id: "n1".into(), ok: true, note: "Moved to FIX.".into() }).unwrap();
        f.answer_the_unanswered(Instant::now() + ANSWER_WAIT * 2);
        let kept: Value = serde_json::from_slice(&std::fs::read(root.join("results").join("n1.json")).unwrap()).unwrap();
        assert_eq!(kept["note"], "Moved to FIX.", "the page's own answer is never overwritten");
        more.write_all(b"{\"id\":\"n3\",\"name\":\"play\"}\n").unwrap();
        assert_eq!(f.poll().len(), 1);
        f.answer_the_unanswered(Instant::now() + ANSWER_WAIT * 2);
        let closed: Value = serde_json::from_slice(&std::fs::read(root.join("results").join("n3.json")).unwrap()).unwrap();
        assert!(closed["note"].as_str().unwrap().contains("not open"));
        // a room switched off answers at once
        more.write_all(b"{\"id\":\"n4\",\"name\":\"play\"}\n").unwrap();
        let read = f.poll();
        f.refuse(read, "The MUSIC room is turned off in Settings.");
        let off: Value = serde_json::from_slice(&std::fs::read(root.join("results").join("n4.json")).unwrap()).unwrap();
        assert!(off["note"].as_str().unwrap().contains("turned off"));
        drop(more);
        std::fs::write(root.join("actions.jsonl"), "{\"id\":\"r1\",\"name\":\"pause\"}\n").unwrap();
        assert_eq!(f.poll().len(), 1, "an emptied file is read from the top");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn music_a_seed_is_copied_into_the_room_with_a_plain_unique_name() {
        let root = temp("seed");
        let mine = root.join("their folder");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(mine.join("Rain at night!.PNG"), b"png").unwrap();
        let (kind, first) = copy_seed(&root, &mine.join("Rain at night!.PNG")).unwrap();
        assert_eq!((kind.as_str(), first.clone()), ("picture", root.join("seeds").join("Rain at night.png")));
        let (_, second) = copy_seed(&root, &mine.join("Rain at night!.PNG")).unwrap();
        assert_eq!(second, root.join("seeds").join("Rain at night-2.png"));
        assert_eq!(copy_seed(&root, &first).unwrap().1, first, "a seed already in the room is not copied again");
        std::fs::write(mine.join("notes.txt"), b"x").unwrap();
        assert!(copy_seed(&root, &mine.join("notes.txt")).is_err());
        assert!(copy_seed(&root, &mine.join("missing.mp3")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn music_the_state_and_answers_stay_inside_their_folder() {
        let root = temp("state");
        write_state(&root, &json!({ "step": "seed" })).unwrap();
        assert!(root.join("state.json").is_file() && !root.join("state.json.tmp").exists());
        assert!(write_state(&root, &json!([1])).is_err());
        assert!(write_result(&root, &Done { id: "..\\evil".into(), ok: true, note: String::new() }).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn music_the_door_writes_one_plain_line_and_hears_the_answer() {
        let line = action_line("d1-2", "stem.volume", r#"{"stem":"drums","value":0.6}"#).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!((v["id"].as_str(), v["name"].as_str(), v["values"]["value"].as_f64()), (Some("d1-2"), Some("stem.volume"), Some(0.6)));
        assert!(!line.contains('\n'));
        assert_eq!(serde_json::from_str::<Value>(&action_line("d1", "pause", "").unwrap()).unwrap()["values"], json!({}));
        assert!(action_line("d1", "Stem Volume", "{}").is_err());
        assert!(action_line("d1", "play", "[1]").is_err());
        assert!(action_line("d1", "play", "{not json").is_err());

        // `act` appends the line, and the answer the page (or Castle) writes is printed
        let memory = temp("door");
        let folder = folder_of(&memory);
        let answer = {
            let folder = folder.clone();
            std::thread::spawn(move || {
                for _ in 0..50 {
                    std::thread::sleep(Duration::from_millis(100));
                    let Ok(text) = std::fs::read_to_string(folder.join("actions.jsonl")) else { continue };
                    let Some(Ok(action)) = text.lines().next().and_then(parse_line) else { continue };
                    write_result(&folder, &Done { id: action.id, ok: true, note: "Step: fix".into() }).unwrap();
                    return;
                }
            })
        };
        let ask = crate::door::parse(&["music".into(), "act".into(), "step.set".into(), r#"{"step":"fix"}"#.into()]).unwrap();
        assert_eq!(door_cli(&ask, &memory).unwrap(), "MUSIC ok: Step: fix");
        answer.join().unwrap();
        // `look` prints the state the page wrote
        assert!(door_cli(&crate::door::parse(&["music".into(), "look".into()]).unwrap(), &memory).is_err(), "no state yet");
        write_state(&folder, &json!({ "step": "fix" })).unwrap();
        assert!(door_cli(&crate::door::parse(&["music".into(), "look".into()]).unwrap(), &memory).unwrap().contains("\"fix\""));
        assert!(door_cli(&crate::door::parse(&["music".into(), "dance".into()]).unwrap(), &memory).is_err());
        let _ = std::fs::remove_dir_all(&memory);
    }

    #[test]
    fn music_the_shelf_reads_albums_tracks_and_takes() {
        let memory = temp("shelf");
        let album = memory.join(SONG_SHELF).join("first-rain");
        std::fs::create_dir_all(&album).unwrap();
        for name in ["01-under-rain.mp3", "02-under-rain.mp3", "02-under-rain.wav", "01-under-rain.jpg", "01-night-bus.mp3", "loose.ogg", "notes.txt"] {
            std::fs::write(album.join(name), b"x").unwrap();
        }
        std::fs::write(album.join("under-rain.song.json"), r#"{"title":"Under the Rain","style":"city pop","lyrics":"[Verse]\nla","track":1,"approved":[2]}"#).unwrap();
        std::fs::create_dir_all(memory.join(SONG_SHELF).join("empty")).unwrap();
        let albums = albums_of(&memory);
        assert_eq!(albums.len(), 1, "a folder with nothing sung is not an album");
        let a = &albums[0];
        assert_eq!((a.folder.as_str(), a.title.as_str()), ("first-rain", "First Rain"));
        assert!(a.cover.as_deref().unwrap().ends_with("01-under-rain.jpg"));
        assert_eq!(a.tracks[0].slug, "under-rain", "the number song.json gives comes first");
        assert_eq!((a.tracks[0].number, a.tracks[0].title.as_str(), a.tracks[0].approved.clone()), (1, "Under the Rain", vec![2]));
        assert_eq!(a.tracks[0].takes.len(), 2);
        assert!(a.tracks[0].takes[1].path.ends_with("02-under-rain.mp3"), "the mp3 before a WAV of the same take");
        let slugs: Vec<&str> = a.tracks.iter().map(|t| t.slug.as_str()).collect();
        assert!(slugs.contains(&"night-bus") && slugs.contains(&"loose"));
        assert_eq!(a.tracks.iter().find(|t| t.slug == "night-bus").unwrap().title, "Night Bus");
        assert_eq!(take_of("loose"), (1, "loose".into()));
        assert_eq!(take_of("12-a-b"), (12, "a-b".into()));

        // approving keeps the other words and never leaves the shelf
        approve(&memory, "first-rain", "night-bus", 1).unwrap();
        approve(&memory, "first-rain", "under-rain", 1).unwrap();
        let words = read_json(&album.join("under-rain.song.json"));
        assert_eq!((words["approved"].clone(), words["style"].as_str()), (json!([1, 2]), Some("city pop")));
        assert!(approve(&memory, "../x", "night-bus", 1).is_err());
        assert!(approve(&memory, "first-rain", "..\\up", 1).is_err());
        assert!(approve(&memory, "missing", "night-bus", 1).is_err());
        let _ = std::fs::remove_dir_all(&memory);
    }

    #[test]
    fn music_the_rules_go_between_their_markers_with_the_names() {
        assert!(RULES.starts_with(BEGIN) && RULES.trim_end().ends_with(END));
        assert!(RULES.contains("{{AI_NAME}}") && RULES.contains("{{USER_NAME}}"));
        assert!(RULES.contains("castle-memory.cmd\" music look") && RULES.contains("castle-memory.cmd\" music act"));
        assert!(RULES.contains("SUNOAPI_ORG_KEY") && RULES.contains("studio <name>"));
        for paid in ["sing", "resing", "extend", "cover", "stems", "sound", "timing", "wav", "cover_art", "video", "style_expand"] {
            assert!(RULES.contains(&format!("`{paid}`")), "the rules teach the paid step {paid}");
        }
        let mine = "# My notes\n\nkeep this";
        let rules = "<!-- castle:music:begin -->\nnew\n<!-- castle:music:end -->";
        assert_eq!(with_rules(mine, rules), format!("{mine}\n\n{rules}\n"));
        let old = format!("{mine}\n\n<!-- castle:music:begin -->\nold\n<!-- castle:music:end -->\nafter");
        assert_eq!(with_rules(&old, rules), format!("{mine}\n\n{rules}\nafter"));

        let memory = temp("prepare");
        let folder = prepare(&memory, "Lyra", "Rin").unwrap();
        assert_eq!(folder, memory.join(MUSIC_FOLDER));
        assert!(memory.join(SONG_SHELF).is_dir() && folder.join("results").is_dir());
        for name in crate::setup::INSTRUCTION_FILES {
            let text = std::fs::read_to_string(folder.join(name)).unwrap();
            assert!(text.contains("You are Lyra") && text.contains("Rin") && !text.contains("{{"));
        }
        let _ = std::fs::remove_dir_all(&memory);
    }
}
