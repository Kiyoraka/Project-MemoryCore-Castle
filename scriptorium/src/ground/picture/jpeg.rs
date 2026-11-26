//! JPEG, decoded and encoded by us: no crate, no `unsafe`, nothing but the standard library.
//!
//! # Decoder ([`decode`], [`decode_with_info`])
//!
//! Reads baseline (SOF0), extended sequential (SOF1, 8-bit) and progressive (SOF2) Huffman-coded files:
//! DQT with 8- and 16-bit tables, DHT, DRI restart intervals, any number of scans (interleaved or not),
//! progressive spectral selection and successive approximation (DC and AC, first and refinement scans).
//! One component is grey, three are YCbCr (or RGB when an Adobe APP14 segment says "no transform" or the
//! component ids spell `R`, `G`, `B`), four are CMYK or YCCK (Adobe APP14 transform 2). Any sampling
//! factors from 1 to 4 are upsampled to full size with centred linear interpolation (the "fancy"
//! triangle filter for the usual 2:1 cases). The IDCT is the accurate integer one (the `islow` design:
//! 13-bit constants, computed in 64-bit so corrupt coefficients cannot overflow).
//!
//! The EXIF orientation (APP1) and the Adobe transform flag (APP14) are reported by
//! [`decode_with_info`]; the decoder does not rotate the picture itself - [`Image::oriented`] does that
//! when the caller wants it.
//!
//! ## Corrupt and truncated data
//!
//! The decoder never panics on any input. A file that ends inside entropy-coded data returns
//! `Err(JpegError::Truncated)`; it does not hand back a partial picture. A file that ends cleanly at a
//! scan boundary - a progressive file cut after a whole scan, or any file that only lacks its EOI -
//! decodes to whatever those scans hold. Entropy data that runs into a marker, a missing restart marker,
//! bad Huffman codes, unknown tables and inconsistent headers return `Err(JpegError::Corrupt(..))`.
//!
//! Not read: arithmetic coding, lossless and hierarchical JPEG, 12-bit samples, two-component files,
//! and a height given later by a DNL marker - each returns `Err(JpegError::Unsupported(..))`. Pictures
//! above [`MAX_PIXELS`] return `Err(JpegError::TooLarge)`. A four-component file without an Adobe
//! segment is read as Adobe-style (inverted) CMYK, as Photoshop writes it.
//!
//! # Encoder ([`encode`])
//!
//! Writes baseline JPEG with a JFIF APP0 segment, the standard (Annex K) quantization tables scaled by
//! quality the way libjpeg scales them, and the standard Huffman tables. Colour pictures are YCbCr 4:2:0
//! below quality 90 and 4:4:4 from quality 90 up; a picture whose pixels are all grey is written as a
//! one-component greyscale file. Alpha is composited onto white. Edges are padded by repeating the last
//! row and column. A picture with a zero dimension is written as a 1x1 white pixel, and dimensions above
//! 65535 (the format's limit) are cropped.

use std::fmt;

/// The largest picture (width x height) the decoder accepts.
pub const MAX_PIXELS: u64 = 1 << 27;

/// A picture in memory: 8-bit RGBA, row-major, top row first, no padding between rows.
///
/// `pixels.len()` is `width * height * 4`, and the pixel at (x, y) starts at `(y * width + x) * 4`.
/// The JPEG decoder always writes alpha 255. Other picture formats (PNG) reuse this type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// A picture of the given size with every pixel set to `rgba`.
    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Image {
        let n = width as usize * height as usize;
        let mut pixels = Vec::with_capacity(n * 4);
        for _ in 0..n {
            pixels.extend_from_slice(&rgba);
        }
        Image { width, height, pixels }
    }

    /// The pixel at (x, y), or `None` outside the picture (or when `pixels` is too short).
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = self.pixels.get(i..i + 4)?;
        Some([p[0], p[1], p[2], p[3]])
    }

    /// The picture turned upright for an EXIF orientation (1 to 8): 2 mirrors, 3 turns 180 degrees,
    /// 4 flips, 5 transposes, 6 turns 90 degrees clockwise, 7 transverses, 8 turns 90 degrees
    /// anticlockwise. Orientation 1 and any unknown value return a copy.
    pub fn oriented(&self, orientation: u8) -> Image {
        let (w, h) = (self.width as usize, self.height as usize);
        let swap = matches!(orientation, 5..=8);
        let (ow, oh) = if swap { (h, w) } else { (w, h) };
        let mut out = vec![0u8; ow * oh * 4];
        for oy in 0..oh {
            for ox in 0..ow {
                let (sx, sy) = match orientation {
                    2 => (w - 1 - ox, oy),
                    3 => (w - 1 - ox, h - 1 - oy),
                    4 => (ox, h - 1 - oy),
                    5 => (oy, ox),
                    6 => (oy, h - 1 - ox),
                    7 => (w - 1 - oy, h - 1 - ox),
                    8 => (w - 1 - oy, ox),
                    _ => (ox, oy),
                };
                let s = (sy * w + sx) * 4;
                let d = (oy * ow + ox) * 4;
                if let Some(p) = self.pixels.get(s..s + 4) {
                    out[d..d + 4].copy_from_slice(p);
                }
            }
        }
        Image { width: ow as u32, height: oh as u32, pixels: out }
    }
}

/// What the decoder learned about a file besides its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpegInfo {
    /// EXIF orientation, 1 (upright, also the default when absent) to 8. Not applied to the pixels;
    /// see [`Image::oriented`].
    pub orientation: u8,
    /// The Adobe APP14 colour transform (0 none / CMYK, 1 YCbCr, 2 YCCK), when the segment is present.
    pub adobe_transform: Option<u8>,
    /// Whether the file is progressive (SOF2).
    pub progressive: bool,
    /// Number of colour components in the frame: 1, 3 or 4.
    pub components: u8,
}

/// Why a JPEG could not be decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JpegError {
    /// The data does not start with a JPEG SOI marker.
    NotJpeg,
    /// The data ended before the picture was complete.
    Truncated,
    /// The data is malformed; the text says where.
    Corrupt(&'static str),
    /// A valid JPEG feature this decoder does not read; the text names it.
    Unsupported(&'static str),
    /// The picture is larger than [`MAX_PIXELS`].
    TooLarge,
}

impl fmt::Display for JpegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JpegError::NotJpeg => write!(f, "not a JPEG file"),
            JpegError::Truncated => write!(f, "JPEG data ends too early"),
            JpegError::Corrupt(what) => write!(f, "corrupt JPEG: {what}"),
            JpegError::Unsupported(what) => write!(f, "unsupported JPEG: {what}"),
            JpegError::TooLarge => write!(f, "JPEG picture is too large"),
        }
    }
}

impl std::error::Error for JpegError {}

/// Zigzag position k -> natural (row-major) position in the 8x8 block.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6,
    7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31,
    39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

fn ceil_div(a: usize, b: usize) -> usize {
    a.div_ceil(b)
}

fn clamp_u8(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

fn clamp_i16(v: i64) -> i16 {
    v.clamp(i16::MIN as i64, i16::MAX as i64) as i16
}

// ---------------------------------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------------------------------

/// Decodes a JPEG file into RGBA pixels. See the module notes for what is read and how corrupt or
/// truncated data is answered.
pub fn decode(data: &[u8]) -> Result<Image, JpegError> {
    decode_with_info(data).map(|(img, _)| img)
}

/// Decodes a JPEG file and also returns what it says about itself: EXIF orientation, the Adobe
/// transform flag, whether it is progressive, and its component count.
pub fn decode_with_info(data: &[u8]) -> Result<(Image, JpegInfo), JpegError> {
    let mut d = Decoder {
        data,
        pos: 0,
        qt: [None; 4],
        dc: [None, None, None, None],
        ac: [None, None, None, None],
        frame: None,
        restart_interval: 0,
        adobe: None,
        orientation: 1,
        scans: 0,
    };
    d.run()?;
    d.finish()
}

/// A canonical Huffman table ready for decoding: a 9-bit lookup for short codes, then the classic
/// max-code walk for codes of 10 to 16 bits.
struct Huff {
    /// Indexed by the next 9 bits: `(length << 8) | symbol`, or 0 when the code is longer than 9 bits.
    lookup: [u16; 512],
    maxcode: [i32; 17],
    mincode: [i32; 17],
    valptr: [i32; 17],
    vals: Vec<u8>,
}

impl Huff {
    fn build(counts: &[u8; 16], vals: &[u8]) -> Result<Huff, JpegError> {
        let mut h = Huff {
            lookup: [0; 512],
            maxcode: [-1; 17],
            mincode: [0; 17],
            valptr: [0; 17],
            vals: vals.to_vec(),
        };
        let mut code: u32 = 0;
        let mut k: usize = 0;
        for l in 1..=16usize {
            let n = counts[l - 1] as usize;
            if n > 0 {
                h.valptr[l] = k as i32;
                h.mincode[l] = code as i32;
                if l <= 9 {
                    for i in 0..n {
                        let c = (code as usize + i) << (9 - l);
                        let entry = ((l as u16) << 8) | vals[k + i] as u16;
                        for j in 0..(1usize << (9 - l)) {
                            if let Some(slot) = h.lookup.get_mut(c | j) {
                                *slot = entry;
                            }
                        }
                    }
                }
                code += n as u32;
                k += n;
                h.maxcode[l] = code as i32 - 1;
            }
            if code > (1u32 << l) {
                return Err(JpegError::Corrupt("Huffman table has too many codes"));
            }
            code <<= 1;
        }
        Ok(h)
    }
}

/// Reads entropy-coded bits, undoing 0xFF00 byte stuffing and stopping at the first marker. Past a
/// marker (or the end of the data) it feeds zero bits for look-ahead, but consuming any of them is an
/// error: a valid stream never needs bits it does not contain.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    /// Bits, most significant first.
    acc: u64,
    /// Bits held in `acc`, real or padding.
    nbits: u32,
    /// Of those, how many came from the data.
    real: u32,
    stopped: bool,
    eof: bool,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        BitReader { data, pos, acc: 0, nbits: 0, real: 0, stopped: false, eof: false }
    }

    fn fill(&mut self) {
        while self.nbits <= 56 {
            let mut byte = 0u8;
            let mut real = false;
            if !self.stopped {
                let d = self.data;
                if self.pos >= d.len() {
                    self.stopped = true;
                    self.eof = true;
                } else if d[self.pos] != 0xFF {
                    byte = d[self.pos];
                    self.pos += 1;
                    real = true;
                } else if self.pos + 1 >= d.len() {
                    self.stopped = true;
                    self.eof = true;
                } else if d[self.pos + 1] == 0 {
                    byte = 0xFF;
                    self.pos += 2;
                    real = true;
                } else {
                    self.stopped = true;
                }
            }
            self.acc |= (byte as u64) << (56 - self.nbits);
            self.nbits += 8;
            if real {
                self.real += 8;
            }
        }
    }

    fn overrun(&self) -> JpegError {
        if self.eof {
            JpegError::Truncated
        } else {
            JpegError::Corrupt("entropy-coded data runs into a marker")
        }
    }

    fn consume(&mut self, n: u32) -> Result<(), JpegError> {
        if n > self.real {
            return Err(self.overrun());
        }
        self.acc <<= n;
        self.nbits -= n;
        self.real -= n;
        Ok(())
    }

    /// The next `n` bits (n <= 16).
    fn get_bits(&mut self, n: u32) -> Result<u32, JpegError> {
        if n == 0 {
            return Ok(0);
        }
        if self.nbits < n {
            self.fill();
        }
        let v = (self.acc >> (64 - n)) as u32;
        self.consume(n)?;
        Ok(v)
    }

    fn bit(&mut self) -> Result<bool, JpegError> {
        Ok(self.get_bits(1)? == 1)
    }

    /// Reads `s` bits (s <= 16) and sign-extends them the JPEG way.
    fn receive_extend(&mut self, s: u8) -> Result<i32, JpegError> {
        if s == 0 {
            return Ok(0);
        }
        let s = s as u32;
        let v = self.get_bits(s)? as i32;
        Ok(if v < (1 << (s - 1)) { v - (1 << s) + 1 } else { v })
    }

    fn decode(&mut self, h: &Huff) -> Result<u8, JpegError> {
        if self.nbits < 16 {
            self.fill();
        }
        let peek = (self.acc >> 48) as u32;
        let e = h.lookup[(peek >> 7) as usize];
        if e != 0 {
            self.consume((e >> 8) as u32)?;
            return Ok(e as u8);
        }
        for l in 10..=16usize {
            let code = (peek >> (16 - l)) as i32;
            if h.maxcode[l] >= 0 && code <= h.maxcode[l] && code >= h.mincode[l] {
                let idx = (h.valptr[l] + code - h.mincode[l]) as usize;
                if let Some(&s) = h.vals.get(idx) {
                    self.consume(l as u32)?;
                    return Ok(s);
                }
            }
        }
        if self.real < 16 {
            Err(self.overrun())
        } else {
            Err(JpegError::Corrupt("invalid Huffman code"))
        }
    }

    /// Drops the rest of the interval's bits and steps over the RSTn marker that must follow.
    fn restart(&mut self) -> Result<(), JpegError> {
        self.acc = 0;
        self.nbits = 0;
        self.real = 0;
        let d = self.data;
        let mut p = self.pos;
        loop {
            if p + 1 >= d.len() {
                self.pos = d.len();
                self.stopped = true;
                self.eof = true;
                return Err(JpegError::Truncated);
            }
            if d[p] != 0xFF {
                p += 1;
                continue;
            }
            match d[p + 1] {
                0x00 => p += 2,
                0xFF => p += 1,
                0xD0..=0xD7 => {
                    self.pos = p + 2;
                    self.stopped = false;
                    self.eof = false;
                    return Ok(());
                }
                _ => {
                    self.pos = p;
                    self.stopped = true;
                    return Err(JpegError::Corrupt("restart marker missing"));
                }
            }
        }
    }
}

struct Comp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    /// Blocks per line and per column in the coefficient store (padded to whole MCUs).
    bw: usize,
    bh: usize,
    /// The component's own size in samples.
    cw: usize,
    ch: usize,
    /// 64 coefficients per block, natural order, not yet dequantized.
    coefs: Vec<i16>,
    /// The quantization table in force at the component's first scan (natural order).
    quant: Option<[u16; 64]>,
    pred: i32,
}

struct Frame {
    width: usize,
    height: usize,
    progressive: bool,
    hmax: usize,
    vmax: usize,
    mcux: usize,
    mcuy: usize,
    comps: Vec<Comp>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Sequential,
    DcFirst,
    DcRefine,
    AcFirst,
    AcRefine,
}

struct ScanComp {
    ci: usize,
    td: usize,
    ta: usize,
}

struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
    qt: [Option<[u16; 64]>; 4],
    dc: [Option<Huff>; 4],
    ac: [Option<Huff>; 4],
    frame: Option<Frame>,
    restart_interval: u16,
    adobe: Option<u8>,
    orientation: u8,
    scans: u32,
}

impl<'a> Decoder<'a> {
    /// The next marker byte, skipping anything that is not one; `None` at the end of the data.
    fn next_marker(&mut self) -> Option<u8> {
        let d = self.data;
        loop {
            while self.pos < d.len() && d[self.pos] != 0xFF {
                self.pos += 1;
            }
            while self.pos < d.len() && d[self.pos] == 0xFF {
                self.pos += 1;
            }
            if self.pos >= d.len() {
                return None;
            }
            let m = d[self.pos];
            self.pos += 1;
            if m != 0x00 {
                return Some(m);
            }
        }
    }

    /// A marker segment's payload (after its length field); advances past it.
    fn segment(&mut self) -> Result<&'a [u8], JpegError> {
        let d = self.data;
        if self.pos + 2 > d.len() {
            return Err(JpegError::Truncated);
        }
        let len = u16::from_be_bytes([d[self.pos], d[self.pos + 1]]) as usize;
        if len < 2 {
            return Err(JpegError::Corrupt("marker segment length below 2"));
        }
        if self.pos + len > d.len() {
            return Err(JpegError::Truncated);
        }
        let seg = &d[self.pos + 2..self.pos + len];
        self.pos += len;
        Ok(seg)
    }

    fn run(&mut self) -> Result<(), JpegError> {
        let d = self.data;
        if d.len() < 2 || d[0] != 0xFF || d[1] != 0xD8 {
            return Err(JpegError::NotJpeg);
        }
        self.pos = 2;
        while let Some(marker) = self.next_marker() {
            match marker {
                0xD9 => return Ok(()),
                0xD8 => return Err(JpegError::Corrupt("second start-of-image marker")),
                0xD0..=0xD7 | 0x01 => continue,
                _ => {}
            }
            let seg = self.segment()?;
            match marker {
                0xC0..=0xC2 => self.read_sof(marker, seg)?,
                0xC3 | 0xC5..=0xC7 => return Err(JpegError::Unsupported("lossless or hierarchical JPEG")),
                0xC9..=0xCB | 0xCD..=0xCF | 0xCC => return Err(JpegError::Unsupported("arithmetic coding")),
                0xC4 => self.read_dht(seg)?,
                0xDB => self.read_dqt(seg)?,
                0xDD => {
                    if seg.len() < 2 {
                        return Err(JpegError::Corrupt("restart interval segment too short"));
                    }
                    self.restart_interval = u16::from_be_bytes([seg[0], seg[1]]);
                }
                0xDA => self.read_sos(seg)?,
                0xE1 => {
                    if seg.len() > 6 && &seg[..6] == b"Exif\0\0" {
                        if let Some(o) = exif_orientation(&seg[6..]) {
                            self.orientation = o;
                        }
                    }
                }
                0xEE => {
                    if seg.len() >= 12 && &seg[..5] == b"Adobe" {
                        self.adobe = Some(seg[11]);
                    }
                }
                _ => {}
            }
        }
        // The data ended without an EOI: fine when whole scans were read, `finish` decides.
        Ok(())
    }

    fn read_sof(&mut self, marker: u8, seg: &[u8]) -> Result<(), JpegError> {
        if self.frame.is_some() {
            return Err(JpegError::Corrupt("more than one frame header"));
        }
        if seg.len() < 6 {
            return Err(JpegError::Corrupt("frame header too short"));
        }
        if seg[0] != 8 {
            return Err(JpegError::Unsupported("sample precision other than 8 bits"));
        }
        let height = u16::from_be_bytes([seg[1], seg[2]]) as usize;
        let width = u16::from_be_bytes([seg[3], seg[4]]) as usize;
        if height == 0 {
            return Err(JpegError::Unsupported("height given later by a DNL marker"));
        }
        if width == 0 {
            return Err(JpegError::Corrupt("zero width"));
        }
        if width as u64 * height as u64 > MAX_PIXELS {
            return Err(JpegError::TooLarge);
        }
        let nc = seg[5] as usize;
        if !matches!(nc, 1 | 3 | 4) {
            return Err(JpegError::Unsupported("component count other than 1, 3 or 4"));
        }
        if seg.len() < 6 + nc * 3 {
            return Err(JpegError::Corrupt("frame header too short"));
        }
        let mut comps: Vec<Comp> = Vec::with_capacity(nc);
        for i in 0..nc {
            let b = &seg[6 + i * 3..9 + i * 3];
            let (id, h, v, tq) = (b[0], (b[1] >> 4) as usize, (b[1] & 15) as usize, b[2] as usize);
            if !(1..=4).contains(&h) || !(1..=4).contains(&v) {
                return Err(JpegError::Corrupt("sampling factor outside 1 to 4"));
            }
            if tq > 3 {
                return Err(JpegError::Corrupt("quantization table id above 3"));
            }
            if comps.iter().any(|c| c.id == id) {
                return Err(JpegError::Corrupt("two components with one id"));
            }
            comps.push(Comp {
                id,
                h,
                v,
                tq,
                bw: 0,
                bh: 0,
                cw: 0,
                ch: 0,
                coefs: Vec::new(),
                quant: None,
                pred: 0,
            });
        }
        let hmax = comps.iter().map(|c| c.h).max().unwrap_or(1);
        let vmax = comps.iter().map(|c| c.v).max().unwrap_or(1);
        let mcux = ceil_div(width, 8 * hmax);
        let mcuy = ceil_div(height, 8 * vmax);
        for c in &mut comps {
            c.bw = mcux * c.h;
            c.bh = mcuy * c.v;
            c.cw = ceil_div(width * c.h, hmax);
            c.ch = ceil_div(height * c.v, vmax);
            let n = c
                .bw
                .checked_mul(c.bh)
                .and_then(|n| n.checked_mul(64))
                .ok_or(JpegError::TooLarge)?;
            c.coefs = vec![0; n];
        }
        self.frame = Some(Frame {
            width,
            height,
            progressive: marker == 0xC2,
            hmax,
            vmax,
            mcux,
            mcuy,
            comps,
        });
        Ok(())
    }

    fn read_dqt(&mut self, seg: &[u8]) -> Result<(), JpegError> {
        let mut i = 0;
        while i < seg.len() {
            let pq = seg[i] >> 4;
            let tq = (seg[i] & 15) as usize;
            i += 1;
            if tq > 3 {
                return Err(JpegError::Corrupt("quantization table id above 3"));
            }
            let mut t = [0u16; 64];
            match pq {
                0 => {
                    let b = seg.get(i..i + 64).ok_or(JpegError::Corrupt("quantization table too short"))?;
                    for k in 0..64 {
                        t[ZIGZAG[k]] = b[k] as u16;
                    }
                    i += 64;
                }
                1 => {
                    let b = seg.get(i..i + 128).ok_or(JpegError::Corrupt("quantization table too short"))?;
                    for k in 0..64 {
                        t[ZIGZAG[k]] = u16::from_be_bytes([b[2 * k], b[2 * k + 1]]);
                    }
                    i += 128;
                }
                _ => return Err(JpegError::Corrupt("quantization table precision")),
            }
            self.qt[tq] = Some(t);
        }
        Ok(())
    }

    fn read_dht(&mut self, seg: &[u8]) -> Result<(), JpegError> {
        let mut i = 0;
        while i < seg.len() {
            let class = seg[i] >> 4;
            let id = (seg[i] & 15) as usize;
            i += 1;
            if class > 1 || id > 3 {
                return Err(JpegError::Corrupt("Huffman table class or id"));
            }
            let c = seg.get(i..i + 16).ok_or(JpegError::Corrupt("Huffman table too short"))?;
            let mut counts = [0u8; 16];
            counts.copy_from_slice(c);
            i += 16;
            let total: usize = counts.iter().map(|&n| n as usize).sum();
            if total > 256 {
                return Err(JpegError::Corrupt("Huffman table has too many symbols"));
            }
            let vals = seg.get(i..i + total).ok_or(JpegError::Corrupt("Huffman table too short"))?;
            i += total;
            let h = Huff::build(&counts, vals)?;
            if class == 0 {
                self.dc[id] = Some(h);
            } else {
                self.ac[id] = Some(h);
            }
        }
        Ok(())
    }

    fn read_sos(&mut self, seg: &[u8]) -> Result<(), JpegError> {
        let frame = self.frame.as_mut().ok_or(JpegError::Corrupt("scan before the frame header"))?;
        let ns = *seg.first().ok_or(JpegError::Corrupt("scan header too short"))? as usize;
        if ns == 0 || ns > 4 || seg.len() < 1 + ns * 2 + 3 {
            return Err(JpegError::Corrupt("scan header"));
        }
        let mut sc: Vec<ScanComp> = Vec::with_capacity(ns);
        for i in 0..ns {
            let id = seg[1 + 2 * i];
            let t = seg[2 + 2 * i];
            let ci = frame
                .comps
                .iter()
                .position(|c| c.id == id)
                .ok_or(JpegError::Corrupt("scan names an unknown component"))?;
            if sc.iter().any(|s| s.ci == ci) {
                return Err(JpegError::Corrupt("scan names a component twice"));
            }
            let (td, ta) = ((t >> 4) as usize, (t & 15) as usize);
            if td > 3 || ta > 3 {
                return Err(JpegError::Corrupt("Huffman table id above 3"));
            }
            sc.push(ScanComp { ci, td, ta });
        }
        let p = 1 + ns * 2;
        let (ss, se) = (seg[p] as usize, seg[p + 1] as usize);
        let (ah, al) = ((seg[p + 2] >> 4) as u32, (seg[p + 2] & 15) as u32);
        let kind = if !frame.progressive {
            Kind::Sequential
        } else {
            if al > 13 || ah > 13 {
                return Err(JpegError::Corrupt("successive approximation bit position"));
            }
            if ss == 0 {
                if se != 0 {
                    return Err(JpegError::Corrupt("progressive DC scan carries AC coefficients"));
                }
                if ah == 0 {
                    Kind::DcFirst
                } else {
                    Kind::DcRefine
                }
            } else {
                if ns != 1 {
                    return Err(JpegError::Corrupt("progressive AC scan with more than one component"));
                }
                if se < ss || se > 63 {
                    return Err(JpegError::Corrupt("spectral selection band"));
                }
                if ah == 0 {
                    Kind::AcFirst
                } else {
                    Kind::AcRefine
                }
            }
        };
        if ns > 1 {
            let blocks: usize = sc.iter().map(|s| frame.comps[s.ci].h * frame.comps[s.ci].v).sum();
            if blocks > 10 {
                return Err(JpegError::Corrupt("more than 10 blocks in an MCU"));
            }
        }
        for s in &sc {
            if matches!(kind, Kind::Sequential | Kind::DcFirst) && self.dc[s.td].is_none() {
                return Err(JpegError::Corrupt("scan uses an undefined Huffman table"));
            }
            if matches!(kind, Kind::Sequential | Kind::AcFirst | Kind::AcRefine) && self.ac[s.ta].is_none() {
                return Err(JpegError::Corrupt("scan uses an undefined Huffman table"));
            }
            let c = &mut frame.comps[s.ci];
            if c.quant.is_none() {
                c.quant =
                    Some(self.qt[c.tq].ok_or(JpegError::Corrupt("scan uses an undefined quantization table"))?);
            }
        }
        let scan = Scan { comps: &sc, kind, ss, se, al, restart: self.restart_interval as usize };
        self.pos = decode_scan(frame, &self.dc, &self.ac, self.data, self.pos, &scan)?;
        self.scans += 1;
        Ok(())
    }

    fn finish(self) -> Result<(Image, JpegInfo), JpegError> {
        let frame = match self.frame {
            Some(f) => f,
            None => return Err(JpegError::Truncated),
        };
        if self.scans == 0 {
            return Err(JpegError::Truncated);
        }
        let (w, h) = (frame.width, frame.height);
        let mut planes: Vec<Vec<u8>> = Vec::with_capacity(frame.comps.len());
        for c in &frame.comps {
            let q = c.quant.or(self.qt[c.tq]).unwrap_or([1; 64]);
            let nbx = ceil_div(c.cw, 8);
            let nby = ceil_div(c.ch, 8);
            let stride = nbx * 8;
            let mut plane = vec![0u8; stride * nby * 8];
            for by in 0..nby {
                for bx in 0..nbx {
                    let off = (by * c.bw + bx) * 64;
                    idct_block(&c.coefs[off..off + 64], &q, &mut plane, by * 8 * stride + bx * 8, stride);
                }
            }
            planes.push(upsample(&plane, stride, c.cw, c.ch, c.h, c.v, frame.hmax, frame.vmax, w, h));
        }
        let n = w * h;
        let mut pixels = vec![255u8; n * 4];
        match planes.len() {
            1 => {
                for (i, &y) in planes[0].iter().enumerate() {
                    pixels[i * 4..i * 4 + 3].copy_from_slice(&[y, y, y]);
                }
            }
            3 => {
                let ids: Vec<u8> = frame.comps.iter().map(|c| c.id).collect();
                let direct = self.adobe == Some(0) || ids == [b'R', b'G', b'B'];
                for i in 0..n {
                    let (a, b, c) = (planes[0][i], planes[1][i], planes[2][i]);
                    let rgb = if direct { [a, b, c] } else { ycc_to_rgb(a, b, c) };
                    pixels[i * 4..i * 4 + 3].copy_from_slice(&rgb);
                }
            }
            _ => {
                let ycck = self.adobe == Some(2);
                for i in 0..n {
                    let (a, b, c, k) = (planes[0][i], planes[1][i], planes[2][i], planes[3][i] as u32);
                    // Adobe stores CMYK inverted: 255 is no ink. YCCK holds 255 - (inverted C, M, Y).
                    let cmy = if ycck {
                        let [r, g, bl] = ycc_to_rgb(a, b, c);
                        [255 - r, 255 - g, 255 - bl]
                    } else {
                        [a, b, c]
                    };
                    for ch in 0..3 {
                        pixels[i * 4 + ch] = ((cmy[ch] as u32 * k + 127) / 255) as u8;
                    }
                }
            }
        }
        let info = JpegInfo {
            orientation: self.orientation,
            adobe_transform: self.adobe,
            progressive: frame.progressive,
            components: frame.comps.len() as u8,
        };
        Ok((Image { width: w as u32, height: h as u32, pixels }, info))
    }
}

struct Scan<'s> {
    comps: &'s [ScanComp],
    kind: Kind,
    ss: usize,
    se: usize,
    al: u32,
    restart: usize,
}

/// Decodes one scan's entropy-coded data starting at `start`; returns where the data stopped.
fn decode_scan(
    frame: &mut Frame,
    dc: &[Option<Huff>; 4],
    ac: &[Option<Huff>; 4],
    data: &[u8],
    start: usize,
    scan: &Scan<'_>,
) -> Result<usize, JpegError> {
    let mut br = BitReader::new(data, start);
    let mut eobrun: u32 = 0;
    for c in &mut frame.comps {
        c.pred = 0;
    }
    let ri = scan.restart;
    if scan.comps.len() == 1 {
        let s = &scan.comps[0];
        let (dct, act) = (dc[s.td].as_ref(), ac[s.ta].as_ref());
        let comp = &mut frame.comps[s.ci];
        let nbx = ceil_div(comp.cw, 8);
        let nby = ceil_div(comp.ch, 8);
        for n in 0..nbx * nby {
            if ri > 0 && n > 0 && n % ri == 0 {
                br.restart()?;
                comp.pred = 0;
                eobrun = 0;
            }
            let (bx, by) = (n % nbx, n / nbx);
            let off = (by * comp.bw + bx) * 64;
            let blk = &mut comp.coefs[off..off + 64];
            decode_block(&mut br, scan, blk, dct, act, &mut comp.pred, &mut eobrun)?;
        }
    } else {
        let mcux = frame.mcux;
        for n in 0..frame.mcux * frame.mcuy {
            if ri > 0 && n > 0 && n % ri == 0 {
                br.restart()?;
                for c in &mut frame.comps {
                    c.pred = 0;
                }
                eobrun = 0;
            }
            let (mx, my) = (n % mcux, n / mcux);
            for s in scan.comps {
                let (dct, act) = (dc[s.td].as_ref(), ac[s.ta].as_ref());
                let comp = &mut frame.comps[s.ci];
                for v in 0..comp.v {
                    for h in 0..comp.h {
                        let (bx, by) = (mx * comp.h + h, my * comp.v + v);
                        let off = (by * comp.bw + bx) * 64;
                        let blk = &mut comp.coefs[off..off + 64];
                        decode_block(&mut br, scan, blk, dct, act, &mut comp.pred, &mut eobrun)?;
                    }
                }
            }
        }
    }
    Ok(br.pos)
}

const NO_TABLE: JpegError = JpegError::Corrupt("scan uses an undefined Huffman table");

fn decode_block(
    br: &mut BitReader<'_>,
    scan: &Scan<'_>,
    blk: &mut [i16],
    dct: Option<&Huff>,
    act: Option<&Huff>,
    pred: &mut i32,
    eobrun: &mut u32,
) -> Result<(), JpegError> {
    let (ss, se, al) = (scan.ss, scan.se, scan.al);
    match scan.kind {
        Kind::Sequential | Kind::DcFirst => {
            let t = br.decode(dct.ok_or(NO_TABLE)?)?;
            if t > 15 {
                return Err(JpegError::Corrupt("DC difference category above 15"));
            }
            *pred = pred.wrapping_add(br.receive_extend(t)?);
            blk[0] = clamp_i16((*pred as i64) << al);
            if scan.kind == Kind::DcFirst {
                return Ok(());
            }
            let act = act.ok_or(NO_TABLE)?;
            let mut k = 1;
            while k < 64 {
                let rs = br.decode(act)?;
                let (r, s) = ((rs >> 4) as usize, rs & 15);
                if s == 0 {
                    if r == 15 {
                        k += 16;
                        continue;
                    }
                    break;
                }
                k += r;
                if k > 63 {
                    return Err(JpegError::Corrupt("AC coefficient past the end of the block"));
                }
                blk[ZIGZAG[k]] = clamp_i16(br.receive_extend(s)? as i64);
                k += 1;
            }
        }
        Kind::DcRefine => {
            if br.bit()? {
                blk[0] |= 1 << al;
            }
        }
        Kind::AcFirst => {
            if *eobrun > 0 {
                *eobrun -= 1;
                return Ok(());
            }
            let act = act.ok_or(NO_TABLE)?;
            let mut k = ss;
            while k <= se {
                let rs = br.decode(act)?;
                let (r, s) = ((rs >> 4) as u32, rs & 15);
                if s == 0 {
                    if r < 15 {
                        *eobrun = (1 << r) - 1 + br.get_bits(r)?;
                        break;
                    }
                    k += 16;
                    continue;
                }
                k += r as usize;
                if k > 63 {
                    return Err(JpegError::Corrupt("AC coefficient past the end of the block"));
                }
                blk[ZIGZAG[k]] = clamp_i16((br.receive_extend(s)? as i64) << al);
                k += 1;
            }
        }
        Kind::AcRefine => {
            let p1: i16 = 1 << al;
            let m1: i16 = -1 << al;
            let mut k = ss;
            if *eobrun == 0 {
                let act = act.ok_or(NO_TABLE)?;
                while k <= se {
                    let rs = br.decode(act)?;
                    let (mut r, s) = ((rs >> 4) as i32, rs & 15);
                    let mut val: i16 = 0;
                    if s != 0 {
                        if s != 1 {
                            return Err(JpegError::Corrupt("refinement coefficient larger than one bit"));
                        }
                        val = if br.bit()? { p1 } else { m1 };
                    } else if r != 15 {
                        *eobrun = (1 << r) + br.get_bits(r as u32)?;
                        break;
                    }
                    loop {
                        let z = ZIGZAG[k];
                        if blk[z] != 0 {
                            if br.bit()? && (blk[z] & p1) == 0 {
                                blk[z] = if blk[z] >= 0 { blk[z].saturating_add(p1) } else { blk[z].saturating_add(m1) };
                            }
                        } else {
                            r -= 1;
                            if r < 0 {
                                break;
                            }
                        }
                        k += 1;
                        if k > se {
                            break;
                        }
                    }
                    if val != 0 && k <= se {
                        blk[ZIGZAG[k]] = val;
                    }
                    k += 1;
                }
            }
            if *eobrun > 0 {
                while k <= se {
                    let z = ZIGZAG[k];
                    if blk[z] != 0 && br.bit()? && (blk[z] & p1) == 0 {
                        blk[z] = if blk[z] >= 0 { blk[z].saturating_add(p1) } else { blk[z].saturating_add(m1) };
                    }
                    k += 1;
                }
                *eobrun -= 1;
            }
        }
    }
    Ok(())
}

/// The EXIF orientation tag (0x0112) from IFD0 of a TIFF structure, when it holds 1 to 8.
fn exif_orientation(t: &[u8]) -> Option<u8> {
    let le = match t.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let rd16 = |o: usize| -> Option<u16> {
        let b = t.get(o..o.checked_add(2)?)?;
        Some(if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
    };
    let rd32 = |o: usize| -> Option<u32> {
        let b = t.get(o..o.checked_add(4)?)?;
        let a = [b[0], b[1], b[2], b[3]];
        Some(if le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    };
    if rd16(2)? != 42 {
        return None;
    }
    let ifd = rd32(4)? as usize;
    let count = rd16(ifd)? as usize;
    for i in 0..count {
        let e = ifd.checked_add(2 + i * 12)?;
        if rd16(e)? == 0x0112 {
            let v = match rd16(e + 2)? {
                3 => rd16(e + 8)? as u32,
                4 => rd32(e + 8)?,
                _ => return None,
            };
            return if (1..=8).contains(&v) { Some(v as u8) } else { None };
        }
    }
    None
}

/// JFIF YCbCr -> RGB in 16-bit fixed point.
fn ycc_to_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let y = y as i64;
    let cb = cb as i64 - 128;
    let cr = cr as i64 - 128;
    let r = y + ((91_881 * cr + 32_768) >> 16);
    let g = y - ((22_554 * cb + 46_802 * cr + 32_768) >> 16);
    let b = y + ((116_130 * cb + 32_768) >> 16);
    [clamp_u8(r), clamp_u8(g), clamp_u8(b)]
}

/// One step of the accurate integer 1-D IDCT (scaled by sqrt(8) and 2^13).
fn idct_1d(i: [i64; 8]) -> [i64; 8] {
    // Even part.
    let z1 = (i[2] + i[6]) * 4433;
    let tmp2 = z1 - i[6] * 15137;
    let tmp3 = z1 + i[2] * 6270;
    let tmp0 = (i[0] + i[4]) << 13;
    let tmp1 = (i[0] - i[4]) << 13;
    let (tmp10, tmp13) = (tmp0 + tmp3, tmp0 - tmp3);
    let (tmp11, tmp12) = (tmp1 + tmp2, tmp1 - tmp2);
    // Odd part.
    let (t0, t1, t2, t3) = (i[7], i[5], i[3], i[1]);
    let z5 = (t0 + t2 + t1 + t3) * 9633;
    let z1 = (t0 + t3) * -7373;
    let z2 = (t1 + t2) * -20995;
    let z3 = (t0 + t2) * -16069 + z5;
    let z4 = (t1 + t3) * -3196 + z5;
    let t0 = t0 * 2446 + z1 + z3;
    let t1 = t1 * 16819 + z2 + z4;
    let t2 = t2 * 25172 + z2 + z3;
    let t3 = t3 * 12299 + z1 + z4;
    [tmp10 + t3, tmp11 + t2, tmp12 + t1, tmp13 + t0, tmp13 - t0, tmp12 - t1, tmp11 - t2, tmp10 - t3]
}

fn descale(x: i64, n: u32) -> i64 {
    (x + (1 << (n - 1))) >> n
}

/// Dequantizes and inverse-transforms one block into `out` at `off`, rows `stride` apart.
fn idct_block(coef: &[i16], q: &[u16; 64], out: &mut [u8], off: usize, stride: usize) {
    let mut ws = [0i64; 64];
    for col in 0..8 {
        let v = |r: usize| coef[r * 8 + col] as i64 * q[r * 8 + col] as i64;
        if (1..8).all(|r| coef[r * 8 + col] == 0) {
            let dc = v(0) << 2;
            for r in 0..8 {
                ws[r * 8 + col] = dc;
            }
            continue;
        }
        let o = idct_1d([v(0), v(1), v(2), v(3), v(4), v(5), v(6), v(7)]);
        for r in 0..8 {
            ws[r * 8 + col] = descale(o[r], 11);
        }
    }
    for row in 0..8 {
        let w = &ws[row * 8..row * 8 + 8];
        let o = idct_1d([w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7]]);
        let base = off + row * stride;
        for x in 0..8 {
            out[base + x] = clamp_u8(descale(o[x], 18) + 128);
        }
    }
}

/// Scales one component plane (`cw` x `ch` samples, rows `stride` apart) to the full picture size
/// with centred linear interpolation; a component at full resolution is just cropped.
#[allow(clippy::too_many_arguments)]
fn upsample(
    plane: &[u8],
    stride: usize,
    cw: usize,
    ch: usize,
    h: usize,
    v: usize,
    hmax: usize,
    vmax: usize,
    w: usize,
    hgt: usize,
) -> Vec<u8> {
    let mut out = vec![0u8; w * hgt];
    if h == hmax && v == vmax {
        for y in 0..hgt {
            let sy = y.min(ch - 1);
            out[y * w..y * w + w].copy_from_slice(&plane[sy * stride..sy * stride + w]);
        }
        return out;
    }
    let taps = |n_out: usize, f: usize, fmax: usize, n_src: usize| -> Vec<(usize, usize, u32)> {
        (0..n_out)
            .map(|x| {
                let p = ((2 * x + 1) * f * 256 / (2 * fmax)) as i64 - 128;
                if p <= 0 {
                    (0, 0, 0)
                } else {
                    let i0 = (p >> 8) as usize;
                    ((i0).min(n_src - 1), (i0 + 1).min(n_src - 1), (p & 255) as u32)
                }
            })
            .collect()
    };
    let xt = taps(w, h, hmax, cw);
    let yt = taps(hgt, v, vmax, ch);
    for (y, &(y0, y1, wy)) in yt.iter().enumerate() {
        let r0 = &plane[y0 * stride..];
        let r1 = &plane[y1 * stride..];
        let row = &mut out[y * w..y * w + w];
        for (x, &(x0, x1, wx)) in xt.iter().enumerate() {
            let top = r0[x0] as u32 * (256 - wx) + r0[x1] as u32 * wx;
            let bot = r1[x0] as u32 * (256 - wx) + r1[x1] as u32 * wx;
            row[x] = ((top * (256 - wy) + bot * wy + 32_768) >> 16) as u8;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------------------------------

/// Annex K luminance quantization table, natural order.
const STD_LUMA_QT: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17,
    22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78,
    87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

/// Annex K chrominance quantization table, natural order.
const STD_CHROMA_QT: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUMA_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22,
    0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33,
    0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34,
    0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55,
    0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76,
    0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96,
    0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5,
    0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4,
    0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1,
    0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];

const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHROMA_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13,
    0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15, 0x62,
    0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26, 0x27, 0x28, 0x29,
    0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54,
    0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75,
    0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94,
    0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3,
    0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2,
    0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea,
    0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];

/// Encodes a picture as a baseline JPEG at `quality` (1 to 100; 0 counts as 1). See the module notes
/// for the layout chosen.
pub fn encode(img: &Image, quality: u8) -> Vec<u8> {
    let q = quality.clamp(1, 100);
    let empty = img.width == 0 || img.height == 0;
    let w = if empty { 1 } else { (img.width as usize).min(65535) };
    let h = if empty { 1 } else { (img.height as usize).min(65535) };
    let src_w = img.width as usize;
    let mut rgb = vec![255u8; w * h * 3];
    let mut grey = true;
    for y in 0..h {
        for x in 0..w {
            let d = (y * w + x) * 3;
            if x < src_w && y < img.height as usize {
                let s = (y * src_w + x) * 4;
                if let Some(p) = img.pixels.get(s..s + 4) {
                    let a = p[3] as u32;
                    for c in 0..3 {
                        rgb[d + c] = ((p[c] as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
                    }
                }
            }
            if rgb[d] != rgb[d + 1] || rgb[d] != rgb[d + 2] {
                grey = false;
            }
        }
    }
    let qt = vec![scaled_table(&STD_LUMA_QT, q), scaled_table(&STD_CHROMA_QT, q)];
    let frame = if grey {
        let ys: Vec<u8> = rgb.chunks_exact(3).map(|p| p[0]).collect();
        build_frame(w, h, &[PlaneIn { samples: &ys, id: 1, h: 1, v: 1, tq: 0, table: 0 }], qt)
    } else {
        let (ys, cbs, crs) = rgb_planes(&rgb);
        let f = if q >= 90 { 1 } else { 2 };
        build_frame(
            w,
            h,
            &[
                PlaneIn { samples: &ys, id: 1, h: f, v: f, tq: 0, table: 0 },
                PlaneIn { samples: &cbs, id: 2, h: 1, v: 1, tq: 1, table: 1 },
                PlaneIn { samples: &crs, id: 3, h: 1, v: 1, tq: 1, table: 1 },
            ],
            qt,
        )
    };
    write_baseline(&frame, 0, &[])
}

/// A quantization table scaled for `quality` the way libjpeg does it, limited to 1..255.
fn scaled_table(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let q = quality.clamp(1, 100) as u32;
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    base.map(|b| ((b as u32 * scale + 50) / 100).clamp(1, 255) as u16)
}

fn rgb_to_ycc(r: u8, g: u8, b: u8) -> [u8; 3] {
    let (r, g, b) = (r as f32, g as f32, b as f32);
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cb = -0.168_736 * r - 0.331_264 * g + 0.5 * b + 128.0;
    let cr = 0.5 * r - 0.418_688 * g - 0.081_312 * b + 128.0;
    [clamp_u8(y.round() as i64), clamp_u8(cb.round() as i64), clamp_u8(cr.round() as i64)]
}

fn rgb_planes(rgb: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let n = rgb.len() / 3;
    let (mut ys, mut cbs, mut crs) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for p in rgb.chunks_exact(3) {
        let [y, cb, cr] = rgb_to_ycc(p[0], p[1], p[2]);
        ys.push(y);
        cbs.push(cb);
        crs.push(cr);
    }
    (ys, cbs, crs)
}

/// One full-resolution sample plane handed to the encoder, with its frame parameters.
struct PlaneIn<'a> {
    samples: &'a [u8],
    id: u8,
    h: usize,
    v: usize,
    tq: u8,
    /// Huffman tables: 0 luminance, 1 chrominance.
    table: u8,
}

struct EncComp {
    id: u8,
    h: usize,
    v: usize,
    tq: u8,
    table: u8,
    cw: usize,
    bw: usize,
    /// Quantized coefficients, natural order, `bw` blocks per line.
    blocks: Vec<[i16; 64]>,
    ch: usize,
}

struct EncFrame {
    width: usize,
    height: usize,
    mcux: usize,
    mcuy: usize,
    comps: Vec<EncComp>,
    qt: Vec<[u16; 64]>,
}

/// Downsamples each plane to its sampling factors, transforms and quantizes every block.
fn build_frame(width: usize, height: usize, planes: &[PlaneIn<'_>], qt: Vec<[u16; 64]>) -> EncFrame {
    let hmax = planes.iter().map(|p| p.h).max().unwrap_or(1);
    let vmax = planes.iter().map(|p| p.v).max().unwrap_or(1);
    let mcux = ceil_div(width, 8 * hmax);
    let mcuy = ceil_div(height, 8 * vmax);
    let cos = cos_table();
    let mut comps = Vec::with_capacity(planes.len());
    for p in planes {
        let cw = ceil_div(width * p.h, hmax);
        let ch = ceil_div(height * p.v, vmax);
        let (fx, fy) = ((hmax / p.h).max(1), (vmax / p.v).max(1));
        let sample = |x: usize, y: usize| -> u32 {
            *p.samples.get(y.min(height - 1) * width + x.min(width - 1)).unwrap_or(&128) as u32
        };
        let mut plane = vec![0u8; cw * ch];
        for j in 0..ch {
            for i in 0..cw {
                let mut sum = 0u32;
                for dy in 0..fy {
                    for dx in 0..fx {
                        sum += sample(i * fx + dx, j * fy + dy);
                    }
                }
                let n = (fx * fy) as u32;
                plane[j * cw + i] = ((sum + n / 2) / n) as u8;
            }
        }
        let (bw, bh) = (mcux * p.h, mcuy * p.v);
        let q = &qt[p.tq as usize];
        let mut blocks = Vec::with_capacity(bw * bh);
        for by in 0..bh {
            for bx in 0..bw {
                let mut f = [0f32; 64];
                for y in 0..8 {
                    let sy = (by * 8 + y).min(ch - 1);
                    for x in 0..8 {
                        let sx = (bx * 8 + x).min(cw - 1);
                        f[y * 8 + x] = plane[sy * cw + sx] as f32 - 128.0;
                    }
                }
                let d = fdct(&f, &cos);
                let mut b = [0i16; 64];
                for i in 0..64 {
                    let v = (d[i] / q[i] as f32).round() as i32;
                    b[i] = if i == 0 { v.clamp(-1024, 1023) } else { v.clamp(-1023, 1023) } as i16;
                }
                blocks.push(b);
            }
        }
        comps.push(EncComp { id: p.id, h: p.h, v: p.v, tq: p.tq, table: p.table, cw, ch, bw, blocks });
    }
    EncFrame { width, height, mcux, mcuy, comps, qt }
}

/// `cos[u][x] = C(u)/2 * cos((2x+1) u pi / 16)`, with C(0) = 1/sqrt(2).
fn cos_table() -> [[f32; 8]; 8] {
    let mut t = [[0f32; 8]; 8];
    for (u, row) in t.iter_mut().enumerate() {
        let c = if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 };
        for (x, v) in row.iter_mut().enumerate() {
            *v = c / 2.0 * (((2 * x + 1) * u) as f32 * std::f32::consts::PI / 16.0).cos();
        }
    }
    t
}

/// Separable floating-point forward DCT of a level-shifted block (natural order in and out).
fn fdct(f: &[f32; 64], cos: &[[f32; 8]; 8]) -> [f32; 64] {
    let mut rows = [0f32; 64];
    for y in 0..8 {
        for u in 0..8 {
            rows[y * 8 + u] = (0..8).map(|x| cos[u][x] * f[y * 8 + x]).sum();
        }
    }
    let mut out = [0f32; 64];
    for v in 0..8 {
        for u in 0..8 {
            out[v * 8 + u] = (0..8).map(|y| cos[v][y] * rows[y * 8 + u]).sum();
        }
    }
    out
}

/// The blocks of a scan over `comps`, grouped by MCU in coding order: `(component index, block index)`.
/// One component is coded block by block over its own size; several are coded MCU by MCU.
fn scan_order(frame: &EncFrame, comps: &[usize]) -> Vec<Vec<(usize, usize)>> {
    let mut mcus = Vec::new();
    if comps.len() == 1 {
        let ci = comps[0];
        let c = &frame.comps[ci];
        for by in 0..ceil_div(c.ch, 8) {
            for bx in 0..ceil_div(c.cw, 8) {
                mcus.push(vec![(ci, by * c.bw + bx)]);
            }
        }
    } else {
        for my in 0..frame.mcuy {
            for mx in 0..frame.mcux {
                let mut m = Vec::new();
                for &ci in comps {
                    let c = &frame.comps[ci];
                    for v in 0..c.v {
                        for h in 0..c.h {
                            m.push((ci, (my * c.v + v) * c.bw + mx * c.h + h));
                        }
                    }
                }
                mcus.push(m);
            }
        }
    }
    mcus
}

struct HuffEnc {
    code: [u16; 256],
    len: [u8; 256],
}

fn huff_enc(bits: &[u8; 16], vals: &[u8]) -> HuffEnc {
    let mut e = HuffEnc { code: [0; 256], len: [0; 256] };
    let mut code: u32 = 0;
    let mut k = 0;
    for l in 1..=16u8 {
        for _ in 0..bits[l as usize - 1] {
            if let Some(&s) = vals.get(k) {
                e.code[s as usize] = code as u16;
                e.len[s as usize] = l;
            }
            k += 1;
            code += 1;
        }
        code <<= 1;
    }
    e
}

/// Writes bits most significant first, stuffing a zero after every 0xFF byte.
struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter { out: Vec::new(), acc: 0, n: 0 }
    }

    fn put(&mut self, bits: u32, len: u32) {
        if len == 0 {
            return;
        }
        self.acc = (self.acc << len) | (bits as u64 & ((1u64 << len) - 1));
        self.n += len;
        while self.n >= 8 {
            let b = (self.acc >> (self.n - 8)) as u8;
            self.out.push(b);
            if b == 0xFF {
                self.out.push(0);
            }
            self.n -= 8;
        }
    }

    fn sym(&mut self, t: &HuffEnc, s: u8) {
        self.put(t.code[s as usize] as u32, t.len[s as usize] as u32);
    }

    /// Pads the last byte with one-bits.
    fn flush(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1 << pad) - 1, pad);
        }
    }

    fn marker(&mut self, m: u8) {
        self.flush();
        self.out.extend_from_slice(&[0xFF, m]);
    }
}

fn bit_len(v: u32) -> u32 {
    32 - v.leading_zeros()
}

/// The magnitude category of `v` and its value bits as JPEG codes them.
fn category(v: i32) -> (u32, u32) {
    let cat = bit_len(v.unsigned_abs());
    let bits = if v < 0 { (v - 1) as u32 } else { v as u32 };
    (cat, bits & ((1u32 << cat) - 1))
}

fn write_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

fn dht_payload(class: u8, id: u8, bits: &[u8; 16], vals: &[u8]) -> Vec<u8> {
    let mut p = vec![(class << 4) | id];
    p.extend_from_slice(bits);
    p.extend_from_slice(vals);
    p
}

/// SOI, JFIF APP0 (for one or three components), any extra segments, DQT and the frame header.
fn write_headers(out: &mut Vec<u8>, frame: &EncFrame, sof: u8, extra: &[(u8, Vec<u8>)]) {
    out.extend_from_slice(&[0xFF, 0xD8]);
    if frame.comps.len() != 4 {
        write_segment(out, 0xE0, &[b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0]);
    }
    for (m, payload) in extra {
        write_segment(out, *m, payload);
    }
    let mut dqt = Vec::new();
    for (i, t) in frame.qt.iter().enumerate() {
        dqt.push(i as u8);
        dqt.extend(ZIGZAG.iter().map(|&z| t[z].min(255) as u8));
    }
    write_segment(out, 0xDB, &dqt);
    let mut sofp = vec![8];
    sofp.extend_from_slice(&(frame.height as u16).to_be_bytes());
    sofp.extend_from_slice(&(frame.width as u16).to_be_bytes());
    sofp.push(frame.comps.len() as u8);
    for c in &frame.comps {
        sofp.extend_from_slice(&[c.id, ((c.h as u8) << 4) | c.v as u8, c.tq]);
    }
    write_segment(out, sof, &sofp);
}

/// A complete baseline file: headers, the standard Huffman tables, one interleaved scan (restart
/// markers every `restart` MCUs when non-zero) and EOI.
fn write_baseline(frame: &EncFrame, restart: u16, extra: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    write_headers(&mut out, frame, 0xC0, extra);
    let mut dht = dht_payload(0, 0, &DC_LUMA_BITS, &DC_VALS);
    dht.extend(dht_payload(1, 0, &AC_LUMA_BITS, &AC_LUMA_VALS));
    if frame.comps.iter().any(|c| c.table == 1) {
        dht.extend(dht_payload(0, 1, &DC_CHROMA_BITS, &DC_VALS));
        dht.extend(dht_payload(1, 1, &AC_CHROMA_BITS, &AC_CHROMA_VALS));
    }
    write_segment(&mut out, 0xC4, &dht);
    if restart > 0 {
        write_segment(&mut out, 0xDD, &restart.to_be_bytes());
    }
    let mut sos = vec![frame.comps.len() as u8];
    for c in &frame.comps {
        sos.extend_from_slice(&[c.id, (c.table << 4) | c.table]);
    }
    sos.extend_from_slice(&[0, 63, 0]);
    write_segment(&mut out, 0xDA, &sos);

    let dc = [huff_enc(&DC_LUMA_BITS, &DC_VALS), huff_enc(&DC_CHROMA_BITS, &DC_VALS)];
    let ac = [huff_enc(&AC_LUMA_BITS, &AC_LUMA_VALS), huff_enc(&AC_CHROMA_BITS, &AC_CHROMA_VALS)];
    let all: Vec<usize> = (0..frame.comps.len()).collect();
    let mut bw = BitWriter::new();
    let mut pred = vec![0i32; frame.comps.len()];
    let mut rst = 0u8;
    for (n, mcu) in scan_order(frame, &all).iter().enumerate() {
        if restart > 0 && n > 0 && n % restart as usize == 0 {
            bw.marker(0xD0 + rst);
            rst = (rst + 1) & 7;
            pred.iter_mut().for_each(|p| *p = 0);
        }
        for &(ci, bi) in mcu {
            let c = &frame.comps[ci];
            let t = c.table as usize;
            encode_block(&mut bw, &c.blocks[bi], &mut pred[ci], &dc[t], &ac[t]);
        }
    }
    bw.flush();
    out.extend_from_slice(&bw.out);
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

fn encode_block(bw: &mut BitWriter, blk: &[i16; 64], pred: &mut i32, dc: &HuffEnc, ac: &HuffEnc) {
    let diff = blk[0] as i32 - *pred;
    *pred = blk[0] as i32;
    let (cat, bits) = category(diff);
    bw.sym(dc, cat as u8);
    bw.put(bits, cat);
    let mut run = 0u32;
    for &z in &ZIGZAG[1..] {
        let v = blk[z] as i32;
        if v == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            bw.sym(ac, 0xF0);
            run -= 16;
        }
        let (cat, bits) = category(v);
        bw.sym(ac, ((run << 4) | cat) as u8);
        bw.put(bits, cat);
        run = 0;
    }
    if run > 0 {
        bw.sym(ac, 0x00);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    fn make(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Image {
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let [r, g, b] = f(x, y);
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        Image { width: w, height: h, pixels }
    }

    fn gradient(w: u32, h: u32) -> Image {
        make(w, h, |x, y| {
            let fx = x * 255 / (w - 1).max(1);
            let fy = y * 255 / (h - 1).max(1);
            [fx as u8, fy as u8, ((x + y) * 255 / (w + h - 2).max(1)) as u8]
        })
    }

    fn checker(w: u32, h: u32, cell: u32) -> Image {
        make(w, h, |x, y| if (x / cell + y / cell) % 2 == 0 { [230, 40, 40] } else { [30, 60, 200] })
    }

    fn smooth(w: u32, h: u32) -> Image {
        make(w, h, |x, y| {
            let (fx, fy) = (x as f32 / 9.0, y as f32 / 7.0);
            [
                (128.0 + 100.0 * fx.sin()) as u8,
                (128.0 + 100.0 * fy.cos()) as u8,
                (128.0 + 90.0 * (fx + fy).sin()) as u8,
            ]
        })
    }

    fn grey_of(img: &Image) -> Image {
        let mut g = img.clone();
        for p in g.pixels.chunks_exact_mut(4) {
            let v = ((p[0] as u32 * 3 + p[1] as u32 * 5 + p[2] as u32 * 2) / 10) as u8;
            p[0] = v;
            p[1] = v;
            p[2] = v;
        }
        g
    }

    fn mae(a: &Image, b: &Image) -> f64 {
        assert_eq!((a.width, a.height), (b.width, b.height));
        let mut sum = 0u64;
        for (p, q) in a.pixels.chunks_exact(4).zip(b.pixels.chunks_exact(4)) {
            for c in 0..3 {
                sum += (p[c] as i32 - q[c] as i32).unsigned_abs() as u64;
            }
        }
        sum as f64 / (a.width as f64 * a.height as f64 * 3.0)
    }

    fn rgb_of(img: &Image) -> Vec<u8> {
        img.pixels.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
    }

    /// A YCbCr frame with luma sampling (hy, vy) and 1x1 chroma.
    fn ycc_frame(img: &Image, quality: u8, hy: usize, vy: usize) -> EncFrame {
        let (ys, cbs, crs) = rgb_planes(&rgb_of(img));
        let qt = vec![scaled_table(&STD_LUMA_QT, quality), scaled_table(&STD_CHROMA_QT, quality)];
        build_frame(
            img.width as usize,
            img.height as usize,
            &[
                PlaneIn { samples: &ys, id: 1, h: hy, v: vy, tq: 0, table: 0 },
                PlaneIn { samples: &cbs, id: 2, h: 1, v: 1, tq: 1, table: 1 },
                PlaneIn { samples: &crs, id: 3, h: 1, v: 1, tq: 1, table: 1 },
            ],
            qt,
        )
    }

    fn grey_frame(img: &Image, quality: u8) -> EncFrame {
        let ys: Vec<u8> = img.pixels.chunks_exact(4).map(|p| p[0]).collect();
        let qt = vec![scaled_table(&STD_LUMA_QT, quality)];
        build_frame(
            img.width as usize,
            img.height as usize,
            &[PlaneIn { samples: &ys, id: 1, h: 1, v: 1, tq: 0, table: 0 }],
            qt,
        )
    }

    /// One-sample-per-component frame from raw planes (all on table 0).
    fn raw_frame(w: usize, h: usize, planes: &[&[u8]], ids: &[u8]) -> EncFrame {
        let ins: Vec<PlaneIn> = planes
            .iter()
            .zip(ids)
            .map(|(s, &id)| PlaneIn { samples: s, id, h: 1, v: 1, tq: 0, table: 0 })
            .collect();
        build_frame(w, h, &ins, vec![scaled_table(&STD_LUMA_QT, 95)])
    }

    // ---- A small progressive encoder, fed by the baseline encoder's quantized coefficients ----

    enum Ev {
        Sym(bool, u8),
        Bits(u32, u32),
        Rst,
    }

    fn flush_eob(evs: &mut Vec<Ev>, eobrun: &mut u32, be: &mut Vec<u32>) {
        if *eobrun == 0 {
            return;
        }
        let nb = bit_len(*eobrun) - 1;
        evs.push(Ev::Sym(true, (nb << 4) as u8));
        evs.push(Ev::Bits(*eobrun & ((1 << nb) - 1), nb));
        for b in be.drain(..) {
            evs.push(Ev::Bits(b, 1));
        }
        *eobrun = 0;
    }

    /// The symbols and bits of one progressive scan, the way libjpeg's encoder orders them.
    fn scan_events(f: &EncFrame, spec: ScanSpec, restart: usize) -> Vec<Ev> {
        let (comps, ss, se, ah, al) = spec;
        let mut evs = Vec::new();
        let mut pred = vec![0i32; f.comps.len()];
        let mut eobrun = 0u32;
        let mut be: Vec<u32> = Vec::new();
        for (n, mcu) in scan_order(f, comps).iter().enumerate() {
            if restart > 0 && n > 0 && n % restart == 0 {
                flush_eob(&mut evs, &mut eobrun, &mut be);
                evs.push(Ev::Rst);
                pred.iter_mut().for_each(|p| *p = 0);
            }
            for &(ci, bi) in mcu {
                let blk = &f.comps[ci].blocks[bi];
                if ss == 0 {
                    if ah == 0 {
                        let v = (blk[0] as i32) >> al;
                        let (cat, bits) = category(v - pred[ci]);
                        pred[ci] = v;
                        evs.push(Ev::Sym(false, cat as u8));
                        evs.push(Ev::Bits(bits, cat));
                    } else {
                        evs.push(Ev::Bits((((blk[0] as i32) >> al) & 1) as u32, 1));
                    }
                } else if ah == 0 {
                    let mut r = 0u32;
                    for k in ss..=se {
                        let c = blk[ZIGZAG[k]] as i32;
                        let a = c.unsigned_abs() >> al;
                        if a == 0 {
                            r += 1;
                            continue;
                        }
                        flush_eob(&mut evs, &mut eobrun, &mut be);
                        while r > 15 {
                            evs.push(Ev::Sym(true, 0xF0));
                            r -= 16;
                        }
                        let (cat, bits) = category(if c < 0 { -(a as i32) } else { a as i32 });
                        evs.push(Ev::Sym(true, ((r << 4) | cat) as u8));
                        evs.push(Ev::Bits(bits, cat));
                        r = 0;
                    }
                    if r > 0 {
                        eobrun += 1;
                        if eobrun == 0x7FFF {
                            flush_eob(&mut evs, &mut eobrun, &mut be);
                        }
                    }
                } else {
                    let mut absv = [0u32; 64];
                    let mut eob = 0;
                    for k in ss..=se {
                        absv[k] = (blk[ZIGZAG[k]] as i32).unsigned_abs() >> al;
                        if absv[k] == 1 {
                            eob = k;
                        }
                    }
                    let mut r = 0u32;
                    let mut br: Vec<u32> = Vec::new();
                    for k in ss..=se {
                        let t = absv[k];
                        if t == 0 {
                            r += 1;
                            continue;
                        }
                        while r > 15 && k <= eob {
                            flush_eob(&mut evs, &mut eobrun, &mut be);
                            evs.push(Ev::Sym(true, 0xF0));
                            r -= 16;
                            for b in br.drain(..) {
                                evs.push(Ev::Bits(b, 1));
                            }
                        }
                        if t > 1 {
                            br.push(t & 1);
                            continue;
                        }
                        flush_eob(&mut evs, &mut eobrun, &mut be);
                        evs.push(Ev::Sym(true, ((r << 4) | 1) as u8));
                        evs.push(Ev::Bits(if blk[ZIGZAG[k]] < 0 { 0 } else { 1 }, 1));
                        for b in br.drain(..) {
                            evs.push(Ev::Bits(b, 1));
                        }
                        r = 0;
                    }
                    if r > 0 || !br.is_empty() {
                        eobrun += 1;
                        be.append(&mut br);
                        if eobrun == 0x7FFF || be.len() > 937 {
                            flush_eob(&mut evs, &mut eobrun, &mut be);
                        }
                    }
                }
            }
        }
        flush_eob(&mut evs, &mut eobrun, &mut be);
        evs
    }

    /// A Huffman table giving every used symbol a code of one length, never the all-ones code.
    fn uniform_table(syms: &BTreeSet<u8>) -> ([u8; 16], Vec<u8>) {
        let n = syms.len();
        assert!(n < 256);
        let mut l = 1;
        while (1usize << l) <= n {
            l += 1;
        }
        let mut bits = [0u8; 16];
        bits[l - 1] = n as u8;
        (bits, syms.iter().copied().collect())
    }

    /// (components, Ss, Se, Ah, Al)
    type ScanSpec = (&'static [usize], usize, usize, u32, u32);

    const SCRIPT_YCC: [ScanSpec; 10] = [
        (&[0, 1, 2], 0, 0, 0, 1),
        (&[0], 1, 5, 0, 2),
        (&[2], 1, 63, 0, 1),
        (&[1], 1, 63, 0, 1),
        (&[0], 6, 63, 0, 2),
        (&[0], 1, 63, 2, 1),
        (&[0, 1, 2], 0, 0, 1, 0),
        (&[2], 1, 63, 1, 0),
        (&[1], 1, 63, 1, 0),
        (&[0], 1, 63, 1, 0),
    ];

    const SCRIPT_GREY: [ScanSpec; 9] = [
        (&[0], 0, 0, 0, 2),
        (&[0], 1, 9, 0, 1),
        (&[0], 10, 63, 0, 3),
        (&[0], 0, 0, 2, 1),
        (&[0], 10, 63, 3, 2),
        (&[0], 1, 9, 1, 0),
        (&[0], 10, 63, 2, 1),
        (&[0], 0, 0, 1, 0),
        (&[0], 10, 63, 1, 0),
    ];

    fn write_progressive(f: &EncFrame, script: &[ScanSpec], restart: usize) -> Vec<u8> {
        let mut out = Vec::new();
        write_headers(&mut out, f, 0xC2, &[]);
        if restart > 0 {
            write_segment(&mut out, 0xDD, &(restart as u16).to_be_bytes());
        }
        for &spec in script {
            let (comps, ss, se, ah, al) = spec;
            let evs = scan_events(f, spec, restart);
            let (mut dcs, mut acs) = (BTreeSet::new(), BTreeSet::new());
            for e in &evs {
                if let Ev::Sym(ac, s) = e {
                    if *ac {
                        acs.insert(*s);
                    } else {
                        dcs.insert(*s);
                    }
                }
            }
            let (dcb, dcv) = uniform_table(&dcs);
            let (acb, acv) = uniform_table(&acs);
            let mut dht = Vec::new();
            if !dcs.is_empty() {
                dht.extend(dht_payload(0, 0, &dcb, &dcv));
            }
            if !acs.is_empty() {
                dht.extend(dht_payload(1, 0, &acb, &acv));
            }
            if !dht.is_empty() {
                write_segment(&mut out, 0xC4, &dht);
            }
            let (dce, ace) = (huff_enc(&dcb, &dcv), huff_enc(&acb, &acv));
            let mut sos = vec![comps.len() as u8];
            for &ci in comps {
                sos.extend_from_slice(&[f.comps[ci].id, 0x00]);
            }
            sos.extend_from_slice(&[ss as u8, se as u8, ((ah << 4) | al) as u8]);
            write_segment(&mut out, 0xDA, &sos);
            let mut bw = BitWriter::new();
            let mut rst = 0u8;
            for e in evs {
                match e {
                    Ev::Sym(ac, s) => bw.sym(if ac { &ace } else { &dce }, s),
                    Ev::Bits(v, n) => bw.put(v, n),
                    Ev::Rst => {
                        bw.marker(0xD0 + rst);
                        rst = (rst + 1) & 7;
                    }
                }
            }
            bw.flush();
            out.extend_from_slice(&bw.out);
        }
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    fn count_rst(data: &[u8]) -> usize {
        data.windows(2).filter(|w| w[0] == 0xFF && (0xD0..=0xD7).contains(&w[1])).count()
    }

    /// Where the first scan's entropy-coded data starts.
    fn scan_start(data: &[u8]) -> usize {
        let mut p = 2;
        loop {
            let m = data[p + 1];
            let len = u16::from_be_bytes([data[p + 2], data[p + 3]]) as usize;
            p += 2 + len;
            if m == 0xDA {
                return p;
            }
        }
    }

    fn find_marker(data: &[u8], m: u8) -> usize {
        data.windows(2).position(|w| w[0] == 0xFF && w[1] == m).expect("marker present")
    }

    // ---- Tests ----

    #[test]
    fn standard_huffman_tables_are_complete() {
        let mut want: BTreeSet<u8> = [0x00, 0xF0].into_iter().collect();
        for r in 0..16u8 {
            for s in 1..=10u8 {
                want.insert((r << 4) | s);
            }
        }
        for (bits, vals) in [(&AC_LUMA_BITS, &AC_LUMA_VALS), (&AC_CHROMA_BITS, &AC_CHROMA_VALS)] {
            assert_eq!(bits.iter().map(|&b| b as usize).sum::<usize>(), vals.len());
            assert_eq!(vals.iter().copied().collect::<BTreeSet<u8>>(), want);
        }
        assert_eq!(DC_LUMA_BITS.iter().map(|&b| b as usize).sum::<usize>(), 12);
        assert_eq!(DC_CHROMA_BITS.iter().map(|&b| b as usize).sum::<usize>(), 12);
    }

    #[test]
    fn integer_idct_matches_float_reference() {
        let mut rng = Rng(0x1234_5678_9abc_def1);
        let cos = cos_table();
        let ones = [1u16; 64];
        let mut worst = 0;
        for round in 0..300 {
            let mut coef = [0i16; 64];
            let spread = if round % 3 == 0 { 300 } else { 60 };
            for (i, c) in coef.iter_mut().enumerate() {
                if i == 0 || rng.below(3) == 0 {
                    *c = rng.below(2 * spread + 1) as i16 - spread as i16;
                }
            }
            let mut out = [0u8; 64];
            idct_block(&coef, &ones, &mut out, 0, 8);
            for y in 0..8 {
                for x in 0..8 {
                    let mut s = 0f64;
                    for v in 0..8 {
                        for u in 0..8 {
                            s += cos[v][y] as f64 * cos[u][x] as f64 * coef[v * 8 + u] as f64;
                        }
                    }
                    let want = (s + 128.0).round().clamp(0.0, 255.0) as i32;
                    worst = worst.max((want - out[y * 8 + x] as i32).abs());
                }
            }
        }
        assert!(worst <= 1, "IDCT off by {worst}");
    }

    #[test]
    fn gradient_round_trip_at_quality_90() {
        let img = gradient(64, 48);
        let data = encode(&img, 90);
        let (back, info) = decode_with_info(&data).unwrap();
        assert_eq!(info.components, 3);
        assert!(!info.progressive);
        assert_eq!(info.orientation, 1);
        let e = mae(&img, &back);
        assert!(e < 1.5, "gradient MAE {e}");
        assert!(back.pixels.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn checker_round_trip_at_quality_90() {
        // Saturated red against blue in 5-pixel cells: hard chroma edges, the worst case for JPEG.
        let img = checker(40, 40, 5);
        let back = decode(&encode(&img, 90)).unwrap();
        let e = mae(&img, &back);
        assert!(e < 7.0, "checker MAE {e}");
        // At quality 100 the whole pipeline is nearly lossless.
        let e = mae(&img, &decode(&encode(&img, 100)).unwrap());
        assert!(e < 0.6, "checker q100 MAE {e}");
    }

    #[test]
    fn quality_75_uses_420_and_stays_close() {
        let img = smooth(80, 60);
        let data = encode(&img, 75);
        // Luma sampling 2x2 in the frame header.
        let sof = find_marker(&data, 0xC0);
        assert_eq!(data[sof + 11], 0x22);
        let e = mae(&img, &decode(&data).unwrap());
        assert!(e < 4.0, "4:2:0 MAE {e}");
        let hi = encode(&img, 95);
        assert_eq!(hi[find_marker(&hi, 0xC0) + 11], 0x11);
        assert!(encode(&img, 10).len() < data.len());
    }

    #[test]
    fn grey_picture_is_written_with_one_component() {
        let img = grey_of(&smooth(50, 30));
        let data = encode(&img, 85);
        let (back, info) = decode_with_info(&data).unwrap();
        assert_eq!(info.components, 1);
        assert!(back.pixels.chunks_exact(4).all(|p| p[0] == p[1] && p[1] == p[2]));
        let e = mae(&img, &back);
        assert!(e < 2.5, "grey MAE {e}");
    }

    #[test]
    fn alpha_is_composited_onto_white() {
        let mut img = Image::filled(9, 9, [0, 0, 0, 0]);
        img.pixels[3] = 255; // one opaque black pixel
        let (back, info) = decode_with_info(&encode(&img, 90)).unwrap();
        assert_eq!(info.components, 1);
        assert!(back.pixel(8, 8).unwrap()[0] > 245);
        assert!(back.pixel(0, 0).unwrap()[0] < 120);
    }

    #[test]
    fn odd_sizes_in_every_sampling_mode() {
        for &(w, h) in &[(1u32, 1u32), (17, 9), (33, 65)] {
            let img = smooth(w, h);
            // What `encode` makes: 4:2:0, 4:4:4 and greyscale.
            for (q, limit) in [(75u8, 9.0), (95, 3.0)] {
                let back = decode(&encode(&img, q)).unwrap();
                let e = mae(&img, &back);
                assert!(e < limit, "{w}x{h} q{q} MAE {e}");
            }
            let grey = grey_of(&img);
            let (back, info) = decode_with_info(&encode(&grey, 80)).unwrap();
            assert_eq!(info.components, 1);
            assert!(mae(&grey, &back) < 4.0, "{w}x{h} grey");
            // Other luma sampling factors, to exercise the decoder's upsampling: 4:2:2, 4:1:1, 4:4:0,
            // 4:2:0 and 4:4:4.
            for (hy, vy) in [(2, 1), (4, 1), (1, 2), (2, 2), (1, 1)] {
                let data = write_baseline(&ycc_frame(&img, 92, hy, vy), 0, &[]);
                let back = decode(&data).unwrap();
                assert_eq!((back.width, back.height), (w, h));
                let e = mae(&img, &back);
                assert!(e < 10.0, "{w}x{h} sampling {hy}x{vy} MAE {e}");
            }
        }
    }

    #[test]
    fn progressive_decodes_to_the_baseline_pixels() {
        for &(w, h) in &[(40u32, 24u32), (17, 9), (1, 1), (70, 33)] {
            let img = smooth(w, h);
            for (hy, vy) in [(2, 2), (1, 1), (2, 1)] {
                let f = ycc_frame(&img, 80, hy, vy);
                let base = decode(&write_baseline(&f, 0, &[])).unwrap();
                let (prog, info) = decode_with_info(&write_progressive(&f, &SCRIPT_YCC, 0)).unwrap();
                assert!(info.progressive);
                assert_eq!(prog, base, "{w}x{h} {hy}x{vy}");
            }
            let g = grey_frame(&grey_of(&img), 70);
            let base = decode(&write_baseline(&g, 0, &[])).unwrap();
            assert_eq!(decode(&write_progressive(&g, &SCRIPT_GREY, 0)).unwrap(), base);
        }
        // Fine detail at top quality: long zero runs, big coefficients, many refinement bits.
        let f = ycc_frame(&checker(64, 64, 1), 100, 1, 1);
        let base = decode(&write_baseline(&f, 0, &[])).unwrap();
        assert_eq!(decode(&write_progressive(&f, &SCRIPT_YCC, 0)).unwrap(), base);
    }

    #[test]
    fn progressive_with_restart_intervals() {
        let img = checker(45, 37, 3);
        let f = ycc_frame(&img, 85, 2, 2);
        let base = decode(&write_baseline(&f, 0, &[])).unwrap();
        for ri in [1, 2, 5] {
            let data = write_progressive(&f, &SCRIPT_YCC, ri);
            assert!(count_rst(&data) > 0);
            assert_eq!(decode(&data).unwrap(), base, "restart {ri}");
        }
    }

    #[test]
    fn progressive_cut_after_whole_scans_still_decodes() {
        let img = smooth(48, 40);
        let f = ycc_frame(&img, 85, 2, 2);
        let mut partial = write_progressive(&f, &SCRIPT_YCC[..5], 0);
        let full = decode(&write_progressive(&f, &SCRIPT_YCC, 0)).unwrap();
        let rough = decode(&partial).unwrap();
        assert!(mae(&rough, &full) < 12.0);
        // Cut inside a scan's entropy data: an error, not a panic.
        partial.truncate(partial.len() - 12);
        assert_eq!(decode(&partial), Err(JpegError::Truncated));
    }

    #[test]
    fn restart_markers_in_baseline() {
        let img = gradient(37, 29);
        for f in [ycc_frame(&img, 80, 2, 2), ycc_frame(&img, 90, 1, 1), grey_frame(&grey_of(&img), 80)] {
            let plain = decode(&write_baseline(&f, 0, &[])).unwrap();
            for ri in [1u16, 2, 5] {
                let data = write_baseline(&f, ri, &[]);
                assert!(count_rst(&data) > 0);
                assert_eq!(decode(&data).unwrap(), plain, "restart {ri}");
            }
        }
        // A missing restart marker is corrupt data.
        let data = write_baseline(&ycc_frame(&img, 80, 2, 2), 1, &[]);
        let start = scan_start(&data);
        let at = start + data[start..].windows(2).position(|w| w[0] == 0xFF && w[1] == 0xD0).unwrap();
        let mut broken = data.clone();
        broken.drain(at..at + 2);
        assert!(decode(&broken).is_err());
    }

    fn exif(le: bool, orientation: u16) -> Vec<u8> {
        let w16 = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let w32 = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let mut p = b"Exif\0\0".to_vec();
        p.extend_from_slice(if le { b"II" } else { b"MM" });
        p.extend_from_slice(&w16(42));
        p.extend_from_slice(&w32(8));
        p.extend_from_slice(&w16(2));
        p.extend_from_slice(&w16(0x010F));
        p.extend_from_slice(&w16(2));
        p.extend_from_slice(&w32(4));
        p.extend_from_slice(b"Cam\0");
        p.extend_from_slice(&w16(0x0112));
        p.extend_from_slice(&w16(3));
        p.extend_from_slice(&w32(1));
        p.extend_from_slice(&w16(orientation));
        p.extend_from_slice(&[0, 0]);
        p.extend_from_slice(&w32(0));
        p
    }

    #[test]
    fn exif_orientation_is_reported() {
        let f = ycc_frame(&smooth(16, 16), 80, 2, 2);
        for (le, o) in [(true, 6u16), (false, 8), (true, 3), (false, 1)] {
            let data = write_baseline(&f, 0, &[(0xE1, exif(le, o))]);
            let (_, info) = decode_with_info(&data).unwrap();
            assert_eq!(info.orientation, o as u8);
        }
        // An out-of-range value and a broken EXIF block are ignored.
        let data = write_baseline(&f, 0, &[(0xE1, exif(true, 42))]);
        assert_eq!(decode_with_info(&data).unwrap().1.orientation, 1);
        let mut bad = exif(false, 6);
        bad.truncate(20);
        let data = write_baseline(&f, 0, &[(0xE1, bad)]);
        assert_eq!(decode_with_info(&data).unwrap().1.orientation, 1);
    }

    #[test]
    fn oriented_turns_and_mirrors() {
        let img = make(3, 2, |x, y| [(x * 10 + y) as u8, 0, 0]);
        let r = img.oriented(6);
        assert_eq!((r.width, r.height), (2, 3));
        assert_eq!(r.pixel(0, 0).unwrap()[0], 1); // bottom-left of the source
        assert_eq!(r.pixel(1, 0).unwrap()[0], 0); // top-left of the source
        assert_eq!(r.oriented(8), img);
        for o in [2, 3, 4, 5, 7] {
            assert_eq!(img.oriented(o).oriented(o), img, "orientation {o}");
        }
        assert_eq!(img.oriented(1), img);
        assert_eq!(img.oriented(2).pixel(0, 0).unwrap()[0], 20);
    }

    fn adobe(transform: u8) -> (u8, Vec<u8>) {
        let mut p = b"Adobe".to_vec();
        p.extend_from_slice(&[0, 100, 0, 0, 0, 0, transform]);
        (0xEE, p)
    }

    #[test]
    fn four_components_cmyk_and_ycck() {
        let (w, h) = (24usize, 20usize);
        let mut planes = [vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h]];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                planes[0][i] = (x * 10) as u8;
                planes[1][i] = (y * 12) as u8;
                planes[2][i] = 200;
                planes[3][i] = (255 - x * 4) as u8;
            }
        }
        let expect: Vec<[u8; 3]> = (0..w * h)
            .map(|i| {
                let k = planes[3][i] as u32;
                [0, 1, 2].map(|c| ((planes[c][i] as u32 * k + 127) / 255) as u8)
            })
            .collect();
        let check = |data: &[u8], transform: u8, limit: f64| {
            let (img, info) = decode_with_info(data).unwrap();
            assert_eq!(info.components, 4);
            assert_eq!(info.adobe_transform, Some(transform));
            let mut sum = 0u64;
            for (i, e) in expect.iter().enumerate() {
                for c in 0..3 {
                    sum += (img.pixels[i * 4 + c] as i32 - e[c] as i32).unsigned_abs() as u64;
                }
            }
            let m = sum as f64 / (w * h * 3) as f64;
            assert!(m < limit, "transform {transform} MAE {m}");
        };
        let f = raw_frame(w, h, &[&planes[0], &planes[1], &planes[2], &planes[3]], &[1, 2, 3, 4]);
        check(&write_baseline(&f, 0, &[adobe(0)]), 0, 2.5);

        // YCCK: Y, Cb, Cr of (255 - C, 255 - M, 255 - Y), then K.
        let (mut y, mut cb, mut cr) = (vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h]);
        for i in 0..w * h {
            let [a, b, c] = rgb_to_ycc(255 - planes[0][i], 255 - planes[1][i], 255 - planes[2][i]);
            y[i] = a;
            cb[i] = b;
            cr[i] = c;
        }
        let f = raw_frame(w, h, &[&y, &cb, &cr, &planes[3]], &[1, 2, 3, 4]);
        check(&write_baseline(&f, 0, &[adobe(2)]), 2, 3.5);
    }

    #[test]
    fn three_components_without_transform_are_rgb() {
        let img = smooth(20, 12);
        let rgb = rgb_of(&img);
        let chans: Vec<Vec<u8>> = (0..3).map(|c| rgb.iter().skip(c).step_by(3).copied().collect()).collect();
        // Ids R, G, B alone are enough, and so is an Adobe "no transform" flag.
        let f = raw_frame(20, 12, &[&chans[0], &chans[1], &chans[2]], b"RGB");
        assert!(mae(&img, &decode(&write_baseline(&f, 0, &[])).unwrap()) < 2.5);
        let f = raw_frame(20, 12, &[&chans[0], &chans[1], &chans[2]], &[1, 2, 3]);
        let (back, info) = decode_with_info(&write_baseline(&f, 0, &[adobe(0)])).unwrap();
        assert_eq!(info.adobe_transform, Some(0));
        assert!(mae(&img, &back) < 2.5);
    }

    #[test]
    fn sixteen_bit_quantization_tables_are_read() {
        let img = smooth(16, 16);
        let data = encode(&img, 80);
        let plain = decode(&data).unwrap();
        // Rewrite the DQT segment with 16-bit entries.
        let at = find_marker(&data, 0xDB);
        let len = u16::from_be_bytes([data[at + 2], data[at + 3]]) as usize;
        let seg = &data[at + 4..at + 2 + len];
        let mut wide = Vec::new();
        for t in seg.chunks(65) {
            wide.push(0x10 | t[0]);
            for &v in &t[1..] {
                wide.extend_from_slice(&[0, v]);
            }
        }
        let mut out = data[..at].to_vec();
        write_segment(&mut out, 0xDB, &wide);
        out.extend_from_slice(&data[at + 2 + len..]);
        assert_eq!(decode(&out).unwrap(), plain);
    }

    #[test]
    fn corrupt_and_truncated_inputs_are_errors() {
        assert_eq!(decode(&[]), Err(JpegError::NotJpeg));
        assert_eq!(decode(b"\x89PNG\r\n"), Err(JpegError::NotJpeg));
        assert!(decode(&[0xFF, 0xD8]).is_err());
        assert!(decode(&[0xFF, 0xD8, 0xFF, 0xD9]).is_err());
        assert!(decode(&[0xFF, 0xD8, 0xFF, 0xDB, 0x00]).is_err());

        let data = encode(&smooth(33, 17), 80);
        // Every cut short of the EOI is an error; dropping only the EOI is fine.
        for cut in 0..data.len() - 2 {
            assert!(decode(&data[..cut]).is_err(), "cut at {cut} of {}", data.len());
        }
        assert!(decode(&data[..data.len() - 2]).is_ok());
        assert_eq!(decode(&data[..scan_start(&data) + 20]), Err(JpegError::Truncated));

        // Header fields.
        let sof = find_marker(&data, 0xC0);
        let patch = |at: usize, bytes: &[u8]| {
            let mut d = data.clone();
            d[at..at + bytes.len()].copy_from_slice(bytes);
            decode(&d)
        };
        assert_eq!(patch(sof + 5, &[0xFF, 0xFF, 0xFF, 0xFF]), Err(JpegError::TooLarge));
        assert!(matches!(patch(sof + 7, &[0, 0]), Err(JpegError::Corrupt(_))));
        assert!(matches!(patch(sof + 4, &[12]), Err(JpegError::Unsupported(_))));
        assert!(matches!(patch(sof + 1, &[0xC9]), Err(JpegError::Unsupported(_))));
        assert!(matches!(patch(sof + 9, &[2]), Err(JpegError::Unsupported(_))));
        assert!(matches!(patch(sof + 11, &[0x55]), Err(JpegError::Corrupt(_))));
        // An over-full Huffman table.
        let dht = find_marker(&data, 0xC4);
        assert!(matches!(patch(dht + 5, &[3]), Err(JpegError::Corrupt(_))));
        // A scan naming a component the frame lacks.
        let sos = find_marker(&data, 0xDA);
        assert!(matches!(patch(sos + 5, &[9]), Err(JpegError::Corrupt(_))));

        // Random damage never panics.
        let prog = write_progressive(&ycc_frame(&smooth(30, 22), 80, 2, 2), &SCRIPT_YCC, 2);
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for src in [&data, &prog] {
            for _ in 0..400 {
                let mut d = src.to_vec();
                for _ in 0..1 + rng.below(4) {
                    let i = rng.below(d.len());
                    d[i] ^= 1 << rng.below(8);
                }
                if let Ok(img) = decode(&d) {
                    assert_eq!(img.pixels.len(), img.width as usize * img.height as usize * 4);
                }
            }
        }
        for _ in 0..200 {
            let mut d = vec![0xFF, 0xD8];
            for _ in 0..rng.below(300) {
                d.push(if rng.below(5) == 0 { 0xFF } else { rng.next() as u8 });
            }
            let _ = decode(&d);
        }
    }

    #[test]
    fn zero_sized_picture_encodes_as_one_white_pixel() {
        let img = Image { width: 0, height: 5, pixels: Vec::new() };
        let back = decode(&encode(&img, 50)).unwrap();
        assert_eq!((back.width, back.height), (1, 1));
        assert!(back.pixels[0] > 250);
    }
}
