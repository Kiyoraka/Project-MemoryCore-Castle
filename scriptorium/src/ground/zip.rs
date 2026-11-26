//! Zip: reading and writing ZIP archives (PKWARE APPNOTE 6.3), the container of DOCX, XLSX and PPTX.
//!
//! * [`ZipArchive::read`] parses the central directory: it finds the End of Central Directory record by
//!   scanning back from the end (aware of the archive comment), follows the ZIP64 locator and record
//!   when present, and reads ZIP64 extra fields. Archives with data prepended (self-extractors) are
//!   handled by re-basing the offsets. [`ZipArchive::data`] returns an entry's bytes (stored or deflate,
//!   decoded by our own [`flate`](super::flate)) after checking its size and CRC-32.
//! * [`ZipWriter`] writes archives: [`ZipWriter::add`] compresses new data, [`ZipWriter::add_raw`] copies
//!   an entry read from another archive with its compressed bytes untouched (so parts we do not
//!   understand keep their exact bytes). ZIP64 records are written automatically when sizes, offsets or
//!   the entry count exceed the classic limits (or always, with [`ZipWriter::force_zip64`]). Unknown
//!   extra fields are passed through; only the ZIP64 field is rebuilt.
//!
//! Not supported: encryption, multi-disk (split) archives, and compression methods other than stored
//! and deflate (such entries can still be listed and copied with `add_raw`).

use super::flate;
use std::fmt;

// ---------------------------------------------------------------------------------------------------
// Errors and constants
// ---------------------------------------------------------------------------------------------------

/// An error from reading or writing an archive. The message says what was wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipError(pub String);

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "zip: {}", self.0)
    }
}

impl std::error::Error for ZipError {}

impl From<flate::FlateError> for ZipError {
    fn from(e: flate::FlateError) -> Self {
        ZipError(e.to_string())
    }
}

fn fail<T>(msg: &str) -> Result<T, ZipError> {
    Err(ZipError(msg.to_string()))
}

const SIG_LOCAL: u32 = 0x0403_4b50;
const SIG_CENTRAL: u32 = 0x0201_4b50;
const SIG_EOCD: u32 = 0x0605_4b50;
const SIG_EOCD64: u32 = 0x0606_4b50;
const SIG_LOCATOR64: u32 = 0x0706_4b50;
const SIG_DESCRIPTOR: u32 = 0x0807_4b50;

/// Compression method number: stored (no compression).
pub const METHOD_STORED: u16 = 0;
/// Compression method number: deflate.
pub const METHOD_DEFLATED: u16 = 8;

/// General purpose flag bit 0: the entry is encrypted.
pub const FLAG_ENCRYPTED: u16 = 1;
/// General purpose flag bit 3: CRC and sizes follow the data in a data descriptor.
pub const FLAG_DATA_DESCRIPTOR: u16 = 1 << 3;
/// General purpose flag bit 11: the name and comment are UTF-8 (otherwise CP437).
pub const FLAG_UTF8: u16 = 1 << 11;

const ZIP64_EXTRA_ID: u16 = 0x0001;
const MAX32: u64 = 0xFFFF_FFFF;
const MAX16: u64 = 0xFFFF;

// ---------------------------------------------------------------------------------------------------
// Text: CP437 and UTF-8
// ---------------------------------------------------------------------------------------------------

/// Code page 437, bytes 0x80..=0xFF (bytes below 0x80 are ASCII).
const CP437_HIGH: [char; 128] = [
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}', '\u{00E7}',
    '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}', '\u{00EE}', '\u{00EC}', '\u{00C4}', '\u{00C5}',
    '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}', '\u{00F2}', '\u{00FB}', '\u{00F9}',
    '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00A2}', '\u{00A3}', '\u{00A5}', '\u{20A7}', '\u{0192}',
    '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}', '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}',
    '\u{00BF}', '\u{2310}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
    '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255D}', '\u{255C}', '\u{255B}', '\u{2510}',
    '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{255E}', '\u{255F}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{2567}',
    '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}', '\u{2552}', '\u{2553}', '\u{256B}',
    '\u{256A}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{258C}', '\u{2590}', '\u{2580}',
    '\u{03B1}', '\u{00DF}', '\u{0393}', '\u{03C0}', '\u{03A3}', '\u{03C3}', '\u{00B5}', '\u{03C4}',
    '\u{03A6}', '\u{0398}', '\u{03A9}', '\u{03B4}', '\u{221E}', '\u{03C6}', '\u{03B5}', '\u{2229}',
    '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00F7}', '\u{2248}',
    '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}', '\u{207F}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
];

/// Decodes CP437 bytes (ASCII below 0x80).
pub fn cp437_decode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if b < 0x80 { b as char } else { CP437_HIGH[(b - 0x80) as usize] })
        .collect()
}

/// Encodes text as CP437, or `None` when a character has no CP437 byte.
pub fn cp437_encode(text: &str) -> Option<Vec<u8>> {
    text.chars()
        .map(|c| {
            if (c as u32) < 0x80 {
                Some(c as u8)
            } else {
                CP437_HIGH.iter().position(|&h| h == c).map(|i| 0x80 + i as u8)
            }
        })
        .collect()
}

/// Decodes an entry name or comment: UTF-8 when flag bit 11 is set, CP437 otherwise.
fn decode_text(bytes: &[u8], flags: u16) -> String {
    if flags & FLAG_UTF8 != 0 {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        cp437_decode(bytes)
    }
}

/// Encodes an entry's text under `flags`: UTF-8 bytes when bit 11 is set; otherwise CP437, falling back
/// to `?` for characters CP437 lacks.
fn encode_text(text: &str, flags: u16) -> Vec<u8> {
    if flags & FLAG_UTF8 != 0 {
        return text.as_bytes().to_vec();
    }
    cp437_encode(text).unwrap_or_else(|| {
        text.chars().map(|c| cp437_encode(&c.to_string()).map_or(b'?', |v| v[0])).collect()
    })
}

// ---------------------------------------------------------------------------------------------------
// Entries
// ---------------------------------------------------------------------------------------------------

/// One entry of the central directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ZipEntry {
    /// The name, decoded (UTF-8 when [`FLAG_UTF8`] is set, CP437 otherwise). Directories end in `/`.
    pub name: String,
    /// The name exactly as stored. [`ZipWriter::add_raw`] writes these bytes back when they still decode
    /// to `name`; clear it (or change `name`) to have the name re-encoded.
    pub raw_name: Vec<u8>,
    /// Compression method ([`METHOD_STORED`], [`METHOD_DEFLATED`], or another number).
    pub method: u16,
    /// CRC-32 of the uncompressed data.
    pub crc32: u32,
    /// Size of the stored (compressed) data.
    pub compressed_size: u64,
    /// Size of the uncompressed data.
    pub size: u64,
    /// Offset of the local header from the start of the archive bytes.
    pub local_offset: u64,
    /// The central directory extra field, every field kept as found (including any ZIP64 field).
    pub extra: Vec<u8>,
    /// The entry comment, decoded like the name.
    pub comment: String,
    /// General purpose bit flags.
    pub flags: u16,
    /// Last modification as MS-DOS `(time, date)`.
    pub mtime_dos: (u16, u16),
    /// External file attributes (0x10 = MS-DOS directory; Unix mode in the high 16 bits).
    pub external_attrs: u32,
    /// Internal file attributes (bit 0 = text).
    pub internal_attrs: u16,
    /// "Version made by" (host system in the high byte).
    pub version_made_by: u16,
    /// "Version needed to extract".
    pub version_needed: u16,
}

impl ZipEntry {
    /// True for a directory entry (its name ends in `/`).
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
}

/// Iterates the `(id, data)` fields of an extra block. Stops at the first malformed field.
fn extra_fields(extra: &[u8]) -> Vec<(u16, &[u8])> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 4 <= extra.len() {
        let id = u16::from_le_bytes([extra[p], extra[p + 1]]);
        let len = u16::from_le_bytes([extra[p + 2], extra[p + 3]]) as usize;
        if p + 4 + len > extra.len() {
            break;
        }
        out.push((id, &extra[p + 4..p + 4 + len]));
        p += 4 + len;
    }
    out
}

/// The extra block without its ZIP64 field (everything else byte for byte, including any trailing
/// bytes that do not form a whole field).
fn strip_zip64(extra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(extra.len());
    let mut p = 0;
    while p + 4 <= extra.len() {
        let id = u16::from_le_bytes([extra[p], extra[p + 1]]);
        let len = u16::from_le_bytes([extra[p + 2], extra[p + 3]]) as usize;
        if p + 4 + len > extra.len() {
            break;
        }
        if id != ZIP64_EXTRA_ID {
            out.extend_from_slice(&extra[p..p + 4 + len]);
        }
        p += 4 + len;
    }
    out.extend_from_slice(&extra[p..]);
    out
}

// ---------------------------------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------------------------------

/// A bounds-checked little-endian cursor.
struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn at(b: &'a [u8], p: usize) -> Self {
        Cur { b, p }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ZipError> {
        match self.p.checked_add(n) {
            Some(end) if end <= self.b.len() => {
                let s = &self.b[self.p..end];
                self.p = end;
                Ok(s)
            }
            _ => fail("truncated archive"),
        }
    }

    fn u16(&mut self) -> Result<u16, ZipError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Result<u32, ZipError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn u64(&mut self) -> Result<u64, ZipError> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_le_bytes(a))
    }
}

fn sig_at(bytes: &[u8], pos: u64) -> Option<u32> {
    let p = usize::try_from(pos).ok()?;
    let s = bytes.get(p..p.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn to_usize(v: u64) -> Result<usize, ZipError> {
    usize::try_from(v).map_err(|_| ZipError("offset or size too large for this platform".into()))
}

/// Finds the End of Central Directory record: the last signature whose comment length reaches exactly
/// the end of the bytes; failing that, the last one whose comment fits (trailing junk tolerated).
fn find_eocd(bytes: &[u8]) -> Result<usize, ZipError> {
    if bytes.len() < 22 {
        return fail("too small to be a zip archive");
    }
    let last = bytes.len() - 22;
    let first = last.saturating_sub(0xFFFF);
    let mut loose = None;
    for p in (first..=last).rev() {
        if sig_at(bytes, p as u64) != Some(SIG_EOCD) {
            continue;
        }
        let clen = u16::from_le_bytes([bytes[p + 20], bytes[p + 21]]) as usize;
        if p + 22 + clen == bytes.len() {
            return Ok(p);
        }
        if loose.is_none() && p + 22 + clen <= bytes.len() {
            loose = Some(p);
        }
    }
    loose.ok_or_else(|| ZipError("end of central directory record not found".into()))
}

/// A parsed archive: the central directory's entries and the archive comment. The archive bytes are
/// not kept; pass them again to [`ZipArchive::data`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ZipArchive {
    /// Entries in central directory order.
    pub entries: Vec<ZipEntry>,
    /// The archive comment (UTF-8 when valid, CP437 otherwise).
    pub comment: String,
}

impl ZipArchive {
    /// Parses the central directory of `bytes`.
    pub fn read(bytes: &[u8]) -> Result<Self, ZipError> {
        let eocd = find_eocd(bytes)?;
        let mut c = Cur::at(bytes, eocd + 4);
        let _disk = c.u16()?;
        let _cd_disk = c.u16()?;
        let _entries_on_disk = c.u16()?;
        let mut total = c.u16()? as u64;
        let mut cd_size = c.u32()? as u64;
        let mut cd_offset = c.u32()? as u64;
        let clen = c.u16()? as usize;
        let comment_bytes = &bytes[eocd + 22..(eocd + 22 + clen).min(bytes.len())];
        let comment = match std::str::from_utf8(comment_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => cp437_decode(comment_bytes),
        };

        // Where the central directory must end: the EOCD, or the ZIP64 record before it.
        let mut cd_end = eocd as u64;
        if eocd >= 20 && sig_at(bytes, (eocd - 20) as u64) == Some(SIG_LOCATOR64) {
            let mut l = Cur::at(bytes, eocd - 16);
            let rec_offset = l.u64()?;
            // The record normally sits right before the locator (56 bytes without extensible data);
            // look there too when the stated offset is off (data prepended to the archive).
            let rec = if sig_at(bytes, rec_offset) == Some(SIG_EOCD64) {
                rec_offset
            } else if eocd >= 76 && sig_at(bytes, (eocd - 76) as u64) == Some(SIG_EOCD64) {
                (eocd - 76) as u64
            } else {
                return fail("ZIP64 end of central directory record not found");
            };
            let mut r = Cur::at(bytes, to_usize(rec)? + 4);
            let _size = r.u64()?;
            let _made = r.u16()?;
            let _needed = r.u16()?;
            let _disk = r.u32()?;
            let _cd_disk = r.u32()?;
            let _on_disk = r.u64()?;
            total = r.u64()?;
            cd_size = r.u64()?;
            cd_offset = r.u64()?;
            cd_end = rec;
        }

        let cd_start = cd_end
            .checked_sub(cd_size)
            .ok_or_else(|| ZipError("central directory size larger than the archive".into()))?;
        // Data prepended to the archive shifts every offset by the same amount.
        let shift = if cd_start > cd_offset
            && total > 0
            && sig_at(bytes, cd_offset) != Some(SIG_CENTRAL)
            && sig_at(bytes, cd_start) == Some(SIG_CENTRAL)
        {
            cd_start - cd_offset
        } else if cd_offset.checked_add(cd_size).is_none_or(|e| e > cd_end) {
            return fail("central directory lies outside the archive");
        } else {
            0
        };

        let mut entries = Vec::with_capacity(total.min(cd_size / 46) as usize);
        let mut c = Cur::at(bytes, to_usize(cd_offset + shift)?);
        for _ in 0..total {
            if c.u32()? != SIG_CENTRAL {
                return fail("bad central directory header signature");
            }
            let version_made_by = c.u16()?;
            let version_needed = c.u16()?;
            let flags = c.u16()?;
            let method = c.u16()?;
            let time = c.u16()?;
            let date = c.u16()?;
            let crc32 = c.u32()?;
            let mut compressed_size = c.u32()? as u64;
            let mut size = c.u32()? as u64;
            let name_len = c.u16()? as usize;
            let extra_len = c.u16()? as usize;
            let comment_len = c.u16()? as usize;
            let disk = c.u16()?;
            let internal_attrs = c.u16()?;
            let external_attrs = c.u32()?;
            let mut local_offset = c.u32()? as u64;
            let raw_name = c.take(name_len)?.to_vec();
            let extra = c.take(extra_len)?.to_vec();
            let comment = decode_text(c.take(comment_len)?, flags);

            let (need_size, need_csize, need_off) =
                (size == MAX32, compressed_size == MAX32, local_offset == MAX32);
            if need_size || need_csize || need_off || disk as u64 == MAX16 {
                if let Some((_, d)) = extra_fields(&extra).into_iter().find(|(id, _)| *id == ZIP64_EXTRA_ID) {
                    let mut z = Cur::at(d, 0);
                    let short = |_| ZipError("ZIP64 extra field too short".into());
                    if need_size {
                        size = z.u64().map_err(short)?;
                    }
                    if need_csize {
                        compressed_size = z.u64().map_err(short)?;
                    }
                    if need_off {
                        local_offset = z.u64().map_err(short)?;
                    }
                }
            }
            let local_offset = local_offset
                .checked_add(shift)
                .ok_or_else(|| ZipError("local header offset overflows".into()))?;
            entries.push(ZipEntry {
                name: decode_text(&raw_name, flags),
                raw_name,
                method,
                crc32,
                compressed_size,
                size,
                local_offset,
                extra,
                comment,
                flags,
                mtime_dos: (time, date),
                external_attrs,
                internal_attrs,
                version_made_by,
                version_needed,
            });
        }
        Ok(ZipArchive { entries, comment })
    }

    /// The first entry named `name`.
    pub fn by_name(&self, name: &str) -> Option<&ZipEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// The entry's stored bytes, exactly as in the archive (still compressed), located through its local
    /// header. Sizes come from the central directory, so entries with a data descriptor work too.
    pub fn raw_data<'a>(&self, bytes: &'a [u8], entry: &ZipEntry) -> Result<&'a [u8], ZipError> {
        let mut c = Cur::at(bytes, to_usize(entry.local_offset)?);
        if c.u32()? != SIG_LOCAL {
            return fail("bad local header signature");
        }
        c.take(22)?; // version, flags, method, time, date, crc, sizes (the central directory's win)
        let name_len = c.u16()? as usize;
        let extra_len = c.u16()? as usize;
        c.take(name_len + extra_len)?;
        c.take(to_usize(entry.compressed_size)?)
    }

    /// The entry's uncompressed bytes. Stored and deflate entries are supported; the size and CRC-32 are
    /// checked, and inflation stops at the declared size (no decompression bombs).
    pub fn data(&self, bytes: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, ZipError> {
        if entry.flags & FLAG_ENCRYPTED != 0 {
            return fail("encrypted entries are not supported");
        }
        let raw = self.raw_data(bytes, entry)?;
        let out = if raw.is_empty() && entry.size == 0 {
            Vec::new()
        } else {
            match entry.method {
                METHOD_STORED => raw.to_vec(),
                METHOD_DEFLATED => flate::inflate_limited(raw, to_usize(entry.size)?)?,
                m => return Err(ZipError(format!("unsupported compression method {m}"))),
            }
        };
        if out.len() as u64 != entry.size {
            return fail("uncompressed size does not match the central directory");
        }
        if flate::crc32(&out) != entry.crc32 {
            return fail("CRC-32 mismatch");
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------------------------------

/// How [`ZipWriter::add`] stores data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// No compression.
    Stored,
    /// Deflate at the given level (0..=9). When deflate would not make the data smaller, the entry is
    /// stored instead.
    Deflated(u8),
}

/// Writes an archive into memory. Add entries, then call [`ZipWriter::finish`].
#[derive(Debug, Clone)]
pub struct ZipWriter {
    out: Vec<u8>,
    central: Vec<ZipEntry>,
    comment: Vec<u8>,
    /// Write ZIP64 fields and records even when nothing needs them (testing and interop checks).
    pub force_zip64: bool,
    /// MS-DOS `(time, date)` given to entries added with [`ZipWriter::add`]. Defaults to 1980-01-01 00:00.
    pub mtime_dos: (u16, u16),
}

impl Default for ZipWriter {
    fn default() -> Self {
        ZipWriter::new()
    }
}

fn put16(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put64(o: &mut Vec<u8>, v: u64) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn clamp32(v: u64, zip64: bool) -> u32 {
    if zip64 { MAX32 as u32 } else { v as u32 }
}

impl ZipWriter {
    /// An empty archive.
    pub fn new() -> Self {
        ZipWriter {
            out: Vec::new(),
            central: Vec::new(),
            comment: Vec::new(),
            force_zip64: false,
            mtime_dos: (0, (1 << 5) | 1),
        }
    }

    /// Sets the archive comment (UTF-8, cut to at most 65535 bytes at a character boundary).
    pub fn set_comment(&mut self, comment: &str) {
        let mut end = comment.len().min(0xFFFF);
        while !comment.is_char_boundary(end) {
            end -= 1;
        }
        self.comment = comment.as_bytes()[..end].to_vec();
    }

    /// Adds a new entry. A name ending in `/` makes a directory entry (pass empty data). Non-ASCII names
    /// are written as UTF-8 with flag bit 11.
    pub fn add(&mut self, name: &str, data: &[u8], method: Method) -> Result<(), ZipError> {
        let deflated = match method {
            Method::Deflated(level) if !data.is_empty() => {
                let c = flate::deflate(data, level);
                if c.len() < data.len() { Some(c) } else { None }
            }
            _ => None,
        };
        let (code, payload): (u16, &[u8]) = match &deflated {
            Some(c) => (METHOD_DEFLATED, c),
            None => (METHOD_STORED, data),
        };
        let is_dir = name.ends_with('/');
        let flags = if name.is_ascii() { 0 } else { FLAG_UTF8 };
        let entry = ZipEntry {
            name: name.to_string(),
            raw_name: name.as_bytes().to_vec(),
            method: code,
            crc32: flate::crc32(data),
            compressed_size: payload.len() as u64,
            size: data.len() as u64,
            local_offset: 0,
            extra: Vec::new(),
            comment: String::new(),
            flags,
            mtime_dos: self.mtime_dos,
            external_attrs: if is_dir { 0x10 } else { 0 },
            internal_attrs: 0,
            version_made_by: 20,
            version_needed: if code == METHOD_DEFLATED || is_dir { 20 } else { 10 },
        };
        self.write_entry(entry, payload)
    }

    /// Copies an entry read from another archive with its stored bytes `raw` (from
    /// [`ZipArchive::raw_data`]) untouched: method, CRC, size, flags, times, attributes, comment and
    /// extra fields are kept; the compressed size becomes `raw.len()`. If the entry used a data
    /// descriptor, one is written after the data again.
    pub fn add_raw(&mut self, entry: &ZipEntry, raw: &[u8]) -> Result<(), ZipError> {
        let mut e = entry.clone();
        e.compressed_size = raw.len() as u64;
        if e.raw_name.is_empty() || decode_text(&e.raw_name, e.flags) != e.name {
            if e.flags & FLAG_UTF8 == 0 && cp437_encode(&e.name).is_none() {
                e.flags |= FLAG_UTF8;
            }
            e.raw_name = encode_text(&e.name, e.flags);
        }
        self.write_entry(e, raw)
    }

    fn write_entry(&mut self, mut e: ZipEntry, payload: &[u8]) -> Result<(), ZipError> {
        let base_extra = strip_zip64(&e.extra);
        if e.raw_name.len() > 0xFFFF {
            return fail("entry name longer than 65535 bytes");
        }
        // The central extra may gain a 28-byte ZIP64 field.
        if base_extra.len() + 28 > 0xFFFF {
            return fail("extra field too long");
        }
        if encode_text(&e.comment, e.flags).len() > 0xFFFF {
            return fail("entry comment longer than 65535 bytes");
        }
        e.local_offset = self.out.len() as u64;
        let descriptor = e.flags & FLAG_DATA_DESCRIPTOR != 0;
        let zip64 = self.force_zip64 || e.size >= MAX32 || e.compressed_size >= MAX32;
        if zip64 || e.local_offset >= MAX32 {
            e.version_needed = e.version_needed.max(45);
        }

        let mut extra = Vec::new();
        if zip64 {
            put16(&mut extra, ZIP64_EXTRA_ID);
            put16(&mut extra, 16);
            let (s, cs) = if descriptor { (0, 0) } else { (e.size, e.compressed_size) };
            put64(&mut extra, s);
            put64(&mut extra, cs);
        }
        extra.extend_from_slice(&base_extra);

        let o = &mut self.out;
        put32(o, SIG_LOCAL);
        put16(o, e.version_needed);
        put16(o, e.flags);
        put16(o, e.method);
        put16(o, e.mtime_dos.0);
        put16(o, e.mtime_dos.1);
        if descriptor {
            put32(o, 0);
            put32(o, if zip64 { MAX32 as u32 } else { 0 });
            put32(o, if zip64 { MAX32 as u32 } else { 0 });
        } else {
            put32(o, e.crc32);
            put32(o, clamp32(e.compressed_size, zip64));
            put32(o, clamp32(e.size, zip64));
        }
        put16(o, e.raw_name.len() as u16);
        put16(o, extra.len() as u16);
        o.extend_from_slice(&e.raw_name);
        o.extend_from_slice(&extra);
        o.extend_from_slice(payload);
        if descriptor {
            put32(o, SIG_DESCRIPTOR);
            put32(o, e.crc32);
            if zip64 {
                put64(o, e.compressed_size);
                put64(o, e.size);
            } else {
                put32(o, e.compressed_size as u32);
                put32(o, e.size as u32);
            }
        }
        self.central.push(e);
        Ok(())
    }

    /// Writes the central directory and end records and returns the archive bytes.
    pub fn finish(self) -> Vec<u8> {
        let ZipWriter { mut out, central, comment, force_zip64, .. } = self;
        let cd_start = out.len() as u64;
        for e in &central {
            let z_size = force_zip64 || e.size >= MAX32;
            let z_csize = force_zip64 || e.compressed_size >= MAX32;
            let z_off = force_zip64 || e.local_offset >= MAX32;
            let mut extra = Vec::new();
            if z_size || z_csize || z_off {
                let mut d = Vec::new();
                if z_size {
                    put64(&mut d, e.size);
                }
                if z_csize {
                    put64(&mut d, e.compressed_size);
                }
                if z_off {
                    put64(&mut d, e.local_offset);
                }
                put16(&mut extra, ZIP64_EXTRA_ID);
                put16(&mut extra, d.len() as u16);
                extra.extend_from_slice(&d);
            }
            extra.extend_from_slice(&strip_zip64(&e.extra));
            let any64 = z_size || z_csize || z_off;
            let made_by = if any64 && (e.version_made_by & 0xFF) < 45 {
                (e.version_made_by & 0xFF00) | 45
            } else {
                e.version_made_by
            };
            let needed = if any64 { e.version_needed.max(45) } else { e.version_needed };
            let comment_bytes = encode_text(&e.comment, e.flags);

            put32(&mut out, SIG_CENTRAL);
            put16(&mut out, made_by);
            put16(&mut out, needed);
            put16(&mut out, e.flags);
            put16(&mut out, e.method);
            put16(&mut out, e.mtime_dos.0);
            put16(&mut out, e.mtime_dos.1);
            put32(&mut out, e.crc32);
            put32(&mut out, clamp32(e.compressed_size, z_csize));
            put32(&mut out, clamp32(e.size, z_size));
            put16(&mut out, e.raw_name.len() as u16);
            put16(&mut out, extra.len() as u16);
            put16(&mut out, comment_bytes.len() as u16);
            put16(&mut out, 0); // disk number start
            put16(&mut out, e.internal_attrs);
            put32(&mut out, e.external_attrs);
            put32(&mut out, clamp32(e.local_offset, z_off));
            out.extend_from_slice(&e.raw_name);
            out.extend_from_slice(&extra);
            out.extend_from_slice(&comment_bytes);
        }
        let cd_size = out.len() as u64 - cd_start;
        let count = central.len() as u64;
        let z_count = force_zip64 || count >= MAX16;
        let z_size = force_zip64 || cd_size >= MAX32;
        let z_start = force_zip64 || cd_start >= MAX32;
        if z_count || z_size || z_start {
            let rec = out.len() as u64;
            put32(&mut out, SIG_EOCD64);
            put64(&mut out, 44); // size of the rest of the record
            put16(&mut out, 45);
            put16(&mut out, 45);
            put32(&mut out, 0);
            put32(&mut out, 0);
            put64(&mut out, count);
            put64(&mut out, count);
            put64(&mut out, cd_size);
            put64(&mut out, cd_start);
            put32(&mut out, SIG_LOCATOR64);
            put32(&mut out, 0);
            put64(&mut out, rec);
            put32(&mut out, 1);
        }
        put32(&mut out, SIG_EOCD);
        put16(&mut out, 0);
        put16(&mut out, 0);
        let count16 = if z_count { MAX16 as u16 } else { count as u16 };
        put16(&mut out, count16);
        put16(&mut out, count16);
        put32(&mut out, clamp32(cd_size, z_size));
        put32(&mut out, clamp32(cd_start, z_start));
        put16(&mut out, comment.len() as u16);
        out.extend_from_slice(&comment);
        out
    }
}

// ---------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn contents(i: usize) -> Vec<u8> {
        match i % 4 {
            0 => format!("entry {i} ").repeat(i + 1).into_bytes(),
            1 => (0..i * 37).map(|k| (k * 7 + i) as u8).collect(),
            2 => Vec::new(),
            _ => vec![b'x'; i * 100],
        }
    }

    fn find_sig(bytes: &[u8], sig: u32) -> bool {
        bytes.windows(4).any(|w| w == sig.to_le_bytes())
    }

    #[test]
    fn write_then_read_many_entries() {
        let mut w = ZipWriter::new();
        for i in 0..60 {
            let method = if i % 3 == 0 { Method::Stored } else { Method::Deflated((i % 10) as u8) };
            w.add(&format!("dir{}/file{i}.txt", i % 5), &contents(i), method).unwrap();
        }
        w.add("empty.bin", b"", Method::Deflated(6)).unwrap();
        w.add("folder/", b"", Method::Stored).unwrap();
        w.add("日本語/ファイル.txt", "こんにちは".as_bytes(), Method::Deflated(9)).unwrap();
        w.add("café.xml", b"<a/>", Method::Stored).unwrap();
        let bytes = w.finish();

        let a = ZipArchive::read(&bytes).unwrap();
        assert_eq!(a.entries.len(), 64);
        for i in 0..60 {
            let e = &a.entries[i];
            assert_eq!(e.name, format!("dir{}/file{i}.txt", i % 5));
            assert_eq!(a.data(&bytes, e).unwrap(), contents(i), "entry {i}");
            assert!(!e.is_dir());
        }
        let deflated = a.entries.iter().filter(|e| e.method == METHOD_DEFLATED).count();
        assert!(deflated > 10, "expected deflated entries, got {deflated}");
        let empty = a.by_name("empty.bin").unwrap();
        assert_eq!((empty.size, empty.method), (0, METHOD_STORED));
        assert_eq!(a.data(&bytes, empty).unwrap(), b"");
        let dir = a.by_name("folder/").unwrap();
        assert!(dir.is_dir());
        assert_eq!(dir.external_attrs & 0x10, 0x10);
        let jp = a.by_name("日本語/ファイル.txt").unwrap();
        assert_ne!(jp.flags & FLAG_UTF8, 0);
        assert_eq!(a.data(&bytes, jp).unwrap(), "こんにちは".as_bytes());
        assert_eq!(a.data(&bytes, a.by_name("café.xml").unwrap()).unwrap(), b"<a/>");
        assert!(!find_sig(&bytes, SIG_EOCD64));
        assert_eq!(a.comment, "");
    }

    #[test]
    fn empty_archive() {
        let bytes = ZipWriter::new().finish();
        assert_eq!(bytes.len(), 22);
        let a = ZipArchive::read(&bytes).unwrap();
        assert!(a.entries.is_empty());
    }

    #[test]
    fn add_raw_preserves_bytes_exactly() {
        let mut w = ZipWriter::new();
        w.add("a.txt", &b"alpha ".repeat(500), Method::Deflated(6)).unwrap();
        w.add("b.bin", &contents(5), Method::Stored).unwrap();
        let src = w.finish();
        let a = ZipArchive::read(&src).unwrap();

        // An entry with an unknown method, an unknown extra field, a comment, a data descriptor and a
        // CP437 name: copied as-is, even though we cannot decode it.
        let mut odd = ZipEntry {
            name: "Çü.dat".to_string(),
            raw_name: vec![0x80, 0x81, b'.', b'd', b'a', b't'],
            method: 99,
            crc32: 0xDEAD_BEEF,
            size: 1234,
            extra: vec![0xFE, 0xCA, 3, 0, 1, 2, 3],
            comment: "a note".to_string(),
            flags: FLAG_DATA_DESCRIPTOR,
            mtime_dos: (0x6000, 0x5A21),
            external_attrs: 0x81A4_0000,
            version_made_by: 0x0314,
            version_needed: 63,
            ..ZipEntry::default()
        };
        let odd_raw = [9u8, 8, 7, 6, 5, 4, 3, 2, 1];

        let mut w2 = ZipWriter::new();
        for e in &a.entries {
            w2.add_raw(e, a.raw_data(&src, e).unwrap()).unwrap();
        }
        w2.add_raw(&odd, &odd_raw).unwrap();
        let copy = w2.finish();
        let b = ZipArchive::read(&copy).unwrap();
        assert_eq!(b.entries.len(), 3);
        for (x, y) in a.entries.iter().zip(&b.entries) {
            assert_eq!(a.raw_data(&src, x).unwrap(), b.raw_data(&copy, y).unwrap());
            assert_eq!((x.crc32, x.size, x.method, &x.raw_name), (y.crc32, y.size, y.method, &y.raw_name));
            assert_eq!(b.data(&copy, y).unwrap(), a.data(&src, x).unwrap());
        }
        let got = &b.entries[2];
        assert_eq!(b.raw_data(&copy, got).unwrap(), odd_raw);
        assert_eq!(got.name, "Çü.dat");
        assert_eq!(got.raw_name, odd.raw_name);
        assert_eq!(got.extra, odd.extra);
        assert_eq!(got.comment, "a note");
        assert_eq!(got.flags, FLAG_DATA_DESCRIPTOR);
        assert_eq!((got.mtime_dos, got.external_attrs, got.version_made_by), (odd.mtime_dos, odd.external_attrs, 0x0314));
        assert_eq!((got.crc32, got.size, got.compressed_size), (0xDEAD_BEEF, 1234, 9));
        assert!(b.data(&copy, got).unwrap_err().0.contains("unsupported compression method"));
        assert!(find_sig(&copy, SIG_DESCRIPTOR));

        // Renaming re-encodes the name.
        odd.name = "renamed €.dat".to_string();
        let mut w3 = ZipWriter::new();
        w3.add_raw(&odd, &odd_raw).unwrap();
        let c3 = w3.finish();
        let e3 = &ZipArchive::read(&c3).unwrap().entries[0];
        assert_eq!(e3.name, "renamed €.dat");
        assert_ne!(e3.flags & FLAG_UTF8, 0);
    }

    #[test]
    fn cp437_names_decode() {
        assert_eq!(cp437_decode(&[b'A', 0x80, 0xE1, 0xFF]), "AÇß\u{00A0}");
        let all: Vec<u8> = (0..=255u8).collect();
        assert_eq!(cp437_encode(&cp437_decode(&all)).unwrap(), all);
        assert!(cp437_encode("日").is_none());
    }

    #[test]
    fn archive_comment_round_trips_even_with_a_fake_signature() {
        let mut w = ZipWriter::new();
        w.add("x.txt", b"hello", Method::Stored).unwrap();
        let tricky = "comment PK\u{5}\u{6} with a signature inside, and more text";
        w.set_comment(tricky);
        let bytes = w.finish();
        let a = ZipArchive::read(&bytes).unwrap();
        assert_eq!(a.comment, tricky);
        assert_eq!(a.data(&bytes, &a.entries[0]).unwrap(), b"hello");

        let mut w = ZipWriter::new();
        w.set_comment(&"é".repeat(40_000)); // 80000 bytes, cut at a boundary
        let bytes = w.finish();
        let a = ZipArchive::read(&bytes).unwrap();
        assert_eq!(a.comment.len(), 65534);
    }

    #[test]
    fn forced_zip64_round_trips() {
        let mut w = ZipWriter::new();
        w.force_zip64 = true;
        w.add("one.txt", &b"zip64 ".repeat(100), Method::Deflated(6)).unwrap();
        w.add("two/", b"", Method::Stored).unwrap();
        w.add("three.bin", &contents(9), Method::Stored).unwrap();
        let bytes = w.finish();
        assert!(find_sig(&bytes, SIG_EOCD64));
        assert!(find_sig(&bytes, SIG_LOCATOR64));
        let a = ZipArchive::read(&bytes).unwrap();
        assert_eq!(a.entries.len(), 3);
        assert_eq!(a.data(&bytes, &a.entries[0]).unwrap(), b"zip64 ".repeat(100));
        assert_eq!(a.data(&bytes, &a.entries[2]).unwrap(), contents(9));
        let e = &a.entries[2];
        assert!(extra_fields(&e.extra).iter().any(|(id, _)| *id == ZIP64_EXTRA_ID));
        assert!(e.version_needed >= 45);
        assert!(e.local_offset > 0);

        // Copying without the flag drops back to classic records.
        let mut w2 = ZipWriter::new();
        for e in &a.entries {
            w2.add_raw(e, a.raw_data(&bytes, e).unwrap()).unwrap();
        }
        let plain = w2.finish();
        assert!(!find_sig(&plain, SIG_EOCD64));
        let b = ZipArchive::read(&plain).unwrap();
        assert_eq!(b.data(&plain, &b.entries[0]).unwrap(), b"zip64 ".repeat(100));
        assert!(b.entries[0].extra.is_empty());
    }

    #[test]
    fn zip64_written_automatically_for_many_entries() {
        let mut w = ZipWriter::new();
        let n = 65_536;
        for i in 0..n {
            w.add(&format!("{i}"), b"", Method::Stored).unwrap();
        }
        let bytes = w.finish();
        assert!(find_sig(&bytes[bytes.len() - 120..], SIG_EOCD64));
        let a = ZipArchive::read(&bytes).unwrap();
        assert_eq!(a.entries.len(), n);
        assert_eq!(a.entries[n - 1].name, format!("{}", n - 1));
    }

    #[test]
    fn prepended_data_is_tolerated() {
        let mut w = ZipWriter::new();
        w.add("p.txt", b"prefixed archive", Method::Deflated(6)).unwrap();
        let zip = w.finish();
        let mut sfx = vec![0x4Du8; 333];
        sfx.extend_from_slice(&zip);
        let a = ZipArchive::read(&sfx).unwrap();
        assert_eq!(a.data(&sfx, &a.entries[0]).unwrap(), b"prefixed archive");
    }

    #[test]
    fn corrupted_archives_are_errors() {
        assert!(ZipArchive::read(&[]).is_err());
        assert!(ZipArchive::read(b"PK\x05\x06").is_err());
        assert!(ZipArchive::read(&[0x55; 1000]).is_err());

        let mut w = ZipWriter::new();
        w.add("data.txt", &b"some text to compress ".repeat(200), Method::Deflated(6)).unwrap();
        w.add("b.txt", b"bbbb", Method::Stored).unwrap();
        let good = w.finish();

        // Truncated: the end record (or the directory) is gone.
        for cut in [1, 10, 30, good.len() / 2] {
            assert!(ZipArchive::read(&good[..good.len() - cut]).is_err(), "cut {cut}");
        }
        // A damaged central directory signature.
        let a = ZipArchive::read(&good).unwrap();
        let eocd = good.len() - 22;
        let cd = u32::from_le_bytes([good[eocd + 16], good[eocd + 17], good[eocd + 18], good[eocd + 19]]) as usize;
        let mut bad = good.clone();
        bad[cd] ^= 0xFF;
        assert!(ZipArchive::read(&bad).is_err());
        // More entries claimed than present.
        let mut bad = good.clone();
        bad[eocd + 10] = 9;
        assert!(ZipArchive::read(&bad).is_err());
        // Damaged compressed data: inflate or the CRC check catches it.
        let e = &a.entries[0];
        let start = e.local_offset as usize + 30 + e.raw_name.len();
        let mut bad = good.clone();
        bad[start + 20] ^= 0x55;
        assert!(a.data(&bad, e).is_err());
        // A wrong CRC on a stored entry.
        let mut e2 = a.entries[1].clone();
        e2.crc32 ^= 1;
        assert!(a.data(&good, &e2).unwrap_err().0.contains("CRC"));
        // A bad local header offset.
        let mut e3 = a.entries[1].clone();
        e3.local_offset = 5;
        assert!(a.data(&good, &e3).is_err());
        e3.local_offset = u64::MAX;
        assert!(a.data(&good, &e3).is_err());
        // Encrypted entries are refused.
        let mut e4 = a.entries[1].clone();
        e4.flags |= FLAG_ENCRYPTED;
        assert!(a.data(&good, &e4).is_err());
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut s = 0x1234_5678u64;
        let mut w = ZipWriter::new();
        w.add("r.txt", &b"random damage ".repeat(50), Method::Deflated(6)).unwrap();
        w.force_zip64 = true;
        w.add("s.txt", b"second", Method::Stored).unwrap();
        let good = w.finish();
        for _ in 0..3000 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mut bad = good.clone();
            let k = (s >> 33) as usize % bad.len();
            bad[k] = (s >> 20) as u8;
            if let Ok(a) = ZipArchive::read(&bad) {
                for e in &a.entries {
                    let _ = a.data(&bad, e);
                }
            }
        }
    }
}
