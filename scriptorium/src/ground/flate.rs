//! Flate: DEFLATE (RFC 1951) and the zlib wrapper (RFC 1950), with the CRC-32 and Adler-32 checksums.
//!
//! Everything here is written from the RFCs with the standard library only.
//!
//! * [`inflate`] / [`inflate_limited`] decode a raw DEFLATE stream (stored, fixed and dynamic blocks).
//!   The decoder never panics and never loops forever on malformed input: every step consumes input,
//!   and every read is bounds-checked. [`inflate_limited`] also caps the output size, which guards
//!   against decompression bombs.
//! * [`deflate`] encodes a raw DEFLATE stream. Level 0 writes stored blocks; levels 1..=9 run LZ77 over
//!   a 32 KiB window with hash chains (lazy matching from level 4 up) and pick, per block, whichever of
//!   a dynamic Huffman, fixed Huffman or stored encoding is smallest. Dynamic codes are length-limited
//!   (15 bits for literals/lengths and distances, 7 bits for the code-length code).
//! * [`zlib_compress`] / [`zlib_decompress`] add and check the 2-byte zlib header and the Adler-32 trailer.
//! * [`crc32`] / [`Crc32`] (the ZIP / PNG polynomial) and [`adler32`].

use std::fmt;

// ---------------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------------

/// An error from decoding a DEFLATE or zlib stream. The message says what was wrong with the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlateError(pub String);

impl fmt::Display for FlateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "flate: {}", self.0)
    }
}

impl std::error::Error for FlateError {}

fn fail<T>(msg: &str) -> Result<T, FlateError> {
    Err(FlateError(msg.to_string()))
}

// ---------------------------------------------------------------------------------------------------
// Checksums
// ---------------------------------------------------------------------------------------------------

const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = make_crc_table();

/// An incremental CRC-32 (IEEE 802.3 polynomial, reflected; the checksum of ZIP, PNG and gzip).
///
/// ```ignore
/// let mut c = Crc32::new();
/// c.update(b"1234");
/// c.update(b"56789");
/// assert_eq!(c.finish(), 0xCBF4_3926);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crc32 {
    state: u32,
}

impl Crc32 {
    /// A fresh checksum (of no bytes yet).
    pub fn new() -> Self {
        Crc32 { state: 0xFFFF_FFFF }
    }

    /// Feeds more bytes.
    pub fn update(&mut self, data: &[u8]) {
        let mut c = self.state;
        for &b in data {
            c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        self.state = c;
    }

    /// The CRC-32 of every byte fed so far. Does not reset; more bytes may still be fed.
    pub fn finish(&self) -> u32 {
        self.state ^ 0xFFFF_FFFF
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Crc32::new()
    }
}

/// The CRC-32 of `data` in one call.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(data);
    c.finish()
}

/// The Adler-32 checksum of `data` (the zlib trailer).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    // 5552 is the largest run for which `b` cannot overflow a u32 before the modulo.
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

// ---------------------------------------------------------------------------------------------------
// Shared tables (RFC 1951 section 3.2.5)
// ---------------------------------------------------------------------------------------------------

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163,
    195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049,
    3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];
/// The order in which code-length code lengths are sent.
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// The code lengths of the fixed literal/length code (288 symbols).
fn fixed_lit_lengths() -> [u8; 288] {
    let mut l = [0u8; 288];
    for (i, v) in l.iter_mut().enumerate() {
        *v = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    l
}

/// Reverses the low `len` bits of `code` (Huffman codes are sent most-significant bit first, while the
/// bit stream is packed least-significant bit first).
fn reverse_bits(code: u32, len: u32) -> u32 {
    let mut r = 0;
    let mut c = code;
    for _ in 0..len {
        r = (r << 1) | (c & 1);
        c >>= 1;
    }
    r
}

// ---------------------------------------------------------------------------------------------------
// Inflate
// ---------------------------------------------------------------------------------------------------

/// Reads bits least-significant first from a byte slice. Every read is checked.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    cnt: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0, buf: 0, cnt: 0 }
    }

    /// Tops the bit buffer up to at least 57 bits, or as many as the input still holds.
    fn refill(&mut self) {
        while self.cnt <= 56 && self.pos < self.data.len() {
            self.buf |= (self.data[self.pos] as u64) << self.cnt;
            self.pos += 1;
            self.cnt += 8;
        }
    }

    fn consume(&mut self, n: u32) {
        self.buf >>= n;
        self.cnt -= n;
    }

    /// Reads `n` (0..=32) bits.
    fn bits(&mut self, n: u32) -> Result<u32, FlateError> {
        if n == 0 {
            return Ok(0);
        }
        if self.cnt < n {
            self.refill();
            if self.cnt < n {
                return fail("unexpected end of data");
            }
        }
        let v = (self.buf & ((1u64 << n) - 1)) as u32;
        self.consume(n);
        Ok(v)
    }

    /// Drops the bits up to the next byte boundary.
    fn align(&mut self) {
        let r = self.cnt % 8;
        self.consume(r);
    }

    /// The index of the first byte not yet consumed. Only meaningful when byte aligned.
    fn byte_pos(&self) -> usize {
        self.pos - (self.cnt / 8) as usize
    }

    /// Takes `n` whole bytes. Must be byte aligned. Hands the buffered bytes back to the slice first.
    fn take_bytes(&mut self, n: usize) -> Result<&'a [u8], FlateError> {
        let start = self.byte_pos();
        self.pos = start;
        self.buf = 0;
        self.cnt = 0;
        match start.checked_add(n) {
            Some(end) if end <= self.data.len() => {
                self.pos = end;
                Ok(&self.data[start..end])
            }
            _ => fail("unexpected end of data in stored block"),
        }
    }
}

const FAST_BITS: u32 = 10;

/// A canonical Huffman decoder: a 10-bit lookup table for short codes and a bit-by-bit walk of the
/// canonical code (counts + symbols, as in zlib's `puff`) for longer ones.
struct Decoder {
    counts: [u16; 16],
    symbols: Vec<u16>,
    /// `symbol << 4 | length`, or 0 when the code is longer than `FAST_BITS` (or unassigned).
    fast: Vec<u16>,
}

impl Decoder {
    fn new(lengths: &[u8]) -> Result<Decoder, FlateError> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l > 15 {
                return fail("code length over 15");
            }
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut left: i32 = 1;
        for &c in counts.iter().skip(1) {
            left <<= 1;
            left -= c as i32;
            if left < 0 {
                return fail("over-subscribed Huffman code");
            }
        }
        // Incomplete codes are accepted; an unassigned code simply fails to decode.
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + counts[len];
        }
        let total: usize = counts.iter().map(|&c| c as usize).sum();
        let mut symbols = vec![0u16; total];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        let mut next_code = [0u32; 16];
        let mut code = 0u32;
        for len in 1..16 {
            code = (code + counts[len - 1] as u32) << 1;
            next_code[len] = code;
        }
        let mut fast = vec![0u16; 1 << FAST_BITS];
        for (sym, &l) in lengths.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let l = l as u32;
            let c = next_code[l as usize];
            next_code[l as usize] += 1;
            if l <= FAST_BITS {
                let entry = ((sym as u16) << 4) | l as u16;
                let mut i = reverse_bits(c, l) as usize;
                while i < fast.len() {
                    fast[i] = entry;
                    i += 1 << l;
                }
            }
        }
        Ok(Decoder { counts, symbols, fast })
    }

    fn decode(&self, br: &mut BitReader) -> Result<u16, FlateError> {
        br.refill();
        let e = self.fast[(br.buf & ((1 << FAST_BITS) - 1)) as usize];
        if e != 0 {
            let l = (e & 15) as u32;
            if l > br.cnt {
                return fail("unexpected end of data");
            }
            br.consume(l);
            return Ok(e >> 4);
        }
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16u32 {
            if len > br.cnt {
                return fail("unexpected end of data");
            }
            code |= ((br.buf >> (len - 1)) & 1) as i32;
            let count = self.counts[len as usize] as i32;
            if code - first < count {
                br.consume(len);
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        fail("invalid Huffman code")
    }
}

/// Decodes a raw DEFLATE stream (no zlib or gzip header). Bytes after the final block are ignored.
pub fn inflate(data: &[u8]) -> Result<Vec<u8>, FlateError> {
    inflate_limited(data, usize::MAX)
}

/// Like [`inflate`], but fails as soon as the output would grow past `max_out` bytes.
pub fn inflate_limited(data: &[u8], max_out: usize) -> Result<Vec<u8>, FlateError> {
    inflate_inner(data, max_out).map(|(out, _)| out)
}

/// Decodes a raw DEFLATE stream; returns the output and the number of input bytes consumed (rounded up
/// to the byte holding the end of the final block).
fn inflate_inner(data: &[u8], max_out: usize) -> Result<(Vec<u8>, usize), FlateError> {
    let cap = data.len().saturating_mul(4).min(max_out).min(64 << 20);
    let mut out = Vec::with_capacity(cap);
    let mut br = BitReader::new(data);
    let mut fixed: Option<(Decoder, Decoder)> = None;
    loop {
        let last = br.bits(1)?;
        match br.bits(2)? {
            0 => {
                br.align();
                let len = br.bits(16)?;
                let nlen = br.bits(16)?;
                if len != !nlen & 0xFFFF {
                    return fail("stored block length check failed");
                }
                let len = len as usize;
                if len > max_out - out.len() {
                    return fail("output limit exceeded");
                }
                let bytes = br.take_bytes(len)?;
                out.extend_from_slice(bytes);
            }
            1 => {
                if fixed.is_none() {
                    fixed = Some((Decoder::new(&fixed_lit_lengths())?, Decoder::new(&[5u8; 30])?));
                }
                let (lit, dist) = fixed.as_ref().expect("built above");
                inflate_block(&mut br, &mut out, lit, dist, max_out)?;
            }
            2 => {
                let (lit, dist) = read_dynamic_header(&mut br)?;
                inflate_block(&mut br, &mut out, &lit, &dist, max_out)?;
            }
            _ => return fail("invalid block type 3"),
        }
        if last == 1 {
            break;
        }
    }
    br.align();
    Ok((out, br.byte_pos()))
}

fn read_dynamic_header(br: &mut BitReader) -> Result<(Decoder, Decoder), FlateError> {
    let hlit = br.bits(5)? as usize + 257;
    let hdist = br.bits(5)? as usize + 1;
    let hclen = br.bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return fail("too many length or distance codes");
    }
    let mut cl = [0u8; 19];
    for &idx in CL_ORDER.iter().take(hclen) {
        cl[idx] = br.bits(3)? as u8;
    }
    let cld = Decoder::new(&cl)?;
    let total = hlit + hdist;
    let mut lengths = vec![0u8; total];
    let mut i = 0;
    while i < total {
        let sym = cld.decode(br)?;
        let (value, repeat) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                if i == 0 {
                    return fail("repeat with no previous length");
                }
                (lengths[i - 1], 3 + br.bits(2)? as usize)
            }
            17 => (0, 3 + br.bits(3)? as usize),
            18 => (0, 11 + br.bits(7)? as usize),
            _ => return fail("invalid code-length symbol"),
        };
        if i + repeat > total {
            return fail("code-length repeat runs past the end");
        }
        for l in &mut lengths[i..i + repeat] {
            *l = value;
        }
        i += repeat;
    }
    if lengths[256] == 0 {
        return fail("no end-of-block code");
    }
    Ok((Decoder::new(&lengths[..hlit])?, Decoder::new(&lengths[hlit..])?))
}

fn inflate_block(
    br: &mut BitReader,
    out: &mut Vec<u8>,
    lit: &Decoder,
    dist: &Decoder,
    max_out: usize,
) -> Result<(), FlateError> {
    loop {
        let sym = lit.decode(br)? as usize;
        if sym < 256 {
            if out.len() >= max_out {
                return fail("output limit exceeded");
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let li = sym - 257;
            if li >= 29 {
                return fail("invalid length symbol");
            }
            let len = LEN_BASE[li] as usize + br.bits(LEN_EXTRA[li] as u32)? as usize;
            let ds = dist.decode(br)? as usize;
            if ds >= 30 {
                return fail("invalid distance symbol");
            }
            let d = DIST_BASE[ds] as usize + br.bits(DIST_EXTRA[ds] as u32)? as usize;
            if d > out.len() {
                return fail("distance reaches before the start of the output");
            }
            if len > max_out - out.len() {
                return fail("output limit exceeded");
            }
            let start = out.len() - d;
            if d >= len {
                out.extend_from_within(start..start + len);
            } else {
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// zlib
// ---------------------------------------------------------------------------------------------------

/// Decodes a zlib stream (RFC 1950): checks the header, inflates, and checks the Adler-32 trailer.
/// Streams that need a preset dictionary are refused.
pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>, FlateError> {
    zlib_decompress_limited(data, usize::MAX)
}

/// Like [`zlib_decompress`], but fails as soon as the output would grow past `max_out` bytes.
pub fn zlib_decompress_limited(data: &[u8], max_out: usize) -> Result<Vec<u8>, FlateError> {
    if data.len() < 2 {
        return fail("zlib header missing");
    }
    let (cmf, flg) = (data[0], data[1]);
    if cmf & 0x0F != 8 {
        return fail("zlib compression method is not deflate");
    }
    if cmf >> 4 > 7 {
        return fail("zlib window size over 32 KiB");
    }
    if ((cmf as u16) << 8 | flg as u16) % 31 != 0 {
        return fail("zlib header check failed");
    }
    if flg & 0x20 != 0 {
        return fail("zlib preset dictionary is not supported");
    }
    let (out, used) = inflate_inner(&data[2..], max_out)?;
    let t = 2 + used;
    if data.len() < t + 4 {
        return fail("zlib Adler-32 trailer missing");
    }
    let want = u32::from_be_bytes([data[t], data[t + 1], data[t + 2], data[t + 3]]);
    if adler32(&out) != want {
        return fail("zlib Adler-32 mismatch");
    }
    Ok(out)
}

/// Encodes `data` as a zlib stream (RFC 1950) at `level` 0..=9 (higher levels are treated as 9).
pub fn zlib_compress(data: &[u8], level: u8) -> Vec<u8> {
    let flevel: u16 = match level {
        0 | 1 => 0,
        2..=5 => 1,
        6 => 2,
        _ => 3,
    };
    let base: u16 = (0x78 << 8) | (flevel << 6);
    let fcheck = (31 - base % 31) % 31;
    let head = base | fcheck;
    let mut out = head.to_be_bytes().to_vec();
    out.extend_from_slice(&deflate(data, level));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

// ---------------------------------------------------------------------------------------------------
// Deflate: bit writer and Huffman code construction
// ---------------------------------------------------------------------------------------------------

struct BitWriter {
    out: Vec<u8>,
    buf: u64,
    cnt: u32,
}

impl BitWriter {
    fn new(cap: usize) -> Self {
        BitWriter { out: Vec::with_capacity(cap), buf: 0, cnt: 0 }
    }

    /// Writes the low `n` (0..=32) bits of `v`, least-significant first.
    fn bits(&mut self, v: u32, n: u32) {
        if n == 0 {
            return;
        }
        self.buf |= ((v as u64) & ((1u64 << n) - 1)) << self.cnt;
        self.cnt += n;
        while self.cnt >= 8 {
            self.out.push(self.buf as u8);
            self.buf >>= 8;
            self.cnt -= 8;
        }
    }

    fn align(&mut self) {
        if self.cnt > 0 {
            self.out.push(self.buf as u8);
            self.buf = 0;
            self.cnt = 0;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        self.align();
        self.out
    }
}

/// Builds Huffman code lengths for `freq`, limited to `limit` bits.
///
/// Lengths come from a Huffman tree (two-queue construction over the frequency-sorted symbols); if the
/// tree is deeper than `limit`, the over-long codes are clamped and the length counts are rebalanced
/// until the Kraft sum is exact again (the method of miniz's `enforce_max_code_size`). The lengths are
/// then handed out longest-first to the least frequent symbols. With fewer than two used symbols, two
/// codes of length 1 are produced so every code is complete.
fn build_lengths(freq: &[u32], limit: usize) -> Vec<u8> {
    let n = freq.len();
    let mut lens = vec![0u8; n];
    let mut syms: Vec<usize> = (0..n).filter(|&i| freq[i] > 0).collect();
    if syms.len() < 2 {
        let s = syms.first().copied().unwrap_or(0);
        lens[s] = 1;
        lens[if s == 0 { 1 } else { 0 }] = 1;
        return lens;
    }
    syms.sort_by_key(|&i| (freq[i], i));
    let m = syms.len();
    let mut weight: Vec<u64> = syms.iter().map(|&s| freq[s] as u64).collect();
    weight.reserve(m - 1);
    let mut parent = vec![0usize; 2 * m - 1];
    let (mut leaf, mut inner, mut next) = (0usize, m, m);
    for _ in 0..m - 1 {
        let mut two = [0usize; 2];
        for slot in two.iter_mut() {
            if leaf < m && (inner >= next || weight[leaf] <= weight[inner]) {
                *slot = leaf;
                leaf += 1;
            } else {
                *slot = inner;
                inner += 1;
            }
        }
        weight.push(weight[two[0]] + weight[two[1]]);
        parent[two[0]] = next;
        parent[two[1]] = next;
        next += 1;
    }
    let root = 2 * m - 2;
    let mut depth = vec![0usize; 2 * m - 1];
    for node in (0..root).rev() {
        depth[node] = depth[parent[node]] + 1;
    }
    let mut num = vec![0u64; limit + 1];
    for &d in depth.iter().take(m) {
        num[d.min(limit)] += 1;
    }
    let mut total: u64 = (1..=limit).map(|i| num[i] << (limit - i)).sum();
    while total > 1u64 << limit {
        if num[limit] == 0 {
            break;
        }
        num[limit] -= 1;
        for i in (1..limit).rev() {
            if num[i] != 0 {
                num[i] -= 1;
                num[i + 1] += 2;
                break;
            }
        }
        total -= 1;
    }
    if total != 1u64 << limit {
        // Cannot happen for deflate's alphabets; a flat code is a safe answer anyway.
        let bits = (usize::BITS - (m - 1).leading_zeros()) as u8;
        for &s in &syms {
            lens[s] = bits.max(1);
        }
        return lens;
    }
    let mut idx = 0;
    for len in (1..=limit).rev() {
        for _ in 0..num[len] {
            lens[syms[idx]] = len as u8;
            idx += 1;
        }
    }
    lens
}

/// Canonical codes for `lens`, already bit-reversed for least-significant-first writing.
fn canonical_codes(lens: &[u8]) -> Vec<u16> {
    let mut count = [0u32; 16];
    for &l in lens {
        count[l as usize] += 1;
    }
    count[0] = 0;
    let mut next = [0u32; 16];
    let mut code = 0u32;
    for len in 1..16 {
        code = (code + count[len - 1]) << 1;
        next[len] = code;
    }
    lens.iter()
        .map(|&l| {
            if l == 0 {
                return 0;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            reverse_bits(c, l as u32) as u16
        })
        .collect()
}

/// Run-length encodes a code-length sequence with symbols 16 (repeat previous), 17 and 18 (zeros).
/// Each item is `(symbol, extra-bits value)`.
fn rle_lengths(lens: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lens.len() {
        let cur = lens[i];
        let mut run = 1;
        while i + run < lens.len() && lens[i + run] == cur {
            run += 1;
        }
        let mut r = run;
        if cur == 0 {
            while r >= 11 {
                let k = r.min(138);
                out.push((18, (k - 11) as u8));
                r -= k;
            }
            if r >= 3 {
                out.push((17, (r - 3) as u8));
                r = 0;
            }
        } else {
            out.push((cur, 0));
            r -= 1;
            while r >= 3 {
                let k = r.min(6);
                out.push((16, (k - 3) as u8));
                r -= k;
            }
        }
        for _ in 0..r {
            out.push((cur, 0));
        }
        i += run;
    }
    out
}

fn cl_extra_bits(sym: u8) -> u32 {
    match sym {
        16 => 2,
        17 => 3,
        18 => 7,
        _ => 0,
    }
}

fn dist_code(d: u16) -> usize {
    match DIST_BASE.binary_search(&d) {
        Ok(i) => i,
        Err(i) => i - 1,
    }
}

// ---------------------------------------------------------------------------------------------------
// Deflate: blocks
// ---------------------------------------------------------------------------------------------------

/// One LZ77 token: a literal (`dist == 0`, byte in `len`) or a match of `len` bytes `dist` back.
#[derive(Clone, Copy)]
struct Token {
    len: u16,
    dist: u16,
}

/// Tokens per block; a block is flushed whenever this many are buffered.
const BLOCK_TOKENS: usize = 16384;

/// Collects tokens and writes each block in whichever encoding is smallest.
struct BlockWriter<'a> {
    data: &'a [u8],
    w: BitWriter,
    tokens: Vec<Token>,
    block_start: usize,
    block_bytes: usize,
    len_code: [u8; 259],
}

impl<'a> BlockWriter<'a> {
    fn new(data: &'a [u8]) -> Self {
        let mut len_code = [0u8; 259];
        for (len, slot) in len_code.iter_mut().enumerate().skip(3) {
            *slot = match LEN_BASE.binary_search(&(len as u16)) {
                Ok(i) => i as u8,
                Err(i) => (i - 1) as u8,
            };
        }
        BlockWriter {
            data,
            w: BitWriter::new(data.len() / 2 + 64),
            tokens: Vec::with_capacity(BLOCK_TOKENS),
            block_start: 0,
            block_bytes: 0,
            len_code,
        }
    }

    fn literal(&mut self, b: u8) {
        self.tokens.push(Token { len: b as u16, dist: 0 });
        self.block_bytes += 1;
        if self.tokens.len() >= BLOCK_TOKENS {
            self.flush(false);
        }
    }

    fn matched(&mut self, len: usize, dist: usize) {
        self.tokens.push(Token { len: len as u16, dist: dist as u16 });
        self.block_bytes += len;
        if self.tokens.len() >= BLOCK_TOKENS {
            self.flush(false);
        }
    }

    /// Writes the buffered tokens as one block (the final one if `last`).
    fn flush(&mut self, last: bool) {
        let mut lit_freq = [0u32; 286];
        let mut dist_freq = [0u32; 30];
        let mut extra_bits: u64 = 0;
        for t in &self.tokens {
            if t.dist == 0 {
                lit_freq[t.len as usize] += 1;
            } else {
                let lc = self.len_code[t.len as usize] as usize;
                let dc = dist_code(t.dist);
                lit_freq[257 + lc] += 1;
                dist_freq[dc] += 1;
                extra_bits += LEN_EXTRA[lc] as u64 + DIST_EXTRA[dc] as u64;
            }
        }
        lit_freq[256] += 1;

        // Dynamic block cost.
        let lit_len = build_lengths(&lit_freq, 15);
        let dist_len = build_lengths(&dist_freq, 15);
        let mut hlit = 286;
        while hlit > 257 && lit_len[hlit - 1] == 0 {
            hlit -= 1;
        }
        let mut hdist = 30;
        while hdist > 1 && dist_len[hdist - 1] == 0 {
            hdist -= 1;
        }
        let mut seq = lit_len[..hlit].to_vec();
        seq.extend_from_slice(&dist_len[..hdist]);
        let rle = rle_lengths(&seq);
        let mut cl_freq = [0u32; 19];
        for &(s, _) in &rle {
            cl_freq[s as usize] += 1;
        }
        let cl_len = build_lengths(&cl_freq, 7);
        let mut hclen = 19;
        while hclen > 4 && cl_len[CL_ORDER[hclen - 1]] == 0 {
            hclen -= 1;
        }
        let symbol_bits = |freq: &[u32], lens: &[u8]| -> u64 {
            freq.iter().zip(lens).map(|(&f, &l)| f as u64 * l as u64).sum()
        };
        let mut dyn_bits = 17 + 3 * hclen as u64 + extra_bits;
        for &(s, _) in &rle {
            dyn_bits += cl_len[s as usize] as u64 + cl_extra_bits(s) as u64;
        }
        dyn_bits += symbol_bits(&lit_freq, &lit_len) + symbol_bits(&dist_freq, &dist_len);

        // Fixed block cost.
        let fixed_lit = fixed_lit_lengths();
        let fixed_bits = 3 + extra_bits + symbol_bits(&lit_freq, &fixed_lit) + symbol_bits(&dist_freq, &[5u8; 30]);

        // Stored cost (an upper bound: header, worst-case padding and LEN/NLEN per 65535-byte chunk).
        let n = self.block_bytes as u64;
        let chunks = n.div_ceil(65535).max(1);
        let stored_bits = chunks * (3 + 7 + 32) + 8 * n;

        if stored_bits < dyn_bits.min(fixed_bits) {
            let bytes = &self.data[self.block_start..self.block_start + self.block_bytes];
            write_stored(&mut self.w, bytes, last);
        } else if fixed_bits <= dyn_bits {
            self.w.bits(last as u32, 1);
            self.w.bits(1, 2);
            let lc = canonical_codes(&fixed_lit);
            let dc = canonical_codes(&[5u8; 30]);
            self.write_tokens(&lc, &fixed_lit, &dc, &[5u8; 30]);
        } else {
            let w = &mut self.w;
            w.bits(last as u32, 1);
            w.bits(2, 2);
            w.bits((hlit - 257) as u32, 5);
            w.bits((hdist - 1) as u32, 5);
            w.bits((hclen - 4) as u32, 4);
            for &idx in CL_ORDER.iter().take(hclen) {
                w.bits(cl_len[idx] as u32, 3);
            }
            let cl_codes = canonical_codes(&cl_len);
            for &(s, e) in &rle {
                w.bits(cl_codes[s as usize] as u32, cl_len[s as usize] as u32);
                w.bits(e as u32, cl_extra_bits(s));
            }
            let lc = canonical_codes(&lit_len);
            let dc = canonical_codes(&dist_len);
            self.write_tokens(&lc, &lit_len, &dc, &dist_len);
        }
        self.block_start += self.block_bytes;
        self.block_bytes = 0;
        self.tokens.clear();
    }

    fn write_tokens(&mut self, lc: &[u16], ll: &[u8], dc: &[u16], dl: &[u8]) {
        let w = &mut self.w;
        for t in &self.tokens {
            if t.dist == 0 {
                let s = t.len as usize;
                w.bits(lc[s] as u32, ll[s] as u32);
            } else {
                let li = self.len_code[t.len as usize] as usize;
                let s = 257 + li;
                w.bits(lc[s] as u32, ll[s] as u32);
                w.bits((t.len - LEN_BASE[li]) as u32, LEN_EXTRA[li] as u32);
                let di = dist_code(t.dist);
                w.bits(dc[di] as u32, dl[di] as u32);
                w.bits((t.dist - DIST_BASE[di]) as u32, DIST_EXTRA[di] as u32);
            }
        }
        w.bits(lc[256] as u32, ll[256] as u32);
    }
}

/// Writes `bytes` as stored blocks of at most 65535 bytes; the last of them is final if `last`.
fn write_stored(w: &mut BitWriter, bytes: &[u8], last: bool) {
    let mut rest = bytes;
    loop {
        let k = rest.len().min(65535);
        let is_last_chunk = k == rest.len();
        w.bits((last && is_last_chunk) as u32, 1);
        w.bits(0, 2);
        w.align();
        w.bits(k as u32, 16);
        w.bits(!k as u32 & 0xFFFF, 16);
        w.out.extend_from_slice(&rest[..k]);
        rest = &rest[k..];
        if is_last_chunk {
            break;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Deflate: LZ77
// ---------------------------------------------------------------------------------------------------

const WSIZE: usize = 32768;
const WMASK: usize = WSIZE - 1;
const HASH_BITS: u32 = 15;
const NONE: usize = usize::MAX;
const MAX_MATCH: usize = 258;
/// Matches of length 3 farther back than this cost more than three literals.
const TOO_FAR: usize = 4096;

/// Search effort for one level: the hash chain walked per search, the length that ends a search
/// early, the length below which a lazy search at the next byte is tried (0 = greedy), and the length
/// above which that lazy search walks only a quarter of the chain.
struct Params {
    chain: usize,
    nice: usize,
    lazy: usize,
    good: usize,
}

fn params(level: u8) -> Params {
    let (chain, nice, lazy, good) = match level {
        1 => (4, 8, 0, 4),
        2 => (8, 16, 0, 4),
        3 => (32, 32, 0, 4),
        4 => (16, 16, 4, 4),
        5 => (32, 32, 16, 8),
        6 => (128, 128, 16, 8),
        7 => (256, 128, 32, 8),
        8 => (1024, 258, 128, 32),
        _ => (4096, 258, 258, 32),
    };
    Params { chain, nice, lazy, good }
}

/// Hash chains over the 3-byte prefixes of every inserted position.
struct Matcher {
    head: Vec<usize>,
    prev: Vec<usize>,
}

fn hash3(d: &[u8], i: usize) -> usize {
    let v = d[i] as u32 | (d[i + 1] as u32) << 8 | (d[i + 2] as u32) << 16;
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

impl Matcher {
    fn new() -> Self {
        Matcher { head: vec![NONE; 1 << HASH_BITS], prev: vec![NONE; WSIZE] }
    }

    fn insert(&mut self, d: &[u8], i: usize) {
        if i + 3 <= d.len() {
            let h = hash3(d, i);
            self.prev[i & WMASK] = self.head[h];
            self.head[h] = i;
        }
    }

    /// The longest match at `i` longer than `prev_len` (and at least 3), as `(len, dist)`, or `(0, 0)`.
    /// Must be called before `i` itself is inserted.
    fn find(&self, d: &[u8], i: usize, mut chain: usize, nice: usize, prev_len: usize) -> (usize, usize) {
        if i + 3 > d.len() {
            return (0, 0);
        }
        let max_len = MAX_MATCH.min(d.len() - i);
        let nice = nice.min(max_len);
        let mut best_len = prev_len.max(2);
        if best_len >= max_len {
            return (0, 0);
        }
        let mut best_dist = 0;
        let mut c = self.head[hash3(d, i)];
        while c != NONE && chain > 0 {
            if c >= i || i - c > WSIZE {
                break;
            }
            if d[c + best_len] == d[i + best_len] && d[c] == d[i] {
                let mut l = 0;
                while l < max_len && d[c + l] == d[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - c;
                    if l >= nice {
                        break;
                    }
                }
            }
            let n = self.prev[c & WMASK];
            if n == NONE || n >= c {
                break;
            }
            c = n;
            chain -= 1;
        }
        if best_dist == 0 || (best_len == 3 && best_dist > TOO_FAR) {
            (0, 0)
        } else {
            (best_len, best_dist)
        }
    }
}

/// Encodes `data` as a raw DEFLATE stream (no zlib header).
///
/// `level` 0 writes stored blocks; 1..=9 trade speed for size like zlib's levels (anything above 9
/// is treated as 9). Each block is written dynamic, fixed or stored, whichever is smallest.
pub fn deflate(data: &[u8], level: u8) -> Vec<u8> {
    if level == 0 {
        let mut w = BitWriter::new(data.len() + data.len() / 65535 * 5 + 8);
        write_stored(&mut w, data, true);
        return w.finish();
    }
    let p = params(level.min(9));
    let mut bw = BlockWriter::new(data);
    let mut m = Matcher::new();
    let n = data.len();
    let mut i = 0;
    // A match found at `i - 1`, held back while the lazy search looks at `i`.
    let mut pending: Option<(usize, usize)> = None;
    while i < n {
        let (len, dist) = match pending {
            Some((pl, _)) if pl >= p.nice => (0, 0),
            Some((pl, _)) => {
                let chain = if pl >= p.good { (p.chain >> 2).max(1) } else { p.chain };
                m.find(data, i, chain, p.nice, pl)
            }
            None => m.find(data, i, p.chain, p.nice, 0),
        };
        m.insert(data, i);
        match pending {
            Some((pl, pd)) => {
                if len > pl {
                    bw.literal(data[i - 1]);
                    pending = Some((len, dist));
                    i += 1;
                } else {
                    bw.matched(pl, pd);
                    let end = i - 1 + pl;
                    for j in i + 1..end {
                        m.insert(data, j);
                    }
                    i = end;
                    pending = None;
                }
            }
            None => {
                if len >= 3 {
                    if p.lazy > 0 && len < p.lazy {
                        pending = Some((len, dist));
                        i += 1;
                    } else {
                        bw.matched(len, dist);
                        for j in i + 1..i + len {
                            m.insert(data, j);
                        }
                        i += len;
                    }
                } else {
                    bw.literal(data[i]);
                    i += 1;
                }
            }
        }
    }
    if let Some((pl, pd)) = pending {
        bw.matched(pl, pd);
    }
    bw.flush(true);
    bw.w.finish()
}

// ---------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A small linear congruential generator, so tests need no outside randomness.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) as u32
        }
        fn bytes(&mut self, n: usize) -> Vec<u8> {
            (0..n).map(|_| self.next() as u8).collect()
        }
    }

    /// Text-like data: words from a small vocabulary, picked by the LCG.
    fn texty(n: usize, seed: u64) -> Vec<u8> {
        let words = [
            "the ", "quick ", "brown ", "fox ", "jumps ", "over ", "lazy ", "dog ", "scriptorium ",
            "ledger ", "stage ", "folio ", "scroll ", "and ", "a ", "page ", "of ", "ink ", "\n",
        ];
        let mut r = Lcg(seed);
        let mut out = Vec::with_capacity(n + 16);
        while out.len() < n {
            out.extend_from_slice(words[r.next() as usize % words.len()].as_bytes());
        }
        out.truncate(n);
        out
    }

    fn round_trip(data: &[u8]) {
        for level in 0..=9u8 {
            let c = deflate(data, level);
            let d = inflate(&c).unwrap_or_else(|e| panic!("level {level}, {} bytes: {e}", data.len()));
            assert!(d == data, "level {level} round trip differs ({} bytes)", data.len());
            let z = zlib_compress(data, level);
            assert_eq!(zlib_decompress(&z).unwrap(), data, "zlib level {level}");
        }
    }

    #[test]
    fn crc32_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        let mut c = Crc32::new();
        c.update(b"1234");
        c.update(b"");
        c.update(b"56789");
        assert_eq!(c.finish(), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }

    #[test]
    fn adler32_known_vector() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
        // Long input exercises the modulo chunking.
        let big = vec![0xFFu8; 100_000];
        let (mut a, mut b) = (1u64, 0u64);
        for &x in &big {
            a = (a + x as u64) % 65521;
            b = (b + a) % 65521;
        }
        assert_eq!(adler32(&big), ((b << 16) | a) as u32);
    }

    #[test]
    fn inflate_known_fixed_stream() {
        // zlib.compress(b"hello") without its header and trailer: one fixed-Huffman block.
        let raw = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x07, 0x00];
        assert_eq!(inflate(&raw).unwrap(), b"hello");
        let z = [0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x07, 0x00, 0x06, 0x2C, 0x02, 0x15];
        assert_eq!(zlib_decompress(&z).unwrap(), b"hello");
    }

    #[test]
    fn inflate_hand_written_fixed_block_with_match() {
        // Hand-assembled: BFINAL=1, BTYPE=01, literals 'a' 'b' 'c', length 6 (symbol 260) at distance 3
        // (distance code 2), end of block. Huffman codes are packed most-significant bit first.
        let mut bits: Vec<u8> = Vec::new();
        let push_lsb = |v: u32, n: u32, bits: &mut Vec<u8>| {
            for k in 0..n {
                bits.push(((v >> k) & 1) as u8);
            }
        };
        let push_code = |code: u32, n: u32, bits: &mut Vec<u8>| {
            for k in (0..n).rev() {
                bits.push(((code >> k) & 1) as u8);
            }
        };
        push_lsb(1, 1, &mut bits); // BFINAL
        push_lsb(1, 2, &mut bits); // BTYPE = fixed
        for &ch in b"abc" {
            push_code(0x30 + ch as u32, 8, &mut bits); // literals 0..=143: 8-bit codes from 0x30
        }
        push_code(260 - 256, 7, &mut bits); // length 6: symbol 260, 7-bit code 0000100
        push_code(2, 5, &mut bits); // distance 3: code 2, no extra bits
        push_code(0, 7, &mut bits); // end of block
        let mut bytes = vec![0u8; bits.len().div_ceil(8)];
        for (i, &b) in bits.iter().enumerate() {
            bytes[i / 8] |= b << (i % 8);
        }
        assert_eq!(inflate(&bytes).unwrap(), b"abcabcabc");
    }

    #[test]
    fn inflate_stored_block() {
        let raw = [0x01, 0x03, 0x00, 0xFC, 0xFF, b'x', b'y', b'z'];
        assert_eq!(inflate(&raw).unwrap(), b"xyz");
        let empty = [0x01, 0x00, 0x00, 0xFF, 0xFF];
        assert_eq!(inflate(&empty).unwrap(), b"");
    }

    #[test]
    fn round_trip_small_inputs() {
        round_trip(b"");
        round_trip(b"a");
        round_trip(b"ab");
        round_trip(b"abc");
        round_trip(b"abcabcabcabcabcabcabcabcabcabc");
    }

    #[test]
    fn round_trip_repeated() {
        round_trip(&vec![0u8; 100_000]);
        round_trip(&b"0123456789".repeat(5000));
        // Runs right at the match limits.
        round_trip(&vec![7u8; 258 * 3 + 2]);
    }

    #[test]
    fn round_trip_pseudo_random() {
        let mut r = Lcg(12345);
        round_trip(&r.bytes(70_000));
        // A small alphabet: many short matches and long hash chains.
        let small: Vec<u8> = (0..40_000).map(|_| b"ACGT"[r.next() as usize % 4]).collect();
        round_trip(&small);
    }

    #[test]
    fn round_trip_300_kib() {
        let n = 300 * 1024;
        let mut r = Lcg(99);
        let mut mixed = texty(n / 2, 7);
        mixed.extend_from_slice(&r.bytes(n / 4));
        mixed.extend(std::iter::repeat_n(b'z', n / 4));
        round_trip(&mixed);
    }

    #[test]
    fn dynamic_blocks_are_written_and_read() {
        let data = texty(50_000, 3);
        let c = deflate(&data, 6);
        assert_eq!((c[0] >> 1) & 3, 2, "expected the first block to be dynamic Huffman");
        assert!(c.len() < data.len() / 2, "text should compress well: {} -> {}", data.len(), c.len());
        assert_eq!(inflate(&c).unwrap(), data);
    }

    #[test]
    fn incompressible_data_falls_back_to_stored() {
        let data = Lcg(5).bytes(200_000);
        for level in 1..=9 {
            let c = deflate(&data, level);
            assert!(c.len() <= data.len() + data.len() / 1000 + 16, "level {level}: {}", c.len());
        }
    }

    #[test]
    fn higher_levels_are_not_worse_on_text() {
        let data = texty(120_000, 11);
        let l1 = deflate(&data, 1).len();
        let l9 = deflate(&data, 9).len();
        assert!(l9 <= l1, "level 9 ({l9}) larger than level 1 ({l1})");
    }

    #[test]
    fn code_lengths_respect_the_limit() {
        // Fibonacci frequencies force a deep Huffman tree.
        let mut freq = vec![0u32; 286];
        let (mut a, mut b) = (1u32, 1u32);
        for f in freq.iter_mut().take(30) {
            *f = a;
            let c = a.saturating_add(b);
            a = b;
            b = c;
        }
        let lens = build_lengths(&freq, 15);
        assert!(lens.iter().all(|&l| l <= 15));
        let kraft: u64 = lens.iter().filter(|&&l| l > 0).map(|&l| 1u64 << (15 - l)).sum();
        assert_eq!(kraft, 1 << 15);
        let cl = build_lengths(&freq[..19], 7);
        assert!(cl.iter().all(|&l| l <= 7));
        let kraft: u64 = cl.iter().filter(|&&l| l > 0).map(|&l| 1u64 << (7 - l)).sum();
        assert_eq!(kraft, 1 << 7);
    }

    #[test]
    fn truncated_input_is_an_error() {
        let data = texty(20_000, 1);
        for level in [0u8, 1, 6, 9] {
            let c = deflate(&data, level);
            for cut in [0, 1, 2, c.len() / 3, c.len() / 2, c.len() - 1] {
                assert!(inflate(&c[..cut]).is_err(), "level {level} cut {cut} should fail");
            }
        }
        assert!(inflate(&[]).is_err());
    }

    #[test]
    fn garbage_input_never_panics() {
        assert!(inflate(&[0xFF; 32]).is_err()); // block type 3
        assert!(inflate(&[0x01, 0x05, 0x00, 0x00, 0x00]).is_err()); // bad LEN/NLEN
        let mut r = Lcg(2024);
        for _ in 0..2000 {
            let n = (r.next() % 200) as usize;
            let g = r.bytes(n);
            let _ = inflate_limited(&g, 1 << 20);
            let _ = zlib_decompress_limited(&g, 1 << 20);
        }
        // Bit flips inside a valid stream: may decode to something else, must never panic.
        let good = deflate(&texty(5000, 4), 6);
        for k in 0..good.len() * 8 {
            let mut bad = good.clone();
            bad[k / 8] ^= 1 << (k % 8);
            let _ = inflate_limited(&bad, 1 << 20);
        }
    }

    #[test]
    fn output_limit_is_enforced() {
        let data = vec![1u8; 100_000];
        let c = deflate(&data, 6);
        assert!(inflate_limited(&c, 99_999).is_err());
        assert_eq!(inflate_limited(&c, 100_000).unwrap(), data);
        let s = deflate(&data, 0);
        assert!(inflate_limited(&s, 50_000).is_err());
    }

    #[test]
    fn zlib_header_and_trailer_are_checked() {
        let z = zlib_compress(b"hello hello hello", 6);
        assert_eq!(((z[0] as u16) << 8 | z[1] as u16) % 31, 0);
        let mut bad = z.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(zlib_decompress(&bad).is_err(), "bad Adler-32");
        assert!(zlib_decompress(&z[..z.len() - 2]).is_err(), "missing trailer");
        let mut bad_head = z.clone();
        bad_head[1] ^= 1;
        assert!(zlib_decompress(&bad_head).is_err(), "bad header check");
        assert!(zlib_decompress(&[0x78]).is_err());
    }
}
