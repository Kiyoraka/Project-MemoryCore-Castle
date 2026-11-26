//! PNG, decoded and encoded by us: no crate, no `unsafe`, nothing but the standard library. The
//! compression underneath is our own zlib ([`crate::ground::flate`]); the pixels land in the same
//! [`Image`] (8-bit RGBA) the JPEG decoder fills.
//!
//! # Decoder ([`decode`], [`decode_with_info`], [`decode_with_options`])
//!
//! Reads every colour type at every legal bit depth: greyscale (0) at 1, 2, 4, 8 and 16 bits, RGB (2)
//! at 8 and 16, palette (3) at 1, 2, 4 and 8, grey + alpha (4) and RGBA (6) at 8 and 16. Samples below
//! 8 bits are scaled up exactly (a 2-bit 3 becomes 255); 16-bit samples are scaled down with rounding
//! (`round(v * 255 / 65535)`). All five scanline filters (None, Sub, Up, Average, Paeth) and Adam7
//! interlacing are read. The image data may be split over any number of IDAT chunks.
//!
//! Transparency: a palette's tRNS alphas, and the grey or RGB key colour of tRNS (compared against the
//! raw samples, before scaling) become alpha 0. Pixels without transparency get alpha 255.
//!
//! Every chunk's CRC is checked. A bad CRC on a critical chunk (IHDR, PLTE, IDAT, IEND) is always an
//! error; for ancillary chunks [`DecodeOptions::check_ancillary_crc`] chooses between an error (the
//! default) and skipping the chunk. An unknown critical chunk is refused; unknown ancillary chunks are
//! skipped. Malformed ancillary chunks (a tRNS or pHYs of the wrong length, text without a keyword
//! separator, text that does not decompress) are skipped too.
//!
//! [`PngInfo`] reports gAMA, sRGB, pHYs (as dots per inch, when the unit is the metre) and the text of
//! tEXt, zTXt and iTXt. **No colour management is applied**: the gamma, the sRGB intent and any iCCP
//! profile are reported (or ignored) but the pixels are handed back exactly as stored.
//!
//! APNG: the animation chunks (acTL, fcTL, fdAT) are ancillary and skipped, so an animated PNG decodes
//! to its default image (the IDAT picture); [`PngInfo::animated`] says whether an acTL was seen.
//!
//! ## Limits and corrupt data
//!
//! The decoder never panics on any input. Pictures above [`MAX_PIXELS`] return
//! `Err(PngError::TooLarge)` before any pixel memory is allocated. The image data is inflated with a
//! limit of exactly the size the header implies, so a decompression bomb stops at that size; more data
//! than the picture holds, or less, is an error. Text chunks share an 8 MiB decompression budget.
//! A palette index past the end of the palette reads as opaque black. A file that stops cleanly after
//! its image data but lacks IEND still decodes; a file that ends inside a chunk returns
//! `Err(PngError::Truncated)`. IDAT chunks that are not consecutive are joined anyway.
//!
//! # Encoder ([`encode`], [`encode_rgba8`])
//!
//! Writes 8-bit PNGs in the smallest of four colour types that loses nothing: greyscale when every
//! pixel is opaque grey (r = g = b), grey + alpha when every pixel is grey, RGB when every pixel is
//! opaque, RGBA otherwise. Each row's filter is chosen adaptively (the filter whose output has the
//! smallest sum of absolute values, read as signed bytes - the heuristic libpng recommends); level 0
//! stores rows unfiltered. The image data is compressed by our zlib at the requested level and written
//! in IDAT chunks of 64 KiB, with a pHYs chunk when [`EncodeOptions::dpi`] asks. No palette output and
//! no interlacing. A picture with a zero dimension is written as one transparent pixel; dimensions
//! above 2^31 - 1 (the format's limit) are cropped; pixels missing from a short buffer are written as
//! transparent black.

use std::fmt;

use super::jpeg::Image;
use crate::ground::flate::{zlib_compress, zlib_decompress_limited, Crc32};

/// The largest picture (width x height) the decoder accepts - the same limit as the JPEG decoder.
pub const MAX_PIXELS: u64 = super::jpeg::MAX_PIXELS;

/// The eight bytes every PNG file starts with.
pub const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// The largest width or height the format allows.
const MAX_DIMENSION: u32 = 0x7FFF_FFFF;

/// How many bytes all the zTXt and compressed iTXt chunks of one file may inflate to, together.
const TEXT_BUDGET: usize = 8 << 20;

/// The IDAT chunk size the encoder writes.
const IDAT_CHUNK: usize = 1 << 16;

// ---------------------------------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------------------------------

/// What the decoder learned about a file besides its pixels.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PngInfo {
    /// Bits per sample as stored: 1, 2, 4, 8 or 16. (The pixels are always handed back as 8-bit.)
    pub bit_depth: u8,
    /// The stored colour type: 0 grey, 2 RGB, 3 palette, 4 grey + alpha, 6 RGBA.
    pub color_type: u8,
    /// Whether the file is Adam7-interlaced.
    pub interlaced: bool,
    /// The gAMA chunk's file gamma (for example 0.45455), when present. Not applied.
    pub gamma: Option<f64>,
    /// Whether an sRGB chunk is present. Not applied.
    pub srgb: bool,
    /// Horizontal and vertical resolution in dots per inch, from a pHYs chunk whose unit is the metre.
    pub dpi: Option<(f64, f64)>,
    /// (keyword, text) of every tEXt, zTXt and iTXt chunk, in file order. Latin-1 text is converted to
    /// Unicode; iTXt text is read as UTF-8 (invalid sequences become U+FFFD).
    pub text: Vec<(String, String)>,
    /// Whether an APNG acTL chunk was seen. Only the default image is decoded.
    pub animated: bool,
}

/// Choices for [`decode_with_options`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeOptions {
    /// When true (the default), a bad CRC on an ancillary chunk is an error, like on a critical one.
    /// When false, an ancillary chunk with a bad CRC is skipped. Critical chunks are always checked.
    pub check_ancillary_crc: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        DecodeOptions { check_ancillary_crc: true }
    }
}

/// Choices for [`encode`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EncodeOptions {
    /// zlib compression level, 0 (stored) to 9 (smallest). Higher values are treated as 9.
    pub level: u8,
    /// When set, a pHYs chunk records this horizontal and vertical resolution in dots per inch.
    pub dpi: Option<(f64, f64)>,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        EncodeOptions { level: 6, dpi: None }
    }
}

/// Why a PNG could not be decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PngError {
    /// The data does not start with the PNG signature.
    NotPng,
    /// The data ended inside a chunk, or before any image data.
    Truncated,
    /// The CRC of the named chunk does not match its contents.
    BadCrc([u8; 4]),
    /// The data is malformed; the text says where.
    Corrupt(&'static str),
    /// A valid-looking feature this decoder does not read (an unknown critical chunk).
    Unsupported(&'static str),
    /// The picture is larger than [`MAX_PIXELS`].
    TooLarge,
    /// The image data is not a valid zlib stream; the text is the inflater's reason.
    Compressed(String),
}

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PngError::NotPng => write!(f, "not a PNG file"),
            PngError::Truncated => write!(f, "PNG data ends too early"),
            PngError::BadCrc(t) => write!(f, "PNG chunk {} has a bad CRC", String::from_utf8_lossy(t)),
            PngError::Corrupt(what) => write!(f, "corrupt PNG: {what}"),
            PngError::Unsupported(what) => write!(f, "unsupported PNG: {what}"),
            PngError::TooLarge => write!(f, "PNG picture is too large"),
            PngError::Compressed(why) => write!(f, "PNG image data does not inflate: {why}"),
        }
    }
}

impl std::error::Error for PngError {}

// ---------------------------------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------------------------------

/// Decodes a PNG file into RGBA pixels. See the module notes for what is read.
pub fn decode(data: &[u8]) -> Result<Image, PngError> {
    decode_with_options(data, DecodeOptions::default()).map(|(img, _)| img)
}

/// Decodes a PNG file and also returns what it says about itself (see [`PngInfo`]).
pub fn decode_with_info(data: &[u8]) -> Result<(Image, PngInfo), PngError> {
    decode_with_options(data, DecodeOptions::default())
}

/// Like [`decode_with_info`], with a choice of how strict to be about ancillary chunks.
pub fn decode_with_options(data: &[u8], opts: DecodeOptions) -> Result<(Image, PngInfo), PngError> {
    if data.len() < 8 || data[..8] != SIGNATURE {
        return Err(PngError::NotPng);
    }
    let mut pos = 8usize;
    let mut header: Option<Header> = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns = Trns::None;
    let mut trns_raw: Option<Vec<u8>> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut seen_idat = false;
    let mut info = PngInfo::default();
    let mut text_budget = TEXT_BUDGET;

    while pos < data.len() {
        if data.len() - pos < 8 {
            return Err(PngError::Truncated);
        }
        let len = be32(&data[pos..pos + 4]);
        if len > MAX_DIMENSION {
            return Err(PngError::Corrupt("chunk length over 2^31 - 1"));
        }
        let len = len as usize;
        let ty = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
        let body_start = pos + 8;
        let end = match body_start.checked_add(len).and_then(|e| e.checked_add(4)) {
            Some(e) if e <= data.len() => e,
            _ => return Err(PngError::Truncated),
        };
        let body = &data[body_start..body_start + len];
        let stored_crc = be32(&data[body_start + len..end]);
        pos = end;

        if !ty.iter().all(|b| b.is_ascii_alphabetic()) {
            return Err(PngError::Corrupt("chunk type is not four letters"));
        }
        let critical = ty[0] & 0x20 == 0;
        let mut crc = Crc32::new();
        crc.update(&ty);
        crc.update(body);
        if crc.finish() != stored_crc {
            if critical || opts.check_ancillary_crc {
                return Err(PngError::BadCrc(ty));
            }
            continue;
        }

        let hdr = match (&header, &ty) {
            (None, b"IHDR") => {
                let h = parse_ihdr(body)?;
                info.bit_depth = h.depth;
                info.color_type = h.ctype;
                info.interlaced = h.interlaced;
                header = Some(h);
                continue;
            }
            (None, _) => return Err(PngError::Corrupt("first chunk is not IHDR")),
            (Some(_), b"IHDR") => return Err(PngError::Corrupt("more than one IHDR")),
            (Some(h), _) => *h,
        };

        match &ty {
            b"IEND" => break,
            b"IDAT" => {
                idat.extend_from_slice(body);
                seen_idat = true;
            }
            b"PLTE" => {
                if body.is_empty() || body.len() % 3 != 0 || body.len() > 256 * 3 {
                    if hdr.ctype == 3 {
                        return Err(PngError::Corrupt("PLTE length is not 3 to 768, a multiple of 3"));
                    }
                } else if palette.is_empty() && !seen_idat {
                    palette = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
                    if hdr.ctype == 3 && palette.len() > (1usize << hdr.depth) {
                        // More entries than the bit depth can index: the extra ones are never used.
                        palette.truncate(1usize << hdr.depth);
                    }
                }
            }
            b"tRNS" => {
                if trns_raw.is_none() && !seen_idat {
                    trns_raw = Some(body.to_vec());
                }
            }
            b"gAMA" => {
                if body.len() == 4 {
                    let g = be32(body);
                    if g > 0 {
                        info.gamma = Some(g as f64 / 100_000.0);
                    }
                }
            }
            b"sRGB" => {
                if body.len() == 1 {
                    info.srgb = true;
                }
            }
            b"pHYs" => {
                if body.len() == 9 && body[8] == 1 {
                    let (x, y) = (be32(&body[0..4]), be32(&body[4..8]));
                    if x > 0 && y > 0 {
                        info.dpi = Some((x as f64 * 0.0254, y as f64 * 0.0254));
                    }
                }
            }
            b"tEXt" | b"zTXt" | b"iTXt" => {
                if let Some(entry) = parse_text(&ty, body, &mut text_budget) {
                    info.text.push(entry);
                }
            }
            b"acTL" => info.animated = true,
            _ => {
                if critical {
                    return Err(PngError::Unsupported("unknown critical chunk"));
                }
            }
        }
    }

    let hdr = header.ok_or(PngError::Truncated)?;
    if !seen_idat {
        return Err(PngError::Truncated);
    }
    if hdr.ctype == 3 && palette.is_empty() {
        return Err(PngError::Corrupt("palette picture without PLTE"));
    }
    if let Some(raw) = trns_raw {
        trns = parse_trns(&hdr, &raw, palette.len());
    }

    let img = decode_pixels(&hdr, &idat, &palette, &trns)?;
    Ok((img, info))
}

#[derive(Clone, Copy, Debug)]
struct Header {
    width: u32,
    height: u32,
    depth: u8,
    ctype: u8,
    interlaced: bool,
}

impl Header {
    fn channels(&self) -> usize {
        match self.ctype {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        }
    }

    fn bits_per_pixel(&self) -> usize {
        self.channels() * self.depth as usize
    }

    /// Bytes of one unfiltered row `w` pixels wide.
    fn row_bytes(&self, w: usize) -> usize {
        (w * self.bits_per_pixel()).div_ceil(8)
    }
}

/// The transparency a tRNS chunk gives, already matched to the colour type.
#[derive(Clone, Debug)]
enum Trns {
    None,
    /// Alpha for palette entries 0.. (entries past the end are opaque).
    Palette(Vec<u8>),
    /// The raw grey sample that is fully transparent.
    Grey(u16),
    /// The raw RGB samples that are fully transparent.
    Rgb(u16, u16, u16),
}

fn be32(b: &[u8]) -> u32 {
    match b {
        [a, b, c, d, ..] => u32::from_be_bytes([*a, *b, *c, *d]),
        _ => 0,
    }
}

fn be16(b: &[u8]) -> u16 {
    match b {
        [a, b, ..] => u16::from_be_bytes([*a, *b]),
        _ => 0,
    }
}

fn parse_ihdr(body: &[u8]) -> Result<Header, PngError> {
    if body.len() != 13 {
        return Err(PngError::Corrupt("IHDR is not 13 bytes"));
    }
    let width = be32(&body[0..4]);
    let height = be32(&body[4..8]);
    let (depth, ctype, compression, filter, interlace) = (body[8], body[9], body[10], body[11], body[12]);
    if width == 0 || height == 0 {
        return Err(PngError::Corrupt("zero width or height"));
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PngError::Corrupt("width or height over 2^31 - 1"));
    }
    let legal = match ctype {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        2 | 4 | 6 => matches!(depth, 8 | 16),
        _ => return Err(PngError::Corrupt("unknown colour type")),
    };
    if !legal {
        return Err(PngError::Corrupt("bit depth not allowed for the colour type"));
    }
    if compression != 0 {
        return Err(PngError::Corrupt("unknown compression method"));
    }
    if filter != 0 {
        return Err(PngError::Corrupt("unknown filter method"));
    }
    if interlace > 1 {
        return Err(PngError::Corrupt("unknown interlace method"));
    }
    if width as u64 * height as u64 > MAX_PIXELS {
        return Err(PngError::TooLarge);
    }
    Ok(Header { width, height, depth, ctype, interlaced: interlace == 1 })
}

fn parse_trns(hdr: &Header, body: &[u8], palette_len: usize) -> Trns {
    match hdr.ctype {
        3 if !body.is_empty() => {
            let mut a = body.to_vec();
            a.truncate(palette_len);
            Trns::Palette(a)
        }
        0 if body.len() == 2 => Trns::Grey(be16(body)),
        2 if body.len() == 6 => Trns::Rgb(be16(&body[0..2]), be16(&body[2..4]), be16(&body[4..6])),
        // Wrong length, or a colour type that already has alpha: ignored.
        _ => Trns::None,
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Reads a tEXt, zTXt or iTXt chunk; `None` when it is malformed (the chunk is then skipped).
fn parse_text(ty: &[u8; 4], body: &[u8], budget: &mut usize) -> Option<(String, String)> {
    let nul = body.iter().position(|&b| b == 0)?;
    let keyword = latin1(&body[..nul]);
    let rest = &body[nul + 1..];
    let mut inflate = |data: &[u8]| -> Option<Vec<u8>> {
        let out = zlib_decompress_limited(data, *budget).ok()?;
        *budget -= out.len();
        Some(out)
    };
    match ty {
        b"tEXt" => Some((keyword, latin1(rest))),
        b"zTXt" => {
            let (&method, data) = rest.split_first()?;
            if method != 0 {
                return None;
            }
            Some((keyword, latin1(&inflate(data)?)))
        }
        _ => {
            // iTXt: compression flag, method, language tag NUL, translated keyword NUL, text.
            if rest.len() < 2 {
                return None;
            }
            let (flag, method) = (rest[0], rest[1]);
            let rest = &rest[2..];
            let lang_end = rest.iter().position(|&b| b == 0)?;
            let rest = &rest[lang_end + 1..];
            let tr_end = rest.iter().position(|&b| b == 0)?;
            let text = &rest[tr_end + 1..];
            let text = match (flag, method) {
                (0, _) => text.to_vec(),
                (1, 0) => inflate(text)?,
                _ => return None,
            };
            Some((keyword, String::from_utf8_lossy(&text).into_owned()))
        }
    }
}

/// One pass of the picture: where its pixels sit and how many there are.
#[derive(Clone, Copy, Debug)]
struct Pass {
    x0: usize,
    y0: usize,
    dx: usize,
    dy: usize,
    w: usize,
    h: usize,
}

fn passes(width: usize, height: usize, interlaced: bool) -> Vec<Pass> {
    if !interlaced {
        return vec![Pass { x0: 0, y0: 0, dx: 1, dy: 1, w: width, h: height }];
    }
    const X0: [usize; 7] = [0, 4, 0, 2, 0, 1, 0];
    const Y0: [usize; 7] = [0, 0, 4, 0, 2, 0, 1];
    const DX: [usize; 7] = [8, 8, 4, 4, 2, 2, 1];
    const DY: [usize; 7] = [8, 8, 8, 4, 4, 2, 2];
    (0..7)
        .map(|p| {
            let count = |size: usize, start: usize, step: usize| {
                if size > start {
                    (size - start).div_ceil(step)
                } else {
                    0
                }
            };
            Pass {
                x0: X0[p],
                y0: Y0[p],
                dx: DX[p],
                dy: DY[p],
                w: count(width, X0[p], DX[p]),
                h: count(height, Y0[p], DY[p]),
            }
        })
        .collect()
}

fn decode_pixels(hdr: &Header, idat: &[u8], palette: &[[u8; 3]], trns: &Trns) -> Result<Image, PngError> {
    let (w, h) = (hdr.width as usize, hdr.height as usize);
    let passes = passes(w, h, hdr.interlaced);
    let mut expected: u64 = 0;
    for p in &passes {
        if p.w > 0 && p.h > 0 {
            let line = (p.w as u64 * hdr.bits_per_pixel() as u64).div_ceil(8) + 1;
            expected += line * p.h as u64;
        }
    }
    let expected = usize::try_from(expected).map_err(|_| PngError::TooLarge)?;
    let mut raw = zlib_decompress_limited(idat, expected).map_err(|e| {
        if e.0.contains("output limit") {
            PngError::Corrupt("more image data than the picture holds")
        } else {
            PngError::Compressed(e.0)
        }
    })?;
    if raw.len() != expected {
        return Err(PngError::Corrupt("less image data than the picture holds"));
    }

    let bpp = hdr.bits_per_pixel().div_ceil(8).max(1);
    let mut pixels = vec![0u8; w * h * 4];
    let mut offset = 0usize;
    for p in &passes {
        if p.w == 0 || p.h == 0 {
            continue;
        }
        let stride = hdr.row_bytes(p.w);
        let size = (stride + 1) * p.h;
        let buf = raw.get_mut(offset..offset + size).ok_or(PngError::Corrupt("image data too short"))?;
        offset += size;
        unfilter(buf, stride, p.h, bpp)?;
        for j in 0..p.h {
            let row = &buf[j * (stride + 1) + 1..(j + 1) * (stride + 1)];
            let y = p.y0 + j * p.dy;
            for i in 0..p.w {
                let x = p.x0 + i * p.dx;
                let px = pixel_at(hdr, row, i, palette, trns);
                let d = (y * w + x) * 4;
                if let Some(dst) = pixels.get_mut(d..d + 4) {
                    dst.copy_from_slice(&px);
                }
            }
        }
    }
    Ok(Image { width: hdr.width, height: hdr.height, pixels })
}

/// Reverses the scanline filters of one pass in place. `buf` holds `rows` lines of a filter byte
/// followed by `stride` bytes; `bpp` is the filter's byte distance (bytes per pixel, at least 1).
fn unfilter(buf: &mut [u8], stride: usize, rows: usize, bpp: usize) -> Result<(), PngError> {
    let line = stride + 1;
    if buf.len() < line * rows {
        return Err(PngError::Corrupt("image data too short"));
    }
    for r in 0..rows {
        let (before, rest) = buf.split_at_mut(r * line);
        let cur = &mut rest[..line];
        let filter = cur[0];
        let cur = &mut cur[1..];
        let prev: &[u8] = if r == 0 { &[] } else { &before[(r - 1) * line + 1..r * line] };
        match filter {
            0 => {}
            1 => {
                for i in bpp..stride {
                    cur[i] = cur[i].wrapping_add(cur[i - bpp]);
                }
            }
            2 => {
                if !prev.is_empty() {
                    for i in 0..stride {
                        cur[i] = cur[i].wrapping_add(prev[i]);
                    }
                }
            }
            3 => {
                for i in 0..stride {
                    let a = if i >= bpp { cur[i - bpp] as u16 } else { 0 };
                    let b = if prev.is_empty() { 0 } else { prev[i] as u16 };
                    cur[i] = cur[i].wrapping_add(((a + b) / 2) as u8);
                }
            }
            4 => {
                for i in 0..stride {
                    let a = if i >= bpp { cur[i - bpp] } else { 0 };
                    let (b, c) = if prev.is_empty() {
                        (0, 0)
                    } else {
                        (prev[i], if i >= bpp { prev[i - bpp] } else { 0 })
                    };
                    cur[i] = cur[i].wrapping_add(paeth(a, b, c));
                }
            }
            _ => return Err(PngError::Corrupt("unknown filter type")),
        }
    }
    Ok(())
}

/// The Paeth predictor: whichever of left, above and upper-left is closest to left + above - upper-left.
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

/// Sample number `i` of a row (counting every channel of every pixel), as stored.
fn sample(row: &[u8], i: usize, depth: u8) -> u16 {
    match depth {
        8 => row.get(i).copied().unwrap_or(0) as u16,
        16 => row.get(2 * i..2 * i + 2).map(be16).unwrap_or(0),
        d => {
            let d = d as usize;
            let bit = i * d;
            let byte = row.get(bit / 8).copied().unwrap_or(0) as usize;
            ((byte >> (8 - d - bit % 8)) & ((1 << d) - 1)) as u16
        }
    }
}

/// A stored sample scaled to 8 bits.
fn to8(v: u16, depth: u8) -> u8 {
    match depth {
        1 => (v & 1) as u8 * 255,
        2 => (v & 3) as u8 * 85,
        4 => (v & 15) as u8 * 17,
        8 => v as u8,
        _ => ((v as u32 * 255 + 32767) / 65535) as u8,
    }
}

fn pixel_at(hdr: &Header, row: &[u8], i: usize, palette: &[[u8; 3]], trns: &Trns) -> [u8; 4] {
    let d = hdr.depth;
    match hdr.ctype {
        0 => {
            let g = sample(row, i, d);
            let a = if matches!(trns, Trns::Grey(k) if *k == g) { 0 } else { 255 };
            let v = to8(g, d);
            [v, v, v, a]
        }
        2 => {
            let (r, g, b) = (sample(row, 3 * i, d), sample(row, 3 * i + 1, d), sample(row, 3 * i + 2, d));
            let a = if matches!(trns, Trns::Rgb(kr, kg, kb) if (*kr, *kg, *kb) == (r, g, b)) { 0 } else { 255 };
            [to8(r, d), to8(g, d), to8(b, d), a]
        }
        3 => {
            let idx = sample(row, i, d) as usize;
            match palette.get(idx) {
                Some(&[r, g, b]) => {
                    let a = match trns {
                        Trns::Palette(alpha) => alpha.get(idx).copied().unwrap_or(255),
                        _ => 255,
                    };
                    [r, g, b, a]
                }
                None => [0, 0, 0, 255],
            }
        }
        4 => {
            let v = to8(sample(row, 2 * i, d), d);
            [v, v, v, to8(sample(row, 2 * i + 1, d), d)]
        }
        _ => [
            to8(sample(row, 4 * i, d), d),
            to8(sample(row, 4 * i + 1, d), d),
            to8(sample(row, 4 * i + 2, d), d),
            to8(sample(row, 4 * i + 3, d), d),
        ],
    }
}

// ---------------------------------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------------------------------

/// Encodes a picture as an 8-bit PNG. See the module notes for the colour type and filters chosen.
pub fn encode(img: &Image, opts: EncodeOptions) -> Vec<u8> {
    encode_raw(img.width, img.height, &img.pixels, opts)
}

/// Encodes `width x height` RGBA pixels (row-major, top row first) with the default options.
pub fn encode_rgba8(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    encode_raw(width, height, rgba, EncodeOptions::default())
}

fn encode_raw(width: u32, height: u32, rgba: &[u8], opts: EncodeOptions) -> Vec<u8> {
    let (src_w, w, h, src): (usize, usize, usize, &[u8]) = if width == 0 || height == 0 {
        (1, 1, 1, &[0, 0, 0, 0])
    } else {
        (width as usize, width.min(MAX_DIMENSION) as usize, height.min(MAX_DIMENSION) as usize, rgba)
    };
    let px = |x: usize, y: usize| -> [u8; 4] {
        let i = y.saturating_mul(src_w).saturating_add(x).saturating_mul(4);
        match src.get(i..i.saturating_add(4)) {
            Some(p) => [p[0], p[1], p[2], p[3]],
            None => [0, 0, 0, 0],
        }
    };

    let (mut opaque, mut grey) = (true, true);
    'scan: for y in 0..h {
        for x in 0..w {
            let p = px(x, y);
            opaque &= p[3] == 255;
            grey &= p[0] == p[1] && p[1] == p[2];
            if !opaque && !grey {
                break 'scan;
            }
        }
    }
    let (ctype, channels): (u8, usize) = match (grey, opaque) {
        (true, true) => (0, 1),
        (true, false) => (4, 2),
        (false, true) => (2, 3),
        (false, false) => (6, 4),
    };

    let stride = w * channels;
    let mut filtered = Vec::with_capacity((stride + 1).saturating_mul(h).min(64 << 20));
    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut cand = vec![0u8; stride];
    let mut best = vec![0u8; stride];
    for y in 0..h {
        for x in 0..w {
            let p = px(x, y);
            let o = x * channels;
            match channels {
                1 => cur[o] = p[0],
                2 => cur[o..o + 2].copy_from_slice(&[p[0], p[3]]),
                3 => cur[o..o + 3].copy_from_slice(&p[..3]),
                _ => cur[o..o + 4].copy_from_slice(&p),
            }
        }
        let first = y == 0;
        let mut best_filter = 0u8;
        if opts.level == 0 {
            best.copy_from_slice(&cur);
        } else {
            let mut best_score = u64::MAX;
            for f in 0..5u8 {
                // Up, and Paeth on the first row, add nothing over None and Sub.
                if first && (f == 2 || f == 4) {
                    continue;
                }
                filter_row(f, &cur, &prev, channels, &mut cand);
                let score: u64 = cand.iter().map(|&b| (b as i8).unsigned_abs() as u64).sum();
                if score < best_score {
                    best_score = score;
                    best_filter = f;
                    std::mem::swap(&mut best, &mut cand);
                }
            }
        }
        filtered.push(best_filter);
        filtered.extend_from_slice(&best);
        std::mem::swap(&mut prev, &mut cur);
    }

    let compressed = zlib_compress(&filtered, opts.level);
    let mut out = Vec::with_capacity(compressed.len() + 128);
    out.extend_from_slice(&SIGNATURE);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, ctype, 0, 0, 0]);
    write_chunk(&mut out, b"IHDR", &ihdr);
    if let Some((dx, dy)) = opts.dpi {
        let ppm = |dpi: f64| -> Option<u32> {
            let v = (dpi / 0.0254).round();
            if v.is_finite() && v >= 1.0 && v <= u32::MAX as f64 {
                Some(v as u32)
            } else {
                None
            }
        };
        if let (Some(x), Some(y)) = (ppm(dx), ppm(dy)) {
            let mut phys = Vec::with_capacity(9);
            phys.extend_from_slice(&x.to_be_bytes());
            phys.extend_from_slice(&y.to_be_bytes());
            phys.push(1);
            write_chunk(&mut out, b"pHYs", &phys);
        }
    }
    for part in compressed.chunks(IDAT_CHUNK) {
        write_chunk(&mut out, b"IDAT", part);
    }
    write_chunk(&mut out, b"IEND", &[]);
    out
}

/// Applies filter `f` to `cur` (with `prev` the unfiltered row above, zeros for the first row).
fn filter_row(f: u8, cur: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) {
    for i in 0..cur.len() {
        let a = if i >= bpp { cur[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        let pred = match f {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((a as u16 + b as u16) / 2) as u8,
            _ => paeth(a, b, c),
        };
        out[i] = cur[i].wrapping_sub(pred);
    }
}

fn write_chunk(out: &mut Vec<u8>, ty: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(body);
    let mut crc = Crc32::new();
    crc.update(ty);
    crc.update(body);
    out.extend_from_slice(&crc.finish().to_be_bytes());
}

// ---------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground::flate::crc32;

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }

        fn byte(&mut self) -> u8 {
            self.next() as u8
        }
    }

    fn make(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                pixels.extend_from_slice(&f(x, y));
            }
        }
        Image { width: w, height: h, pixels }
    }

    fn round_trip(img: &Image) -> (Vec<u8>, Image) {
        let png = encode(img, EncodeOptions::default());
        let back = decode(&png).expect("our own PNG decodes");
        assert_eq!((back.width, back.height), (img.width, img.height));
        assert!(back.pixels == img.pixels, "pixels differ after round trip");
        (png, back)
    }

    /// The colour type the encoder chose, read straight from IHDR.
    fn ctype_of(png: &[u8]) -> u8 {
        png[25]
    }

    // ----- a tiny raw PNG builder, independent of the encoder ------------------------------------

    struct Raw {
        w: usize,
        h: usize,
        depth: u8,
        ctype: u8,
        interlace: bool,
        /// Filter type per row, cycled.
        filters: Vec<u8>,
        /// Chunks written between IHDR and IDAT.
        before: Vec<([u8; 4], Vec<u8>)>,
        /// Size of each IDAT chunk.
        idat_split: usize,
    }

    impl Raw {
        fn new(w: usize, h: usize, depth: u8, ctype: u8) -> Raw {
            Raw { w, h, depth, ctype, interlace: false, filters: vec![0], before: Vec::new(), idat_split: 1 << 20 }
        }

        fn channels(&self) -> usize {
            match self.ctype {
                0 | 3 => 1,
                2 => 3,
                4 => 2,
                _ => 4,
            }
        }

        /// Builds the file from raw samples: `samples(x, y)` gives one value per channel at this depth.
        fn build(&self, samples: &dyn Fn(usize, usize) -> Vec<u16>) -> Vec<u8> {
            let ch = self.channels();
            let bits = ch * self.depth as usize;
            let bpp = bits.div_ceil(8).max(1);
            let geometry: Vec<(usize, usize, usize, usize)> = if self.interlace {
                vec![(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
            } else {
                vec![(0, 0, 1, 1)]
            };
            let mut stream = Vec::new();
            let mut row_no = 0usize;
            for (x0, y0, dx, dy) in geometry {
                let xs: Vec<usize> = (x0..self.w).step_by(dx).collect();
                let ys: Vec<usize> = (y0..self.h).step_by(dy).collect();
                if xs.is_empty() || ys.is_empty() {
                    continue;
                }
                let stride = (xs.len() * bits).div_ceil(8);
                let mut prev = vec![0u8; stride];
                for &y in &ys {
                    // Pack the samples, most significant bits first.
                    let mut row = vec![0u8; stride];
                    let mut bit = 0usize;
                    for &x in &xs {
                        let s = samples(x, y);
                        assert_eq!(s.len(), ch);
                        for v in s {
                            match self.depth {
                                16 => {
                                    row[bit / 8] = (v >> 8) as u8;
                                    row[bit / 8 + 1] = v as u8;
                                }
                                8 => row[bit / 8] = v as u8,
                                d => row[bit / 8] |= (v as u8) << (8 - d as usize - bit % 8),
                            }
                            bit += self.depth as usize;
                        }
                    }
                    let f = self.filters[row_no % self.filters.len()];
                    row_no += 1;
                    stream.push(f);
                    for i in 0..stride {
                        let a = if i >= bpp { row[i - bpp] as i32 } else { 0 };
                        let b = prev[i] as i32;
                        let c = if i >= bpp { prev[i - bpp] as i32 } else { 0 };
                        let pred = match f {
                            0 => 0,
                            1 => a,
                            2 => b,
                            3 => (a + b) / 2,
                            _ => {
                                let p = a + b - c;
                                let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                                if pa <= pb && pa <= pc {
                                    a
                                } else if pb <= pc {
                                    b
                                } else {
                                    c
                                }
                            }
                        };
                        stream.push((row[i] as i32 - pred) as u8);
                    }
                    prev = row;
                }
            }
            let z = zlib_compress(&stream, 6);
            let mut out = SIGNATURE.to_vec();
            let mut ihdr = Vec::new();
            ihdr.extend_from_slice(&(self.w as u32).to_be_bytes());
            ihdr.extend_from_slice(&(self.h as u32).to_be_bytes());
            ihdr.extend_from_slice(&[self.depth, self.ctype, 0, 0, self.interlace as u8]);
            raw_chunk(&mut out, b"IHDR", &ihdr);
            for (t, b) in &self.before {
                raw_chunk(&mut out, t, b);
            }
            for part in z.chunks(self.idat_split.max(1)) {
                raw_chunk(&mut out, b"IDAT", part);
            }
            raw_chunk(&mut out, b"IEND", &[]);
            out
        }
    }

    fn raw_chunk(out: &mut Vec<u8>, ty: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut tb = ty.to_vec();
        tb.extend_from_slice(body);
        out.extend_from_slice(&tb);
        out.extend_from_slice(&crc32(&tb).to_be_bytes());
    }

    /// Independent expected scaling to 8 bits.
    fn scale(v: u16, depth: u8) -> u8 {
        let max = (1u32 << depth) - 1;
        ((v as f64) * 255.0 / max as f64).round() as u8
    }

    /// A deterministic sample pattern for (x, y, channel) at a depth.
    fn pattern(x: usize, y: usize, c: usize, depth: u8) -> u16 {
        let max = ((1u32 << depth) - 1) as usize;
        let v = (x * 7 + y * 13 + c * 29 + x * y * 3) ^ (y << 3);
        (v % (max + 1)) as u16
    }

    /// Checks a build of every pixel against expectations computed from the pattern.
    fn check_type(raw: &Raw, palette: Option<&[[u8; 3]]>) {
        let ch = raw.channels();
        let d = raw.depth;
        let png = raw.build(&|x, y| (0..ch).map(|c| pattern(x, y, c, d)).collect());
        let (img, info) = decode_with_info(&png).unwrap_or_else(|e| {
            panic!("ctype {} depth {} interlace {}: {e}", raw.ctype, d, raw.interlace)
        });
        assert_eq!((img.width as usize, img.height as usize), (raw.w, raw.h));
        assert_eq!((info.bit_depth, info.color_type, info.interlaced), (d, raw.ctype, raw.interlace));
        for y in 0..raw.h {
            for x in 0..raw.w {
                let s: Vec<u16> = (0..ch).map(|c| pattern(x, y, c, d)).collect();
                let want = match raw.ctype {
                    0 => {
                        let g = scale(s[0], d);
                        [g, g, g, 255]
                    }
                    2 => [scale(s[0], d), scale(s[1], d), scale(s[2], d), 255],
                    3 => {
                        let p = palette.unwrap()[s[0] as usize];
                        [p[0], p[1], p[2], 255]
                    }
                    4 => {
                        let g = scale(s[0], d);
                        [g, g, g, scale(s[1], d)]
                    }
                    _ => [scale(s[0], d), scale(s[1], d), scale(s[2], d), scale(s[3], d)],
                };
                assert_eq!(
                    img.pixel(x as u32, y as u32).unwrap(),
                    want,
                    "ctype {} depth {} interlace {} at ({x},{y})",
                    raw.ctype,
                    d,
                    raw.interlace
                );
            }
        }
    }

    fn full_palette(depth: u8) -> Vec<[u8; 3]> {
        (0..1usize << depth).map(|i| [(i * 37) as u8, (255 - i) as u8, (i * 11 + 5) as u8]).collect()
    }

    fn plte_body(p: &[[u8; 3]]) -> Vec<u8> {
        p.iter().flatten().copied().collect()
    }

    // ----- round trips ------------------------------------------------------------------------------

    #[test]
    fn round_trip_gradient() {
        let img = make(64, 48, |x, y| [(x * 4) as u8, (y * 5) as u8, ((x + y) * 2) as u8, 255]);
        let (png, _) = round_trip(&img);
        assert_eq!(ctype_of(&png), 2);
    }

    #[test]
    fn round_trip_checker() {
        let img = make(40, 30, |x, y| if (x / 5 + y / 5) % 2 == 0 { [230, 40, 40, 255] } else { [30, 60, 200, 255] });
        round_trip(&img);
    }

    #[test]
    fn round_trip_random_with_alpha() {
        let mut rng = Rng(0x1234_5678_9abc_def1);
        let mut img = make(37, 29, |_, _| [0, 0, 0, 0]);
        for b in img.pixels.iter_mut() {
            *b = rng.byte();
        }
        let (png, _) = round_trip(&img);
        assert_eq!(ctype_of(&png), 6);
        for level in [0, 1, 4, 9] {
            let png = encode(&img, EncodeOptions { level, dpi: None });
            assert_eq!(decode(&png).unwrap(), img, "level {level}");
        }
    }

    #[test]
    fn round_trip_all_grey() {
        let img = make(33, 17, |x, y| {
            let v = (x * 7 + y * 3) as u8;
            [v, v, v, 255]
        });
        let (png, _) = round_trip(&img);
        assert_eq!(ctype_of(&png), 0);
        let img = make(33, 17, |x, y| {
            let v = (x * 7 + y * 3) as u8;
            [v, v, v, (x * 9) as u8]
        });
        let (png, _) = round_trip(&img);
        assert_eq!(ctype_of(&png), 4);
    }

    #[test]
    fn round_trip_odd_shapes() {
        for (w, h) in [(1, 1), (1, 50), (50, 1), (257, 3), (3, 257), (2, 2), (9, 9)] {
            let img = make(w, h, |x, y| [(x * 31 + y) as u8, (y * 17) as u8, (x ^ y) as u8, ((x + y) * 50) as u8]);
            round_trip(&img);
            let opaque = make(w, h, |x, y| [(x * 31 + y) as u8, (y * 17) as u8, (x ^ y) as u8, 255]);
            round_trip(&opaque);
        }
    }

    #[test]
    fn round_trip_large_spans_many_idat_chunks() {
        let mut rng = Rng(77);
        let mut img = make(300, 300, |_, _| [0, 0, 0, 0]);
        for b in img.pixels.iter_mut() {
            *b = rng.byte();
        }
        let png = encode(&img, EncodeOptions { level: 1, dpi: None });
        let idats = png.windows(4).filter(|w| w == b"IDAT").count();
        assert!(idats > 1, "expected several IDAT chunks, got {idats}");
        assert_eq!(decode(&png).unwrap(), img);
    }

    #[test]
    fn encode_rgba8_and_dpi() {
        let px: Vec<u8> = (0..4 * 5 * 4).map(|i| (i * 13) as u8).collect();
        let png = encode_rgba8(4, 5, &px);
        let img = decode(&png).unwrap();
        assert_eq!(img.pixels, px);
        let png = encode(&img, EncodeOptions { level: 6, dpi: Some((300.0, 150.0)) });
        let (back, info) = decode_with_info(&png).unwrap();
        assert_eq!(back, img);
        let (dx, dy) = info.dpi.unwrap();
        assert!((dx - 300.0).abs() < 0.05 && (dy - 150.0).abs() < 0.05, "{dx} {dy}");
    }

    #[test]
    fn encode_degenerate_inputs() {
        let png = encode_rgba8(0, 10, &[]);
        let img = decode(&png).unwrap();
        assert_eq!((img.width, img.height, img.pixels.clone()), (1, 1, vec![0, 0, 0, 0]));
        // A short buffer: the missing pixels are transparent black.
        let png = encode_rgba8(2, 2, &[10, 20, 30, 255]);
        let img = decode(&png).unwrap();
        assert_eq!(img.pixels, vec![10, 20, 30, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn adaptive_filters_are_used() {
        // A smooth gradient should pick something other than None on most rows.
        let img = make(64, 64, |x, y| [(x * 3 + y) as u8, (y * 2) as u8, (x + 2 * y) as u8, 255]);
        let png = encode(&img, EncodeOptions::default());
        let raw = zlib_decompress_limited(&idat_of(&png), usize::MAX).unwrap();
        let line = 64 * 3 + 1;
        let non_none = (0..64).filter(|r| raw[r * line] != 0).count();
        assert!(non_none > 32, "only {non_none} rows filtered");
        assert_eq!(decode(&png).unwrap(), img);
    }

    fn idat_of(png: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut pos = 8;
        while pos + 8 <= png.len() {
            let len = be32(&png[pos..]) as usize;
            if &png[pos + 4..pos + 8] == b"IDAT" {
                out.extend_from_slice(&png[pos + 8..pos + 8 + len]);
            }
            pos += 12 + len;
        }
        out
    }

    // ----- hand-built files -------------------------------------------------------------------------

    #[test]
    fn every_colour_type_and_depth() {
        let combos: [(u8, &[u8]); 5] =
            [(0, &[1, 2, 4, 8, 16]), (2, &[8, 16]), (3, &[1, 2, 4, 8]), (4, &[8, 16]), (6, &[8, 16])];
        for (ctype, depths) in combos {
            for &depth in depths {
                for interlace in [false, true] {
                    for (w, h) in [(13, 11), (1, 1), (3, 9), (17, 2)] {
                        let mut raw = Raw::new(w, h, depth, ctype);
                        raw.interlace = interlace;
                        raw.filters = vec![0, 1, 2, 3, 4];
                        raw.idat_split = 7;
                        let pal = if ctype == 3 { full_palette(depth) } else { Vec::new() };
                        if ctype == 3 {
                            raw.before.push((*b"PLTE", plte_body(&pal)));
                        }
                        check_type(&raw, Some(&pal));
                    }
                }
            }
        }
    }

    #[test]
    fn each_filter_type_alone() {
        for f in 0..5u8 {
            for (ctype, depth) in [(2u8, 8u8), (6, 16), (0, 1), (0, 4), (4, 8)] {
                let mut raw = Raw::new(19, 7, depth, ctype);
                raw.filters = vec![f];
                check_type(&raw, None);
                raw.interlace = true;
                check_type(&raw, None);
            }
        }
    }

    #[test]
    fn sixteen_bit_downscaling() {
        let mut raw = Raw::new(4, 1, 16, 0);
        raw.filters = vec![0];
        let vals = [0u16, 0x00FF, 0x8080, 0xFFFF];
        let png = raw.build(&|x, _| vec![vals[x]]);
        let img = decode(&png).unwrap();
        let got: Vec<u8> = img.pixels.chunks(4).map(|p| p[0]).collect();
        assert_eq!(got, vec![0, 1, 128, 255]);
        // round(v * 255 / 65535) for every value.
        for v in (0..=0xFFFFu32).step_by(97) {
            let want = ((v as f64) * 255.0 / 65535.0).round() as u8;
            assert_eq!(to8(v as u16, 16), want, "v = {v}");
        }
    }

    #[test]
    fn palette_with_trns() {
        let pal = full_palette(2);
        let mut raw = Raw::new(8, 4, 2, 3);
        raw.before.push((*b"PLTE", plte_body(&pal)));
        // Alpha for entries 0 and 1 only; 2 and 3 stay opaque.
        raw.before.push((*b"tRNS", vec![0, 128]));
        let png = raw.build(&|x, y| vec![((x + y) % 4) as u16]);
        let img = decode(&png).unwrap();
        for y in 0..4 {
            for x in 0..8 {
                let i = (x + y) % 4;
                let a = [0, 128, 255, 255][i];
                let p = pal[i];
                assert_eq!(img.pixel(x as u32, y as u32).unwrap(), [p[0], p[1], p[2], a]);
            }
        }
    }

    #[test]
    fn grey_and_rgb_key_colours() {
        let mut raw = Raw::new(4, 1, 4, 0);
        raw.before.push((*b"tRNS", vec![0, 9]));
        let png = raw.build(&|x, _| vec![[3u16, 9, 15, 9][x]]);
        let img = decode(&png).unwrap();
        let alphas: Vec<u8> = img.pixels.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alphas, vec![255, 0, 255, 0]);

        let mut raw = Raw::new(3, 1, 16, 2);
        raw.before.push((*b"tRNS", vec![0x12, 0x34, 0, 5, 0xFF, 0xFF]));
        let png = raw.build(&|x, _| [vec![0x1234, 5, 0xFFFF], vec![0x1234, 6, 0xFFFF], vec![0, 0, 0]][x].clone());
        let img = decode(&png).unwrap();
        let alphas: Vec<u8> = img.pixels.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alphas, vec![0, 255, 255]);

        // A tRNS of the wrong length is ignored.
        let mut raw = Raw::new(2, 1, 8, 2);
        raw.before.push((*b"tRNS", vec![0, 1]));
        let png = raw.build(&|_, _| vec![0, 1, 0]);
        assert!(decode(&png).unwrap().pixels.chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn palette_index_out_of_range_is_black() {
        let mut raw = Raw::new(4, 1, 8, 3);
        raw.before.push((*b"PLTE", vec![255, 0, 0, 0, 255, 0]));
        let png = raw.build(&|x, _| vec![x as u16]);
        let img = decode(&png).unwrap();
        assert_eq!(img.pixels, vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn info_chunks_are_read() {
        let mut raw = Raw::new(2, 2, 8, 6);
        raw.before.push((*b"gAMA", 45455u32.to_be_bytes().to_vec()));
        raw.before.push((*b"sRGB", vec![0]));
        let mut phys = 3780u32.to_be_bytes().to_vec();
        phys.extend_from_slice(&3780u32.to_be_bytes());
        phys.push(1);
        raw.before.push((*b"pHYs", phys));
        raw.before.push((*b"tEXt", b"Title\0Caf\xe9".to_vec()));
        let mut z = b"Comment\0\0".to_vec();
        z.extend_from_slice(&zlib_compress(b"squeezed words", 6));
        raw.before.push((*b"zTXt", z));
        let mut it = b"Author\0\0\0en\0Auteur\0".to_vec();
        it.extend_from_slice("Kiyo \u{263A}".as_bytes());
        raw.before.push((*b"iTXt", it));
        let mut itz = b"Note\0\x01\0\0\0".to_vec();
        itz.extend_from_slice(&zlib_compress("compressed \u{00FC}".as_bytes(), 9));
        raw.before.push((*b"iTXt", itz));
        raw.before.push((*b"acTL", vec![0, 0, 0, 1, 0, 0, 0, 0]));
        raw.before.push((*b"zTXt", b"Broken\0\0notzlib".to_vec()));
        raw.before.push((*b"tEXt", b"no separator".to_vec()));
        raw.before.push((*b"prVt", vec![1, 2, 3]));
        let png = raw.build(&|x, y| vec![x as u16, y as u16, 7, 255]);
        let (_, info) = decode_with_info(&png).unwrap();
        assert!((info.gamma.unwrap() - 0.45455).abs() < 1e-9);
        assert!(info.srgb);
        let (dx, dy) = info.dpi.unwrap();
        assert!((dx - 96.012).abs() < 1e-6 && (dy - 96.012).abs() < 1e-6);
        assert!(info.animated);
        assert_eq!(
            info.text,
            vec![
                ("Title".to_string(), "Caf\u{e9}".to_string()),
                ("Comment".to_string(), "squeezed words".to_string()),
                ("Author".to_string(), "Kiyo \u{263A}".to_string()),
                ("Note".to_string(), "compressed \u{00FC}".to_string()),
            ]
        );
    }

    #[test]
    fn apng_default_image_decodes() {
        // An acTL/fcTL before IDAT and an fdAT after it: only IDAT's picture comes back.
        let mut raw = Raw::new(3, 2, 8, 2);
        raw.before.push((*b"acTL", vec![0, 0, 0, 2, 0, 0, 0, 0]));
        raw.before.push((*b"fcTL", vec![0; 26]));
        let mut png = raw.build(&|x, y| vec![x as u16 * 50, y as u16 * 90, 1]);
        let iend = png.len() - 12;
        let mut tail = Vec::new();
        raw_chunk(&mut tail, b"fdAT", &[0, 0, 0, 2, 1, 2, 3]);
        png.splice(iend..iend, tail);
        let (img, info) = decode_with_info(&png).unwrap();
        assert!(info.animated);
        assert_eq!(img.pixel(2, 1).unwrap(), [100, 90, 1, 255]);
    }

    // ----- corrupt data -----------------------------------------------------------------------------

    fn sample_png() -> Vec<u8> {
        let img = make(9, 7, |x, y| [(x * 20) as u8, (y * 30) as u8, 99, (x * y * 4) as u8]);
        encode(&img, EncodeOptions::default())
    }

    /// Recomputes the CRC of every complete chunk.
    fn fix_crcs(png: &mut [u8]) {
        let mut pos = 8;
        while pos + 12 <= png.len() {
            let len = be32(&png[pos..]) as usize;
            let end = match pos.checked_add(12).and_then(|e| e.checked_add(len)) {
                Some(e) if e <= png.len() => e,
                _ => return,
            };
            let crc = crc32(&png[pos + 4..end - 4]);
            png[end - 4..end].copy_from_slice(&crc.to_be_bytes());
            pos = end;
        }
    }

    #[test]
    fn bad_signature() {
        assert_eq!(decode(b""), Err(PngError::NotPng));
        assert_eq!(decode(b"\x89PNG\r\n\x1a"), Err(PngError::NotPng));
        assert_eq!(decode(b"GIF89a.........."), Err(PngError::NotPng));
        assert_eq!(decode(&SIGNATURE), Err(PngError::Truncated));
    }

    #[test]
    fn corrupt_crc() {
        let mut png = sample_png();
        // Flip a byte of IHDR's body.
        png[20] ^= 1;
        assert_eq!(decode(&png), Err(PngError::BadCrc(*b"IHDR")));
        let mut png = sample_png();
        let last_idat_crc = png.len() - 12 - 1;
        png[last_idat_crc] ^= 0x40;
        assert_eq!(decode(&png), Err(PngError::BadCrc(*b"IDAT")));
    }

    #[test]
    fn lenient_ancillary_crc() {
        let mut raw = Raw::new(2, 1, 8, 0);
        raw.before.push((*b"gAMA", 45455u32.to_be_bytes().to_vec()));
        let mut png = raw.build(&|x, _| vec![x as u16 * 200]);
        // gAMA starts right after IHDR (8 + 25); spoil its CRC.
        png[33 + 15] ^= 0xFF;
        assert_eq!(decode(&png), Err(PngError::BadCrc(*b"gAMA")));
        let (img, info) = decode_with_options(&png, DecodeOptions { check_ancillary_crc: false }).unwrap();
        assert_eq!(info.gamma, None);
        assert_eq!(img.pixels, vec![0, 0, 0, 255, 200, 200, 200, 255]);
    }

    #[test]
    fn truncated_chunks() {
        let png = sample_png();
        for cut in [9, 15, 20, 33, 40, png.len() - 13, png.len() - 20] {
            let r = decode(&png[..cut]);
            assert!(r.is_err(), "cut at {cut} decoded");
        }
        assert_eq!(decode(&png[..png.len() - 5]), Err(PngError::Truncated));
        // Missing only IEND: still decodes.
        assert!(decode(&png[..png.len() - 12]).is_ok());
    }

    #[test]
    fn bad_ihdr() {
        let base = sample_png();
        let cases: [(usize, u8); 6] = [
            (24, 3), // bit depth 3
            (25, 1), // colour type 1
            (26, 1), // compression method
            (27, 1), // filter method
            (28, 2), // interlace method
            (25, 7), // colour type 7
        ];
        for (i, (at, v)) in cases.iter().enumerate() {
            let mut png = base.clone();
            png[*at] = *v;
            fix_crcs(&mut png);
            assert!(matches!(decode(&png), Err(PngError::Corrupt(_))), "case {i}: {:?}", decode(&png));
        }
        // Palette at 16 bits.
        let mut png = base.clone();
        png[24] = 16;
        png[25] = 3;
        fix_crcs(&mut png);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
        // Zero width.
        let mut png = base.clone();
        png[16..20].copy_from_slice(&[0, 0, 0, 0]);
        fix_crcs(&mut png);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
        // IHDR of the wrong length.
        let mut png = SIGNATURE.to_vec();
        raw_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0]);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
        // A first chunk that is not IHDR.
        let mut png = SIGNATURE.to_vec();
        raw_chunk(&mut png, b"IDAT", &[1, 2, 3]);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
        // Palette type without PLTE.
        let raw = Raw::new(2, 2, 8, 3);
        let png = raw.build(&|_, _| vec![0]);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
        // Unknown critical chunk.
        let mut raw = Raw::new(2, 2, 8, 0);
        raw.before.push((*b"CgBI", vec![0, 0, 0, 0]));
        let png = raw.build(&|_, _| vec![0]);
        assert!(matches!(decode(&png), Err(PngError::Unsupported(_))));
        // Unknown filter type in the data.
        let mut raw = Raw::new(2, 2, 8, 0);
        raw.filters = vec![5];
        let png = raw.build(&|_, _| vec![0]);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
    }

    #[test]
    fn oversized_dimensions() {
        let mut png = sample_png();
        png[16..20].copy_from_slice(&40_000u32.to_be_bytes());
        png[20..24].copy_from_slice(&40_000u32.to_be_bytes());
        fix_crcs(&mut png);
        assert_eq!(decode(&png), Err(PngError::TooLarge));
        let mut png = sample_png();
        png[16..20].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        fix_crcs(&mut png);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
    }

    #[test]
    fn wrong_amount_of_image_data() {
        // The header says 9x8 but the data holds 9x7 rows; and the other way round.
        let mut png = sample_png();
        png[20..24].copy_from_slice(&8u32.to_be_bytes());
        fix_crcs(&mut png);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
        let mut png = sample_png();
        png[20..24].copy_from_slice(&6u32.to_be_bytes());
        fix_crcs(&mut png);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
        // A decompression bomb: a 1x1 header over a megabyte of zeros stops at the expected size.
        let mut raw = Raw::new(1, 1, 8, 0);
        raw.filters = vec![0];
        let mut png = raw.build(&|_, _| vec![0]);
        let bomb = zlib_compress(&vec![0u8; 1 << 20], 9);
        let start = 33;
        let len = be32(&png[start..]) as usize;
        let mut idat = Vec::new();
        raw_chunk(&mut idat, b"IDAT", &bomb);
        png.splice(start..start + 12 + len, idat);
        assert!(matches!(decode(&png), Err(PngError::Corrupt(_))));
    }

    #[test]
    fn fuzz_mutations_never_panic() {
        let mut seeds = vec![sample_png()];
        let mut raw = Raw::new(11, 9, 2, 3);
        raw.interlace = true;
        raw.filters = vec![0, 1, 2, 3, 4];
        raw.idat_split = 13;
        raw.before.push((*b"PLTE", plte_body(&full_palette(2))));
        raw.before.push((*b"tRNS", vec![0, 100, 200]));
        raw.before.push((*b"tEXt", b"k\0v".to_vec()));
        seeds.push(raw.build(&|x, y| vec![((x * y + x) % 4) as u16]));
        let mut raw = Raw::new(7, 5, 16, 6);
        raw.interlace = true;
        raw.filters = vec![4, 3, 1];
        seeds.push(raw.build(&|x, y| (0..4).map(|c| pattern(x, y, c, 16)).collect()));
        let mut raw = Raw::new(10, 3, 1, 0);
        raw.before.push((*b"tRNS", vec![0, 1]));
        seeds.push(raw.build(&|x, y| vec![pattern(x, y, 0, 1)]));

        let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
        let mut decoded = 0usize;
        for round in 0..4000 {
            let mut png = seeds[round % seeds.len()].clone();
            let edits = 1 + rng.below(4);
            for _ in 0..edits {
                match rng.below(6) {
                    0 | 1 | 2 => {
                        let i = rng.below(png.len());
                        png[i] = rng.byte();
                    }
                    3 => {
                        let i = rng.below(png.len());
                        png[i] ^= 1 << rng.below(8);
                    }
                    4 => {
                        let cut = 8 + rng.below(png.len() - 8);
                        png.truncate(cut);
                    }
                    _ => {
                        let i = rng.below(png.len());
                        png.insert(i, rng.byte());
                    }
                }
            }
            // Half the time fix the CRCs so the mutation reaches the chunk parsers and the inflater.
            if round % 2 == 0 {
                fix_crcs(&mut png);
            }
            let strict = rng.below(2) == 0;
            if let Ok((img, _)) = decode_with_options(&png, DecodeOptions { check_ancillary_crc: strict }) {
                assert_eq!(img.pixels.len(), img.width as usize * img.height as usize * 4);
                decoded += 1;
            }
        }
        assert!(decoded > 0, "no mutation ever decoded - the fuzz is too harsh to reach the pixels");
    }

    #[test]
    fn error_messages_display() {
        assert_eq!(PngError::BadCrc(*b"IDAT").to_string(), "PNG chunk IDAT has a bad CRC");
        assert!(PngError::Corrupt("x").to_string().contains('x'));
    }
}
