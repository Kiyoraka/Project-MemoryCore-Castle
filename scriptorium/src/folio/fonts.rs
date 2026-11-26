//! PDF fonts: simple fonts (Type1, TrueType, the standard 14), composite fonts (Type0 over
//! CIDFontType0/CFF and CIDFontType2/TrueType, Identity-H/V and embedded CMaps, CIDToGIDMap) and
//! Type 3 fonts (glyphs as content streams). Each one answers, for a string shown on the page, the
//! glyphs to draw, their advances, and the text they stand for.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use super::afm::Std14;
use super::cff;
use super::cmap::CMap;
use super::encoding::{glyph_to_unicode, named_encoding, standard_encoding, Encoding};
use super::obj::{to_bytes, Dict, Obj};
use super::parse::PdfFile;
use crate::ground::display::Matrix;
use crate::ground::font::{Font, FontIndex};

/// The installed fonts, indexed once per process (only when a PDF needs a font it does not embed).
pub fn system_index() -> &'static FontIndex {
    static IDX: OnceLock<FontIndex> = OnceLock::new();
    IDX.get_or_init(FontIndex::system)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Simple,
    Composite,
    Type3,
}

/// A Type 3 font's own drawing: glyph procedures run through the font matrix.
#[derive(Debug, Clone)]
pub struct Type3 {
    pub matrix: Matrix,
    pub procs: Dict,
    pub resources: Option<Dict>,
    pub names: Vec<Option<String>>,
}

/// One shown character.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub code: u32,
    /// The glyph id in the face (the code itself for Type 3).
    pub gid: u16,
    /// Advance in text space for a font size of 1.
    pub width: f64,
    pub text: String,
    /// A single-byte code 32 (word spacing applies).
    pub word_space: bool,
}

#[derive(Debug)]
pub struct PdfFont {
    /// The display font id (in `display::Resources`).
    pub id: u32,
    /// The font's PostScript name, subset prefix removed.
    #[allow(dead_code)]
    pub base: String,
    pub kind: Kind,
    pub face: Option<Arc<Font>>,
    pub embedded: bool,
    pub upem: f64,
    simple_gid: Vec<u16>,
    simple_uni: Vec<Option<String>>,
    simple_w: Vec<f64>,
    enc: CMap,
    ucs2: bool,
    cid_gid: Option<Vec<u16>>,
    cff_cid: Option<HashMap<u32, u16>>,
    cid_w: HashMap<u32, f64>,
    dw: f64,
    tounicode: Option<CMap>,
    reverse: HashMap<u16, String>,
    pub type3: Option<Type3>,
    pub vertical: bool,
}

pub fn strip_subset(name: &str) -> &str {
    let b = name.as_bytes();
    if b.len() > 7 && b[6] == b'+' && b[..6].iter().all(|c| c.is_ascii_uppercase()) {
        &name[7..]
    } else {
        name
    }
}

/// Find an installed face standing in for a font the PDF does not embed.
pub fn system_face(base: &str, flags: i64) -> Option<Arc<Font>> {
    let idx = system_index();
    let base = strip_subset(base);
    let std = Std14::from_name(base);
    let lower = base.to_ascii_lowercase();
    let bold = lower.contains("bold") || lower.contains("black") || lower.contains("heavy") || flags & (1 << 18) != 0;
    let italic = lower.contains("italic") || lower.contains("oblique") || flags & 64 != 0;
    let weight = if bold { 700 } else { 400 };
    let mut fams: Vec<String> = Vec::new();
    if let Some(s) = std {
        fams.extend(s.families().iter().map(|f| f.to_string()));
    }
    let family_part = base.split([',', '-']).next().unwrap_or(base);
    let trimmed = family_part.trim_end_matches("PSMT").trim_end_matches("MT").trim_end_matches("PS");
    fams.push(family_part.to_string());
    fams.push(camel_split(trimmed));
    fams.push(base.to_string());
    let fallback = if flags & 1 != 0 {
        Std14::Courier { bold, italic }
    } else if flags & 2 != 0 {
        Std14::Times { bold, italic }
    } else {
        Std14::Helvetica { bold, italic }
    };
    fams.extend(fallback.families().iter().map(|f| f.to_string()));
    fams.extend(["Segoe UI", "DejaVu Sans", "Noto Sans"].iter().map(|s| s.to_string()));
    for fam in fams {
        if fam.is_empty() {
            continue;
        }
        if let Some(r) = idx.find(&fam, weight, italic) {
            if let Some(f) = idx.load(&r) {
                return Some(f);
            }
        }
    }
    idx.faces().first().and_then(|f| idx.load(&f.font))
}

fn camel_split(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() && chars[i - 1].is_lowercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// The embedded font program, if any: the face and, for bare CFF, what it says about names.
fn embedded_face(file: &PdfFile, desc: Option<&Dict>) -> (Option<Arc<Font>>, Option<cff::CffInfo>) {
    let Some(desc) = desc else { return (None, None) };
    if let Some(s) = file.dget(desc, "FontFile2").as_stream() {
        let data = file.stream_data(s);
        return (Font::parse(data, 0).ok().map(Arc::new), None);
    }
    if let Some(s) = file.dget(desc, "FontFile3").as_stream() {
        let data = file.stream_data(s);
        if data.starts_with(b"OTTO") || data.starts_with(&[0, 1, 0, 0]) || data.starts_with(b"true") {
            return (Font::parse(data, 0).ok().map(Arc::new), None);
        }
        if let Some(info) = cff::parse(&data) {
            let otf = cff::wrap_otf(&data, info.num_glyphs, info.upem);
            return (Font::parse(otf, 0).ok().map(Arc::new), Some(info));
        }
    }
    (None, None)
}

fn read_widths(file: &PdfFile, d: &Dict) -> Option<(i64, Vec<f64>)> {
    let w = file.dget(d, "Widths");
    let arr = w.as_arr()?;
    let first = file.dget(d, "FirstChar").as_i64().unwrap_or(0);
    Some((first, arr.iter().map(|v| file.resolve(v).as_f64().unwrap_or(0.0)).collect()))
}

fn tounicode(file: &PdfFile, d: &Dict) -> Option<CMap> {
    match file.dget(d, "ToUnicode") {
        Obj::Stream(s) => Some(CMap::parse(&file.stream_data(&s))),
        _ => None,
    }
}

impl PdfFont {
    fn blank(id: u32, base: String, kind: Kind) -> PdfFont {
        PdfFont {
            id,
            base,
            kind,
            face: None,
            embedded: false,
            upem: 1000.0,
            simple_gid: vec![0; 256],
            simple_uni: vec![None; 256],
            simple_w: vec![0.0; 256],
            enc: CMap::default(),
            ucs2: false,
            cid_gid: None,
            cff_cid: None,
            cid_w: HashMap::new(),
            dw: 1000.0,
            tounicode: None,
            reverse: HashMap::new(),
            type3: None,
            vertical: false,
        }
    }

    /// Load a font dictionary. Never fails: an unreadable font becomes a Helvetica stand-in.
    pub fn load(file: &PdfFile, d: &Dict, id: u32) -> PdfFont {
        match d.name("Subtype") {
            Some("Type0") => PdfFont::load_composite(file, d, id),
            Some("Type3") => PdfFont::load_type3(file, d, id),
            _ => PdfFont::load_simple(file, d, id),
        }
    }

    fn encoding_names(file: &PdfFile, d: &Dict) -> (Option<Encoding>, Vec<(usize, String)>, bool) {
        let mut diffs = Vec::new();
        let mut base = None;
        let mut has = false;
        match file.dget(d, "Encoding") {
            Obj::Name(n) => {
                base = named_encoding(&n);
                has = true;
            }
            Obj::Dict(e) => {
                has = true;
                if let Some(n) = file.dget(&e, "BaseEncoding").as_name() {
                    base = named_encoding(n);
                }
                if let Obj::Arr(a) = file.dget(&e, "Differences") {
                    let mut code = 0i64;
                    for v in a {
                        match file.resolve(&v) {
                            Obj::Int(i) => code = i,
                            Obj::Real(r) => code = r as i64,
                            Obj::Name(n) => {
                                if (0..256).contains(&code) {
                                    diffs.push((code as usize, n));
                                }
                                code += 1;
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
        (base, diffs, has)
    }

    fn load_simple(file: &PdfFile, d: &Dict, id: u32) -> PdfFont {
        let base = strip_subset(d.name("BaseFont").unwrap_or("Helvetica")).to_string();
        let mut f = PdfFont::blank(id, base.clone(), Kind::Simple);
        let desc = file.dict_of(d, "FontDescriptor");
        let flags = desc.as_ref().and_then(|x| file.dget(x, "Flags").as_i64()).unwrap_or(32);
        let std = Std14::from_name(&base);
        let symbolic = (flags & 4 != 0 && flags & 32 == 0) || std.map_or(false, |s| s.is_symbolic());
        let truetype = d.name("Subtype") == Some("TrueType");
        let (face, cffi) = embedded_face(file, desc.as_ref());
        f.embedded = face.is_some();
        f.face = face.or_else(|| system_face(&base, flags));
        f.upem = f.face.as_ref().map_or(1000.0, |x| x.units_per_em() as f64);
        f.tounicode = tounicode(file, d);
        let (base_enc, diffs, has_enc) = PdfFont::encoding_names(file, d);
        let mut names: Vec<Option<String>> = vec![None; 256];
        let default_enc = if base_enc.is_some() {
            base_enc
        } else if cffi.is_some() || symbolic || (truetype && f.embedded) {
            None
        } else {
            Some(standard_encoding())
        };
        if let Some(e) = default_enc {
            for (i, n) in e.iter().enumerate() {
                names[i] = n.map(str::to_string);
            }
        }
        for (c, n) in diffs {
            names[c] = Some(n);
        }
        let widths = read_widths(file, d);
        let missing = desc.as_ref().and_then(|x| file.dget(x, "MissingWidth").as_f64()).unwrap_or(0.0);
        let face = f.face.clone();
        let mappings_empty = face.as_ref().map_or(true, |x| x.mappings().is_empty());
        for code in 0..256usize {
            let name = names[code].as_deref();
            // Glyph id.
            let mut gid: Option<u16> = None;
            if let Some(info) = &cffi {
                if let Some(n) = name {
                    gid = info.gid_for_name(n);
                }
                if gid.is_none() || (!has_enc && info.builtin[code] != 0) {
                    let b = info.builtin[code];
                    if b != 0 {
                        gid = Some(b);
                    }
                }
            } else if let Some(face) = &face {
                let sym_first = std.map_or(false, |s| s.is_symbolic()) && !f.embedded;
                if sym_first {
                    gid = face.glyph_id(char::from_u32(0xF000 + code as u32).unwrap_or('\0')).or_else(|| face.glyph_id(code as u8 as char));
                }
                if gid.is_none() {
                    if let Some(u) = name.and_then(glyph_to_unicode) {
                        if let Some(c) = u.chars().next() {
                            gid = face.glyph_id(c);
                        }
                    }
                }
                if gid.is_none() && (f.embedded || name.is_none()) {
                    gid = face
                        .glyph_id(char::from_u32(0xF000 + code as u32).unwrap_or('\0'))
                        .or_else(|| face.glyph_id(code as u8 as char));
                }
                if gid.is_none() && f.embedded && mappings_empty && truetype {
                    gid = Some(code as u16);
                }
            }
            f.simple_gid[code] = gid.unwrap_or(0);
            // Text.
            let mut text = f.tounicode.as_ref().and_then(|t| t.unicode.get(&(code as u32)).cloned());
            if text.is_none() {
                text = name.and_then(glyph_to_unicode);
            }
            if text.is_none() && name.is_none() && !symbolic && (32..127).contains(&code) {
                text = Some((code as u8 as char).to_string());
            }
            if text.is_none() && symbolic && !f.embedded && (32..127).contains(&code) && std.is_none() {
                text = Some((code as u8 as char).to_string());
            }
            f.simple_uni[code] = text;
            // Width (1/1000 em).
            let mut w: Option<f64> = None;
            if let Some((first, ws)) = &widths {
                let i = code as i64 - first;
                if i >= 0 && (i as usize) < ws.len() {
                    w = Some(ws[i as usize]);
                } else {
                    w = Some(missing);
                }
            }
            // No /Widths: an embedded font's own advances, then the standard-14 tables, then the
            // stand-in face's advances.
            let face_w = match (&face, gid) {
                (Some(face), Some(g)) if face.advance(g) > 0 => Some(face.advance(g) as f64 * 1000.0 / f.upem.max(1.0)),
                _ => None,
            };
            if w.is_none() && f.embedded {
                w = face_w;
            }
            if w.is_none() {
                if let Some(s) = std {
                    w = Some(s.width(name.unwrap_or("space")));
                }
            }
            if w.is_none() {
                w = face_w;
            }
            f.simple_w[code] = w.unwrap_or(if missing > 0.0 { missing } else { 500.0 });
        }
        f
    }

    fn load_composite(file: &PdfFile, d: &Dict, id: u32) -> PdfFont {
        let base = strip_subset(d.name("BaseFont").unwrap_or("")).to_string();
        let mut f = PdfFont::blank(id, base.clone(), Kind::Composite);
        match file.dget(d, "Encoding") {
            Obj::Name(n) => {
                f.vertical = n.ends_with("-V");
                if n.contains("UCS2") || n.contains("UTF16") {
                    f.ucs2 = true;
                    f.enc = CMap { codespace: vec![(0, 0xFFFF, 2)], ..CMap::default() };
                } else {
                    f.enc = CMap::identity(f.vertical);
                }
            }
            Obj::Stream(s) => {
                f.enc = CMap::parse(&file.stream_data(&s));
                if f.enc.codespace.is_empty() {
                    f.enc.codespace.push((0, 0xFFFF, 2));
                }
                f.vertical = f.enc.vertical;
            }
            _ => f.enc = CMap::identity(false),
        }
        f.tounicode = tounicode(file, d);
        let desc_font = match file.dget(d, "DescendantFonts") {
            Obj::Arr(a) => a.first().map(|x| file.resolve(x)).and_then(|x| x.as_dict().cloned()),
            Obj::Dict(x) => Some(x),
            _ => None,
        }
        .unwrap_or_default();
        f.dw = file.dget(&desc_font, "DW").as_f64().unwrap_or(1000.0);
        if let Obj::Arr(w) = file.dget(&desc_font, "W") {
            let w: Vec<Obj> = w.iter().map(|x| file.resolve(x)).collect();
            let mut i = 0;
            let mut total = 0usize;
            while i < w.len() && total < 1 << 20 {
                let Some(c0) = w[i].as_i64() else {
                    i += 1;
                    continue;
                };
                match w.get(i + 1) {
                    Some(Obj::Arr(list)) => {
                        for (k, v) in list.iter().enumerate() {
                            let cid = c0 + k as i64;
                            if cid >= 0 {
                                f.cid_w.insert(cid as u32, file.resolve(v).as_f64().unwrap_or(f.dw));
                                total += 1;
                            }
                        }
                        i += 2;
                    }
                    Some(c1) => {
                        let c1 = c1.as_i64().unwrap_or(c0);
                        let wv = w.get(i + 2).and_then(Obj::as_f64).unwrap_or(f.dw);
                        if c1 >= c0 && c0 >= 0 && c1 - c0 < 65536 {
                            for cid in c0..=c1 {
                                f.cid_w.insert(cid as u32, wv);
                                total += 1;
                            }
                        }
                        i += 3;
                    }
                    None => break,
                }
            }
        }
        match file.dget(&desc_font, "CIDToGIDMap") {
            Obj::Stream(s) => {
                let data = file.stream_data(&s);
                f.cid_gid = Some(data.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect());
            }
            _ => {}
        }
        let desc = file.dict_of(&desc_font, "FontDescriptor");
        let flags = desc.as_ref().and_then(|x| file.dget(x, "Flags").as_i64()).unwrap_or(32);
        let (face, cffi) = embedded_face(file, desc.as_ref());
        f.embedded = face.is_some();
        if let Some(info) = &cffi {
            f.cff_cid = info.cid_map();
        }
        let base_for_system = if base.is_empty() { desc_font.name("BaseFont").unwrap_or("").to_string() } else { base.clone() };
        f.face = face.or_else(|| system_face(&base_for_system, flags));
        f.upem = f.face.as_ref().map_or(1000.0, |x| x.units_per_em() as f64);
        if f.tounicode.is_none() && f.embedded {
            if let Some(face) = &f.face {
                for (c, g) in face.mappings() {
                    f.reverse.entry(g).or_insert_with(|| c.to_string());
                }
            }
        }
        f
    }

    fn load_type3(file: &PdfFile, d: &Dict, id: u32) -> PdfFont {
        let mut f = PdfFont::blank(id, d.name("Name").unwrap_or("Type3").to_string(), Kind::Type3);
        let m = file.dget(d, "FontMatrix").nums();
        let matrix = if m.len() >= 6 { Matrix::new(m[0], m[1], m[2], m[3], m[4], m[5]) } else { Matrix::new(0.001, 0.0, 0.0, 0.001, 0.0, 0.0) };
        let procs = file.dict_of(d, "CharProcs").unwrap_or_default();
        let resources = file.dict_of(d, "Resources");
        let (_, diffs, _) = PdfFont::encoding_names(file, d);
        let mut names: Vec<Option<String>> = vec![None; 256];
        for (c, n) in diffs {
            names[c] = Some(n);
        }
        f.tounicode = tounicode(file, d);
        let widths = read_widths(file, d);
        for code in 0..256usize {
            let w = widths.as_ref().and_then(|(first, ws)| {
                let i = code as i64 - first;
                if i >= 0 {
                    ws.get(i as usize).copied()
                } else {
                    None
                }
            });
            // Type 3 widths are in glyph space: through the font matrix, then x1000 like the others.
            f.simple_w[code] = w.unwrap_or(0.0) * matrix.a * 1000.0;
            f.simple_gid[code] = code as u16;
            let text = f.tounicode.as_ref().and_then(|t| t.unicode.get(&(code as u32)).cloned()).or_else(|| names[code].as_deref().and_then(glyph_to_unicode));
            f.simple_uni[code] = text;
        }
        f.type3 = Some(Type3 { matrix, procs, resources, names });
        f
    }

    /// The characters of a shown string.
    pub fn decode(&self, s: &[u8]) -> Vec<Glyph> {
        let mut out = Vec::with_capacity(s.len());
        match self.kind {
            Kind::Simple | Kind::Type3 => {
                for &c in s {
                    let i = c as usize;
                    out.push(Glyph {
                        code: c as u32,
                        gid: self.simple_gid[i],
                        width: self.simple_w[i] / 1000.0,
                        text: self.simple_uni[i].clone().unwrap_or_default(),
                        word_space: c == 32,
                    });
                }
            }
            Kind::Composite => {
                let mut i = 0;
                while i < s.len() {
                    let (code, n) = self.enc.next_code(s, i, 2);
                    i += n;
                    let cid = if self.ucs2 { code } else { self.enc.cid(code) };
                    let mut text = self.tounicode.as_ref().and_then(|t| t.unicode.get(&code).cloned());
                    if text.is_none() && self.ucs2 {
                        text = char::from_u32(code).map(|c| c.to_string());
                    }
                    let gid: u16 = if self.embedded {
                        if let Some(m) = &self.cff_cid {
                            m.get(&cid).copied().unwrap_or(0)
                        } else if let Some(m) = &self.cid_gid {
                            m.get(cid as usize).copied().unwrap_or(0)
                        } else {
                            cid.min(65535) as u16
                        }
                    } else {
                        let ch = text.as_ref().and_then(|t| t.chars().next());
                        ch.and_then(|c| self.face.as_ref().and_then(|f| f.glyph_id(c))).unwrap_or(0)
                    };
                    if text.is_none() {
                        text = self.reverse.get(&gid).cloned();
                    }
                    out.push(Glyph {
                        code,
                        gid,
                        width: self.cid_w.get(&cid).copied().unwrap_or(self.dw) / 1000.0,
                        text: text.unwrap_or_default(),
                        word_space: n == 1 && code == 32,
                    });
                }
            }
        }
        out
    }

    /// The Type 3 glyph procedure for a code.
    pub fn type3_proc(&self, code: u32) -> Option<&Obj> {
        let t = self.type3.as_ref()?;
        let name = t.names.get(code as usize)?.as_deref()?;
        t.procs.get(name)
    }
}

/// Fonts by their PDF object, each loaded once per document and given a display font id.
#[derive(Debug, Default)]
pub struct FontCache {
    map: HashMap<String, Arc<PdfFont>>,
    next_id: u32,
}

impl FontCache {
    pub fn new() -> FontCache {
        FontCache { map: HashMap::new(), next_id: 1 }
    }

    pub fn get(&mut self, file: &PdfFile, o: &Obj) -> Option<Arc<PdfFont>> {
        let key = match o {
            Obj::Ref(n, g) => format!("{} {}", n, g),
            other => format!("d{}", String::from_utf8_lossy(&to_bytes(other))),
        };
        if let Some(f) = self.map.get(&key) {
            return Some(f.clone());
        }
        let d = file.resolve(o);
        let d = d.as_dict()?;
        if self.next_id == 0 {
            self.next_id = 1;
        }
        let f = Arc::new(PdfFont::load(file, d, self.next_id));
        self.next_id += 1;
        self.map.insert(key, f.clone());
        Some(f)
    }
}
