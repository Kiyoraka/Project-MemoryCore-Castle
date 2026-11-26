//! Subsetting for PDF embedding: a valid TrueType font holding only the glyphs a document uses
//! (+ .notdef and every composite component), with glyf/loca/hmtx/maxp/head/hhea rewritten and a
//! minimal cmap/post/name. Hinting tables (cvt/fpgm/prep/gasp) and OS/2 are copied as they are.
//!
//! CFF (PostScript-outline) subsetting is a later TODO: for such faces `subset_truetype` returns the
//! whole face as a standalone OpenType file (`extract_face`), embeddable as FontFile3/OpenType.

use super::glyf::components;
use super::read::{put16, put32, rd16, rd32, w16, w32, wi16};
use super::Font;
use std::collections::HashMap;

/// Subset keeping the original glyph ids (unused ids become empty glyphs with zero advance), so a
/// PDF can address glyphs with the same ids it measured with (CIDToGIDMap /Identity). The font's
/// glyph count shrinks to the highest kept id + 1.
pub fn subset_truetype(font: &Font, glyphs: &[u16]) -> Vec<u8> {
    if !font.has_truetype_outlines() {
        return extract_face(font);
    }
    let kept = closure(font, glyphs);
    let max = kept.iter().copied().max().unwrap_or(0) as usize;
    let mut old_of_new: Vec<Option<u16>> = vec![None; max + 1];
    for &g in &kept {
        old_of_new[g as usize] = Some(g);
    }
    build(font, &old_of_new, false)
}

/// Subset renumbering glyphs compactly: new id 0 is .notdef, then the requested glyphs in the order
/// given (duplicates and 0 skipped), then composite components. Returns the font and, for each new
/// id, the original id.
pub fn subset_truetype_compact(font: &Font, glyphs: &[u16]) -> (Vec<u8>, Vec<u16>) {
    if !font.has_truetype_outlines() {
        return (extract_face(font), (0..font.num_glyphs()).collect());
    }
    let mut seen = vec![false; font.num_glyphs() as usize + 1];
    let mut order: Vec<u16> = vec![0];
    seen[0] = true;
    for &g in glyphs.iter().chain(closure(font, glyphs).iter()) {
        if g < font.num_glyphs() && !seen[g as usize] {
            seen[g as usize] = true;
            order.push(g);
        }
    }
    let old_of_new: Vec<Option<u16>> = order.iter().map(|&g| Some(g)).collect();
    (build(font, &old_of_new, true), order)
}

/// The face as a standalone sfnt file (all its tables copied; useful for collection members and
/// CFF faces).
pub fn extract_face(font: &Font) -> Vec<u8> {
    let mut tables = Vec::new();
    for tag in font.table_tags() {
        if let Some(t) = font.table(&tag) {
            if tables.iter().any(|(t2, _): &([u8; 4], Vec<u8>)| *t2 == tag) {
                continue;
            }
            let mut v = t.to_vec();
            if &tag == b"head" {
                put32(&mut v, 8, 0);
            }
            tables.push((tag, v));
        }
    }
    write_sfnt(font.sfnt_version(), tables)
}

/// The requested glyphs plus .notdef and all composite components (sorted, unique, in range).
fn closure(font: &Font, glyphs: &[u16]) -> Vec<u16> {
    let n = font.num_glyphs();
    let mut seen = vec![false; n as usize + 1];
    seen[0] = true;
    let mut work: Vec<u16> = glyphs.iter().copied().filter(|&g| g < n).collect();
    work.push(0);
    let gl = font.glyf();
    while let Some(g) = work.pop() {
        seen[g as usize] = true;
        let Some(data) = gl.and_then(|gl| gl.data(g)) else { continue };
        for c in components(data) {
            if c.gid < n && !seen[c.gid as usize] {
                seen[c.gid as usize] = true;
                work.push(c.gid);
            }
        }
    }
    (0..n).filter(|&g| seen[g as usize]).collect()
}

fn build(font: &Font, old_of_new: &[Option<u16>], renumber: bool) -> Vec<u8> {
    let Some(gl) = font.glyf() else { return Vec::new() };
    let new_of_old: HashMap<u16, u16> =
        old_of_new.iter().enumerate().filter_map(|(n, o)| o.map(|o| (o, n as u16))).collect();
    let count = old_of_new.len();

    // glyf + loca offsets
    let mut glyf: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::with_capacity(count + 1);
    for old in old_of_new {
        offsets.push(glyf.len());
        let Some(old) = old else { continue };
        let Some(data) = gl.data(*old) else { continue };
        if data.is_empty() {
            continue;
        }
        let mut g = data.to_vec();
        if renumber {
            for c in components(data) {
                let ng = new_of_old.get(&c.gid).copied().unwrap_or(0);
                put16(&mut g, c.gid_pos, ng);
            }
        }
        glyf.extend_from_slice(&g);
        while glyf.len() % 4 != 0 {
            glyf.push(0);
        }
    }
    offsets.push(glyf.len());
    let long = glyf.len() > 0x1FFFE;
    let mut loca = Vec::new();
    for &o in &offsets {
        if long {
            w32(&mut loca, o as u32);
        } else {
            w16(&mut loca, (o / 2) as u16);
        }
    }

    // hmtx: one full metric per glyph
    let mut hmtx = Vec::with_capacity(4 * count);
    let mut max_adv = 0u16;
    for old in old_of_new {
        let (adv, lsb) = match old {
            Some(o) => (font.advance(*o), font.left_side_bearing(*o)),
            None => (0, 0),
        };
        max_adv = max_adv.max(adv);
        w16(&mut hmtx, adv);
        wi16(&mut hmtx, lsb);
    }

    let mut head = font.table(b"head").unwrap_or(&[]).to_vec();
    if head.len() < 54 {
        return Vec::new();
    }
    put32(&mut head, 8, 0);
    put16(&mut head, 50, if long { 1 } else { 0 });

    let mut hhea = font.table(b"hhea").map(|h| h.to_vec()).unwrap_or_default();
    if hhea.len() < 36 {
        hhea = default_hhea(font);
    }
    let old_max = rd16(&hhea, 10).unwrap_or(0);
    put16(&mut hhea, 10, max_adv.max(old_max));
    put16(&mut hhea, 34, count as u16);

    let mut maxp = font.table(b"maxp").map(|m| m.to_vec()).unwrap_or_default();
    if maxp.len() < 6 {
        maxp = vec![0, 0, 0x50, 0, 0, 0];
    }
    put16(&mut maxp, 4, count as u16);

    // cmap from the original mappings of kept glyphs
    let (symbol, raw) = font.raw_mappings();
    let mut pairs: Vec<(u32, u16)> =
        raw.into_iter().filter_map(|(c, g)| new_of_old.get(&g).map(|&n| (c, n))).filter(|&(_, n)| n != 0).collect();
    pairs.sort_unstable();
    pairs.dedup_by_key(|p| p.0);
    let cmap = build_cmap(&pairs, symbol);

    let mut post = vec![0u8; 32];
    put32(&mut post, 0, 0x0003_0000);
    if let Some(p) = font.table(b"post") {
        if let (Some(a), Some(b), Some(c)) = (rd32(p, 4), rd32(p, 8), rd32(p, 12)) {
            put32(&mut post, 4, a);
            put32(&mut post, 8, b);
            put32(&mut post, 12, c);
        }
    }

    let names = font.names();
    let family = if names.family.is_empty() { "Subset".to_string() } else { names.family.clone() };
    let subfamily = if names.subfamily.is_empty() { "Regular".to_string() } else { names.subfamily.clone() };
    let full = if names.full_name.is_empty() { format!("{family} {subfamily}") } else { names.full_name.clone() };
    let ps = if names.postscript_name.is_empty() {
        full.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect()
    } else {
        names.postscript_name.clone()
    };
    let name = build_name(&[(1, &family), (2, &subfamily), (3, &ps), (4, &full), (6, &ps)]);

    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"maxp", maxp),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"glyf", glyf),
        (*b"cmap", cmap),
        (*b"post", post),
        (*b"name", name),
    ];
    for tag in [b"OS/2", b"cvt ", b"fpgm", b"prep", b"gasp"] {
        if let Some(t) = font.table(tag) {
            tables.push((*tag, t.to_vec()));
        }
    }
    write_sfnt(0x0001_0000, tables)
}

fn default_hhea(font: &Font) -> Vec<u8> {
    let m = font.metrics();
    let mut h = Vec::with_capacity(36);
    w32(&mut h, 0x0001_0000);
    wi16(&mut h, m.ascender);
    wi16(&mut h, m.descender);
    wi16(&mut h, m.line_gap);
    h.resize(36, 0);
    put16(&mut h, 18, 1); // caretSlopeRise
    h
}

/// A cmap with a format 4 subtable (BMP) and, when needed, a format 12 one.
pub(crate) fn build_cmap(pairs: &[(u32, u16)], symbol: bool) -> Vec<u8> {
    let bmp: Vec<(u32, u16)> = pairs.iter().copied().filter(|&(c, _)| c < 0xFFFF).collect();
    let need12 = pairs.iter().any(|&(c, _)| c > 0xFFFF);
    let f4 = format4(&bmp);
    let f12 = if need12 { Some(format12(pairs)) } else { None };
    let n_records: u16 = if f12.is_some() { 2 } else { 1 };
    let mut out = Vec::new();
    w16(&mut out, 0);
    w16(&mut out, n_records);
    let first = 4 + 8 * n_records as u32;
    w16(&mut out, 3);
    w16(&mut out, if symbol { 0 } else { 1 });
    w32(&mut out, first);
    if f12.is_some() {
        w16(&mut out, 3);
        w16(&mut out, 10);
        w32(&mut out, first + f4.len() as u32);
    }
    out.extend_from_slice(&f4);
    if let Some(f) = f12 {
        out.extend_from_slice(&f);
    }
    out
}

fn runs(pairs: &[(u32, u16)]) -> Vec<(u32, u32, u16)> {
    let mut out: Vec<(u32, u32, u16)> = Vec::new();
    for &(c, g) in pairs {
        if let Some(last) = out.last_mut() {
            if c == last.1 + 1 && g as u32 == last.2 as u32 + (c - last.0) && g != 0 {
                last.1 = c;
                continue;
            }
        }
        out.push((c, c, g));
    }
    out
}

fn format4(pairs: &[(u32, u16)]) -> Vec<u8> {
    let mut segs = runs(pairs);
    segs.push((0xFFFF, 0xFFFF, 0));
    let n = segs.len();
    let seg_x2 = (2 * n) as u16;
    let es = (usize::BITS - 1 - n.leading_zeros()) as u16;
    let sr = 2 * (1u16 << es);
    let mut v = Vec::new();
    w16(&mut v, 4);
    w16(&mut v, (16 + 8 * n) as u16);
    w16(&mut v, 0);
    w16(&mut v, seg_x2);
    w16(&mut v, sr);
    w16(&mut v, es);
    w16(&mut v, seg_x2.wrapping_sub(sr));
    for s in &segs {
        w16(&mut v, s.1 as u16);
    }
    w16(&mut v, 0);
    for s in &segs {
        w16(&mut v, s.0 as u16);
    }
    for s in &segs {
        let delta = if s.0 == 0xFFFF { 1u16 } else { s.2.wrapping_sub(s.0 as u16) };
        w16(&mut v, delta);
    }
    for _ in &segs {
        w16(&mut v, 0);
    }
    v
}

fn format12(pairs: &[(u32, u16)]) -> Vec<u8> {
    let groups = runs(pairs);
    let mut v = Vec::new();
    w16(&mut v, 12);
    w16(&mut v, 0);
    w32(&mut v, (16 + 12 * groups.len()) as u32);
    w32(&mut v, 0);
    w32(&mut v, groups.len() as u32);
    for g in &groups {
        w32(&mut v, g.0);
        w32(&mut v, g.1);
        w32(&mut v, g.2 as u32);
    }
    v
}

/// A `name` table with Windows Unicode English records.
pub(crate) fn build_name(entries: &[(u16, &str)]) -> Vec<u8> {
    let mut storage: Vec<u8> = Vec::new();
    let mut v = Vec::new();
    w16(&mut v, 0);
    w16(&mut v, entries.len() as u16);
    w16(&mut v, (6 + 12 * entries.len()) as u16);
    for (id, s) in entries {
        let start = storage.len();
        for u in s.encode_utf16() {
            w16(&mut storage, u);
        }
        w16(&mut v, 3);
        w16(&mut v, 1);
        w16(&mut v, 0x0409);
        w16(&mut v, *id);
        w16(&mut v, (storage.len() - start) as u16);
        w16(&mut v, start as u16);
    }
    v.extend_from_slice(&storage);
    v
}

fn checksum(d: &[u8]) -> u32 {
    let mut sum = 0u32;
    for c in d.chunks(4) {
        let mut b = [0u8; 4];
        b[..c.len()].copy_from_slice(c);
        sum = sum.wrapping_add(u32::from_be_bytes(b));
    }
    sum
}

/// Assemble an sfnt file: sorted table directory, 4-byte padding, checksums and the head
/// checkSumAdjustment (the head table must carry 0 there when passed in).
pub(crate) fn write_sfnt(version: u32, mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by(|a, b| a.0.cmp(&b.0));
    let n = tables.len();
    let es = if n == 0 { 0 } else { (usize::BITS - 1 - n.leading_zeros()) as u16 };
    let sr = (1u16 << es) * 16;
    let mut out = Vec::new();
    w32(&mut out, version);
    w16(&mut out, n as u16);
    w16(&mut out, sr);
    w16(&mut out, es);
    w16(&mut out, (n as u16 * 16).wrapping_sub(sr));
    let mut off = 12 + 16 * n;
    let mut head_off = None;
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        w32(&mut out, checksum(data));
        w32(&mut out, off as u32);
        w32(&mut out, data.len() as u32);
        if tag == b"head" {
            head_off = Some(off);
        }
        off += (data.len() + 3) & !3;
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    if let Some(h) = head_off {
        let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        put32(&mut out, h + 8, adj);
    }
    out
}
