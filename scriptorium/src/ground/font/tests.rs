//! Font tests: a tiny TrueType font and a tiny CFF font built byte by byte here, plus the real
//! system fonts when this machine has them (skipped silently otherwise).

use super::cff::{charstring_outline, Index};
use super::read::{put16, put32, w16, w32, wi16};
use super::subset::{build_cmap, build_name, write_sfnt};
use super::*;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- builders

fn head_table(upem: u16) -> Vec<u8> {
    let mut h = vec![0u8; 54];
    put32(&mut h, 0, 0x0001_0000);
    put32(&mut h, 12, 0x5F0F_3CF5);
    put16(&mut h, 18, upem);
    put16(&mut h, 36, 0);
    put16(&mut h, 38, (-10i16) as u16);
    put16(&mut h, 40, 700);
    put16(&mut h, 42, 800);
    h
}

fn hhea_table(n_hmetrics: u16) -> Vec<u8> {
    let mut h = vec![0u8; 36];
    put32(&mut h, 0, 0x0001_0000);
    put16(&mut h, 4, 800);
    put16(&mut h, 6, (-200i16) as u16);
    put16(&mut h, 8, 90);
    put16(&mut h, 34, n_hmetrics);
    h
}

fn hmtx_table(m: &[(u16, i16)]) -> Vec<u8> {
    let mut v = Vec::new();
    for &(a, l) in m {
        w16(&mut v, a);
        wi16(&mut v, l);
    }
    v
}

/// A simple glyph with every coordinate as a 16-bit delta.
fn simple_glyph(contours: &[&[(i16, i16, bool)]]) -> Vec<u8> {
    let mut v = Vec::new();
    let pts: Vec<(i16, i16, bool)> = contours.iter().flat_map(|c| c.iter().copied()).collect();
    wi16(&mut v, contours.len() as i16);
    let xs: Vec<i16> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<i16> = pts.iter().map(|p| p.1).collect();
    for b in [
        *xs.iter().min().unwrap(),
        *ys.iter().min().unwrap(),
        *xs.iter().max().unwrap(),
        *ys.iter().max().unwrap(),
    ] {
        wi16(&mut v, b);
    }
    let mut end = -1i32;
    for c in contours {
        end += c.len() as i32;
        w16(&mut v, end as u16);
    }
    w16(&mut v, 0); // no instructions
    for p in &pts {
        v.push(if p.2 { 1 } else { 0 });
    }
    let mut prev = 0i16;
    for p in &pts {
        wi16(&mut v, p.0 - prev);
        prev = p.0;
    }
    prev = 0;
    for p in &pts {
        wi16(&mut v, p.1 - prev);
        prev = p.1;
    }
    v
}

fn composite_glyph(gid: u16, dx: i16, dy: i16) -> Vec<u8> {
    let mut v = Vec::new();
    wi16(&mut v, -1);
    for b in [0i16, 0, 700, 800] {
        wi16(&mut v, b);
    }
    w16(&mut v, 0x0001 | 0x0002);
    w16(&mut v, gid);
    wi16(&mut v, dx);
    wi16(&mut v, dy);
    v
}

const TRI: &[(i16, i16, bool)] = &[(0, 0, true), (300, 700, true), (600, 0, true)];
const CURVY: &[(i16, i16, bool)] = &[(0, 0, true), (0, 500, false), (500, 500, false), (500, 0, true)];
const ALL_OFF: &[(i16, i16, bool)] = &[(100, 100, false), (200, 100, false), (200, 200, false)];
const BOX: &[(i16, i16, bool)] = &[(50, 0, true), (50, 700, true), (450, 700, true), (450, 0, true)];

fn os2_table() -> Vec<u8> {
    let mut o = vec![0u8; 96];
    put16(&mut o, 0, 4);
    put16(&mut o, 4, 700);
    put16(&mut o, 6, 5);
    put16(&mut o, 62, 1); // italic
    put16(&mut o, 68, 780);
    put16(&mut o, 70, (-220i16) as u16);
    put16(&mut o, 72, 0);
    put16(&mut o, 74, 900);
    put16(&mut o, 76, 300);
    put16(&mut o, 86, 480);
    put16(&mut o, 88, 690);
    o
}

fn post_table() -> Vec<u8> {
    let mut p = vec![0u8; 32];
    put32(&mut p, 0, 0x0003_0000);
    put32(&mut p, 4, (-12i32 * 65536) as u32);
    put16(&mut p, 8, (-100i16) as u16);
    put16(&mut p, 10, 50);
    p
}

fn kern_table() -> Vec<u8> {
    let mut k = Vec::new();
    w16(&mut k, 0);
    w16(&mut k, 1);
    w16(&mut k, 0); // subtable version
    w16(&mut k, 20); // length
    w16(&mut k, 0x0001); // format 0, horizontal
    w16(&mut k, 1); // nPairs
    w16(&mut k, 6);
    w16(&mut k, 0);
    w16(&mut k, 0);
    w16(&mut k, 1);
    w16(&mut k, 2);
    wi16(&mut k, -50);
    k
}

/// GPOS with the 'kern' feature: lookup 0 pair format 1 (2,1) = -80; lookup 1 pair format 2,
/// class(3) x class(1) = -30.
fn gpos_table() -> Vec<u8> {
    let mut g = Vec::new();
    w32(&mut g, 0x0001_0000);
    w16(&mut g, 10);
    w16(&mut g, 12);
    w16(&mut g, 28);
    assert_eq!(g.len(), 10);
    w16(&mut g, 0); // ScriptList (unused by us)
    assert_eq!(g.len(), 12);
    w16(&mut g, 1);
    g.extend_from_slice(b"kern");
    w16(&mut g, 8);
    assert_eq!(g.len(), 20);
    w16(&mut g, 0);
    w16(&mut g, 2);
    w16(&mut g, 0);
    w16(&mut g, 1);
    assert_eq!(g.len(), 28);
    w16(&mut g, 2);
    w16(&mut g, 6);
    w16(&mut g, 14);
    assert_eq!(g.len(), 34);
    for sub_off in [16u16, 32] {
        w16(&mut g, 2);
        w16(&mut g, 0);
        w16(&mut g, 1);
        w16(&mut g, sub_off);
    }
    assert_eq!(g.len(), 50);
    // PairPos format 1
    for v in [1u16, 12, 4, 0, 1, 18] {
        w16(&mut g, v);
    }
    for v in [1u16, 1, 2] {
        w16(&mut g, v);
    }
    w16(&mut g, 1);
    w16(&mut g, 1);
    wi16(&mut g, -80);
    assert_eq!(g.len(), 74);
    // PairPos format 2
    for v in [2u16, 24, 4, 0, 30, 38, 2, 2] {
        w16(&mut g, v);
    }
    for v in [0i16, 0, 0, -30] {
        wi16(&mut g, v);
    }
    assert_eq!(g.len(), 98);
    for v in [1u16, 1, 3] {
        w16(&mut g, v);
    }
    for v in [1u16, 3, 1, 1] {
        w16(&mut g, v);
    }
    for v in [2u16, 1, 1, 1, 1] {
        w16(&mut g, v);
    }
    assert_eq!(g.len(), 122);
    g
}

fn tiny_ttf(with_gpos: bool) -> Vec<u8> {
    let glyphs = vec![
        simple_glyph(&[BOX]),
        simple_glyph(&[TRI]),
        simple_glyph(&[CURVY, ALL_OFF]),
        composite_glyph(1, 50, 10),
        Vec::new(),
    ];
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for g in &glyphs {
        w16(&mut loca, (glyf.len() / 2) as u16);
        glyf.extend_from_slice(g);
        while glyf.len() % 4 != 0 {
            glyf.push(0);
        }
    }
    w16(&mut loca, (glyf.len() / 2) as u16);
    let mut maxp = vec![0u8; 32];
    put32(&mut maxp, 0, 0x0001_0000);
    put16(&mut maxp, 4, glyphs.len() as u16);
    let mut tables = vec![
        (*b"head", head_table(1000)),
        (*b"hhea", hhea_table(5)),
        (*b"maxp", maxp),
        (*b"hmtx", hmtx_table(&[(500, 50), (600, 0), (700, 0), (650, 50), (250, 0)])),
        (*b"loca", loca),
        (*b"glyf", glyf),
        (*b"cmap", build_cmap(&[(0x20, 4), (0x41, 1), (0x42, 2), (0x43, 3)], false)),
        (*b"OS/2", os2_table()),
        (*b"post", post_table()),
        (*b"name", build_name(&[(1, "Tiny"), (2, "Bold Italic"), (4, "Tiny Bold Italic"), (6, "Tiny-BoldItalic")])),
    ];
    if with_gpos {
        tables.push((*b"GPOS", gpos_table()));
    } else {
        tables.push((*b"kern", kern_table()));
    }
    write_sfnt(0x0001_0000, tables)
}

fn cs_num(v: i32) -> Vec<u8> {
    if (-107..=107).contains(&v) {
        vec![(v + 139) as u8]
    } else {
        let mut o = vec![28];
        o.extend_from_slice(&(v as i16).to_be_bytes());
        o
    }
}

fn cs(parts: &[&[i32]], ops: &[&[u8]]) -> Vec<u8> {
    let mut o = Vec::new();
    for (nums, op) in parts.iter().zip(ops) {
        for &n in *nums {
            o.extend(cs_num(n));
        }
        o.extend_from_slice(op);
    }
    o
}

fn cff_index(items: &[Vec<u8>]) -> Vec<u8> {
    let mut o = Vec::new();
    w16(&mut o, items.len() as u16);
    if items.is_empty() {
        return o;
    }
    o.push(2);
    let mut off = 1u16;
    w16(&mut o, off);
    for it in items {
        off += it.len() as u16;
        w16(&mut o, off);
    }
    for it in items {
        o.extend_from_slice(it);
    }
    o
}

fn cff_int(v: i32) -> Vec<u8> {
    let mut o = vec![29];
    o.extend_from_slice(&v.to_be_bytes());
    o
}

/// Glyph 1: width 500, triangle with one edge from a local subroutine and a closing cubic.
fn tiny_cff() -> Vec<u8> {
    let g1 = cs(
        &[&[500, 100, 0], &[400, 0], &[-107], &[-100, -300, -50, -200, -50, -100], &[]],
        &[&[21], &[5], &[10], &[8], &[14]],
    );
    let subr0 = cs(&[&[-200, 600], &[]], &[&[5], &[11]]);
    let name_idx = cff_index(&[b"T".to_vec()]);
    let cs_pos = 4 + name_idx.len() + 24 + 2 + 2;
    let cs_idx = cff_index(&[vec![14], g1]);
    let priv_pos = cs_pos + cs_idx.len();
    let mut private = cff_int(6);
    private.push(19);
    let subrs = cff_index(&[subr0]);
    let mut top = cff_int(cs_pos as i32);
    top.push(17);
    top.extend(cff_int(private.len() as i32));
    top.extend(cff_int(priv_pos as i32));
    top.push(18);
    let top_idx = cff_index(&[top]);
    assert_eq!(top_idx.len(), 24);
    let mut out = vec![1, 0, 4, 4];
    out.extend(name_idx);
    out.extend(top_idx);
    out.extend(cff_index(&[]));
    out.extend(cff_index(&[]));
    assert_eq!(out.len(), cs_pos);
    out.extend(cs_idx);
    out.extend(private);
    out.extend(subrs);
    out
}

fn tiny_otf() -> Vec<u8> {
    let maxp = vec![0, 0, 0x50, 0, 0, 2];
    write_sfnt(
        0x4F54_544F,
        vec![
            (*b"head", head_table(1000)),
            (*b"hhea", hhea_table(2)),
            (*b"maxp", maxp),
            (*b"hmtx", hmtx_table(&[(500, 0), (500, 100)])),
            (*b"cmap", build_cmap(&[(0x41, 1)], false)),
            (*b"name", build_name(&[(1, "TinyCFF"), (2, "Regular")])),
            (*b"CFF ", tiny_cff()),
        ],
    )
}

use PathOp::*;

// ---------------------------------------------------------------- tiny TrueType

#[test]
fn tiny_parses_names_style_metrics() {
    let f = Font::parse(tiny_ttf(false), 0).unwrap();
    assert_eq!(f.num_glyphs(), 5);
    assert!(!f.is_cff());
    let n = f.names();
    assert_eq!(n.family, "Tiny");
    assert_eq!(n.subfamily, "Bold Italic");
    assert_eq!(n.full_name, "Tiny Bold Italic");
    assert_eq!(n.postscript_name, "Tiny-BoldItalic");
    assert_eq!(f.style(), Style { weight: 700, italic: true, stretch: 5 });
    let m = f.metrics();
    assert_eq!(m.units_per_em, 1000);
    assert_eq!((m.ascender, m.descender, m.line_gap), (800, -200, 90));
    assert_eq!((m.typo_ascender, m.typo_descender, m.win_ascent, m.win_descent), (780, -220, 900, 300));
    assert_eq!((m.x_height, m.cap_height), (480, 690));
    assert_eq!((m.underline_pos, m.underline_thickness), (-100, 50));
    assert_eq!(m.italic_angle, -12.0);
    assert_eq!(m.bbox, [0, -10, 700, 800]);
    assert_eq!(Font::face_count(f.data()), 1);
    assert!(Font::parse(tiny_ttf(false), 1).is_err());
}

#[test]
fn tiny_cmap_and_advances() {
    let f = Font::parse(tiny_ttf(false), 0).unwrap();
    assert_eq!(f.glyph_id('A'), Some(1));
    assert_eq!(f.glyph_id('B'), Some(2));
    assert_eq!(f.glyph_id('C'), Some(3));
    assert_eq!(f.glyph_id(' '), Some(4));
    assert_eq!(f.glyph_id('Z'), None);
    assert_eq!(f.glyph_id('\u{1F600}'), None);
    assert_eq!([0, 1, 2, 3, 4].map(|g| f.advance(g)), [500, 600, 700, 650, 250]);
    assert_eq!(f.left_side_bearing(3), 50);
    assert_eq!(f.advance(99), 0);
    let mut maps = f.mappings();
    maps.sort();
    assert_eq!(maps, vec![(' ', 4), ('A', 1), ('B', 2), ('C', 3)]);
}

#[test]
fn tiny_outlines_simple_quadratic_and_composite() {
    let f = Font::parse(tiny_ttf(false), 0).unwrap();
    assert_eq!(
        f.outline(1).unwrap(),
        vec![MoveTo(0.0, 0.0), LineTo(300.0, 700.0), LineTo(600.0, 0.0), Close]
    );
    let b = f.outline(2).unwrap();
    assert_eq!(
        b,
        vec![
            MoveTo(0.0, 0.0),
            QuadTo(0.0, 500.0, 250.0, 500.0),
            QuadTo(500.0, 500.0, 500.0, 0.0),
            Close,
            MoveTo(150.0, 150.0),
            QuadTo(100.0, 100.0, 150.0, 100.0),
            QuadTo(200.0, 100.0, 200.0, 150.0),
            QuadTo(200.0, 200.0, 150.0, 150.0),
            Close,
        ]
    );
    assert_eq!(
        f.outline(3).unwrap(),
        vec![MoveTo(50.0, 10.0), LineTo(350.0, 710.0), LineTo(650.0, 10.0), Close]
    );
    assert_eq!(f.outline(4), Some(vec![]));
    assert_eq!(f.outline(99), None);
    assert_eq!(f.glyph_bounds(1), Some([0.0, 0.0, 600.0, 700.0]));
}

#[test]
fn tiny_kern_table_pairs() {
    let f = Font::parse(tiny_ttf(false), 0).unwrap();
    assert_eq!(f.kerning(1, 2), -50);
    assert_eq!(f.kerning(2, 1), 0);
    assert_eq!(f.kerning(1, 1), 0);
}

#[test]
fn gpos_pair_formats_1_and_2() {
    let f = Font::parse(tiny_ttf(true), 0).unwrap();
    assert_eq!(f.kerning(2, 1), -80);
    assert_eq!(f.kerning(3, 1), -30);
    assert_eq!(f.kerning(3, 2), 0);
    assert_eq!(f.kerning(1, 2), 0);
}

#[test]
fn cmap_format4_with_range_offsets_and_format12() {
    // format 4: one segment 0x41..=0x43 through glyphIdArray [7, 8, 9], then the 0xFFFF segment.
    let mut t = Vec::new();
    for v in [0u16, 1, 3, 1] {
        w16(&mut t, v);
    }
    w32(&mut t, 12);
    let mut f4 = Vec::new();
    for v in [4u16, 0, 0, 4, 4, 1, 0] {
        w16(&mut f4, v);
    }
    for v in [0x43u16, 0xFFFF, 0, 0x41, 0xFFFF, 0, 1, 4, 0, 7, 8, 9] {
        w16(&mut f4, v);
    }
    let f4_len = f4.len() as u16;
    put16(&mut f4, 2, f4_len);
    t.extend(f4);
    let sub = cmap::choose(&t).unwrap();
    assert_eq!(sub.format, 4);
    assert_eq!(cmap::lookup(&t, sub, 0x41), Some(7));
    assert_eq!(cmap::lookup(&t, sub, 0x43), Some(9));
    assert_eq!(cmap::lookup(&t, sub, 0x44), None);
    assert_eq!(cmap::lookup(&t, sub, 0x40), None);

    let pairs = [(0x41u32, 1u16), (0x1F600, 10), (0x1F601, 11), (0x1F602, 12)];
    let c = build_cmap(&pairs, false);
    let sub = cmap::choose(&c).unwrap();
    assert_eq!(sub.format, 12);
    assert_eq!(cmap::lookup(&c, sub, 0x1F601), Some(11));
    assert_eq!(cmap::lookup(&c, sub, 0x41), Some(1));
    assert_eq!(cmap::lookup(&c, sub, 0x1F603), None);
}

#[test]
fn subset_keeps_ids_and_outlines() {
    let f = Font::parse(tiny_ttf(false), 0).unwrap();
    let bytes = subset_truetype(&f, &[3]);
    let s = Font::parse(bytes, 0).unwrap();
    assert_eq!(s.num_glyphs(), 4);
    assert_eq!(s.outline(3), f.outline(3));
    assert_eq!(s.outline(1), f.outline(1)); // composite component kept
    assert_eq!(s.outline(0), f.outline(0)); // .notdef kept
    assert_eq!(s.outline(2), Some(vec![])); // dropped glyph is empty
    assert_eq!(s.advance(3), 650);
    assert_eq!(s.advance(2), 0);
    assert_eq!(s.glyph_id('C'), Some(3));
    assert_eq!(s.glyph_id('A'), Some(1));
    assert_eq!(s.glyph_id('B'), None);
    assert_eq!(s.names().family, "Tiny");
    assert_eq!(s.style().weight, 700);
    assert_eq!(s.metrics().italic_angle, -12.0);
}

#[test]
fn subset_compact_renumbers_components() {
    let f = Font::parse(tiny_ttf(false), 0).unwrap();
    let (bytes, map) = subset_truetype_compact(&f, &[2, 3, 2]);
    assert_eq!(map, vec![0, 2, 3, 1]);
    let s = Font::parse(bytes, 0).unwrap();
    assert_eq!(s.num_glyphs(), 4);
    for (new, &old) in map.iter().enumerate() {
        assert_eq!(s.outline(new as u16), f.outline(old), "glyph {old}");
        assert_eq!(s.advance(new as u16), f.advance(old));
    }
    assert_eq!(s.glyph_id('A'), Some(3));
    assert_eq!(s.glyph_id('C'), Some(2));
}

#[test]
fn malformed_fonts_never_panic() {
    for gpos in [false, true] {
        let good = tiny_ttf(gpos);
        let exercise = |f: &Font| {
            for c in ['A', 'B', 'C', ' ', 'Z'] {
                let _ = f.glyph_id(c);
            }
            for g in 0..8u16 {
                let _ = f.advance(g);
                let _ = f.outline(g);
                for h in 0..8u16 {
                    let _ = f.kerning(g, h);
                }
            }
            let _ = f.mappings();
            let _ = subset_truetype(f, &[1, 2, 3]);
            let _ = subset_truetype_compact(f, &[3]);
        };
        for cut in 0..good.len() {
            if let Ok(f) = Font::parse(good[..cut].to_vec(), 0) {
                exercise(&f);
            }
        }
        let mut seed = 0x1234_5678u32;
        for _ in 0..400 {
            let mut bad = good.clone();
            for _ in 0..8 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let i = (seed >> 8) as usize % bad.len();
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                bad[i] = (seed >> 16) as u8;
            }
            if let Ok(f) = Font::parse(bad, 0) {
                exercise(&f);
            }
        }
    }
    let otf = tiny_otf();
    for cut in 0..otf.len() {
        if let Ok(f) = Font::parse(otf[..cut].to_vec(), 0) {
            let _ = f.outline(1);
        }
    }
    assert!(Font::parse(Vec::new(), 0).is_err());
    assert!(Font::parse(b"not a font at all".to_vec(), 0).is_err());
}

#[test]
fn mac_roman_table_is_complete() {
    assert_eq!(names::MAC_ROMAN_HIGH.chars().count(), 128);
    assert_eq!(names::mac_roman_to_char(0x80), 'Ä');
    assert_eq!(names::char_to_mac_roman('é'), Some(0x8E));
}

// ---------------------------------------------------------------- tiny CFF

#[test]
fn tiny_cff_outline_with_subroutine() {
    let f = Font::parse(tiny_otf(), 0).unwrap();
    assert!(f.is_cff());
    assert!(!f.has_truetype_outlines());
    assert_eq!(f.glyph_id('A'), Some(1));
    let expect = vec![
        MoveTo(100.0, 0.0),
        LineTo(500.0, 0.0),
        LineTo(300.0, 600.0),
        CubicTo(200.0, 300.0, 150.0, 100.0, 100.0, 0.0),
        Close,
    ];
    assert_eq!(f.outline(1).unwrap(), expect);
    assert_eq!(f.outline(0), Some(vec![]));
    assert_eq!(f.names().family, "TinyCFF");
    // CFF subsetting is a later TODO: the whole face comes back, still parseable.
    let s = Font::parse(subset_truetype(&f, &[1]), 0).unwrap();
    assert_eq!(s.outline(1).unwrap(), expect);
}

#[test]
fn charstring_hints_and_curve_shorthands() {
    let prog = cs(
        &[&[10, 20, 30, 40], &[50, 60], &[], &[100, 200], &[10, 20, 30, 40], &[10, 20, 30, 40, 5], &[]],
        &[&[1], &[3], &[19, 0xFF], &[21], &[31], &[30], &[14]],
    );
    let ops = charstring_outline(&[], &prog, Index::default(), None).unwrap();
    assert_eq!(
        ops,
        vec![
            MoveTo(100.0, 200.0),
            CubicTo(110.0, 200.0, 130.0, 230.0, 130.0, 270.0),
            CubicTo(130.0, 280.0, 150.0, 310.0, 190.0, 315.0),
            Close,
        ]
    );
    // hhcurveto with a leading dy, vvcurveto with a leading dx, flex.
    let prog = cs(
        &[&[0, 0], &[5, 10, 20, 30, 40], &[5, 10, 20, 30, 40], &[10, 0, 10, 0, 10, 0, 10, 0, 10, 0, 10, 0, 50], &[]],
        &[&[21], &[27], &[26], &[12, 35], &[14]],
    );
    let ops = charstring_outline(&[], &prog, Index::default(), None).unwrap();
    assert_eq!(ops[1], CubicTo(10.0, 5.0, 30.0, 35.0, 70.0, 35.0));
    assert_eq!(ops[2], CubicTo(75.0, 45.0, 95.0, 75.0, 95.0, 115.0));
    assert_eq!(ops.len(), 6);
    // Recursion into a missing subroutine is an error, not a panic.
    let bad = cs(&[&[0], &[]], &[&[10], &[14]]);
    assert_eq!(charstring_outline(&[], &bad, Index::default(), None), None);
}

// ---------------------------------------------------------------- system fonts (if present)

fn win_font(name: &str) -> Option<Vec<u8>> {
    let dir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    std::fs::read(dir.join("Fonts").join(name)).ok()
}

#[test]
fn system_arial() {
    let Some(data) = win_font("arial.ttf") else { return };
    let f = Font::parse(data, 0).unwrap();
    assert_eq!(f.names().family, "Arial");
    let m = f.metrics();
    assert_eq!(m.units_per_em, 2048);
    assert!(m.ascender > 1500 && m.descender < -300, "{m:?}");
    assert!(m.x_height > 900 && m.cap_height > 1300, "{m:?}");
    let a = f.glyph_id('A').unwrap();
    let adv = f.advance(a) as f32 / m.units_per_em as f32;
    assert!((0.5..0.8).contains(&adv), "advance {adv}");
    let o = f.outline(a).unwrap();
    assert!(o.len() > 4 && matches!(o[0], MoveTo(..)));
    let v = f.glyph_id('V').unwrap();
    assert!(f.kerning(a, v) < 0, "A-V kerning {}", f.kerning(a, v));
    // Every glyph outlines without trouble.
    for g in 0..f.num_glyphs() {
        let _ = f.outline(g);
    }
    // Subset with an accented (composite) letter round-trips.
    let aacute = f.glyph_id('Á').unwrap();
    let s = Font::parse(subset_truetype(&f, &[a, aacute, v]), 0).unwrap();
    for g in [a, aacute, v] {
        assert_eq!(s.outline(g), f.outline(g));
        assert_eq!(s.advance(g), f.advance(g));
    }
    assert_eq!(s.glyph_id('Á'), Some(aacute));
    let (bytes, map) = subset_truetype_compact(&f, &[aacute]);
    let c = Font::parse(bytes, 0).unwrap();
    assert_eq!(c.outline(1), f.outline(aacute));
    assert_eq!(map[1], aacute);
}

#[test]
fn system_segoe_ui() {
    let Some(data) = win_font("segoeui.ttf") else { return };
    let f = Font::parse(data, 0).unwrap();
    assert_eq!(f.names().family, "Segoe UI");
    let a = f.glyph_id('A').unwrap();
    assert!(!f.outline(a).unwrap().is_empty());
    let adv = f.advance(a) as f32 / f.units_per_em() as f32;
    assert!((0.4..0.9).contains(&adv), "advance {adv}");
    assert_eq!(f.style().weight, 400);
    assert!(!f.style().italic);
}

#[test]
fn system_collection_by_index() {
    for file in ["msgothic.ttc", "simsun.ttc"] {
        let Some(data) = win_font(file) else { continue };
        let n = Font::face_count(&data);
        assert!(n >= 2, "{file} has {n} faces");
        let f0 = Font::parse(data.clone(), 0).unwrap();
        let f1 = Font::parse(data.clone(), 1).unwrap();
        assert_ne!(f0.names().full_name, f1.names().full_name);
        let ch = if file == "msgothic.ttc" { 'あ' } else { '中' };
        for f in [&f0, &f1] {
            let g = f.glyph_id(ch).unwrap();
            assert!(!f.outline(g).unwrap().is_empty());
            assert!(f.advance(g) > 0);
        }
        assert!(Font::parse(data, n).is_err());
        // Subset of a collection member is a standalone font.
        let g = f1.glyph_id(ch).unwrap();
        let s = Font::parse(subset_truetype(&f1, &[g]), 0).unwrap();
        assert_eq!(s.outline(g), f1.outline(g));
    }
}

#[test]
fn installed_cff_fonts_outline() {
    let mut dirs = vec![];
    if let Some(w) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(w).join("Fonts"));
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
    }
    let mut checked = 0;
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if !p.extension().map_or(false, |x| x.eq_ignore_ascii_case("otf")) {
                continue;
            }
            let Ok(data) = std::fs::read(&p) else { continue };
            let Ok(f) = Font::parse(data, 0) else { continue };
            if !f.is_cff() {
                continue;
            }
            let Some(a) = f.glyph_id('A') else { continue };
            let o = f.outline(a).unwrap_or_else(|| panic!("{} 'A' did not outline", p.display()));
            assert!(o.iter().any(|op| matches!(op, CubicTo(..) | LineTo(..))), "{}", p.display());
            // The outline sits inside the font's bounding box (with slack for sloppy bboxes).
            let b = bounds(&o).unwrap();
            let m = f.metrics();
            let slack = m.units_per_em as f32 * 0.5;
            assert!(
                b[0] >= m.bbox[0] as f32 - slack
                    && b[1] >= m.bbox[1] as f32 - slack
                    && b[2] <= m.bbox[2] as f32 + slack
                    && b[3] <= m.bbox[3] as f32 + slack
                    && b[3] > b[1],
                "{} 'A' bounds {b:?} vs bbox {:?}",
                p.display(),
                m.bbox
            );
            for g in 0..f.num_glyphs().min(400) {
                let _ = f.outline(g);
            }
            checked += 1;
            if checked >= 4 {
                return;
            }
        }
    }
}

#[test]
fn system_font_index_finds_and_falls_back() {
    if win_font("arial.ttf").is_none() || win_font("arialbd.ttf").is_none() {
        return;
    }
    let idx = FontIndex::system();
    assert!(idx.families().iter().any(|f| f == "Arial"));
    let bold = idx.find("Arial", 700, false).unwrap();
    let f = idx.load(&bold).unwrap();
    assert_eq!(f.names().family, "Arial");
    assert!(f.style().weight >= 700 && !f.style().italic, "{:?}", f.style());
    let regular = idx.find("arial", 400, false).unwrap();
    assert_eq!(idx.load(&regular).unwrap().style().weight, 400);
    let italic = idx.find("Arial", 400, true).unwrap();
    assert!(idx.load(&italic).unwrap().style().italic);
    // Nearest weight: 600 lands on bold, 300 on regular.
    assert!(idx.load(&idx.find("Arial", 600, false).unwrap()).unwrap().style().weight >= 700);
    assert_eq!(idx.load(&idx.find("Arial", 300, false).unwrap()).unwrap().style().weight, 400);
    assert!(idx.find("No Such Family 12345", 400, false).is_none());
    // Latin text stays in Arial; CJK falls back to a covering face.
    assert_eq!(idx.fallback_for('A', &["Arial"]), Some(regular));
    if win_font("msgothic.ttc").is_some() || win_font("msyh.ttc").is_some() {
        let r = idx.fallback_for('あ', &["Arial"]).unwrap();
        assert!(idx.covers(&r, 'あ'));
        assert!(idx.load(&r).unwrap().glyph_id('あ').is_some());
    }
    if win_font("malgun.ttf").is_some() {
        let r = idx.fallback_for('한', &[]).unwrap();
        assert!(idx.covers(&r, '한'));
    }
}

#[test]
fn font_index_on_a_built_font_file() {
    let dir = std::env::temp_dir().join(format!("scriptorium-font-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path: PathBuf = dir.join("tiny.ttf");
    std::fs::write(&path, tiny_ttf(false)).unwrap();
    let mut idx = FontIndex::new();
    idx.scan_dir(Path::new(&dir));
    assert_eq!(idx.faces().len(), 1);
    let r = idx.find("Tiny", 700, true).unwrap();
    assert_eq!(r, FontRef { path: path.clone(), index: 0 });
    assert_eq!(idx.find("Tiny Bold Italic", 400, false), Some(r.clone()));
    assert!(idx.covers(&r, 'B'));
    assert!(!idx.covers(&r, 'Z'));
    assert_eq!(idx.fallback_for('C', &["Tiny"]), Some(r.clone()));
    assert_eq!(idx.fallback_for('Z', &["Tiny"]), None);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}
