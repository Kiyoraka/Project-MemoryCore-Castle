//! The PDF file: cross-reference tables and streams, object streams, incremental updates (newest
//! wins along the `/Prev` chain) and repair by scanning for `n g obj` when the cross-reference is
//! missing or wrong. Objects are parsed on first use and cached.
//!
//! Edits never touch the original bytes: changed and new objects live in an overlay that
//! [`super::edit`] appends as an incremental update.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::crypt::{self, Security, UnlockError};
use super::filters::{self, Decoded};
use super::lexer::{find, is_regular, is_white, rfind, Lexer, Parsed, Tok};
use super::obj::{Dict, FolioError, Obj, Stream};

/// Where an object lives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum XRef {
    /// Byte offset of `n g obj`, and the generation.
    Offset(usize, u16),
    /// Inside object stream `stm`, at position `idx`.
    Compressed(u32, u32),
}

/// A parsed object stream: its decoded bytes, where its objects start, and their numbers.
struct ObjStm {
    data: Vec<u8>,
    first: usize,
    entries: Vec<(u32, usize)>,
}

pub struct PdfFile {
    /// The bytes as opened. Never changed.
    pub data: Vec<u8>,
    pub version: String,
    xref: HashMap<u32, XRef>,
    trailer: Dict,
    /// Keys an edit set on the trailer (a new /Info).
    pub trailer_edits: Dict,
    /// Offset of the newest cross-reference section (the update's /Prev).
    pub startxref: usize,
    /// True when the newest section is a cross-reference stream (the update then writes one too).
    pub xref_is_stream: bool,
    /// True when the cross-reference had to be rebuilt by scanning.
    pub repaired: bool,
    /// Changed and new objects (the edits).
    pub overlay: BTreeMap<u32, Obj>,
    /// The keys of a locked file (strings and streams are unlocked as objects load, and locked again
    /// when an update is written).
    pub security: Option<Security>,
    cache: Mutex<HashMap<u32, Obj>>,
    objstms: Mutex<HashMap<u32, Option<Arc<ObjStm>>>>,
    /// Object streams being loaded right now (a stream whose /Length lives inside itself).
    loading: Mutex<HashSet<u32>>,
    /// Object offsets found by scanning, built the first time an offset proves wrong.
    scanned: Mutex<Option<Arc<HashMap<u32, usize>>>>,
}

impl std::fmt::Debug for PdfFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfFile").field("bytes", &self.data.len()).field("objects", &self.xref.len()).field("repaired", &self.repaired).finish()
    }
}

/// How deep reference chains and nested lookups may go.
const MAX_DEPTH: u32 = 16;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl PdfFile {
    /// Parse a file. A locked file opens when its user password is empty; otherwise the answer is
    /// [`FolioError::NeedsPassword`] (see [`PdfFile::open_with`]).
    pub fn open(data: Vec<u8>) -> Result<PdfFile, FolioError> {
        PdfFile::open_with(data, None)
    }

    /// Parse a file, unlocking it with `password` (the user's or the owner's) when it is locked.
    pub fn open_with(data: Vec<u8>, password: Option<&str>) -> Result<PdfFile, FolioError> {
        let head = &data[..data.len().min(1024)];
        let version = match find(head, b"%PDF-", 0) {
            Some(p) => {
                let v: String = data[p + 5..].iter().take(3).take_while(|c| c.is_ascii_digit() || **c == b'.').map(|&c| c as char).collect();
                if v.is_empty() {
                    "1.4".to_string()
                } else {
                    v
                }
            }
            None => {
                if find(&data, b" obj", 0).is_none() {
                    return Err(FolioError::NotPdf);
                }
                "1.4".to_string()
            }
        };
        let mut f = PdfFile {
            data,
            version,
            xref: HashMap::new(),
            trailer: Dict::new(),
            trailer_edits: Dict::new(),
            startxref: 0,
            xref_is_stream: false,
            repaired: false,
            overlay: BTreeMap::new(),
            security: None,
            cache: Mutex::new(HashMap::new()),
            objstms: Mutex::new(HashMap::new()),
            loading: Mutex::new(HashSet::new()),
            scanned: Mutex::new(None),
        };
        let ok = f.load_xref_chain().is_ok() && f.root_ok();
        if !ok {
            f.repair();
        }
        if f.xref.is_empty() {
            return Err(FolioError::Broken("no objects could be found".into()));
        }
        if let Some(enc) = f.trailer.get("Encrypt").filter(|e| !e.is_null()).cloned() {
            f.unlock(&enc, password)?;
        }
        Ok(f)
    }

    /// Unlock a locked file: find its keys, then forget every object read so far (they were read
    /// still locked).
    fn unlock(&mut self, enc: &Obj, password: Option<&str>) -> Result<(), FolioError> {
        let dict_num = enc.as_ref().map_or(0, |r| r.0);
        let ed = self.resolve(enc);
        let Some(ed) = ed.as_dict().cloned() else {
            return Err(FolioError::Unsupported("its /Encrypt dictionary is missing".into()));
        };
        let id0 = match self.resolve(self.trailer.get("ID").unwrap_or(&Obj::Null)) {
            Obj::Arr(a) => a.first().map(|v| self.resolve(v)).and_then(|v| v.as_bytes().map(|b| b.to_vec())).unwrap_or_default(),
            _ => Vec::new(),
        };
        let sec = {
            let resolve = |o: &Obj| self.resolve(o);
            crypt::unlock(&ed, dict_num, &id0, password, &resolve)
        };
        match sec {
            Ok(sec) => {
                self.security = Some(sec);
                *lock(&self.cache) = HashMap::new();
                *lock(&self.objstms) = HashMap::new();
                if self.repaired {
                    // The object streams' contents could only be listed once they were unlocked.
                    let nums: Vec<u32> = self.xref.iter().filter(|(_, x)| matches!(x, XRef::Offset(..))).map(|(n, _)| *n).collect();
                    for num in nums {
                        if self.get(num).as_dict().map_or(false, |d| d.is_type("ObjStm")) {
                            if let Some(stm) = self.objstm(num) {
                                for (i, (n, _)) in stm.entries.iter().enumerate() {
                                    self.xref.entry(*n).or_insert(XRef::Compressed(num, i as u32));
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            Err(UnlockError::Password) => Err(if password.map_or(false, |p| !p.is_empty()) { FolioError::WrongPassword } else { FolioError::NeedsPassword }),
            Err(UnlockError::Unsupported(m)) => Err(FolioError::Unsupported(m)),
        }
    }

    /// An object just read from the file, unlocked when the file is locked.
    fn unlocked(&self, o: Obj, num: u32, gen: u16) -> Obj {
        match &self.security {
            Some(sec) => sec.decrypt_obj(o, num, gen, &|x: &Obj| self.resolve(x)),
            None => o,
        }
    }

    /// An object as an update writes it: locked again when the file is locked.
    pub fn locked(&self, o: &Obj, num: u32, gen: u16) -> Obj {
        match &self.security {
            Some(sec) => sec.encrypt_obj(o, num, gen, &|x: &Obj| self.resolve(x)),
            None => o.clone(),
        }
    }

    fn root_ok(&self) -> bool {
        match self.trailer.get("Root") {
            Some(r) => self.resolve(r).as_dict().map_or(false, |d| d.contains("Pages") || d.is_type("Catalog")),
            None => false,
        }
    }

    /// The trailer, with any edits applied.
    pub fn trailer(&self) -> Dict {
        let mut t = self.trailer.clone();
        for (k, v) in &self.trailer_edits.0 {
            t.set(k, v.clone());
        }
        t
    }

    /// The trailer as read from the file.
    pub fn base_trailer(&self) -> &Dict {
        &self.trailer
    }

    /// Every object number the cross-reference knows (overlay included), ascending.
    pub fn object_numbers(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.xref.keys().copied().chain(self.overlay.keys().copied()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn xref_entry(&self, num: u32) -> Option<XRef> {
        self.xref.get(&num).copied()
    }

    /// The generation an object has in the file (0 when new).
    pub fn generation(&self, num: u32) -> u16 {
        match self.xref.get(&num) {
            Some(XRef::Offset(_, g)) => *g,
            _ => 0,
        }
    }

    /// One past the highest object number in use.
    pub fn size(&self) -> u32 {
        let from_trailer = self.trailer.get("Size").and_then(Obj::as_i64).unwrap_or(0).clamp(0, 10_000_000) as u32;
        let max_x = self.xref.keys().copied().max().map_or(0, |m| m + 1);
        let max_o = self.overlay.keys().copied().max().map_or(0, |m| m + 1);
        from_trailer.max(max_x).max(max_o).max(1)
    }

    // -----------------------------------------------------------------------------------------------
    // Cross-reference
    // -----------------------------------------------------------------------------------------------

    fn load_xref_chain(&mut self) -> Result<(), String> {
        let tail_from = self.data.len().saturating_sub(4096);
        let sx = rfind(&self.data[tail_from..], b"startxref").map(|p| p + tail_from).or_else(|| rfind(&self.data, b"startxref")).ok_or("no startxref")?;
        let mut lx = Lexer::new(&self.data, sx + 9);
        let first = lx.uint().ok_or("bad startxref")? as usize;
        self.startxref = first;
        let mut visited = HashSet::new();
        let mut next = Some(first);
        let mut sections = 0;
        let mut first_section = true;
        while let Some(off) = next {
            if !visited.insert(off) || sections > 256 {
                break;
            }
            sections += 1;
            let (trailer, is_stream) = self.load_section(off)?;
            if first_section {
                self.xref_is_stream = is_stream;
                self.trailer = trailer.clone();
                first_section = false;
            } else {
                for k in ["Root", "Info", "ID", "Encrypt"] {
                    if !self.trailer.contains(k) {
                        if let Some(v) = trailer.get(k) {
                            self.trailer.set(k, v.clone());
                        }
                    }
                }
            }
            // A hybrid file's classic section points at a stream with the compressed objects.
            if !is_stream {
                if let Some(x) = trailer.get("XRefStm").and_then(Obj::as_i64) {
                    if x > 0 && visited.insert(x as usize) {
                        let _ = self.load_section(x as usize);
                    }
                }
            }
            next = trailer.get("Prev").and_then(Obj::as_i64).filter(|&p| p > 0 && (p as usize) < self.data.len()).map(|p| p as usize);
        }
        if self.xref.is_empty() {
            return Err("empty cross-reference".into());
        }
        // Spot-check: each in-file offset should point at "n g obj" (a few samples are enough).
        let mut bad = 0;
        let mut checked = 0;
        for (&num, x) in self.xref.iter() {
            if let XRef::Offset(off, _) = x {
                checked += 1;
                if !self.obj_header_at(*off).map_or(false, |(n, _)| n == num) {
                    bad += 1;
                }
                if checked >= 64 {
                    break;
                }
            }
        }
        if checked > 0 && bad * 4 > checked {
            return Err("cross-reference offsets are wrong".into());
        }
        Ok(())
    }

    fn insert(&mut self, num: u32, x: XRef) {
        self.xref.entry(num).or_insert(x);
    }

    /// Load one section (classic table or stream). Returns its trailer and whether it was a stream.
    fn load_section(&mut self, off: usize) -> Result<(Dict, bool), String> {
        let mut lx = Lexer::new(&self.data, off);
        lx.skip_ws();
        let start = lx.pos;
        if self.data[start..].starts_with(b"xref") {
            lx.pos = start + 4;
            let mut entries: Vec<(u32, XRef)> = Vec::new();
            loop {
                let save = lx.pos;
                match lx.next() {
                    Tok::Int(s) if s >= 0 => {
                        let count = lx.uint().ok_or("bad xref subsection")? as usize;
                        if count > self.data.len() / 18 + 1 {
                            return Err("xref count too large".into());
                        }
                        let mut s = s as u64;
                        for k in 0..count {
                            let o = lx.uint().ok_or("bad xref entry")?;
                            let g = lx.uint().ok_or("bad xref entry")?;
                            let kind = match lx.next() {
                                Tok::Keyword(kw) => kw,
                                _ => return Err("bad xref entry".into()),
                            };
                            // A first subsection written "1 n" but starting with the free head.
                            if k == 0 && s == 1 && kind == b"f" && g == 65535 && o == 0 {
                                s = 0;
                            }
                            let num = s + k as u64;
                            if num > u32::MAX as u64 {
                                continue;
                            }
                            if kind == b"n" && o > 0 && (o as usize) < self.data.len() {
                                entries.push((num as u32, XRef::Offset(o as usize, g.min(65535) as u16)));
                            }
                        }
                    }
                    Tok::Keyword(k) if k == b"trailer" => break,
                    _ => {
                        lx.pos = save;
                        break;
                    }
                }
            }
            let trailer = match lx.parse() {
                Parsed::Obj(Obj::Dict(d)) => d,
                _ => return Err("no trailer".into()),
            };
            for (n, x) in entries {
                self.insert(n, x);
            }
            return Ok((trailer, false));
        }
        // A cross-reference stream: "n g obj << /Type /XRef ... >> stream".
        let (_, _, obj) = self.parse_indirect_at(off, 0).ok_or("bad xref stream")?;
        let s = obj.as_stream().ok_or("xref is not a stream")?.clone();
        if s.dict.name("Type") != Some("XRef") {
            return Err("not an xref stream".into());
        }
        let decoded = self.decode_stream(&s, 0)?.data;
        let w: Vec<usize> = s.dict.get("W").map(|w| w.nums().iter().map(|v| v.clamp(0.0, 8.0) as usize).collect()).unwrap_or_default();
        if w.len() < 3 {
            return Err("bad /W".into());
        }
        let row = w[0] + w[1] + w[2];
        if row == 0 {
            return Err("bad /W".into());
        }
        let size = s.dict.get("Size").and_then(Obj::as_i64).unwrap_or(0).max(0) as u64;
        let index: Vec<u64> = match s.dict.get("Index") {
            Some(Obj::Arr(a)) => a.iter().map(|v| v.as_i64().unwrap_or(0).max(0) as u64).collect(),
            _ => vec![0, size],
        };
        let read = |p: usize, n: usize| -> Option<u64> {
            let b = decoded.get(p..p + n)?;
            Some(b.iter().fold(0u64, |acc, &x| (acc << 8) | x as u64))
        };
        let mut pos = 0usize;
        let mut entries = Vec::new();
        for pair in index.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            for k in 0..pair[1] {
                if pos + row > decoded.len() {
                    break;
                }
                let t = if w[0] == 0 { 1 } else { read(pos, w[0]).unwrap_or(0) };
                let f2 = read(pos + w[0], w[1]).unwrap_or(0);
                let f3 = if w[2] == 0 { 0 } else { read(pos + w[0] + w[1], w[2]).unwrap_or(0) };
                pos += row;
                let num = pair[0] + k;
                if num > u32::MAX as u64 {
                    continue;
                }
                match t {
                    1 if f2 > 0 && (f2 as usize) < self.data.len() => entries.push((num as u32, XRef::Offset(f2 as usize, f3.min(65535) as u16))),
                    2 if f2 <= u32::MAX as u64 => entries.push((num as u32, XRef::Compressed(f2 as u32, f3.min(u32::MAX as u64) as u32))),
                    _ => {}
                }
            }
        }
        for (n, x) in entries {
            self.insert(n, x);
        }
        Ok((s.dict.clone(), true))
    }

    /// Rebuild the cross-reference by scanning for `n g obj` (later definitions win), collect the
    /// trailers seen, and look inside object streams.
    fn repair(&mut self) {
        self.repaired = true;
        self.xref.clear();
        self.cache = Mutex::new(HashMap::new());
        let scanned = scan_objects(&self.data);
        for (&num, &(off, gen)) in &scanned.0 {
            self.xref.insert(num, XRef::Offset(off, gen));
        }
        let mut trailer = Dict::new();
        for t in &scanned.1 {
            for (k, v) in &t.0 {
                trailer.set(k, v.clone());
            }
        }
        // Object streams, and cross-reference streams as trailers.
        let nums: Vec<u32> = self.xref.keys().copied().collect();
        let mut catalog: Option<u32> = None;
        let mut sorted = nums.clone();
        sorted.sort_unstable();
        for num in sorted {
            let o = self.get(num);
            if let Some(d) = o.as_dict() {
                match d.name("Type") {
                    Some("ObjStm") => {
                        if let Some(stm) = self.objstm(num) {
                            for (i, (n, _)) in stm.entries.iter().enumerate() {
                                self.xref.entry(*n).or_insert(XRef::Compressed(num, i as u32));
                            }
                        }
                    }
                    Some("XRef") => {
                        for k in ["Root", "Info", "ID", "Encrypt"] {
                            if let Some(v) = d.get(k) {
                                trailer.set(k, v.clone());
                            }
                        }
                    }
                    Some("Catalog") => catalog = Some(num),
                    _ => {}
                }
            }
        }
        let root_ok = trailer.get("Root").map_or(false, |r| self.resolve(r).as_dict().map_or(false, |d| d.contains("Pages")));
        if !root_ok {
            if catalog.is_none() {
                // Compressed catalogs show up only now.
                let all: Vec<u32> = self.xref.keys().copied().collect();
                for n in all {
                    if self.get(n).as_dict().map_or(false, |d| d.is_type("Catalog")) {
                        catalog = Some(catalog.map_or(n, |c: u32| c.max(n)));
                    }
                }
            }
            match catalog {
                Some(c) => trailer.set("Root", Obj::Ref(c, self.generation(c))),
                None => {
                    trailer.remove("Root");
                }
            }
        }
        trailer.remove("Prev");
        trailer.remove("XRefStm");
        self.trailer = trailer;
        *lock(&self.scanned) = Some(Arc::new(scanned.0.iter().map(|(&n, &(o, _))| (n, o)).collect()));
    }

    // -----------------------------------------------------------------------------------------------
    // Objects
    // -----------------------------------------------------------------------------------------------

    /// Object `num` (overlay first), or `Null` when it does not exist.
    pub fn get(&self, num: u32) -> Obj {
        self.get_depth(num, 0)
    }

    /// Object `num` as the file has it, ignoring edits.
    pub fn base_object(&self, num: u32) -> Obj {
        if let Some(o) = lock(&self.cache).get(&num) {
            return o.clone();
        }
        let o = self.load(num, 0);
        lock(&self.cache).insert(num, o.clone());
        o
    }

    fn get_depth(&self, num: u32, depth: u32) -> Obj {
        if let Some(o) = self.overlay.get(&num) {
            return o.clone();
        }
        if let Some(o) = lock(&self.cache).get(&num) {
            return o.clone();
        }
        if depth > MAX_DEPTH {
            return Obj::Null;
        }
        let o = self.load(num, depth);
        lock(&self.cache).insert(num, o.clone());
        o
    }

    fn load(&self, num: u32, depth: u32) -> Obj {
        match self.xref.get(&num).copied() {
            Some(XRef::Offset(off, _)) => {
                if let Some((n, g, o)) = self.parse_indirect_at(off, depth) {
                    if n == num {
                        return self.unlocked(o, n, g);
                    }
                }
                self.load_scanned(num, depth)
            }
            Some(XRef::Compressed(stm, idx)) => self.load_compressed(num, stm, idx).unwrap_or(Obj::Null),
            None => Obj::Null,
        }
    }

    fn load_scanned(&self, num: u32, depth: u32) -> Obj {
        let table = {
            let mut g = lock(&self.scanned);
            if g.is_none() {
                let s = scan_objects(&self.data);
                *g = Some(Arc::new(s.0.iter().map(|(&n, &(o, _))| (n, o)).collect()));
            }
            g.clone().unwrap_or_default()
        };
        match table.get(&num) {
            Some(&off) => match self.parse_indirect_at(off, depth) {
                Some((n, g, o)) if n == num => self.unlocked(o, n, g),
                _ => Obj::Null,
            },
            None => Obj::Null,
        }
    }

    fn objstm(&self, stm: u32) -> Option<Arc<ObjStm>> {
        if let Some(v) = lock(&self.objstms).get(&stm) {
            return v.clone();
        }
        if !lock(&self.loading).insert(stm) {
            return None;
        }
        let parsed = (|| {
            let o = self.get(stm);
            let s = o.as_stream()?;
            let n = s.dict.get("N").and_then(Obj::as_i64)?.clamp(0, 1_000_000) as usize;
            let first = s.dict.get("First").and_then(Obj::as_i64)?.max(0) as usize;
            let data = self.decode_stream(s, 1).ok()?.data;
            let mut lx = Lexer::new(&data, 0);
            let mut entries = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                let (Some(num), Some(off)) = (lx.uint(), lx.uint()) else { break };
                if num <= u32::MAX as u64 {
                    entries.push((num as u32, off as usize));
                }
            }
            Some(Arc::new(ObjStm { data, first, entries }))
        })();
        lock(&self.loading).remove(&stm);
        lock(&self.objstms).insert(stm, parsed.clone());
        parsed
    }

    fn load_compressed(&self, num: u32, stm: u32, idx: u32) -> Option<Obj> {
        if stm == num {
            return None;
        }
        let s = self.objstm(stm)?;
        let pos = match s.entries.get(idx as usize) {
            Some(&(n, off)) if n == num => off,
            _ => s.entries.iter().find(|(n, _)| *n == num)?.1,
        };
        let mut lx = Lexer::new(&s.data, s.first.checked_add(pos)?);
        match lx.parse() {
            Parsed::Obj(o) => Some(o),
            _ => None,
        }
    }

    /// The object number and generation at `off` when it starts `n g obj`.
    fn obj_header_at(&self, off: usize) -> Option<(u32, u16)> {
        let mut lx = Lexer::new(&self.data, off);
        let n = lx.uint()?;
        let g = lx.uint()?;
        match lx.next() {
            Tok::Keyword(k) if k == b"obj" => Some((n.min(u32::MAX as u64) as u32, g.min(65535) as u16)),
            _ => None,
        }
    }

    /// Parse `n g obj ... endobj` at `off`.
    pub fn parse_indirect_at(&self, off: usize, depth: u32) -> Option<(u32, u16, Obj)> {
        if off >= self.data.len() {
            return None;
        }
        let (num, gen) = self.obj_header_at(off)?;
        let mut lx = Lexer::new(&self.data, off);
        lx.uint();
        lx.uint();
        lx.next();
        let obj = match lx.parse() {
            Parsed::Obj(o) => o,
            Parsed::Keyword(k) if k == b"endobj" => Obj::Null,
            _ => return Some((num, gen, Obj::Null)),
        };
        if let Obj::Dict(d) = &obj {
            if let Tok::Keyword(k) = lx.next() {
                if k == b"stream" {
                    let data = self.stream_bytes(lx.pos, d, depth);
                    return Some((num, gen, Obj::Stream(Arc::new(Stream { dict: d.clone(), data }))));
                }
            }
        }
        Some((num, gen, obj))
    }

    /// A stream's raw bytes starting just after the `stream` keyword: by /Length when that lands on
    /// `endstream`, else up to the next `endstream`.
    fn stream_bytes(&self, after_kw: usize, d: &Dict, depth: u32) -> Vec<u8> {
        let data = &self.data;
        let mut p = after_kw;
        while p < data.len() && (data[p] == b' ' || data[p] == b'\t') {
            p += 1;
        }
        if data.get(p) == Some(&b'\r') {
            p += 1;
        }
        if data.get(p) == Some(&b'\n') {
            p += 1;
        }
        let len = match d.get("Length") {
            Some(Obj::Ref(n, _)) if depth < 4 => self.get_depth(*n, depth + 1).as_i64(),
            Some(o) => o.as_i64(),
            None => None,
        };
        if let Some(len) = len {
            if len >= 0 {
                let end = p.saturating_add(len as usize);
                if end <= data.len() {
                    let mut q = end;
                    while q < data.len() && is_white(data[q]) {
                        q += 1;
                    }
                    if data[q..].starts_with(b"endstream") {
                        return data[p..end].to_vec();
                    }
                }
            }
        }
        let end = find(data, b"endstream", p).unwrap_or(data.len());
        let mut e = end;
        if e > p && data[e - 1] == b'\n' {
            e -= 1;
        }
        if e > p && data[e - 1] == b'\r' {
            e -= 1;
        }
        data[p..e.max(p)].to_vec()
    }

    /// Follow references (a chain of at most a few hops) to the object itself.
    pub fn resolve(&self, o: &Obj) -> Obj {
        let mut cur = o.clone();
        for _ in 0..8 {
            match cur {
                Obj::Ref(n, _) => cur = self.get(n),
                other => return other,
            }
        }
        Obj::Null
    }

    /// `d[key]`, resolved.
    pub fn dget(&self, d: &Dict, key: &str) -> Obj {
        match d.get(key) {
            Some(o) => self.resolve(o),
            None => Obj::Null,
        }
    }

    /// `d[key]` resolved to a dictionary (a stream's dictionary counts).
    pub fn dict_of(&self, d: &Dict, key: &str) -> Option<Dict> {
        self.dget(d, key).as_dict().cloned()
    }

    /// A stream's filters with their (resolved) parameters.
    pub fn filters_of(&self, d: &Dict) -> Vec<(String, Dict)> {
        let f = self.resolve(d.get("Filter").or_else(|| d.get("F")).unwrap_or(&Obj::Null));
        let p = self.resolve(d.get("DecodeParms").or_else(|| d.get("DP")).unwrap_or(&Obj::Null));
        let names: Vec<String> = match &f {
            Obj::Name(n) => vec![n.clone()],
            Obj::Arr(a) => a.iter().filter_map(|v| self.resolve(v).as_name().map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        let parms: Vec<Dict> = match &p {
            Obj::Dict(d) => vec![d.clone()],
            Obj::Arr(a) => a.iter().map(|v| self.resolve(v).as_dict().cloned().unwrap_or_default()).collect(),
            _ => Vec::new(),
        };
        names.into_iter().enumerate().map(|(i, n)| (n, parms.get(i).cloned().unwrap_or_default())).collect()
    }

    /// Run a stream's filters.
    pub fn decode_stream(&self, s: &Stream, _depth: u32) -> Result<Decoded, String> {
        let f = self.filters_of(&s.dict);
        filters::decode(&s.data, &f)
    }

    /// A stream's decoded bytes, or empty when it cannot be decoded (image codecs give their raw bytes).
    pub fn stream_data(&self, s: &Stream) -> Vec<u8> {
        self.decode_stream(s, 0).map(|d| d.data).unwrap_or_default()
    }
}

/// Scan for `n g obj` (offsets of the object number; later definitions win) and parse every
/// `trailer` dictionary in file order.
pub fn scan_objects(data: &[u8]) -> (HashMap<u32, (usize, u16)>, Vec<Dict>) {
    let mut objs = HashMap::new();
    let mut trailers = Vec::new();
    let mut i = 0;
    while let Some(p) = find(data, b"obj", i) {
        i = p + 3;
        // Not "endobj", not "objstm": the keyword must stand alone.
        if data.get(p + 3).map_or(false, |&c| is_regular(c)) {
            continue;
        }
        if p == 0 || !is_white(data[p - 1]) {
            continue;
        }
        let mut q = p;
        while q > 0 && is_white(data[q - 1]) {
            q -= 1;
        }
        let gen_end = q;
        while q > 0 && data[q - 1].is_ascii_digit() {
            q -= 1;
        }
        if q == gen_end || gen_end - q > 5 {
            continue;
        }
        let gen_start = q;
        if q == 0 || !is_white(data[q - 1]) {
            continue;
        }
        while q > 0 && is_white(data[q - 1]) {
            q -= 1;
        }
        let num_end = q;
        while q > 0 && data[q - 1].is_ascii_digit() {
            q -= 1;
        }
        if q == num_end || num_end - q > 10 {
            continue;
        }
        if q > 0 && is_regular(data[q - 1]) {
            continue;
        }
        let num: u64 = std::str::from_utf8(&data[q..num_end]).ok().and_then(|s| s.parse().ok()).unwrap_or(u64::MAX);
        let gen: u64 = std::str::from_utf8(&data[gen_start..gen_end]).ok().and_then(|s| s.parse().ok()).unwrap_or(u64::MAX);
        if num > u32::MAX as u64 || gen > 65535 {
            continue;
        }
        objs.insert(num as u32, (q, gen as u16));
    }
    let mut i = 0;
    while let Some(p) = find(data, b"trailer", i) {
        i = p + 7;
        let mut lx = Lexer::new(data, p + 7);
        if let Parsed::Obj(Obj::Dict(d)) = lx.parse() {
            trailers.push(d);
        }
    }
    (objs, trailers)
}
