//! CMaps: the embedded encodings of composite fonts (codespace ranges, `cidrange`, `cidchar`) and the
//! ToUnicode maps (`bfchar`, `bfrange`) that turn character codes into text.

use std::collections::HashMap;

use super::lexer::{Lexer, Tok};
use super::obj::utf16be;

/// The most code-to-text entries one CMap may expand to.
const MAX_ENTRIES: usize = 1 << 20;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CMap {
    /// (low, high, byte count).
    pub codespace: Vec<(u32, u32, usize)>,
    /// (low code, high code, first CID).
    pub cid_ranges: Vec<(u32, u32, u32)>,
    pub cid_chars: HashMap<u32, u32>,
    pub unicode: HashMap<u32, String>,
    /// `usecmap /Identity-H` (or -V) seen, or a predefined identity map.
    pub identity: bool,
    /// Vertical writing (WMode 1).
    pub vertical: bool,
}

fn code_of(b: &[u8]) -> u32 {
    b.iter().take(4).fold(0u32, |acc, &x| (acc << 8) | x as u32)
}

impl CMap {
    /// Identity-H / Identity-V: two-byte codes that are their own CIDs.
    pub fn identity(vertical: bool) -> CMap {
        CMap { codespace: vec![(0, 0xFFFF, 2)], identity: true, vertical, ..CMap::default() }
    }

    /// Parse a CMap stream's bytes.
    pub fn parse(data: &[u8]) -> CMap {
        let mut m = CMap::default();
        let mut lx = Lexer::new(data, 0);
        let mut operands: Vec<Tok> = Vec::new();
        let mut entries = 0usize;
        loop {
            let t = lx.next();
            match t {
                Tok::Eof => break,
                Tok::Keyword(ref k) => {
                    match k.as_slice() {
                        b"begincodespacerange" => loop {
                            let a = lx.next();
                            let Tok::Str(lo) = a else { break };
                            let Tok::Str(hi) = lx.next() else { break };
                            let n = lo.len().clamp(1, 4);
                            m.codespace.push((code_of(&lo), code_of(&hi), n));
                        },
                        b"beginbfchar" => loop {
                            let Tok::Str(src) = lx.next() else { break };
                            let dst = lx.next();
                            let text = match dst {
                                Tok::Str(d) => utf16be(&d),
                                Tok::Name(n) => super::encoding::glyph_to_unicode(&n).unwrap_or_default(),
                                _ => break,
                            };
                            if entries < MAX_ENTRIES {
                                m.unicode.insert(code_of(&src), text);
                                entries += 1;
                            }
                        },
                        b"beginbfrange" => loop {
                            let Tok::Str(lo) = lx.next() else { break };
                            let Tok::Str(hi) = lx.next() else { break };
                            let (lo, hi) = (code_of(&lo), code_of(&hi));
                            match lx.next() {
                                Tok::Str(d) => {
                                    if hi < lo || (hi - lo) as usize > 0xFFFF {
                                        continue;
                                    }
                                    for (i, code) in (lo..=hi).enumerate() {
                                        if entries >= MAX_ENTRIES {
                                            break;
                                        }
                                        let mut dst = d.clone();
                                        add_to_last(&mut dst, i as u32);
                                        m.unicode.insert(code, utf16be(&dst));
                                        entries += 1;
                                    }
                                }
                                Tok::ArrOpen => {
                                    let mut code = lo;
                                    loop {
                                        match lx.next() {
                                            Tok::Str(d) => {
                                                if code <= hi && entries < MAX_ENTRIES {
                                                    m.unicode.insert(code, utf16be(&d));
                                                    entries += 1;
                                                }
                                                code = code.saturating_add(1);
                                            }
                                            Tok::ArrClose | Tok::Eof => break,
                                            _ => {}
                                        }
                                    }
                                }
                                _ => break,
                            }
                        },
                        b"begincidrange" => loop {
                            let Tok::Str(lo) = lx.next() else { break };
                            let Tok::Str(hi) = lx.next() else { break };
                            let Tok::Int(cid) = lx.next() else { break };
                            if m.cid_ranges.len() < MAX_ENTRIES {
                                m.cid_ranges.push((code_of(&lo), code_of(&hi), cid.clamp(0, u32::MAX as i64) as u32));
                            }
                        },
                        b"begincidchar" => loop {
                            let Tok::Str(c) = lx.next() else { break };
                            let Tok::Int(cid) = lx.next() else { break };
                            if m.cid_chars.len() < MAX_ENTRIES {
                                m.cid_chars.insert(code_of(&c), cid.clamp(0, u32::MAX as i64) as u32);
                            }
                        },
                        b"usecmap" => {
                            if let Some(Tok::Name(n)) = operands.last() {
                                if n.starts_with("Identity") {
                                    m.identity = true;
                                }
                            }
                        }
                        b"def" => {
                            if operands.len() >= 2 {
                                if let (Tok::Name(n), Tok::Int(v)) = (&operands[operands.len() - 2], &operands[operands.len() - 1]) {
                                    if n == "WMode" && *v == 1 {
                                        m.vertical = true;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    operands.clear();
                }
                other => {
                    operands.push(other);
                    if operands.len() > 8 {
                        operands.remove(0);
                    }
                }
            }
        }
        if m.identity && m.codespace.is_empty() {
            m.codespace.push((0, 0xFFFF, 2));
        }
        m
    }

    /// The next character code in `s` at `i`: (code, byte count). Codespace ranges decide the
    /// length; without any, codes are `default_len` bytes.
    pub fn next_code(&self, s: &[u8], i: usize, default_len: usize) -> (u32, usize) {
        if !self.codespace.is_empty() {
            let mut code = 0u32;
            for n in 1..=4usize {
                let Some(&b) = s.get(i + n - 1) else { break };
                code = (code << 8) | b as u32;
                if self.codespace.iter().any(|&(lo, hi, len)| len == n && code >= lo && code <= hi) {
                    return (code, n);
                }
            }
            // No range matched: take the shortest codespace length (at least one byte).
            let n = self.codespace.iter().map(|c| c.2).min().unwrap_or(1).max(1);
            let n = n.min(s.len() - i).max(1);
            return (code_of(&s[i..i + n]), n);
        }
        let n = default_len.clamp(1, 4).min(s.len() - i).max(1);
        (code_of(&s[i..i + n]), n)
    }

    /// The CID for a code (identity when the map says so or knows nothing).
    pub fn cid(&self, code: u32) -> u32 {
        if let Some(&c) = self.cid_chars.get(&code) {
            return c;
        }
        for &(lo, hi, cid) in &self.cid_ranges {
            if code >= lo && code <= hi {
                return cid.saturating_add(code - lo);
            }
        }
        code
    }
}

/// Add `n` to the last UTF-16 unit of a big-endian destination (a bfrange step).
fn add_to_last(dst: &mut Vec<u8>, n: u32) {
    if dst.is_empty() {
        return;
    }
    if dst.len() == 1 {
        dst[0] = dst[0].wrapping_add(n as u8);
        return;
    }
    let l = dst.len();
    let v = u16::from_be_bytes([dst[l - 2], dst[l - 1]]).wrapping_add(n as u16);
    let b = v.to_be_bytes();
    dst[l - 2] = b[0];
    dst[l - 1] = b[1];
}
