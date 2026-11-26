//! Fonts: TrueType and OpenType read by us. One `Font` per face (TrueType Collections by index)
//! answers what the text engine (advances, kerning, metrics), the canvas (glyph outlines as paths)
//! and the PDF writer (subset embedding) need. `FontIndex` finds installed fonts by family and style
//! and picks a fallback that covers a character.
//!
//! Every read is bounds-checked: a malformed font yields an error, a missing table, or `None` for one
//! glyph - never a panic.
//!
//! Units: everything is in font units (see `Metrics::units_per_em`), y pointing up.

mod cff;
mod cmap;
mod glyf;
mod index;
mod kern;
mod names;
mod read;
mod subset;
#[cfg(test)]
mod tests;

pub use index::{FaceInfo, FontIndex, FontRef};
pub use subset::{extract_face, subset_truetype, subset_truetype_compact};

use read::{rd16, rd32, rdi16, TableRec};
use std::fmt;

/// One drawing step of a glyph outline, in font units with y up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathOp {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    /// Control point, end point.
    QuadTo(f32, f32, f32, f32),
    /// Control point 1, control point 2, end point.
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    /// The data ends before a required structure.
    Truncated,
    /// Not a TrueType/OpenType/collection file.
    NotAFont,
    /// The collection has no face with that index.
    NoSuchFace(u32),
    /// A table every font needs is absent.
    MissingTable(&'static str),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::Truncated => write!(f, "font data is truncated"),
            FontError::NotAFont => write!(f, "not a TrueType or OpenType font"),
            FontError::NoSuchFace(i) => write!(f, "font collection has no face {i}"),
            FontError::MissingTable(t) => write!(f, "font has no '{t}' table"),
        }
    }
}

impl std::error::Error for FontError {}

/// Vertical metrics in font units. `ascender`/`descender`/`line_gap` are the ones to lay text out
/// with: OS/2 typo values when the font asks for them (USE_TYPO_METRICS), else `hhea`, else typo,
/// else win. All three sources are also given as found.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Metrics {
    pub units_per_em: u16,
    pub ascender: i16,
    /// Negative below the baseline.
    pub descender: i16,
    pub line_gap: i16,
    pub x_height: i16,
    pub cap_height: i16,
    /// Top of the underline stroke (negative below the baseline), as in `post`.
    pub underline_pos: i16,
    pub underline_thickness: i16,
    pub strikeout_pos: i16,
    pub strikeout_thickness: i16,
    pub hhea_ascender: i16,
    pub hhea_descender: i16,
    pub hhea_line_gap: i16,
    pub typo_ascender: i16,
    pub typo_descender: i16,
    pub typo_line_gap: i16,
    /// usWinAscent (positive, above the baseline).
    pub win_ascent: u16,
    /// usWinDescent (positive, below the baseline).
    pub win_descent: u16,
    pub use_typo_metrics: bool,
    /// Degrees counter-clockwise from vertical (negative for right-leaning italics).
    pub italic_angle: f32,
    /// Font bounding box: x_min, y_min, x_max, y_max.
    pub bbox: [i16; 4],
}

/// The face's names, best language chosen (Windows Unicode English, then Mac Roman).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    /// Name ID 1 (e.g. "Segoe UI Semibold" for that face).
    pub family: String,
    /// Name ID 2 ("Regular", "Bold Italic").
    pub subfamily: String,
    /// Name ID 4.
    pub full_name: String,
    /// Name ID 6.
    pub postscript_name: String,
    /// Name ID 16 when present (e.g. "Segoe UI"), else empty.
    pub typographic_family: String,
    /// Name ID 17 when present, else empty.
    pub typographic_subfamily: String,
}

impl Names {
    /// The family a user would pick in a font menu: typographic family if given, else family.
    pub fn preferred_family(&self) -> &str {
        if self.typographic_family.is_empty() {
            &self.family
        } else {
            &self.typographic_family
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// 100..=900 (400 regular, 700 bold).
    pub weight: u16,
    pub italic: bool,
    /// OS/2 width class 1..=9 (5 normal, 3 condensed, 7 expanded).
    pub stretch: u16,
}

impl Default for Style {
    fn default() -> Self {
        Style { weight: 400, italic: false, stretch: 5 }
    }
}

#[derive(Clone, Copy, Debug)]
struct Tab {
    off: usize,
    len: usize,
}

/// A parsed font face.
pub struct Font {
    data: Vec<u8>,
    index: u32,
    sfnt_version: u32,
    tables: Vec<TableRec>,
    num_glyphs: u16,
    num_hmetrics: u16,
    hmtx: Option<Tab>,
    loca: Option<Tab>,
    glyf: Option<Tab>,
    loca_long: bool,
    cmap: Option<(Tab, cmap::Sub)>,
    cff: Option<(Tab, cff::Cff)>,
    kern: Option<Tab>,
    gpos: Option<(Tab, kern::Gpos)>,
    metrics: Metrics,
    names: Names,
    style: Style,
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("full_name", &self.names.full_name)
            .field("index", &self.index)
            .field("num_glyphs", &self.num_glyphs)
            .field("cff", &self.cff.is_some())
            .finish()
    }
}

impl Font {
    /// Parse face `index` of a font file (0 for a plain .ttf/.otf; any face of a .ttc/.otc).
    pub fn parse(data: Vec<u8>, index: u32) -> Result<Font, FontError> {
        let face = read::face_offset(&data, index)?;
        let dir = data.get(face..).ok_or(FontError::Truncated)?;
        let sfnt_version = read::sfnt_version(dir).ok_or(FontError::Truncated)?;
        if sfnt_version != read::SFNT_TRUE && sfnt_version != read::SFNT_OTTO && sfnt_version != read::SFNT_APPLE {
            return Err(FontError::NotAFont);
        }
        let recs = read::table_records(dir).ok_or(FontError::Truncated)?;
        let mut tables = Vec::with_capacity(recs.len());
        for r in recs {
            if r.off >= data.len() {
                continue;
            }
            let len = r.len.min(data.len() - r.off);
            tables.push(TableRec { tag: r.tag, off: r.off, len });
        }
        let find = |tag: &[u8; 4]| tables.iter().find(|t| &t.tag == tag).map(|t| Tab { off: t.off, len: t.len });
        let bytes = |t: Tab| -> &[u8] { data.get(t.off..t.off + t.len).unwrap_or(&[]) };

        let head_t = find(b"head").ok_or(FontError::MissingTable("head"))?;
        let head = bytes(head_t);
        if head.len() < 54 {
            return Err(FontError::Truncated);
        }
        let maxp_t = find(b"maxp").ok_or(FontError::MissingTable("maxp"))?;
        let num_glyphs = rd16(bytes(maxp_t), 4).ok_or(FontError::Truncated)?;
        let hhea = find(b"hhea").map(bytes);
        let num_hmetrics = hhea.and_then(|h| rd16(h, 34)).unwrap_or(0);
        let loca_long = rdi16(head, 50).unwrap_or(0) != 0;
        let cmap = find(b"cmap").and_then(|t| cmap::choose(bytes(t)).map(|s| (t, s)));
        let cff = find(b"CFF ").and_then(|t| cff::Cff::parse(bytes(t)).map(|c| (t, c)));
        let gpos = find(b"GPOS").and_then(|t| kern::Gpos::parse(bytes(t)).map(|g| (t, g)));
        let os2 = find(b"OS/2").map(bytes);
        let names = names::parse_names(find(b"name").map(bytes));
        let style = names::parse_style(os2, Some(head), &names.subfamily);

        let mut font = Font {
            sfnt_version,
            num_glyphs,
            num_hmetrics,
            hmtx: find(b"hmtx"),
            loca: find(b"loca"),
            glyf: find(b"glyf"),
            loca_long,
            cmap,
            cff,
            kern: find(b"kern"),
            gpos,
            metrics: Metrics::default(),
            names,
            style,
            tables,
            index,
            data: Vec::new(),
        };
        font.data = data;
        font.metrics = font.compute_metrics();
        Ok(font)
    }

    /// How many faces a font file holds (0 if it is not a font).
    pub fn face_count(data: &[u8]) -> u32 {
        read::face_count(data)
    }

    /// The whole file the face came from.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn face_index(&self) -> u32 {
        self.index
    }

    fn bytes(&self, t: Tab) -> &[u8] {
        self.data.get(t.off..t.off + t.len).unwrap_or(&[])
    }

    /// Raw bytes of a table, if the face has it.
    pub fn table(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.tables.iter().find(|t| &t.tag == tag).map(|t| self.bytes(Tab { off: t.off, len: t.len }))
    }

    pub(crate) fn table_tags(&self) -> Vec<[u8; 4]> {
        self.tables.iter().map(|t| t.tag).collect()
    }

    pub(crate) fn sfnt_version(&self) -> u32 {
        self.sfnt_version
    }

    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    pub fn units_per_em(&self) -> u16 {
        self.metrics.units_per_em
    }

    /// True for PostScript-flavoured OpenType (outlines in `CFF `).
    pub fn is_cff(&self) -> bool {
        self.cff.is_some()
    }

    /// True when the face has `glyf`/`loca` TrueType outlines (what `subset_truetype` subsets).
    pub fn has_truetype_outlines(&self) -> bool {
        self.glyf.is_some() && self.loca.is_some()
    }

    pub(crate) fn glyf(&self) -> Option<glyf::Glyf<'_>> {
        Some(glyf::Glyf {
            glyf: self.bytes(self.glyf?),
            loca: self.bytes(self.loca?),
            long: self.loca_long,
            num_glyphs: self.num_glyphs,
        })
    }

    /// The glyph for a character, or None when the font does not cover it.
    pub fn glyph_id(&self, c: char) -> Option<u16> {
        let (t, s) = self.cmap?;
        cmap::lookup(self.bytes(t), s, c as u32).filter(|&g| g < self.num_glyphs)
    }

    pub fn has_char(&self, c: char) -> bool {
        self.glyph_id(c).is_some()
    }

    /// Every (character, glyph) pair of the font's cmap.
    pub fn mappings(&self) -> Vec<(char, u16)> {
        let mut out = Vec::new();
        if let Some((t, s)) = self.cmap {
            cmap::for_each(self.bytes(t), s, &mut |c, g| {
                if let Some(ch) = char::from_u32(c) {
                    if g < self.num_glyphs {
                        out.push((ch, g));
                    }
                }
            });
        }
        out
    }

    /// Raw cmap codes (Symbol fonts keep their U+F0xx codes) and whether the cmap is a Symbol one.
    pub(crate) fn raw_mappings(&self) -> (bool, Vec<(u32, u16)>) {
        let mut out = Vec::new();
        let mut symbol = false;
        if let Some((t, s)) = self.cmap {
            symbol = s.kind == cmap::Kind::Symbol;
            cmap::for_each(self.bytes(t), s, &mut |c, g| out.push((c, g)));
        }
        (symbol, out)
    }

    /// Horizontal advance in font units (0 for a glyph id out of range).
    pub fn advance(&self, gid: u16) -> u16 {
        let Some(t) = self.hmtx else { return 0 };
        if gid >= self.num_glyphs || self.num_hmetrics == 0 {
            return 0;
        }
        let h = self.bytes(t);
        let i = (gid.min(self.num_hmetrics - 1)) as usize;
        rd16(h, 4 * i).unwrap_or(0)
    }

    /// Left side bearing in font units.
    pub fn left_side_bearing(&self, gid: u16) -> i16 {
        let Some(t) = self.hmtx else { return 0 };
        if gid >= self.num_glyphs {
            return 0;
        }
        let h = self.bytes(t);
        let nh = self.num_hmetrics as usize;
        let g = gid as usize;
        if g < nh {
            rdi16(h, 4 * g + 2).unwrap_or(0)
        } else {
            rdi16(h, 4 * nh + 2 * (g - nh)).unwrap_or(0)
        }
    }

    /// Pair kerning in font units (negative brings the glyphs closer). GPOS 'kern' pair positioning
    /// wins when the font has it; otherwise the `kern` table.
    pub fn kerning(&self, left: u16, right: u16) -> i16 {
        let v = if let Some((t, g)) = &self.gpos {
            g.kern(self.bytes(*t), left, right).unwrap_or(0)
        } else if let Some(t) = self.kern {
            kern::kern_table(self.bytes(t), left, right)
        } else {
            0
        };
        v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }

    /// The glyph's outline in font units (y up). `Some(empty)` for blank glyphs such as space;
    /// None when the glyph id is out of range, malformed, or the face has no outlines we read.
    pub fn outline(&self, gid: u16) -> Option<Vec<PathOp>> {
        if gid >= self.num_glyphs {
            return None;
        }
        if let Some(g) = self.glyf() {
            return g.outline(gid);
        }
        if let Some((t, c)) = &self.cff {
            return c.outline(self.bytes(*t), gid);
        }
        None
    }

    /// Bounding box of the outline (including control points): x_min, y_min, x_max, y_max.
    pub fn glyph_bounds(&self, gid: u16) -> Option<[f32; 4]> {
        bounds(&self.outline(gid)?)
    }

    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    pub fn names(&self) -> Names {
        self.names.clone()
    }

    pub fn style(&self) -> Style {
        self.style
    }

    fn compute_metrics(&self) -> Metrics {
        let head = self.table(b"head").unwrap_or(&[]);
        let mut upem = rd16(head, 18).unwrap_or(1000);
        if !(16..=16384).contains(&upem) {
            upem = 1000;
        }
        let bbox = [
            rdi16(head, 36).unwrap_or(0),
            rdi16(head, 38).unwrap_or(0),
            rdi16(head, 40).unwrap_or(0),
            rdi16(head, 42).unwrap_or(0),
        ];
        let mut m = Metrics { units_per_em: upem, bbox, ..Metrics::default() };
        if let Some(h) = self.table(b"hhea") {
            m.hhea_ascender = rdi16(h, 4).unwrap_or(0);
            m.hhea_descender = rdi16(h, 6).unwrap_or(0);
            m.hhea_line_gap = rdi16(h, 8).unwrap_or(0);
        }
        let mut x_height = 0i16;
        let mut cap_height = 0i16;
        let mut has_typo = false;
        if let Some(o) = self.table(b"OS/2") {
            m.strikeout_thickness = rdi16(o, 26).unwrap_or(0);
            m.strikeout_pos = rdi16(o, 28).unwrap_or(0);
            let fs = rd16(o, 62).unwrap_or(0);
            if o.len() >= 78 {
                has_typo = true;
                m.typo_ascender = rdi16(o, 68).unwrap_or(0);
                m.typo_descender = rdi16(o, 70).unwrap_or(0);
                m.typo_line_gap = rdi16(o, 72).unwrap_or(0);
                m.win_ascent = rd16(o, 74).unwrap_or(0);
                m.win_descent = rd16(o, 76).unwrap_or(0);
                m.use_typo_metrics = fs & (1 << 7) != 0;
            }
            if rd16(o, 0).unwrap_or(0) >= 2 {
                x_height = rdi16(o, 86).unwrap_or(0);
                cap_height = rdi16(o, 88).unwrap_or(0);
            }
        }
        let typo_ok = has_typo && (m.typo_ascender != 0 || m.typo_descender != 0);
        let (a, d, g) = if m.use_typo_metrics && typo_ok {
            (m.typo_ascender, m.typo_descender, m.typo_line_gap)
        } else if m.hhea_ascender != 0 || m.hhea_descender != 0 {
            (m.hhea_ascender, m.hhea_descender, m.hhea_line_gap)
        } else if typo_ok {
            (m.typo_ascender, m.typo_descender, m.typo_line_gap)
        } else if m.win_ascent != 0 || m.win_descent != 0 {
            (m.win_ascent.min(i16::MAX as u16) as i16, -(m.win_descent.min(i16::MAX as u16) as i16), 0)
        } else {
            (bbox[3], bbox[1], 0)
        };
        m.ascender = a;
        m.descender = d;
        m.line_gap = g;
        if let Some(p) = self.table(b"post") {
            m.italic_angle = rd32(p, 4).map_or(0.0, |v| v as i32 as f32 / 65536.0);
            m.underline_pos = rdi16(p, 8).unwrap_or(0);
            m.underline_thickness = rdi16(p, 10).unwrap_or(0);
        }
        if m.underline_thickness <= 0 {
            m.underline_thickness = (upem / 14).max(1) as i16;
        }
        if m.underline_pos == 0 {
            m.underline_pos = -((upem / 10) as i16);
        }
        if m.strikeout_thickness <= 0 {
            m.strikeout_thickness = m.underline_thickness;
        }
        let top_of = |c: char| {
            self.glyph_id(c).and_then(|g| self.glyph_bounds(g)).map(|b| b[3].round() as i16).filter(|&v| v > 0)
        };
        if x_height <= 0 {
            x_height = top_of('x').unwrap_or((upem / 2) as i16);
        }
        if cap_height <= 0 {
            cap_height = top_of('H').unwrap_or((upem as u32 * 7 / 10) as i16);
        }
        m.x_height = x_height;
        m.cap_height = cap_height;
        if m.strikeout_pos == 0 {
            m.strikeout_pos = x_height / 2;
        }
        m
    }
}

/// Bounding box of a path's points (control points included).
pub fn bounds(ops: &[PathOp]) -> Option<[f32; 4]> {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    let mut any = false;
    let mut add = |x: f32, y: f32| {
        any = true;
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    };
    for op in ops {
        match *op {
            PathOp::MoveTo(x, y) | PathOp::LineTo(x, y) => add(x, y),
            PathOp::QuadTo(a, b2, x, y) => {
                add(a, b2);
                add(x, y)
            }
            PathOp::CubicTo(a, b2, c, d, x, y) => {
                add(a, b2);
                add(c, d);
                add(x, y)
            }
            PathOp::Close => {}
        }
    }
    if any {
        Some(b)
    } else {
        None
    }
}
