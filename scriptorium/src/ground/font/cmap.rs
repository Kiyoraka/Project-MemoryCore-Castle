//! The `cmap` table: characters to glyph ids. Formats 4 and 12 (plus 0, 6 and 13 as fallbacks), from
//! the Windows Unicode, Unicode, Windows Symbol or Mac Roman encodings - best one chosen once.
//! All offsets here are relative to the start of the `cmap` table.

use super::names::{char_to_mac_roman, mac_roman_to_char};
use super::read::{rd16, rd32, rd8};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Unicode,
    /// Windows Symbol (3,0): codes are usually U+F0xx.
    Symbol,
    /// Mac Roman (1,0): codes are Mac Roman bytes.
    MacRoman,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Sub {
    pub off: usize,
    pub format: u16,
    pub kind: Kind,
}

/// Pick the best supported subtable.
pub(crate) fn choose(cmap: &[u8]) -> Option<Sub> {
    let n = rd16(cmap, 2)? as usize;
    let mut best: Option<(Sub, u8)> = None;
    for i in 0..n {
        let r = 4 + 8 * i;
        let (Some(plat), Some(enc), Some(off)) = (rd16(cmap, r), rd16(cmap, r + 2), rd32(cmap, r + 4)) else {
            break;
        };
        let off = off as usize;
        let Some(format) = rd16(cmap, off) else { continue };
        let unicode = plat == 0 || (plat == 3 && (enc == 1 || enc == 10));
        let score = match (format, plat, enc) {
            (12, 3, 10) => 100,
            (12, 0, 4) | (12, 0, 6) => 95,
            (12, _, _) if unicode => 90,
            (4, 3, 1) => 80,
            (4, _, _) if unicode => 75,
            (6, _, _) if unicode => 60,
            (0, _, _) if unicode => 50,
            (4, 3, 0) | (12, 3, 0) | (6, 3, 0) | (0, 3, 0) => 40,
            (0, 1, 0) | (6, 1, 0) | (4, 1, 0) => 30,
            (13, _, _) if unicode => 10,
            _ => 0,
        };
        if score == 0 {
            continue;
        }
        let kind = if plat == 3 && enc == 0 {
            Kind::Symbol
        } else if plat == 1 {
            Kind::MacRoman
        } else {
            Kind::Unicode
        };
        if best.map_or(true, |(_, s)| score > s) {
            best = Some((Sub { off, format, kind }, score));
        }
    }
    best.map(|(s, _)| s)
}

/// Glyph for a Unicode scalar, or None when unmapped (glyph 0 counts as unmapped).
pub(crate) fn lookup(cmap: &[u8], sub: Sub, c: u32) -> Option<u16> {
    let g = match sub.kind {
        Kind::Unicode => lookup_code(cmap, sub, c),
        Kind::Symbol => {
            let g = lookup_code(cmap, sub, c);
            if g == 0 && c < 0x100 {
                lookup_code(cmap, sub, 0xF000 + c)
            } else {
                g
            }
        }
        Kind::MacRoman => match char::from_u32(c).and_then(char_to_mac_roman) {
            Some(b) => lookup_code(cmap, sub, b as u32),
            None => 0,
        },
    };
    if g == 0 {
        None
    } else {
        Some(g)
    }
}

fn lookup_code(d: &[u8], s: Sub, c: u32) -> u16 {
    let o = s.off;
    match s.format {
        0 => {
            if c < 256 {
                rd8(d, o + 6 + c as usize).unwrap_or(0) as u16
            } else {
                0
            }
        }
        4 => format4(d, o, c).unwrap_or(0),
        6 => {
            let (Some(first), Some(count)) = (rd16(d, o + 6), rd16(d, o + 8)) else { return 0 };
            let first = first as u32;
            if c < first || c >= first + count as u32 {
                return 0;
            }
            rd16(d, o + 10 + 2 * (c - first) as usize).unwrap_or(0)
        }
        12 | 13 => format12(d, o, c, s.format == 13).unwrap_or(0),
        _ => 0,
    }
}

fn format4(d: &[u8], o: usize, c: u32) -> Option<u16> {
    if c > 0xFFFF {
        return Some(0);
    }
    let seg_x2 = rd16(d, o + 6)? as usize;
    let segs = seg_x2 / 2;
    let ends = o + 14;
    let starts = ends + seg_x2 + 2;
    let deltas = starts + seg_x2;
    let ranges = deltas + seg_x2;
    // Binary search for the first segment whose end >= c.
    let (mut lo, mut hi) = (0usize, segs);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if (rd16(d, ends + 2 * mid)? as u32) < c {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo >= segs {
        return Some(0);
    }
    let i = lo;
    let start = rd16(d, starts + 2 * i)? as u32;
    if c < start {
        return Some(0);
    }
    let delta = rd16(d, deltas + 2 * i)?;
    let ro = rd16(d, ranges + 2 * i)? as usize;
    if ro == 0 {
        Some((c as u16).wrapping_add(delta))
    } else {
        let addr = ranges + 2 * i + ro + 2 * (c - start) as usize;
        let g = rd16(d, addr)?;
        Some(if g == 0 { 0 } else { g.wrapping_add(delta) })
    }
}

fn format12(d: &[u8], o: usize, c: u32, many_to_one: bool) -> Option<u16> {
    let n = rd32(d, o + 12)? as usize;
    let groups = o + 16;
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let g = groups + 12 * mid;
        let start = rd32(d, g)?;
        let end = rd32(d, g + 4)?;
        if c < start {
            hi = mid;
        } else if c > end {
            lo = mid + 1;
        } else {
            let sg = rd32(d, g + 8)?;
            let gid = if many_to_one { sg } else { sg.wrapping_add(c - start) };
            return Some(if gid > 0xFFFF { 0 } else { gid as u16 });
        }
    }
    Some(0)
}

/// Visit every (code, glyph) pair of the subtable, codes converted to Unicode where the encoding
/// allows (Symbol codes are reported as stored). Glyph 0 entries are skipped.
pub(crate) fn for_each(d: &[u8], s: Sub, f: &mut dyn FnMut(u32, u16)) {
    let mut emit = |code: u32, g: u16| {
        if g == 0 {
            return;
        }
        let code = if s.kind == Kind::MacRoman && code < 256 { mac_roman_to_char(code as u8) as u32 } else { code };
        f(code, g)
    };
    let o = s.off;
    match s.format {
        0 => {
            for c in 0..256u32 {
                emit(c, rd8(d, o + 6 + c as usize).unwrap_or(0) as u16);
            }
        }
        4 => {
            let Some(seg_x2) = rd16(d, o + 6) else { return };
            let segs = seg_x2 as usize / 2;
            let ends = o + 14;
            let starts = ends + seg_x2 as usize + 2;
            for i in 0..segs {
                let (Some(end), Some(start)) = (rd16(d, ends + 2 * i), rd16(d, starts + 2 * i)) else { return };
                if start > end {
                    continue;
                }
                for c in start as u32..=end as u32 {
                    if c == 0xFFFF {
                        break;
                    }
                    emit(c, format4(d, o, c).unwrap_or(0));
                }
            }
        }
        6 => {
            let (Some(first), Some(count)) = (rd16(d, o + 6), rd16(d, o + 8)) else { return };
            for i in 0..count as u32 {
                emit(first as u32 + i, rd16(d, o + 10 + 2 * i as usize).unwrap_or(0));
            }
        }
        12 | 13 => {
            let Some(n) = rd32(d, o + 12) else { return };
            let mut budget: u32 = 0x11_0000;
            for i in 0..n as usize {
                let g = o + 16 + 12 * i;
                let (Some(start), Some(end), Some(sg)) = (rd32(d, g), rd32(d, g + 4), rd32(d, g + 8)) else { return };
                if start > end || end > 0x10_FFFF {
                    continue;
                }
                for c in start..=end {
                    if budget == 0 {
                        return;
                    }
                    budget -= 1;
                    let gid = if s.format == 13 { sg } else { sg.wrapping_add(c - start) };
                    if gid <= 0xFFFF {
                        emit(c, gid as u16);
                    }
                }
            }
        }
        _ => {}
    }
}
