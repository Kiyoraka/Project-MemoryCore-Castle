//! The PDF tokenizer and object parser, shared by the file parser, content streams and CMaps.
//! Lenient by design: a malformed token is skipped, never a panic.

use super::obj::{Dict, Obj};

/// How deep arrays and dictionaries may nest before the parser gives up on the inner part.
const MAX_DEPTH: usize = 64;

pub fn is_white(c: u8) -> bool {
    matches!(c, 0 | 9 | 10 | 12 | 13 | 32)
}

pub fn is_delim(c: u8) -> bool {
    matches!(c, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

pub fn is_regular(c: u8) -> bool {
    !is_white(c) && !is_delim(c)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Real(f64),
    Str(Vec<u8>),
    Name(String),
    ArrOpen,
    ArrClose,
    DictOpen,
    DictClose,
    /// `{` and `}` (PostScript functions in CMaps) and other operators and keywords.
    Keyword(Vec<u8>),
    Eof,
}

/// What [`Lexer::parse`] found: an object, or a keyword (an operator in a content stream, or `endobj`,
/// `stream` and the like in a file).
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Obj(Obj),
    Keyword(Vec<u8>),
    End,
}

pub struct Lexer<'a> {
    pub d: &'a [u8],
    pub pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(d: &'a [u8], pos: usize) -> Lexer<'a> {
        Lexer { d, pos }
    }

    pub fn skip_ws(&mut self) {
        while self.pos < self.d.len() {
            let c = self.d[self.pos];
            if is_white(c) {
                self.pos += 1;
            } else if c == b'%' {
                while self.pos < self.d.len() && self.d[self.pos] != b'\n' && self.d[self.pos] != b'\r' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    pub fn next(&mut self) -> Tok {
        // Stray closers are skipped (in a loop: a long run of them must not recurse).
        loop {
            self.skip_ws();
            match self.d.get(self.pos) {
                Some(b')') => self.pos += 1,
                Some(b'>') if self.d.get(self.pos + 1) != Some(&b'>') => self.pos += 1,
                _ => break,
            }
        }
        let Some(&c) = self.d.get(self.pos) else { return Tok::Eof };
        match c {
            b'[' => {
                self.pos += 1;
                Tok::ArrOpen
            }
            b']' => {
                self.pos += 1;
                Tok::ArrClose
            }
            b'{' | b'}' => {
                self.pos += 1;
                Tok::Keyword(vec![c])
            }
            b'<' => {
                if self.d.get(self.pos + 1) == Some(&b'<') {
                    self.pos += 2;
                    Tok::DictOpen
                } else {
                    self.pos += 1;
                    Tok::Str(self.hex_string())
                }
            }
            b'>' => {
                self.pos += 2;
                Tok::DictClose
            }
            b'(' => {
                self.pos += 1;
                Tok::Str(self.literal_string())
            }
            b'/' => {
                self.pos += 1;
                Tok::Name(self.name())
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number(),
            _ => {
                let start = self.pos;
                while self.pos < self.d.len() && is_regular(self.d[self.pos]) {
                    self.pos += 1;
                }
                if self.pos == start {
                    self.pos += 1;
                }
                Tok::Keyword(self.d[start..self.pos].to_vec())
            }
        }
    }

    fn number(&mut self) -> Tok {
        let start = self.pos;
        while self.pos < self.d.len() && is_regular(self.d[self.pos]) {
            self.pos += 1;
        }
        let raw = &self.d[start..self.pos];
        // Numbers like "--5", "5-", "1.2.3" exist in the wild: take the leading well-formed part.
        let mut neg = false;
        let mut i = 0;
        while i < raw.len() && (raw[i] == b'-' || raw[i] == b'+') {
            if raw[i] == b'-' {
                neg = !neg;
            }
            i += 1;
        }
        let mut int: i64 = 0;
        let mut overflow = false;
        let mut digits = 0;
        while i < raw.len() && raw[i].is_ascii_digit() {
            match int.checked_mul(10).and_then(|v| v.checked_add((raw[i] - b'0') as i64)) {
                Some(v) => int = v,
                None => overflow = true,
            }
            digits += 1;
            i += 1;
        }
        if i < raw.len() && raw[i] == b'.' {
            i += 1;
            let mut frac = 0f64;
            let mut scale = 1f64;
            let mut fd = 0;
            while i < raw.len() && raw[i].is_ascii_digit() {
                if fd < 18 {
                    frac = frac * 10.0 + (raw[i] - b'0') as f64;
                    scale *= 10.0;
                }
                fd += 1;
                i += 1;
            }
            if digits == 0 && fd == 0 {
                return Tok::Keyword(raw.to_vec());
            }
            let whole = if overflow { parse_f64(&raw[..]) } else { int as f64 + frac / scale };
            return Tok::Real(if neg { -whole } else { whole });
        }
        if digits == 0 {
            return Tok::Keyword(raw.to_vec());
        }
        if overflow {
            let v = parse_f64(raw);
            return Tok::Real(if neg { -v.abs() } else { v.abs() });
        }
        Tok::Int(if neg { -int } else { int })
    }

    fn name(&mut self) -> String {
        let mut out = String::new();
        while self.pos < self.d.len() && is_regular(self.d[self.pos]) {
            let c = self.d[self.pos];
            if c == b'#' {
                let h = self.d.get(self.pos + 1).and_then(|&a| hex_val(a));
                let l = self.d.get(self.pos + 2).and_then(|&a| hex_val(a));
                if let (Some(h), Some(l)) = (h, l) {
                    out.push((h * 16 + l) as char);
                    self.pos += 3;
                    continue;
                }
            }
            out.push(c as char);
            self.pos += 1;
        }
        out
    }

    fn hex_string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut hi: Option<u8> = None;
        while self.pos < self.d.len() {
            let c = self.d[self.pos];
            self.pos += 1;
            if c == b'>' {
                break;
            }
            if let Some(v) = hex_val(c) {
                match hi {
                    None => hi = Some(v),
                    Some(h) => {
                        out.push(h * 16 + v);
                        hi = None;
                    }
                }
            }
        }
        if let Some(h) = hi {
            out.push(h * 16);
        }
        out
    }

    fn literal_string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut depth = 1usize;
        while self.pos < self.d.len() {
            let c = self.d[self.pos];
            self.pos += 1;
            match c {
                b'(' => {
                    depth += 1;
                    out.push(c);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(c);
                }
                b'\\' => {
                    let Some(&e) = self.d.get(self.pos) else { break };
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'0'..=b'7' => {
                            let mut v = (e - b'0') as u32;
                            for _ in 0..2 {
                                match self.d.get(self.pos) {
                                    Some(&o) if (b'0'..=b'7').contains(&o) => {
                                        v = v * 8 + (o - b'0') as u32;
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((v & 0xFF) as u8);
                        }
                        b'\r' => {
                            if self.d.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        other => out.push(other),
                    }
                }
                b'\r' => {
                    if self.d.get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    out.push(b'\n');
                }
                _ => out.push(c),
            }
        }
        out
    }

    /// Parse one object (an indirect reference `n g R` included) or report the keyword met.
    pub fn parse(&mut self) -> Parsed {
        self.parse_depth(0)
    }

    fn parse_depth(&mut self, depth: usize) -> Parsed {
        let tok = self.next();
        self.from_tok(tok, depth)
    }

    fn from_tok(&mut self, tok: Tok, depth: usize) -> Parsed {
        match tok {
            Tok::Eof => Parsed::End,
            Tok::Int(i) => {
                // `n g R`?
                if i >= 0 {
                    let save = self.pos;
                    if let Tok::Int(g) = self.next() {
                        if (0..=65535).contains(&g) {
                            let save2 = self.pos;
                            if let Tok::Keyword(k) = self.next() {
                                if k == b"R" && i <= u32::MAX as i64 {
                                    return Parsed::Obj(Obj::Ref(i as u32, g as u16));
                                }
                            }
                            let _ = save2;
                        }
                    }
                    self.pos = save;
                }
                Parsed::Obj(Obj::Int(i))
            }
            Tok::Real(r) => Parsed::Obj(Obj::Real(r)),
            Tok::Str(s) => Parsed::Obj(Obj::Str(s)),
            Tok::Name(n) => Parsed::Obj(Obj::Name(n)),
            Tok::ArrOpen => {
                let mut items = Vec::new();
                loop {
                    let t = self.next();
                    match t {
                        Tok::ArrClose | Tok::Eof => break,
                        Tok::DictClose => continue,
                        _ => {}
                    }
                    if depth >= MAX_DEPTH {
                        continue;
                    }
                    match self.from_tok(t, depth + 1) {
                        Parsed::Obj(o) => items.push(o),
                        Parsed::Keyword(k) => match k.as_slice() {
                            b"true" => items.push(Obj::Bool(true)),
                            b"false" => items.push(Obj::Bool(false)),
                            b"null" => items.push(Obj::Null),
                            b"endobj" | b"stream" | b"endstream" | b"obj" => break,
                            _ => {}
                        },
                        Parsed::End => break,
                    }
                }
                Parsed::Obj(Obj::Arr(items))
            }
            Tok::DictOpen => {
                let mut d = Dict::new();
                loop {
                    let t = self.next();
                    let key = match t {
                        Tok::DictClose | Tok::Eof => break,
                        Tok::Name(n) => n,
                        Tok::Keyword(k) if matches!(k.as_slice(), b"endobj" | b"stream" | b"endstream" | b"obj") => break,
                        _ => continue,
                    };
                    if depth >= MAX_DEPTH {
                        continue;
                    }
                    let save = self.pos;
                    let vt = self.next();
                    match vt {
                        Tok::DictClose => {
                            d.0.push((key, Obj::Null));
                            break;
                        }
                        Tok::Name(n) => d.0.push((key, Obj::Name(n))),
                        Tok::Eof => break,
                        _ => match self.from_tok(vt, depth + 1) {
                            Parsed::Obj(o) => d.0.push((key, o)),
                            Parsed::Keyword(k) => match k.as_slice() {
                                b"true" => d.0.push((key, Obj::Bool(true))),
                                b"false" => d.0.push((key, Obj::Bool(false))),
                                b"null" => d.0.push((key, Obj::Null)),
                                b"endobj" | b"stream" | b"endstream" | b"obj" => {
                                    self.pos = save;
                                    // Let the caller see the keyword: close the dict here.
                                    let _ = self.next();
                                    self.pos = save;
                                    break;
                                }
                                _ => {}
                            },
                            Parsed::End => break,
                        },
                    }
                }
                Parsed::Obj(Obj::Dict(d))
            }
            Tok::ArrClose | Tok::DictClose => Parsed::Keyword(Vec::new()),
            Tok::Keyword(k) => match k.as_slice() {
                b"true" => Parsed::Obj(Obj::Bool(true)),
                b"false" => Parsed::Obj(Obj::Bool(false)),
                b"null" => Parsed::Obj(Obj::Null),
                _ => Parsed::Keyword(k),
            },
        }
    }

    /// Parse an object where an object must be (keywords become `Null`).
    pub fn object(&mut self) -> Obj {
        match self.parse() {
            Parsed::Obj(o) => o,
            _ => Obj::Null,
        }
    }

    /// Read an unsigned integer token (for xref tables), or None.
    pub fn uint(&mut self) -> Option<u64> {
        match self.next() {
            Tok::Int(i) if i >= 0 => Some(i as u64),
            _ => None,
        }
    }
}

pub fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn parse_f64(raw: &[u8]) -> f64 {
    let s: String = raw.iter().filter(|c| c.is_ascii_digit() || **c == b'.' || **c == b'-').map(|&c| c as char).collect();
    s.parse::<f64>().unwrap_or(0.0)
}

/// Find `needle` in `hay` from `from`.
pub fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() || hay.len() - from < needle.len() {
        return None;
    }
    let first = needle[0];
    let last = hay.len() - needle.len();
    let mut i = from;
    while i <= last {
        if hay[i] == first && &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find the last `needle` in `hay`.
pub fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let mut i = hay.len() - needle.len();
    loop {
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}
