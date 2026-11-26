//! Stream filters: FlateDecode (with PNG and TIFF predictors), LZWDecode, ASCIIHexDecode,
//! ASCII85Decode and RunLengthDecode. Image codecs (DCTDecode, JPXDecode, CCITTFaxDecode, JBIG2Decode)
//! stop the chain: the caller gets the bytes still encoded and the codec's name.

use super::obj::{Dict, Obj};
use crate::ground::flate::{inflate_limited, zlib_decompress_limited};

/// The most bytes one stream may decode to (a guard against decompression bombs).
pub const MAX_DECODED: usize = 256 * 1024 * 1024;

/// The result of running a stream's filters.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub data: Vec<u8>,
    /// An image codec that was not run (`DCTDecode`, `JPXDecode`, ...), with its parameters.
    pub image_codec: Option<(String, Dict)>,
}

/// Normalise abbreviated filter names (inline images use them).
pub fn full_filter_name(n: &str) -> &str {
    match n {
        "AHx" => "ASCIIHexDecode",
        "A85" => "ASCII85Decode",
        "LZW" => "LZWDecode",
        "Fl" => "FlateDecode",
        "RL" => "RunLengthDecode",
        "CCF" => "CCITTFaxDecode",
        "DCT" => "DCTDecode",
        other => other,
    }
}

/// Run `filters` (each with its parameters, already resolved) over `data`.
pub fn decode(data: &[u8], filters: &[(String, Dict)]) -> Result<Decoded, String> {
    let mut cur: Vec<u8> = data.to_vec();
    for (i, (name, parms)) in filters.iter().enumerate() {
        let name = full_filter_name(name);
        cur = match name {
            "FlateDecode" => predict(&flate(&cur)?, parms),
            "LZWDecode" => {
                let early = parms.get("EarlyChange").and_then(Obj::as_i64).unwrap_or(1) != 0;
                predict(&lzw(&cur, early), parms)
            }
            "ASCIIHexDecode" => ascii_hex(&cur),
            "ASCII85Decode" => ascii85(&cur),
            "RunLengthDecode" => run_length(&cur),
            "DCTDecode" | "JPXDecode" | "CCITTFaxDecode" | "JBIG2Decode" => {
                if i + 1 != filters.len() {
                    return Err(format!("{} must be the last filter", name));
                }
                return Ok(Decoded { data: cur, image_codec: Some((name.to_string(), parms.clone())) });
            }
            "Crypt" => cur,
            other => return Err(format!("unsupported filter {}", other)),
        };
        if cur.len() > MAX_DECODED {
            return Err("stream too large".into());
        }
    }
    Ok(Decoded { data: cur, image_codec: None })
}

fn flate(data: &[u8]) -> Result<Vec<u8>, String> {
    if let Ok(v) = zlib_decompress_limited(data, MAX_DECODED) {
        return Ok(v);
    }
    // Some writers forget the zlib header, or write a wrong checksum.
    if data.len() > 2 {
        if let Ok(v) = inflate_limited(&data[2..], MAX_DECODED) {
            return Ok(v);
        }
    }
    inflate_limited(data, MAX_DECODED).map_err(|e| format!("flate: {}", e))
}

/// Undo a PNG (10..=15) or TIFF (2) predictor.
pub fn predict(data: &[u8], parms: &Dict) -> Vec<u8> {
    let predictor = parms.get("Predictor").and_then(Obj::as_i64).unwrap_or(1);
    if predictor <= 1 {
        return data.to_vec();
    }
    let colors = parms.get("Colors").and_then(Obj::as_i64).unwrap_or(1).clamp(1, 32) as usize;
    let bpc = parms.get("BitsPerComponent").and_then(Obj::as_i64).unwrap_or(8).clamp(1, 16) as usize;
    let columns = parms.get("Columns").and_then(Obj::as_i64).unwrap_or(1).clamp(1, 1 << 20) as usize;
    let bits_per_pixel = colors * bpc;
    let row_len = (bits_per_pixel * columns + 7) / 8;
    let bpp = ((bits_per_pixel + 7) / 8).max(1);
    if predictor == 2 {
        return tiff_predict(data, colors, bpc, columns, row_len);
    }
    // PNG: every row starts with its own filter type byte.
    let mut out = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; row_len];
    let mut row = vec![0u8; row_len];
    let mut i = 0;
    while i < data.len() {
        let ft = data[i];
        i += 1;
        let n = row_len.min(data.len() - i);
        row[..n].copy_from_slice(&data[i..i + n]);
        for b in row[n..].iter_mut() {
            *b = 0;
        }
        i += n;
        for x in 0..row_len {
            let a = if x >= bpp { row[x - bpp] } else { 0 };
            let b = prev[x];
            let c = if x >= bpp { prev[x - bpp] } else { 0 };
            let v = row[x];
            row[x] = match ft {
                1 => v.wrapping_add(a),
                2 => v.wrapping_add(b),
                3 => v.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => v.wrapping_add(paeth(a, b, c)),
                _ => v,
            };
        }
        out.extend_from_slice(&row[..n]);
        std::mem::swap(&mut prev, &mut row);
    }
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn tiff_predict(data: &[u8], colors: usize, bpc: usize, columns: usize, row_len: usize) -> Vec<u8> {
    let mut out = data.to_vec();
    for row in out.chunks_mut(row_len) {
        match bpc {
            8 => {
                for x in colors..row.len() {
                    row[x] = row[x].wrapping_add(row[x - colors]);
                }
            }
            16 => {
                let step = colors * 2;
                let mut x = step;
                while x + 1 < row.len() {
                    let cur = u16::from_be_bytes([row[x], row[x + 1]]);
                    let left = u16::from_be_bytes([row[x - step], row[x - step + 1]]);
                    let v = cur.wrapping_add(left).to_be_bytes();
                    row[x] = v[0];
                    row[x + 1] = v[1];
                    x += 2;
                }
            }
            1 | 2 | 4 => {
                let mask = (1u16 << bpc) - 1;
                let get = |r: &[u8], i: usize| -> u16 {
                    let bit = i * bpc;
                    let byte = r.get(bit / 8).copied().unwrap_or(0) as u16;
                    (byte >> (8 - bpc - bit % 8)) & mask
                };
                let n = columns * colors;
                for i in colors..n {
                    if (i * bpc) / 8 >= row.len() {
                        break;
                    }
                    let v = (get(row, i) + get(row, i - colors)) & mask;
                    let bit = i * bpc;
                    let shift = 8 - bpc - bit % 8;
                    let b = &mut row[bit / 8];
                    *b = (*b & !((mask as u8) << shift)) | ((v as u8) << shift);
                }
            }
            _ => {}
        }
    }
    out
}

/// LZW as PDF uses it (9 to 12 bit codes, 256 = clear, 257 = end).
pub fn lzw(data: &[u8], early_change: bool) -> Vec<u8> {
    let early = if early_change { 1 } else { 0 };
    let mut out = Vec::new();
    let mut table: Vec<Vec<u8>> = (0..256).map(|i| vec![i as u8]).collect();
    table.push(Vec::new());
    table.push(Vec::new());
    let mut width = 9u32;
    let mut buf: u32 = 0;
    let mut nbits = 0u32;
    let mut prev: Option<usize> = None;
    let mut i = 0;
    loop {
        while nbits < width {
            if i >= data.len() {
                return out;
            }
            buf = (buf << 8) | data[i] as u32;
            nbits += 8;
            i += 1;
        }
        let code = ((buf >> (nbits - width)) & ((1 << width) - 1)) as usize;
        nbits -= width;
        buf &= (1u32 << nbits).wrapping_sub(1);
        if code == 256 {
            table.truncate(258);
            width = 9;
            prev = None;
            continue;
        }
        if code == 257 {
            break;
        }
        let entry: Vec<u8> = if code < table.len() {
            table[code].clone()
        } else if let Some(p) = prev {
            // The KwKwK case.
            let mut e = table[p].clone();
            let first = e.first().copied().unwrap_or(0);
            e.push(first);
            e
        } else {
            break;
        };
        out.extend_from_slice(&entry);
        if out.len() > MAX_DECODED {
            break;
        }
        if let Some(p) = prev {
            if table.len() < 4096 {
                let mut e = table[p].clone();
                e.push(entry.first().copied().unwrap_or(0));
                table.push(e);
            }
        }
        prev = Some(code.min(table.len() - 1));
        let next = table.len() + early;
        width = if next >= 2048 {
            12
        } else if next >= 1024 {
            11
        } else if next >= 512 {
            10
        } else {
            9
        };
    }
    out
}

pub fn ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut hi: Option<u8> = None;
    for &c in data {
        if c == b'>' {
            break;
        }
        if let Some(v) = super::lexer::hex_val(c) {
            match hi {
                None => hi = Some(v),
                Some(h) => {
                    out.push(h * 16 + v);
                    hi = None;
                }
            }
        }
    }
    if let Some(h) = hi {
        out.push(h * 16);
    }
    out
}

pub fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 4 / 5);
    let mut group = [0u8; 5];
    let mut n = 0;
    let mut i = 0;
    // Skip an optional "<~" prefix.
    if data.starts_with(b"<~") {
        i = 2;
    }
    while i < data.len() {
        let c = data[i];
        i += 1;
        if c == b'~' {
            break;
        }
        if super::lexer::is_white(c) {
            continue;
        }
        if c == b'z' && n == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&c) {
            continue;
        }
        group[n] = c - b'!';
        n += 1;
        if n == 5 {
            let v = group.iter().fold(0u64, |acc, &d| acc * 85 + d as u64);
            out.extend_from_slice(&(v as u32).to_be_bytes());
            n = 0;
        }
    }
    if n > 1 {
        for g in group.iter_mut().skip(n) {
            *g = 84;
        }
        let v = group.iter().fold(0u64, |acc, &d| acc * 85 + d as u64);
        let b = (v as u32).to_be_bytes();
        out.extend_from_slice(&b[..n - 1]);
    }
    out
}

pub fn run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let n = data[i];
        i += 1;
        if n == 128 {
            break;
        }
        if n < 128 {
            let len = n as usize + 1;
            let end = (i + len).min(data.len());
            out.extend_from_slice(&data[i..end]);
            i = end;
        } else {
            let Some(&b) = data.get(i) else { break };
            i += 1;
            for _ in 0..(257 - n as usize) {
                out.push(b);
            }
        }
        if out.len() > MAX_DECODED {
            break;
        }
    }
    out
}
