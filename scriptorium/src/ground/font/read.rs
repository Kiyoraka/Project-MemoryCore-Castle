//! Bounds-checked big-endian reads and writes. Every read answers `None` past the end - a malformed
//! font can make us give up on a table, never panic.

#[inline]
pub(crate) fn rd8(d: &[u8], o: usize) -> Option<u8> {
    d.get(o).copied()
}

#[inline]
pub(crate) fn rd16(d: &[u8], o: usize) -> Option<u16> {
    let b = d.get(o..o.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

#[inline]
pub(crate) fn rdi16(d: &[u8], o: usize) -> Option<i16> {
    rd16(d, o).map(|v| v as i16)
}

#[inline]
pub(crate) fn rd32(d: &[u8], o: usize) -> Option<u32> {
    let b = d.get(o..o.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
pub(crate) fn sub(d: &[u8], o: usize, len: usize) -> Option<&[u8]> {
    d.get(o..o.checked_add(len)?)
}

#[inline]
pub(crate) fn w16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}

#[inline]
pub(crate) fn wi16(v: &mut Vec<u8>, x: i16) {
    v.extend_from_slice(&x.to_be_bytes());
}

#[inline]
pub(crate) fn w32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// Overwrite a big-endian u16 in place (ignored when out of range).
pub(crate) fn put16(v: &mut [u8], o: usize, x: u16) {
    if let Some(b) = v.get_mut(o..o.saturating_add(2)) {
        if b.len() == 2 {
            b.copy_from_slice(&x.to_be_bytes());
        }
    }
}

pub(crate) fn put32(v: &mut [u8], o: usize, x: u32) {
    if let Some(b) = v.get_mut(o..o.saturating_add(4)) {
        if b.len() == 4 {
            b.copy_from_slice(&x.to_be_bytes());
        }
    }
}

pub(crate) const TTCF: u32 = 0x7474_6366; // 'ttcf'
pub(crate) const SFNT_TRUE: u32 = 0x0001_0000;
pub(crate) const SFNT_OTTO: u32 = 0x4F54_544F; // 'OTTO'
pub(crate) const SFNT_APPLE: u32 = 0x7472_7565; // 'true'

/// One entry of a face's table directory. Offsets are absolute file offsets (also inside a TTC).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TableRec {
    pub tag: [u8; 4],
    pub off: usize,
    pub len: usize,
}

/// Number of faces in a font file: the TTC count, or 1 for a plain sfnt.
pub(crate) fn face_count(d: &[u8]) -> u32 {
    match rd32(d, 0) {
        Some(TTCF) => rd32(d, 8).unwrap_or(0),
        Some(SFNT_TRUE) | Some(SFNT_OTTO) | Some(SFNT_APPLE) => 1,
        _ => 0,
    }
}

/// Where face `index`'s sfnt header starts. `d` needs only the file's first bytes (the TTC header).
pub(crate) fn face_offset(d: &[u8], index: u32) -> Result<usize, super::FontError> {
    use super::FontError;
    let tag = rd32(d, 0).ok_or(FontError::Truncated)?;
    if tag == TTCF {
        let n = rd32(d, 8).ok_or(FontError::Truncated)?;
        if index >= n {
            return Err(FontError::NoSuchFace(index));
        }
        let o = rd32(d, 12 + 4 * index as usize).ok_or(FontError::Truncated)?;
        Ok(o as usize)
    } else if tag == SFNT_TRUE || tag == SFNT_OTTO || tag == SFNT_APPLE {
        if index != 0 {
            return Err(FontError::NoSuchFace(index));
        }
        Ok(0)
    } else {
        Err(FontError::NotAFont)
    }
}

/// Read a face's table records from bytes starting at its sfnt header.
pub(crate) fn table_records(dir: &[u8]) -> Option<Vec<TableRec>> {
    let n = rd16(dir, 4)? as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let r = 12 + 16 * i;
        let tag = sub(dir, r, 4)?;
        let off = rd32(dir, r + 8)? as usize;
        let len = rd32(dir, r + 12)? as usize;
        out.push(TableRec { tag: [tag[0], tag[1], tag[2], tag[3]], off, len });
    }
    Some(out)
}

/// The sfnt version word of a face (0x00010000, 'OTTO' or 'true').
pub(crate) fn sfnt_version(dir: &[u8]) -> Option<u32> {
    rd32(dir, 0)
}
