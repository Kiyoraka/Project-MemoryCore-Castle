//! A tiny PDF reader for the tests only: reads our own output's cross-reference table, parses the
//! objects it points at, and inflates streams, so tests can check structure.

use crate::ground::flate::zlib_decompress;

#[derive(Debug, Clone, PartialEq)]
pub enum Obj {
    Null,
    Bool(bool),
    Num(f64),
    Name(String),
    Str(Vec<u8>),
    Arr(Vec<Obj>),
    Dict(Vec<(String, Obj)>),
    Ref(u32),
    /// The dictionary and the raw (still encoded) bytes.
    Stream(Vec<(String, Obj)>, Vec<u8>),
}

impl Obj {
    pub fn get(&self, key: &str) -> Option<&Obj> {
        match self {
            Obj::Dict(d) | Obj::Stream(d, _) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        if let Obj::Num(n) = self {
            Some(*n)
        } else {
            None
        }
    }

    pub fn name(&self) -> Option<&str> {
        if let Obj::Name(n) = self {
            Some(n)
        } else {
            None
        }
    }

    pub fn arr(&self) -> Option<&[Obj]> {
        if let Obj::Arr(a) = self {
            Some(a)
        } else {
            None
        }
    }

    pub fn reference(&self) -> Option<u32> {
        if let Obj::Ref(r) = self {
            Some(*r)
        } else {
            None
        }
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        if let Obj::Str(s) = self {
            Some(s)
        } else {
            None
        }
    }

    pub fn dict(&self) -> Option<&[(String, Obj)]> {
        match self {
            Obj::Dict(d) | Obj::Stream(d, _) => Some(d),
            _ => None,
        }
    }
}

pub struct Doc {
    pub data: Vec<u8>,
    /// Byte offset of each object by number (index 0 is the free head).
    pub offsets: Vec<usize>,
    pub trailer: Obj,
}

struct P<'a> {
    d: &'a [u8],
    i: usize,
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\n' | b'\r' | b'\t' | 0x0c | 0)
}

fn is_delim(b: u8) -> bool {
    b"()<>[]{}/%".contains(&b)
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.d.len() {
            let b = self.d[self.i];
            if is_ws(b) {
                self.i += 1;
            } else if b == b'%' {
                while self.i < self.d.len() && self.d[self.i] != b'\n' && self.d[self.i] != b'\r' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    fn word(&mut self) -> &'a [u8] {
        let s = self.i;
        while self.i < self.d.len() && !is_ws(self.d[self.i]) && !is_delim(self.d[self.i]) {
            self.i += 1;
        }
        &self.d[s..self.i]
    }

    fn peek_int_int_r(&self) -> Option<(u32, usize)> {
        // At "N G R"? Returns N and the index after the R.
        let mut q = P { d: self.d, i: self.i };
        let a = std::str::from_utf8(q.word()).ok()?.parse::<u32>().ok()?;
        q.ws();
        std::str::from_utf8(q.word()).ok()?.parse::<u32>().ok()?;
        q.ws();
        if q.d.get(q.i) == Some(&b'R') {
            Some((a, q.i + 1))
        } else {
            None
        }
    }

    fn value(&mut self) -> Result<Obj, String> {
        self.ws();
        let b = *self.d.get(self.i).ok_or("eof")?;
        match b {
            b'/' => {
                self.i += 1;
                let raw = self.word();
                let mut s = String::new();
                let mut k = 0;
                while k < raw.len() {
                    if raw[k] == b'#' && k + 2 < raw.len() {
                        let h = std::str::from_utf8(&raw[k + 1..k + 3]).map_err(|e| e.to_string())?;
                        s.push(u8::from_str_radix(h, 16).map_err(|e| e.to_string())? as char);
                        k += 3;
                    } else {
                        s.push(raw[k] as char);
                        k += 1;
                    }
                }
                Ok(Obj::Name(s))
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.d.get(self.i) == Some(&b']') {
                        self.i += 1;
                        return Ok(Obj::Arr(v));
                    }
                    v.push(self.value()?);
                }
            }
            b'<' if self.d.get(self.i + 1) == Some(&b'<') => {
                self.i += 2;
                let mut d = Vec::new();
                loop {
                    self.ws();
                    if self.d.get(self.i..self.i + 2) == Some(b">>") {
                        self.i += 2;
                        break;
                    }
                    let k = match self.value()? {
                        Obj::Name(n) => n,
                        o => return Err(format!("dict key {o:?}")),
                    };
                    let v = self.value()?;
                    d.push((k, v));
                }
                // A stream follows?
                let save = self.i;
                self.ws();
                if self.d[self.i..].starts_with(b"stream") {
                    self.i += 6;
                    if self.d.get(self.i) == Some(&b'\r') {
                        self.i += 1;
                    }
                    if self.d.get(self.i) == Some(&b'\n') {
                        self.i += 1;
                    }
                    let len = d.iter().find(|(k, _)| k == "Length").and_then(|(_, v)| v.num()).ok_or("stream without direct Length")? as usize;
                    let raw = self.d.get(self.i..self.i + len).ok_or("stream past end")?.to_vec();
                    self.i += len;
                    self.ws();
                    if !self.d[self.i..].starts_with(b"endstream") {
                        return Err("no endstream after Length bytes".into());
                    }
                    self.i += 9;
                    return Ok(Obj::Stream(d, raw));
                }
                self.i = save;
                Ok(Obj::Dict(d))
            }
            b'<' => {
                self.i += 1;
                let mut hex = Vec::new();
                while self.i < self.d.len() && self.d[self.i] != b'>' {
                    if !is_ws(self.d[self.i]) {
                        hex.push(self.d[self.i]);
                    }
                    self.i += 1;
                }
                self.i += 1;
                if hex.len() % 2 == 1 {
                    hex.push(b'0');
                }
                let mut out = Vec::new();
                for p in hex.chunks(2) {
                    let s = std::str::from_utf8(p).map_err(|e| e.to_string())?;
                    out.push(u8::from_str_radix(s, 16).map_err(|e| e.to_string())?);
                }
                Ok(Obj::Str(out))
            }
            b'(' => {
                self.i += 1;
                let mut out = Vec::new();
                let mut depth = 0;
                loop {
                    let c = *self.d.get(self.i).ok_or("eof in string")?;
                    self.i += 1;
                    match c {
                        b'\\' => {
                            let e = *self.d.get(self.i).ok_or("eof")?;
                            self.i += 1;
                            match e {
                                b'n' => out.push(b'\n'),
                                b'r' => out.push(b'\r'),
                                b't' => out.push(b'\t'),
                                b'0'..=b'7' => {
                                    let mut v = (e - b'0') as u32;
                                    for _ in 0..2 {
                                        match self.d.get(self.i) {
                                            Some(&d @ b'0'..=b'7') => {
                                                v = v * 8 + (d - b'0') as u32;
                                                self.i += 1;
                                            }
                                            _ => break,
                                        }
                                    }
                                    out.push(v as u8);
                                }
                                other => out.push(other),
                            }
                        }
                        b'(' => {
                            depth += 1;
                            out.push(c);
                        }
                        b')' => {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                            out.push(c);
                        }
                        _ => out.push(c),
                    }
                }
                Ok(Obj::Str(out))
            }
            _ => {
                if let Some((n, after)) = self.peek_int_int_r() {
                    self.i = after;
                    return Ok(Obj::Ref(n));
                }
                let w = self.word();
                match w {
                    b"true" => Ok(Obj::Bool(true)),
                    b"false" => Ok(Obj::Bool(false)),
                    b"null" => Ok(Obj::Null),
                    _ => {
                        let s = std::str::from_utf8(w).map_err(|e| e.to_string())?;
                        s.parse::<f64>().map(Obj::Num).map_err(|_| format!("bad token {s:?} at {}", self.i))
                    }
                }
            }
        }
    }
}

impl Doc {
    pub fn parse(data: Vec<u8>) -> Result<Doc, String> {
        if !data.starts_with(b"%PDF-") {
            return Err("no header".into());
        }
        let tail = &data[data.len().saturating_sub(64)..];
        let pos = tail.windows(9).rposition(|w| w == b"startxref").ok_or("no startxref")?;
        let mut p = P { d: tail, i: pos + 9 };
        p.ws();
        let xref: usize = std::str::from_utf8(p.word()).map_err(|e| e.to_string())?.parse().map_err(|_| "bad startxref")?;
        let mut p = P { d: &data, i: xref };
        if p.word() != b"xref" {
            return Err("startxref does not point at xref".into());
        }
        p.ws();
        let first: usize = std::str::from_utf8(p.word()).unwrap().parse().map_err(|_| "bad xref")?;
        p.ws();
        let count: usize = std::str::from_utf8(p.word()).unwrap().parse().map_err(|_| "bad xref")?;
        if first != 0 {
            return Err("xref section not from 0".into());
        }
        p.ws();
        let mut offsets = Vec::with_capacity(count);
        for k in 0..count {
            let e = data.get(p.i..p.i + 20).ok_or("xref truncated")?;
            if e[18] != b'\r' || e[19] != b'\n' {
                return Err(format!("xref entry {k} not 20 bytes"));
            }
            let off: usize = std::str::from_utf8(&e[..10]).unwrap().parse().map_err(|_| "bad offset")?;
            offsets.push(off);
            p.i += 20;
        }
        p.ws();
        if p.word() != b"trailer" {
            return Err("no trailer".into());
        }
        let trailer = p.value()?;
        Ok(Doc { data, offsets, trailer })
    }

    pub fn count(&self) -> usize {
        self.offsets.len()
    }

    /// Object `n`, checking that its offset points at `n 0 obj`.
    pub fn obj(&self, n: u32) -> Result<Obj, String> {
        let off = *self.offsets.get(n as usize).ok_or(format!("no object {n}"))?;
        let head = format!("{n} 0 obj");
        if !self.data[off..].starts_with(head.as_bytes()) {
            return Err(format!("offset of {n} does not point at '{head}'"));
        }
        let mut p = P { d: &self.data, i: off + head.len() };
        let v = p.value()?;
        p.ws();
        if !self.data[p.i..].starts_with(b"endobj") {
            return Err(format!("object {n} has no endobj"));
        }
        Ok(v)
    }

    /// Follow a reference (anything else is returned as it is).
    pub fn resolve(&self, o: &Obj) -> Obj {
        match o {
            Obj::Ref(n) => self.obj(*n).expect("resolvable reference"),
            other => other.clone(),
        }
    }

    /// `dict[key]`, references followed.
    pub fn get(&self, o: &Obj, key: &str) -> Option<Obj> {
        o.get(key).map(|v| self.resolve(v))
    }

    /// A stream's bytes, inflated when FlateDecode.
    pub fn stream_data(&self, o: &Obj) -> Vec<u8> {
        let o = self.resolve(o);
        let Obj::Stream(_, raw) = &o else { panic!("not a stream: {o:?}") };
        match o.get("Filter").and_then(|f| f.name()) {
            Some("FlateDecode") => zlib_decompress(raw).expect("inflates"),
            _ => raw.clone(),
        }
    }

    pub fn catalog(&self) -> Obj {
        self.get(&self.trailer, "Root").expect("catalog")
    }

    /// The page dictionaries in order (a flat Pages tree is what we write).
    pub fn pages(&self) -> Vec<Obj> {
        let pages = self.get(&self.catalog(), "Pages").expect("pages");
        pages.get("Kids").and_then(|k| k.arr()).unwrap_or(&[]).iter().map(|k| self.resolve(k)).collect()
    }

    pub fn page_ref(&self, index: usize) -> u32 {
        let pages = self.get(&self.catalog(), "Pages").expect("pages");
        pages.get("Kids").and_then(|k| k.arr()).expect("kids")[index].reference().expect("ref")
    }

    /// Every object whose dictionary has `/Type` or `/Subtype` equal to `kind`.
    pub fn find_all(&self, kind: &str) -> Vec<(u32, Obj)> {
        let mut out = Vec::new();
        for n in 1..self.count() as u32 {
            if let Ok(o) = self.obj(n) {
                let t = o.get("Type").and_then(|t| t.name()) == Some(kind);
                let s = o.get("Subtype").and_then(|t| t.name()) == Some(kind);
                if t || s {
                    out.push((n, o));
                }
            }
        }
        out
    }
}
