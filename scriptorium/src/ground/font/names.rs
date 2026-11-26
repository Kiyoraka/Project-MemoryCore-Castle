//! The `name` table: family, style and PostScript names, plus the head/OS/2 style bits. Shared by
//! `Font` and the system `FontIndex` (which reads only these few tables from disk).

use super::read::{rd16, sub};
use super::{Names, Style};

/// Mac Roman bytes 0x80..=0xFF as Unicode.
pub(crate) const MAC_ROMAN_HIGH: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»…\u{A0}ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ\u{F8FF}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

pub(crate) fn mac_roman_to_char(b: u8) -> char {
    if b < 0x80 {
        b as char
    } else {
        MAC_ROMAN_HIGH.chars().nth((b - 0x80) as usize).unwrap_or('\u{FFFD}')
    }
}

pub(crate) fn char_to_mac_roman(c: char) -> Option<u8> {
    if (c as u32) < 0x80 {
        return Some(c as u8);
    }
    MAC_ROMAN_HIGH.chars().position(|m| m == c).map(|i| 0x80 + i as u8)
}

struct Rec {
    platform: u16,
    encoding: u16,
    language: u16,
    id: u16,
    start: usize,
    len: usize,
}

fn records(name: &[u8]) -> Vec<Rec> {
    let mut out = Vec::new();
    let count = rd16(name, 2).unwrap_or(0) as usize;
    let storage = rd16(name, 4).unwrap_or(0) as usize;
    for i in 0..count {
        let r = 6 + 12 * i;
        let (Some(platform), Some(encoding), Some(language), Some(id), Some(len), Some(off)) = (
            rd16(name, r),
            rd16(name, r + 2),
            rd16(name, r + 4),
            rd16(name, r + 6),
            rd16(name, r + 8),
            rd16(name, r + 10),
        ) else {
            break;
        };
        out.push(Rec { platform, encoding, language, id, start: storage + off as usize, len: len as usize });
    }
    out
}

fn score(r: &Rec) -> u8 {
    match (r.platform, r.encoding) {
        (3, 1) | (3, 10) if r.language == 0x0409 => 10,
        (3, 1) | (3, 10) if r.language & 0xFF == 0x09 => 9,
        (3, 1) | (3, 10) => 7,
        (3, 0) if r.language == 0x0409 => 6,
        (0, _) => 6,
        (3, 0) => 5,
        (1, 0) if r.language == 0 => 4,
        (1, 0) => 2,
        _ => 0,
    }
}

fn decode(name: &[u8], r: &Rec) -> Option<String> {
    let bytes = sub(name, r.start, r.len)?;
    match r.platform {
        0 | 3 => {
            let units = bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]]));
            Some(char::decode_utf16(units).map(|c| c.unwrap_or('\u{FFFD}')).collect())
        }
        1 if r.encoding == 0 => Some(bytes.iter().map(|&b| mac_roman_to_char(b)).collect()),
        _ => None,
    }
}

/// The best string for one name ID (Windows Unicode English first, Mac Roman last).
pub(crate) fn best(name: &[u8], id: u16) -> Option<String> {
    let recs = records(name);
    let mut best: Option<(&Rec, u8)> = None;
    for r in recs.iter().filter(|r| r.id == id) {
        let s = score(r);
        if s > 0 && best.map_or(true, |(_, b)| s > b) {
            best = Some((r, s));
        }
    }
    let (r, _) = best?;
    decode(name, r).map(|s| s.trim_end_matches('\0').trim().to_string()).filter(|s| !s.is_empty())
}

/// Every distinct decodable string for a name ID, in all languages (localized family names).
pub(crate) fn all(name: &[u8], id: u16) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in records(name).iter().filter(|r| r.id == id && score(r) > 0) {
        if let Some(s) = decode(name, r) {
            let s = s.trim_end_matches('\0').trim().to_string();
            if !s.is_empty() && !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

pub(crate) fn parse_names(name: Option<&[u8]>) -> Names {
    let Some(name) = name else { return Names::default() };
    Names {
        family: best(name, 1).unwrap_or_default(),
        subfamily: best(name, 2).unwrap_or_default(),
        full_name: best(name, 4).unwrap_or_default(),
        postscript_name: best(name, 6).unwrap_or_default(),
        typographic_family: best(name, 16).unwrap_or_default(),
        typographic_subfamily: best(name, 17).unwrap_or_default(),
    }
}

pub(crate) fn parse_style(os2: Option<&[u8]>, head: Option<&[u8]>, subfamily: &str) -> Style {
    let mac_style = head.and_then(|h| rd16(h, 44)).unwrap_or(0);
    let sub_lower = subfamily.to_ascii_lowercase();
    let mut weight = os2.and_then(|o| rd16(o, 4)).unwrap_or(0);
    if (1..=9).contains(&weight) {
        weight *= 100; // some old fonts wrote 1-9
    }
    if weight == 0 || weight > 1000 {
        weight = if mac_style & 1 != 0 || sub_lower.contains("bold") { 700 } else { 400 };
    }
    let fs = os2.and_then(|o| rd16(o, 62)).unwrap_or(0);
    let italic = fs & 1 != 0
        || fs & (1 << 9) != 0
        || mac_style & 2 != 0
        || sub_lower.contains("italic")
        || sub_lower.contains("oblique");
    let mut stretch = os2.and_then(|o| rd16(o, 6)).unwrap_or(5);
    if !(1..=9).contains(&stretch) {
        stretch = 5;
    }
    Style { weight, italic, stretch }
}
