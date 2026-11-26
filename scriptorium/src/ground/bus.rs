//! The command bus: every function is a command. Buttons, shortcuts and the AI's chat lines all
//! dispatch the same named command (`"<editor>.<verb>"`) with JSON args, and the command log is the
//! undo history. The AI's door has four verbs - `describe`, `read`, `do`, `shot` - and no way around
//! the commands.
//!
//! The JSON this bus speaks lives here too, in [`json`] (a small RFC 8259 value, parser and writer).

use std::fmt;

use super::display::DisplayList;
use super::edit::{self, Editing};
use self::json::Json;

// ---------------------------------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------------------------------

/// A small JSON value with an RFC 8259 parser and a compact writer. Objects keep their keys in order
/// (a `Vec` of pairs); a parsed object may hold a key twice, and [`Json::get`] then answers the last.
pub mod json {
    use std::fmt;

    /// How deep arrays and objects may nest before the parser refuses (stack safety).
    pub const MAX_DEPTH: usize = 128;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Json {
        Null,
        Bool(bool),
        Num(f64),
        Str(String),
        Arr(Vec<Json>),
        Obj(Vec<(String, Json)>),
    }

    /// Parse one JSON text (surrounding whitespace allowed, nothing else after it).
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser { src: text, b: text.as_bytes(), i: 0, depth: 0 };
        p.ws();
        let v = p.value()?;
        p.ws();
        if p.i != p.b.len() {
            return Err(p.err("unexpected text after the value"));
        }
        Ok(v)
    }

    impl Json {
        /// Same as [`parse`].
        pub fn parse(text: &str) -> Result<Json, String> {
            parse(text)
        }

        // --- builders ---

        /// An empty object.
        pub fn obj() -> Json {
            Json::Obj(Vec::new())
        }

        /// An empty array.
        pub fn arr() -> Json {
            Json::Arr(Vec::new())
        }

        /// Builder: this object with `key` set to `value` (replacing an existing key). A non-object
        /// becomes an object first.
        pub fn with(mut self, key: &str, value: impl Into<Json>) -> Json {
            self.set(key, value);
            self
        }

        /// Set `key` on an object (replacing an existing key). A non-object becomes an object first.
        pub fn set(&mut self, key: &str, value: impl Into<Json>) {
            let value = value.into();
            if !matches!(self, Json::Obj(_)) {
                *self = Json::obj();
            }
            if let Json::Obj(pairs) = self {
                if let Some(slot) = pairs.iter_mut().rev().find(|(k, _)| k == key) {
                    slot.1 = value;
                } else {
                    pairs.push((key.to_string(), value));
                }
            }
        }

        /// Builder: this array with `value` pushed. A non-array becomes an array first.
        pub fn item(mut self, value: impl Into<Json>) -> Json {
            self.push(value);
            self
        }

        /// Push onto an array. A non-array becomes an array first.
        pub fn push(&mut self, value: impl Into<Json>) {
            let value = value.into();
            if !matches!(self, Json::Arr(_)) {
                *self = Json::arr();
            }
            if let Json::Arr(items) = self {
                items.push(value);
            }
        }

        // --- getters ---

        /// The value under `key` of an object (the last one, when a parsed object repeats a key).
        pub fn get(&self, key: &str) -> Option<&Json> {
            match self {
                Json::Obj(pairs) => pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }

        /// The element at `index` of an array.
        pub fn at(&self, index: usize) -> Option<&Json> {
            match self {
                Json::Arr(items) => items.get(index),
                _ => None,
            }
        }

        pub fn is_null(&self) -> bool {
            matches!(self, Json::Null)
        }

        pub fn as_str(&self) -> Option<&str> {
            match self {
                Json::Str(s) => Some(s),
                _ => None,
            }
        }

        pub fn as_f64(&self) -> Option<f64> {
            match self {
                Json::Num(n) => Some(*n),
                _ => None,
            }
        }

        /// A number that is a whole value within +-2^53 (the exact-integer range of a double).
        pub fn as_i64(&self) -> Option<i64> {
            match self {
                Json::Num(n) if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_992.0 => Some(*n as i64),
                _ => None,
            }
        }

        /// A whole, non-negative number (an index or a count).
        pub fn as_usize(&self) -> Option<usize> {
            self.as_i64().filter(|n| *n >= 0).map(|n| n as usize)
        }

        pub fn as_bool(&self) -> Option<bool> {
            match self {
                Json::Bool(b) => Some(*b),
                _ => None,
            }
        }

        pub fn as_arr(&self) -> Option<&[Json]> {
            match self {
                Json::Arr(items) => Some(items),
                _ => None,
            }
        }

        pub fn as_obj(&self) -> Option<&[(String, Json)]> {
            match self {
                Json::Obj(pairs) => Some(pairs),
                _ => None,
            }
        }
    }

    // --- conversions (builders) ---

    impl From<bool> for Json {
        fn from(v: bool) -> Json {
            Json::Bool(v)
        }
    }
    impl From<f64> for Json {
        fn from(v: f64) -> Json {
            Json::Num(v)
        }
    }
    impl From<f32> for Json {
        fn from(v: f32) -> Json {
            Json::Num(v as f64)
        }
    }
    impl From<i32> for Json {
        fn from(v: i32) -> Json {
            Json::Num(v as f64)
        }
    }
    impl From<i64> for Json {
        fn from(v: i64) -> Json {
            Json::Num(v as f64)
        }
    }
    impl From<u32> for Json {
        fn from(v: u32) -> Json {
            Json::Num(v as f64)
        }
    }
    impl From<u64> for Json {
        fn from(v: u64) -> Json {
            Json::Num(v as f64)
        }
    }
    impl From<usize> for Json {
        fn from(v: usize) -> Json {
            Json::Num(v as f64)
        }
    }
    impl From<&str> for Json {
        fn from(v: &str) -> Json {
            Json::Str(v.to_string())
        }
    }
    impl From<String> for Json {
        fn from(v: String) -> Json {
            Json::Str(v)
        }
    }
    impl From<&String> for Json {
        fn from(v: &String) -> Json {
            Json::Str(v.clone())
        }
    }
    impl From<Vec<Json>> for Json {
        fn from(v: Vec<Json>) -> Json {
            Json::Arr(v)
        }
    }
    impl<T: Into<Json>> From<Option<T>> for Json {
        fn from(v: Option<T>) -> Json {
            match v {
                Some(v) => v.into(),
                None => Json::Null,
            }
        }
    }

    // --- writer ---

    /// Compact JSON text (`to_string()`). Non-finite numbers are written as `null`.
    impl fmt::Display for Json {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let mut out = String::new();
            write_value(&mut out, self);
            f.write_str(&out)
        }
    }

    fn write_value(out: &mut String, v: &Json) {
        match v {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Num(n) => write_num(out, *n),
            Json::Str(s) => write_str(out, s),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_value(out, item);
                }
                out.push(']');
            }
            Json::Obj(pairs) => {
                out.push('{');
                for (i, (k, item)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(out, k);
                    out.push(':');
                    write_value(out, item);
                }
                out.push('}');
            }
        }
    }

    /// Write a number as JSON: whole values without a fraction, very large or tiny ones in exponent
    /// form, the shortest round-trip digits otherwise; NaN and infinities as `null`.
    pub fn write_num(out: &mut String, n: f64) {
        use std::fmt::Write;
        if !n.is_finite() {
            out.push_str("null");
        } else if n.fract() == 0.0 && n.abs() < 1e15 {
            let _ = write!(out, "{}", n as i64);
        } else if n.abs() >= 1e21 || n.abs() < 1e-6 {
            let _ = write!(out, "{:e}", n);
        } else {
            let _ = write!(out, "{}", n);
        }
    }

    /// Write a quoted, escaped JSON string. U+2028 and U+2029 are escaped too, so the text is also a
    /// safe JavaScript literal.
    pub fn write_str(out: &mut String, s: &str) {
        use std::fmt::Write;
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                    let _ = write!(out, "\\u{:04x}", c as u32);
                }
                c => out.push(c),
            }
        }
        out.push('"');
    }

    // --- parser ---

    struct Parser<'a> {
        src: &'a str,
        b: &'a [u8],
        i: usize,
        depth: usize,
    }

    impl<'a> Parser<'a> {
        fn err(&self, msg: &str) -> String {
            format!("json: {} at byte {}", msg, self.i)
        }

        fn ws(&mut self) {
            while let Some(&c) = self.b.get(self.i) {
                if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                    self.i += 1;
                } else {
                    break;
                }
            }
        }

        fn literal(&mut self, word: &str, v: Json) -> Result<Json, String> {
            if self.b[self.i..].starts_with(word.as_bytes()) {
                self.i += word.len();
                Ok(v)
            } else {
                Err(self.err("unknown word"))
            }
        }

        fn value(&mut self) -> Result<Json, String> {
            match self.b.get(self.i) {
                None => Err(self.err("expected a value, found the end")),
                Some(b'n') => self.literal("null", Json::Null),
                Some(b't') => self.literal("true", Json::Bool(true)),
                Some(b'f') => self.literal("false", Json::Bool(false)),
                Some(b'"') => self.string().map(Json::Str),
                Some(b'[') => self.array(),
                Some(b'{') => self.object(),
                Some(b'-') | Some(b'0'..=b'9') => self.number(),
                Some(_) => Err(self.err("expected a value")),
            }
        }

        fn enter(&mut self) -> Result<(), String> {
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                Err(self.err("nested too deep"))
            } else {
                Ok(())
            }
        }

        fn array(&mut self) -> Result<Json, String> {
            self.enter()?;
            self.i += 1; // [
            let mut items = Vec::new();
            self.ws();
            if self.b.get(self.i) == Some(&b']') {
                self.i += 1;
                self.depth -= 1;
                return Ok(Json::Arr(items));
            }
            loop {
                self.ws();
                items.push(self.value()?);
                self.ws();
                match self.b.get(self.i) {
                    Some(b',') => self.i += 1,
                    Some(b']') => {
                        self.i += 1;
                        break;
                    }
                    _ => return Err(self.err("expected ',' or ']'")),
                }
            }
            self.depth -= 1;
            Ok(Json::Arr(items))
        }

        fn object(&mut self) -> Result<Json, String> {
            self.enter()?;
            self.i += 1; // {
            let mut pairs = Vec::new();
            self.ws();
            if self.b.get(self.i) == Some(&b'}') {
                self.i += 1;
                self.depth -= 1;
                return Ok(Json::Obj(pairs));
            }
            loop {
                self.ws();
                if self.b.get(self.i) != Some(&b'"') {
                    return Err(self.err("expected a string key"));
                }
                let key = self.string()?;
                self.ws();
                if self.b.get(self.i) != Some(&b':') {
                    return Err(self.err("expected ':'"));
                }
                self.i += 1;
                self.ws();
                let v = self.value()?;
                pairs.push((key, v));
                self.ws();
                match self.b.get(self.i) {
                    Some(b',') => self.i += 1,
                    Some(b'}') => {
                        self.i += 1;
                        break;
                    }
                    _ => return Err(self.err("expected ',' or '}'")),
                }
            }
            self.depth -= 1;
            Ok(Json::Obj(pairs))
        }

        fn digits(&mut self) -> usize {
            let start = self.i;
            while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            self.i - start
        }

        fn number(&mut self) -> Result<Json, String> {
            let start = self.i;
            if self.b.get(self.i) == Some(&b'-') {
                self.i += 1;
            }
            match self.b.get(self.i) {
                Some(b'0') => {
                    self.i += 1;
                    if matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                        return Err(self.err("leading zero in a number"));
                    }
                }
                Some(b'1'..=b'9') => {
                    self.digits();
                }
                _ => return Err(self.err("expected a digit")),
            }
            if self.b.get(self.i) == Some(&b'.') {
                self.i += 1;
                if self.digits() == 0 {
                    return Err(self.err("expected a digit after '.'"));
                }
            }
            if matches!(self.b.get(self.i), Some(b'e') | Some(b'E')) {
                self.i += 1;
                if matches!(self.b.get(self.i), Some(b'+') | Some(b'-')) {
                    self.i += 1;
                }
                if self.digits() == 0 {
                    return Err(self.err("expected a digit in the exponent"));
                }
            }
            let n: f64 = self.src[start..self.i].parse().map_err(|_| self.err("bad number"))?;
            if !n.is_finite() {
                return Err(self.err("number out of range"));
            }
            Ok(Json::Num(n))
        }

        fn hex4(&mut self) -> Result<u32, String> {
            let mut v = 0u32;
            for _ in 0..4 {
                let c = *self.b.get(self.i).ok_or_else(|| self.err("short \\u escape"))?;
                let d = (c as char).to_digit(16).ok_or_else(|| self.err("bad hex digit in \\u escape"))?;
                v = v * 16 + d;
                self.i += 1;
            }
            Ok(v)
        }

        fn string(&mut self) -> Result<String, String> {
            self.i += 1; // opening quote
            let mut out: Vec<u8> = Vec::new();
            loop {
                let c = *self.b.get(self.i).ok_or_else(|| self.err("unterminated string"))?;
                match c {
                    b'"' => {
                        self.i += 1;
                        break;
                    }
                    b'\\' => {
                        self.i += 1;
                        let e = *self.b.get(self.i).ok_or_else(|| self.err("unterminated escape"))?;
                        self.i += 1;
                        let ch = match e {
                            b'"' => '"',
                            b'\\' => '\\',
                            b'/' => '/',
                            b'b' => '\u{8}',
                            b'f' => '\u{c}',
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'u' => {
                                let hi = self.hex4()?;
                                let code = if (0xD800..0xDC00).contains(&hi) {
                                    if self.b.get(self.i) != Some(&b'\\') || self.b.get(self.i + 1) != Some(&b'u') {
                                        return Err(self.err("lone high surrogate"));
                                    }
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&lo) {
                                        return Err(self.err("bad low surrogate"));
                                    }
                                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                } else if (0xDC00..0xE000).contains(&hi) {
                                    return Err(self.err("lone low surrogate"));
                                } else {
                                    hi
                                };
                                char::from_u32(code).ok_or_else(|| self.err("bad code point"))?
                            }
                            _ => return Err(self.err("unknown escape")),
                        };
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    c if c < 0x20 => return Err(self.err("control character in a string")),
                    _ => {
                        // Copy a run of plain bytes; the input is a &str, so the run is valid UTF-8.
                        let start = self.i;
                        while let Some(&c) = self.b.get(self.i) {
                            if c == b'"' || c == b'\\' || c < 0x20 {
                                break;
                            }
                            self.i += 1;
                        }
                        out.extend_from_slice(&self.b[start..self.i]);
                    }
                }
            }
            String::from_utf8(out).map_err(|_| self.err("bad UTF-8 in a string"))
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Commands, documents, errors
// ---------------------------------------------------------------------------------------------------

/// One named action with JSON args: `"<editor>.<verb>"`, e.g. `"ledger.set_cell"`.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub name: String,
    pub args: Json,
}

impl Command {
    pub fn new(name: impl Into<String>, args: Json) -> Command {
        Command { name: name.into(), args }
    }

    /// `{"name":..,"args":..}`
    pub fn to_json(&self) -> Json {
        Json::obj().with("name", self.name.as_str()).with("args", self.args.clone())
    }

    /// Read `{"name":"..","args":{..}}`; missing args mean `{}`.
    pub fn from_json(v: &Json) -> Result<Command, BusError> {
        let name = v
            .get("name")
            .and_then(Json::as_str)
            .ok_or_else(|| BusError::BadArgs { command: String::new(), message: "each command needs a \"name\" string".into() })?;
        let args = match v.get("args") {
            None | Some(Json::Null) => Json::obj(),
            Some(a) => a.clone(),
        };
        Ok(Command::new(name, args))
    }
}

/// What a document says about one of its commands (for the AI, the palette and the docs).
#[derive(Debug, Clone, PartialEq)]
pub struct CommandSpec {
    pub name: String,
    pub summary: String,
    /// The args schema, as plain text, e.g. `{ "cell": "A1", "value": string|number }`.
    pub args: String,
}

impl CommandSpec {
    pub fn new(name: impl Into<String>, summary: impl Into<String>, args: impl Into<String>) -> CommandSpec {
        CommandSpec { name: name.into(), summary: summary.into(), args: args.into() }
    }

    pub fn to_json(&self) -> Json {
        Json::obj().with("name", self.name.as_str()).with("summary", self.summary.as_str()).with("args", self.args.as_str())
    }
}

/// What applying one command changed: the inverse command(s) that undo it (applied in the order
/// given) and the pages that need repainting.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Change {
    pub undo: Vec<Command>,
    pub pages_touched: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BusError {
    /// No such command; `known` lists the ones that exist.
    UnknownCommand { name: String, known: Vec<String> },
    /// The command exists but its args are wrong.
    BadArgs { command: String, message: String },
    /// The command was understood but could not be done (out of range, locked, ...).
    Failed(String),
    /// The door has no such verb.
    UnknownVerb(String),
    NothingToUndo,
    NothingToRedo,
}

impl BusError {
    pub fn bad_args(command: &str, message: impl Into<String>) -> BusError {
        BusError::BadArgs { command: command.to_string(), message: message.into() }
    }

    pub fn failed(message: impl Into<String>) -> BusError {
        BusError::Failed(message.into())
    }

    /// For a document's `apply`: refuse a name it does not know (the bus fills in the known list).
    pub fn unknown(name: &str) -> BusError {
        BusError::UnknownCommand { name: name.to_string(), known: Vec::new() }
    }
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BusError::UnknownCommand { name, known } => {
                write!(f, "unknown command '{}'", name)?;
                if !known.is_empty() {
                    write!(f, "; the commands are: {}", known.join(", "))?;
                }
                Ok(())
            }
            BusError::BadArgs { command, message } if command.is_empty() => write!(f, "bad args: {}", message),
            BusError::BadArgs { command, message } => write!(f, "bad args for '{}': {}", command, message),
            BusError::Failed(m) => write!(f, "{}", m),
            BusError::UnknownVerb(v) => {
                write!(f, "unknown verb '{}'; the door's verbs are: {}", v, DOOR_VERBS.join(", "))
            }
            BusError::NothingToUndo => write!(f, "nothing to undo"),
            BusError::NothingToRedo => write!(f, "nothing to redo"),
        }
    }
}

impl std::error::Error for BusError {}

/// A document the bus can drive: Scroll, Ledger, Stage or Folio (or a test double).
pub trait Document {
    /// The editor's short name, also the command prefix: `"scroll"`, `"ledger"`, `"stage"`, `"folio"`.
    fn kind(&self) -> &'static str;
    /// Apply one command, returning how to undo it.
    fn apply(&mut self, cmd: &Command) -> Result<Change, BusError>;
    /// A summary of the document for the AI (structure, sizes, names).
    fn describe(&self) -> Json;
    /// Read part of the document (`what` is the editor's own query, e.g. `{"range":"A1:C9"}`).
    fn read(&self, what: &Json) -> Result<Json, BusError>;
    /// The display list of one page.
    fn display(&self, page: usize) -> super::display::DisplayList;
    fn page_count(&self) -> usize;
    /// Every command this document accepts (the only ones the bus lets through from users and the AI).
    fn commands(&self) -> Vec<CommandSpec>;
    /// The fonts and images the display lists refer to, when the document keeps them.
    fn resources(&self) -> Option<&super::display::Resources> {
        None
    }
    /// The caret, the selection and the copy, once the document can be edited by hand (see
    /// [`super::edit`]). An editor answers `Some(self)` from its own `impl Editing`.
    fn editing(&self) -> Option<&dyn Editing> {
        None
    }
    /// The faces (and original JPEGs) behind the display lists' ids, for export and print as real,
    /// searchable PDF text. Without them [`super::export`] draws each glyph from its outline in
    /// [`Document::resources`] (it looks the same, but copies as no text).
    fn pdf_resources(&self) -> Option<super::pdfw::PageResources> {
        None
    }
}

// ---------------------------------------------------------------------------------------------------
// The bus
// ---------------------------------------------------------------------------------------------------

/// Who asked for a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    User,
    Ai,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::User => "user",
            Origin::Ai => "ai",
        }
    }
}

/// One undoable step: one command or a group, all-or-nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub id: u64,
    pub origin: Origin,
    pub label: String,
    /// The commands as done (redo replays these).
    pub commands: Vec<Command>,
    /// The inverse commands, in the order to apply them.
    pub undo: Vec<Command>,
    pub pages_touched: Vec<usize>,
}

/// One line of the history, for the UI ("the AI's change" with an undo beside it).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub id: u64,
    pub origin: Origin,
    pub label: String,
    pub command_count: usize,
    /// True for steps that were undone and wait on the redo stack.
    pub undone: bool,
}

impl HistoryEntry {
    pub fn to_json(&self) -> Json {
        Json::obj()
            .with("id", self.id)
            .with("origin", self.origin.as_str())
            .with("label", self.label.as_str())
            .with("commands", self.command_count)
            .with("undone", self.undone)
    }
}

/// What a do, undo or redo did.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub step: u64,
    pub pages_touched: Vec<usize>,
}

/// How many steps the undo history keeps unless told otherwise.
pub const DEFAULT_HISTORY_CAP: usize = 500;

/// The door's verbs.
pub const DOOR_VERBS: [&str; 4] = ["describe", "read", "do", "shot"];

pub struct Bus<D: Document> {
    doc: D,
    undo: Vec<Step>,
    redo: Vec<Step>,
    cap: usize,
    next_id: u64,
}

impl<D: Document> Bus<D> {
    pub fn new(doc: D) -> Bus<D> {
        Bus::with_cap(doc, DEFAULT_HISTORY_CAP)
    }

    /// A bus that keeps at most `cap` undo steps (at least 1).
    pub fn with_cap(doc: D, cap: usize) -> Bus<D> {
        Bus { doc, undo: Vec::new(), redo: Vec::new(), cap: cap.max(1), next_id: 1 }
    }

    pub fn doc(&self) -> &D {
        &self.doc
    }

    pub fn into_doc(self) -> D {
        self.doc
    }

    pub fn history_cap(&self) -> usize {
        self.cap
    }

    pub fn set_history_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        self.trim();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The steps oldest first, then the undone ones (next redo first), each marked with its origin.
    pub fn history(&self) -> Vec<HistoryEntry> {
        let entry = |s: &Step, undone: bool| HistoryEntry {
            id: s.id,
            origin: s.origin,
            label: s.label.clone(),
            command_count: s.commands.len(),
            undone,
        };
        let mut out: Vec<HistoryEntry> = self.undo.iter().map(|s| entry(s, false)).collect();
        out.extend(self.redo.iter().rev().map(|s| entry(s, true)));
        out
    }

    /// The step the next undo would take back.
    pub fn last_step(&self) -> Option<&Step> {
        self.undo.last()
    }

    /// Do one command as one undo step.
    pub fn do_one(&mut self, cmd: Command, origin: Origin) -> Result<Done, BusError> {
        self.do_group_labelled(vec![cmd], origin, None)
    }

    /// Do several commands as ONE undo step, all or nothing: if one fails, those already applied are
    /// undone in reverse and the error is returned. Commands that changed nothing (no inverse, no page
    /// touched - a find) leave no step: the answer's `step` is 0.
    pub fn do_group(&mut self, cmds: Vec<Command>, origin: Origin) -> Result<Done, BusError> {
        self.do_group_labelled(cmds, origin, None)
    }

    /// [`Bus::do_group`] with a label for the history (default: built from the command names).
    pub fn do_group_labelled(&mut self, cmds: Vec<Command>, origin: Origin, label: Option<&str>) -> Result<Done, BusError> {
        if cmds.is_empty() {
            return Err(BusError::bad_args("", "no commands to do"));
        }
        let specs = self.doc.commands();
        for c in &cmds {
            if !specs.iter().any(|s| s.name == c.name) {
                return Err(BusError::UnknownCommand { name: c.name.clone(), known: specs.iter().map(|s| s.name.clone()).collect() });
            }
        }
        let (undo, pages) = self.apply_all(&cmds)?;
        if undo.is_empty() && pages.is_empty() {
            // Nothing changed (a find, a command that met nothing to do): no undo step, and the redo
            // stack stays as it was.
            return Ok(Done { step: 0, pages_touched: Vec::new() });
        }
        let id = self.next_id;
        self.next_id += 1;
        let label = match label {
            Some(l) if !l.trim().is_empty() => l.to_string(),
            _ => auto_label(&cmds),
        };
        self.undo.push(Step { id, origin, label, commands: cmds, undo, pages_touched: pages.clone() });
        self.redo.clear();
        self.trim();
        Ok(Done { step: id, pages_touched: pages })
    }

    /// Take back the last step.
    pub fn undo(&mut self) -> Result<Done, BusError> {
        let step = self.undo.pop().ok_or(BusError::NothingToUndo)?;
        match self.apply_all(&step.undo) {
            Ok((_, pages)) => {
                let pages = merge_pages(&step.pages_touched, &pages);
                let done = Done { step: step.id, pages_touched: pages };
                self.redo.push(step);
                Ok(done)
            }
            Err(e) => {
                self.undo.push(step);
                Err(e)
            }
        }
    }

    /// Do again the last undone step.
    pub fn redo(&mut self) -> Result<Done, BusError> {
        let mut step = self.redo.pop().ok_or(BusError::NothingToRedo)?;
        match self.apply_all(&step.commands) {
            Ok((undo, pages)) => {
                step.undo = undo;
                step.pages_touched = pages.clone();
                let done = Done { step: step.id, pages_touched: pages };
                self.undo.push(step);
                self.trim();
                Ok(done)
            }
            Err(e) => {
                self.redo.push(step);
                Err(e)
            }
        }
    }

    /// Apply commands atomically. Returns the inverse list (in apply order for undoing) and the
    /// pages touched; on failure rolls back what was applied and returns the error.
    fn apply_all(&mut self, cmds: &[Command]) -> Result<(Vec<Command>, Vec<usize>), BusError> {
        let mut changes: Vec<Change> = Vec::new();
        for c in cmds {
            match self.doc.apply(c) {
                Ok(ch) => changes.push(ch),
                Err(e) => {
                    for ch in changes.iter().rev() {
                        for u in &ch.undo {
                            // Best effort: an inverse the document itself produced should apply.
                            let _ = self.doc.apply(u);
                        }
                    }
                    let e = match e {
                        BusError::UnknownCommand { name, known } if known.is_empty() => BusError::UnknownCommand {
                            name,
                            known: self.doc.commands().into_iter().map(|s| s.name).collect(),
                        },
                        e => e,
                    };
                    return Err(e);
                }
            }
        }
        let mut undo = Vec::new();
        let mut pages = Vec::new();
        for ch in changes.iter().rev() {
            undo.extend(ch.undo.iter().cloned());
        }
        for ch in &changes {
            pages = merge_pages(&pages, &ch.pages_touched);
        }
        Ok((undo, pages))
    }

    fn trim(&mut self) {
        if self.undo.len() > self.cap {
            let extra = self.undo.len() - self.cap;
            self.undo.drain(..extra);
        }
        if self.redo.len() > self.cap {
            let extra = self.redo.len() - self.cap;
            self.redo.drain(..extra);
        }
    }

    // --- the AI's door ---

    /// The AI's door, four verbs and nothing else:
    /// - `describe` -> `{ "kind", "pages", "editable", "document": doc.describe(), "commands": [{name,summary,args}] }`
    ///   (`editable`: the document takes a caret and a selection, see [`super::edit`])
    /// - `read` -> `doc.read(args)`
    /// - `do` -> args `{ "commands": [{ "name", "args" }, ...], "label"?: string }`, applied as ONE
    ///   group with `Origin::Ai`; answers `{ "ok": true, "step", "pages" }`
    /// - `shot` -> args `{ "page"?: n }` (default 0); answers `{ "page", "width", "height", "display" }`
    ///   where `display` is the page's display list (the host turns it into a picture)
    pub fn door(&mut self, verb: &str, args: &Json) -> Result<Json, BusError> {
        match verb {
            "describe" => Ok(Json::obj()
                .with("kind", self.doc.kind())
                .with("pages", self.doc.page_count())
                .with("editable", self.doc.editing().is_some())
                .with("document", self.doc.describe())
                .with("commands", Json::Arr(self.doc.commands().iter().map(CommandSpec::to_json).collect()))),
            "read" => self.doc.read(args),
            "do" => {
                let list = args
                    .get("commands")
                    .and_then(Json::as_arr)
                    .ok_or_else(|| BusError::bad_args("", "do takes { \"commands\": [ { \"name\": .., \"args\": {..} }, ... ] }"))?;
                let cmds = list.iter().map(Command::from_json).collect::<Result<Vec<_>, _>>()?;
                let label = args.get("label").and_then(Json::as_str);
                let done = self.do_group_labelled(cmds, Origin::Ai, label)?;
                Ok(Json::obj()
                    .with("ok", true)
                    .with("step", done.step)
                    .with("pages", Json::Arr(done.pages_touched.iter().map(|p| Json::from(*p)).collect())))
            }
            "shot" => {
                let page = match args.get("page") {
                    None | Some(Json::Null) => 0,
                    Some(p) => p.as_usize().ok_or_else(|| BusError::bad_args("", "shot's \"page\" is a whole number from 0"))?,
                };
                let count = self.doc.page_count();
                if page >= count {
                    return Err(BusError::failed(format!("no page {} (the document has {} page{})", page, count, if count == 1 { "" } else { "s" })));
                }
                let dl: DisplayList = self.doc.display(page);
                Ok(Json::obj()
                    .with("page", page)
                    .with("width", dl.width)
                    .with("height", dl.height)
                    .with("display", dl.to_json_value()))
            }
            other => Err(BusError::UnknownVerb(other.to_string())),
        }
    }

    // --- the editing door (the caret, the selection, the copy; never a change) ---

    /// The document's editing, when it can be edited by hand.
    pub fn editing(&self) -> Option<&dyn Editing> {
        self.doc.editing()
    }

    /// The editing door ([`edit::door`]): `hit`, `caret`, `selection`, `state`, `copy`, `move`.
    pub fn edit(&self, verb: &str, args: &Json) -> Result<Json, BusError> {
        edit::door(&self.doc, verb, args)
    }

    /// [`Bus::edit`] for a host edge that speaks text; errors as `{"ok":false,"error":".."}`.
    pub fn edit_json(&self, verb: &str, args_text: &str) -> String {
        edit::door_json(&self.doc, verb, args_text)
    }

    /// The member's own command, answered for the page: `{"ok":true,"step","pages","selection"}` where
    /// `selection` is where the document says the selection landed (`null` when it does not say).
    pub fn do_user(&mut self, cmd: Command) -> Result<Json, BusError> {
        let probe = cmd.clone();
        let done = self.do_one(cmd, Origin::User)?;
        Ok(Json::obj()
            .with("ok", true)
            .with("step", done.step)
            .with("pages", Json::Arr(done.pages_touched.iter().map(|p| Json::from(*p)).collect()))
            .with("selection", edit::landed_json(&self.doc, &probe)))
    }

    /// The door for a host edge that speaks text: args as JSON text in, JSON text out. Errors come
    /// back as `{"ok":false,"error":".."}`.
    pub fn door_json(&mut self, verb: &str, args_text: &str) -> String {
        let args = if args_text.trim().is_empty() {
            Ok(Json::obj())
        } else {
            json::parse(args_text).map_err(|e| BusError::bad_args("", e))
        };
        match args.and_then(|a| self.door(verb, &a)) {
            Ok(v) => v.to_string(),
            Err(e) => Json::obj().with("ok", false).with("error", e.to_string()).to_string(),
        }
    }
}

fn auto_label(cmds: &[Command]) -> String {
    let first = &cmds[0].name;
    if cmds.len() == 1 {
        first.clone()
    } else if cmds.iter().all(|c| &c.name == first) {
        format!("{} x{}", first, cmds.len())
    } else {
        format!("{} (+{} more)", first, cmds.len() - 1)
    }
}

fn merge_pages(a: &[usize], b: &[usize]) -> Vec<usize> {
    let mut out: Vec<usize> = a.iter().chain(b.iter()).copied().collect();
    out.sort_unstable();
    out.dedup();
    out
}

// ---------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::json::{parse, Json};
    use super::*;
    use crate::ground::display::{Color, DisplayList, Item, Paint, PathOp};

    // --- JSON ---

    #[test]
    fn json_parses_primitives_and_nesting() {
        assert_eq!(parse("null").unwrap(), Json::Null);
        assert_eq!(parse(" true ").unwrap(), Json::Bool(true));
        assert_eq!(parse("false").unwrap(), Json::Bool(false));
        assert_eq!(parse("-12.5e1").unwrap(), Json::Num(-125.0));
        assert_eq!(parse("0").unwrap(), Json::Num(0.0));
        let v = parse(r#" { "a" : [1, 2, {"b": "c"}], "d": {} , "e": [] } "#).unwrap();
        assert_eq!(v.get("a").unwrap().at(2).unwrap().get("b").unwrap().as_str(), Some("c"));
        assert_eq!(v.get("d"), Some(&Json::obj()));
        assert_eq!(v.get("e").unwrap().as_arr().unwrap().len(), 0);
    }

    #[test]
    fn json_string_escapes_and_surrogates() {
        let v = parse(r#""a\"b\\c\/d\b\f\n\r\t\u00e9\u4e2d\ud83d\ude00""#).unwrap();
        assert_eq!(v.as_str().unwrap(), "a\"b\\c/d\u{8}\u{c}\n\r\t\u{e9}\u{4e2d}\u{1F600}");
        assert_eq!(parse("\"caf\u{e9} \u{1F600}\"").unwrap().as_str(), Some("caf\u{e9} \u{1F600}"));
        assert!(parse(r#""\ud83d""#).is_err());
        assert!(parse(r#""\ud83dx""#).is_err());
        assert!(parse(r#""\ude00""#).is_err());
        assert!(parse(r#""\ud83d\u0041""#).is_err());
        assert!(parse(r#""\x""#).is_err());
        assert!(parse("\"a\nb\"").is_err());
        assert!(parse(r#""abc"#).is_err());
    }

    #[test]
    fn json_rejects_bad_text() {
        for bad in ["", " ", "01", "-", "1.", ".5", "1e", "+1", "[1,]", "{\"a\":1,}", "[1 2]", "{a:1}", "{\"a\" 1}", "tru", "nul", "1 2", "[", "1e400", "NaN", "'a'"] {
            assert!(parse(bad).is_err(), "should refuse {:?}", bad);
        }
    }

    #[test]
    fn json_depth_limit() {
        let ok = "[".repeat(json::MAX_DEPTH) + &"]".repeat(json::MAX_DEPTH);
        assert!(parse(&ok).is_ok());
        let deep = "[".repeat(json::MAX_DEPTH + 1) + &"]".repeat(json::MAX_DEPTH + 1);
        assert!(parse(&deep).unwrap_err().contains("deep"));
        let deep_obj = "{\"a\":".repeat(500) + "1" + &"}".repeat(500);
        assert!(parse(&deep_obj).is_err());
    }

    #[test]
    fn json_writes_compact_and_escaped() {
        let v = Json::obj()
            .with("s", "q\"\\\n\u{1}\u{2028}\u{e9}")
            .with("n", 3)
            .with("f", 0.25)
            .with("neg", -0.0)
            .with("big", 1e22)
            .with("tiny", 1e-9)
            .with("nan", f64::NAN)
            .with("a", Json::arr().item(true).item(Json::Null));
        assert_eq!(
            v.to_string(),
            concat!(r#"{"s":"q\"\\\n\u0001"#, "\\u2028\u{e9}", r#"","n":3,"f":0.25,"neg":0,"big":1e22,"tiny":1e-9,"nan":null,"a":[true,null]}"#)
        );
        // Round trip.
        let text = r#"{"x":[1,2.5,"\u0007",{"y":false}],"z":-7}"#;
        assert_eq!(parse(text).unwrap().to_string(), text);
        let v2 = parse(&v.to_string()).unwrap();
        assert_eq!(v2.get("s"), v.get("s"));
        assert_eq!(v2.get("tiny").unwrap().as_f64(), Some(1e-9));
    }

    #[test]
    fn json_getters_and_builders() {
        let mut v = Json::obj().with("a", 1).with("b", "x").with("a", 2);
        assert_eq!(v.as_obj().unwrap().len(), 2, "with replaces an existing key");
        assert_eq!(v.get("a").unwrap().as_i64(), Some(2));
        v.set("c", Some(1.5));
        v.set("d", None::<f64>);
        assert_eq!(v.get("c").unwrap().as_i64(), None);
        assert_eq!(v.get("c").unwrap().as_f64(), Some(1.5));
        assert!(v.get("d").unwrap().is_null());
        assert_eq!(v.get("b").unwrap().as_f64(), None);
        assert_eq!(Json::from(-3).as_usize(), None);
        assert_eq!(Json::from(4usize).as_usize(), Some(4));
        assert_eq!(Json::Bool(true).as_bool(), Some(true));
        assert_eq!(Json::from(1e300).as_i64(), None);
        // A parsed object with a repeated key answers the last.
        assert_eq!(parse(r#"{"k":1,"k":2}"#).unwrap().get("k").unwrap().as_i64(), Some(2));
        let mut a = Json::Null;
        a.push(1);
        assert_eq!(a.to_string(), "[1]");
    }

    // --- a tiny document: a list of strings ---

    struct Lines {
        lines: Vec<String>,
    }

    impl Document for Lines {
        fn kind(&self) -> &'static str {
            "test"
        }
        fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
            let at = cmd.args.get("at").and_then(Json::as_usize).ok_or_else(|| BusError::bad_args(&cmd.name, "\"at\" is a whole number"))?;
            match cmd.name.as_str() {
                "test.insert" => {
                    let text = cmd.args.get("text").and_then(Json::as_str).ok_or_else(|| BusError::bad_args(&cmd.name, "\"text\" is a string"))?;
                    if at > self.lines.len() {
                        return Err(BusError::failed(format!("no line {}", at)));
                    }
                    self.lines.insert(at, text.to_string());
                    Ok(Change { undo: vec![Command::new("test.delete", Json::obj().with("at", at))], pages_touched: vec![0] })
                }
                "test.delete" => {
                    if at >= self.lines.len() {
                        return Err(BusError::failed(format!("no line {}", at)));
                    }
                    let text = self.lines.remove(at);
                    Ok(Change { undo: vec![Command::new("test.insert", Json::obj().with("at", at).with("text", text))], pages_touched: vec![0] })
                }
                other => Err(BusError::unknown(other)),
            }
        }
        fn describe(&self) -> Json {
            Json::obj().with("lines", self.lines.len())
        }
        fn read(&self, what: &Json) -> Result<Json, BusError> {
            match what.get("line").and_then(Json::as_usize) {
                Some(i) => self.lines.get(i).map(|s| Json::from(s.as_str())).ok_or_else(|| BusError::failed(format!("no line {}", i))),
                None => Ok(Json::Arr(self.lines.iter().map(Json::from).collect())),
            }
        }
        fn display(&self, _page: usize) -> DisplayList {
            let mut dl = DisplayList::new(100.0, 200.0);
            for (i, _) in self.lines.iter().enumerate() {
                let y = i as f64 * 10.0;
                dl.items.push(Item::Path {
                    ops: vec![PathOp::MoveTo(0.0, y), PathOp::LineTo(50.0, y), PathOp::LineTo(50.0, y + 8.0), PathOp::Close],
                    fill: Some(Paint::Solid(Color::BLACK)),
                    stroke: None,
                });
            }
            dl
        }
        fn page_count(&self) -> usize {
            1
        }
        fn commands(&self) -> Vec<CommandSpec> {
            vec![
                CommandSpec::new("test.insert", "Insert a line", r#"{ "at": n, "text": string }"#),
                CommandSpec::new("test.delete", "Delete a line", r#"{ "at": n }"#),
            ]
        }
    }

    fn bus() -> Bus<Lines> {
        Bus::new(Lines { lines: Vec::new() })
    }

    fn ins(at: usize, text: &str) -> Command {
        Command::new("test.insert", Json::obj().with("at", at).with("text", text))
    }

    fn del(at: usize) -> Command {
        Command::new("test.delete", Json::obj().with("at", at))
    }

    #[test]
    fn undo_and_redo() {
        let mut b = bus();
        assert!(!b.can_undo() && !b.can_redo());
        b.do_one(ins(0, "a"), Origin::User).unwrap();
        b.do_one(ins(1, "b"), Origin::User).unwrap();
        b.do_one(del(0), Origin::User).unwrap();
        assert_eq!(b.doc().lines, vec!["b"]);
        b.undo().unwrap();
        assert_eq!(b.doc().lines, vec!["a", "b"]);
        b.undo().unwrap();
        assert_eq!(b.doc().lines, vec!["a"]);
        assert!(b.can_redo());
        b.redo().unwrap();
        assert_eq!(b.doc().lines, vec!["a", "b"]);
        b.undo().unwrap();
        b.undo().unwrap();
        assert!(b.doc().lines.is_empty());
        assert_eq!(b.undo(), Err(BusError::NothingToUndo));
        b.redo().unwrap();
        b.redo().unwrap();
        b.redo().unwrap();
        assert_eq!(b.doc().lines, vec!["b"]);
        assert_eq!(b.redo(), Err(BusError::NothingToRedo));
        // A new step clears the redo stack.
        b.undo().unwrap();
        b.do_one(ins(0, "z"), Origin::User).unwrap();
        assert!(!b.can_redo());
        assert_eq!(b.doc().lines, vec!["z", "a", "b"]);
    }

    #[test]
    fn group_is_one_step_and_atomic() {
        let mut b = bus();
        b.do_one(ins(0, "keep"), Origin::User).unwrap();
        let done = b.do_group(vec![ins(0, "x"), ins(1, "y"), del(2)], Origin::User).unwrap();
        assert_eq!(done.pages_touched, vec![0]);
        assert_eq!(b.doc().lines, vec!["x", "y"]);
        b.undo().unwrap();
        assert_eq!(b.doc().lines, vec!["keep"]);
        b.redo().unwrap();
        assert_eq!(b.doc().lines, vec!["x", "y"]);
        b.undo().unwrap();

        // The third command fails: the first two are taken back, no step is recorded.
        let before = b.history().len();
        let err = b.do_group(vec![ins(0, "p"), del(0), del(9)], Origin::User).unwrap_err();
        assert_eq!(err, BusError::Failed("no line 9".into()));
        assert_eq!(b.doc().lines, vec!["keep"]);
        assert_eq!(b.history().len(), before);
        assert!(b.can_redo(), "a failed group leaves the redo stack alone");
        assert!(b.do_group(vec![], Origin::User).is_err());
    }

    #[test]
    fn history_labels_the_ai() {
        let mut b = bus();
        b.do_one(ins(0, "a"), Origin::User).unwrap();
        b.do_group(vec![ins(1, "b"), ins(2, "c")], Origin::Ai).unwrap();
        b.do_group_labelled(vec![ins(0, "q"), del(0)], Origin::User, Some("Tidy")).unwrap();
        b.undo().unwrap();
        let h = b.history();
        assert_eq!(h.len(), 3);
        assert_eq!((h[0].origin, h[0].label.as_str(), h[0].undone), (Origin::User, "test.insert", false));
        assert_eq!((h[1].origin, h[1].label.as_str(), h[1].command_count), (Origin::Ai, "test.insert x2", 2));
        assert_eq!((h[2].label.as_str(), h[2].undone), ("Tidy", true));
        assert_eq!(h[1].to_json().to_string(), r#"{"id":2,"origin":"ai","label":"test.insert x2","commands":2,"undone":false}"#);
        assert_eq!(b.last_step().unwrap().origin, Origin::Ai);
    }

    #[test]
    fn unknown_command_refused_with_the_list() {
        let mut b = bus();
        let err = b.do_one(Command::new("test.explode", Json::obj()), Origin::User).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("test.explode") && msg.contains("test.insert, test.delete"), "{}", msg);
        // Even inside a group, nothing runs.
        assert!(b.do_group(vec![ins(0, "a"), Command::new("ledger.set_cell", Json::obj())], Origin::Ai).is_err());
        assert!(b.doc().lines.is_empty());
        assert!(!b.can_undo());
    }

    #[test]
    fn history_cap_drops_the_oldest() {
        let mut b = Bus::with_cap(Lines { lines: Vec::new() }, 3);
        for i in 0..5 {
            b.do_one(ins(i, "l"), Origin::User).unwrap();
        }
        let ids: Vec<u64> = b.history().iter().map(|h| h.id).collect();
        assert_eq!(ids, vec![3, 4, 5]);
        for _ in 0..3 {
            b.undo().unwrap();
        }
        assert!(!b.can_undo());
        assert_eq!(b.doc().lines.len(), 2);
        b.set_history_cap(1);
        assert_eq!(b.history().len(), 1);
    }

    #[test]
    fn door_describe_read_do_shot() {
        let mut b = bus();
        let d = b.door("describe", &Json::Null).unwrap();
        assert_eq!(d.get("kind").unwrap().as_str(), Some("test"));
        assert_eq!(d.get("pages").unwrap().as_i64(), Some(1));
        assert_eq!(d.get("document").unwrap().get("lines").unwrap().as_i64(), Some(0));
        assert_eq!(d.get("commands").unwrap().at(1).unwrap().get("name").unwrap().as_str(), Some("test.delete"));

        let args = parse(r#"{"commands":[{"name":"test.insert","args":{"at":0,"text":"hello"}},{"name":"test.insert","args":{"at":1,"text":"world"}}]}"#).unwrap();
        let r = b.door("do", &args).unwrap();
        assert_eq!(r.to_string(), r#"{"ok":true,"step":1,"pages":[0]}"#);
        assert_eq!(b.history()[0].origin, Origin::Ai);
        assert_eq!(b.history()[0].command_count, 2);

        assert_eq!(b.door("read", &Json::obj().with("line", 1)).unwrap().as_str(), Some("world"));
        assert_eq!(b.door("read", &Json::obj()).unwrap().to_string(), r#"["hello","world"]"#);
        assert!(b.door("read", &Json::obj().with("line", 7)).is_err());

        let s = b.door("shot", &Json::obj().with("page", 0)).unwrap();
        assert_eq!(s.get("width").unwrap().as_f64(), Some(100.0));
        assert_eq!(s.get("height").unwrap().as_f64(), Some(200.0));
        assert_eq!(s.get("display").unwrap().get("items").unwrap().as_arr().unwrap().len(), 2);
        assert!(b.door("shot", &Json::obj().with("page", 3)).unwrap_err().to_string().contains("no page 3"));

        // One undo takes back the AI's whole group.
        b.undo().unwrap();
        assert!(b.doc().lines.is_empty());
    }

    #[test]
    fn door_refuses_what_does_not_exist() {
        let mut b = bus();
        let e = b.door("delete_everything", &Json::Null).unwrap_err();
        assert!(e.to_string().contains("describe, read, do, shot"));
        let e = b.door("do", &parse(r#"{"commands":[{"name":"test.nuke"}]}"#).unwrap()).unwrap_err();
        assert!(matches!(e, BusError::UnknownCommand { ref known, .. } if known.len() == 2));
        assert!(b.door("do", &Json::obj()).is_err());
        assert!(b.door("do", &parse(r#"{"commands":[{"args":{}}]}"#).unwrap()).is_err());
        // A failing AI group changes nothing.
        let bad = parse(r#"{"commands":[{"name":"test.insert","args":{"at":0,"text":"a"}},{"name":"test.delete","args":{"at":5}}]}"#).unwrap();
        assert!(b.door("do", &bad).is_err());
        assert!(b.doc().lines.is_empty() && !b.can_undo());
    }

    #[test]
    fn door_json_text_edge() {
        let mut b = bus();
        let out = b.door_json("do", r#"{"commands":[{"name":"test.insert","args":{"at":0,"text":"hi"}}],"label":"Greeting"}"#);
        assert_eq!(out, r#"{"ok":true,"step":1,"pages":[0]}"#);
        assert_eq!(b.history()[0].label, "Greeting");
        let out = b.door_json("read", "");
        assert_eq!(out, r#"["hi"]"#);
        let out = b.door_json("do", "{oops");
        assert!(out.starts_with(r#"{"ok":false,"error":"bad args: json:"#), "{}", out);
        let out = b.door_json("fly", "{}");
        assert!(out.contains("unknown verb 'fly'"));
    }

    // --- the editing seam ---

    #[test]
    fn a_document_is_not_editable_until_it_says_so() {
        let mut b = bus();
        assert!(b.editing().is_none());
        let d = b.door("describe", &Json::Null).unwrap();
        assert_eq!(d.get("editable"), Some(&Json::Bool(false)));
        assert!(b.edit("hit", &parse(r#"{"page":0,"x":1,"y":1}"#).unwrap()).unwrap().is_null());
        assert_eq!(b.edit_json("selection", r#"{"sel":{"anchor":1}}"#), "[]");
        assert!(b.edit_json("nope", "{}").contains("\"ok\":false"));
        // The member's command answers where the selection landed (nowhere said, here).
        let out = b.do_user(ins(0, "a")).unwrap();
        assert_eq!(out.to_string(), r#"{"ok":true,"step":1,"pages":[0],"selection":null}"#);
        assert!(b.do_user(Command::new("test.nuke", Json::obj())).is_err());
    }

    #[test]
    fn a_command_that_changed_nothing_leaves_no_step() {
        /// Knows `quiet.find` (changes nothing) and `quiet.add` (one number up).
        struct Quiet {
            n: usize,
        }
        impl Document for Quiet {
            fn kind(&self) -> &'static str {
                "quiet"
            }
            fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
                match cmd.name.as_str() {
                    "quiet.find" => Ok(Change::default()),
                    "quiet.add" => {
                        self.n += 1;
                        Ok(Change { undo: vec![Command::new("quiet.sub", Json::obj())], pages_touched: vec![0] })
                    }
                    "quiet.sub" => {
                        self.n -= 1;
                        Ok(Change { undo: vec![Command::new("quiet.add", Json::obj())], pages_touched: vec![0] })
                    }
                    other => Err(BusError::unknown(other)),
                }
            }
            fn describe(&self) -> Json {
                Json::obj()
            }
            fn read(&self, _what: &Json) -> Result<Json, BusError> {
                Ok(Json::Null)
            }
            fn display(&self, _page: usize) -> DisplayList {
                DisplayList::new(10.0, 10.0)
            }
            fn page_count(&self) -> usize {
                1
            }
            fn commands(&self) -> Vec<CommandSpec> {
                ["quiet.find", "quiet.add", "quiet.sub"].iter().map(|n| CommandSpec::new(*n, "", "{}")).collect()
            }
        }
        let mut b = Bus::new(Quiet { n: 0 });
        b.do_one(Command::new("quiet.add", Json::obj()), Origin::User).unwrap();
        b.do_one(Command::new("quiet.add", Json::obj()), Origin::User).unwrap();
        b.undo().unwrap();
        assert!(b.can_redo());
        let done = b.do_one(Command::new("quiet.find", Json::obj()), Origin::User).unwrap();
        assert_eq!(done, Done { step: 0, pages_touched: Vec::new() });
        assert_eq!(b.history().len(), 2, "one done, one undone: the find left nothing");
        assert!(b.can_redo(), "a find keeps the redo stack");
        b.redo().unwrap();
        assert_eq!(b.doc().n, 2);
    }
}
