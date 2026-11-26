//! The Ground's XML: a pull reader, a writer and a small DOM, made for Office parts (OOXML `.docx`,
//! `.xlsx`, `.pptx`) whose unknown content must survive a round trip byte for byte.
//!
//! - [`Reader`] pulls [`Event`]s out of a UTF-8 byte slice. Every event carries its exact byte `span`
//!   in the source, so a caller can copy anything it does not understand verbatim
//!   (`writer.raw(&src[span])`), and [`Reader::subtree_span`] hands back the whole range of an unknown
//!   element. Namespaces are tracked on a scope stack ([`Reader::resolve`]). Entities: the five
//!   predefined ones, numeric character references and the internal general entities of a simple
//!   `<!DOCTYPE>` internal subset (never anything external, and expansion is capped).
//! - [`to_utf8`] transcodes a UTF-16 document (BOM or `<?` sniffed) to UTF-8 first.
//! - [`Writer`] builds a `Vec<u8>` with escaping, optional pretty indentation (off by default) and
//!   `raw` for verbatim copies.
//! - [`Element`] / [`Node`] with [`parse_dom`] and [`write_dom`] for convenience; comments, PIs and
//!   CDATA inside the tree are kept as [`Node::Raw`] bytes.
//!
//! Malformed input never panics: it is an [`XmlError`] with line and column.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::rc::Rc;

/// The namespace the `xml:` prefix is always bound to.
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
/// The namespace of `xmlns` / `xmlns:*` attributes themselves.
pub const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
/// An empty attribute list for [`Writer::start`] / [`Writer::empty`].
pub const NO_ATTRS: &[(&str, &str)] = &[];

/// Deepest element nesting the reader accepts (keeps every consumer's recursion safe).
pub const MAX_DEPTH: usize = 4096;
/// Largest replacement text a single DTD entity may expand to.
pub const MAX_ENTITY_LEN: usize = 1 << 20;
/// Total bytes all entity expansions of one document may produce (the billion-laughs cap).
pub const MAX_EXPANSION_TOTAL: usize = 8 << 20;
/// Deepest entity-inside-entity nesting.
const MAX_ENTITY_NESTING: usize = 16;

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

/// What went wrong and where: byte `offset`, 1-based `line` and `column` (column counts characters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlError {
    pub message: String,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "XML error at line {}, column {}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for XmlError {}

fn error_at(src: &[u8], offset: usize, message: impl Into<String>) -> XmlError {
    let offset = offset.min(src.len());
    let before = &src[..offset];
    let line = before.iter().filter(|&&c| c == b'\n').count() + 1;
    let line_start = before.iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1);
    let column = before[line_start..].iter().filter(|&&c| (c & 0xC0) != 0x80).count() + 1;
    XmlError { message: message.into(), offset, line, column }
}

// ---------------------------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------------------------

fn utf16_kind(bytes: &[u8]) -> Option<(bool, usize)> {
    match bytes {
        [0xFF, 0xFE, ..] => Some((true, 2)),
        [0xFE, 0xFF, ..] => Some((false, 2)),
        [b'<', 0, b'?', 0, ..] => Some((true, 0)),
        [0, b'<', 0, b'?', ..] => Some((false, 0)),
        _ => None,
    }
}

/// Returns the document as UTF-8: borrowed when it already is, transcoded when it is UTF-16 (a BOM,
/// or `<?` sniffed without one). A transcoded document's declaration `encoding="UTF-16"` is rewritten
/// to `UTF-8` so the bytes stay honest. Spans of a [`Reader`] refer to the returned bytes.
pub fn to_utf8(bytes: &[u8]) -> Result<Cow<'_, [u8]>, XmlError> {
    let Some((little, skip)) = utf16_kind(bytes) else {
        return Ok(Cow::Borrowed(bytes));
    };
    let body = &bytes[skip..];
    if body.len() % 2 != 0 {
        return Err(error_at(bytes, bytes.len(), "UTF-16 input has an odd number of bytes"));
    }
    let units = body.chunks_exact(2).map(|p| {
        if little {
            u16::from_le_bytes([p[0], p[1]])
        } else {
            u16::from_be_bytes([p[0], p[1]])
        }
    });
    let mut out = String::with_capacity(body.len());
    let mut consumed = 0usize;
    for r in char::decode_utf16(units) {
        match r {
            Ok(c) => {
                consumed += c.len_utf16();
                out.push(c);
            }
            Err(_) => {
                return Err(error_at(bytes, skip + consumed * 2, "unpaired UTF-16 surrogate"));
            }
        }
    }
    if out.starts_with("<?xml") {
        if let Some(close) = out.find("?>") {
            let lower = out[..close].to_ascii_lowercase();
            if let Some(p) = lower.find("encoding") {
                if let Some(q) = lower[p..].find("utf-16") {
                    let at = p + q;
                    let mut end = at + 6;
                    if lower[end..].starts_with("le") || lower[end..].starts_with("be") {
                        end += 2;
                    }
                    out.replace_range(at..end, "UTF-8");
                }
            }
        }
    }
    Ok(Cow::Owned(out.into_bytes()))
}

// ---------------------------------------------------------------------------------------------
// Names, attributes, events
// ---------------------------------------------------------------------------------------------

/// A qualified name as written: `prefix:local` (`prefix` is `None` when there is no colon).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QName<'a> {
    pub prefix: Option<&'a str>,
    pub local: &'a str,
    /// The whole name as written (`w:p`).
    pub full: &'a str,
}

impl<'a> QName<'a> {
    /// Splits a name at its first colon. Never fails; the reader validates names it reads.
    pub fn parse(full: &'a str) -> QName<'a> {
        match full.find(':') {
            Some(i) => QName { prefix: Some(&full[..i]), local: &full[i + 1..], full },
            None => QName { prefix: None, local: full, full },
        }
    }
}

/// One attribute of a start or empty tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr<'a> {
    pub name: QName<'a>,
    /// The value exactly as written between the quotes (still escaped).
    pub raw: &'a str,
    /// The value with references expanded and attribute whitespace normalized.
    pub value: Cow<'a, str>,
    /// The quote byte used: `b'"'` or `b'\''`.
    pub quote: u8,
    /// From the first byte of the name to the closing quote inclusive.
    pub span: Range<usize>,
}

/// One thing pulled out of the document. Every variant carries its exact byte `span` in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event<'a> {
    /// `<?xml version=".." encoding=".." standalone=".."?>` (only at the very start).
    Decl { version: &'a str, encoding: Option<&'a str>, standalone: Option<&'a str>, span: Range<usize> },
    /// `<!DOCTYPE name ... [internal subset]>`.
    DocType { name: &'a str, internal_subset: Option<&'a str>, span: Range<usize> },
    /// `<name attr="..">`.
    Start { name: QName<'a>, attrs: Vec<Attr<'a>>, span: Range<usize> },
    /// `<name attr=".."/>`.
    Empty { name: QName<'a>, attrs: Vec<Attr<'a>>, span: Range<usize> },
    /// `</name>`.
    End { name: QName<'a>, span: Range<usize> },
    /// Character data: `raw` as written, `text` with references expanded and line ends normalized.
    /// Outside the root it is whitespace only and `text == raw`.
    Text { raw: &'a str, text: Cow<'a, str>, span: Range<usize> },
    /// `<![CDATA[text]]>`.
    CData { text: &'a str, span: Range<usize> },
    /// `<!--text-->`.
    Comment { text: &'a str, span: Range<usize> },
    /// `<?target data?>`.
    PI { target: &'a str, data: &'a str, span: Range<usize> },
}

impl<'a> Event<'a> {
    /// The event's exact byte range in the source.
    pub fn span(&self) -> Range<usize> {
        match self {
            Event::Decl { span, .. }
            | Event::DocType { span, .. }
            | Event::Start { span, .. }
            | Event::Empty { span, .. }
            | Event::End { span, .. }
            | Event::Text { span, .. }
            | Event::CData { span, .. }
            | Event::Comment { span, .. }
            | Event::PI { span, .. } => span.clone(),
        }
    }

    /// The attributes of a `Start` or `Empty`, empty for every other event.
    pub fn attrs(&self) -> &[Attr<'a>] {
        match self {
            Event::Start { attrs, .. } | Event::Empty { attrs, .. } => attrs,
            _ => &[],
        }
    }

    /// The unescaped value of the attribute named exactly `full` (as written, e.g. `w:val`).
    pub fn attr(&self, full: &str) -> Option<&str> {
        self.attrs().iter().find(|a| a.name.full == full).map(|a| a.value.as_ref())
    }
}

// ---------------------------------------------------------------------------------------------
// Byte helpers
// ---------------------------------------------------------------------------------------------

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b':' || c >= 0x80
}

fn is_name_char(c: u8) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == b'-' || c == b'.'
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && is_ws(b[i]) {
        i += 1;
    }
    i
}

/// End of the name starting at `i`, or `None` when no name starts there.
fn name_end(b: &[u8], i: usize) -> Option<usize> {
    if i >= b.len() || !is_name_start(b[i]) {
        return None;
    }
    let mut j = i + 1;
    while j < b.len() && is_name_char(b[j]) {
        j += 1;
    }
    Some(j)
}

fn starts_at(b: &[u8], i: usize, pat: &[u8]) -> bool {
    b.get(i..).is_some_and(|r| r.starts_with(pat))
}

fn find(b: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || from > b.len() {
        return None;
    }
    let first = pat[0];
    let mut i = from;
    while i + pat.len() <= b.len() {
        let p = b[i..].iter().position(|&c| c == first)?;
        i += p;
        if i + pat.len() > b.len() {
            return None;
        }
        if &b[i..i + pat.len()] == pat {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn parse_char_ref(num: &str) -> Option<char> {
    let (digits, radix) = match num.strip_prefix('x') {
        Some(h) => (h, 16),
        None => (num, 10),
    };
    if digits.is_empty() || digits.len() > 8 || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let v = u32::from_str_radix(digits, radix).ok()?;
    let c = char::from_u32(v)?;
    is_xml_char(c).then_some(c)
}

fn is_predefined(name: &str) -> bool {
    matches!(name, "lt" | "gt" | "amp" | "apos" | "quot")
}

// ---------------------------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------------------------

enum EntityDef<'a> {
    Internal { raw: &'a str, offset: usize },
    External,
}

struct Entity<'a> {
    def: EntityDef<'a>,
    expanded: Option<Rc<str>>,
    busy: bool,
}

enum Lookup<'a> {
    Missing,
    Ready(Rc<str>),
    Busy,
    External,
    Raw(&'a str, usize),
}

/// A pull reader over a UTF-8 document. Call [`Reader::next`] until it returns `Ok(None)`.
pub struct Reader<'a> {
    src: &'a str,
    pos: usize,
    bom_len: usize,
    /// Open elements: full name and the namespace-stack mark to restore when it closes.
    stack: Vec<(&'a str, usize)>,
    /// Namespace bindings in scope: (prefix, uri); prefix "" is the default namespace.
    ns: Vec<(&'a str, Cow<'a, str>)>,
    pending_pop: Option<usize>,
    after_start: Option<usize>,
    root_seen: bool,
    doctype_seen: bool,
    entities: HashMap<&'a str, Entity<'a>>,
    expanded_total: usize,
    poisoned: Option<XmlError>,
}

impl<'a> Reader<'a> {
    /// Starts reading `src`, which must be UTF-8 (a UTF-8 BOM is fine). A UTF-16 document is an
    /// error: pass it through [`to_utf8`] first.
    pub fn new(src: &'a [u8]) -> Result<Reader<'a>, XmlError> {
        if utf16_kind(src).is_some() {
            return Err(error_at(src, 0, "UTF-16 document: transcode it with xml::to_utf8 first"));
        }
        let s = std::str::from_utf8(src)
            .map_err(|e| error_at(src, e.valid_up_to(), "invalid UTF-8"))?;
        let bom_len = if s.starts_with('\u{FEFF}') { 3 } else { 0 };
        Ok(Reader {
            src: s,
            pos: bom_len,
            bom_len,
            stack: Vec::new(),
            ns: Vec::new(),
            pending_pop: None,
            after_start: None,
            root_seen: false,
            doctype_seen: false,
            entities: HashMap::new(),
            expanded_total: 0,
            poisoned: None,
        })
    }

    /// The source bytes the spans refer to.
    pub fn source(&self) -> &'a [u8] {
        self.src.as_bytes()
    }

    /// Length of a leading UTF-8 BOM (0 or 3). No event covers it: copy `&src[..bom_len()]` first
    /// when reproducing a document byte for byte.
    pub fn bom_len(&self) -> usize {
        self.bom_len
    }

    /// Byte offset of the next unread byte.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Number of currently open elements.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Pulls the next event, `Ok(None)` at the end of a well-formed document. After an error every
    /// further call returns the same error.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<Event<'a>>, XmlError> {
        if let Some(e) = &self.poisoned {
            return Err(e.clone());
        }
        match self.next_inner() {
            Ok(v) => Ok(v),
            Err(e) => {
                self.poisoned = Some(e.clone());
                Err(e)
            }
        }
    }

    /// The namespace URI of an element name in the current scope (the default namespace for an
    /// unprefixed name). Valid for the element of the last `Start`, `Empty` or `End` event.
    pub fn resolve(&self, name: &QName) -> Option<&str> {
        self.lookup(name.prefix.unwrap_or(""))
    }

    /// The namespace URI of an attribute name: unprefixed attributes are in no namespace.
    pub fn resolve_attr(&self, name: &QName) -> Option<&str> {
        match name.prefix {
            None if name.full == "xmlns" => Some(XMLNS_NS),
            None => None,
            Some(p) => self.lookup(p),
        }
    }

    /// The URI bound to `prefix` ("" for the default namespace) in the current scope.
    pub fn lookup(&self, prefix: &str) -> Option<&str> {
        match prefix {
            "xml" => return Some(XML_NS),
            "xmlns" => return Some(XMLNS_NS),
            _ => {}
        }
        for (p, uri) in self.ns.iter().rev() {
            if *p == prefix {
                return if uri.is_empty() { None } else { Some(uri) };
            }
        }
        None
    }

    /// Called right after a `Start` event: reads through to its matching `End` and returns the byte
    /// range from the start tag's `<` to the end tag's `>`, so the element can be copied verbatim.
    pub fn subtree_span(&mut self) -> Result<Range<usize>, XmlError> {
        let start = match self.after_start.take() {
            Some(s) => s,
            None => return Err(self.err(self.pos, "subtree_span must be called right after a Start event")),
        };
        let depth = self.stack.len();
        loop {
            match self.next()? {
                Some(Event::End { span, .. }) if self.stack.len() + 1 == depth => return Ok(start..span.end),
                Some(_) => {}
                None => return Err(self.err(self.pos, "unexpected end of input inside an element")),
            }
        }
    }

    fn err(&self, offset: usize, message: impl Into<String>) -> XmlError {
        error_at(self.src.as_bytes(), offset, message)
    }

    fn next_inner(&mut self) -> Result<Option<Event<'a>>, XmlError> {
        if let Some(mark) = self.pending_pop.take() {
            self.ns.truncate(mark);
        }
        self.after_start = None;
        let b = self.src.as_bytes();
        let len = b.len();
        let start = self.pos;
        if start >= len {
            if let Some(&(name, _)) = self.stack.last() {
                return Err(self.err(len, format!("unexpected end of input: <{}> is never closed", name)));
            }
            if !self.root_seen {
                return Err(self.err(len, "no root element"));
            }
            return Ok(None);
        }
        if b[start] != b'<' {
            return self.read_text(start).map(Some);
        }
        if starts_at(b, start, b"<?") {
            return self.read_pi(start).map(Some);
        }
        if starts_at(b, start, b"<!--") {
            return self.read_comment(start).map(Some);
        }
        if starts_at(b, start, b"<![CDATA[") {
            return self.read_cdata(start).map(Some);
        }
        if starts_at(b, start, b"<!DOCTYPE") {
            return self.read_doctype(start).map(Some);
        }
        if starts_at(b, start, b"</") {
            return self.read_end(start).map(Some);
        }
        if starts_at(b, start, b"<!") {
            return Err(self.err(start, "unknown markup declaration"));
        }
        self.read_start(start).map(Some)
    }

    fn read_text(&mut self, start: usize) -> Result<Event<'a>, XmlError> {
        let s = self.src;
        let b = s.as_bytes();
        let end = b[start..].iter().position(|&c| c == b'<').map_or(b.len(), |p| start + p);
        let raw = &s[start..end];
        self.pos = end;
        if self.stack.is_empty() {
            if let Some(p) = raw.bytes().position(|c| !is_ws(c)) {
                let msg = if self.root_seen { "content after the root element" } else { "text before the root element" };
                return Err(self.err(start + p, msg));
            }
            return Ok(Event::Text { raw, text: Cow::Borrowed(raw), span: start..end });
        }
        if let Some(p) = raw.find("]]>") {
            return Err(self.err(start + p, "']]>' is not allowed in text"));
        }
        let text = self.expand(raw, start, false)?;
        Ok(Event::Text { raw, text, span: start..end })
    }

    fn read_pi(&mut self, start: usize) -> Result<Event<'a>, XmlError> {
        let s = self.src;
        let b = s.as_bytes();
        let ts = start + 2;
        let te = name_end(b, ts).ok_or_else(|| self.err(ts, "expected a processing-instruction target"))?;
        let target = &s[ts..te];
        let close = find(b, te, b"?>")
            .ok_or_else(|| self.err(b.len(), "unexpected end of input: processing instruction is never closed"))?;
        if close > te && !is_ws(b[te]) {
            return Err(self.err(te, "expected whitespace after the processing-instruction target"));
        }
        let end = close + 2;
        let data = s[te..close].trim_start_matches([' ', '\t', '\n', '\r']);
        self.pos = end;
        if target.eq_ignore_ascii_case("xml") {
            if target == "xml" && start == self.bom_len {
                return self.parse_decl(data, close - data.len(), start..end);
            }
            return Err(self.err(start, "the XML declaration is only allowed at the very start"));
        }
        Ok(Event::PI { target, data, span: start..end })
    }

    fn parse_decl(&self, data: &'a str, base: usize, span: Range<usize>) -> Result<Event<'a>, XmlError> {
        let db = data.as_bytes();
        let mut pairs: Vec<(&'a str, &'a str)> = Vec::new();
        let mut i = 0;
        loop {
            let before = i;
            i = skip_ws(db, i);
            if i >= db.len() {
                break;
            }
            if !pairs.is_empty() && i == before {
                return Err(self.err(base + i, "expected whitespace in the XML declaration"));
            }
            let ne = name_end(db, i).ok_or_else(|| self.err(base + i, "malformed XML declaration"))?;
            let name = &data[i..ne];
            i = skip_ws(db, ne);
            if db.get(i) != Some(&b'=') {
                return Err(self.err(base + i, "expected '=' in the XML declaration"));
            }
            i = skip_ws(db, i + 1);
            let q = match db.get(i) {
                Some(&q) if q == b'"' || q == b'\'' => q,
                _ => return Err(self.err(base + i, "expected a quoted value in the XML declaration")),
            };
            let vs = i + 1;
            let ve = db[vs..]
                .iter()
                .position(|&c| c == q)
                .map(|p| vs + p)
                .ok_or_else(|| self.err(base + vs, "unterminated value in the XML declaration"))?;
            pairs.push((name, &data[vs..ve]));
            i = ve + 1;
        }
        let mut it = pairs.iter().peekable();
        let version = match it.next() {
            Some(&("version", v)) if v.starts_with("1.") => v,
            _ => return Err(self.err(span.start, "the XML declaration needs version=\"1.x\" first")),
        };
        let mut encoding = None;
        let mut standalone = None;
        if let Some(&&("encoding", v)) = it.peek() {
            encoding = Some(v);
            it.next();
        }
        if let Some(&&("standalone", v)) = it.peek() {
            if v != "yes" && v != "no" {
                return Err(self.err(span.start, "standalone must be \"yes\" or \"no\""));
            }
            standalone = Some(v);
            it.next();
        }
        if it.next().is_some() {
            return Err(self.err(span.start, "unexpected pseudo-attribute in the XML declaration"));
        }
        Ok(Event::Decl { version, encoding, standalone, span })
    }

    fn read_comment(&mut self, start: usize) -> Result<Event<'a>, XmlError> {
        let s = self.src;
        let b = s.as_bytes();
        let cs = start + 4;
        let close = find(b, cs, b"-->")
            .ok_or_else(|| self.err(b.len(), "unexpected end of input: comment is never closed"))?;
        let text = &s[cs..close];
        if let Some(p) = text.find("--") {
            return Err(self.err(cs + p, "'--' is not allowed inside a comment"));
        }
        if text.ends_with('-') {
            return Err(self.err(close - 1, "a comment may not end with '-'"));
        }
        self.pos = close + 3;
        Ok(Event::Comment { text, span: start..close + 3 })
    }

    fn read_cdata(&mut self, start: usize) -> Result<Event<'a>, XmlError> {
        if self.stack.is_empty() {
            return Err(self.err(start, "CDATA outside the root element"));
        }
        let s = self.src;
        let b = s.as_bytes();
        let cs = start + 9;
        let close = find(b, cs, b"]]>")
            .ok_or_else(|| self.err(b.len(), "unexpected end of input: CDATA section is never closed"))?;
        self.pos = close + 3;
        Ok(Event::CData { text: &s[cs..close], span: start..close + 3 })
    }

    fn skip_quoted(&self, i: usize) -> Result<usize, XmlError> {
        let b = self.src.as_bytes();
        match b.get(i) {
            Some(&q) if q == b'"' || q == b'\'' => match b[i + 1..].iter().position(|&c| c == q) {
                Some(p) => Ok(i + 1 + p + 1),
                None => Err(self.err(b.len(), "unexpected end of input inside a quoted literal")),
            },
            _ => Err(self.err(i, "expected a quoted literal")),
        }
    }

    fn skip_external_id(&self, mut i: usize) -> Result<Option<usize>, XmlError> {
        let b = self.src.as_bytes();
        if starts_at(b, i, b"SYSTEM") {
            i = skip_ws(b, i + 6);
            return self.skip_quoted(i).map(Some);
        }
        if starts_at(b, i, b"PUBLIC") {
            i = skip_ws(b, i + 6);
            i = self.skip_quoted(i)?;
            i = skip_ws(b, i);
            return self.skip_quoted(i).map(Some);
        }
        Ok(None)
    }

    fn read_doctype(&mut self, start: usize) -> Result<Event<'a>, XmlError> {
        if self.root_seen {
            return Err(self.err(start, "DOCTYPE after the root element"));
        }
        if self.doctype_seen {
            return Err(self.err(start, "a second DOCTYPE"));
        }
        let s = self.src;
        let b = s.as_bytes();
        let mut i = start + 9;
        if b.get(i).is_none_or(|&c| !is_ws(c)) {
            return Err(self.err(i, "expected whitespace after <!DOCTYPE"));
        }
        i = skip_ws(b, i);
        let ne = name_end(b, i).ok_or_else(|| self.err(i, "expected the document type name"))?;
        let name = &s[i..ne];
        i = skip_ws(b, ne);
        if let Some(j) = self.skip_external_id(i)? {
            i = skip_ws(b, j);
        }
        let mut internal_subset = None;
        if b.get(i) == Some(&b'[') {
            let ss = i + 1;
            let se = self.read_internal_subset(ss)?;
            internal_subset = Some(&s[ss..se]);
            i = skip_ws(b, se + 1);
        }
        match b.get(i) {
            Some(b'>') => {}
            Some(_) => return Err(self.err(i, "expected '>' to close the DOCTYPE")),
            None => return Err(self.err(i, "unexpected end of input inside the DOCTYPE")),
        }
        let end = i + 1;
        self.pos = end;
        self.doctype_seen = true;
        Ok(Event::DocType { name, internal_subset, span: start..end })
    }

    /// Scans the internal subset from `i`; returns the index of its closing `]`.
    fn read_internal_subset(&mut self, mut i: usize) -> Result<usize, XmlError> {
        let b = self.src.as_bytes();
        loop {
            i = skip_ws(b, i);
            if i >= b.len() {
                return Err(self.err(i, "unexpected end of input: DOCTYPE internal subset is never closed"));
            }
            if b[i] == b']' {
                return Ok(i);
            }
            if starts_at(b, i, b"<!--") {
                let c = find(b, i + 4, b"-->").ok_or_else(|| self.err(b.len(), "comment is never closed"))?;
                i = c + 3;
            } else if starts_at(b, i, b"<?") {
                let c = find(b, i + 2, b"?>")
                    .ok_or_else(|| self.err(b.len(), "processing instruction is never closed"))?;
                i = c + 2;
            } else if starts_at(b, i, b"<!ENTITY") {
                i = self.read_entity_decl(i)?;
            } else if starts_at(b, i, b"<!") {
                let mut j = i + 2;
                loop {
                    match b.get(j) {
                        None => return Err(self.err(j, "unexpected end of input inside a markup declaration")),
                        Some(b'"') | Some(b'\'') => j = self.skip_quoted(j)?,
                        Some(b'>') => {
                            j += 1;
                            break;
                        }
                        Some(_) => j += 1,
                    }
                }
                i = j;
            } else if b[i] == b'%' {
                let ne = name_end(b, i + 1).ok_or_else(|| self.err(i, "bad parameter-entity reference"))?;
                if b.get(ne) != Some(&b';') {
                    return Err(self.err(ne, "expected ';' after the parameter-entity name"));
                }
                i = ne + 1;
            } else {
                return Err(self.err(i, "unexpected character in the DOCTYPE internal subset"));
            }
        }
    }

    fn read_entity_decl(&mut self, i: usize) -> Result<usize, XmlError> {
        let s = self.src;
        let b = s.as_bytes();
        let mut j = i + 8;
        if b.get(j).is_none_or(|&c| !is_ws(c)) {
            return Err(self.err(j, "expected whitespace after <!ENTITY"));
        }
        j = skip_ws(b, j);
        let mut param = false;
        if b.get(j) == Some(&b'%') {
            param = true;
            j += 1;
            if b.get(j).is_none_or(|&c| !is_ws(c)) {
                return Err(self.err(j, "expected whitespace after '%'"));
            }
            j = skip_ws(b, j);
        }
        let ne = name_end(b, j).ok_or_else(|| self.err(j, "expected an entity name"))?;
        let name = &s[j..ne];
        j = ne;
        if b.get(j).is_none_or(|&c| !is_ws(c)) {
            return Err(self.err(j, "expected whitespace after the entity name"));
        }
        j = skip_ws(b, j);
        let def = match b.get(j) {
            Some(b'"') | Some(b'\'') => {
                let after = self.skip_quoted(j)?;
                let def = EntityDef::Internal { raw: &s[j + 1..after - 1], offset: j + 1 };
                j = after;
                def
            }
            _ => match self.skip_external_id(j)? {
                Some(after) => {
                    j = after;
                    EntityDef::External
                }
                None => return Err(self.err(j, "expected an entity value or external id")),
            },
        };
        j = skip_ws(b, j);
        if starts_at(b, j, b"NDATA") {
            j = skip_ws(b, j + 5);
            let ne = name_end(b, j).ok_or_else(|| self.err(j, "expected a notation name"))?;
            j = skip_ws(b, ne);
        }
        if b.get(j) != Some(&b'>') {
            return Err(self.err(j, "expected '>' to close the entity declaration"));
        }
        if !param && !is_predefined(name) {
            self.entities.entry(name).or_insert(Entity { def, expanded: None, busy: false });
        }
        Ok(j + 1)
    }

    fn read_end(&mut self, start: usize) -> Result<Event<'a>, XmlError> {
        let s = self.src;
        let b = s.as_bytes();
        let ns = start + 2;
        let ne = name_end(b, ns).ok_or_else(|| self.err(ns, "expected a name in the end tag"))?;
        let full = &s[ns..ne];
        let i = skip_ws(b, ne);
        match b.get(i) {
            Some(b'>') => {}
            Some(_) => return Err(self.err(i, "expected '>' to close the end tag")),
            None => return Err(self.err(i, "unexpected end of input inside an end tag")),
        }
        match self.stack.last() {
            None => return Err(self.err(start, format!("end tag </{}> without a matching start tag", full))),
            Some(&(open, mark)) => {
                if open != full {
                    return Err(self.err(start, format!("mismatched end tag: expected </{}>, found </{}>", open, full)));
                }
                self.stack.pop();
                self.pending_pop = Some(mark);
            }
        }
        self.pos = i + 1;
        Ok(Event::End { name: QName::parse(full), span: start..i + 1 })
    }

    fn qname(&self, full: &'a str, offset: usize) -> Result<QName<'a>, XmlError> {
        let q = QName::parse(full);
        if let Some(p) = q.prefix {
            let lb = q.local.as_bytes();
            let ok = !p.is_empty()
                && !lb.is_empty()
                && !q.local.contains(':')
                && !lb[0].is_ascii_digit()
                && lb[0] != b'-'
                && lb[0] != b'.';
            if !ok {
                return Err(self.err(offset, format!("malformed qualified name '{}'", full)));
            }
        }
        Ok(q)
    }

    fn read_start(&mut self, start: usize) -> Result<Event<'a>, XmlError> {
        if self.root_seen && self.stack.is_empty() {
            return Err(self.err(start, "a second root element"));
        }
        let s = self.src;
        let b = s.as_bytes();
        let ns0 = start + 1;
        let ne = name_end(b, ns0).ok_or_else(|| self.err(ns0, "expected a tag name after '<'"))?;
        let full = &s[ns0..ne];
        let name = self.qname(full, ns0)?;
        let mut attrs: Vec<Attr<'a>> = Vec::new();
        let mut i = ne;
        let empty;
        loop {
            let before = i;
            i = skip_ws(b, i);
            match b.get(i) {
                None => return Err(self.err(i, "unexpected end of input inside a tag")),
                Some(b'>') => {
                    i += 1;
                    empty = false;
                    break;
                }
                Some(b'/') => {
                    if b.get(i + 1) == Some(&b'>') {
                        i += 2;
                        empty = true;
                        break;
                    }
                    return Err(self.err(i, "expected '>' after '/'"));
                }
                Some(_) => {
                    if i == before {
                        return Err(self.err(i, "expected whitespace before an attribute"));
                    }
                    let an = i;
                    let ae = name_end(b, i).ok_or_else(|| self.err(i, "expected an attribute name"))?;
                    let aname = self.qname(&s[an..ae], an)?;
                    i = skip_ws(b, ae);
                    if b.get(i) != Some(&b'=') {
                        return Err(self.err(i, format!("expected '=' after attribute '{}'", aname.full)));
                    }
                    i = skip_ws(b, i + 1);
                    let quote = match b.get(i) {
                        Some(&q) if q == b'"' || q == b'\'' => q,
                        _ => return Err(self.err(i, "attribute values must be quoted")),
                    };
                    let vs = i + 1;
                    let ve = b[vs..]
                        .iter()
                        .position(|&c| c == quote)
                        .map(|p| vs + p)
                        .ok_or_else(|| self.err(b.len(), "unexpected end of input inside an attribute value"))?;
                    let raw = &s[vs..ve];
                    if let Some(p) = raw.find('<') {
                        return Err(self.err(vs + p, "'<' is not allowed in an attribute value"));
                    }
                    let value = self.expand(raw, vs, true)?;
                    i = ve + 1;
                    if attrs.iter().any(|a| a.name.full == aname.full) {
                        return Err(self.err(an, format!("duplicate attribute '{}'", aname.full)));
                    }
                    attrs.push(Attr { name: aname, raw, value, quote, span: an..i });
                }
            }
        }
        // Namespace declarations open a new scope for this element.
        let mark = self.ns.len();
        for a in &attrs {
            if a.name.full == "xmlns" {
                self.ns.push(("", a.value.clone()));
            } else if a.name.prefix == Some("xmlns") {
                let p = a.name.local;
                if p == "xmlns" || (p == "xml" && a.value != XML_NS) {
                    return Err(self.err(a.span.start, format!("the prefix '{}' cannot be redeclared", p)));
                }
                if a.value.is_empty() {
                    return Err(self.err(a.span.start, format!("empty namespace for prefix '{}'", p)));
                }
                self.ns.push((p, a.value.clone()));
            }
        }
        if let Some(p) = name.prefix {
            if self.lookup(p).is_none() {
                return Err(self.err(ns0, format!("unbound namespace prefix '{}'", p)));
            }
        }
        for a in &attrs {
            if let Some(p) = a.name.prefix {
                if self.lookup(p).is_none() {
                    return Err(self.err(a.span.start, format!("unbound namespace prefix '{}'", p)));
                }
            }
        }
        self.root_seen = true;
        self.pos = i;
        let span = start..i;
        if empty {
            self.pending_pop = Some(mark);
            Ok(Event::Empty { name, attrs, span })
        } else {
            if self.stack.len() >= MAX_DEPTH {
                return Err(self.err(start, "elements nest too deeply"));
            }
            self.stack.push((full, mark));
            self.after_start = Some(start);
            Ok(Event::Start { name, attrs, span })
        }
    }

    // ----- references -----

    fn expand(&mut self, raw: &'a str, base: usize, attr: bool) -> Result<Cow<'a, str>, XmlError> {
        if let Some(p) = raw.bytes().position(|c| c < 0x20 && c != b'\t' && c != b'\n' && c != b'\r') {
            return Err(self.err(base + p, "control character not allowed in XML"));
        }
        let needs = raw.bytes().any(|c| c == b'&' || c == b'\r' || (attr && (c == b'\t' || c == b'\n')));
        if !needs {
            return Ok(Cow::Borrowed(raw));
        }
        let mut out = String::with_capacity(raw.len());
        self.expand_into(raw, base, attr, &mut out, 0)?;
        Ok(Cow::Owned(out))
    }

    fn expand_into(&mut self, raw: &str, base: usize, attr: bool, out: &mut String, depth: usize) -> Result<(), XmlError> {
        let b = raw.as_bytes();
        let mut i = 0;
        let mut run = 0;
        while i < b.len() {
            match b[i] {
                b'&' => {
                    out.push_str(&raw[run..i]);
                    let semi = b[i + 1..]
                        .iter()
                        .position(|&c| c == b';')
                        .map(|p| i + 1 + p)
                        .ok_or_else(|| self.err(base + i, "unterminated entity reference (missing ';')"))?;
                    let body = &raw[i + 1..semi];
                    if let Some(num) = body.strip_prefix('#') {
                        let c = parse_char_ref(num)
                            .ok_or_else(|| self.err(base + i, format!("bad character reference &{};", body)))?;
                        out.push(c);
                    } else {
                        match body {
                            "lt" => out.push('<'),
                            "gt" => out.push('>'),
                            "amp" => out.push('&'),
                            "apos" => out.push('\''),
                            "quot" => out.push('"'),
                            _ => {
                                if name_end(body.as_bytes(), 0) != Some(body.len()) {
                                    return Err(self.err(base + i, format!("bad entity reference &{};", body)));
                                }
                                if depth >= MAX_ENTITY_NESTING {
                                    return Err(self.err(base + i, "entity references nest too deeply"));
                                }
                                let text = self.entity_text(body, base + i, depth + 1)?;
                                self.expanded_total += text.len();
                                if self.expanded_total > MAX_EXPANSION_TOTAL {
                                    return Err(self.err(base + i, "entity expansion exceeds the size cap"));
                                }
                                if attr {
                                    out.extend(text.chars().map(|c| if matches!(c, '\t' | '\n' | '\r') { ' ' } else { c }));
                                } else {
                                    out.push_str(&text);
                                }
                                if out.len() > MAX_EXPANSION_TOTAL {
                                    return Err(self.err(base + i, "entity expansion exceeds the size cap"));
                                }
                            }
                        }
                    }
                    i = semi + 1;
                    run = i;
                }
                b'\r' => {
                    out.push_str(&raw[run..i]);
                    out.push(if attr { ' ' } else { '\n' });
                    i += if b.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                    run = i;
                }
                b'\t' | b'\n' if attr => {
                    out.push_str(&raw[run..i]);
                    out.push(' ');
                    i += 1;
                    run = i;
                }
                _ => i += 1,
            }
        }
        out.push_str(&raw[run..]);
        Ok(())
    }

    fn entity_text(&mut self, name: &str, at: usize, depth: usize) -> Result<Rc<str>, XmlError> {
        let look = match self.entities.get_mut(name) {
            None => Lookup::Missing,
            Some(e) => {
                if let Some(t) = &e.expanded {
                    Lookup::Ready(t.clone())
                } else if e.busy {
                    Lookup::Busy
                } else {
                    match e.def {
                        EntityDef::External => Lookup::External,
                        EntityDef::Internal { raw, offset } => {
                            e.busy = true;
                            Lookup::Raw(raw, offset)
                        }
                    }
                }
            }
        };
        let (raw, offset) = match look {
            Lookup::Ready(t) => return Ok(t),
            Lookup::Missing => return Err(self.err(at, format!("undefined entity &{};", name))),
            Lookup::Busy => return Err(self.err(at, format!("entity &{}; refers to itself", name))),
            Lookup::External => {
                return Err(self.err(at, format!("external entity &{}; is never fetched", name)));
            }
            Lookup::Raw(raw, offset) => (raw, offset),
        };
        let mut text = String::with_capacity(raw.len());
        let r = self.expand_into(raw, offset, false, &mut text, depth);
        if let Some(e) = self.entities.get_mut(name) {
            e.busy = false;
        }
        r?;
        if text.len() > MAX_ENTITY_LEN {
            return Err(self.err(at, format!("entity &{}; expands beyond the size cap", name)));
        }
        let rc: Rc<str> = Rc::from(text);
        if let Some(e) = self.entities.get_mut(name) {
            e.expanded = Some(rc.clone());
        }
        Ok(rc)
    }
}

// ---------------------------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------------------------

fn escape(s: &str, attr: bool) -> Cow<'_, str> {
    let needs = |c: u8| match c {
        b'&' | b'<' | b'>' | b'\r' => true,
        b'"' | b'\t' | b'\n' => attr,
        _ => false,
    };
    if !s.bytes().any(needs) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            '"' if attr => out.push_str("&quot;"),
            '\t' if attr => out.push_str("&#x9;"),
            '\n' if attr => out.push_str("&#xA;"),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Escapes character data: `&`, `<`, `>` (and `\r` as `&#xD;` so it survives a re-read).
pub fn escape_text(s: &str) -> Cow<'_, str> {
    escape(s, false)
}

/// Escapes a double-quoted attribute value: `&`, `<`, `>`, `"` and tab / line ends as character
/// references (so attribute-value normalization cannot eat them).
pub fn escape_attr(s: &str) -> Cow<'_, str> {
    escape(s, true)
}

/// Builds an XML document into a `Vec<u8>`. Compact by default; [`Writer::pretty`] indents element
/// content (never inside an element that already holds text).
#[derive(Debug, Default, Clone)]
pub struct Writer {
    buf: Vec<u8>,
    indent: Option<String>,
    depth: usize,
    mixed: Vec<bool>,
    last_open: bool,
}

impl Writer {
    /// A compact writer: exactly the bytes asked for, nothing added.
    pub fn new() -> Writer {
        Writer::default()
    }

    /// A writer that puts each element on its own line, indented by `indent` per level.
    pub fn pretty(indent: &str) -> Writer {
        Writer { indent: Some(indent.to_string()), ..Writer::default() }
    }

    fn in_mixed(&self) -> bool {
        self.mixed.last().copied().unwrap_or(false)
    }

    fn newline(&mut self, depth: usize) {
        if let Some(ind) = &self.indent {
            if !self.buf.is_empty() {
                self.buf.push(b'\n');
                for _ in 0..depth {
                    self.buf.extend_from_slice(ind.as_bytes());
                }
            }
        }
    }

    fn break_line(&mut self) {
        if self.indent.is_some() && !self.in_mixed() {
            self.newline(self.depth);
        }
    }

    fn write_attrs<K: AsRef<str>, V: AsRef<str>>(&mut self, attrs: &[(K, V)]) {
        for (k, v) in attrs {
            self.buf.push(b' ');
            self.buf.extend_from_slice(k.as_ref().as_bytes());
            self.buf.extend_from_slice(b"=\"");
            self.buf.extend_from_slice(escape_attr(v.as_ref()).as_bytes());
            self.buf.push(b'"');
        }
    }

    /// `<?xml version="1.0" encoding="UTF-8"?>`, with `standalone="yes|no"` when given.
    pub fn decl(&mut self, standalone: Option<bool>) {
        self.buf.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\"");
        match standalone {
            Some(true) => self.buf.extend_from_slice(b" standalone=\"yes\""),
            Some(false) => self.buf.extend_from_slice(b" standalone=\"no\""),
            None => {}
        }
        self.buf.extend_from_slice(b"?>");
        self.last_open = false;
    }

    /// `<name k="v" ...>`.
    pub fn start<K: AsRef<str>, V: AsRef<str>>(&mut self, name: &str, attrs: &[(K, V)]) {
        self.break_line();
        self.buf.push(b'<');
        self.buf.extend_from_slice(name.as_bytes());
        self.write_attrs(attrs);
        self.buf.push(b'>');
        let mixed = self.in_mixed();
        self.mixed.push(mixed);
        self.depth += 1;
        self.last_open = true;
    }

    /// `<name k="v" .../>`.
    pub fn empty<K: AsRef<str>, V: AsRef<str>>(&mut self, name: &str, attrs: &[(K, V)]) {
        self.break_line();
        self.buf.push(b'<');
        self.buf.extend_from_slice(name.as_bytes());
        self.write_attrs(attrs);
        self.buf.extend_from_slice(b"/>");
        self.last_open = false;
    }

    /// `</name>`.
    pub fn end(&mut self, name: &str) {
        self.depth = self.depth.saturating_sub(1);
        let was_mixed = self.mixed.pop().unwrap_or(false);
        if self.indent.is_some() && !self.last_open && !was_mixed {
            self.newline(self.depth);
        }
        self.buf.extend_from_slice(b"</");
        self.buf.extend_from_slice(name.as_bytes());
        self.buf.push(b'>');
        self.last_open = false;
    }

    fn mark_mixed(&mut self) {
        if let Some(m) = self.mixed.last_mut() {
            *m = true;
        }
    }

    /// Escaped character data.
    pub fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.mark_mixed();
        self.buf.extend_from_slice(escape_text(s).as_bytes());
        self.last_open = false;
    }

    /// `<![CDATA[s]]>`; a `]]>` inside `s` is split across two sections.
    pub fn cdata(&mut self, s: &str) {
        self.mark_mixed();
        self.buf.extend_from_slice(b"<![CDATA[");
        self.buf.extend_from_slice(s.replace("]]>", "]]]]><![CDATA[>").as_bytes());
        self.buf.extend_from_slice(b"]]>");
        self.last_open = false;
    }

    /// `<!--s-->`; a `--` inside (or a trailing `-`) is spaced apart so the comment stays legal.
    pub fn comment(&mut self, s: &str) {
        self.break_line();
        let mut t = s.to_string();
        while t.contains("--") {
            t = t.replace("--", "- -");
        }
        if t.ends_with('-') {
            t.push(' ');
        }
        self.buf.extend_from_slice(b"<!--");
        self.buf.extend_from_slice(t.as_bytes());
        self.buf.extend_from_slice(b"-->");
        self.last_open = false;
    }

    /// `<?target data?>`; a `?>` inside `data` is spaced apart.
    pub fn pi(&mut self, target: &str, data: &str) {
        self.break_line();
        self.buf.extend_from_slice(b"<?");
        self.buf.extend_from_slice(target.as_bytes());
        if !data.is_empty() {
            self.buf.push(b' ');
            self.buf.extend_from_slice(data.replace("?>", "? >").as_bytes());
        }
        self.buf.extend_from_slice(b"?>");
        self.last_open = false;
    }

    /// Copies bytes verbatim (an unknown subtree, a preserved comment, a line break ...).
    pub fn raw(&mut self, bytes: &[u8]) {
        if self.indent.is_some() {
            self.break_line();
        }
        self.buf.extend_from_slice(bytes);
        self.last_open = false;
    }

    /// Writes a whole DOM element (and its subtree).
    pub fn element(&mut self, el: &Element) {
        if el.children.is_empty() {
            self.empty(&el.name, &el.attrs);
            return;
        }
        self.start(&el.name, &el.attrs);
        for c in &el.children {
            match c {
                Node::Element(e) => self.element(e),
                Node::Text(t) => self.text(t),
                Node::Raw(b) => self.raw(b),
            }
        }
        self.end(&el.name);
    }

    /// The bytes written so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Nothing written yet.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Finishes and hands back the document.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

// ---------------------------------------------------------------------------------------------
// DOM
// ---------------------------------------------------------------------------------------------

/// A DOM element. Names are kept as written (`w:p`); `xmlns` declarations stay ordinary attributes,
/// so a tree written back is self-consistent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

/// A DOM child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    /// Unescaped character data.
    Text(String),
    /// Bytes copied verbatim: comments, processing instructions and CDATA sections from the source,
    /// or anything a caller wants written untouched.
    Raw(Vec<u8>),
}

fn local_of(name: &str) -> &str {
    name.find(':').map_or(name, |i| &name[i + 1..])
}

/// `query` with a colon matches the whole name, without one only the local part.
fn name_matches(name: &str, query: &str) -> bool {
    if query.contains(':') {
        name == query
    } else {
        local_of(name) == query
    }
}

impl Element {
    pub fn new(name: impl Into<String>) -> Element {
        Element { name: name.into(), attrs: Vec::new(), children: Vec::new() }
    }

    /// The name without its prefix.
    pub fn local(&self) -> &str {
        local_of(&self.name)
    }

    /// The prefix, if the name has one.
    pub fn prefix(&self) -> Option<&str> {
        self.name.find(':').map(|i| &self.name[..i])
    }

    /// The value of the attribute named exactly `name` (as written, e.g. `w:val`).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Sets (or adds) an attribute, keeping its position when it already exists.
    pub fn set_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        match self.attrs.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = value,
            None => self.attrs.push((name, value)),
        }
    }

    /// The child elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    /// The first child element whose local name is `local` (or whole name, if `local` has a colon).
    pub fn child(&self, local: &str) -> Option<&Element> {
        self.elements().find(|e| name_matches(&e.name, local))
    }

    /// Mutable [`Element::child`].
    pub fn child_mut(&mut self, local: &str) -> Option<&mut Element> {
        self.children.iter_mut().find_map(|c| match c {
            Node::Element(e) if name_matches(&e.name, local) => Some(e),
            _ => None,
        })
    }

    /// Every child element matching `local` (as in [`Element::child`]).
    pub fn children_named<'s>(&'s self, local: &'s str) -> impl Iterator<Item = &'s Element> + 's {
        self.elements().filter(move |e| name_matches(&e.name, local))
    }

    /// All descendant character data concatenated (CDATA content included).
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        for c in &self.children {
            match c {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => e.collect_text(out),
                Node::Raw(b) => {
                    if b.starts_with(b"<![CDATA[") && b.ends_with(b"]]>") && b.len() >= 12 {
                        out.push_str(&String::from_utf8_lossy(&b[9..b.len() - 3]));
                    }
                }
            }
        }
    }
}

fn dom_attrs(attrs: Vec<Attr<'_>>) -> Vec<(String, String)> {
    attrs.into_iter().map(|a| (a.name.full.to_string(), a.value.into_owned())).collect()
}

/// Parses a whole document (UTF-8 or UTF-16) into its root element. The prolog (declaration,
/// DOCTYPE, comments around the root) is not kept; comments, PIs and CDATA inside the root become
/// [`Node::Raw`] with their exact bytes; entity references become plain text.
pub fn parse_dom(bytes: &[u8]) -> Result<Element, XmlError> {
    let src = to_utf8(bytes)?;
    let mut r = Reader::new(&src)?;
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    while let Some(ev) = r.next()? {
        match ev {
            Event::Start { name, attrs, .. } => {
                stack.push(Element { name: name.full.to_string(), attrs: dom_attrs(attrs), children: Vec::new() });
            }
            Event::Empty { name, attrs, .. } => {
                let el = Element { name: name.full.to_string(), attrs: dom_attrs(attrs), children: Vec::new() };
                match stack.last_mut() {
                    Some(p) => p.children.push(Node::Element(el)),
                    None => root = Some(el),
                }
            }
            Event::End { .. } => {
                if let Some(el) = stack.pop() {
                    match stack.last_mut() {
                        Some(p) => p.children.push(Node::Element(el)),
                        None => root = Some(el),
                    }
                }
            }
            Event::Text { text, .. } => {
                if let Some(p) = stack.last_mut() {
                    match p.children.last_mut() {
                        Some(Node::Text(t)) => t.push_str(&text),
                        _ => p.children.push(Node::Text(text.into_owned())),
                    }
                }
            }
            Event::CData { span, .. } | Event::Comment { span, .. } | Event::PI { span, .. } => {
                if let Some(p) = stack.last_mut() {
                    p.children.push(Node::Raw(src[span].to_vec()));
                }
            }
            Event::Decl { .. } | Event::DocType { .. } => {}
        }
    }
    root.ok_or_else(|| error_at(&src, src.len(), "no root element"))
}

/// Writes a DOM as an OOXML part: `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>`, CRLF,
/// then the tree, compact.
pub fn write_dom(root: &Element) -> Vec<u8> {
    let mut w = Writer::new();
    w.decl(Some(true));
    w.raw(b"\r\n");
    w.element(root);
    w.into_bytes()
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn events(src: &str) -> Vec<Event<'_>> {
        let mut r = Reader::new(src.as_bytes()).expect("reader");
        let mut out = Vec::new();
        while let Some(e) = r.next().expect("well-formed") {
            out.push(e);
        }
        out
    }

    fn read_all(src: &[u8]) -> Result<usize, XmlError> {
        let mut r = Reader::new(src)?;
        let mut n = 0;
        while r.next()?.is_some() {
            n += 1;
        }
        Ok(n)
    }

    fn ooxml_doc() -> String {
        let mut d = String::new();
        d.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
        d.push_str("<w:document xmlns:wpc=\"http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas\" ");
        d.push_str("xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" ");
        d.push_str("xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ");
        d.push_str("xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ");
        d.push_str("xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" mc:Ignorable=\"w14 wp14\">");
        d.push_str("\r\n\t<w:body>\r\n\t\t<w:p w14:paraId=\"1A2B3C4D\" w:rsidR='00AB12CD'>");
        d.push_str("<w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>");
        d.push_str("<w:r><w:t xml:space=\"preserve\">Fish &amp; Chips &lt;today&gt; &#169; &#x2014; caf\u{e9}  </w:t></w:r>");
        d.push_str("\r\n\t\t\t<!-- a comment the editor never touches -->\r\n\t\t\t");
        d.push_str("<w14:unknownExt w14:kind=\"glow\"><w14:unknownExt><w14:deep a = \"1\" /></w14:unknownExt>text");
        d.push_str("<![CDATA[ <b>&</b> ]]></w14:unknownExt>");
        d.push_str("<?mso-hint keep me?>\r\n\t\t</w:p>\r\n\t\t<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>");
        d.push_str("<w:pgMar w:top=\"1440\" w:right='1440' r:id=\"rId7\"/></w:sectPr>\r\n\t</w:body>\r\n</w:document>");
        d
    }

    #[test]
    fn namespaces_and_default_scoping() {
        let src = "<root xmlns=\"urn:a\" xmlns:b=\"urn:b\"><b:x attr=\"1\" b:y=\"2\" xml:lang=\"en\"/>\
                   <inner xmlns=\"urn:c\"><leaf/></inner><after/><n xmlns=\"\"><m/></n><b:z xmlns:b=\"urn:b2\"/><b:w/></root>";
        let mut r = Reader::new(src.as_bytes()).unwrap();
        let mut seen: Vec<(String, String, Option<String>)> = Vec::new();
        while let Some(ev) = r.next().unwrap() {
            match &ev {
                Event::Start { name, attrs, .. } | Event::Empty { name, attrs, .. } => {
                    let kind = if matches!(ev, Event::Start { .. }) { "start" } else { "empty" };
                    seen.push((kind.into(), name.full.into(), r.resolve(name).map(String::from)));
                    if name.full == "b:x" {
                        let a = &attrs[0];
                        assert_eq!(r.resolve_attr(&a.name), None, "unprefixed attribute has no namespace");
                        assert_eq!(r.resolve_attr(&attrs[1].name), Some("urn:b"));
                        assert_eq!(r.resolve_attr(&attrs[2].name), Some(XML_NS));
                        assert_eq!(attrs[1].name.prefix, Some("b"));
                        assert_eq!(attrs[1].name.local, "y");
                    }
                    if name.full == "root" {
                        assert_eq!(r.resolve_attr(&attrs[0].name), Some(XMLNS_NS));
                    }
                }
                Event::End { name, .. } => {
                    seen.push(("end".into(), name.full.into(), r.resolve(name).map(String::from)));
                }
                _ => {}
            }
        }
        let get = |kind: &str, n: &str| {
            seen.iter().find(|(k, nm, _)| k == kind && nm == n).map(|(_, _, u)| u.clone()).unwrap()
        };
        assert_eq!(get("start", "root"), Some("urn:a".into()));
        assert_eq!(get("empty", "b:x"), Some("urn:b".into()));
        assert_eq!(get("start", "inner"), Some("urn:c".into()));
        assert_eq!(get("empty", "leaf"), Some("urn:c".into()));
        assert_eq!(get("end", "inner"), Some("urn:c".into()), "scope still open on its own End");
        assert_eq!(get("empty", "after"), Some("urn:a".into()), "scope closed after </inner>");
        assert_eq!(get("start", "n"), None, "xmlns=\"\" undeclares the default");
        assert_eq!(get("empty", "m"), None);
        assert_eq!(get("empty", "b:z"), Some("urn:b2".into()), "prefix rebinding on the element itself");
        assert_eq!(get("empty", "b:w"), Some("urn:b".into()), "rebinding scoped to the empty element");
        assert_eq!(get("end", "root"), Some("urn:a".into()));
        assert_eq!(r.lookup("xml"), Some(XML_NS));
    }

    #[test]
    fn unbound_prefixes_are_errors() {
        assert!(read_all(b"<p:a/>").is_err());
        assert!(read_all(b"<a q:x=\"1\"/>").is_err());
        assert!(read_all(b"<a xmlns:p=\"\"/>").is_err());
        assert!(read_all(b"<a xmlns:xmlns=\"urn:x\"/>").is_err());
        assert!(read_all(b"<a><p:b xmlns:p=\"urn:p\"/><p:c/></a>").is_err(), "binding does not leak out");
    }

    #[test]
    fn entities_and_char_refs() {
        let src = "<a t=\"&lt;&amp;&#65;&#x42;&quot;&apos;\">x &gt; &apos;y&apos; &#x1F600; &#169;</a>";
        let ev = events(src);
        match &ev[0] {
            Event::Start { attrs, .. } => {
                assert_eq!(attrs[0].raw, "&lt;&amp;&#65;&#x42;&quot;&apos;");
                assert_eq!(attrs[0].value, "<&AB\"'");
            }
            other => panic!("{:?}", other),
        }
        match &ev[1] {
            Event::Text { raw, text, .. } => {
                assert_eq!(*raw, "x &gt; &apos;y&apos; &#x1F600; &#169;");
                assert_eq!(text, "x > 'y' \u{1F600} \u{a9}");
            }
            other => panic!("{:?}", other),
        }
        // No references: the text is borrowed, not copied.
        let ev = events("<a>plain</a>");
        assert!(matches!(&ev[1], Event::Text { text: Cow::Borrowed("plain"), .. }));
    }

    #[test]
    fn line_ends_and_attribute_normalization() {
        let ev = events("<a v=\"x\ty\r\nz&#10;\">l1\r\nl2\rl3</a>");
        assert_eq!(ev[0].attr("v"), Some("x y z\n"));
        match &ev[1] {
            Event::Text { text, .. } => assert_eq!(text, "l1\nl2\nl3"),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn dtd_internal_entities() {
        let src = "<!DOCTYPE r [\n  <!-- notes -->\n  <!ELEMENT r ANY>\n  <!ATTLIST r a CDATA #IMPLIED>\n\
                   <!ENTITY co \"Kiyo &amp; Co\">\n  <!ENTITY full '&co; Ltd'>\n  <!ENTITY % pe \"ignored\">\n  %pe;\n]>\
                   <r a=\"&full;\">&full;!</r>";
        let ev = events(src);
        match &ev[0] {
            Event::DocType { name, internal_subset, .. } => {
                assert_eq!(*name, "r");
                assert!(internal_subset.unwrap().contains("<!ENTITY co"));
            }
            other => panic!("{:?}", other),
        }
        assert_eq!(ev[1].attr("a"), Some("Kiyo & Co Ltd"));
        match &ev[2] {
            Event::Text { text, .. } => assert_eq!(text, "Kiyo & Co Ltd!"),
            other => panic!("{:?}", other),
        }
        // External ids are skipped, never fetched.
        let ev = events("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0//EN\" \"http://example.invalid/x.dtd\"><html/>");
        assert!(matches!(ev[0], Event::DocType { internal_subset: None, .. }));
    }

    #[test]
    fn external_undefined_and_recursive_entities_fail() {
        assert!(read_all(b"<!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><r>&x;</r>").is_err());
        assert!(read_all(b"<r>&nope;</r>").is_err());
        assert!(read_all(b"<!DOCTYPE r [<!ENTITY a \"&b;\"><!ENTITY b \"&a;\">]><r>&a;</r>").is_err());
        assert!(read_all(b"<!DOCTYPE r [<!ENTITY a \"x&a;\">]><r>&a;</r>").is_err());
    }

    #[test]
    fn billion_laughs_is_capped() {
        let mut d = String::from("<?xml version=\"1.0\"?>\n<!DOCTYPE lolz [\n <!ENTITY lol \"lol\">\n");
        d.push_str(" <!ENTITY lol1 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\">\n");
        for k in 2..=9 {
            let p = format!("&lol{};", k - 1);
            d.push_str(&format!(" <!ENTITY lol{} \"{}\">\n", k, p.repeat(10)));
        }
        d.push_str("]>\n<lolz>&lol9;</lolz>");
        let started = std::time::Instant::now();
        let err = read_all(d.as_bytes()).unwrap_err();
        assert!(err.message.contains("cap"), "{}", err);
        assert!(started.elapsed().as_secs() < 5);
        // Many references to a mid-sized entity are capped by the document total.
        let mut d = String::from("<!DOCTYPE r [<!ENTITY big \"");
        d.push_str(&"x".repeat(1 << 19));
        d.push_str("\">]><r>");
        d.push_str(&"&big;".repeat(40));
        d.push_str("</r>");
        let err = read_all(d.as_bytes()).unwrap_err();
        assert!(err.message.contains("cap"), "{}", err);
    }

    #[test]
    fn cdata_and_comments() {
        let ev = events("<a><!-- note --><![CDATA[<raw> & ]] stuff]]></a><!--after-->");
        assert!(matches!(ev[1], Event::Comment { text: " note ", .. }));
        assert!(matches!(ev[2], Event::CData { text: "<raw> & ]] stuff", .. }));
        assert!(matches!(ev[4], Event::Comment { text: "after", .. }));
        assert!(read_all(b"<a><!-- bad -- comment --></a>").is_err());
        assert!(read_all(b"<a><!-- bad ---></a>").is_err());
        assert!(read_all(b"<![CDATA[x]]><a/>").is_err());
    }

    #[test]
    fn processing_instructions_and_decl() {
        let ev = events("<?xml version=\"1.0\" encoding='UTF-8' standalone=\"yes\"?><?mso-application progid=\"Word.Document\"?><a><?pi   data here?><?bare?></a>");
        match &ev[0] {
            Event::Decl { version, encoding, standalone, span } => {
                assert_eq!((*version, *encoding, *standalone), ("1.0", Some("UTF-8"), Some("yes")));
                assert_eq!(span.start, 0);
            }
            other => panic!("{:?}", other),
        }
        assert!(matches!(ev[1], Event::PI { target: "mso-application", data: "progid=\"Word.Document\"", .. }));
        assert!(matches!(ev[3], Event::PI { target: "pi", data: "data here", .. }));
        assert!(matches!(ev[4], Event::PI { target: "bare", data: "", .. }));
        assert!(read_all(b" <?xml version=\"1.0\"?><a/>").is_err(), "declaration not at the start");
        assert!(read_all(b"<a><?XML x?></a>").is_err(), "reserved target");
        assert!(read_all(b"<?xml encoding=\"UTF-8\"?><a/>").is_err(), "version missing");
        assert!(read_all(b"<?xml version=\"1.0\" standalone=\"maybe\"?><a/>").is_err());
    }

    #[test]
    fn attribute_quoting_both_kinds() {
        let ev = events("<a x='he said \"hi\"' y=\"it's\" z = 'sp' />");
        match &ev[0] {
            Event::Empty { attrs, .. } => {
                assert_eq!(attrs[0].value, "he said \"hi\"");
                assert_eq!(attrs[0].quote, b'\'');
                assert_eq!(attrs[1].value, "it's");
                assert_eq!(attrs[1].quote, b'"');
                assert_eq!(attrs[2].value, "sp");
            }
            other => panic!("{:?}", other),
        }
        let mut w = Writer::new();
        w.empty("a", &[("x", "he said \"hi\" & <left>"), ("y", "it's")]);
        assert_eq!(w.as_bytes(), b"<a x=\"he said &quot;hi&quot; &amp; &lt;left&gt;\" y=\"it's\"/>");
        let back = parse_dom(w.as_bytes()).unwrap();
        assert_eq!(back.attr("x"), Some("he said \"hi\" & <left>"));
    }

    #[test]
    fn self_closing_elements() {
        let ev = events("<a><b/><c x=\"1\" /><d></d></a>");
        assert!(matches!(&ev[1], Event::Empty { name, span, .. } if name.full == "b" && *span == (3..7)));
        assert!(matches!(&ev[2], Event::Empty { name, .. } if name.full == "c"));
        assert_eq!(ev[2].attr("x"), Some("1"));
        assert!(matches!(&ev[3], Event::Start { name, .. } if name.full == "d"));
        assert!(matches!(&ev[4], Event::End { name, .. } if name.full == "d"));
        let ev = events("<only/>");
        assert_eq!(ev.len(), 1);
    }

    #[test]
    fn ooxml_round_trip_is_byte_identical() {
        let doc = ooxml_doc();
        let src = doc.as_bytes();
        // Every event copied back by its span.
        let mut r = Reader::new(src).unwrap();
        let mut w = Writer::new();
        let mut last_end = 0;
        let mut count = 0;
        while let Some(ev) = r.next().unwrap() {
            let span = ev.span();
            assert_eq!(span.start, last_end, "spans are contiguous at {:?}", ev);
            last_end = span.end;
            w.raw(&src[span]);
            count += 1;
        }
        assert!(count > 30);
        assert_eq!(w.as_bytes(), src);

        // Known elements rewritten from their parts, unknown ones copied whole with subtree_span.
        let mut r = Reader::new(src).unwrap();
        let mut w = Writer::new();
        let mut unknown = 0;
        while let Some(ev) = r.next().unwrap() {
            match ev {
                Event::Start { name, span, .. } if name.local == "unknownExt" => {
                    let full = r.subtree_span().unwrap();
                    assert_eq!(full.start, span.start);
                    assert!(doc[full.clone()].ends_with("</w14:unknownExt>"));
                    assert!(doc[full.clone()].contains("CDATA"));
                    w.raw(&src[full]);
                    unknown += 1;
                }
                other => w.raw(&src[other.span()]),
            }
        }
        assert_eq!(unknown, 1);
        assert_eq!(w.as_bytes(), src);

        // The DOM keeps the meaning (and the comment, PI and CDATA bytes).
        let dom = parse_dom(src).unwrap();
        assert_eq!(dom.name, "w:document");
        let p = dom.child("body").unwrap().child("w:p").unwrap();
        assert_eq!(p.attr("w:rsidR"), Some("00AB12CD"));
        let t = p.child("r").unwrap().child("t").unwrap();
        assert_eq!(t.text(), "Fish & Chips <today> \u{a9} \u{2014} caf\u{e9}  ");
        assert!(p.children.iter().any(|c| matches!(c, Node::Raw(b) if b == b"<!-- a comment the editor never touches -->")));
        assert!(p.children.iter().any(|c| matches!(c, Node::Raw(b) if b == b"<?mso-hint keep me?>")));
        let again = parse_dom(&write_dom(&dom)).unwrap();
        assert_eq!(again, dom);
        assert!(write_dom(&dom).starts_with(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:document "));
    }

    #[test]
    fn utf8_bom_round_trip() {
        let mut src = vec![0xEF, 0xBB, 0xBF];
        src.extend_from_slice(b"<?xml version=\"1.0\"?><a>x</a>");
        let mut r = Reader::new(&src).unwrap();
        assert_eq!(r.bom_len(), 3);
        let mut w = Writer::new();
        w.raw(&src[..r.bom_len()]);
        while let Some(ev) = r.next().unwrap() {
            w.raw(&src[ev.span()]);
        }
        assert_eq!(w.into_bytes(), src);
    }

    #[test]
    fn utf16_is_transcoded() {
        let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><a k=\"v\">h\u{e9}llo \u{1F600}</a>";
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut le = vec![0xFF, 0xFE];
        let mut be = vec![0xFE, 0xFF];
        let mut le_nobom = Vec::new();
        for u in &units {
            le.extend_from_slice(&u.to_le_bytes());
            be.extend_from_slice(&u.to_be_bytes());
            le_nobom.extend_from_slice(&u.to_le_bytes());
        }
        for bytes in [&le, &be, &le_nobom] {
            assert!(Reader::new(bytes).is_err(), "the reader wants UTF-8");
            let utf8 = to_utf8(bytes).unwrap();
            assert!(matches!(utf8, Cow::Owned(_)));
            let s = std::str::from_utf8(&utf8).unwrap();
            assert!(s.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"), "{}", s);
            let ev = events(s);
            assert!(matches!(&ev[2], Event::Text { text, .. } if text == "h\u{e9}llo \u{1F600}"));
            assert_eq!(parse_dom(bytes).unwrap().text(), "h\u{e9}llo \u{1F600}");
        }
        assert!(matches!(to_utf8(b"<a/>").unwrap(), Cow::Borrowed(_)));
        assert!(to_utf8(&[0xFF, 0xFE, b'<']).is_err(), "odd length");
        assert!(to_utf8(&[0xFF, 0xFE, 0x00, 0xD8, b'a', 0]).is_err(), "unpaired surrogate");
    }

    #[test]
    fn subtree_span_on_nested_same_name_elements() {
        let src = "<r><g a=\"1\"><g><g/></g><x>t</x><g></g></g><tail/></r>";
        let mut r = Reader::new(src.as_bytes()).unwrap();
        assert!(matches!(r.next().unwrap(), Some(Event::Start { .. })));
        let outer = r.next().unwrap().unwrap();
        assert!(matches!(&outer, Event::Start { name, .. } if name.full == "g"));
        let span = r.subtree_span().unwrap();
        assert_eq!(&src[span], "<g a=\"1\"><g><g/></g><x>t</x><g></g></g>");
        assert_eq!(r.depth(), 1);
        assert!(matches!(r.next().unwrap(), Some(Event::Empty { name, .. }) if name.full == "tail"));
        assert!(r.subtree_span().is_err(), "only right after a Start");

        // A subtree that is never closed is an error, not a hang.
        let mut r = Reader::new(b"<r><g><g></g>").unwrap();
        r.next().unwrap();
        r.next().unwrap();
        assert!(r.subtree_span().is_err());
    }

    #[test]
    fn malformed_inputs_are_errors() {
        let cases: &[&[u8]] = &[
            b"",
            b"   ",
            b"<a></b>",
            b"<a><b></a></b>",
            b"<a><b>",
            b"<a",
            b"<a x=\"1>",
            b"<a x=1/>",
            b"<a x=\"1\"y=\"2\"/>",
            b"<a x=\"1\" x=\"2\"/>",
            b"<a x=\"<\"/>",
            b"<a><!-- never closed</a>",
            b"<a>&#xFFFFFFFFF;</a>",
            b"<a>&#0;</a>",
            b"<a>&#xD800;</a>",
            b"<a>&#+65;</a>",
            b"<a>&#X41;</a>",
            b"<a>&bogus;</a>",
            b"<a>&amp</a>",
            b"<a>& b;</a>",
            b"<a/><b/>",
            b"x<a/>",
            b"<a/>x",
            b"</a>",
            b"<a>]]></a>",
            b"<a:b:c/>",
            b"<:a/>",
            b"<1a/>",
            b"< a/>",
            b"<a/ >",
            b"<a>\x01</a>",
            b"<a>\xff</a>",
            b"<!DOCTYPE a><!DOCTYPE a><a/>",
            b"<a/><!DOCTYPE a>",
            b"<!DOCTYPE a [<!ENTITY x \"y\">",
            b"<!WEIRD><a/>",
            b"<a><![CDATA[open</a>",
            b"<a><?pi never</a>",
        ];
        for case in cases {
            let r = read_all(case);
            assert!(r.is_err(), "expected an error for {:?}", String::from_utf8_lossy(case));
        }
        // Errors carry line and column, and stay sticky.
        let src = b"<a>\n  <b></c></a>";
        let mut r = Reader::new(src).unwrap();
        let err = loop {
            match r.next() {
                Ok(Some(_)) => {}
                Ok(None) => panic!("should fail"),
                Err(e) => break e,
            }
        };
        assert_eq!((err.line, err.column), (2, 6), "{}", err);
        assert!(err.message.contains("mismatched"));
        assert_eq!(r.next().unwrap_err(), err);
        assert!(err.to_string().contains("line 2, column 6"));
    }

    #[test]
    fn truncated_and_mutated_input_never_panics() {
        let doc = ooxml_doc();
        let src = doc.as_bytes();
        for cut in 0..src.len() {
            assert!(read_all(&src[..cut]).is_err(), "prefix of {} bytes should be an error", cut);
        }
        for i in (0..src.len()).step_by(3) {
            for byte in [b'<', b'>', b'&', b'"', b'\'', b'/', b';', 0xFF, b'x'] {
                let mut m = src.to_vec();
                m[i] = byte;
                let _ = read_all(&m);
                let _ = parse_dom(&m);
            }
        }
    }

    #[test]
    fn nesting_depth_is_capped() {
        let deep = format!("{}{}", "<a>".repeat(MAX_DEPTH + 1), "</a>".repeat(MAX_DEPTH + 1));
        let err = read_all(deep.as_bytes()).unwrap_err();
        assert!(err.message.contains("deep"));
        let ok = format!("{}{}", "<a>".repeat(100), "</a>".repeat(100));
        assert!(read_all(ok.as_bytes()).is_ok());
    }

    #[test]
    fn writer_escapes_and_sections() {
        let mut w = Writer::new();
        w.decl(None);
        w.start("doc", NO_ATTRS);
        w.text("a & b < c > d\r");
        w.cdata("x ]]> y");
        w.comment("no -- dashes-");
        w.pi("go", "now ?> later");
        w.raw(b"<keep me=\"1\"/>");
        w.end("doc");
        let out = String::from_utf8(w.into_bytes()).unwrap();
        assert_eq!(
            out,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><doc>a &amp; b &lt; c &gt; d&#xD;\
             <![CDATA[x ]]]]><![CDATA[> y]]><!--no - - dashes- --><?go now ? > later?><keep me=\"1\"/></doc>"
        );
        let ev = events(&out);
        assert!(matches!(&ev[2], Event::Text { text, .. } if text == "a & b < c > d\r"));
        let mut cdata = String::new();
        for e in &ev {
            if let Event::CData { text, .. } = e {
                cdata.push_str(text);
            }
        }
        assert_eq!(cdata, "x ]]> y");
        assert_eq!(escape_attr("\t\n\"'"), "&#x9;&#xA;&quot;'");
        assert!(matches!(escape_text("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn writer_pretty_indentation() {
        let mut w = Writer::pretty("  ");
        w.start("a", &[("x", "1")]);
        w.empty("b", NO_ATTRS);
        w.start("c", NO_ATTRS);
        w.text("hi");
        w.empty("i", NO_ATTRS);
        w.end("c");
        w.start("d", NO_ATTRS);
        w.end("d");
        w.end("a");
        assert_eq!(
            String::from_utf8(w.into_bytes()).unwrap(),
            "<a x=\"1\">\n  <b/>\n  <c>hi<i/></c>\n  <d></d>\n</a>"
        );
        let mut w = Writer::new();
        w.start("a", NO_ATTRS);
        w.empty("b", NO_ATTRS);
        w.end("a");
        assert_eq!(w.as_bytes(), b"<a><b/></a>", "compact by default");
    }

    #[test]
    fn dom_helpers() {
        let src = "<x:root xmlns:x=\"urn:x\" id=\"7\"><x:item n=\"1\">one</x:item><other/><x:item n=\"2\">t<![CDATA[w]]>o</x:item></x:root>";
        let mut root = parse_dom(src.as_bytes()).unwrap();
        assert_eq!(root.local(), "root");
        assert_eq!(root.prefix(), Some("x"));
        assert_eq!(root.attr("id"), Some("7"));
        assert_eq!(root.attr("missing"), None);
        assert_eq!(root.child("item").unwrap().attr("n"), Some("1"));
        assert_eq!(root.child("x:item").unwrap().text(), "one");
        assert!(root.child("y:item").is_none());
        assert_eq!(root.children_named("item").count(), 2);
        assert_eq!(root.children_named("item").nth(1).unwrap().text(), "two");
        assert_eq!(root.elements().count(), 3);
        root.child_mut("other").unwrap().set_attr("k", "v & w");
        root.set_attr("id", "8");
        let mut extra = Element::new("x:item");
        extra.children.push(Node::Text("<3".into()));
        root.children.push(Node::Element(extra));
        let out = write_dom(&root);
        let back = parse_dom(&out).unwrap();
        assert_eq!(back, root);
        assert_eq!(back.attr("id"), Some("8"));
        assert_eq!(back.child("other").unwrap().attr("k"), Some("v & w"));
        assert!(String::from_utf8(out).unwrap().contains("<other k=\"v &amp; w\"/>"));
    }
}
