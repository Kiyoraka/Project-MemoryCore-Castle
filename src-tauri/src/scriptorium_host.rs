//! SCRIPTORIUM's host edge: the only place the Castle meets our own office suite. Everything of the
//! suite lives in the `scriptorium` crate (Rust) and `src/scriptorium/` (Vue) and knows nothing of the
//! Castle; this file hands the page the crate's display lists, its command bus and its undo, and gives
//! the room's companion the AI's door.
//!
//! - The open documents live in [`ScriptoriumState`], each behind its own [`Bus`]. M0 has one kind,
//!   [`BlankDocument`] (one A4 page, `blank.note`), which proves the bus, undo and the door end to end;
//!   real files (.docx, .xlsx, .pptx, .pdf) open in the next milestone.
//! - The companion's door is `castle-memory scriptorium describe|read|do|shot`. The door runs in its
//!   own short CLI process without the app's state, so it speaks to the running Castle through a small
//!   file inbox in the room's folder:
//!
//!   ```text
//!   <memory>/scriptorium/door/requests/<id>.json    the CLI writes {id, verb, args, doc?} (via .part + rename)
//!   <memory>/scriptorium/door/responses/<id>.json   the app answers with the bus's door JSON
//!   <memory>/scriptorium/door/alive                 touched by the app while the room is open
//!   ```
//!
//!   The app reads the inbox every 300 ms while the room is open (started by `scriptorium_prepare`,
//!   stopped by `scriptorium_close`, the room's switch or Castle leaving); the CLI waits up to 10 s for
//!   its answer. A request older than 30 s is dropped unanswered (its CLI gave up long ago).
//! - The room's session (`Room::Scriptorium`, `<memory>/scriptorium`) hears the room's rules from
//!   `resources/scriptorium-rules.md`, written into its CLAUDE.md and AGENTS.md between markers.

use crate::settings::SettingsStore;
use scriptorium::ground::bus::json::{self, Json};
use scriptorium::ground::bus::{Bus, BusError, Change, Command, CommandSpec, Document, Origin, DOOR_VERBS};
use scriptorium::ground::display::{Color, DisplayList, Item, Paint, PathOp, Resources, Stroke};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

/// The room's id (Settings > Rooms, the launcher, `rooms::need`).
pub const ROOM: &str = "scriptorium";
/// The editors the page can ask for. In M0 each opens a blank page.
pub const EDITORS: [&str; 4] = ["scroll", "ledger", "stage", "folio"];
/// The event the page hears when the companion changed a document through the door.
pub const CHANGED_EVENT: &str = "scriptorium-changed";
/// What `scriptorium_open` and `scriptorium_save` answer until files arrive.
pub const NEXT_MILESTONE: &str = "Opening .docx/.xlsx/.pptx/.pdf arrives in the next milestone.";
pub const SAVE_NEXT_MILESTONE: &str = "Saving .docx/.xlsx/.pptx/.pdf arrives in the next milestone.";
/// How many recent files are remembered.
pub const RECENT_MAX: usize = 20;
const RECENT_FILE: &str = "scriptorium-recent.json";

const POLL: Duration = Duration::from_millis(300);
/// How long the CLI waits for the room's answer.
pub const DOOR_TIMEOUT: Duration = Duration::from_secs(10);
/// A request nobody answered for this long is dropped.
const STALE: Duration = Duration::from_secs(30);
/// The room's heartbeat is fresh for this long.
const ALIVE_FOR: Duration = Duration::from_secs(5);
const ALIVE_EVERY: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------------------------------
// The blank document (M0)
// ---------------------------------------------------------------------------------------------------

/// A4 in points.
pub const A4: (f64, f64) = (595.0, 842.0);
const MARGIN: f64 = 72.0;
const MAX_NOTES: usize = 500;
const MAX_NOTE_CHARS: usize = 2000;

/// One white A4 page that keeps a list of notes: just enough to prove the bus, undo and the door.
/// Each note is drawn as a grey bar as long as its text (the page has no fonts yet).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct BlankDocument {
    pub notes: Vec<String>,
}

fn note_text(cmd: &Command) -> Result<String, BusError> {
    let text = cmd.args.get("text").and_then(Json::as_str).ok_or_else(|| BusError::bad_args(&cmd.name, "\"text\" is a string"))?;
    let text = text.trim();
    if text.is_empty() {
        return Err(BusError::bad_args(&cmd.name, "\"text\" is empty"));
    }
    Ok(text.chars().take(MAX_NOTE_CHARS).collect())
}

impl Document for BlankDocument {
    fn kind(&self) -> &'static str {
        "blank"
    }

    fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
        match cmd.name.as_str() {
            "blank.note" => {
                let text = note_text(cmd)?;
                let at = match cmd.args.get("at") {
                    None | Some(Json::Null) => self.notes.len(),
                    Some(v) => v.as_usize().ok_or_else(|| BusError::bad_args(&cmd.name, "\"at\" is a whole number from 0"))?,
                };
                if at > self.notes.len() {
                    return Err(BusError::failed(format!("no place {at} (the page has {} notes)", self.notes.len())));
                }
                if self.notes.len() >= MAX_NOTES {
                    return Err(BusError::failed(format!("the page holds at most {MAX_NOTES} notes")));
                }
                self.notes.insert(at, text);
                Ok(Change { undo: vec![Command::new("blank.remove_note", Json::obj().with("at", at))], pages_touched: vec![0] })
            }
            "blank.remove_note" => {
                let at = cmd.args.get("at").and_then(Json::as_usize).ok_or_else(|| BusError::bad_args(&cmd.name, "\"at\" is a whole number from 0"))?;
                if at >= self.notes.len() {
                    return Err(BusError::failed(format!("no note {at} (the page has {} notes)", self.notes.len())));
                }
                let text = self.notes.remove(at);
                Ok(Change { undo: vec![Command::new("blank.note", Json::obj().with("at", at).with("text", text))], pages_touched: vec![0] })
            }
            other => Err(BusError::unknown(other)),
        }
    }

    fn describe(&self) -> Json {
        Json::obj()
            .with("page", Json::obj().with("width", A4.0).with("height", A4.1).with("unit", "pt"))
            .with("notes", self.notes.len())
            .with("read", "{} for every note, or {\"note\": n} for one (from 0)")
    }

    fn read(&self, what: &Json) -> Result<Json, BusError> {
        match what.get("note") {
            None | Some(Json::Null) => Ok(Json::obj().with("notes", Json::Arr(self.notes.iter().map(Json::from).collect()))),
            Some(n) => {
                let i = n.as_usize().ok_or_else(|| BusError::bad_args("", "\"note\" is a whole number from 0"))?;
                self.notes.get(i).map(|t| Json::obj().with("note", i).with("text", t.as_str())).ok_or_else(|| BusError::failed(format!("no note {i}")))
            }
        }
    }

    fn display(&self, _page: usize) -> DisplayList {
        let (w, h) = A4;
        let rect = |x: f64, y: f64, w: f64, h: f64| vec![PathOp::MoveTo(x, y), PathOp::LineTo(x + w, y), PathOp::LineTo(x + w, y + h), PathOp::LineTo(x, y + h), PathOp::Close];
        let mut dl = DisplayList::new(w, h);
        dl.push(Item::Path { ops: rect(0.0, 0.0, w, h), fill: Some(Paint::Solid(Color::WHITE)), stroke: None });
        // the margin: a thin line where the text will stand
        dl.push(Item::Path { ops: rect(MARGIN, MARGIN, w - 2.0 * MARGIN, h - 2.0 * MARGIN), fill: None, stroke: Some(Stroke::new(0.5, Color::rgb(0xd8, 0xd4, 0xcc))) });
        let line = 18.0;
        let room = w - 2.0 * MARGIN;
        for (i, note) in self.notes.iter().enumerate() {
            let y = MARGIN + 12.0 + i as f64 * line;
            if y + 8.0 > h - MARGIN {
                break;
            }
            let len = (note.chars().count() as f64 * 5.5).clamp(18.0, room);
            dl.push(Item::Path { ops: rect(MARGIN + 6.0, y, len - 12.0, 8.0), fill: Some(Paint::Solid(Color::rgb(0x9a, 0x94, 0x8a))), stroke: None });
        }
        dl
    }

    fn page_count(&self) -> usize {
        1
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![
            CommandSpec::new("blank.note", "Write a note on the page (at the end, or before note `at`)", r#"{ "text": string, "at"?: n }"#),
            CommandSpec::new("blank.remove_note", "Take note `at` off the page", r#"{ "at": n }"#),
        ]
    }
}

/// Any document behind one bus type, so the state can hold every editor's documents together.
pub struct AnyDoc(pub Box<dyn Document + Send>);

impl Document for AnyDoc {
    fn kind(&self) -> &'static str {
        self.0.kind()
    }
    fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
        self.0.apply(cmd)
    }
    fn describe(&self) -> Json {
        self.0.describe()
    }
    fn read(&self, what: &Json) -> Result<Json, BusError> {
        self.0.read(what)
    }
    fn display(&self, page: usize) -> DisplayList {
        self.0.display(page)
    }
    fn page_count(&self) -> usize {
        self.0.page_count()
    }
    fn commands(&self) -> Vec<CommandSpec> {
        self.0.commands()
    }
    fn resources(&self) -> Option<&Resources> {
        self.0.resources()
    }
}

// ---------------------------------------------------------------------------------------------------
// The open documents
// ---------------------------------------------------------------------------------------------------

pub struct OpenDoc {
    /// The editor the page shows it in: scroll, ledger, stage or folio.
    pub editor: String,
    pub bus: Bus<AnyDoc>,
}

/// The documents open in the room, by id. `current` is the one on the page (the door's default).
#[derive(Default)]
pub struct Docs {
    next: u64,
    open: BTreeMap<u64, OpenDoc>,
    current: Option<u64>,
}

fn err_json(message: impl Into<String>) -> String {
    Json::obj().with("ok", false).with("error", message.into()).to_string()
}

fn parse_args(text: &str) -> Result<Json, BusError> {
    if text.trim().is_empty() {
        Ok(Json::obj())
    } else {
        json::parse(text).map_err(|e| BusError::bad_args("", e))
    }
}

/// `{"ok":true,"step":n,"pages":[..]}` for a do, undo or redo.
fn done_json(done: &scriptorium::ground::bus::Done) -> String {
    Json::obj().with("ok", true).with("step", done.step).with("pages", Json::Arr(done.pages_touched.iter().map(|p| Json::from(*p)).collect())).to_string()
}

impl Docs {
    /// A new blank document for `editor` (M0: every editor's page is blank); it becomes the current one.
    pub fn new_doc(&mut self, editor: &str) -> Result<u64, String> {
        let editor = editor.trim().to_lowercase();
        let editor = if editor.is_empty() || editor == "blank" { "scroll".to_string() } else { editor };
        if !EDITORS.contains(&editor.as_str()) {
            return Err(format!("SCRIPTORIUM has no editor called {editor}. Its editors are Scroll, Ledger, Stage and Folio."));
        }
        self.next += 1;
        let id = self.next;
        self.open.insert(id, OpenDoc { editor, bus: Bus::new(AnyDoc(Box::new(BlankDocument::default()))) });
        self.current = Some(id);
        Ok(id)
    }

    pub fn get(&mut self, id: u64) -> Result<&mut OpenDoc, String> {
        self.open.get_mut(&id).ok_or_else(|| "That document is not open any more.".to_string())
    }

    /// The page shows this one now: the door's default.
    pub fn show(&mut self, id: u64) -> Result<&mut OpenDoc, String> {
        if !self.open.contains_key(&id) {
            return Err("That document is not open any more.".into());
        }
        self.current = Some(id);
        self.get(id)
    }

    pub fn close_all(&mut self) {
        self.open.clear();
        self.current = None;
    }

    /// The member's own command (a button, a shortcut): one undo step with `Origin::User`.
    pub fn command(&mut self, id: u64, name: &str, args_text: &str) -> Result<String, String> {
        let open = self.get(id)?;
        let args = parse_args(args_text).map_err(|e| e.to_string())?;
        open.bus.do_one(Command::new(name, args), Origin::User).map(|d| done_json(&d)).map_err(|e| e.to_string())
    }

    pub fn history(&mut self, id: u64) -> Result<String, String> {
        Ok(Json::Arr(self.get(id)?.bus.history().iter().map(|h| h.to_json()).collect()).to_string())
    }

    /// The AI's door on document `doc` (or the one on the page). Always answers JSON text (errors as
    /// `{"ok":false,"error"}`); the second value is the document a `do` changed.
    pub fn door(&mut self, doc: Option<u64>, verb: &str, args_text: &str) -> (String, Option<u64>) {
        let Some(id) = doc.or(self.current) else {
            return (err_json("No document is open in SCRIPTORIUM yet. Ask your person to open one."), None);
        };
        let open = match self.get(id) {
            Ok(open) => open,
            Err(why) => return (err_json(why), None),
        };
        let answer = parse_args(args_text).and_then(|args| open.bus.door(verb, &args));
        match answer {
            Ok(mut value) => {
                if verb == "describe" {
                    value.set("doc", id);
                    value.set("editor", open.editor.as_str());
                }
                (value.to_string(), (verb == "do").then_some(id))
            }
            Err(e) => (err_json(e.to_string()), None),
        }
    }
}

/// The room's state, managed by the app.
#[derive(Default)]
pub struct ScriptoriumState {
    pub docs: Mutex<Docs>,
    /// The inbox reader's stop switch, while it runs.
    inbox: Mutex<Option<Arc<AtomicBool>>>,
}

// ---------------------------------------------------------------------------------------------------
// The door's inbox
// ---------------------------------------------------------------------------------------------------

/// One request the CLI left for the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub verb: String,
    /// The door's args as JSON text.
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub doc: Option<u64>,
}

pub fn door_dir(memory: &Path) -> PathBuf {
    memory.join(crate::session::SCRIPTORIUM_FOLDER).join("door")
}

/// A request id is a plain file name: letters, digits, `-` and `_`, at most 64.
pub fn good_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Writes `text` to `path` whole: a `.part` beside it first, then renamed into place.
fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    let part = path.with_extension("json.part");
    std::fs::write(&part, text)?;
    std::fs::rename(&part, path)
}

/// Answers every waiting request in `dir` with `answer`, writing `responses/<id>.json` and removing
/// the request. Stale and unreadable requests are dropped (an unreadable one with a good name still
/// gets its error back). Returns how many were answered.
pub fn answer_waiting(dir: &Path, now: SystemTime, mut answer: impl FnMut(&Request) -> String) -> usize {
    let Ok(entries) = std::fs::read_dir(dir.join("requests")) else {
        return 0;
    };
    let responses = dir.join("responses");
    let mut answered = 0;
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    for file in files {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).unwrap_or_default();
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let _ = std::fs::remove_file(&file);
        if age > STALE || !good_id(&stem) {
            continue;
        }
        let reply = match serde_json::from_str::<Request>(text.trim_start_matches('\u{feff}')) {
            Ok(req) if req.id == stem => answer(&req),
            _ => err_json("The door could not read that request."),
        };
        if std::fs::create_dir_all(&responses).is_ok() && write_whole(&responses.join(format!("{stem}.json")), &reply).is_ok() {
            answered += 1;
        }
    }
    answered
}

/// Whether the room's heartbeat is fresh (the app is reading the inbox).
pub fn room_alive(dir: &Path, now: SystemTime) -> bool {
    std::fs::metadata(dir.join("alive")).and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age <= ALIVE_FOR)
}

/// The CLI's side: leaves `req` in the inbox and waits up to `timeout` for its answer.
pub fn ask_room(dir: &Path, req: &Request, timeout: Duration) -> Result<String, String> {
    let requests = dir.join("requests");
    std::fs::create_dir_all(&requests).map_err(|_| "The SCRIPTORIUM door's folder could not be made.".to_string())?;
    let text = serde_json::to_string(req).map_err(|_| "The request could not be written.".to_string())?;
    let request = requests.join(format!("{}.json", req.id));
    write_whole(&request, &text).map_err(|_| "The request could not be written.".to_string())?;
    let response = dir.join("responses").join(format!("{}.json", req.id));
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        if let Ok(answer) = std::fs::read_to_string(&response) {
            let _ = std::fs::remove_file(&response);
            return Ok(answer);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_file(&request);
    Err(format!("SCRIPTORIUM did not answer in {} seconds. Is the room open in Castle with a document on its page?", timeout.as_secs()))
}

/// The args JSON text for one door call from the CLI's flags: `--file`/`--body` (or `--args`) as the
/// base, `--label` for a `do` (a bare command list becomes `{"commands":[..]}`), `--page` for a `shot`.
pub fn cli_args(verb: &str, base: Option<&str>, label: Option<&str>, page: Option<&str>) -> Result<String, String> {
    let base = base.map(|t| t.trim_start_matches('\u{feff}').trim()).filter(|t| !t.is_empty());
    let mut value: serde_json::Value = match base {
        Some(text) => serde_json::from_str(text).map_err(|_| "The door's arguments are not JSON. Write them to a file and pass --file <path.json>.".to_string())?,
        None => serde_json::json!({}),
    };
    if verb == "do" && value.is_array() {
        value = serde_json::json!({ "commands": value });
    }
    if let Some(label) = label.map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("label".into(), serde_json::Value::String(label.chars().take(120).collect()));
        }
    }
    if let Some(page) = page {
        let n: u64 = page.trim().parse().map_err(|_| "--page is a whole number from 0.".to_string())?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("page".into(), serde_json::Value::from(n));
        }
    }
    Ok(value.to_string())
}

/// `castle-memory scriptorium <verb>`: one call through the room's inbox; the answer printed.
pub fn door_cli(ask: &crate::door::Ask, memory: &Path) -> Result<String, String> {
    let verb = ask.words.get(1).map(String::as_str).unwrap_or("");
    if !DOOR_VERBS.contains(&verb) {
        return Err("Scriptorium verbs: describe, read, do, shot. Arguments in JSON go through --file <path.json>.".into());
    }
    let dir = door_dir(memory);
    if !room_alive(&dir, SystemTime::now()) {
        return Err("SCRIPTORIUM is not open in Castle right now. Ask your person to open it.".into());
    }
    let base = match ask.text()? {
        Some(text) => Some(text),
        None => ask.get("args").map(str::to_string),
    };
    let doc = match ask.get("doc") {
        Some(d) => Some(d.trim().trim_start_matches('#').parse::<u64>().map_err(|_| "--doc is the number describe gave.".to_string())?),
        None => None,
    };
    let args = cli_args(verb, base.as_deref(), ask.get("label"), ask.get("page"))?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    let req = Request { id: format!("{}-{nanos}", std::process::id()), verb: verb.into(), args, doc };
    let answer = ask_room(&dir, &req, DOOR_TIMEOUT)?;
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&answer) {
        if value.get("ok") == Some(&serde_json::Value::Bool(false)) {
            return Err(value.get("error").and_then(|e| e.as_str()).unwrap_or("The door said no.").to_string());
        }
    }
    Ok(answer)
}

#[derive(Clone, Serialize)]
struct Changed {
    doc: u64,
}

/// Starts reading the inbox (once): every 300 ms while the room is open.
fn start_inbox(app: &AppHandle, memory: PathBuf) {
    let state = app.state::<ScriptoriumState>();
    let mut slot = state.inbox.lock().unwrap();
    if slot.as_ref().is_some_and(|stop| !stop.load(Ordering::SeqCst)) {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    *slot = Some(stop.clone());
    let app = app.clone();
    std::thread::spawn(move || {
        let dir = door_dir(&memory);
        let _ = std::fs::create_dir_all(dir.join("requests"));
        let mut beat = std::time::Instant::now() - ALIVE_EVERY;
        while !stop.load(Ordering::SeqCst) {
            if beat.elapsed() >= ALIVE_EVERY {
                let _ = std::fs::write(dir.join("alive"), b"open");
                beat = std::time::Instant::now();
            }
            let mut changed = Vec::new();
            answer_waiting(&dir, SystemTime::now(), |req| {
                let state = app.state::<ScriptoriumState>();
                let (text, doc) = state.docs.lock().unwrap().door(req.doc, &req.verb, &req.args);
                changed.extend(doc);
                text
            });
            for doc in changed {
                let _ = app.emit(CHANGED_EVENT, Changed { doc });
            }
            std::thread::sleep(POLL);
        }
        let _ = std::fs::remove_file(dir.join("alive"));
    });
}

/// Everything the room runs stops: its session ends, the inbox is no longer read, the documents close.
pub fn stop(app: &AppHandle) {
    app.state::<crate::session::ScriptoriumHost>().0.kill();
    let state = app.state::<ScriptoriumState>();
    if let Some(stop) = state.inbox.lock().unwrap().take() {
        stop.store(true, Ordering::SeqCst);
    }
    state.docs.lock().unwrap().close_all();
}

// ---------------------------------------------------------------------------------------------------
// The room's folder and its rules
// ---------------------------------------------------------------------------------------------------

const RULES: &str = include_str!("../resources/scriptorium-rules.md");
const BEGIN: &str = "<!-- castle:scriptorium:begin";
const END: &str = "<!-- castle:scriptorium:end -->";

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

/// Makes the folder (when missing) and writes the rules with the names, only when they changed.
pub fn prepare(memory: &Path, companion: &str, member: &str) -> Result<PathBuf, String> {
    let folder = memory.join(crate::session::SCRIPTORIUM_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|_| "The scriptorium folder could not be created.".to_string())?;
    let rules = RULES.replace("{{AI_NAME}}", companion.trim()).replace("{{USER_NAME}}", member.trim());
    for name in crate::setup::INSTRUCTION_FILES {
        let path = folder.join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let next = with_rules(&text, &rules);
        if next != text {
            std::fs::write(&path, next).map_err(|_| format!("{name} could not be written in the scriptorium folder."))?;
        }
    }
    Ok(folder)
}

// ---------------------------------------------------------------------------------------------------
// Recent files
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFile {
    pub path: String,
    pub name: String,
    pub editor: String,
    /// seconds since 1970
    pub opened_at: u64,
}

/// The editor a file opens in, by its extension.
pub fn editor_for(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?.to_lowercase();
    Some(match ext.as_str() {
        "docx" => "scroll",
        "xlsx" => "ledger",
        "pptx" => "stage",
        "pdf" => "folio",
        _ => return None,
    })
}

/// `list` with `file` first: once per path (the same file in any case of letters), at most RECENT_MAX. Pure.
pub fn remember(list: Vec<RecentFile>, file: RecentFile) -> Vec<RecentFile> {
    let key = |p: &str| p.replace('/', "\\").to_lowercase();
    let mine = key(&file.path);
    let mut out = vec![file];
    out.extend(list.into_iter().filter(|f| key(&f.path) != mine));
    out.truncate(RECENT_MAX);
    out
}

fn recent_path(store: &SettingsStore) -> PathBuf {
    store.folder().join(RECENT_FILE)
}

fn read_recent(store: &SettingsStore) -> Vec<RecentFile> {
    std::fs::read_to_string(recent_path(store)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn note_recent(store: &SettingsStore, path: &str, editor: &str) {
    let name = Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string();
    let opened_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default();
    let list = remember(read_recent(store), RecentFile { path: path.to_string(), name, editor: editor.to_string(), opened_at });
    if let Ok(text) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(recent_path(store), text);
    }
}

/// Opens a real file as a document. M0: no format opens yet (they arrive in the next milestone).
fn open_file(path: &Path) -> Result<OpenDoc, String> {
    if !path.is_file() {
        return Err("That file is not there any more.".into());
    }
    Err(NEXT_MILESTONE.into())
}

// ---------------------------------------------------------------------------------------------------
// The page's commands
// ---------------------------------------------------------------------------------------------------

type RoomState<'a> = (State<'a, SettingsStore>, State<'a, ScriptoriumState>);

fn docs<'a>(room: &'a RoomState<'_>) -> Result<std::sync::MutexGuard<'a, Docs>, String> {
    crate::rooms::need(&room.0, ROOM)?;
    Ok(room.1.docs.lock().unwrap())
}

/// A new blank page for `kind` (scroll, ledger, stage or folio); its id.
#[tauri::command]
pub fn scriptorium_new(kind: String, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<u64, String> {
    docs(&(store, state))?.new_doc(&kind)
}

/// One page's display list as JSON (`DisplayList::to_json`); the page shows this document now.
#[tauri::command]
pub fn scriptorium_display(doc: u64, page: usize, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let room = (store, state);
    let mut docs = docs(&room)?;
    let open = docs.show(doc)?;
    let count = open.bus.doc().page_count();
    if page >= count {
        return Err(format!("The document has {count} page{}.", if count == 1 { "" } else { "s" }));
    }
    Ok(open.bus.doc().display(page).to_json())
}

/// Glyph outlines for the painter (`Resources::glyphs_json`).
#[tauri::command]
pub fn scriptorium_glyphs(doc: u64, font: u32, gids: Vec<u16>, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let room = (store, state);
    let mut docs = docs(&room)?;
    let open = docs.get(doc)?;
    open.bus.doc().resources().and_then(|r| r.glyphs_json(font, &gids)).ok_or_else(|| "That font is not in this document.".to_string())
}

/// An image's pixels for the painter (`Resources::image_json`).
#[tauri::command]
pub fn scriptorium_image(doc: u64, id: u32, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let room = (store, state);
    let mut docs = docs(&room)?;
    let open = docs.get(doc)?;
    open.bus.doc().resources().and_then(|r| r.image_json(id)).ok_or_else(|| "That picture is not in this document.".to_string())
}

/// The member's command: `{"ok":true,"step","pages"}`.
#[tauri::command]
pub fn scriptorium_command(doc: u64, name: String, args_json: Option<String>, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    docs(&(store, state))?.command(doc, &name, args_json.as_deref().unwrap_or(""))
}

#[tauri::command]
pub fn scriptorium_undo(doc: u64, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let room = (store, state);
    let mut docs = docs(&room)?;
    docs.get(doc)?.bus.undo().map(|d| done_json(&d)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn scriptorium_redo(doc: u64, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let room = (store, state);
    let mut docs = docs(&room)?;
    docs.get(doc)?.bus.redo().map(|d| done_json(&d)).map_err(|e| e.to_string())
}

/// The undo history: `[{"id","origin":"user"|"ai","label","commands","undone"}, ..]`.
#[tauri::command]
pub fn scriptorium_history(doc: u64, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    docs(&(store, state))?.history(doc)
}

/// The door from the page itself (the same four verbs the companion has); answers its JSON.
#[tauri::command]
pub fn scriptorium_door(doc: u64, verb: String, args_json: Option<String>, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    Ok(docs(&(store, state))?.door(Some(doc), &verb, args_json.as_deref().unwrap_or("")).0)
}

/// The member picks a document to open; None when the picker is closed.
#[tauri::command]
pub async fn scriptorium_open_dialog(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Open a document")
            .add_filter("Documents, sheets, slides and PDFs", &["docx", "xlsx", "pptx", "pdf"])
            .blocking_pick_file()
            .and_then(|file| file.into_path().ok())
            .map(|path| path.display().to_string())
    })
    .await
    .ok()
    .flatten()
}

/// The member picks where to save; None when the picker is closed.
#[tauri::command]
pub async fn scriptorium_save_dialog(app: AppHandle, name: Option<String>) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Untitled.docx".into());
        app.dialog()
            .file()
            .set_title("Save the document")
            .set_file_name(&name)
            .add_filter("Documents, sheets, slides and PDFs", &["docx", "xlsx", "pptx", "pdf"])
            .blocking_save_file()
            .and_then(|file| file.into_path().ok())
            .map(|path| path.display().to_string())
    })
    .await
    .ok()
    .flatten()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub doc: u64,
    pub editor: String,
}

/// Opens a file the member chose; it joins the recent files once it opened.
#[tauri::command]
pub fn scriptorium_open(path: String, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<Opened, String> {
    crate::rooms::need(&store, ROOM)?;
    let editor = editor_for(&path).ok_or("SCRIPTORIUM opens .docx, .xlsx, .pptx and .pdf files.")?;
    let open = open_file(Path::new(&path))?;
    let mut docs = state.docs.lock().unwrap();
    docs.next += 1;
    let doc = docs.next;
    let editor_name = open.editor.clone();
    docs.open.insert(doc, open);
    docs.current = Some(doc);
    note_recent(&store, &path, editor);
    Ok(Opened { doc, editor: editor_name })
}

/// Saves a document to a file. M0: not yet.
#[tauri::command]
pub fn scriptorium_save(doc: u64, path: String, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<(), String> {
    let room = (store, state);
    docs(&room)?.get(doc)?;
    editor_for(&path).ok_or("SCRIPTORIUM saves .docx, .xlsx, .pptx and .pdf files.")?;
    Err(SAVE_NEXT_MILESTONE.into())
}

/// The recent files, newest first.
#[tauri::command]
pub fn scriptorium_recent(store: State<SettingsStore>) -> Result<Vec<RecentFile>, String> {
    crate::rooms::need(&store, ROOM)?;
    Ok(read_recent(&store))
}

/// The room opened: its folder and rules ready for the session, and the door's inbox read.
#[tauri::command]
pub fn scriptorium_prepare(app: AppHandle, store: State<SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, ROOM)?;
    let s = store.get();
    let memory = s.memory_folder.clone().map(PathBuf::from).ok_or("No MemoryCore folder is chosen yet.")?;
    let member = s.member_name.clone().unwrap_or_else(|| "the member".into());
    let folder = prepare(&memory, &s.companion_label(), &member)?;
    start_inbox(&app, memory);
    Ok(folder.display().to_string())
}

/// The room closed: its session ends, the inbox stops, the documents close.
#[tauri::command]
pub fn scriptorium_close(app: AppHandle) {
    stop(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("castle-scriptorium-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn note(text: &str) -> Command {
        Command::new("blank.note", Json::obj().with("text", text))
    }

    #[test]
    fn a_blank_page_is_a4_white_with_its_margin() {
        let doc = BlankDocument::default();
        assert_eq!(doc.kind(), "blank");
        assert_eq!(doc.page_count(), 1);
        let dl = doc.display(0);
        assert_eq!((dl.width, dl.height), A4);
        assert_eq!(dl.items.len(), 2, "the white page and the margin");
        assert!(matches!(&dl.items[0], Item::Path { fill: Some(Paint::Solid(c)), .. } if *c == Color::WHITE));
        assert!(matches!(&dl.items[1], Item::Path { fill: None, stroke: Some(s), .. } if s.width == 0.5));
        let text = dl.to_json();
        assert!(text.starts_with("{\"w\":595,\"h\":842,\"items\":[") && text.contains("#ffffff"));
    }

    #[test]
    fn notes_go_through_the_bus_and_undo_takes_them_back() {
        let mut bus = Bus::new(BlankDocument::default());
        bus.do_one(note("first"), Origin::User).unwrap();
        bus.do_one(note("second"), Origin::User).unwrap();
        assert_eq!(bus.doc().notes, vec!["first", "second"]);
        assert_eq!(bus.doc().display(0).items.len(), 4, "each note drawn as a bar");
        bus.undo().unwrap();
        assert_eq!(bus.doc().notes, vec!["first"]);
        bus.redo().unwrap();
        assert_eq!(bus.doc().notes, vec!["first", "second"]);
        bus.do_one(Command::new("blank.remove_note", Json::obj().with("at", 0)), Origin::User).unwrap();
        assert_eq!(bus.doc().notes, vec!["second"]);
        bus.undo().unwrap();
        assert_eq!(bus.doc().notes, vec!["first", "second"], "a removed note comes back in its place");
        assert!(bus.do_one(note("   "), Origin::User).is_err(), "an empty note is refused");
        assert!(bus.do_one(Command::new("blank.remove_note", Json::obj().with("at", 9)), Origin::User).is_err());
        assert!(matches!(bus.do_one(Command::new("blank.erase", Json::obj()), Origin::User), Err(BusError::UnknownCommand { .. })));
    }

    #[test]
    fn the_door_describes_reads_does_and_shoots_and_the_ai_step_is_marked() {
        let mut docs = Docs::default();
        let (none, _) = docs.door(None, "describe", "");
        assert!(none.contains("\"ok\":false") && none.contains("No document is open"));
        let id = docs.new_doc("ledger").unwrap();
        let (described, changed) = docs.door(None, "describe", "");
        assert_eq!(changed, None);
        assert!(described.contains("\"kind\":\"blank\"") && described.contains("blank.note") && described.contains("\"editor\":\"ledger\""));
        assert!(described.contains(&format!("\"doc\":{id}")));
        let (done, changed) = docs.door(None, "do", r#"{"commands":[{"name":"blank.note","args":{"text":"from the AI"}}],"label":"A note"}"#);
        assert_eq!(changed, Some(id));
        assert!(done.contains("\"ok\":true"));
        let (read, _) = docs.door(Some(id), "read", "{}");
        assert_eq!(read, r#"{"notes":["from the AI"]}"#);
        let (shot, _) = docs.door(Some(id), "shot", r#"{"page":0}"#);
        assert!(shot.contains("\"width\":595") && shot.contains("\"display\""));
        let history = docs.history(id).unwrap();
        assert!(history.contains("\"origin\":\"ai\"") && history.contains("\"label\":\"A note\""));
        docs.command(id, "blank.note", r#"{"text":"mine"}"#).unwrap();
        assert!(docs.history(id).unwrap().contains("\"origin\":\"user\""));
        let (refused, changed) = docs.door(Some(id), "do", r#"{"commands":[{"name":"scroll.delete_everything"}]}"#);
        assert!(refused.contains("\"ok\":false") && refused.contains("blank.note") && changed.is_none(), "only the listed commands get through");
        let (bad, _) = docs.door(Some(id), "erase", "");
        assert!(bad.contains("unknown verb"));
        assert!(docs.door(Some(99), "describe", "").0.contains("not open"));
        assert!(docs.new_doc("spreadsheet").is_err());
    }

    #[test]
    fn the_inbox_answers_a_request_the_cli_waits_for() {
        let dir = temp("inbox");
        let answerer = {
            let dir = dir.clone();
            std::thread::spawn(move || {
                let mut docs = Docs::default();
                docs.new_doc("scroll").unwrap();
                for _ in 0..100 {
                    if answer_waiting(&dir, SystemTime::now(), |req| docs.door(req.doc, &req.verb, &req.args).0) > 0 {
                        return docs;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                docs
            })
        };
        let req = Request { id: "test-1".into(), verb: "do".into(), args: cli_args("do", Some(r#"[{"name":"blank.note","args":{"text":"hi"}}]"#), Some("Say hi"), None).unwrap(), doc: None };
        let answer = ask_room(&dir, &req, Duration::from_secs(5)).unwrap();
        assert!(answer.contains("\"ok\":true"), "{answer}");
        let mut docs = answerer.join().unwrap();
        assert!(docs.history(1).unwrap().contains("\"label\":\"Say hi\""));
        assert!(!dir.join("requests").join("test-1.json").exists() && !dir.join("responses").join("test-1.json").exists(), "both files are cleaned up");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_inbox_drops_bad_names_and_stale_requests_and_the_cli_gives_up() {
        let dir = temp("inbox-bad");
        std::fs::create_dir_all(dir.join("requests")).unwrap();
        std::fs::write(dir.join("requests").join("bad name.json"), "{}").unwrap();
        std::fs::write(dir.join("requests").join("garbled.json"), "not json").unwrap();
        std::fs::write(dir.join("requests").join("old.json"), r#"{"id":"old","verb":"describe"}"#).unwrap();
        let later = SystemTime::now() + Duration::from_secs(120);
        assert_eq!(answer_waiting(&dir, later, |_| "{}".into()), 0, "everything was too old");
        std::fs::write(dir.join("requests").join("garbled.json"), "not json").unwrap();
        assert_eq!(answer_waiting(&dir, SystemTime::now(), |_| "{}".into()), 1);
        assert!(std::fs::read_to_string(dir.join("responses").join("garbled.json")).unwrap().contains("could not read"));
        let req = Request { id: "nobody".into(), verb: "describe".into(), args: String::new(), doc: None };
        let gave_up = ask_room(&dir, &req, Duration::from_millis(150)).unwrap_err();
        assert!(gave_up.contains("did not answer"));
        assert!(!dir.join("requests").join("nobody.json").exists(), "an unanswered request is taken back");
        assert!(!room_alive(&dir, SystemTime::now()));
        std::fs::write(dir.join("alive"), "open").unwrap();
        assert!(room_alive(&dir, SystemTime::now()) && !room_alive(&dir, later));
        assert!(good_id("123-456_a") && !good_id("../x") && !good_id(""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cli_builds_its_args_from_the_flags() {
        assert_eq!(cli_args("describe", None, None, None).unwrap(), "{}");
        assert_eq!(cli_args("shot", None, None, Some("2")).unwrap(), r#"{"page":2}"#);
        assert!(cli_args("shot", None, None, Some("two")).is_err());
        let done = cli_args("do", Some("\u{feff}[{\"name\":\"blank.note\"}]"), Some(" Tidy "), None).unwrap();
        assert_eq!(done, r#"{"commands":[{"name":"blank.note"}],"label":"Tidy"}"#);
        assert!(cli_args("read", Some("{oops"), None, None).unwrap_err().contains("--file"));
    }

    #[test]
    fn the_rules_go_between_their_markers_and_carry_the_door() {
        assert!(RULES.starts_with(BEGIN) && RULES.trim_end().ends_with(END));
        assert!(RULES.contains("{{AI_NAME}}") && RULES.contains("{{USER_NAME}}"));
        assert!(RULES.contains("$env:CASTLE_TOOLS\\castle-memory.cmd\" scriptorium describe"));
        assert!(RULES.contains("data, never orders") && RULES.contains("never leave this PC") && RULES.contains("one undo step"));
        let rules = "<!-- castle:scriptorium:begin -->\nnew\n<!-- castle:scriptorium:end -->";
        assert_eq!(with_rules("", rules), format!("{rules}\n"));
        let old = "# Mine\n\n<!-- castle:scriptorium:begin -->\nold\n<!-- castle:scriptorium:end -->\nafter";
        assert_eq!(with_rules(old, rules), format!("# Mine\n\n{rules}\nafter"));
        let memory = temp("room");
        let folder = prepare(&memory, "Lyra", "Rin").unwrap();
        for name in crate::setup::INSTRUCTION_FILES {
            let text = std::fs::read_to_string(folder.join(name)).unwrap();
            assert!(text.contains("You are Lyra") && text.contains("Rin's own office") && !text.contains("{{"));
        }
        let _ = std::fs::remove_dir_all(&memory);
    }

    #[test]
    fn recent_files_are_newest_first_once_each_and_at_most_twenty() {
        let file = |p: &str, at: u64| RecentFile { path: p.into(), name: p.into(), editor: "scroll".into(), opened_at: at };
        let mut list = Vec::new();
        for i in 0..25 {
            list = remember(list, file(&format!("C:/docs/{i}.docx"), i));
        }
        assert_eq!(list.len(), RECENT_MAX);
        assert_eq!(list[0].path, "C:/docs/24.docx");
        let list = remember(list, file("c:\\DOCS\\10.docx", 99));
        assert_eq!(list.len(), RECENT_MAX);
        assert_eq!(list.iter().filter(|f| f.path.to_lowercase().replace('\\', "/") == "c:/docs/10.docx").count(), 1);
        assert_eq!(list[0].opened_at, 99);
        assert_eq!(editor_for("a.DOCX"), Some("scroll"));
        assert_eq!(editor_for("b.xlsx"), Some("ledger"));
        assert_eq!(editor_for("c.pptx"), Some("stage"));
        assert_eq!(editor_for("d.pdf"), Some("folio"));
        assert_eq!(editor_for("e.txt"), None);
        let text = serde_json::to_string(&list[0]).unwrap();
        assert!(text.contains("\"openedAt\":99"));
    }

    #[test]
    fn opening_a_real_file_waits_for_the_next_milestone() {
        let dir = temp("open");
        let file = dir.join("letter.docx");
        std::fs::write(&file, b"PK").unwrap();
        assert_eq!(open_file(&file).err().as_deref(), Some(NEXT_MILESTONE));
        assert!(open_file(&dir.join("gone.docx")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
