//! Bare CFF font programs (a PDF's FontFile3 /Type1C and /CIDFontType0C): the glyph names or CIDs
//! (charset), the built-in encoding, and a wrapper that dresses the CFF as a minimal OpenType file so
//! `ground::font::Font` can draw its outlines.

use super::encoding::{cff_standard_string, standard_encoding};
use std::collections::HashMap;

pub struct CffInfo {
    pub num_glyphs: u16,
    /// Glyph names by glyph id (empty for CID-keyed fonts).
    pub names: Vec<String>,
    /// CIDs by glyph id, for CID-keyed fonts.
    pub cids: Option<Vec<u16>>,
    /// Code to glyph id from the font's own encoding (0 = none).
    pub builtin: Vec<u16>,
    /// Units per em from the FontMatrix.
    pub upem: f64,
}

impl CffInfo {
    pub fn gid_for_name(&self, name: &str) -> Option<u16> {
        self.names.iter().position(|n| n == name).map(|i| i as u16)
    }

    /// CID to glyph id (identity when the font is not CID-keyed).
    pub fn cid_map(&self) -> Option<HashMap<u32, u16>> {
        self.cids.as_ref().map(|c| c.iter().enumerate().map(|(gid, &cid)| (cid as u32, gid as u16)).collect())
    }
}

fn rd8(d: &[u8], o: usize) -> Option<usize> {
    d.get(o).map(|&v| v as usize)
}

fn rd16(d: &[u8], o: usize) -> Option<usize> {
    Some(((*d.get(o)? as usize) << 8) | *d.get(o + 1)? as usize)
}

struct Index {
    count: usize,
    off_size: usize,
    offsets: usize,
    base: usize,
}

fn index(d: &[u8], pos: usize) -> Option<(Index, usize)> {
    let count = rd16(d, pos)?;
    if count == 0 {
        return Some((Index { count: 0, off_size: 1, offsets: pos + 2, base: pos + 2 }, pos + 2));
    }
    let off_size = rd8(d, pos + 2)?;
    if !(1..=4).contains(&off_size) {
        return None;
    }
    let offsets = pos + 3;
    let base = offsets + (count + 1) * off_size - 1;
    let idx = Index { count, off_size, offsets, base };
    let end = base.checked_add(idx.offset(d, count)?)?;
    Some((idx, end))
}

impl Index {
    fn offset(&self, d: &[u8], i: usize) -> Option<usize> {
        let p = self.offsets + i * self.off_size;
        let mut v = 0usize;
        for k in 0..self.off_size {
            v = (v << 8) | rd8(d, p + k)?;
        }
        Some(v)
    }

    fn get<'a>(&self, d: &'a [u8], i: usize) -> Option<&'a [u8]> {
        if i >= self.count {
            return None;
        }
        let a = self.offset(d, i)?;
        let b = self.offset(d, i + 1)?;
        if a == 0 || b < a {
            return None;
        }
        d.get(self.base.checked_add(a)?..self.base.checked_add(b)?)
    }
}

/// A Top DICT as operator -> operands (escaped operators are 1200 + byte).
fn dict(d: &[u8]) -> HashMap<u16, Vec<f64>> {
    let mut out = HashMap::new();
    let mut ops: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < d.len() {
        let b = d[i];
        match b {
            0..=21 => {
                let op = if b == 12 {
                    i += 1;
                    1200 + *d.get(i).unwrap_or(&0) as u16
                } else {
                    b as u16
                };
                out.insert(op, std::mem::take(&mut ops));
                i += 1;
            }
            28 => {
                let v = ((*d.get(i + 1).unwrap_or(&0) as i16) << 8 | *d.get(i + 2).unwrap_or(&0) as i16) as f64;
                ops.push(v);
                i += 3;
            }
            29 => {
                let mut v: i32 = 0;
                for k in 1..=4 {
                    v = (v << 8) | *d.get(i + k).unwrap_or(&0) as i32;
                }
                ops.push(v as f64);
                i += 5;
            }
            30 => {
                // Real: nibbles.
                let mut s = String::new();
                i += 1;
                'outer: while i < d.len() {
                    for nib in [d[i] >> 4, d[i] & 15] {
                        match nib {
                            0..=9 => s.push((b'0' + nib) as char),
                            0xA => s.push('.'),
                            0xB => s.push('E'),
                            0xC => s.push_str("E-"),
                            0xE => s.push('-'),
                            0xF => {
                                i += 1;
                                break 'outer;
                            }
                            _ => {}
                        }
                    }
                    i += 1;
                }
                ops.push(s.parse().unwrap_or(0.0));
            }
            32..=246 => {
                ops.push(b as f64 - 139.0);
                i += 1;
            }
            247..=250 => {
                ops.push((b as f64 - 247.0) * 256.0 + *d.get(i + 1).unwrap_or(&0) as f64 + 108.0);
                i += 2;
            }
            251..=254 => {
                ops.push(-(b as f64 - 251.0) * 256.0 - *d.get(i + 1).unwrap_or(&0) as f64 - 108.0);
                i += 2;
            }
            _ => i += 1,
        }
        if ops.len() > 48 {
            ops.clear();
        }
    }
    out
}

/// Read what Folio needs from a bare CFF.
pub fn parse(d: &[u8]) -> Option<CffInfo> {
    if rd8(d, 0)? != 1 {
        return None;
    }
    let hdr = rd8(d, 2)?;
    let (_names, p) = index(d, hdr)?;
    let (tops, p) = index(d, p)?;
    let (strings, _) = index(d, p)?;
    let top = dict(tops.get(d, 0)?);
    let off = |k: u16| top.get(&k).and_then(|v| v.first().copied()).map(|v| v.max(0.0) as usize);
    let cs = off(17)?;
    let (charstrings, _) = index(d, cs)?;
    let n = charstrings.count.min(65535);
    let upem = match top.get(&1207) {
        Some(m) if m.len() >= 1 && m[0].abs() > 1e-7 => (1.0 / m[0].abs()).clamp(16.0, 16384.0),
        _ => 1000.0,
    };
    let cid_keyed = top.contains_key(&1230);
    let string = |sid: usize| -> String {
        if sid < 391 {
            cff_standard_string(sid).map(str::to_string).unwrap_or_else(|| format!("sid{}", sid))
        } else {
            strings.get(d, sid - 391).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
        }
    };
    // Charset: a SID (or CID) per glyph.
    let charset_off = off(15).unwrap_or(0);
    let mut sids: Vec<usize> = vec![0; n];
    if charset_off > 2 {
        let fmt = rd8(d, charset_off).unwrap_or(0);
        let mut gid = 1;
        let mut p = charset_off + 1;
        match fmt {
            0 => {
                while gid < n {
                    let Some(s) = rd16(d, p) else { break };
                    sids[gid] = s;
                    gid += 1;
                    p += 2;
                }
            }
            1 | 2 => {
                while gid < n {
                    let Some(first) = rd16(d, p) else { break };
                    let left = if fmt == 1 { rd8(d, p + 2) } else { rd16(d, p + 2) };
                    let Some(left) = left else { break };
                    p += if fmt == 1 { 3 } else { 4 };
                    for k in 0..=left {
                        if gid >= n {
                            break;
                        }
                        sids[gid] = first + k;
                        gid += 1;
                    }
                }
            }
            _ => {}
        }
    } else {
        for (i, s) in sids.iter_mut().enumerate() {
            *s = i;
        }
    }
    let (names, cids) = if cid_keyed {
        (Vec::new(), Some(sids.iter().map(|&s| s.min(65535) as u16).collect::<Vec<u16>>()))
    } else {
        (sids.iter().map(|&s| string(s)).collect::<Vec<String>>(), None)
    };
    // Built-in encoding.
    let mut builtin = vec![0u16; 256];
    if !cid_keyed {
        let enc_off = off(16).unwrap_or(0);
        if enc_off == 0 {
            let std = standard_encoding();
            for (code, name) in std.iter().enumerate() {
                if let Some(name) = name {
                    if let Some(g) = names.iter().position(|n| n == name) {
                        builtin[code] = g as u16;
                    }
                }
            }
        } else if enc_off > 1 {
            let fmt = rd8(d, enc_off).unwrap_or(0);
            let mut p = enc_off + 1;
            match fmt & 0x7F {
                0 => {
                    let nc = rd8(d, p).unwrap_or(0);
                    p += 1;
                    for k in 0..nc {
                        if let Some(code) = rd8(d, p + k) {
                            if k + 1 < n {
                                builtin[code] = (k + 1) as u16;
                            }
                        }
                    }
                    p += nc;
                }
                1 => {
                    let nr = rd8(d, p).unwrap_or(0);
                    p += 1;
                    let mut gid = 1usize;
                    for _ in 0..nr {
                        let (Some(first), Some(left)) = (rd8(d, p), rd8(d, p + 1)) else { break };
                        p += 2;
                        for k in 0..=left {
                            let code = first + k;
                            if code < 256 && gid < n {
                                builtin[code] = gid as u16;
                            }
                            gid += 1;
                        }
                    }
                }
                _ => {}
            }
            if fmt & 0x80 != 0 {
                let ns = rd8(d, p).unwrap_or(0);
                p += 1;
                for _ in 0..ns {
                    let (Some(code), Some(sid)) = (rd8(d, p), rd16(d, p + 1)) else { break };
                    p += 3;
                    let name = string(sid);
                    if let Some(g) = names.iter().position(|nm| *nm == name) {
                        builtin[code] = g as u16;
                    }
                }
            }
        }
    }
    Some(CffInfo { num_glyphs: n as u16, names, cids, builtin, upem })
}

/// A minimal OpenType ('OTTO') file holding the CFF, so `Font::parse` reads its outlines.
pub fn wrap_otf(cff: &[u8], num_glyphs: u16, upem: f64) -> Vec<u8> {
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head[18..20].copy_from_slice(&(upem.round().clamp(16.0, 16384.0) as u16).to_be_bytes());
    let mut maxp = vec![0u8; 6];
    maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&num_glyphs.to_be_bytes());
    let tables: [(&[u8; 4], &[u8]); 3] = [(b"CFF ", cff), (b"head", &head), (b"maxp", &maxp)];
    let mut out = Vec::with_capacity(cff.len() + 128);
    out.extend_from_slice(b"OTTO");
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&[0, 32, 0, 1, 0, 16]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in tables.iter() {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    out.extend_from_slice(&body);
    out
}
