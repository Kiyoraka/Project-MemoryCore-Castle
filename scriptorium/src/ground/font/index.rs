//! The system font index: scans font directories reading only each face's table directory and its
//! `name`/`OS/2`/`head` tables, then finds faces by family + weight + italic and picks a fallback
//! face covering a character (cmap tables read lazily, one per face, and cached).

use super::cmap;
use super::names::{all, parse_names, parse_style};
use super::read::{face_offset, rd16, rd32, table_records, TableRec, TTCF};
use super::{Font, Style};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// Where a face lives: the file and the face index inside it (0 unless a collection).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontRef {
    pub path: PathBuf,
    pub index: u32,
}

impl FontRef {
    /// Read the file and parse the face.
    pub fn load(&self) -> Option<Font> {
        let data = std::fs::read(&self.path).ok()?;
        Font::parse(data, self.index).ok()
    }
}

/// One indexed face.
#[derive(Debug, Clone)]
pub struct FaceInfo {
    /// Name ID 1.
    pub family: String,
    /// Name ID 16, or empty.
    pub typographic_family: String,
    pub subfamily: String,
    pub full_name: String,
    pub postscript_name: String,
    /// Family names in every language the font carries (IDs 1 and 16), e.g. "ＭＳ ゴシック".
    pub aliases: Vec<String>,
    pub style: Style,
    pub font: FontRef,
}

impl FaceInfo {
    pub fn preferred_family(&self) -> &str {
        if self.typographic_family.is_empty() {
            &self.family
        } else {
            &self.typographic_family
        }
    }
}

struct Coverage {
    table: Vec<u8>,
    sub: cmap::Sub,
}

/// An index of installed fonts. Cheap to query; `Sync` (caches sit behind mutexes).
pub struct FontIndex {
    faces: Vec<FaceInfo>,
    coverage: Mutex<HashMap<FontRef, Option<Arc<Coverage>>>>,
    loaded: Mutex<HashMap<FontRef, Option<Arc<Font>>>>,
}

impl Default for FontIndex {
    fn default() -> Self {
        FontIndex::new()
    }
}

impl std::fmt::Debug for FontIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontIndex").field("faces", &self.faces.len()).finish()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Families tried after the caller's preferences, by script of the character.
fn default_families(ch: char) -> &'static [&'static str] {
    let c = ch as u32;
    const LATIN: &[&str] = &[
        "Segoe UI", "Arial", "Calibri", "Times New Roman", "Helvetica", "Helvetica Neue", "DejaVu Sans",
        "Liberation Sans", "Noto Sans", "Segoe UI Symbol", "Segoe UI Emoji", "Arial Unicode MS",
        "Microsoft YaHei", "Yu Gothic", "MS Gothic", "SimSun", "Malgun Gothic", "Noto Sans CJK SC",
        "Noto Sans CJK JP", "Noto Sans CJK KR",
    ];
    const HAN: &[&str] = &[
        "Microsoft YaHei", "SimSun", "Yu Gothic", "MS Gothic", "Microsoft JhengHei", "Malgun Gothic",
        "Noto Sans CJK SC", "Noto Sans CJK TC", "Noto Sans CJK JP", "Noto Sans SC", "Source Han Sans SC",
        "PingFang SC", "Hiragino Sans", "Hiragino Sans GB", "WenQuanYi Micro Hei", "SimSun-ExtB",
        "SimSun-ExtG", "Arial Unicode MS",
    ];
    const KANA: &[&str] = &[
        "Yu Gothic", "MS Gothic", "Meiryo", "Noto Sans CJK JP", "Noto Sans JP", "Hiragino Sans",
        "Microsoft YaHei", "SimSun", "Arial Unicode MS",
    ];
    const HANGUL: &[&str] =
        &["Malgun Gothic", "Gulim", "Noto Sans CJK KR", "Noto Sans KR", "Apple SD Gothic Neo", "Arial Unicode MS"];
    const SYMBOL: &[&str] = &[
        "Segoe UI Emoji", "Segoe UI Symbol", "Apple Color Emoji", "Noto Color Emoji", "Noto Emoji",
        "Noto Sans Symbols", "Noto Sans Symbols 2", "DejaVu Sans", "Arial Unicode MS", "Segoe UI",
    ];
    const INDIC_SEA: &[&str] = &[
        "Nirmala UI", "Leelawadee UI", "Mangal", "Noto Sans Devanagari", "Noto Sans Thai", "Tahoma",
        "Arial Unicode MS", "Segoe UI",
    ];
    match c {
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => HANGUL,
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF65..=0xFF9F => KANA,
        0x2E80..=0x2FFF | 0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
        | 0xFF00..=0xFF64 | 0x20000..=0x3FFFF => HAN,
        0x0900..=0x0DFF | 0x0E00..=0x0EFF | 0x1000..=0x109F | 0x1780..=0x17FF => INDIC_SEA,
        0x2190..=0x2BFF | 0x1F000..=0x1FAFF => SYMBOL,
        _ => LATIN,
    }
}

fn read_at(f: &mut File, off: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut v = vec![0u8; len];
    f.read_exact(&mut v).ok()?;
    Some(v)
}

/// The table records of every face in a file, read with a few small reads.
fn faces_of(f: &mut File) -> Vec<(u32, Vec<TableRec>)> {
    let mut out = Vec::new();
    let Some(hdr) = read_at(f, 0, 12) else { return out };
    let n = if rd32(&hdr, 0) == Some(TTCF) { rd32(&hdr, 8).unwrap_or(0).min(256) } else { 1 };
    let ttc_hdr = if n > 1 || rd32(&hdr, 0) == Some(TTCF) {
        match read_at(f, 0, 12 + 4 * n as usize) {
            Some(h) => h,
            None => return out,
        }
    } else {
        hdr
    };
    for i in 0..n {
        let Ok(off) = face_offset(&ttc_hdr, i) else { continue };
        let Some(h) = read_at(f, off as u64, 12) else { continue };
        let Some(nt) = rd16(&h, 4) else { continue };
        let Some(dir) = read_at(f, off as u64, 12 + 16 * nt as usize) else { continue };
        if let Some(recs) = table_records(&dir) {
            out.push((i, recs));
        }
    }
    out
}

fn read_table(f: &mut File, file_len: u64, recs: &[TableRec], tag: &[u8; 4], max: usize) -> Option<Vec<u8>> {
    let r = recs.iter().find(|r| &r.tag == tag)?;
    let off = r.off as u64;
    if off >= file_len {
        return None;
    }
    let len = (r.len as u64).min(file_len - off) as usize;
    if len > max {
        return None;
    }
    read_at(f, off, len)
}

fn is_font_file(p: &Path) -> bool {
    match p.extension().and_then(|e| e.to_str()) {
        Some(e) => matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc"),
        None => false,
    }
}

impl FontIndex {
    /// An empty index.
    pub fn new() -> FontIndex {
        FontIndex { faces: Vec::new(), coverage: Mutex::new(HashMap::new()), loaded: Mutex::new(HashMap::new()) }
    }

    /// Index every font in this platform's system and user font directories.
    pub fn system() -> FontIndex {
        let mut idx = FontIndex::new();
        for d in FontIndex::system_dirs() {
            idx.scan_dir(&d);
        }
        idx
    }

    /// The platform's font directories (those that exist or not - scanning skips missing ones).
    pub fn system_dirs() -> Vec<PathBuf> {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let mut dirs = Vec::new();
        if cfg!(windows) {
            dirs.push(env("WINDIR").or_else(|| env("SystemRoot")).unwrap_or_else(|| PathBuf::from("C:\\Windows")).join("Fonts"));
            if let Some(l) = env("LOCALAPPDATA") {
                dirs.push(l.join("Microsoft").join("Windows").join("Fonts"));
            }
        } else if cfg!(target_os = "macos") {
            dirs.push(PathBuf::from("/System/Library/Fonts"));
            dirs.push(PathBuf::from("/Library/Fonts"));
            if let Some(h) = env("HOME") {
                dirs.push(h.join("Library").join("Fonts"));
            }
        } else {
            dirs.push(PathBuf::from("/usr/share/fonts"));
            dirs.push(PathBuf::from("/usr/local/share/fonts"));
            if let Some(h) = env("HOME") {
                dirs.push(h.join(".local").join("share").join("fonts"));
                dirs.push(h.join(".fonts"));
            }
        }
        dirs
    }

    /// Index the fonts in a directory (recursively, a few levels deep).
    pub fn scan_dir(&mut self, dir: &Path) {
        self.scan_rec(dir, 0);
    }

    fn scan_rec(&mut self, dir: &Path, depth: u32) {
        if depth > 6 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                self.scan_rec(&p, depth + 1);
            } else if is_font_file(&p) {
                self.add_file(&p);
            }
        }
    }

    /// Index every face of one font file; returns how many faces were added.
    pub fn add_file(&mut self, path: &Path) -> usize {
        let Ok(mut f) = File::open(path) else { return 0 };
        let file_len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let mut added = 0;
        for (index, recs) in faces_of(&mut f) {
            let name = read_table(&mut f, file_len, &recs, b"name", 4 << 20);
            let os2 = read_table(&mut f, file_len, &recs, b"OS/2", 4096);
            let head = read_table(&mut f, file_len, &recs, b"head", 4096);
            let names = parse_names(name.as_deref());
            if names.family.is_empty() {
                continue;
            }
            let style = parse_style(os2.as_deref(), head.as_deref(), &names.subfamily);
            let mut aliases = Vec::new();
            if let Some(n) = name.as_deref() {
                for id in [1u16, 16, 21] {
                    for s in all(n, id) {
                        if !aliases.contains(&s) {
                            aliases.push(s);
                        }
                    }
                }
            }
            self.faces.push(FaceInfo {
                family: names.family,
                typographic_family: names.typographic_family,
                subfamily: names.subfamily,
                full_name: names.full_name,
                postscript_name: names.postscript_name,
                aliases,
                style,
                font: FontRef { path: path.to_path_buf(), index },
            });
            added += 1;
        }
        added
    }

    pub fn faces(&self) -> &[FaceInfo] {
        &self.faces
    }

    /// Family names for a font menu (typographic families, sorted, unique).
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = self.faces.iter().map(|f| f.preferred_family().to_string()).collect();
        v.sort_by_key(|s| s.to_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        v
    }

    /// The face of `family` nearest to `weight` (100..=900) and `italic`. The family is matched
    /// case-insensitively against typographic family, family, any localized family name, and full
    /// name (an exact full-name match such as "Arial Bold" wins outright).
    pub fn find(&self, family: &str, weight: u16, italic: bool) -> Option<FontRef> {
        let want = family.trim().to_lowercase();
        if want.is_empty() {
            return None;
        }
        let mut best: Option<(&FaceInfo, i64)> = None;
        for f in &self.faces {
            let fam_match = f.typographic_family.to_lowercase() == want
                || f.family.to_lowercase() == want
                || f.aliases.iter().any(|a| a.to_lowercase() == want);
            let full_match = f.full_name.to_lowercase() == want || f.postscript_name.to_lowercase() == want;
            if !fam_match && !full_match {
                continue;
            }
            let mut score: i64 = 0;
            if full_match && !fam_match {
                score -= 1_000_000;
            }
            if f.style.italic != italic {
                score += 10_000;
            }
            score += (f.style.stretch as i64 - 5).abs() * 1_000;
            let w = f.style.weight as i64;
            let want_w = weight.clamp(1, 1000) as i64;
            let mut wd = (w - want_w).abs() * 2;
            // CSS-like tie breaks: heavy requests prefer heavier faces, light ones lighter.
            if want_w > 500 && w < want_w {
                wd += 1;
            } else if want_w < 400 && w > want_w {
                wd += 1;
            } else if (400..=500).contains(&want_w) && w < want_w {
                wd += 1;
            }
            score += wd;
            // Prefer the face whose own family name is the request (Arial over a typographic group).
            if f.family.to_lowercase() != want {
                score += 1;
            }
            if best.map_or(true, |(_, s)| score < s) {
                best = Some((f, score));
            }
        }
        best.map(|(f, _)| f.font.clone())
    }

    /// Load (and cache) a face.
    pub fn load(&self, r: &FontRef) -> Option<Arc<Font>> {
        if let Some(v) = lock(&self.loaded).get(r) {
            return v.clone();
        }
        let font = r.load().map(Arc::new);
        lock(&self.loaded).insert(r.clone(), font.clone());
        font
    }

    /// Whether a face maps the character (reads only its cmap table, cached).
    pub fn covers(&self, r: &FontRef, ch: char) -> bool {
        let cov = {
            let cache = lock(&self.coverage);
            cache.get(r).cloned()
        };
        let cov = match cov {
            Some(c) => c,
            None => {
                let c = read_coverage(r).map(Arc::new);
                lock(&self.coverage).insert(r.clone(), c.clone());
                c
            }
        };
        match cov {
            Some(c) => cmap::lookup(&c.table, c.sub, ch as u32).is_some(),
            None => false,
        }
    }

    /// A regular upright face that covers `ch`: the preferred families first, then sensible
    /// defaults for the character's script, then any indexed face.
    pub fn fallback_for(&self, ch: char, preferred_families: &[&str]) -> Option<FontRef> {
        self.fallback_for_style(ch, preferred_families, 400, false)
    }

    /// `fallback_for` with a wanted weight and slant (the nearest face of a covering family).
    pub fn fallback_for_style(&self, ch: char, preferred_families: &[&str], weight: u16, italic: bool) -> Option<FontRef> {
        let mut tried: Vec<String> = Vec::new();
        for fam in preferred_families.iter().chain(default_families(ch).iter()) {
            let key = fam.to_lowercase();
            if tried.contains(&key) {
                continue;
            }
            tried.push(key);
            if let Some(r) = self.find(fam, weight, italic) {
                if self.covers(&r, ch) {
                    return Some(r);
                }
            }
        }
        for f in &self.faces {
            if tried.contains(&f.preferred_family().to_lowercase()) {
                continue;
            }
            if self.covers(&f.font, ch) {
                if let Some(r) = self.find(f.preferred_family(), weight, italic) {
                    if self.covers(&r, ch) {
                        return Some(r);
                    }
                }
                return Some(f.font.clone());
            }
        }
        None
    }
}

fn read_coverage(r: &FontRef) -> Option<Coverage> {
    let mut f = File::open(&r.path).ok()?;
    let file_len = f.metadata().ok()?.len();
    let (_, recs) = faces_of(&mut f).into_iter().find(|(i, _)| *i == r.index)?;
    let table = read_table(&mut f, file_len, &recs, b"cmap", 64 << 20)?;
    let sub = cmap::choose(&table)?;
    Some(Coverage { table, sub })
}
