//! OPC: the Open Packaging Conventions (ECMA-376 Part 2) under `.docx`, `.xlsx` and `.pptx`.
//!
//! The promise of this module is that **nothing unknown is ever lost**:
//!
//! * A [`Package`] keeps every zip entry in its original order, with its original [`ZipEntry`] and its
//!   compressed bytes. A part becomes dirty only through [`Package::set_part_bytes`],
//!   [`Package::set_xml`], [`Package::edit_xml`] or a change the package itself has to make
//!   (relationships, content types, core properties).
//! * [`Package::save`] returns the input bytes unchanged when nothing was modified. Otherwise untouched
//!   parts are copied with [`ZipWriter::add_raw`] (same compressed bytes, order, times, extras,
//!   comments), dirty and new parts are deflated, and `[Content_Types].xml` and relationship parts are
//!   rewritten only when what they say changed.
//! * Even a rewritten `[Content_Types].xml`, `.rels`, `core.xml` or `app.xml` is *patched*, not
//!   regenerated: [`patch_xml`] copies every element the change did not touch byte for byte, so
//!   unknown elements, attributes, comments and `mc:AlternateContent` / `mc:Ignorable` markup survive.
//!
//! Part names are kept as found in the zip with a leading `/` (`/word/document.xml`). Every lookup is
//! case-insensitive and percent-decoding (`/word/media/image%201.png` == `/WORD/media/image 1.png`).
//!
//! Limits, honestly:
//! * Only the archive layout written by Office-like producers (and by our own [`ZipWriter`]) is
//!   reproduced byte-identically once something changed: [`ZipWriter::add_raw`] writes the local
//!   header from the central directory record, so an archive whose local headers disagree with its
//!   central directory gets consistent headers on the first modified save. The entry data is always
//!   kept exactly.
//! * Encrypted (`EncryptedPackage` / OLE) files are not zips and give an error.
//! * A relationships part that cannot be parsed is kept untouched and reported in
//!   [`Package::warnings`]; adding a relationship to its source replaces it with a fresh part.
//! * Interleaved (`[trash]`, piece-split) packages from the old XPS days are not joined.

use super::xml::{self, Element, Event, Node, Reader, XmlError};
use super::zip::{Method, ZipArchive, ZipEntry, ZipError, ZipWriter};
use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

// ---------------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------------

const REL_T: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const REL_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";

/// Relationship type: the package's main part.
pub const REL_OFFICE_DOCUMENT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
/// Relationship type: the main part of a Strict (ISO 29500 strict) document.
pub const REL_OFFICE_DOCUMENT_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
/// Relationship type: core properties (`docProps/core.xml`).
pub const REL_CORE_PROPERTIES: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
/// Relationship type: extended (app) properties (`docProps/app.xml`).
pub const REL_EXTENDED_PROPERTIES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
/// Relationship type: custom properties (`docProps/custom.xml`).
pub const REL_CUSTOM_PROPERTIES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
/// Relationship type: the package thumbnail.
pub const REL_THUMBNAIL: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
/// Relationship type: styles.
pub const REL_STYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
/// Relationship type: theme.
pub const REL_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
/// Relationship type: image.
pub const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
/// Relationship type: hyperlink (normally `TargetMode="External"`).
pub const REL_HYPERLINK: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
/// Relationship type: worksheet.
pub const REL_WORKSHEET: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
/// Relationship type: slide.
pub const REL_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
/// Relationship type: slide layout.
pub const REL_SLIDE_LAYOUT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
/// Relationship type: slide master.
pub const REL_SLIDE_MASTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
/// Relationship type: shared strings.
pub const REL_SHARED_STRINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
/// Relationship type: numbering.
pub const REL_NUMBERING: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
/// Relationship type: font table.
pub const REL_FONT_TABLE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable";
/// Relationship type: settings.
pub const REL_SETTINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
/// Relationship type: header.
pub const REL_HEADER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
/// Relationship type: footer.
pub const REL_FOOTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
/// Relationship type: comments.
pub const REL_COMMENTS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";

/// Content type of a relationships part.
pub const CT_RELATIONSHIPS: &str = "application/vnd.openxmlformats-package.relationships+xml";
/// Content type of the core properties part.
pub const CT_CORE_PROPERTIES: &str = "application/vnd.openxmlformats-package.core-properties+xml";
/// Content type of the extended (app) properties part.
pub const CT_EXTENDED_PROPERTIES: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
/// Plain XML.
pub const CT_XML: &str = "application/xml";
/// PNG image.
pub const CT_PNG: &str = "image/png";
/// JPEG image.
pub const CT_JPEG: &str = "image/jpeg";
/// WordprocessingML main document (`.docx`).
pub const CT_WORDPROCESSINGML_MAIN: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
/// SpreadsheetML workbook (`.xlsx`).
pub const CT_SPREADSHEETML_MAIN: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
/// PresentationML presentation (`.pptx`).
pub const CT_PRESENTATIONML_MAIN: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
/// WordprocessingML styles.
pub const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
/// SpreadsheetML styles.
pub const CT_SPREADSHEETML_STYLES: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
/// Theme.
pub const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
/// Worksheet.
pub const CT_WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
/// Shared strings.
pub const CT_SHARED_STRINGS: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
/// Slide.
pub const CT_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
/// Slide layout.
pub const CT_SLIDE_LAYOUT: &str = "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
/// Slide master.
pub const CT_SLIDE_MASTER: &str = "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
/// Numbering.
pub const CT_NUMBERING: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
/// Font table.
pub const CT_FONT_TABLE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml";
/// Settings.
pub const CT_SETTINGS: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
/// Header.
pub const CT_HEADER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
/// Footer.
pub const CT_FOOTER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
/// Comments.
pub const CT_COMMENTS: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";

/// Namespace of `[Content_Types].xml`.
pub const NS_CONTENT_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
/// Namespace of relationships parts.
pub const NS_RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
/// Namespace of core properties (`cp:`).
pub const NS_CORE: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
/// Dublin Core elements (`dc:`).
pub const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
/// Dublin Core terms (`dcterms:`).
pub const NS_DCTERMS: &str = "http://purl.org/dc/terms/";
/// Dublin Core types (`dcmitype:`).
pub const NS_DCMITYPE: &str = "http://purl.org/dc/dcmitype/";
/// XML Schema instance (`xsi:`).
pub const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
/// Extended (app) properties.
pub const NS_EXTENDED: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";

/// The name of the content types item.
pub const CONTENT_TYPES_NAME: &str = "/[Content_Types].xml";

const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";
/// How far ahead [`patch_xml`] looks for an unchanged sibling to copy verbatim.
const MATCH_WINDOW: usize = 256;

// ---------------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------------

/// What went wrong with a package. The message says what and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpcError(pub String);

impl fmt::Display for OpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "opc: {}", self.0)
    }
}

impl std::error::Error for OpcError {}

impl From<ZipError> for OpcError {
    fn from(e: ZipError) -> Self {
        OpcError(e.to_string())
    }
}

impl From<XmlError> for OpcError {
    fn from(e: XmlError) -> Self {
        OpcError(e.to_string())
    }
}

fn err<T>(msg: impl Into<String>) -> Result<T, OpcError> {
    Err(OpcError(msg.into()))
}

// ---------------------------------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------------------------------

/// A part name in package form: backslashes turned into `/`, a leading `/` added. Nothing else is
/// changed (percent-encoding and case are kept as given).
pub fn normalize_name(name: &str) -> String {
    let n = name.replace('\\', "/");
    if n.starts_with('/') {
        n
    } else {
        format!("/{n}")
    }
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Decodes `%XX` sequences (invalid ones are left as they are; invalid UTF-8 becomes U+FFFD).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The comparison key of a part name: normalized, percent-decoded, lower-cased.
pub fn name_key(name: &str) -> String {
    percent_decode(&normalize_name(name)).to_lowercase()
}

/// True when two part names denote the same part (case-insensitive, percent-decoding).
pub fn same_part(a: &str, b: &str) -> bool {
    name_key(a) == name_key(b)
}

/// True for a target that is a URL (has a scheme like `https:`, `mailto:`, `file:`) or a network
/// path (`//host/...`): such targets are never resolved against the package.
pub fn is_external_url(target: &str) -> bool {
    if target.starts_with("//") || target.starts_with("\\\\") {
        return true;
    }
    let b = target.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphabetic() {
        return false;
    }
    for (i, &c) in b.iter().enumerate() {
        match c {
            b':' => return i >= 2,
            c if c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.' => {}
            _ => return false,
        }
    }
    false
}

fn base_dir(source: &str) -> String {
    let s = normalize_name(source);
    match s.rfind('/') {
        Some(i) => s[..=i].to_string(),
        None => "/".to_string(),
    }
}

fn normalize_dots(path: &str) -> String {
    let mut segs: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    format!("/{}", segs.join("/"))
}

/// Resolves a relationship `target` written in the relationships of `source` (a part name, or `/`
/// for the package itself) to an absolute part name: relative targets are taken from the source's
/// folder, `../` climbs (never above the root), an absolute `/x` stays absolute, a `#fragment` is cut
/// off (a bare `#fragment` points at the source itself) and URLs are returned untouched.
pub fn resolve_target(source: &str, target: &str) -> String {
    if is_external_url(target) {
        return target.to_string();
    }
    let t = target.split('#').next().unwrap_or("").replace('\\', "/");
    if t.is_empty() {
        return normalize_dots(&normalize_name(source));
    }
    let joined = if t.starts_with('/') { t } else { format!("{}{}", base_dir(source), t) };
    normalize_dots(&joined)
}

/// The relative target that [`resolve_target`] turns back into `part` from `source`
/// (`/word/document.xml` + `/word/media/a.png` gives `media/a.png`; from `/` it is `word/document.xml`).
pub fn relative_target(source: &str, part: &str) -> String {
    let base = base_dir(source);
    let base_segs: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    let part = normalize_dots(&normalize_name(part));
    let part_segs: Vec<&str> = part.split('/').filter(|s| !s.is_empty()).collect();
    let mut common = 0;
    while common < base_segs.len()
        && common + 1 < part_segs.len()
        && base_segs[common].eq_ignore_ascii_case(part_segs[common])
    {
        common += 1;
    }
    let mut out = String::new();
    for _ in common..base_segs.len() {
        out.push_str("../");
    }
    out.push_str(&part_segs[common..].join("/"));
    out
}

/// The relationships part that holds the relationships of `source` (`/` gives `/_rels/.rels`,
/// `/word/document.xml` gives `/word/_rels/document.xml.rels`).
pub fn rels_part_name(source: &str) -> String {
    let s = normalize_name(source);
    if s == "/" {
        return "/_rels/.rels".to_string();
    }
    let i = s.rfind('/').unwrap_or(0);
    format!("{}_rels/{}.rels", &s[..=i], &s[i + 1..])
}

/// The source part of a relationships part name, or `None` when `rels_part` is not one.
pub fn rels_source(rels_part: &str) -> Option<String> {
    let n = normalize_name(rels_part);
    if !n.to_ascii_lowercase().ends_with(".rels") {
        return None;
    }
    let slash = n.rfind('/')?;
    let dir = &n[..slash];
    let file = &n[slash + 1..];
    if !dir.to_ascii_lowercase().ends_with("/_rels") {
        return None;
    }
    let parent = &dir[..dir.len() - 6];
    let src_file = &file[..file.len() - 5];
    Some(format!("{parent}/{src_file}"))
}

fn extension_of(name: &str) -> Option<&str> {
    let file = name.rsplit('/').next().unwrap_or(name);
    file.rfind('.').map(|i| &file[i + 1..]).filter(|e| !e.is_empty())
}

fn valid_new_name(name: &str) -> Result<(), OpcError> {
    if name == "/" || name.ends_with('/') || name.len() > 0xFFFF {
        return err(format!("invalid part name {name:?}"));
    }
    for seg in name[1..].split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.ends_with('.') {
            return err(format!("invalid part name {name:?}: bad segment {seg:?}"));
        }
    }
    if name_key(name) == name_key(CONTENT_TYPES_NAME) {
        return err("[Content_Types].xml is not a part; it is managed by the package");
    }
    Ok(())
}

/// Two relationship types are the same if equal, or the same type in the Transitional and Strict
/// namespaces.
pub fn rel_type_matches(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let tail = |s: &str| s.strip_prefix(REL_T).or_else(|| s.strip_prefix(REL_STRICT)).map(str::to_string);
    matches!((tail(a), tail(b)), (Some(x), Some(y)) if x == y)
}

// ---------------------------------------------------------------------------------------------------
// Content types
// ---------------------------------------------------------------------------------------------------

/// `[Content_Types].xml`: defaults by extension and overrides by part name, in document order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentTypes {
    /// `(Extension, ContentType)` as written.
    pub defaults: Vec<(String, String)>,
    /// `(PartName, ContentType)` as written.
    pub overrides: Vec<(String, String)>,
}

impl ContentTypes {
    /// Parses a `[Content_Types].xml`.
    pub fn parse(bytes: &[u8]) -> Result<ContentTypes, OpcError> {
        let root = xml::parse_dom(bytes)?;
        if root.local() != "Types" {
            return err(format!("[Content_Types].xml: root is <{}>, not <Types>", root.name));
        }
        let mut ct = ContentTypes::default();
        for e in root.elements() {
            match e.local() {
                "Default" => {
                    if let (Some(x), Some(c)) = (e.attr("Extension"), e.attr("ContentType")) {
                        ct.defaults.push((x.to_string(), c.to_string()));
                    }
                }
                "Override" => {
                    if let (Some(p), Some(c)) = (e.attr("PartName"), e.attr("ContentType")) {
                        ct.overrides.push((p.to_string(), c.to_string()));
                    }
                }
                _ => {}
            }
        }
        Ok(ct)
    }

    /// The Default content type for an extension (case-insensitive).
    pub fn default_for(&self, ext: &str) -> Option<&str> {
        self.defaults.iter().find(|(x, _)| x.eq_ignore_ascii_case(ext)).map(|(_, c)| c.as_str())
    }

    /// The Override content type for a part (case-insensitive, percent-decoding).
    pub fn override_for(&self, part: &str) -> Option<&str> {
        let k = name_key(part);
        self.overrides.iter().find(|(p, _)| name_key(p) == k).map(|(_, c)| c.as_str())
    }

    /// The content type of a part: its Override, else the Default of its extension.
    pub fn lookup(&self, part: &str) -> Option<&str> {
        self.override_for(part).or_else(|| extension_of(part).and_then(|e| self.default_for(e)))
    }

    fn set_override(&mut self, part: &str, ct: &str) {
        let k = name_key(part);
        match self.overrides.iter_mut().find(|(p, _)| name_key(p) == k) {
            Some(slot) => slot.1 = ct.to_string(),
            None => self.overrides.push((part.to_string(), ct.to_string())),
        }
    }

    fn remove_override(&mut self, part: &str) -> bool {
        let k = name_key(part);
        let before = self.overrides.len();
        self.overrides.retain(|(p, _)| name_key(p) != k);
        before != self.overrides.len()
    }

    fn to_xml(&self, base: Option<&[u8]>) -> Vec<u8> {
        let defaults: Vec<Vec<(&str, Want)>> = self
            .defaults
            .iter()
            .map(|(x, c)| vec![("Extension", Want::Set(x.clone())), ("ContentType", Want::Set(c.clone()))])
            .collect();
        let overrides: Vec<Vec<(&str, Want)>> = self
            .overrides
            .iter()
            .map(|(p, c)| vec![("PartName", Want::Set(p.clone())), ("ContentType", Want::Set(c.clone()))])
            .collect();
        let edit = |root: &mut Element| {
            sync_children(root, "Default", &defaults, &|a, b| a.eq_ignore_ascii_case(b));
            sync_children(root, "Override", &overrides, &|a, b| same_part(a, b));
        };
        let skeleton = format!("{XML_DECL}<Types xmlns=\"{NS_CONTENT_TYPES}\"></Types>");
        match base.map(|b| try_patch_xml(b, edit)) {
            Some(Ok(v)) => v,
            _ => patch_xml(skeleton.as_bytes(), edit),
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Relationships
// ---------------------------------------------------------------------------------------------------

/// One `<Relationship>`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Relationship {
    pub id: String,
    pub rel_type: String,
    /// The target exactly as written (relative, absolute, or a URL; maybe with a `#fragment`).
    pub target: String,
    /// `TargetMode="External"`.
    pub target_mode_external: bool,
}

/// Parses a relationships part. Entries missing `Type` or `Target` are kept with empty strings so
/// that a rewrite never drops them.
pub fn parse_rels(bytes: &[u8]) -> Result<Vec<Relationship>, OpcError> {
    let root = xml::parse_dom(bytes)?;
    if root.local() != "Relationships" {
        return err(format!("relationships part: root is <{}>, not <Relationships>", root.name));
    }
    Ok(root
        .elements()
        .filter(|e| e.local() == "Relationship")
        .filter_map(|e| {
            Some(Relationship {
                id: e.attr("Id")?.to_string(),
                rel_type: e.attr("Type").unwrap_or("").to_string(),
                target: e.attr("Target").unwrap_or("").to_string(),
                target_mode_external: e.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External")),
            })
        })
        .collect())
}

fn rels_to_xml(base: Option<&[u8]>, list: &[Relationship]) -> Vec<u8> {
    let wanted: Vec<Vec<(&str, Want)>> = list
        .iter()
        .map(|r| {
            vec![
                ("Id", Want::Set(r.id.clone())),
                ("Type", Want::Set(r.rel_type.clone())),
                ("Target", Want::Set(r.target.clone())),
                (
                    "TargetMode",
                    if r.target_mode_external { Want::Set("External".into()) } else { Want::RemoveUnless("Internal") },
                ),
            ]
        })
        .collect();
    let edit = |root: &mut Element| sync_children(root, "Relationship", &wanted, &|a, b| a == b);
    let skeleton = format!("{XML_DECL}<Relationships xmlns=\"{NS_RELATIONSHIPS}\"></Relationships>");
    match base.map(|b| try_patch_xml(b, edit)) {
        Some(Ok(v)) => v,
        _ => patch_xml(skeleton.as_bytes(), edit),
    }
}

#[derive(Debug, Clone)]
struct RelSet {
    /// Normalized source part name (`/` for the package).
    source: String,
    list: Vec<Relationship>,
    /// The part could not be parsed; `list` is empty and the part is left as it was.
    broken: bool,
}

// ---------------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------------

/// `docProps/core.xml`. `None` means the element is absent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoreProperties {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub keywords: Option<String>,
    pub description: Option<String>,
    pub last_modified_by: Option<String>,
    pub revision: Option<String>,
    /// W3CDTF, e.g. `2026-11-26T09:00:00Z`.
    pub created: Option<String>,
    /// W3CDTF.
    pub modified: Option<String>,
    pub category: Option<String>,
}

/// `docProps/app.xml`: the two fields we read. Everything else in the part stays as it is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppProperties {
    pub application: Option<String>,
    pub app_version: Option<String>,
}

struct PropField<'a> {
    local: &'static str,
    ns: &'static str,
    prefix: &'static str,
    value: Option<&'a str>,
    w3cdtf: bool,
}

fn core_fields(c: &CoreProperties) -> Vec<PropField<'_>> {
    fn f<'a>(local: &'static str, ns: &'static str, prefix: &'static str, value: &'a Option<String>, w3cdtf: bool) -> PropField<'a> {
        PropField { local, ns, prefix, value: value.as_deref(), w3cdtf }
    }
    vec![
        f("title", NS_DC, "dc", &c.title, false),
        f("subject", NS_DC, "dc", &c.subject, false),
        f("creator", NS_DC, "dc", &c.creator, false),
        f("keywords", NS_CORE, "cp", &c.keywords, false),
        f("description", NS_DC, "dc", &c.description, false),
        f("lastModifiedBy", NS_CORE, "cp", &c.last_modified_by, false),
        f("revision", NS_CORE, "cp", &c.revision, false),
        f("created", NS_DCTERMS, "dcterms", &c.created, true),
        f("modified", NS_DCTERMS, "dcterms", &c.modified, true),
        f("category", NS_CORE, "cp", &c.category, false),
    ]
}

fn app_fields(a: &AppProperties) -> Vec<PropField<'_>> {
    vec![
        PropField { local: "Application", ns: NS_EXTENDED, prefix: "", value: a.application.as_deref(), w3cdtf: false },
        PropField { local: "AppVersion", ns: NS_EXTENDED, prefix: "", value: a.app_version.as_deref(), w3cdtf: false },
    ]
}

fn read_field(root: &Element, local: &str) -> Option<String> {
    root.elements().find(|e| e.local() == local).map(|e| e.text())
}

/// Parses `docProps/core.xml`.
pub fn parse_core(bytes: &[u8]) -> Result<CoreProperties, OpcError> {
    let r = xml::parse_dom(bytes)?;
    Ok(CoreProperties {
        title: read_field(&r, "title"),
        subject: read_field(&r, "subject"),
        creator: read_field(&r, "creator"),
        keywords: read_field(&r, "keywords"),
        description: read_field(&r, "description"),
        last_modified_by: read_field(&r, "lastModifiedBy"),
        revision: read_field(&r, "revision"),
        created: read_field(&r, "created"),
        modified: read_field(&r, "modified"),
        category: read_field(&r, "category"),
    })
}

/// Parses the two known fields of `docProps/app.xml`.
pub fn parse_app(bytes: &[u8]) -> Result<AppProperties, OpcError> {
    let r = xml::parse_dom(bytes)?;
    Ok(AppProperties { application: read_field(&r, "Application"), app_version: read_field(&r, "AppVersion") })
}

/// The prefix bound to `ns` on `root` (`Some("")` for the default namespace).
fn prefix_for(root: &Element, ns: &str) -> Option<String> {
    root.attrs.iter().find_map(|(k, v)| {
        if v != ns {
            None
        } else if k == "xmlns" {
            Some(String::new())
        } else {
            k.strip_prefix("xmlns:").map(str::to_string)
        }
    })
}

fn ensure_prefix(root: &mut Element, ns: &str, preferred: &str) -> String {
    if let Some(p) = prefix_for(root, ns) {
        return p;
    }
    let taken = |root: &Element, p: &str| {
        if p.is_empty() {
            root.attr("xmlns").is_some()
        } else {
            root.attr(&format!("xmlns:{p}")).is_some() || root.prefix() == Some(p)
        }
    };
    let mut p = if preferred.is_empty() { "ns".to_string() } else { preferred.to_string() };
    if !preferred.is_empty() || taken(root, "") {
        let mut n = 0;
        while taken(root, &p) {
            n += 1;
            p = format!("{}{n}", if preferred.is_empty() { "ns" } else { preferred });
        }
    } else {
        p.clear();
    }
    let key = if p.is_empty() { "xmlns".to_string() } else { format!("xmlns:{p}") };
    root.attrs.push((key, ns.to_string()));
    p
}

fn qualify(prefix: &str, local: &str) -> String {
    if prefix.is_empty() {
        local.to_string()
    } else {
        format!("{prefix}:{local}")
    }
}

fn apply_props(root: &mut Element, fields: &[PropField<'_>]) {
    for f in fields {
        let pos = root.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == f.local));
        match (pos, f.value) {
            (Some(i), Some(v)) => {
                if let Node::Element(e) = &mut root.children[i] {
                    if e.text() != v {
                        e.children = if v.is_empty() { Vec::new() } else { vec![Node::Text(v.to_string())] };
                    }
                }
            }
            (Some(i), None) => {
                root.children.remove(i);
            }
            (None, Some(v)) => {
                let p = ensure_prefix(root, f.ns, f.prefix);
                let mut e = Element::new(qualify(&p, f.local));
                if f.w3cdtf {
                    let x = ensure_prefix(root, NS_XSI, "xsi");
                    e.attrs.push((qualify(&x, "type"), qualify(&p, "W3CDTF")));
                }
                if !v.is_empty() {
                    e.children.push(Node::Text(v.to_string()));
                }
                root.children.push(Node::Element(e));
            }
            (None, None) => {}
        }
    }
}

fn core_skeleton() -> String {
    format!(
        "{XML_DECL}<cp:coreProperties xmlns:cp=\"{NS_CORE}\" xmlns:dc=\"{NS_DC}\" xmlns:dcterms=\"{NS_DCTERMS}\" \
         xmlns:dcmitype=\"{NS_DCMITYPE}\" xmlns:xsi=\"{NS_XSI}\"></cp:coreProperties>"
    )
}

fn app_skeleton() -> String {
    format!(
        "{XML_DECL}<Properties xmlns=\"{NS_EXTENDED}\" \
         xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"></Properties>"
    )
}

// ---------------------------------------------------------------------------------------------------
// patch_xml: edit what you know, copy what you don't
// ---------------------------------------------------------------------------------------------------

/// Where every node of a parsed element came from.
#[derive(Debug, Clone)]
struct SpanEl {
    start_tag: Range<usize>,
    end_tag: Option<Range<usize>>,
    full: Range<usize>,
    kids: Vec<SpanKid>,
}

#[derive(Debug, Clone)]
enum SpanKid {
    El(SpanEl),
    Other(Range<usize>),
}

impl SpanKid {
    fn range(&self) -> Range<usize> {
        match self {
            SpanKid::El(s) => s.full.clone(),
            SpanKid::Other(r) => r.clone(),
        }
    }
}

/// Parses like [`xml::parse_dom`], also recording each node's byte span.
fn parse_spanned(src: &[u8]) -> Result<(Element, SpanEl), XmlError> {
    let mut r = Reader::new(src)?;
    let mut stack: Vec<(Element, SpanEl)> = Vec::new();
    let mut root: Option<(Element, SpanEl)> = None;
    fn attach(stack: &mut [(Element, SpanEl)], root: &mut Option<(Element, SpanEl)>, el: Element, sp: SpanEl) {
        match stack.last_mut() {
            Some((p, ps)) => {
                p.children.push(Node::Element(el));
                ps.kids.push(SpanKid::El(sp));
            }
            None => *root = Some((el, sp)),
        }
    }
    let own = |attrs: Vec<xml::Attr<'_>>| -> Vec<(String, String)> {
        attrs.into_iter().map(|a| (a.name.full.to_string(), a.value.into_owned())).collect()
    };
    while let Some(ev) = r.next()? {
        match ev {
            Event::Start { name, attrs, span } => {
                let el = Element { name: name.full.to_string(), attrs: own(attrs), children: Vec::new() };
                let sp = SpanEl { start_tag: span.clone(), end_tag: None, full: span.start..span.end, kids: Vec::new() };
                stack.push((el, sp));
            }
            Event::Empty { name, attrs, span } => {
                let el = Element { name: name.full.to_string(), attrs: own(attrs), children: Vec::new() };
                let sp = SpanEl { start_tag: span.clone(), end_tag: None, full: span, kids: Vec::new() };
                attach(&mut stack, &mut root, el, sp);
            }
            Event::End { span, .. } => {
                if let Some((el, mut sp)) = stack.pop() {
                    sp.full.end = span.end;
                    sp.end_tag = Some(span);
                    attach(&mut stack, &mut root, el, sp);
                }
            }
            Event::Text { text, span, .. } => {
                if let Some((p, ps)) = stack.last_mut() {
                    let merged = match (p.children.last_mut(), ps.kids.last_mut()) {
                        (Some(Node::Text(t)), Some(SpanKid::Other(rg))) if rg.end == span.start => {
                            t.push_str(&text);
                            rg.end = span.end;
                            true
                        }
                        _ => false,
                    };
                    if !merged {
                        p.children.push(Node::Text(text.into_owned()));
                        ps.kids.push(SpanKid::Other(span));
                    }
                }
            }
            Event::CData { span, .. } | Event::Comment { span, .. } | Event::PI { span, .. } => {
                if let Some((p, ps)) = stack.last_mut() {
                    p.children.push(Node::Raw(src[span.clone()].to_vec()));
                    ps.kids.push(SpanKid::Other(span));
                }
            }
            Event::Decl { .. } | Event::DocType { .. } => {}
        }
    }
    root.ok_or_else(|| XmlError { message: "no root element".into(), offset: src.len(), line: 0, column: 0 })
}

fn write_tag_open(out: &mut Vec<u8>, e: &Element, empty: bool) {
    out.push(b'<');
    out.extend_from_slice(e.name.as_bytes());
    for (k, v) in &e.attrs {
        out.push(b' ');
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b"=\"");
        out.extend_from_slice(xml::escape_attr(v).as_bytes());
        out.push(b'"');
    }
    out.extend_from_slice(if empty { b"/>" } else { b">" });
}

fn emit_element(out: &mut Vec<u8>, src: &[u8], e: &Element, orig: Option<(&Element, &SpanEl)>) {
    if let Some((o, s)) = orig {
        if e == o {
            out.extend_from_slice(&src[s.full.clone()]);
            return;
        }
    }
    let same_head = orig.is_some_and(|(o, _)| o.name == e.name && o.attrs == e.attrs);
    if e.children.is_empty() {
        match orig {
            Some((o, s)) if same_head && (s.end_tag.is_none() || o.children.is_empty()) => {
                // `<a/>` stays `<a/>`; `<a></a>` (or `<a>x</a>` emptied) keeps its own tags.
                if s.end_tag.is_none() {
                    out.extend_from_slice(&src[s.start_tag.clone()]);
                } else if o.children.is_empty() {
                    out.extend_from_slice(&src[s.full.clone()]);
                }
            }
            Some((_, s)) if same_head => {
                out.extend_from_slice(&src[s.start_tag.clone()]);
                if let Some(end) = &s.end_tag {
                    out.extend_from_slice(&src[end.clone()]);
                }
            }
            _ => write_tag_open(out, e, true),
        }
        return;
    }
    let reuse_tags = same_head && orig.is_some_and(|(_, s)| s.end_tag.is_some());
    match orig {
        Some((_, s)) if reuse_tags => out.extend_from_slice(&src[s.start_tag.clone()]),
        _ => write_tag_open(out, e, false),
    }
    match orig {
        Some((o, s)) => emit_children(out, src, &e.children, &o.children, &s.kids),
        None => emit_children(out, src, &e.children, &[], &[]),
    }
    match orig {
        Some((_, s)) if reuse_tags => {
            if let Some(end) = &s.end_tag {
                out.extend_from_slice(&src[end.clone()]);
            }
        }
        _ => {
            out.extend_from_slice(b"</");
            out.extend_from_slice(e.name.as_bytes());
            out.push(b'>');
        }
    }
}

fn emit_children(out: &mut Vec<u8>, src: &[u8], new: &[Node], old: &[Node], spans: &[SpanKid]) {
    let mut ptr = 0;
    for n in new {
        let lim = (ptr + MATCH_WINDOW).min(old.len());
        if let Some(j) = (ptr..lim).find(|&j| &old[j] == n) {
            out.extend_from_slice(&src[spans[j].range()]);
            ptr = j + 1;
            continue;
        }
        match n {
            Node::Element(e) => {
                let partial = (ptr..lim).find_map(|j| match (&old[j], &spans[j]) {
                    (Node::Element(o), SpanKid::El(s)) if o.name == e.name => Some((o, s)),
                    _ => None,
                });
                emit_element(out, src, e, partial);
            }
            Node::Text(t) => out.extend_from_slice(xml::escape_text(t).as_bytes()),
            Node::Raw(b) => out.extend_from_slice(b),
        }
    }
}

/// Parses `original`, lets `edits` change the tree, and writes it back so that every subtree the
/// edit left equal is copied **byte for byte** from the original: unknown elements, their attribute
/// order and quoting, comments, CDATA, whitespace, entity spellings, the prolog and the epilog.
/// Nothing changed gives the original bytes exactly.
///
/// Limits: an element whose name or attributes changed gets a freshly written start tag (attributes
/// in `Element::attrs` order, double quotes, escaped), so original quoting and whitespace inside that
/// one tag are normalized; text nodes the edit replaced are written escaped; new elements must use
/// prefixes that are declared. A UTF-16 document that changed is written back as UTF-8 with a fresh
/// declaration. When the input is not well-formed, `original` is returned unchanged (use
/// [`try_patch_xml`] to see the error).
pub fn patch_xml(original: &[u8], edits: impl FnOnce(&mut Element)) -> Vec<u8> {
    try_patch_xml(original, edits).unwrap_or_else(|_| original.to_vec())
}

/// [`patch_xml`] that reports a parse error instead of returning the input.
pub fn try_patch_xml(original: &[u8], edits: impl FnOnce(&mut Element)) -> Result<Vec<u8>, OpcError> {
    let utf8 = xml::to_utf8(original)?;
    let transcoded = matches!(utf8, std::borrow::Cow::Owned(_));
    let src: &[u8] = &utf8;
    let (orig, spans) = parse_spanned(src)?;
    let mut edited = orig.clone();
    edits(&mut edited);
    if edited == orig {
        return Ok(original.to_vec());
    }
    if transcoded {
        return Ok(xml::write_dom(&edited));
    }
    let mut out = Vec::with_capacity(src.len() + 64);
    out.extend_from_slice(&src[..spans.full.start]);
    emit_element(&mut out, src, &edited, Some((&orig, &spans)));
    out.extend_from_slice(&src[spans.full.end..]);
    Ok(out)
}

/// What [`sync_children`] wants of one attribute.
enum Want {
    Set(String),
    /// Remove the attribute unless its value is this one (an explicit default like `Internal`).
    RemoveUnless(&'static str),
}

/// Makes the `local` children of `root` match `wanted` (each a list of attributes; the first is the
/// key): matching elements get their attributes updated in place (only when different), elements
/// whose key is no longer wanted are dropped, missing ones are added after the last sibling of the
/// same kind. Everything else (other elements, comments, whitespace) is left alone.
fn sync_children(root: &mut Element, local: &str, wanted: &[Vec<(&str, Want)>], key_eq: &dyn Fn(&str, &str) -> bool) {
    let mut seen = vec![false; wanted.len()];
    let old = std::mem::take(&mut root.children);
    let mut kept = Vec::with_capacity(old.len() + wanted.len());
    for child in old {
        let mut child = child;
        if let Node::Element(e) = &mut child {
            if e.local() == local {
                let key_attr = wanted.first().map_or(None, |w| w.first().map(|(k, _)| *k));
                let key_attr = key_attr.unwrap_or(match local {
                    "Default" => "Extension",
                    "Override" => "PartName",
                    _ => "Id",
                });
                if let Some(key) = e.attr(key_attr).map(str::to_string) {
                    let hit = (0..wanted.len()).find(|&i| {
                        !seen[i] && matches!(&wanted[i][0].1, Want::Set(k) if key_eq(k, &key))
                    });
                    match hit {
                        Some(i) => {
                            seen[i] = true;
                            for (name, want) in wanted[i].iter().skip(1) {
                                match want {
                                    Want::Set(v) => {
                                        if e.attr(name) != Some(v.as_str()) {
                                            e.set_attr(*name, v.clone());
                                        }
                                    }
                                    Want::RemoveUnless(keep) => {
                                        if e.attr(name).is_some_and(|v| v != *keep) {
                                            e.attrs.retain(|(k, _)| k != name);
                                        }
                                    }
                                }
                            }
                        }
                        None => continue, // dropped
                    }
                }
            }
        }
        kept.push(child);
    }
    let prefix = root.prefix().map(str::to_string);
    let name = match &prefix {
        Some(p) => format!("{p}:{local}"),
        None => local.to_string(),
    };
    let mut insert_at = kept
        .iter()
        .rposition(|c| matches!(c, Node::Element(e) if e.local() == local))
        .map_or(kept.len(), |i| i + 1);
    for (i, w) in wanted.iter().enumerate() {
        if seen[i] {
            continue;
        }
        let mut e = Element::new(name.clone());
        for (k, want) in w {
            if let Want::Set(v) = want {
                e.attrs.push((k.to_string(), v.clone()));
            }
        }
        kept.insert(insert_at, Node::Element(e));
        insert_at += 1;
    }
    root.children = kept;
}

// ---------------------------------------------------------------------------------------------------
// Parts and the package
// ---------------------------------------------------------------------------------------------------

/// One zip entry of the package (a part, `[Content_Types].xml`, or a directory entry).
#[derive(Debug, Clone)]
pub struct Part {
    name: String,
    entry: Option<ZipEntry>,
    raw: Range<usize>,
    data: Option<Vec<u8>>,
}

impl Part {
    /// The part name as found (`/word/document.xml`), percent-encoding and case kept.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Changed (or new) since the package was opened: it will be written from new bytes.
    pub fn is_dirty(&self) -> bool {
        self.data.is_some()
    }
    /// Added after the package was opened.
    pub fn is_new(&self) -> bool {
        self.entry.is_none()
    }
    /// The original zip entry (`None` for a new part).
    pub fn entry(&self) -> Option<&ZipEntry> {
        self.entry.as_ref()
    }
    /// A directory entry of the zip (not a part; kept so the archive round-trips).
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
}

fn decompress(source: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, OpcError> {
    Ok(ZipArchive::default().data(source, entry)?)
}

/// An open package. See the [module documentation](self).
#[derive(Debug, Clone)]
pub struct Package {
    source: Vec<u8>,
    comment: String,
    parts: Vec<Part>,
    index: HashMap<String, usize>,
    content_types: ContentTypes,
    rels: Vec<RelSet>,
    core: CoreProperties,
    core_part: Option<String>,
    app: AppProperties,
    app_part: Option<String>,
    modified: bool,
    warnings: Vec<String>,
}

impl Default for Package {
    fn default() -> Self {
        Package::new()
    }
}

impl Package {
    /// An empty package: only a `[Content_Types].xml` with the `rels` and `xml` defaults.
    pub fn new() -> Package {
        let content_types = ContentTypes {
            defaults: vec![("rels".into(), CT_RELATIONSHIPS.into()), ("xml".into(), CT_XML.into())],
            overrides: Vec::new(),
        };
        let bytes = content_types.to_xml(None);
        let mut p = Package {
            source: Vec::new(),
            comment: String::new(),
            parts: vec![Part { name: CONTENT_TYPES_NAME.into(), entry: None, raw: 0..0, data: Some(bytes) }],
            index: HashMap::new(),
            content_types,
            rels: Vec::new(),
            core: CoreProperties::default(),
            core_part: None,
            app: AppProperties::default(),
            app_part: None,
            modified: true,
            warnings: Vec::new(),
        };
        p.reindex();
        p
    }

    /// Opens a package. Errors: not a zip, no `[Content_Types].xml`, or one that is not well-formed.
    /// A broken relationships, core or app part is not an error: it is kept untouched and reported in
    /// [`Package::warnings`].
    pub fn open(bytes: &[u8]) -> Result<Package, OpcError> {
        let archive = ZipArchive::read(bytes)?;
        let base = bytes.as_ptr() as usize;
        let mut parts = Vec::with_capacity(archive.entries.len());
        let mut warnings = Vec::new();
        for e in &archive.entries {
            let raw = archive.raw_data(bytes, e)?;
            let start = raw.as_ptr() as usize - base;
            let name = format!("/{}", e.name.replace('\\', "/").trim_start_matches('/'));
            parts.push(Part { name, entry: Some(e.clone()), raw: start..start + raw.len(), data: None });
        }
        let mut p = Package {
            source: bytes.to_vec(),
            comment: archive.comment.clone(),
            parts,
            index: HashMap::new(),
            content_types: ContentTypes::default(),
            rels: Vec::new(),
            core: CoreProperties::default(),
            core_part: None,
            app: AppProperties::default(),
            app_part: None,
            modified: false,
            warnings: Vec::new(),
        };
        p.reindex();
        if p.index.len() < p.parts.len() {
            warnings.push("the archive holds names that differ only in case or encoding; the first wins".into());
        }

        let ct_idx = p.find(CONTENT_TYPES_NAME).ok_or_else(|| OpcError("missing [Content_Types].xml".into()))?;
        let ct_bytes = p.bytes_at(ct_idx)?;
        p.content_types = ContentTypes::parse(&ct_bytes)?;

        for i in 0..p.parts.len() {
            if p.parts[i].is_dir() {
                continue;
            }
            let Some(source) = rels_source(&p.parts[i].name) else { continue };
            let parsed = p.bytes_at(i).and_then(|b| parse_rels(&b));
            match parsed {
                Ok(list) => p.rels.push(RelSet { source, list, broken: false }),
                Err(e) => {
                    warnings.push(format!("{}: unreadable relationships ({}), kept as it is", p.parts[i].name, e.0));
                    p.rels.push(RelSet { source, list: Vec::new(), broken: true });
                }
            }
        }

        p.core_part = p.prop_part(REL_CORE_PROPERTIES, "/docProps/core.xml");
        if let Some(name) = p.core_part.clone() {
            match p.part_bytes(&name).and_then(|b| parse_core(&b)) {
                Ok(c) => p.core = c,
                Err(e) => warnings.push(format!("{name}: unreadable core properties ({})", e.0)),
            }
        }
        p.app_part = p.prop_part(REL_EXTENDED_PROPERTIES, "/docProps/app.xml");
        if let Some(name) = p.app_part.clone() {
            match p.part_bytes(&name).and_then(|b| parse_app(&b)) {
                Ok(a) => p.app = a,
                Err(e) => warnings.push(format!("{name}: unreadable app properties ({})", e.0)),
            }
        }
        p.warnings = warnings;
        Ok(p)
    }

    fn prop_part(&self, rel_type: &str, fallback: &str) -> Option<String> {
        self.related("/", rel_type)
            .into_iter()
            .find(|n| self.find(n).is_some())
            .or_else(|| self.find(fallback).map(|i| self.parts[i].name.clone()))
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (i, p) in self.parts.iter().enumerate() {
            self.index.entry(name_key(&p.name)).or_insert(i);
        }
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.index.get(&name_key(name)).copied()
    }

    fn bytes_at(&self, i: usize) -> Result<Vec<u8>, OpcError> {
        let p = &self.parts[i];
        match (&p.data, &p.entry) {
            (Some(d), _) => Ok(d.clone()),
            (None, Some(e)) => decompress(&self.source, e),
            (None, None) => Ok(Vec::new()),
        }
    }

    fn rel_set(&self, source: &str) -> Option<usize> {
        let k = name_key(source);
        self.rels.iter().position(|s| name_key(&s.source) == k)
    }

    // --- reading -----------------------------------------------------------------------------------

    /// Problems found while opening that did not stop it (broken relationships, unreadable properties).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Anything changed since opening (a save will rebuild the archive).
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// The content types as they stand.
    pub fn content_types(&self) -> &ContentTypes {
        &self.content_types
    }

    /// Every part in zip order (no directory entries, no `[Content_Types].xml`).
    pub fn parts(&self) -> impl Iterator<Item = &Part> {
        self.parts.iter().filter(|p| !p.is_dir() && !same_part(&p.name, CONTENT_TYPES_NAME))
    }

    /// The names of [`Package::parts`].
    pub fn part_names(&self) -> Vec<String> {
        self.parts().map(|p| p.name.clone()).collect()
    }

    /// Every zip entry in order, including `[Content_Types].xml` and directory entries.
    pub fn entries(&self) -> &[Part] {
        &self.parts
    }

    /// A part by name (case-insensitive, percent-decoding).
    pub fn part(&self, name: &str) -> Option<&Part> {
        self.find(name).map(|i| &self.parts[i])
    }

    /// The part's uncompressed bytes (its new bytes when dirty).
    pub fn part_bytes(&self, name: &str) -> Result<Vec<u8>, OpcError> {
        match self.find(name) {
            Some(i) => self.bytes_at(i),
            None => err(format!("no part {name}")),
        }
    }

    /// The part's compressed bytes exactly as in the opened archive (`None` for new or dirty parts).
    pub fn raw_part(&self, name: &str) -> Option<&[u8]> {
        let p = self.part(name)?;
        if p.data.is_some() || p.entry.is_none() {
            return None;
        }
        self.source.get(p.raw.clone())
    }

    /// The content type of a part (Override, else Default by extension).
    pub fn content_type(&self, name: &str) -> Option<&str> {
        self.content_types.lookup(&normalize_name(name))
    }

    /// The names of the parts whose content type is `ct` (compared case-insensitively).
    pub fn parts_of_type(&self, ct: &str) -> Vec<String> {
        self.parts()
            .filter(|p| self.content_types.lookup(&p.name).is_some_and(|c| c.eq_ignore_ascii_case(ct)))
            .map(|p| p.name.clone())
            .collect()
    }

    /// The relationships of `source` (`/` or `""` for the package's own).
    pub fn rels(&self, source: &str) -> &[Relationship] {
        match self.rel_set(source) {
            Some(i) => &self.rels[i].list,
            None => &[],
        }
    }

    /// Resolves a relationship of `source` to a part name (the existing part's spelling when there is
    /// one), or the target untouched for an external one.
    pub fn resolve(&self, source: &str, rel: &Relationship) -> String {
        if rel.target_mode_external {
            return rel.target.clone();
        }
        let name = resolve_target(source, &rel.target);
        match self.find(&name) {
            Some(i) => self.parts[i].name.clone(),
            None => name,
        }
    }

    /// The targets of `source`'s relationships of `rel_type` (Transitional and Strict spellings
    /// match), resolved to part names; external targets are returned as written.
    pub fn related(&self, source: &str, rel_type: &str) -> Vec<String> {
        self.rels(source)
            .iter()
            .filter(|r| rel_type_matches(&r.rel_type, rel_type))
            .map(|r| self.resolve(source, r))
            .collect()
    }

    /// The relationship of `source` with this id.
    pub fn rel_by_id(&self, source: &str, id: &str) -> Option<&Relationship> {
        self.rels(source).iter().find(|r| r.id == id)
    }

    /// The main part: the target of the package's officeDocument relationship.
    pub fn main_part(&self) -> Option<String> {
        self.related("/", REL_OFFICE_DOCUMENT).into_iter().next()
    }

    /// The core properties (all `None` when the package has none).
    pub fn core(&self) -> &CoreProperties {
        &self.core
    }

    /// The core properties part's name, if there is one.
    pub fn core_part(&self) -> Option<&str> {
        self.core_part.as_deref()
    }

    /// The app properties (all `None` when the package has none).
    pub fn app(&self) -> &AppProperties {
        &self.app
    }

    // --- writing -----------------------------------------------------------------------------------

    /// Replaces a part's bytes; the part becomes dirty. Writing `[Content_Types].xml` or a
    /// relationships part re-reads it (malformed bytes are refused and nothing changes).
    pub fn set_part_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), OpcError> {
        let i = self.find(name).ok_or_else(|| OpcError(format!("no part {name}")))?;
        if self.parts[i].is_dir() {
            return err(format!("{name} is a directory entry"));
        }
        let pname = self.parts[i].name.clone();
        if same_part(&pname, CONTENT_TYPES_NAME) {
            self.content_types = ContentTypes::parse(&bytes)?;
        } else if let Some(source) = rels_source(&pname) {
            let list = parse_rels(&bytes)?;
            match self.rel_set(&source) {
                Some(s) => self.rels[s] = RelSet { source, list, broken: false },
                None => self.rels.push(RelSet { source, list, broken: false }),
            }
        }
        if self.core_part.as_deref().is_some_and(|c| same_part(c, &pname)) {
            self.core = parse_core(&bytes).unwrap_or_default();
        }
        if self.app_part.as_deref().is_some_and(|c| same_part(c, &pname)) {
            self.app = parse_app(&bytes).unwrap_or_default();
        }
        self.parts[i].data = Some(bytes);
        self.modified = true;
        Ok(())
    }

    /// Replaces a part with a whole DOM (written by [`xml::write_dom`]: everything is rewritten).
    /// Prefer [`Package::edit_xml`] to keep unknown content byte-exact.
    pub fn set_xml(&mut self, name: &str, root: &Element) -> Result<(), OpcError> {
        self.set_part_bytes(name, xml::write_dom(root))
    }

    /// Edits a part's XML with [`patch_xml`] semantics. Returns whether anything changed (an edit that
    /// changes nothing leaves the part clean).
    pub fn edit_xml(&mut self, name: &str, edits: impl FnOnce(&mut Element)) -> Result<bool, OpcError> {
        let before = self.part_bytes(name)?;
        let after = try_patch_xml(&before, edits)?;
        if after == before {
            return Ok(false);
        }
        self.set_part_bytes(name, after)?;
        Ok(true)
    }

    /// Sets the content type of a part: an Override is added, changed or removed (when the Default of
    /// the extension already says so) as needed.
    pub fn set_content_type(&mut self, name: &str, ct: &str) {
        let name = normalize_name(name);
        if self.content_types.lookup(&name).is_some_and(|c| c == ct) {
            return;
        }
        let default_ok = extension_of(&name).and_then(|e| self.content_types.default_for(e)) == Some(ct);
        if default_ok {
            self.content_types.remove_override(&name);
        } else {
            self.content_types.set_override(&name, ct);
        }
        self.flush_content_types();
    }

    /// Sets (or adds) a Default content type for an extension.
    pub fn set_default_content_type(&mut self, ext: &str, ct: &str) {
        if self.content_types.default_for(ext) == Some(ct) {
            return;
        }
        match self.content_types.defaults.iter_mut().find(|(x, _)| x.eq_ignore_ascii_case(ext)) {
            Some(slot) => slot.1 = ct.to_string(),
            None => self.content_types.defaults.push((ext.to_string(), ct.to_string())),
        }
        self.flush_content_types();
    }

    fn flush_content_types(&mut self) {
        let i = match self.find(CONTENT_TYPES_NAME) {
            Some(i) => i,
            None => {
                self.parts.insert(0, Part { name: CONTENT_TYPES_NAME.into(), entry: None, raw: 0..0, data: None });
                self.reindex();
                0
            }
        };
        let base = self.bytes_at(i).ok().filter(|b| !b.is_empty());
        let bytes = self.content_types.to_xml(base.as_deref());
        self.parts[i].data = Some(bytes);
        self.modified = true;
    }

    fn flush_rels(&mut self, set: usize) {
        let source = self.rels[set].source.clone();
        let rels_name = rels_part_name(&source);
        let i = match self.find(&rels_name) {
            Some(i) => i,
            None => {
                self.parts.push(Part { name: rels_name.clone(), entry: None, raw: 0..0, data: None });
                self.reindex();
                self.parts.len() - 1
            }
        };
        let base = if self.rels[set].broken { None } else { self.bytes_at(i).ok().filter(|b| !b.is_empty()) };
        let bytes = rels_to_xml(base.as_deref(), &self.rels[set].list);
        self.rels[set].broken = false;
        self.parts[i].data = Some(bytes);
        self.modified = true;
        if self.content_types.lookup(&rels_name) != Some(CT_RELATIONSHIPS) {
            if self.content_types.default_for("rels").is_none() {
                self.set_default_content_type("rels", CT_RELATIONSHIPS);
            } else {
                self.set_content_type(&rels_name, CT_RELATIONSHIPS);
            }
        }
    }

    /// Adds a new part with its content type. Errors when the name is invalid or already taken.
    /// A relationships part is read as such.
    pub fn add_part(&mut self, name: &str, content_type: &str, bytes: Vec<u8>) -> Result<(), OpcError> {
        let name = normalize_name(name);
        valid_new_name(&name)?;
        if self.find(&name).is_some() {
            return err(format!("part {name} already exists; use set_part_bytes"));
        }
        if let Some(source) = rels_source(&name) {
            let list = parse_rels(&bytes)?;
            match self.rel_set(&source) {
                Some(s) => self.rels[s] = RelSet { source, list, broken: false },
                None => self.rels.push(RelSet { source, list, broken: false }),
            }
        }
        self.parts.push(Part { name: name.clone(), entry: None, raw: 0..0, data: Some(bytes) });
        self.reindex();
        self.modified = true;
        self.set_content_type(&name, content_type);
        Ok(())
    }

    /// Removes a part, its Override, its own relationships part, and every relationship (anywhere in
    /// the package) that points at it. Returns false when there is no such part.
    pub fn remove_part(&mut self, name: &str) -> bool {
        let Some(i) = self.find(name) else { return false };
        let pname = self.parts[i].name.clone();
        if same_part(&pname, CONTENT_TYPES_NAME) || self.parts[i].is_dir() {
            return false;
        }
        self.parts.remove(i);
        self.reindex();
        self.modified = true;
        if let Some(source) = rels_source(&pname) {
            if let Some(s) = self.rel_set(&source) {
                self.rels.remove(s);
            }
        }
        // Its own relationships go with it.
        let own = rels_part_name(&pname);
        if self.find(&own).is_some() {
            self.remove_part(&own);
        }
        if let Some(s) = self.rel_set(&pname) {
            self.rels.remove(s);
        }
        if self.content_types.remove_override(&pname) {
            self.flush_content_types();
        }
        let key = name_key(&pname);
        for s in 0..self.rels.len() {
            let source = self.rels[s].source.clone();
            let before = self.rels[s].list.len();
            self.rels[s]
                .list
                .retain(|r| r.target_mode_external || name_key(&resolve_target(&source, &r.target)) != key);
            if self.rels[s].list.len() != before {
                self.flush_rels(s);
            }
        }
        if self.core_part.as_deref().is_some_and(|c| same_part(c, &pname)) {
            self.core_part = None;
            self.core = CoreProperties::default();
        }
        if self.app_part.as_deref().is_some_and(|c| same_part(c, &pname)) {
            self.app_part = None;
            self.app = AppProperties::default();
        }
        true
    }

    /// Adds a relationship from `source` (`/` for the package) and returns its new id (`rIdN`, one
    /// past the highest). An internal absolute target (`/word/media/a.png`) is written relative to the
    /// source; relative targets and URLs are written as given.
    pub fn add_relationship(&mut self, source: &str, rel_type: &str, target: &str, external: bool) -> String {
        let source = if source.is_empty() { "/".to_string() } else { normalize_name(source) };
        let s = match self.rel_set(&source) {
            Some(s) => s,
            None => {
                self.rels.push(RelSet { source: source.clone(), list: Vec::new(), broken: false });
                self.rels.len() - 1
            }
        };
        let list = &self.rels[s].list;
        let mut n = list
            .iter()
            .filter_map(|r| r.id.strip_prefix("rId").and_then(|d| d.parse::<u64>().ok()))
            .max()
            .unwrap_or(0)
            + 1;
        while list.iter().any(|r| r.id == format!("rId{n}")) {
            n += 1;
        }
        let id = format!("rId{n}");
        let target = if !external && target.starts_with('/') { relative_target(&source, target) } else { target.to_string() };
        self.rels[s].list.push(Relationship {
            id: id.clone(),
            rel_type: rel_type.to_string(),
            target,
            target_mode_external: external,
        });
        self.flush_rels(s);
        id
    }

    /// Removes the relationship `id` of `source`. Returns false when there is none.
    pub fn remove_relationship(&mut self, source: &str, id: &str) -> bool {
        let Some(s) = self.rel_set(source) else { return false };
        let before = self.rels[s].list.len();
        self.rels[s].list.retain(|r| r.id != id);
        if self.rels[s].list.len() == before {
            return false;
        }
        self.flush_rels(s);
        true
    }

    /// Writes the core properties. An existing `core.xml` is patched (unknown elements stay); without
    /// one, `/docProps/core.xml` is created with its content type and package relationship.
    pub fn set_core(&mut self, core: CoreProperties) -> Result<(), OpcError> {
        if core == self.core && self.core_part.is_some() {
            return Ok(());
        }
        let fields = core_fields(&core);
        match self.core_part.clone() {
            Some(name) => {
                let base = self.part_bytes(&name)?;
                let base = if xml::parse_dom(&base).is_ok() { base } else { core_skeleton().into_bytes() };
                let bytes = patch_xml(&base, |r| apply_props(r, &fields));
                if bytes != self.part_bytes(&name)? {
                    self.set_part_bytes(&name, bytes)?;
                }
            }
            None => {
                let name = "/docProps/core.xml";
                let bytes = patch_xml(core_skeleton().as_bytes(), |r| apply_props(r, &fields));
                if self.find(name).is_some() {
                    self.set_part_bytes(name, bytes)?;
                    self.set_content_type(name, CT_CORE_PROPERTIES);
                } else {
                    self.add_part(name, CT_CORE_PROPERTIES, bytes)?;
                }
                if self.related("/", REL_CORE_PROPERTIES).is_empty() {
                    self.add_relationship("/", REL_CORE_PROPERTIES, "docProps/core.xml", false);
                }
                self.core_part = Some(name.to_string());
            }
        }
        self.core = core;
        Ok(())
    }

    /// Writes the app properties' Application and AppVersion (the rest of `app.xml` is untouched).
    /// Without an app part, `/docProps/app.xml` is created with its content type and relationship.
    pub fn set_app(&mut self, app: AppProperties) -> Result<(), OpcError> {
        if app == self.app && self.app_part.is_some() {
            return Ok(());
        }
        let fields = app_fields(&app);
        match self.app_part.clone() {
            Some(name) => {
                let base = self.part_bytes(&name)?;
                let base = if xml::parse_dom(&base).is_ok() { base } else { app_skeleton().into_bytes() };
                let bytes = patch_xml(&base, |r| apply_props(r, &fields));
                if bytes != self.part_bytes(&name)? {
                    self.set_part_bytes(&name, bytes)?;
                }
            }
            None => {
                let name = "/docProps/app.xml";
                let bytes = patch_xml(app_skeleton().as_bytes(), |r| apply_props(r, &fields));
                if self.find(name).is_some() {
                    self.set_part_bytes(name, bytes)?;
                    self.set_content_type(name, CT_EXTENDED_PROPERTIES);
                } else {
                    self.add_part(name, CT_EXTENDED_PROPERTIES, bytes)?;
                }
                if self.related("/", REL_EXTENDED_PROPERTIES).is_empty() {
                    self.add_relationship("/", REL_EXTENDED_PROPERTIES, "docProps/app.xml", false);
                }
                self.app_part = Some(name.to_string());
            }
        }
        self.app = app;
        Ok(())
    }

    // --- saving ------------------------------------------------------------------------------------

    /// The package as zip bytes. Unmodified: the opened bytes exactly. Modified: untouched entries are
    /// copied raw (same compressed bytes, order, times, extras), dirty and new parts deflated.
    /// Should rebuilding ever fail (an entry whose extra field cannot take a ZIP64 record), the opened
    /// bytes are returned; use [`Package::try_save`] to see the error.
    pub fn save(&self) -> Vec<u8> {
        self.try_save().unwrap_or_else(|_| self.source.clone())
    }

    /// [`Package::save`] that reports a failure.
    pub fn try_save(&self) -> Result<Vec<u8>, OpcError> {
        if !self.modified && !self.source.is_empty() {
            return Ok(self.source.clone());
        }
        self.rebuild()
    }

    /// Always writes a fresh archive, even when nothing changed.
    pub fn rebuild(&self) -> Result<Vec<u8>, OpcError> {
        let mut w = ZipWriter::new();
        let default_time = w.mtime_dos;
        if !self.comment.is_empty() {
            w.set_comment(&self.comment);
        }
        for p in &self.parts {
            match (&p.entry, &p.data) {
                (Some(e), None) => {
                    let raw = &self.source[p.raw.clone()];
                    if w.add_raw(e, raw).is_err() {
                        // Only an oversized extra field or comment can refuse; drop those, keep the data.
                        let mut e2 = e.clone();
                        e2.extra.clear();
                        e2.comment.clear();
                        w.add_raw(&e2, raw)?;
                    }
                }
                (Some(e), Some(d)) => {
                    w.mtime_dos = e.mtime_dos;
                    w.add(&e.name, d, Method::Deflated(6))?;
                }
                (None, Some(d)) => {
                    w.mtime_dos = default_time;
                    w.add(&p.name[1..], d, Method::Deflated(6))?;
                }
                (None, None) => {}
            }
        }
        Ok(w.finish())
    }
}

// ---------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const CT: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
        "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
        "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
        "<Default Extension=\"PNG\" ContentType=\"image/png\"/>",
        "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
        "<Override PartName=\"/WORD/Styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>",
        "<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
        "<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>",
        "<Override PartName=\"/customXml/item1.xml\" ContentType=\"application/x-kiyo-unknown+xml\"/>",
        "<!-- a comment nobody asked for -->",
        "</Types>"
    );

    const ROOT_RELS: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/>",
        "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>",
        "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>",
        "</Relationships>"
    );

    const DOC_RELS: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
        "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image%201.png\"/>",
        "<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://example.com/a?b=1#frag\" TargetMode=\"External\"/>",
        "<Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml\" Target=\"../customXml/item1.xml\"/>",
        "<Relationship Id=\"rId7\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/x-bookmark\" Target=\"styles.xml#Heading1\" Kiyo=\"keep-me\"/>",
        "</Relationships>"
    );

    const DOC: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
        "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
        "xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" ",
        "xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" mc:Ignorable=\"w14\">\r\n",
        "  <w:body>\n",
        "    <w:p w14:paraId='1A2B3C4D'  w:rsidR=\"00AB\"><w:r><w:t xml:space=\"preserve\">Hello &amp; &#x77;orld </w:t></w:r></w:p>\n",
        "    <mc:AlternateContent><mc:Choice Requires=\"w14\"><w14:shiny   level = '3' /></mc:Choice><mc:Fallback><w:p/></mc:Fallback></mc:AlternateContent>\n",
        "    <w14:unknownThing a='1' b=\"2\"><!-- keep --><![CDATA[x<y]]></w14:unknownThing>\n",
        "    <w:sectPr/>\n",
        "  </w:body>\n",
        "</w:document>\r\n"
    );

    const STYLES: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
        "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
        "<w:style w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>",
        "<w:latentStyles   w:count='376'/>",
        "</w:styles>"
    );

    const CORE: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
        "<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" ",
        "xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" ",
        "xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
        "<dc:title>The Castle</dc:title><dc:creator>Kiyo</dc:creator><cp:lastModifiedBy>Alice</cp:lastModifiedBy>",
        "<cp:revision>3</cp:revision>",
        "<dcterms:created xsi:type=\"dcterms:W3CDTF\">2026-11-01T08:00:00Z</dcterms:created>",
        "<dcterms:modified xsi:type=\"dcterms:W3CDTF\">2026-11-02T09:30:00Z</dcterms:modified>",
        "<cp:contentStatus>Draft</cp:contentStatus>",
        "</cp:coreProperties>"
    );

    const APP: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
        "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" ",
        "xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">",
        "<Template>Normal.dotm</Template><TotalTime>4</TotalTime><Application>Scriptorium</Application>",
        "<DocSecurity>0</DocSecurity><AppVersion>0.1.0</AppVersion></Properties>"
    );

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 1, 2, 3];
    const UNKNOWN_BIN: &[u8] = &[0, 1, 2, 3, 250, 251, 252, 253, 254, 255, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7];

    fn build(entries: &[(&str, &[u8], Method)], comment: &str) -> Vec<u8> {
        let mut w = ZipWriter::new();
        w.mtime_dos = (0x6000, 0x5B7A);
        if !comment.is_empty() {
            w.set_comment(comment);
        }
        for (name, data, m) in entries {
            w.add(name, data, *m).expect("add");
        }
        w.finish()
    }

    fn sample() -> Vec<u8> {
        let d = Method::Deflated(6);
        build(
            &[
                ("[Content_Types].xml", CT.as_bytes(), d),
                ("_rels/.rels", ROOT_RELS.as_bytes(), d),
                ("word/", b"", Method::Stored),
                ("word/document.xml", DOC.as_bytes(), d),
                ("word/_rels/document.xml.rels", DOC_RELS.as_bytes(), d),
                ("word/styles.xml", STYLES.as_bytes(), Method::Stored),
                ("word/media/image%201.png", PNG, Method::Stored),
                ("word/unknown.bin", UNKNOWN_BIN, d),
                ("customXml/item1.xml", b"<kiyo:data xmlns:kiyo=\"urn:kiyo\"><kiyo:x/></kiyo:data>", d),
                ("docProps/core.xml", CORE.as_bytes(), d),
                ("docProps/app.xml", APP.as_bytes(), d),
            ],
            "made by the test",
        )
    }

    fn names_in_zip(bytes: &[u8]) -> Vec<String> {
        ZipArchive::read(bytes).unwrap().entries.iter().map(|e| e.name.clone()).collect()
    }

    fn raw_of(bytes: &[u8], name: &str) -> Vec<u8> {
        let a = ZipArchive::read(bytes).unwrap();
        let e = a.by_name(name).unwrap();
        a.raw_data(bytes, e).unwrap().to_vec()
    }

    fn s(b: &[u8]) -> String {
        String::from_utf8(b.to_vec()).unwrap()
    }

    // --- round trips ------------------------------------------------------------------------------

    #[test]
    fn open_then_save_is_byte_identical() {
        let input = sample();
        let p = Package::open(&input).unwrap();
        assert!(p.warnings().is_empty(), "{:?}", p.warnings());
        assert!(!p.is_modified());
        assert_eq!(p.save(), input);
    }

    #[test]
    fn rebuild_without_edits_is_byte_identical_too() {
        // Not the fast path: every entry goes through add_raw.
        let input = sample();
        let p = Package::open(&input).unwrap();
        assert_eq!(p.rebuild().unwrap(), input);
    }

    #[test]
    fn edit_one_part_keeps_every_other_part_raw_and_in_order() {
        let input = sample();
        let mut p = Package::open(&input).unwrap();
        let new_doc = DOC.replace("Hello", "Goodbye");
        p.set_part_bytes("/word/document.xml", new_doc.clone().into_bytes()).unwrap();
        assert!(p.part("/word/document.xml").unwrap().is_dirty());
        assert!(!p.part("/word/styles.xml").unwrap().is_dirty());
        let out = p.save();
        assert_ne!(out, input);
        assert_eq!(names_in_zip(&out), names_in_zip(&input));
        for name in names_in_zip(&input) {
            if name == "word/document.xml" {
                continue;
            }
            assert_eq!(raw_of(&out, &name), raw_of(&input, &name), "{name} changed");
        }
        let a_in = ZipArchive::read(&input).unwrap();
        let a_out = ZipArchive::read(&out).unwrap();
        for (x, y) in a_in.entries.iter().zip(&a_out.entries) {
            if x.name != "word/document.xml" {
                assert_eq!((x.crc32, x.mtime_dos, x.method, &x.extra), (y.crc32, y.mtime_dos, y.method, &y.extra));
            } else {
                assert_eq!(x.mtime_dos, y.mtime_dos, "an edited part keeps its timestamp");
            }
        }
        assert_eq!(a_out.comment, "made by the test");
        let q = Package::open(&out).unwrap();
        assert_eq!(q.part_bytes("/word/document.xml").unwrap(), new_doc.into_bytes());
        assert_eq!(q.part_bytes("/word/unknown.bin").unwrap(), UNKNOWN_BIN);
    }

    // --- reading -----------------------------------------------------------------------------------

    #[test]
    fn content_types_are_case_insensitive() {
        let p = Package::open(&sample()).unwrap();
        assert_eq!(p.content_type("/word/document.xml"), Some(CT_WORDPROCESSINGML_MAIN));
        assert_eq!(p.content_type("word/styles.xml"), Some(CT_STYLES), "Override /WORD/Styles.xml");
        assert_eq!(p.content_type("/word/media/image%201.png"), Some(CT_PNG), "Default PNG");
        assert_eq!(p.content_type("/x/y.Xml"), Some(CT_XML));
        assert_eq!(p.content_type("/x/y.nothing"), None);
        assert_eq!(p.parts_of_type(CT_PNG), vec!["/word/media/image%201.png".to_string()]);
        assert_eq!(p.parts_of_type("application/x-kiyo-unknown+xml"), vec!["/customXml/item1.xml".to_string()]);
    }

    #[test]
    fn part_lookup_is_case_insensitive_and_percent_decoding() {
        let p = Package::open(&sample()).unwrap();
        assert!(p.part("/WORD/DOCUMENT.XML").is_some());
        assert!(p.part("word/document.xml").is_some());
        assert_eq!(p.part("/word/media/image 1.png").unwrap().name(), "/word/media/image%201.png");
        assert_eq!(p.part_bytes("/Word/Media/Image%201.PNG").unwrap(), PNG);
        assert!(p.part("/word/nope.xml").is_none());
        assert!(p.part_bytes("/word/nope.xml").is_err());
        assert!(p.raw_part("/word/document.xml").is_some());
        let names = p.part_names();
        assert!(!names.iter().any(|n| n.ends_with('/') || n.contains("Content_Types")));
        assert_eq!(names.len(), 9);
    }

    #[test]
    fn relationships_and_main_part() {
        let p = Package::open(&sample()).unwrap();
        assert_eq!(p.main_part().as_deref(), Some("/word/document.xml"));
        let doc = "/word/document.xml";
        assert_eq!(p.rels(doc).len(), 5);
        assert_eq!(p.rels("/").len(), 3);
        assert_eq!(p.rels(""), p.rels("/"));
        assert_eq!(p.related(doc, REL_STYLES), vec!["/word/styles.xml".to_string()]);
        assert_eq!(p.related(doc, REL_IMAGE), vec!["/word/media/image%201.png".to_string()]);
        assert_eq!(p.related(doc, REL_HYPERLINK), vec!["https://example.com/a?b=1#frag".to_string()]);
        let h = p.rel_by_id(doc, "rId3").unwrap();
        assert!(h.target_mode_external);
        assert_eq!(
            p.related(doc, "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml"),
            vec!["/customXml/item1.xml".to_string()]
        );
        // A fragment target resolves to the part.
        assert_eq!(
            p.related(doc, "http://schemas.openxmlformats.org/officeDocument/2006/relationships/x-bookmark"),
            vec!["/word/styles.xml".to_string()]
        );
        // Strict spelling of the same type matches.
        assert_eq!(p.related(doc, "http://purl.oclc.org/ooxml/officeDocument/relationships/styles").len(), 1);
        assert!(p.rels("/word/styles.xml").is_empty());
    }

    #[test]
    fn core_and_app_properties_are_read() {
        let p = Package::open(&sample()).unwrap();
        let c = p.core();
        assert_eq!(c.title.as_deref(), Some("The Castle"));
        assert_eq!(c.creator.as_deref(), Some("Kiyo"));
        assert_eq!(c.last_modified_by.as_deref(), Some("Alice"));
        assert_eq!(c.revision.as_deref(), Some("3"));
        assert_eq!(c.created.as_deref(), Some("2026-11-01T08:00:00Z"));
        assert_eq!(c.modified.as_deref(), Some("2026-11-02T09:30:00Z"));
        assert_eq!(c.subject, None);
        assert_eq!(c.category, None);
        assert_eq!(p.app().application.as_deref(), Some("Scriptorium"));
        assert_eq!(p.app().app_version.as_deref(), Some("0.1.0"));
        assert_eq!(p.core_part(), Some("/docProps/core.xml"));
    }

    // --- resolution ------------------------------------------------------------------------------

    #[test]
    fn resolve_target_cases() {
        assert_eq!(resolve_target("/word/document.xml", "styles.xml"), "/word/styles.xml");
        assert_eq!(resolve_target("/word/document.xml", "media/image1.png"), "/word/media/image1.png");
        assert_eq!(resolve_target("/word/document.xml", "../customXml/item1.xml"), "/customXml/item1.xml");
        assert_eq!(resolve_target("/word/document.xml", "./theme/../theme/theme1.xml"), "/word/theme/theme1.xml");
        assert_eq!(resolve_target("/word/document.xml", "../../../../x.xml"), "/x.xml", "never above the root");
        assert_eq!(resolve_target("/word/document.xml", "/xl/workbook.xml"), "/xl/workbook.xml");
        assert_eq!(resolve_target("/", "word/document.xml"), "/word/document.xml");
        assert_eq!(resolve_target("", "word/document.xml"), "/word/document.xml");
        assert_eq!(resolve_target("/ppt/slides/slide1.xml", "../slideLayouts/slideLayout2.xml"), "/ppt/slideLayouts/slideLayout2.xml");
        assert_eq!(resolve_target("/word/document.xml", "media\\a.png"), "/word/media/a.png");
        assert_eq!(resolve_target("/word/document.xml", "styles.xml#Heading1"), "/word/styles.xml");
        assert_eq!(resolve_target("/word/document.xml", "#_Toc123"), "/word/document.xml");
        assert_eq!(resolve_target("/word/document.xml", "https://example.com/x#y"), "https://example.com/x#y");
        assert_eq!(resolve_target("/word/document.xml", "mailto:kiyo@example.com"), "mailto:kiyo@example.com");
        assert_eq!(resolve_target("/word/document.xml", "file:///C:/a.docx"), "file:///C:/a.docx");
        assert_eq!(resolve_target("/word/document.xml", "//server/share/a.xlsx"), "//server/share/a.xlsx");
        assert_eq!(resolve_target("/word/document.xml", "media/image%201.png"), "/word/media/image%201.png");
    }

    #[test]
    fn relative_target_and_rels_names() {
        assert_eq!(relative_target("/word/document.xml", "/word/media/a.png"), "media/a.png");
        assert_eq!(relative_target("/word/document.xml", "/customXml/item1.xml"), "../customXml/item1.xml");
        assert_eq!(relative_target("/", "/word/document.xml"), "word/document.xml");
        assert_eq!(relative_target("/ppt/slides/slide1.xml", "/ppt/slideLayouts/l.xml"), "../slideLayouts/l.xml");
        for (src, part) in [("/word/document.xml", "/word/a/b/c.xml"), ("/a/b/c/d.xml", "/x.xml"), ("/", "/a.xml")] {
            assert_eq!(resolve_target(src, &relative_target(src, part)), part);
        }
        assert_eq!(rels_part_name("/"), "/_rels/.rels");
        assert_eq!(rels_part_name("/word/document.xml"), "/word/_rels/document.xml.rels");
        assert_eq!(rels_source("/_rels/.rels").as_deref(), Some("/"));
        assert_eq!(rels_source("word/_rels/document.xml.rels").as_deref(), Some("/word/document.xml"));
        assert_eq!(rels_source("/word/_RELS/document.xml.RELS").as_deref(), Some("/word/document.xml"));
        assert_eq!(rels_source("/word/document.xml"), None);
        assert_eq!(rels_source("/word/x.rels"), None);
        assert!(is_external_url("http://a"));
        assert!(!is_external_url("C:/a.xml"));
        assert!(!is_external_url("media/a:b.png"));
    }

    // --- adding and removing ---------------------------------------------------------------------

    #[test]
    fn add_part_and_relationship_make_valid_types_and_rels() {
        let input = sample();
        let mut p = Package::open(&input).unwrap();
        p.add_part("/word/media/pic2.png", CT_PNG, PNG.to_vec()).unwrap();
        p.add_part("word/header1.xml", CT_HEADER, b"<w:hdr xmlns:w=\"urn:w\"/>".to_vec()).unwrap();
        assert!(p.add_part("/word/header1.xml", CT_HEADER, Vec::new()).is_err(), "taken");
        assert!(p.add_part("/WORD/HEADER1.XML", CT_HEADER, Vec::new()).is_err(), "taken, any case");
        assert!(p.add_part("/word/../x.xml", CT_XML, Vec::new()).is_err());
        assert!(p.add_part("/word/", CT_XML, Vec::new()).is_err());
        assert!(p.add_part("/[content_types].xml", CT_XML, Vec::new()).is_err());
        let doc = "/word/document.xml";
        let img = p.add_relationship(doc, REL_IMAGE, "/word/media/pic2.png", false);
        assert_eq!(img, "rId8", "one past the highest (rId7)");
        let hdr = p.add_relationship(doc, REL_HEADER, "header1.xml", false);
        assert_eq!(hdr, "rId9");
        let link = p.add_relationship("/word/header1.xml", REL_HYPERLINK, "https://kiyo.example/", true);
        assert_eq!(link, "rId1");
        let out = p.save();

        let q = Package::open(&out).unwrap();
        assert!(q.warnings().is_empty(), "{:?}", q.warnings());
        assert_eq!(q.content_type("/word/media/pic2.png"), Some(CT_PNG));
        assert_eq!(q.content_type("/word/header1.xml"), Some(CT_HEADER));
        assert_eq!(q.content_type("/word/_rels/header1.xml.rels"), Some(CT_RELATIONSHIPS));
        // The PNG needed no Override (the Default covers it); the header did.
        assert!(q.content_types().override_for("/word/media/pic2.png").is_none());
        assert!(q.content_types().override_for("/word/header1.xml").is_some());
        assert_eq!(q.rel_by_id(doc, &img).unwrap().target, "media/pic2.png", "written relative");
        assert_eq!(q.related(doc, REL_IMAGE), vec!["/word/media/image%201.png".to_string(), "/word/media/pic2.png".to_string()]);
        assert_eq!(q.related(doc, REL_HEADER), vec!["/word/header1.xml".to_string()]);
        assert_eq!(q.related("/word/header1.xml", REL_HYPERLINK), vec!["https://kiyo.example/".to_string()]);
        assert!(q.rel_by_id("/word/header1.xml", "rId1").unwrap().target_mode_external);
        // Unknown attribute and the comment survived the rewrite of the rels and content types.
        let rels = s(&q.part_bytes("/word/_rels/document.xml.rels").unwrap());
        assert!(rels.contains("Kiyo=\"keep-me\""));
        assert!(rels.contains(&DOC_RELS[DOC_RELS.find("<Relationship Id=\"rId1\"").unwrap()..DOC_RELS.find("</Relationships>").unwrap()]));
        let ct = s(&raw_ct(&q));
        assert!(ct.contains("<!-- a comment nobody asked for -->"));
        assert!(ct.contains("<Override PartName=\"/WORD/Styles.xml\""));
        // Untouched parts are still raw-identical.
        for name in ["word/document.xml", "word/styles.xml", "word/media/image%201.png", "docProps/core.xml"] {
            assert_eq!(raw_of(&out, name), raw_of(&input, name));
        }
        // The package rels did not change, so they were copied raw.
        assert_eq!(raw_of(&out, "_rels/.rels"), raw_of(&input, "_rels/.rels"));
    }

    fn raw_ct(p: &Package) -> Vec<u8> {
        p.bytes_at(p.find(CONTENT_TYPES_NAME).unwrap()).unwrap()
    }

    #[test]
    fn remove_part_drops_its_rels_and_override() {
        let mut p = Package::open(&sample()).unwrap();
        let doc = "/word/document.xml";
        assert!(p.remove_part("/WORD/STYLES.XML"));
        assert!(!p.remove_part("/word/styles.xml"), "already gone");
        assert!(!p.remove_part(CONTENT_TYPES_NAME));
        assert!(p.related(doc, REL_STYLES).is_empty());
        assert!(p.rel_by_id(doc, "rId7").is_none(), "the #fragment rel to styles goes too");
        assert!(p.rel_by_id(doc, "rId2").is_some());
        assert!(p.content_types().override_for("/word/styles.xml").is_none());
        let out = p.save();
        let q = Package::open(&out).unwrap();
        assert!(q.part("/word/styles.xml").is_none());
        assert_eq!(q.rels(doc).len(), 3);
        assert!(!s(&q.part_bytes("/word/_rels/document.xml.rels").unwrap()).contains("styles.xml"));
        assert!(!s(&raw_ct(&q)).to_lowercase().contains("styles.xml"));

        // Removing a source removes its own relationships part.
        let mut p = Package::open(&out).unwrap();
        assert!(p.remove_part(doc));
        assert!(p.part("/word/_rels/document.xml.rels").is_none());
        assert!(p.rels(doc).is_empty());
        assert_eq!(p.main_part(), None, "the package rel to it was dropped");
        let q = Package::open(&p.save()).unwrap();
        assert_eq!(q.main_part(), None);
        assert!(q.part("/word/media/image%201.png").is_some(), "other parts stay");

        // Removing the core part forgets the properties.
        let mut p = Package::open(&sample()).unwrap();
        assert!(p.remove_part("/docProps/core.xml"));
        assert_eq!(p.core(), &CoreProperties::default());
        assert!(p.related("/", REL_CORE_PROPERTIES).is_empty());
    }

    #[test]
    fn remove_relationship_works() {
        let mut p = Package::open(&sample()).unwrap();
        assert!(p.remove_relationship("/word/document.xml", "rId3"));
        assert!(!p.remove_relationship("/word/document.xml", "rId3"));
        assert!(!p.remove_relationship("/nowhere.xml", "rId1"));
        let q = Package::open(&p.save()).unwrap();
        assert!(q.related("/word/document.xml", REL_HYPERLINK).is_empty());
    }

    // --- properties -------------------------------------------------------------------------------

    #[test]
    fn core_properties_write_and_reread() {
        let input = sample();
        let mut p = Package::open(&input).unwrap();
        let mut c = p.core().clone();
        c.title = Some("A New <Title> & more".into());
        c.subject = Some("Subjects".into());
        c.revision = Some("4".into());
        c.creator = None;
        c.category = Some("Report".into());
        p.set_core(c.clone()).unwrap();
        let out = p.save();
        let q = Package::open(&out).unwrap();
        assert_eq!(q.core(), &c);
        let xml_text = s(&q.part_bytes("/docProps/core.xml").unwrap());
        assert!(xml_text.contains("<cp:contentStatus>Draft</cp:contentStatus>"), "unknown property kept");
        assert!(xml_text.contains("<dcterms:created xsi:type=\"dcterms:W3CDTF\">2026-11-01T08:00:00Z</dcterms:created>"));
        assert!(!xml_text.contains("dc:creator"));
        assert!(xml_text.contains("<dc:subject>Subjects</dc:subject>"));
        // Setting the same values again changes nothing.
        let mut r = Package::open(&out).unwrap();
        r.set_core(c).unwrap();
        assert!(!r.is_modified());
        // Only core.xml changed.
        for name in names_in_zip(&input) {
            if name != "docProps/core.xml" {
                assert_eq!(raw_of(&out, &name), raw_of(&input, &name), "{name}");
            }
        }
    }

    #[test]
    fn missing_core_is_empty_and_set_core_creates_it() {
        let d = Method::Deflated(6);
        let ct = concat!(
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
            "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
            "<Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>"
        );
        let rels = concat!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"/xl/workbook.xml\"/>",
            "</Relationships>"
        );
        let bytes = build(
            &[("[Content_Types].xml", ct.as_bytes(), d), ("_rels/.rels", rels.as_bytes(), d), ("xl/workbook.xml", b"<workbook/>", d)],
            "",
        );
        let mut p = Package::open(&bytes).unwrap();
        assert_eq!(p.main_part().as_deref(), Some("/xl/workbook.xml"), "absolute target");
        assert_eq!(p.core(), &CoreProperties::default());
        assert_eq!(p.core_part(), None);
        assert_eq!(p.app(), &AppProperties::default());
        let c = CoreProperties { title: Some("Book".into()), created: Some("2026-11-26T00:00:00Z".into()), ..Default::default() };
        p.set_core(c.clone()).unwrap();
        p.set_app(AppProperties { application: Some("Scriptorium".into()), app_version: None }).unwrap();
        let q = Package::open(&p.save()).unwrap();
        assert!(q.warnings().is_empty(), "{:?}", q.warnings());
        assert_eq!(q.core(), &c);
        assert_eq!(q.core_part(), Some("/docProps/core.xml"));
        assert_eq!(q.content_type("/docProps/core.xml"), Some(CT_CORE_PROPERTIES));
        assert_eq!(q.related("/", REL_CORE_PROPERTIES), vec!["/docProps/core.xml".to_string()]);
        assert_eq!(q.app().application.as_deref(), Some("Scriptorium"));
        assert_eq!(q.content_type("/docProps/app.xml"), Some(CT_EXTENDED_PROPERTIES));
        let core_xml = s(&q.part_bytes("/docProps/core.xml").unwrap());
        assert!(core_xml.contains("<dcterms:created xsi:type=\"dcterms:W3CDTF\">2026-11-26T00:00:00Z</dcterms:created>"));
        assert!(xml::parse_dom(core_xml.as_bytes()).is_ok());
    }

    #[test]
    fn set_app_keeps_the_rest_raw() {
        let mut p = Package::open(&sample()).unwrap();
        p.set_app(AppProperties { application: Some("Scriptorium".into()), app_version: Some("0.2.0".into()) }).unwrap();
        let text = s(&p.part_bytes("/docProps/app.xml").unwrap());
        assert_eq!(text, APP.replace("0.1.0", "0.2.0"));
    }

    // --- nothing unknown is lost -------------------------------------------------------------------

    #[test]
    fn unknown_parts_and_elements_survive_an_edit_of_another_part() {
        let input = sample();
        let mut p = Package::open(&input).unwrap();
        let changed = p
            .edit_xml("/word/styles.xml", |root| {
                if let Some(Node::Element(st)) = root.children.first_mut() {
                    st.set_attr("w:styleId", "Body");
                }
            })
            .unwrap();
        assert!(changed);
        assert!(!p.edit_xml("/word/document.xml", |_| {}).unwrap(), "a no-op edit stays clean");
        assert!(!p.part("/word/document.xml").unwrap().is_dirty());
        let out = p.save();
        let q = Package::open(&out).unwrap();
        assert_eq!(q.part_bytes("/word/document.xml").unwrap(), DOC.as_bytes(), "mc: and w14: content intact");
        assert_eq!(q.part_bytes("/word/unknown.bin").unwrap(), UNKNOWN_BIN);
        assert_eq!(q.content_type("/customXml/item1.xml"), Some("application/x-kiyo-unknown+xml"));
        assert_eq!(q.part_bytes("/customXml/item1.xml").unwrap(), b"<kiyo:data xmlns:kiyo=\"urn:kiyo\"><kiyo:x/></kiyo:data>");
        let styles = s(&q.part_bytes("/word/styles.xml").unwrap());
        assert_eq!(styles, STYLES.replace("w:styleId=\"Normal\"", "w:styleId=\"Body\""));
        assert!(styles.contains("<w:latentStyles   w:count='376'/>"));
        // The content types did not change: copied raw, with its comment.
        assert_eq!(raw_of(&out, "[Content_Types].xml"), raw_of(&input, "[Content_Types].xml"));
        assert_eq!(raw_of(&out, "word/_rels/document.xml.rels"), raw_of(&input, "word/_rels/document.xml.rels"));
    }

    #[test]
    fn edit_inside_a_document_keeps_alternate_content_byte_exact() {
        let out = patch_xml(DOC.as_bytes(), |root| {
            let body = root.child_mut("body").unwrap();
            let p = body.child_mut("p").unwrap();
            let r = p.child_mut("r").unwrap();
            let t = r.child_mut("t").unwrap();
            t.children = vec![Node::Text("Hi & bye".into())];
        });
        let text = s(&out);
        let expected = DOC.replace("Hello &amp; &#x77;orld ", "Hi &amp; bye");
        assert_eq!(text, expected, "only the text node changed; the p's odd attribute quoting stays");
    }

    #[test]
    fn patch_xml_unchanged_is_identical_and_insertions_keep_neighbours() {
        assert_eq!(patch_xml(DOC.as_bytes(), |_| {}), DOC.as_bytes());
        let out = patch_xml(DOC.as_bytes(), |root| {
            let body = root.child_mut("body").unwrap();
            let mut np = Element::new("w:p");
            np.set_attr("w:rsidR", "00CD");
            body.children.insert(0, Node::Element(np));
            body.children.retain(|c| !matches!(c, Node::Element(e) if e.name == "w:sectPr"));
        });
        let text = s(&out);
        assert!(text.contains("<w:body><w:p w:rsidR=\"00CD\"/>\n    <w:p w14:paraId='1A2B3C4D'  w:rsidR=\"00AB\">"));
        assert!(text.contains("<w14:unknownThing a='1' b=\"2\"><!-- keep --><![CDATA[x<y]]></w14:unknownThing>"));
        assert!(text.contains("<w14:shiny   level = '3' />"));
        assert!(!text.contains("sectPr"));
        assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:document"));
        assert!(text.ends_with("</w:document>\r\n"));
        // Edited start tag of the root: attributes rewritten, children still verbatim.
        let out = patch_xml(DOC.as_bytes(), |root| root.set_attr("mc:Ignorable", "w14 w15"));
        let text = s(&out);
        assert!(text.contains("mc:Ignorable=\"w14 w15\">\r\n  <w:body>\n"));
        assert!(text.contains("<w:p w14:paraId='1A2B3C4D'  w:rsidR=\"00AB\">"));
        assert!(xml::parse_dom(&out).is_ok());
    }

    #[test]
    fn patch_xml_on_malformed_input_returns_it() {
        let bad = b"<a><b></a>";
        assert_eq!(patch_xml(bad, |r| r.set_attr("x", "1")), bad);
        assert!(try_patch_xml(bad, |_| {}).is_err());
        assert!(try_patch_xml(b"", |_| {}).is_err());
    }

    #[test]
    fn patch_xml_utf16_changed_becomes_utf8() {
        let src = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><a x=\"1\"><b/></a>";
        let mut utf16 = vec![0xFF, 0xFE];
        for u in src.encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(patch_xml(&utf16, |_| {}), utf16, "unchanged stays UTF-16");
        let out = patch_xml(&utf16, |r| r.set_attr("x", "2"));
        let root = xml::parse_dom(&out).unwrap();
        assert_eq!(root.attr("x"), Some("2"));
        assert!(s(&out).contains("encoding=\"UTF-8\""));
    }

    #[test]
    fn set_xml_replaces_a_part() {
        let mut p = Package::open(&sample()).unwrap();
        let mut root = Element::new("kiyo:data");
        root.set_attr("xmlns:kiyo", "urn:kiyo");
        p.set_xml("/customXml/item1.xml", &root).unwrap();
        let q = Package::open(&p.save()).unwrap();
        assert_eq!(xml::parse_dom(&q.part_bytes("/customXml/item1.xml").unwrap()).unwrap(), root);
    }

    #[test]
    fn set_part_bytes_on_rels_and_content_types_rereads_them() {
        let mut p = Package::open(&sample()).unwrap();
        assert!(p.set_part_bytes("/word/_rels/document.xml.rels", b"<broken".to_vec()).is_err());
        assert_eq!(p.rels("/word/document.xml").len(), 5, "refused, nothing changed");
        let one = DOC_RELS.replace(
            "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image%201.png\"/>",
            "",
        );
        p.set_part_bytes("/word/_rels/document.xml.rels", one.into_bytes()).unwrap();
        assert!(p.related("/word/document.xml", REL_IMAGE).is_empty());
        let ct = CT.replace("image/png", "image/x-png");
        p.set_part_bytes(CONTENT_TYPES_NAME, ct.into_bytes()).unwrap();
        assert_eq!(p.content_type("/word/media/image%201.png"), Some("image/x-png"));
        assert!(p.set_part_bytes("/word/", Vec::new()).is_err());
        assert!(p.set_part_bytes("/nope", Vec::new()).is_err());
    }

    #[test]
    fn new_package_from_scratch() {
        let mut p = Package::new();
        p.add_part("/word/document.xml", CT_WORDPROCESSINGML_MAIN, b"<w:document xmlns:w=\"urn:w\"/>".to_vec()).unwrap();
        p.add_relationship("/", REL_OFFICE_DOCUMENT, "/word/document.xml", false);
        p.set_core(CoreProperties { creator: Some("Alice".into()), ..Default::default() }).unwrap();
        let bytes = p.save();
        let names = names_in_zip(&bytes);
        assert_eq!(names[0], "[Content_Types].xml");
        let q = Package::open(&bytes).unwrap();
        assert!(q.warnings().is_empty(), "{:?}", q.warnings());
        assert_eq!(q.main_part().as_deref(), Some("/word/document.xml"));
        assert_eq!(q.rel_by_id("/", "rId1").unwrap().target, "word/document.xml");
        assert_eq!(q.core().creator.as_deref(), Some("Alice"));
        assert_eq!(q.content_type("/_rels/.rels"), Some(CT_RELATIONSHIPS));
        assert_eq!(q.save(), bytes);
    }

    // --- malformed packages -----------------------------------------------------------------------

    #[test]
    fn malformed_packages_give_errors_not_panics() {
        assert!(Package::open(b"").is_err());
        assert!(Package::open(b"this is not a zip file at all").is_err());
        let d = Method::Deflated(6);
        let no_ct = build(&[("word/document.xml", DOC.as_bytes(), d)], "");
        assert!(Package::open(&no_ct).unwrap_err().0.contains("Content_Types"));
        let bad_ct = build(&[("[Content_Types].xml", b"<Types><Default", d)], "");
        assert!(Package::open(&bad_ct).is_err());
        let wrong_root = build(&[("[Content_Types].xml", b"<Kinds/>", d)], "");
        assert!(Package::open(&wrong_root).is_err());
        // [Content_Types].xml found case-insensitively.
        let lower = build(&[("[content_types].XML", CT.as_bytes(), d), ("word/document.xml", DOC.as_bytes(), d)], "");
        let p = Package::open(&lower).unwrap();
        assert_eq!(p.content_type("/word/document.xml"), Some(CT_WORDPROCESSINGML_MAIN));
    }

    #[test]
    fn broken_rels_are_kept_and_reported() {
        let d = Method::Deflated(6);
        let bytes = build(
            &[
                ("[Content_Types].xml", CT.as_bytes(), d),
                ("_rels/.rels", ROOT_RELS.as_bytes(), d),
                ("word/document.xml", DOC.as_bytes(), d),
                ("word/_rels/document.xml.rels", b"<Relationships><Relationship Id=", d),
                ("docProps/core.xml", b"<cp:coreProperties", d),
            ],
            "",
        );
        let mut p = Package::open(&bytes).unwrap();
        assert_eq!(p.warnings().len(), 2, "{:?}", p.warnings());
        assert!(p.rels("/word/document.xml").is_empty());
        assert_eq!(p.core(), &CoreProperties::default());
        assert_eq!(p.save(), bytes, "kept as it was");
        // An edit elsewhere still copies the broken part raw.
        p.set_part_bytes("/word/document.xml", b"<w:document xmlns:w=\"urn:w\"/>".to_vec()).unwrap();
        let out = p.save();
        assert_eq!(raw_of(&out, "word/_rels/document.xml.rels"), raw_of(&bytes, "word/_rels/document.xml.rels"));
        // Adding a relationship replaces it with a fresh, valid part.
        let id = p.add_relationship("/word/document.xml", REL_STYLES, "styles.xml", false);
        assert_eq!(id, "rId1");
        let q = Package::open(&p.save()).unwrap();
        assert_eq!(q.rels("/word/document.xml").len(), 1);
        // A core.xml that cannot be read is replaced on set_core.
        let mut r = Package::open(&bytes).unwrap();
        r.set_core(CoreProperties { title: Some("T".into()), ..Default::default() }).unwrap();
        let q = Package::open(&r.save()).unwrap();
        assert_eq!(q.core().title.as_deref(), Some("T"));
    }

    #[test]
    fn truncated_and_corrupted_packages_never_panic() {
        let input = sample();
        for cut in (0..input.len()).step_by(13) {
            let _ = Package::open(&input[..cut]);
        }
        let mut x = 0x1234_5678u32;
        for _ in 0..300 {
            let mut bad = input.clone();
            for _ in 0..4 {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let i = x as usize % bad.len();
                bad[i] ^= (x >> 24) as u8 | 1;
            }
            if let Ok(p) = Package::open(&bad) {
                let _ = p.save();
                for n in p.part_names() {
                    let _ = p.part_bytes(&n);
                }
            }
        }
    }

    #[test]
    fn percent_and_names_helpers() {
        assert_eq!(percent_decode("image%201.png"), "image 1.png");
        assert_eq!(percent_decode("%E2%9C%93"), "\u{2713}");
        assert_eq!(percent_decode("bad%zzand%4"), "bad%zzand%4");
        assert_eq!(percent_decode("end%"), "end%");
        assert!(same_part("/Word/Media/Image%201.PNG", "word/media/image 1.png"));
        assert!(!same_part("/word/a.xml", "/word/b.xml"));
        assert!(rel_type_matches(REL_OFFICE_DOCUMENT, REL_OFFICE_DOCUMENT_STRICT));
        assert!(!rel_type_matches(REL_STYLES, REL_THEME));
    }
}
