//! PDF objects: the nine kinds (null, boolean, number, string, name, array, dictionary, stream and the
//! indirect reference), text strings in and out, and the writer that turns an object back into bytes
//! for an incremental update.

use std::fmt;
use std::sync::Arc;

/// One PDF object. Names hold their decoded bytes as Latin-1 characters (one char per byte), so any
/// name round-trips exactly.
#[derive(Debug, Clone, PartialEq)]
pub enum Obj {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(Vec<u8>),
    Name(String),
    Arr(Vec<Obj>),
    Dict(Dict),
    /// Shared, since font files and images can be large and objects get cloned.
    Stream(Arc<Stream>),
    /// Object number, generation.
    Ref(u32, u16),
}

/// A dictionary, keys in file order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dict(pub Vec<(String, Obj)>);

/// A stream: its dictionary and its raw (still encoded) bytes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stream {
    pub dict: Dict,
    pub data: Vec<u8>,
}

impl Dict {
    pub fn new() -> Dict {
        Dict(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&Obj> {
        self.0.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Set a key (replacing it when present).
    pub fn set(&mut self, key: &str, value: Obj) {
        if let Some(slot) = self.0.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
        } else {
            self.0.push((key.to_string(), value));
        }
    }

    pub fn with(mut self, key: &str, value: Obj) -> Dict {
        self.set(key, value);
        self
    }

    pub fn remove(&mut self, key: &str) -> Option<Obj> {
        let i = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(i).1)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// The name under `key`, when it is a name.
    pub fn name(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Obj::as_name)
    }

    pub fn is_type(&self, t: &str) -> bool {
        self.name("Type") == Some(t)
    }
}

impl Obj {
    pub fn name(s: &str) -> Obj {
        Obj::Name(s.to_string())
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Obj::Null)
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Obj::Int(i) => Some(*i),
            Obj::Real(r) if r.is_finite() => Some(*r as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Obj::Int(i) => Some(*i as f64),
            Obj::Real(r) if r.is_finite() => Some(*r),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Obj::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_name(&self) -> Option<&str> {
        match self {
            Obj::Name(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Obj::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Obj]> {
        match self {
            Obj::Arr(a) => Some(a),
            _ => None,
        }
    }

    /// A dictionary, or a stream's dictionary.
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Obj::Dict(d) => Some(d),
            Obj::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    pub fn as_stream(&self) -> Option<&Arc<Stream>> {
        match self {
            Obj::Stream(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_ref(&self) -> Option<(u32, u16)> {
        match self {
            Obj::Ref(n, g) => Some((*n, *g)),
            _ => None,
        }
    }

    /// A numeric array as f64s (non-numbers become 0).
    pub fn nums(&self) -> Vec<f64> {
        self.as_arr().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default()
    }

    /// A text string (title, outline entry, contents) as Rust text.
    pub fn as_text(&self) -> Option<String> {
        self.as_bytes().map(decode_text)
    }
}

/// The error a PDF can give when opened.
#[derive(Debug, Clone, PartialEq)]
pub enum FolioError {
    /// Not a PDF at all.
    NotPdf,
    /// Locked with a password (or a certificate).
    Encrypted,
    /// Damaged beyond what repair could recover.
    Broken(String),
}

impl fmt::Display for FolioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FolioError::NotPdf => write!(f, "This file is not a PDF."),
            FolioError::Encrypted => write!(f, "This PDF is password-protected - opening locked files arrives later."),
            FolioError::Broken(m) => write!(f, "This PDF is damaged and could not be opened: {}", m),
        }
    }
}

impl std::error::Error for FolioError {}

// ---------------------------------------------------------------------------------------------------
// Text strings
// ---------------------------------------------------------------------------------------------------

/// PDFDocEncoding 0x80..=0x9F and 0xA0 differ from Latin-1.
const PDFDOC_80: [u32; 33] = [
    0x2022, 0x2020, 0x2021, 0x2026, 0x2014, 0x2013, 0x0192, 0x2044, 0x2039, 0x203A, 0x2212, 0x2030, 0x201E, 0x201C, 0x201D, 0x2018,
    0x2019, 0x201A, 0x2122, 0xFB01, 0xFB02, 0x0141, 0x0152, 0x0160, 0x0178, 0x017D, 0x0131, 0x0142, 0x0153, 0x0161, 0x017E, 0xFFFD,
    0x20AC,
];

/// Decode a text string: UTF-16BE with a BOM, UTF-8 with a BOM, or PDFDocEncoding.
pub fn decode_text(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        return utf16be(&b[2..]);
    }
    if b.len() >= 3 && b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF {
        return String::from_utf8_lossy(&b[3..]).into_owned();
    }
    b.iter()
        .map(|&c| match c {
            0x18..=0x1F => {
                // breve caron circumflex dotaccent hungarumlaut ogonek ring tilde
                const T: [u32; 8] = [0x02D8, 0x02C7, 0x02C6, 0x02D9, 0x02DD, 0x02DB, 0x02DA, 0x02DC];
                char::from_u32(T[(c - 0x18) as usize]).unwrap_or('\u{FFFD}')
            }
            0x80..=0xA0 => char::from_u32(PDFDOC_80[(c - 0x80) as usize]).unwrap_or('\u{FFFD}'),
            _ => c as char,
        })
        .collect()
}

/// UTF-16BE (surrogate pairs joined; lone halves become U+FFFD).
pub fn utf16be(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
    char::decode_utf16(units.iter().copied()).map(|r| r.unwrap_or('\u{FFFD}')).collect()
}

/// Encode text for a text string: plain bytes when it is all printable ASCII, else UTF-16BE with a BOM.
pub fn encode_text(s: &str) -> Vec<u8> {
    if s.chars().all(|c| (' '..='~').contains(&c) || c == '\n' || c == '\r' || c == '\t') {
        return s.as_bytes().to_vec();
    }
    let mut out = vec![0xFE, 0xFF];
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_be_bytes());
    }
    out
}

// ---------------------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------------------

fn is_regular(c: u8) -> bool {
    !matches!(c, 0 | 9 | 10 | 12 | 13 | 32 | b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

/// A number the way PDF writes it (no exponent, at most 6 decimals, no trailing zeros).
pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let mut s = format!("{:.6}", v);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// Write a name with `#xx` escapes for anything that is not a regular printable character.
pub fn write_name(out: &mut Vec<u8>, name: &str) {
    out.push(b'/');
    for ch in name.chars() {
        let c = ch as u32;
        let b = if c < 256 { c as u8 } else { b'?' };
        if b > b' ' && b < 0x7F && b != b'#' && is_regular(b) {
            out.push(b);
        } else {
            out.extend_from_slice(format!("#{:02X}", b).as_bytes());
        }
    }
}

/// Write a string: literal with escapes when mostly printable, hex otherwise.
pub fn write_string(out: &mut Vec<u8>, s: &[u8]) {
    let binary = s.iter().filter(|&&c| c < 0x20 && !matches!(c, b'\n' | b'\r' | b'\t') || c >= 0x7F).count();
    if binary * 4 > s.len() {
        out.push(b'<');
        for c in s {
            out.extend_from_slice(format!("{:02X}", c).as_bytes());
        }
        out.push(b'>');
        return;
    }
    out.push(b'(');
    for &c in s {
        match c {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(c);
            }
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x20..=0x7E => out.push(c),
            _ => out.extend_from_slice(format!("\\{:03o}", c).as_bytes()),
        }
    }
    out.push(b')');
}

/// Serialize an object. A stream's /Length is rewritten to its data's length.
pub fn write_obj(out: &mut Vec<u8>, o: &Obj) {
    match o {
        Obj::Null => out.extend_from_slice(b"null"),
        Obj::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Obj::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Obj::Real(r) => out.extend_from_slice(fmt_num(*r).as_bytes()),
        Obj::Str(s) => write_string(out, s),
        Obj::Name(n) => write_name(out, n),
        Obj::Arr(a) => {
            out.push(b'[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_obj(out, v);
            }
            out.push(b']');
        }
        Obj::Dict(d) => write_dict(out, d),
        Obj::Stream(s) => {
            let mut d = s.dict.clone();
            d.set("Length", Obj::Int(s.data.len() as i64));
            write_dict(out, &d);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&s.data);
            out.extend_from_slice(b"\nendstream");
        }
        Obj::Ref(n, g) => out.extend_from_slice(format!("{} {} R", n, g).as_bytes()),
    }
}

pub fn write_dict(out: &mut Vec<u8>, d: &Dict) {
    out.extend_from_slice(b"<<");
    for (k, v) in &d.0 {
        write_name(out, k);
        out.push(b' ');
        write_obj(out, v);
    }
    out.extend_from_slice(b">>");
}

pub fn to_bytes(o: &Obj) -> Vec<u8> {
    let mut v = Vec::new();
    write_obj(&mut v, o);
    v
}
