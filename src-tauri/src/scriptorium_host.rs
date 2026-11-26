//! SCRIPTORIUM's host edge: the only place the Castle meets our own office suite. Everything of the
//! suite lives in the `scriptorium` crate (Rust) and `src/scriptorium/` (Vue) and knows nothing of the
//! Castle; this file hands the page the crate's display lists, its command bus and its undo, and gives
//! the room's companion the AI's door.
//!
//! - The open documents live in [`ScriptoriumState`], each behind its own [`Bus`] as an [`AnyDoc`]:
//!   Scroll (.docx/.docm), Ledger (.xlsx/.xlsm, and .csv/.txt as one sheet), Stage (.pptx/.pptm) and
//!   Folio (.pdf, a locked one with its password), plus M0's
//!   [`BlankDocument`] (one A4 page of `blank.note`s) kept for the bus's own checks. Files open off
//!   the window's thread (reading a big file or indexing the fonts never freezes the room) and save
//!   whole: a `.part` renamed into place, the version before kept as `<name>.bak`.
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
use scriptorium::folio::{FolioDoc, FolioError};
use scriptorium::ground::display::{Color, DisplayList, Item, Paint, PathOp, Resources, Stroke};
use scriptorium::ground::edit::Editing;
use scriptorium::ground::export::{export_pdf_with, ExportOptions};
use scriptorium::ground::font::FontIndex;
use scriptorium::ground::pdfw::{PageResources, PdfWriter};
use scriptorium::ground::text::FontCache;
use scriptorium::ledger::LedgerDoc;
use scriptorium::scroll::ScrollDoc;
use scriptorium::stage::StageDoc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

/// The room's id (Settings > Rooms, the launcher, `rooms::need`).
pub const ROOM: &str = "scriptorium";
/// The editors the page can ask for.
pub const EDITORS: [&str; 4] = ["scroll", "ledger", "stage", "folio"];
/// The event the page hears when the companion changed a document through the door.
pub const CHANGED_EVENT: &str = "scriptorium-changed";
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

/// Any document behind one bus type, so the state can hold every editor's documents together: the M0
/// blank page (kept for the door's own checks) and the four editors' real documents.
#[derive(Debug)]
pub enum AnyDoc {
    Blank(BlankDocument),
    Scroll(ScrollDoc),
    Ledger(LedgerDoc),
    Stage(StageDoc),
    Folio(FolioDoc),
}

/// `$body` with `$d` bound to whichever document `$doc` holds.
macro_rules! each_doc {
    ($doc:expr, $d:ident => $body:expr) => {
        match $doc {
            AnyDoc::Blank($d) => $body,
            AnyDoc::Scroll($d) => $body,
            AnyDoc::Ledger($d) => $body,
            AnyDoc::Stage($d) => $body,
            AnyDoc::Folio($d) => $body,
        }
    };
}

impl AnyDoc {
    /// The file's bytes to write (`None` for the blank page, which is no file). Untouched parts keep
    /// their exact bytes; a PDF is its original with an appended update.
    pub fn save(&self) -> Option<Vec<u8>> {
        match self {
            AnyDoc::Blank(_) => None,
            AnyDoc::Scroll(d) => Some(d.save()),
            AnyDoc::Ledger(d) => Some(d.save()),
            AnyDoc::Stage(d) => Some(d.save()),
            AnyDoc::Folio(d) => Some(d.save()),
        }
    }
}

impl Document for AnyDoc {
    fn kind(&self) -> &'static str {
        each_doc!(self, d => d.kind())
    }
    fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
        each_doc!(self, d => d.apply(cmd))
    }
    fn describe(&self) -> Json {
        each_doc!(self, d => d.describe())
    }
    fn read(&self, what: &Json) -> Result<Json, BusError> {
        each_doc!(self, d => d.read(what))
    }
    fn display(&self, page: usize) -> DisplayList {
        each_doc!(self, d => d.display(page))
    }
    fn page_count(&self) -> usize {
        each_doc!(self, d => d.page_count())
    }
    fn commands(&self) -> Vec<CommandSpec> {
        each_doc!(self, d => d.commands())
    }
    fn resources(&self) -> Option<&Resources> {
        each_doc!(self, d => d.resources())
    }
    fn editing(&self) -> Option<&dyn Editing> {
        each_doc!(self, d => d.editing())
    }
    fn pdf_resources(&self) -> Option<PageResources> {
        each_doc!(self, d => d.pdf_resources())
    }
}

// ---------------------------------------------------------------------------------------------------
// Files: which editor, opening, a blank of each kind, saving
// ---------------------------------------------------------------------------------------------------

/// The largest file SCRIPTORIUM opens.
pub const MAX_OPEN_BYTES: u64 = 300 * 1024 * 1024;
pub const OPENS: &str = "SCRIPTORIUM opens .docx, .xlsx, .pptx, .pdf and .csv files.";
pub const OLD_FORMATS: &str = "The older Office 97\u{2013}2003 formats arrive in a later milestone - save it as .docx/.xlsx/.pptx in Office first.";
pub const PROTECTED: &str = "This file is locked with a password (or is an older Office file renamed). SCRIPTORIUM cannot open protected files yet - remove the password in Office first.";

/// Where a file goes, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// It opens in this editor.
    Editor(&'static str),
    /// An Office 97-2003 file (.doc, .xls, .ppt): later.
    Old,
    /// Nothing SCRIPTORIUM reads.
    Unknown,
}

pub fn route(path: &str) -> Route {
    match ext_of(Path::new(path)).as_str() {
        "docx" | "docm" => Route::Editor("scroll"),
        "xlsx" | "xlsm" | "csv" | "txt" => Route::Editor("ledger"),
        "pptx" | "pptm" => Route::Editor("stage"),
        "pdf" => Route::Editor("folio"),
        "doc" | "xls" | "ppt" => Route::Old,
        _ => Route::Unknown,
    }
}

/// The editor for `path`, or why SCRIPTORIUM does not open it.
pub fn editor_or_why(path: &str) -> Result<&'static str, String> {
    match route(path) {
        Route::Editor(e) => Ok(e),
        Route::Old => Err(OLD_FORMATS.into()),
        Route::Unknown => Err(OPENS.into()),
    }
}

/// A file's extension in small letters ("" for none).
fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

/// A file's own name.
pub fn file_name(path: &Path) -> String {
    path.file_name().and_then(|n| n.to_str()).map(str::to_string).unwrap_or_else(|| path.display().to_string())
}

/// The extension a new blank of `editor` saves as.
pub fn blank_ext(editor: &str) -> &'static str {
    match editor {
        "ledger" => "xlsx",
        "stage" => "pptx",
        "folio" => "pdf",
        _ => "docx",
    }
}

/// Which fonts the editors lay out with: this machine's (the app) or none (tests: fast and the same
/// everywhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fonts {
    System,
    #[cfg_attr(not(test), allow(dead_code))]
    Empty,
}

fn label(editor: &str) -> &'static str {
    match editor {
        "ledger" => "Ledger",
        "stage" => "Stage",
        "folio" => "Folio",
        _ => "Scroll",
    }
}

/// A document of `editor` from a file's bytes.
pub fn open_bytes(editor: &str, bytes: Vec<u8>, fonts: Fonts) -> Result<AnyDoc, String> {
    open_bytes_with(editor, bytes, fonts, None)
}

/// True for a file opened as text into Ledger (.csv, .txt).
pub fn is_csv(ext: &str) -> bool {
    matches!(ext, "csv" | "txt")
}

/// A CSV (or tab-separated text) file as a one-sheet Ledger named after the file, and whether its
/// text came with a byte order mark (a save writes it back the same way).
pub fn open_csv_bytes(bytes: &[u8], sheet_name: &str, fonts: Fonts) -> Result<(AnyDoc, bool), String> {
    let cache = match fonts {
        Fonts::System => scriptorium::ledger::system_fonts(),
        Fonts::Empty => FontCache::empty(),
    };
    let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]) || bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    let doc = LedgerDoc::open_csv(bytes, sheet_name, cache).map_err(|e| format!("Ledger could not open it: {e}"))?;
    Ok((AnyDoc::Ledger(doc), bom))
}

/// [`open_bytes`] with a locked PDF's password. A locked PDF's own words (it needs a password, or
/// that one does not open it) come back as they are, so the page knows to ask; the password is
/// never kept or written anywhere.
pub fn open_bytes_with(editor: &str, bytes: Vec<u8>, fonts: Fonts, password: Option<&str>) -> Result<AnyDoc, String> {
    if editor != "folio" {
        if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
            return Err(PROTECTED.into());
        }
        if !bytes.starts_with(b"PK") {
            return Err(format!("{} could not open it: there is no Office file inside.", label(editor)));
        }
    }
    let failed = |e: String| format!("{} could not open it: {e}", label(editor));
    match editor {
        "scroll" => {
            let index = match fonts {
                Fonts::System => scriptorium::scroll::system_fonts(),
                Fonts::Empty => Arc::new(FontIndex::new()),
            };
            ScrollDoc::open_with_fonts(&bytes, index).map(AnyDoc::Scroll).map_err(failed)
        }
        "ledger" => {
            let cache = match fonts {
                Fonts::System => scriptorium::ledger::system_fonts(),
                Fonts::Empty => FontCache::empty(),
            };
            LedgerDoc::open_with_fonts(&bytes, cache).map(AnyDoc::Ledger).map_err(failed)
        }
        "stage" => {
            let cache = match fonts {
                Fonts::System => scriptorium::stage::system_fonts(),
                Fonts::Empty => FontCache::empty(),
            };
            StageDoc::open_with_fonts(&bytes, cache).map(AnyDoc::Stage).map_err(|e| failed(e.to_string()))
        }
        "folio" => {
            let opened = match password {
                Some(p) => scriptorium::folio::open_with_password(bytes, p),
                None => FolioDoc::open(bytes),
            };
            opened.map(AnyDoc::Folio).map_err(folio_error)
        }
        other => Err(format!("SCRIPTORIUM has no editor called {other}.")),
    }
}

/// A PDF that did not open, in words: a locked one's own words as they are (the page asks for the
/// password when it reads them), anything else after "Folio could not open it".
pub fn folio_error(e: FolioError) -> String {
    match e {
        FolioError::NeedsPassword | FolioError::WrongPassword => e.to_string(),
        other => format!("{} could not open it: {other}", label("folio")),
    }
}

/// The bytes of a blank one-page A4 PDF, written by our own PDF writer.
pub fn blank_pdf() -> Vec<u8> {
    let mut writer = PdfWriter::new();
    writer.add_page(&DisplayList::new(A4.0, A4.1), &PageResources::new());
    writer.finish()
}

/// A new, empty document of `editor`: a Letter page (Scroll), one sheet (Ledger), one title slide
/// (Stage) or one A4 page (Folio). `blank` is the M0 page of notes (the bus's own check).
pub fn blank_doc(editor: &str, fonts: Fonts) -> Result<AnyDoc, String> {
    match editor {
        "blank" => Ok(AnyDoc::Blank(BlankDocument::default())),
        // Scroll's blank always lays out with this machine's fonts (the crate makes it so)
        "scroll" => Ok(AnyDoc::Scroll(ScrollDoc::new_blank())),
        "ledger" => open_bytes(editor, scriptorium::ledger::blank_xlsx(), fonts),
        "stage" => open_bytes(editor, scriptorium::stage::blank::blank_deck(), fonts),
        "folio" => open_bytes(editor, blank_pdf(), fonts),
        other => Err(format!("SCRIPTORIUM has no editor called {other}. Its editors are Scroll, Ledger, Stage and Folio.")),
    }
}

/// The editor and a new blank document for the page's `kind` (an empty kind is Scroll; `blank` is
/// M0's page of notes, shown in Scroll).
pub fn make_blank(kind: &str, fonts: Fonts) -> Result<(&'static str, AnyDoc), String> {
    let kind = kind.trim().to_lowercase();
    let kind = if kind.is_empty() { "scroll".to_string() } else { kind };
    let editor = if kind == "blank" {
        "scroll"
    } else {
        EDITORS.iter().copied().find(|e| *e == kind).ok_or_else(|| format!("SCRIPTORIUM has no editor called {kind}. Its editors are Scroll, Ledger, Stage and Folio."))?
    };
    Ok((editor, blank_doc(&kind, fonts)?))
}

/// Reads and opens a file the member chose: (its editor, the document). Runs off the window's thread.
pub fn load_file(path: &Path, fonts: Fonts) -> Result<(&'static str, AnyDoc), String> {
    load_file_with(path, fonts, None).map(|l| (l.editor, l.doc))
}

/// A file read and opened.
pub struct Loaded {
    pub editor: &'static str,
    pub doc: AnyDoc,
    /// Some(byte order mark) for a CSV or text file opened into Ledger.
    pub csv_bom: Option<bool>,
}

/// [`load_file`] with a locked PDF's password, telling a CSV file apart.
pub fn load_file_with(path: &Path, fonts: Fonts, password: Option<&str>) -> Result<Loaded, String> {
    let editor = editor_or_why(&path.to_string_lossy())?;
    let meta = std::fs::metadata(path).map_err(|_| "That file is not there any more.".to_string())?;
    if !meta.is_file() {
        return Err("That is a folder, not a file.".into());
    }
    let mb = |n: u64| n / (1024 * 1024);
    if meta.len() > MAX_OPEN_BYTES {
        return Err(format!("{} is {} MB; SCRIPTORIUM opens files up to {} MB.", file_name(path), mb(meta.len()), mb(MAX_OPEN_BYTES)));
    }
    let bytes = std::fs::read(path).map_err(|_| format!("{} could not be read (is it open in another program?).", file_name(path)))?;
    if is_csv(&ext_of(path)) {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Sheet1");
        let (doc, bom) = open_csv_bytes(&bytes, stem, fonts)?;
        return Ok(Loaded { editor, doc, csv_bom: Some(bom) });
    }
    if bytes.is_empty() {
        return Err(format!("{} is empty.", file_name(path)));
    }
    Ok(Loaded { editor, doc: open_bytes_with(editor, bytes, fonts, password)?, csv_bom: None })
}

/// Why `path` is not a place to save a document of type `.ext`: a document keeps its own type (a
/// .docm stays a .docm, so a macro workbook's parts never land in a plain .xlsx).
pub fn save_place(ext: &str, path: &Path) -> Result<(), String> {
    if ext_of(path) != ext {
        return Err(format!("Save it as a .{ext} file."));
    }
    Ok(())
}

/// Writes `bytes` to `path` whole: a `.scriptorium-part` beside it first, then renamed into place.
/// When the member saves over a file, the version before is kept as `<name>.bak` (one, replaced by
/// the next save).
pub fn write_document(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = file_name(path);
    let part = path.with_file_name(format!("{name}.scriptorium-part"));
    let fail = |why: &str| format!("{name} could not be saved: {why}.");
    if std::fs::write(&part, bytes).is_err() {
        let _ = std::fs::remove_file(&part);
        return Err(fail("the folder is not writable or the disk is full"));
    }
    if path.is_file() && std::fs::copy(path, path.with_file_name(format!("{name}.bak"))).is_err() {
        let _ = std::fs::remove_file(&part);
        return Err(fail("the version before could not be kept as .bak"));
    }
    std::fs::rename(&part, path).map_err(|_| {
        let _ = std::fs::remove_file(&part);
        fail("it is open in another program")
    })
}

// ---------------------------------------------------------------------------------------------------
// The open documents
// ---------------------------------------------------------------------------------------------------

pub struct OpenDoc {
    /// The editor the page shows it in: scroll, ledger, stage or folio.
    pub editor: String,
    pub bus: Bus<AnyDoc>,
    /// The file it was opened from or last saved to (None for a new document).
    pub path: Option<PathBuf>,
    /// The file's name, or "Untitled" for a new one.
    pub title: String,
    /// The type it saves as, in small letters (docx, docm, xlsx, .., pdf, csv).
    pub ext: String,
    /// Some(byte order mark) while a Ledger opened from a CSV or text file saves as one.
    pub csv_bom: Option<bool>,
}

impl OpenDoc {
    pub fn new(editor: &str, doc: AnyDoc, path: Option<PathBuf>) -> OpenDoc {
        let title = path.as_deref().map(file_name).unwrap_or_else(|| "Untitled".into());
        let ext = path.as_deref().map(ext_of).filter(|e| !e.is_empty()).unwrap_or_else(|| blank_ext(editor).into());
        OpenDoc { editor: editor.into(), bus: Bus::new(doc), path, title, ext, csv_bom: None }
    }

    /// The bytes to write to `target`: the document's own type, or for a sheet opened from CSV its
    /// sheet on show as CSV (the byte order mark it had) or the whole workbook as .xlsx.
    pub fn bytes_for(&self, target: &Path) -> Result<Vec<u8>, String> {
        if let (Some(bom), AnyDoc::Ledger(d)) = (self.csv_bom, self.bus.doc()) {
            return match ext_of(target).as_str() {
                "csv" | "txt" => Ok(d.save_csv(d.active_sheet(), bom)),
                "xlsx" => Ok(d.save()),
                _ => Err(format!("Save it as a .{} or .xlsx file.", self.ext)),
            };
        }
        save_place(&self.ext, target)?;
        self.bus.doc().save().ok_or_else(|| "This page is not a file; there is nothing to save.".to_string())
    }

    /// After a save to `target`: a CSV sheet saved as .xlsx is a workbook from then on.
    pub fn saved_to(&mut self, target: &Path) {
        self.title = file_name(target);
        self.path = Some(target.to_path_buf());
        if self.csv_bom.is_some() {
            let ext = ext_of(target);
            if !is_csv(&ext) {
                self.csv_bom = None;
            }
            self.ext = ext;
        }
    }
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
    /// Puts an opened document in the room; it becomes the current one (the door's default). Its id.
    pub fn add(&mut self, open: OpenDoc) -> u64 {
        self.next += 1;
        let id = self.next;
        self.open.insert(id, open);
        self.current = Some(id);
        id
    }

    /// The page let a document go (another took its place).
    pub fn close(&mut self, id: u64) {
        self.open.remove(&id);
        if self.current == Some(id) {
            self.current = None;
        }
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

    /// The member's own command (a button, a shortcut, typing): one undo step with `Origin::User`
    /// (none when it changed nothing, as a find). `{"ok":true,"step","pages","selection"}`, where
    /// `selection` is where the editor says the selection landed (null when it does not say).
    pub fn command(&mut self, id: u64, name: &str, args_text: &str) -> Result<String, String> {
        let open = self.get(id)?;
        let args = parse_args(args_text).map_err(|e| e.to_string())?;
        open.bus.do_user(Command::new(name, args)).map(|v| v.to_string()).map_err(|e| e.to_string())
    }

    /// The editing door on a document (`hit`, `caret`, `selection`, `state`, `copy`, `move`): JSON
    /// text in and out. A document that cannot be edited by hand yet answers nothing (null, [], {}).
    pub fn edit(&mut self, id: u64, verb: &str, args: &Json) -> Result<String, String> {
        let open = self.get(id)?;
        open.bus.edit(verb, args).map(|v| v.to_string()).map_err(|e| e.to_string())
    }

    /// The document as one PDF (every page), titled by its file's name.
    pub fn export_pdf(&mut self, id: u64) -> Result<Vec<u8>, String> {
        let open = self.get(id)?;
        let title = Path::new(&open.title).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(export_pdf_with(open.bus.doc(), &ExportOptions { title, date: None }))
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

// ---------------------------------------------------------------------------------------------------
// The page's commands
// ---------------------------------------------------------------------------------------------------

type RoomState<'a> = (State<'a, SettingsStore>, State<'a, ScriptoriumState>);

fn docs<'a>(room: &'a RoomState<'_>) -> Result<std::sync::MutexGuard<'a, Docs>, String> {
    crate::rooms::need(&room.0, ROOM)?;
    Ok(room.1.docs.lock().unwrap())
}

/// A new blank document of `kind` (scroll, ledger, stage or folio); its id. Made off the window's
/// thread: the first one indexes this machine's fonts.
#[tauri::command]
pub async fn scriptorium_new(kind: String, store: State<'_, SettingsStore>, state: State<'_, ScriptoriumState>) -> Result<u64, String> {
    crate::rooms::need(&store, ROOM)?;
    let (editor, doc) = tauri::async_runtime::spawn_blocking(move || make_blank(&kind, Fonts::System)).await.map_err(|_| "The new document could not be made.".to_string())??;
    Ok(state.docs.lock().unwrap().add(OpenDoc::new(editor, doc, None)))
}

/// The page let a document go (a new or opened one took its place in its editor).
#[tauri::command]
pub fn scriptorium_close_doc(doc: u64, state: State<ScriptoriumState>) {
    state.docs.lock().unwrap().close(doc);
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

/// JSON text of a page's argument (a pos or a selection); an empty one is null.
fn arg_json(text: &str, what: &str) -> Result<Json, String> {
    if text.trim().is_empty() {
        return Ok(Json::Null);
    }
    json::parse(text).map_err(|e| format!("The {what} is not readable: {e}"))
}

/// The place under a page point (points): the editor's own pos as JSON, or `null`.
#[tauri::command]
pub fn scriptorium_hit(doc: u64, page: usize, x: f64, y: f64, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let args = Json::obj().with("page", page).with("x", x).with("y", y);
    docs(&(store, state))?.edit(doc, "hit", &args)
}

/// The caret's box at a pos: `{"page","x","y","height"}` or `null`.
#[tauri::command]
pub fn scriptorium_caret(doc: u64, pos_json: String, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let args = Json::obj().with("pos", arg_json(&pos_json, "place")?);
    docs(&(store, state))?.edit(doc, "caret", &args)
}

/// The boxes that paint a selection: `[{"page","x","y","w","h"}, ..]`.
#[tauri::command]
pub fn scriptorium_selection(doc: u64, sel_json: String, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let args = Json::obj().with("sel", arg_json(&sel_json, "selection")?);
    docs(&(store, state))?.edit(doc, "selection", &args)
}

/// The formatting under a selection, for the ribbon's pressed buttons (the editor's own keys).
#[tauri::command]
pub fn scriptorium_state(doc: u64, sel_json: String, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let args = Json::obj().with("sel", arg_json(&sel_json, "selection")?);
    docs(&(store, state))?.edit(doc, "state", &args)
}

/// What a copy of a selection takes: `{"text","rich"}`.
#[tauri::command]
pub fn scriptorium_copy(doc: u64, sel_json: String, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let args = Json::obj().with("sel", arg_json(&sel_json, "selection")?);
    docs(&(store, state))?.edit(doc, "copy", &args)
}

/// The place one caret motion away (left, right, up, down, word_left, word_right, line_start,
/// line_end, doc_start, doc_end): a pos, or `null` when the caret stays.
#[tauri::command]
pub fn scriptorium_move(doc: u64, pos_json: String, motion: String, store: State<SettingsStore>, state: State<ScriptoriumState>) -> Result<String, String> {
    let args = Json::obj().with("pos", arg_json(&pos_json, "place")?).with("motion", motion.as_str());
    docs(&(store, state))?.edit(doc, "move", &args)
}

/// The PDF name offered for a document's export: its own name with .pdf.
pub fn export_name(title: &str) -> String {
    let stem = Path::new(title.trim()).file_stem().map(|s| s.to_string_lossy().trim().to_string()).filter(|s| !s.is_empty());
    format!("{}.pdf", stem.unwrap_or_else(|| "Untitled".into()))
}

/// Exports a document as a PDF of every page: the save picker first (None when it is closed), then our
/// own PDF written whole. Answers the path written.
#[tauri::command]
pub async fn scriptorium_export_pdf(app: AppHandle, doc: u64, store: State<'_, SettingsStore>, state: State<'_, ScriptoriumState>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    crate::rooms::need(&store, ROOM)?;
    let name = export_name(&state.docs.lock().unwrap().get(doc)?.title);
    let picked = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app.dialog()
                .file()
                .set_title("Export as PDF")
                .set_file_name(&name)
                .add_filter("PDF", &["pdf"])
                .blocking_save_file()
                .and_then(|file| file.into_path().ok())
        })
        .await
        .ok()
        .flatten()
    };
    let Some(mut target) = picked else { return Ok(None) };
    if ext_of(&target) != "pdf" {
        target.set_extension("pdf");
    }
    let bytes = state.docs.lock().unwrap().export_pdf(doc)?;
    {
        let target = target.clone();
        tauri::async_runtime::spawn_blocking(move || write_document(&target, &bytes)).await.map_err(|_| "The PDF could not be written.".to_string())??;
    }
    Ok(Some(target.display().to_string()))
}

/// Where a print's PDF waits for the printer: the system's temp folder, one file per document.
pub fn print_path(doc: u64) -> PathBuf {
    std::env::temp_dir().join(format!("scriptorium-print-{}-{doc}.pdf", std::process::id()))
}

/// Prints a document: a PDF of every page in the temp folder, handed to the system's print verb
/// (Windows: the PDF app's "print"; when no app offers it, the PDF opens so it can be printed from
/// there; macOS and Linux: `lp`). Answers what happened in plain words.
#[tauri::command]
pub async fn scriptorium_print(doc: u64, store: State<'_, SettingsStore>, state: State<'_, ScriptoriumState>) -> Result<String, String> {
    crate::rooms::need(&store, ROOM)?;
    let bytes = state.docs.lock().unwrap().export_pdf(doc)?;
    let file = print_path(doc);
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::write(&file, &bytes).map_err(|_| "The page could not be made ready for printing (the temp folder is not writable).".to_string())?;
        print_file(&file)
    })
    .await
    .map_err(|_| "The document could not be printed.".to_string())?
}

#[cfg(windows)]
fn print_file(file: &Path) -> Result<String, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let path = wide(file.as_os_str());
    let run = |verb: &str, show| {
        let verb = wide(std::ffi::OsStr::new(verb));
        // SAFETY: every pointer points into a Vec that outlives the call; null is allowed for the
        // window, the parameters and the folder.
        let code = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), path.as_ptr(), std::ptr::null(), std::ptr::null(), show) };
        // ShellExecuteW answers a value above 32 when it worked
        code as isize > 32
    };
    if run("print", SW_HIDE) {
        return Ok("Sent to the printer.".into());
    }
    if run("open", SW_SHOWNORMAL) {
        return Ok("No app here prints PDFs directly, so the PDF opened - print it from there.".into());
    }
    Err("Windows has no app to print PDFs with.".into())
}

#[cfg(not(windows))]
fn print_file(file: &Path) -> Result<String, String> {
    let out = std::process::Command::new("lp").arg(file).output().map_err(|_| "The system's printing (lp) could not be reached.".to_string())?;
    if out.status.success() {
        Ok("Sent to the printer.".into())
    } else {
        Err(format!("The printer refused it: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// The member picks a document to open; None when the picker is closed.
#[tauri::command]
pub async fn scriptorium_open_dialog(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Open a document")
            .add_filter("Documents, sheets, slides and PDFs", &["docx", "docm", "xlsx", "xlsm", "pptx", "pptm", "pdf", "csv"])
            .add_filter("Sheets as text (CSV)", &["csv", "txt"])
            .add_filter("Older Office files (later)", &["doc", "xls", "ppt"])
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
        let mut dialog = app.dialog().file().set_title("Save the document").set_file_name(&name);
        for (what, ext) in save_filters(&name) {
            dialog = dialog.add_filter(what, &[ext.as_str()]);
        }
        dialog
            .blocking_save_file()
            .and_then(|file| file.into_path().ok())
            .map(|path| path.display().to_string())
    })
    .await
    .ok()
    .flatten()
}

/// The save picker's filters for a file name: its own first, and a sheet opened from CSV can be
/// saved as a whole workbook too.
pub fn save_filters(name: &str) -> Vec<(&'static str, String)> {
    let first = save_filter(name);
    if is_csv(&first.1) {
        vec![first, ("Sheet", "xlsx".into())]
    } else {
        vec![first]
    }
}

/// The save picker's filter for a file name: its kind in words and its extension.
pub fn save_filter(name: &str) -> (&'static str, String) {
    let ext = ext_of(Path::new(name));
    if ext == "csv" {
        return ("Comma-separated values", ext);
    }
    if ext == "txt" {
        return ("Text", ext);
    }
    let what = match route(name) {
        Route::Editor("ledger") => "Sheet",
        Route::Editor("stage") => "Slides",
        Route::Editor("folio") => "PDF",
        Route::Editor(_) => "Document",
        _ => return ("Document", "docx".into()),
    };
    (what, ext)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub doc: u64,
    pub editor: String,
    pub pages: usize,
    pub title: String,
    pub path: String,
}

/// Opens a file the member chose (off the window's thread); it becomes the current document and joins
/// the recent files. `{doc, editor, pages, title, path}`. `password` opens a locked PDF (the page asks
/// for it when the first try says the PDF is locked); it is used once and never kept or logged.
#[tauri::command]
pub async fn scriptorium_open(path: String, password: Option<String>, store: State<'_, SettingsStore>, state: State<'_, ScriptoriumState>) -> Result<Opened, String> {
    crate::rooms::need(&store, ROOM)?;
    let file = PathBuf::from(path.trim());
    let loaded = {
        let file = file.clone();
        tauri::async_runtime::spawn_blocking(move || load_file_with(&file, Fonts::System, password.as_deref())).await.map_err(|_| "The file could not be opened.".to_string())??
    };
    let editor = loaded.editor;
    let mut open = OpenDoc::new(editor, loaded.doc, Some(file.clone()));
    open.csv_bom = loaded.csv_bom;
    let (pages, title) = (open.bus.doc().page_count(), open.title.clone());
    let doc = state.docs.lock().unwrap().add(open);
    let shown = file.display().to_string();
    note_recent(&store, &shown, editor);
    Ok(Opened { doc, editor: editor.into(), pages, title, path: shown })
}

/// Saves a document: to `path` when given (Save as), else to the file it came from. Its bytes are
/// written whole off the window's thread; answers the path it was saved to.
#[tauri::command]
pub async fn scriptorium_save(doc: u64, path: Option<String>, store: State<'_, SettingsStore>, state: State<'_, ScriptoriumState>) -> Result<String, String> {
    crate::rooms::need(&store, ROOM)?;
    let (target, bytes, editor) = {
        let mut docs = state.docs.lock().unwrap();
        let open = docs.get(doc)?;
        let target = match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => PathBuf::from(p),
            None => open.path.clone().ok_or("Choose where to save it first (Save as).")?,
        };
        let bytes = open.bytes_for(&target)?;
        (target, bytes, open.editor.clone())
    };
    {
        let target = target.clone();
        tauri::async_runtime::spawn_blocking(move || write_document(&target, &bytes)).await.map_err(|_| "The file could not be saved.".to_string())??;
    }
    if let Ok(open) = state.docs.lock().unwrap().get(doc) {
        open.saved_to(&target);
    }
    let shown = target.display().to_string();
    note_recent(&store, &shown, &editor);
    Ok(shown)
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

    #[test]
    fn the_editing_edge_answers_for_every_document() {
        let mut docs = Docs::default();
        let id = docs.add(OpenDoc::new("scroll", AnyDoc::Blank(BlankDocument::default()), None));
        // the blank page takes no caret yet: nothing comes back, never an error
        assert_eq!(docs.edit(id, "hit", &Json::obj().with("page", 0).with("x", 80.0).with("y", 90.0)).unwrap(), "null");
        assert_eq!(docs.edit(id, "selection", &Json::obj().with("sel", Json::obj().with("anchor", 1))).unwrap(), "[]");
        assert_eq!(docs.edit(id, "state", &Json::obj().with("sel", Json::obj().with("anchor", 1))).unwrap(), "{}");
        assert!(docs.edit(id, "state", &Json::obj()).is_err(), "state needs a selection");
        assert!(docs.edit(id + 1, "hit", &Json::obj()).is_err());
        // a command answers where the selection landed
        let out = docs.command(id, "blank.note", r#"{"text":"x"}"#).unwrap();
        assert_eq!(out, r#"{"ok":true,"step":1,"pages":[0],"selection":null}"#);
        // export: every page as our own PDF
        let pdf = docs.export_pdf(id).unwrap();
        assert!(pdf.starts_with(b"%PDF-") && pdf.ends_with(b"%%EOF\n"));
        assert_eq!(export_name("Report.docx"), "Report.pdf");
        assert_eq!(export_name("  "), "Untitled.pdf");
        assert_eq!(export_name("Untitled"), "Untitled.pdf");
        assert!(print_path(7).to_string_lossy().ends_with("-7.pdf"));
        assert_eq!(arg_json("", "place").unwrap(), Json::Null);
        assert!(arg_json("{", "place").is_err());
    }

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
        let id = docs.add(OpenDoc::new("ledger", AnyDoc::Blank(BlankDocument::default()), None));
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
        assert!(make_blank("spreadsheet", Fonts::Empty).is_err());
    }

    #[test]
    fn the_inbox_answers_a_request_the_cli_waits_for() {
        let dir = temp("inbox");
        let answerer = {
            let dir = dir.clone();
            std::thread::spawn(move || {
                let mut docs = Docs::default();
                let (editor, page) = make_blank("blank", Fonts::Empty).unwrap();
                docs.add(OpenDoc::new(editor, page, None));
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
        assert_eq!(route("a.DOCX"), Route::Editor("scroll"));
        assert_eq!(route("b.xlsx"), Route::Editor("ledger"));
        assert_eq!(route("c.pptx"), Route::Editor("stage"));
        assert_eq!(route("d.pdf"), Route::Editor("folio"));
        assert_eq!(route("e.txt"), Route::Editor("ledger"));
        assert_eq!(route("f.CSV"), Route::Editor("ledger"));
        assert_eq!(route("g.odt"), Route::Unknown);
        let text = serde_json::to_string(&list[0]).unwrap();
        assert!(text.contains("\"openedAt\":99"));
    }

    // --- real files (M1) ---------------------------------------------------------------------------

    use scriptorium::ground::zip::{Method, ZipWriter};

    fn zip(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut w = ZipWriter::new();
        for (name, text) in parts {
            w.add(name, text.as_bytes(), Method::Deflated(6)).unwrap();
        }
        w.finish()
    }

    const RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";

    /// A tiny .docx made here: two paragraphs, nothing else.
    fn invented_docx() -> Vec<u8> {
        zip(&[
            (
                "[Content_Types].xml",
                &format!(r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELS}"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
            ),
            ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#),
            ("word/document.xml", r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Dear Rin</w:t></w:r></w:p><w:p><w:r><w:t>The second line</w:t></w:r></w:p></w:body></w:document>"#),
        ])
    }

    /// A tiny .xlsx made here: two sheets, a number in Sheet1!B2.
    fn invented_xlsx() -> Vec<u8> {
        let ws = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
        let sheet = |body: &str| format!(r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{body}</sheetData></worksheet>"#);
        zip(&[
            (
                "[Content_Types].xml",
                &format!(r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{RELS}"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="{ws}"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="{ws}"/></Types>"#),
            ),
            ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
            ("xl/workbook.xml", r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/><sheet name="Costs" sheetId="2" r:id="rId2"/></sheets></workbook>"#),
            ("xl/_rels/workbook.xml.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#),
            ("xl/worksheets/sheet1.xml", &sheet(r#"<row r="2"><c r="B2"><v>42</v></c></row>"#)),
            ("xl/worksheets/sheet2.xml", &sheet("")),
        ])
    }

    /// A tiny .pdf made by our own writer: two pages, a title.
    fn invented_pdf() -> Vec<u8> {
        let mut w = PdfWriter::new();
        w.set_info("Field notes", "", "", "", "", "");
        w.set_date(1_700_000_000);
        w.add_page(&DisplayList::new(400.0, 300.0), &PageResources::new());
        w.add_page(&DisplayList::new(400.0, 300.0), &PageResources::new());
        w.finish()
    }

    fn reopen(editor: &str, doc: &AnyDoc) -> AnyDoc {
        open_bytes(editor, doc.save().expect("a real document saves"), Fonts::Empty).unwrap()
    }

    #[test]
    fn files_go_to_their_editor_by_extension_and_the_rest_are_told_why() {
        assert_eq!(route("C:/a/Letter.DOCX"), Route::Editor("scroll"));
        assert_eq!(route("macro.docm"), Route::Editor("scroll"));
        assert_eq!(route("Budget.xlsm"), Route::Editor("ledger"));
        assert_eq!(route("deck.pptm"), Route::Editor("stage"));
        assert_eq!(route("scan.Pdf"), Route::Editor("folio"));
        for old in ["old.doc", "old.XLS", "old.ppt"] {
            assert_eq!(route(old), Route::Old);
            assert_eq!(editor_or_why(old).unwrap_err(), OLD_FORMATS);
        }
        assert!(OLD_FORMATS.contains("97\u{2013}2003") && OLD_FORMATS.contains("save it as .docx/.xlsx/.pptx in Office first"));
        assert_eq!(editor_or_why("notes.odt").unwrap_err(), OPENS);
        assert_eq!(editor_or_why("no-extension").unwrap_err(), OPENS);
        assert_eq!(save_filter("Budget.xlsx"), ("Sheet", "xlsx".to_string()));
        assert_eq!(save_filter("Talk.pptm"), ("Slides", "pptm".to_string()));
        assert_eq!(save_filter("odd.odt"), ("Document", "docx".to_string()));
        assert_eq!(save_filters("Budget.xlsx"), vec![("Sheet", "xlsx".to_string())]);
        assert_eq!(save_filters("People.csv"), vec![("Comma-separated values", "csv".to_string()), ("Sheet", "xlsx".to_string())]);
        assert_eq!(save_filters("List.txt")[0], ("Text", "txt".to_string()));
    }

    #[test]
    fn invented_files_of_each_kind_open_through_anydoc() {
        let scroll = open_bytes("scroll", invented_docx(), Fonts::Empty).unwrap();
        assert_eq!(scroll.kind(), "scroll");
        assert!(scroll.page_count() >= 1 && scroll.resources().is_some());
        assert_eq!(scroll.read(&Json::obj().with("paragraph", 1)).unwrap().get("text").and_then(Json::as_str), Some("The second line"));
        assert!(scroll.commands().iter().any(|c| c.to_json().to_string().contains("scroll.set_paragraph_text")));

        let ledger = open_bytes("ledger", invented_xlsx(), Fonts::Empty).unwrap();
        assert_eq!(ledger.kind(), "ledger");
        assert_eq!(ledger.page_count(), 2, "a page per sheet");
        let described = ledger.describe().to_string();
        assert!(described.contains("\"name\":\"Budget\"") && described.contains("\"name\":\"Costs\""));
        assert!(ledger.read(&Json::obj().with("cell", "B2")).unwrap().to_string().contains("42"));

        let stage = open_bytes("stage", scriptorium::stage::blank::blank_deck(), Fonts::Empty).unwrap();
        assert_eq!((stage.kind(), stage.page_count()), ("stage", 1));
        assert!(stage.describe().get("slide_count").is_some());

        let folio = open_bytes("folio", invented_pdf(), Fonts::Empty).unwrap();
        assert_eq!((folio.kind(), folio.page_count()), ("folio", 2));
        let dl = folio.display(1);
        assert_eq!((dl.width, dl.height), (400.0, 300.0));
        assert!(folio.describe().to_string().contains("Field notes"));
        assert!(folio.resources().is_some());
        assert_eq!(folio.save().unwrap(), invented_pdf(), "an untouched PDF saves as it came");
    }

    #[test]
    fn what_is_not_a_document_is_refused_in_plain_words() {
        assert_eq!(open_bytes("scroll", vec![0xD0, 0xCF, 0x11, 0xE0, 0, 0], Fonts::Empty).err().as_deref(), Some(PROTECTED));
        assert!(open_bytes("ledger", b"hello".to_vec(), Fonts::Empty).unwrap_err().starts_with("Ledger could not open it"));
        assert!(open_bytes("stage", invented_docx(), Fonts::Empty).unwrap_err().starts_with("Stage could not open it"));
        assert!(open_bytes("folio", b"not a pdf".to_vec(), Fonts::Empty).unwrap_err().starts_with("Folio could not open it"));
        let dir = temp("refuse");
        assert_eq!(load_file(&dir.join("gone.docx"), Fonts::Empty).err().as_deref(), Some("That file is not there any more."));
        std::fs::write(dir.join("empty.xlsx"), b"").unwrap();
        assert!(load_file(&dir.join("empty.xlsx"), Fonts::Empty).unwrap_err().contains("is empty"));
        std::fs::write(dir.join("old.doc"), b"x").unwrap();
        assert_eq!(load_file(&dir.join("old.doc"), Fonts::Empty).err().as_deref(), Some(OLD_FORMATS));
        std::fs::write(dir.join("letter.docx"), invented_docx()).unwrap();
        let (editor, doc) = load_file(&dir.join("letter.docx"), Fonts::Empty).unwrap();
        assert_eq!((editor, doc.kind()), ("scroll", "scroll"));
        assert_eq!(MAX_OPEN_BYTES, 300 * 1024 * 1024);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_locked_pdf_says_so_in_its_own_words_for_the_page_to_ask() {
        let needs = folio_error(FolioError::NeedsPassword);
        assert!(needs.contains("locked with a password") && !needs.starts_with("Folio could not"), "{needs}");
        let wrong = folio_error(FolioError::WrongPassword);
        assert!(wrong.contains("password does not open"), "{wrong}");
        // an unlocked PDF opens with or without a password given
        let folio = open_bytes_with("folio", invented_pdf(), Fonts::Empty, Some("anything")).unwrap();
        assert_eq!(folio.page_count(), 2);
    }

    #[test]
    fn a_csv_opens_as_one_sheet_and_saves_back_as_csv_or_as_a_workbook() {
        let dir = temp("csv");
        let file = dir.join("People.csv");
        std::fs::write(&file, b"\xEF\xBB\xBFName,Age\nRin,7\n").unwrap();
        let loaded = load_file_with(&file, Fonts::Empty, None).unwrap();
        assert_eq!((loaded.editor, loaded.doc.kind(), loaded.csv_bom), ("ledger", "ledger", Some(true)));
        assert!(loaded.doc.read(&Json::obj().with("cell", "A2")).unwrap().to_string().contains("Rin"));
        assert!(loaded.doc.describe().to_string().contains("\"name\":\"People\""), "the sheet is named after the file");
        let mut open = OpenDoc::new(loaded.editor, loaded.doc, Some(file.clone()));
        open.csv_bom = loaded.csv_bom;
        assert_eq!(open.ext, "csv");
        let csv = open.bytes_for(&file).unwrap();
        assert!(csv.starts_with(b"\xEF\xBB\xBFName,Age"), "{:?}", String::from_utf8_lossy(&csv));
        assert!(open.bytes_for(&dir.join("People.xlsx")).unwrap().starts_with(b"PK"));
        assert!(open.bytes_for(&dir.join("People.docx")).unwrap_err().contains(".xlsx"));
        open.saved_to(&dir.join("People.xlsx"));
        assert_eq!((open.ext.as_str(), open.csv_bom, open.title.as_str()), ("xlsx", None, "People.xlsx"));
        assert_eq!(open.bytes_for(&dir.join("People.csv")).unwrap_err(), "Save it as a .xlsx file.");

        let plain = dir.join("list.txt");
        std::fs::write(&plain, b"a\tb\n1\t2\n").unwrap();
        let loaded = load_file_with(&plain, Fonts::Empty, None).unwrap();
        assert_eq!(loaded.csv_bom, Some(false));
        let mut open = OpenDoc::new(loaded.editor, loaded.doc, Some(plain.clone()));
        open.csv_bom = loaded.csv_bom;
        let text = open.bytes_for(&plain).unwrap();
        assert!(!text.starts_with(b"\xEF\xBB\xBF"), "no mark before, none after");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_blank_of_each_editor_is_real_and_saves_and_reopens() {
        for kind in ["ledger", "stage", "folio"] {
            let (editor, doc) = make_blank(kind, Fonts::Empty).unwrap();
            assert_eq!((editor, doc.kind()), (kind, kind));
            assert_eq!(doc.page_count(), 1, "{kind}");
            let again = reopen(kind, &doc);
            assert_eq!(again.page_count(), 1, "{kind} reopens");
        }
        let (_, pdf) = make_blank("folio", Fonts::Empty).unwrap();
        assert_eq!((pdf.display(0).width, pdf.display(0).height), A4);
        let (editor, page) = make_blank("blank", Fonts::Empty).unwrap();
        assert_eq!((editor, page.kind()), ("scroll", "blank"));
        assert!(page.save().is_none(), "the M0 page is no file");
        assert!(make_blank("spreadsheet", Fonts::Empty).unwrap_err().contains("no editor called spreadsheet"));
        assert_eq!(blank_ext("stage"), "pptx");
    }

    #[test]
    fn scrolls_blank_is_a_real_docx() {
        let (editor, doc) = make_blank("", Fonts::System).unwrap();
        assert_eq!((editor, doc.kind()), ("scroll", "scroll"), "an empty kind is Scroll");
        let again = reopen("scroll", &doc);
        assert!(again.page_count() >= 1);
        assert!(doc.save().unwrap().starts_with(b"PK"));
    }

    #[test]
    fn the_companions_first_commands_go_through_the_bus_and_survive_a_save() {
        let mut docs = Docs::default();
        let ledger = docs.add(OpenDoc::new("ledger", open_bytes("ledger", invented_xlsx(), Fonts::Empty).unwrap(), Some(PathBuf::from("C:/x/Budget.xlsx"))));
        assert_eq!(docs.get(ledger).unwrap().title, "Budget.xlsx");
        let (done, changed) = docs.door(None, "do", r#"{"commands":[{"name":"ledger.set_cell","args":{"sheet":"Costs","cell":"A1","value":7}}],"label":"Seven"}"#);
        assert!(done.contains("\"ok\":true"), "{done}");
        assert_eq!(changed, Some(ledger));
        let saved = reopen("ledger", docs.get(ledger).unwrap().bus.doc());
        assert!(saved.read(&Json::obj().with("sheet", "Costs").with("cell", "A1")).unwrap().to_string().contains('7'));
        docs.get(ledger).unwrap().bus.undo().unwrap();

        let scroll = docs.add(OpenDoc::new("scroll", open_bytes("scroll", invented_docx(), Fonts::Empty).unwrap(), None));
        assert_eq!(docs.get(scroll).unwrap().ext, "docx");
        docs.door(Some(scroll), "do", r#"{"commands":[{"name":"scroll.set_paragraph_text","args":{"index":0,"text":"Dear Lyra"}}]}"#);
        let saved = reopen("scroll", docs.get(scroll).unwrap().bus.doc());
        assert_eq!(saved.read(&Json::obj().with("paragraph", 0)).unwrap().get("text").and_then(Json::as_str), Some("Dear Lyra"));

        let original = invented_pdf();
        let folio = docs.add(OpenDoc::new("folio", open_bytes("folio", original.clone(), Fonts::Empty).unwrap(), Some(PathBuf::from("notes.pdf"))));
        let (done, _) = docs.door(Some(folio), "do", r#"{"commands":[{"name":"folio.set_title","args":{"title":"Kept notes"}},{"name":"folio.add_note","args":{"page":0,"x":20,"y":20,"text":"Look here"}}]}"#);
        assert!(done.contains("\"ok\":true"), "{done}");
        let bytes = docs.get(folio).unwrap().bus.doc().save().unwrap();
        assert!(bytes.len() > original.len() && bytes.starts_with(&original), "a PDF saves as an appended update");
        let saved = open_bytes("folio", bytes, Fonts::Empty).unwrap();
        assert!(saved.describe().to_string().contains("Kept notes"));
        assert!(docs.history(folio).unwrap().contains("\"origin\":\"ai\""));

        docs.close(folio);
        assert!(docs.get(folio).is_err());
        assert!(docs.door(None, "describe", "").0.contains("No document is open"), "the closed one was current");
    }

    #[test]
    fn saving_writes_the_file_whole_and_keeps_the_version_before() {
        let dir = temp("save");
        let file = dir.join("Report.docx");
        assert!(save_place("docx", &file).is_ok());
        assert_eq!(save_place("docm", &file).unwrap_err(), "Save it as a .docm file.");
        assert_eq!(save_place("pdf", &dir.join("Report")).unwrap_err(), "Save it as a .pdf file.");
        write_document(&file, b"first").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"first");
        assert!(!dir.join("Report.docx.bak").exists(), "nothing was there before");
        write_document(&file, b"second").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"second");
        assert_eq!(std::fs::read(dir.join("Report.docx.bak")).unwrap(), b"first", "saving over a file keeps the one before");
        assert!(!dir.join("Report.docx.scriptorium-part").exists());
        assert!(write_document(&dir.join("no-folder").join("x.docx"), b"x").unwrap_err().contains("could not be saved"));
        let opened = OpenDoc::new("folio", make_blank("folio", Fonts::Empty).unwrap().1, None);
        assert_eq!((opened.title.as_str(), opened.ext.as_str(), opened.path.is_none()), ("Untitled", "pdf", true));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
