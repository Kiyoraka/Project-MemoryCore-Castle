//! Fonts in the PDF: one Type0 font per face, Identity-H, glyph ids as CIDs.
//!
//! TrueType faces: CIDFontType2 with a subset FontFile2 (ids kept, so `/CIDToGIDMap /Identity`) and a
//! `ABCDEF+` subset tag. CFF faces: CIDFontType0 with the whole face as FontFile3/OpenType (CFF
//! subsetting is a TODO in `font/`; the CIDs are glyph ids, which is right for name-keyed CFF - the
//! usual kind - but a CID-keyed CFF would want its charset mapped).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;

use super::md5::md5;
use super::obj::{name, num, nums};
use super::PdfWriter;
use crate::ground::font::{extract_face, subset_truetype, Font};

pub(super) struct FontEntry {
    pub font: Arc<Font>,
    /// The Type0 font's object number (reserved when the font is first used).
    pub type0: u32,
    pub used: BTreeSet<u16>,
    /// Text the caller gave for glyphs (wins over the cmap).
    pub text: BTreeMap<u16, String>,
}

impl FontEntry {
    pub fn new(font: Arc<Font>, type0: u32) -> FontEntry {
        FontEntry { font, type0, used: BTreeSet::new(), text: BTreeMap::new() }
    }

    /// A glyph's width in PDF glyph space (1/1000 em), rounded to 3 decimals so `/W` and the `TJ`
    /// arithmetic agree exactly.
    pub fn width(&self, gid: u16) -> f64 {
        glyph_width(&self.font, gid)
    }
}

pub(super) fn glyph_width(font: &Font, gid: u16) -> f64 {
    let upem = font.units_per_em().max(1) as f64;
    (font.advance(gid) as f64 * 1000.0 / upem * 1000.0).round() / 1000.0
}

/// The PostScript name, cleaned to what a PDF font name may hold.
fn base_name(font: &Font) -> String {
    let n = font.names();
    let raw = if !n.postscript_name.is_empty() { n.postscript_name } else { n.full_name.replace(' ', "") };
    let s: String = raw.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(100).collect();
    if s.is_empty() {
        "Font".to_string()
    } else {
        s
    }
}

/// Six capital letters from the name and the glyphs kept: the same subset always gets the same tag.
fn subset_tag(base: &str, used: &BTreeSet<u16>) -> String {
    let mut bytes = base.as_bytes().to_vec();
    for g in used {
        bytes.extend_from_slice(&g.to_be_bytes());
    }
    md5(&bytes)[..6].iter().map(|b| (b'A' + b % 26) as char).collect()
}

pub(super) fn write_font(w: &mut PdfWriter, e: &FontEntry) {
    let font = &e.font;
    let cff = !font.has_truetype_outlines() && font.is_cff();
    let base = base_name(font);
    let full = if cff { base.clone() } else { format!("{}+{}", subset_tag(&base, &e.used), base) };
    let m = font.metrics();
    let upem = m.units_per_em.max(1) as f64;
    let k = 1000.0 / upem;

    // The font program.
    let file = if cff {
        let data = extract_face(font);
        w.new_stream("/Subtype /OpenType", &data, true)
    } else {
        let mut keep: Vec<u16> = e.used.iter().copied().collect();
        keep.sort_unstable();
        let data = subset_truetype(font, &keep);
        w.new_stream(&format!("/Length1 {}", data.len()), &data, true)
    };

    // The descriptor.
    let style = font.style();
    let fixed = font.table(b"post").and_then(|p| p.get(12..16)).map_or(false, |b| b != [0, 0, 0, 0]);
    let mut flags = 4u32; // symbolic: the glyphs are addressed by id, not by a standard encoding
    if fixed {
        flags |= 1;
    }
    if style.italic || m.italic_angle != 0.0 {
        flags |= 64;
    }
    let weight = style.weight.clamp(100, 900) as f64;
    let stem_v = (10.0 + 220.0 * (weight - 50.0) / 900.0).round();
    let bbox = [m.bbox[0] as f64 * k, m.bbox[1] as f64 * k, m.bbox[2] as f64 * k, m.bbox[3] as f64 * k];
    let fd = w.alloc();
    let fd_body = format!(
        "<< /Type /FontDescriptor /FontName {} /Flags {} /FontBBox [{}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /XHeight {} /StemV {} /{} {} 0 R >>",
        name(&full),
        flags,
        nums(&bbox),
        num(m.italic_angle as f64),
        num(m.ascender as f64 * k),
        num(m.descender as f64 * k),
        num(m.cap_height as f64 * k),
        num(m.x_height as f64 * k),
        num(stem_v),
        if cff { "FontFile3" } else { "FontFile2" },
        file
    );
    w.put(fd, fd_body.into_bytes());

    // Widths: runs of consecutive ids, `gid [w w w]`.
    let mut wa = String::new();
    let used: Vec<u16> = e.used.iter().copied().collect();
    let mut i = 0;
    while i < used.len() {
        let start = used[i];
        let mut j = i;
        while j + 1 < used.len() && used[j + 1] == used[j] + 1 {
            j += 1;
        }
        let ws: Vec<f64> = used[i..=j].iter().map(|&g| e.width(g)).collect();
        let _ = write!(wa, "{} [{}] ", start, nums(&ws));
        i = j + 1;
    }

    let cid = w.alloc();
    let cid_body = format!(
        "<< /Type /Font /Subtype /{} /BaseFont {} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {} 0 R /DW {} /W [{}]{} >>",
        if cff { "CIDFontType0" } else { "CIDFontType2" },
        name(&full),
        fd,
        num(e.width(0)),
        wa.trim_end(),
        if cff { "" } else { " /CIDToGIDMap /Identity" }
    );
    w.put(cid, cid_body.into_bytes());

    let tu = w.new_stream("", to_unicode(font, &e.used, &e.text).as_bytes(), true);

    let t0 = format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont {} /Encoding /Identity-H /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>",
        name(&full),
        cid,
        tu
    );
    w.put(e.type0, t0.into_bytes());
}

/// The ToUnicode CMap: each used glyph to the text it stands for (the caller's text first, else the
/// font's cmap reversed, preferring the lowest code point outside the private use areas).
pub(super) fn to_unicode(font: &Font, used: &BTreeSet<u16>, text: &BTreeMap<u16, String>) -> String {
    let mut best: BTreeMap<u16, char> = BTreeMap::new();
    let rank = |c: char| -> (bool, u32) {
        let u = c as u32;
        ((0xE000..=0xF8FF).contains(&u) || u >= 0xF0000, u)
    };
    for (c, g) in font.mappings() {
        if !used.contains(&g) {
            continue;
        }
        match best.get(&g) {
            Some(&b) if rank(b) <= rank(c) => {}
            _ => {
                best.insert(g, c);
            }
        }
    }
    let mut pairs: Vec<(u16, String)> = Vec::new();
    for &g in used {
        if let Some(t) = text.get(&g) {
            if !t.is_empty() {
                pairs.push((g, t.clone()));
                continue;
            }
        }
        if let Some(&c) = best.get(&g) {
            pairs.push((g, c.to_string()));
        }
    }
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in pairs.chunks(100) {
        let _ = writeln!(s, "{} beginbfchar", chunk.len());
        for (g, t) in chunk {
            let mut hex = String::new();
            let mut buf = [0u16; 2];
            for c in t.chars() {
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(hex, "{u:04X}");
                }
            }
            let _ = writeln!(s, "<{g:04X}> <{hex}>");
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s
}
