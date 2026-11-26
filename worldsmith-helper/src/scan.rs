//! Value types, byte encoding, transforms, matching and narrowing.
//!
//! Pure code: it never touches a process, so it compiles and is tested on any OS.
//!
//! Decisions (documented in the README too):
//! - Every scan walks 4-byte aligned addresses, for all four value types.
//! - Floats match a target `t` when `|stored - t| <= max(0.5 * |factor|, |t| * 1e-3)`, where
//!   `factor` is the transform's multiplier (1, 10, 100 or -1). The 0.5 covers a value the game
//!   shows rounded (an HP bar of 250 may hold 249.7); the relative 1e-3 covers large values.
//! - One label keeps at most [`CANDIDATE_CAP`] candidates; a first scan stops there.

use serde::{Deserialize, Serialize};

/// The most candidates one label keeps (a first scan stops adding past it).
pub const CANDIDATE_CAP: usize = 5_000_000;
/// Absolute float tolerance for a shown value, before the transform's factor.
pub const FLOAT_ABS_TOLERANCE: f64 = 0.5;
/// Relative float tolerance.
pub const FLOAT_REL_TOLERANCE: f64 = 1e-3;
/// Scans walk addresses in steps of this many bytes.
pub const ALIGN: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    I32,
    I64,
    F32,
    F64,
}

impl ValueType {
    pub fn parse(s: &str) -> Option<ValueType> {
        match s.trim().to_ascii_lowercase().as_str() {
            "i32" | "int" | "int32" => Some(ValueType::I32),
            "i64" | "int64" | "long" => Some(ValueType::I64),
            "f32" | "float" => Some(ValueType::F32),
            "f64" | "double" => Some(ValueType::F64),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ValueType::I32 => "i32",
            ValueType::I64 => "i64",
            ValueType::F32 => "f32",
            ValueType::F64 => "f64",
        }
    }

    pub fn size(self) -> usize {
        match self {
            ValueType::I32 | ValueType::F32 => 4,
            ValueType::I64 | ValueType::F64 => 8,
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, ValueType::F32 | ValueType::F64)
    }
}

/// A scan's type: one value type, or `auto` (i32, f32 and f64 at once).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanType {
    I32,
    I64,
    F32,
    F64,
    Auto,
}

impl ScanType {
    pub fn parse(s: &str) -> Option<ScanType> {
        if s.trim().eq_ignore_ascii_case("auto") {
            return Some(ScanType::Auto);
        }
        ValueType::parse(s).map(|t| match t {
            ValueType::I32 => ScanType::I32,
            ValueType::I64 => ScanType::I64,
            ValueType::F32 => ScanType::F32,
            ValueType::F64 => ScanType::F64,
        })
    }

    pub fn types(self) -> Vec<ValueType> {
        match self {
            ScanType::I32 => vec![ValueType::I32],
            ScanType::I64 => vec![ValueType::I64],
            ScanType::F32 => vec![ValueType::F32],
            ScanType::F64 => vec![ValueType::F64],
            ScanType::Auto => vec![ValueType::I32, ValueType::F32, ValueType::F64],
        }
    }
}

/// How a shown value becomes the stored one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transform {
    #[default]
    None,
    X10,
    X100,
    Neg,
}

impl Transform {
    pub fn parse(s: &str) -> Option<Transform> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "none" => Some(Transform::None),
            "x10" => Some(Transform::X10),
            "x100" => Some(Transform::X100),
            "neg" => Some(Transform::Neg),
            _ => None,
        }
    }

    pub fn factor(self) -> i64 {
        match self {
            Transform::None => 1,
            Transform::X10 => 10,
            Transform::X100 => 100,
            Transform::Neg => -1,
        }
    }
}

/// What a scan or `next --value` compares against: the stored value after the transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    /// The exact stored integer, when the stored value is a whole number.
    pub int: Option<i64>,
    /// The stored value as a float.
    pub float: f64,
    /// The float tolerance for this target.
    pub tolerance: f64,
}

impl Target {
    /// Parse a shown value and apply the transform.
    pub fn new(shown: &str, transform: Transform) -> Result<Target, String> {
        let s = shown.trim().replace('_', "");
        if s.is_empty() {
            return Err("No value given.".into());
        }
        let factor = transform.factor();
        // whole numbers stay exact (no f64 rounding past 2^53)
        if let Ok(i) = s.parse::<i64>() {
            let stored = i.checked_mul(factor).ok_or("That value is too large.")?;
            return Ok(Target {
                int: Some(stored),
                float: stored as f64,
                tolerance: tolerance(stored as f64, factor),
            });
        }
        let f: f64 = s.parse().map_err(|_| format!("\"{}\" isn't a number.", shown.trim()))?;
        if !f.is_finite() {
            return Err(format!("\"{}\" isn't a number.", shown.trim()));
        }
        let stored = f * factor as f64;
        let rounded = stored.round();
        let int = if (stored - rounded).abs() < 1e-9 && rounded.abs() < 9.0e18 {
            Some(rounded as i64)
        } else {
            None
        };
        Ok(Target { int, float: stored, tolerance: tolerance(stored, factor) })
    }

    /// Does a stored value of this type match?
    pub fn matches(&self, ty: ValueType, bytes: &[u8]) -> bool {
        match ty {
            ValueType::I32 => match self.int {
                Some(t) if t >= i32::MIN as i64 && t <= i32::MAX as i64 => {
                    read_i32(bytes) == t as i32
                }
                _ => false,
            },
            ValueType::I64 => match self.int {
                Some(t) => read_i64(bytes) == t,
                None => false,
            },
            ValueType::F32 => self.float_matches(read_f32(bytes) as f64),
            ValueType::F64 => self.float_matches(read_f64(bytes)),
        }
    }

    fn float_matches(&self, v: f64) -> bool {
        v.is_finite() && (v - self.float).abs() <= self.tolerance
    }
}

fn tolerance(stored: f64, factor: i64) -> f64 {
    let abs = FLOAT_ABS_TOLERANCE * (factor.unsigned_abs() as f64);
    abs.max(stored.abs() * FLOAT_REL_TOLERANCE)
}

pub fn read_i32(b: &[u8]) -> i32 {
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
pub fn read_i64(b: &[u8]) -> i64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    i64::from_le_bytes(a)
}
pub fn read_f32(b: &[u8]) -> f32 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
pub fn read_f64(b: &[u8]) -> f64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    f64::from_le_bytes(a)
}

/// Encode a raw (already stored-form) value for `write` / `freeze`.
pub fn encode(ty: ValueType, value: &str) -> Result<Vec<u8>, String> {
    let s = value.trim().replace('_', "");
    let bad = || format!("\"{}\" isn't a valid {} value.", value.trim(), ty.name());
    Ok(match ty {
        ValueType::I32 => parse_int(&s, i32::MIN as i64, i32::MAX as i64)
            .map(|v| (v as i32).to_le_bytes().to_vec())
            .ok_or_else(bad)?,
        ValueType::I64 => parse_int(&s, i64::MIN, i64::MAX)
            .map(|v| v.to_le_bytes().to_vec())
            .ok_or_else(bad)?,
        ValueType::F32 => {
            let f: f32 = s.parse().map_err(|_| bad())?;
            if !f.is_finite() {
                return Err(bad());
            }
            f.to_le_bytes().to_vec()
        }
        ValueType::F64 => {
            let f: f64 = s.parse().map_err(|_| bad())?;
            if !f.is_finite() {
                return Err(bad());
            }
            f.to_le_bytes().to_vec()
        }
    })
}

fn parse_int(s: &str, min: i64, max: i64) -> Option<i64> {
    let v = match s.parse::<i64>() {
        Ok(v) => v,
        Err(_) => {
            // "1500.0" is accepted for an integer type; "15.5" is not
            let f: f64 = s.parse().ok()?;
            if !f.is_finite() || f.fract() != 0.0 || f.abs() > 9.0e18 {
                return None;
            }
            f as i64
        }
    };
    (v >= min && v <= max).then_some(v)
}

/// A stored value as text (ints whole, floats in their shortest exact form).
pub fn display(ty: ValueType, bytes: &[u8]) -> String {
    match ty {
        ValueType::I32 => read_i32(bytes).to_string(),
        ValueType::I64 => read_i64(bytes).to_string(),
        ValueType::F32 => read_f32(bytes).to_string(),
        ValueType::F64 => read_f64(bytes).to_string(),
    }
}

/// A stored value packed into 8 bytes (little-endian, zero padded) for the old-value column.
pub fn pack(ty: ValueType, bytes: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a[..ty.size()].copy_from_slice(&bytes[..ty.size()]);
    u64::from_le_bytes(a)
}

pub fn unpack(packed: u64) -> [u8; 8] {
    packed.to_le_bytes()
}

// ---- memory region filter (Win32 constants, kept here so the rule is testable) ----

pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_PRIVATE: u32 = 0x20000;
pub const MEM_IMAGE: u32 = 0x100_0000;
pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_WRITECOPY: u32 = 0x08;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
pub const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;
pub const PAGE_GUARD: u32 = 0x100;

/// Is a region worth scanning: committed, writable, not guard / no-access, and private or image
/// memory (mapped files and shared sections are skipped, as Cheat Engine does by default).
pub fn is_scannable(state: u32, protect: u32, kind: u32) -> bool {
    if state != MEM_COMMIT {
        return false;
    }
    if protect & (PAGE_GUARD | PAGE_NOACCESS) != 0 {
        return false;
    }
    if kind != MEM_PRIVATE && kind != MEM_IMAGE {
        return false;
    }
    matches!(
        protect & 0xFF,
        PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
    )
}

// ---- candidates ----

/// One label's candidates, column by column (17 bytes each, so the cap stays near 85 MB).
#[derive(Clone, Debug, Default)]
pub struct CandList {
    pub addrs: Vec<u64>,
    pub types: Vec<ValueType>,
    pub olds: Vec<u64>,
}

impl CandList {
    pub fn len(&self) -> usize {
        self.addrs.len()
    }
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.addrs.is_empty()
    }
    pub fn push(&mut self, addr: u64, ty: ValueType, old: u64) {
        self.addrs.push(addr);
        self.types.push(ty);
        self.olds.push(old);
    }
}

/// Scan one buffer read from `base`. Only start offsets below `limit` are tried (the bytes past
/// it are overlap so a value that straddles two chunks is still seen once). Returns false when the
/// cap was hit.
pub fn scan_buffer(
    buf: &[u8],
    base: u64,
    limit: usize,
    types: &[ValueType],
    target: &Target,
    out: &mut CandList,
    cap: usize,
) -> bool {
    let limit = limit.min(buf.len());
    // align the first offset to an absolute 4-byte boundary
    let mut off = ((ALIGN as u64 - (base % ALIGN as u64)) % ALIGN as u64) as usize;
    // a fast path for the common int-only scan
    let i32_only = types == [ValueType::I32];
    let i32_needle = match target.int {
        Some(t) if t >= i32::MIN as i64 && t <= i32::MAX as i64 => Some((t as i32).to_le_bytes()),
        _ => None,
    };
    if i32_only {
        let Some(needle) = i32_needle else { return true };
        while off < limit && off + 4 <= buf.len() {
            if buf[off..off + 4] == needle {
                if out.len() >= cap {
                    return false;
                }
                out.push(base + off as u64, ValueType::I32, pack(ValueType::I32, &buf[off..]));
            }
            off += ALIGN;
        }
        return true;
    }
    while off < limit {
        for &ty in types {
            let n = ty.size();
            if off + n > buf.len() {
                continue;
            }
            let b = &buf[off..off + n];
            if target.matches(ty, b) {
                if out.len() >= cap {
                    return false;
                }
                out.push(base + off as u64, ty, pack(ty, b));
            }
        }
        off += ALIGN;
    }
    true
}

/// How `next` narrows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NextRule {
    Value(String),
    Changed,
    Unchanged,
    Increased,
    Decreased,
}

fn numeric(ty: ValueType, b: &[u8]) -> f64 {
    match ty {
        ValueType::I32 => read_i32(b) as f64,
        ValueType::I64 => read_i64(b) as f64,
        ValueType::F32 => read_f32(b) as f64,
        ValueType::F64 => read_f64(b),
    }
}

fn int_cmp(ty: ValueType, new: &[u8], old: &[u8]) -> std::cmp::Ordering {
    match ty {
        ValueType::I32 => read_i32(new).cmp(&read_i32(old)),
        _ => read_i64(new).cmp(&read_i64(old)),
    }
}

/// Does one candidate survive? `new` holds the freshly read bytes, `old` the last ones.
pub fn keep(rule: &NextRule, target: Option<&Target>, ty: ValueType, new: &[u8], old: &[u8]) -> bool {
    let n = ty.size();
    let (new, old) = (&new[..n], &old[..n]);
    use std::cmp::Ordering::*;
    match rule {
        NextRule::Value(_) => target.map(|t| t.matches(ty, new)).unwrap_or(false),
        NextRule::Changed => new != old,
        NextRule::Unchanged => new == old,
        NextRule::Increased | NextRule::Decreased => {
            let ord = if ty.is_float() {
                let (a, b) = (numeric(ty, new), numeric(ty, old));
                if !a.is_finite() || !b.is_finite() {
                    return false;
                }
                a.partial_cmp(&b).unwrap_or(Equal)
            } else {
                int_cmp(ty, new, old)
            };
            if *rule == NextRule::Increased {
                ord == Greater
            } else {
                ord == Less
            }
        }
    }
}

/// Reads candidate values in blocks so millions of candidates don't cost millions of reads.
/// `read(addr, buf)` fills the whole buffer or returns false.
pub struct BlockReader<F: FnMut(u64, &mut [u8]) -> bool> {
    read: F,
    base: u64,
    buf: Vec<u8>,
    valid: usize,
}

pub const BLOCK: usize = 64 * 1024;
const PAGE: u64 = 4096;

impl<F: FnMut(u64, &mut [u8]) -> bool> BlockReader<F> {
    pub fn new(read: F) -> Self {
        BlockReader { read, base: 0, buf: vec![0u8; BLOCK], valid: 0 }
    }

    /// The `n` bytes at `addr`, or None when they can't be read.
    pub fn get(&mut self, addr: u64, n: usize, out: &mut [u8]) -> bool {
        if addr >= self.base && addr + n as u64 <= self.base + self.valid as u64 {
            let o = (addr - self.base) as usize;
            out[..n].copy_from_slice(&self.buf[o..o + n]);
            return true;
        }
        // a big block from the page that holds addr, then a single page, then the value alone
        let page = addr & !(PAGE - 1);
        for len in [BLOCK, PAGE as usize] {
            if addr + n as u64 > page + len as u64 {
                continue;
            }
            if (self.read)(page, &mut self.buf[..len]) {
                self.base = page;
                self.valid = len;
                let o = (addr - page) as usize;
                out[..n].copy_from_slice(&self.buf[o..o + n]);
                return true;
            }
        }
        self.valid = 0;
        (self.read)(addr, &mut out[..n])
    }
}

/// Narrow a candidate list. Returns the survivors with their old values updated.
pub fn narrow<F: FnMut(u64, &mut [u8]) -> bool>(
    list: &CandList,
    rule: &NextRule,
    target: Option<&Target>,
    read: F,
) -> CandList {
    let mut reader = BlockReader::new(read);
    let mut out = CandList::default();
    let mut new = [0u8; 8];
    for i in 0..list.len() {
        let (addr, ty) = (list.addrs[i], list.types[i]);
        if !reader.get(addr, ty.size(), &mut new) {
            continue;
        }
        let old = unpack(list.olds[i]);
        if keep(rule, target, ty, &new, &old) {
            out.push(addr, ty, pack(ty, &new));
        }
    }
    out
}

// ---- unknown-value scans: a snapshot of the bytes, not a list of addresses ----
//
// `scan --unknown` makes every aligned address of one type in the writable regions a candidate.
// Listing them would cost 17 bytes per 4 bytes of game memory, so instead each region keeps a copy
// of its bytes and one bit per aligned slot (alive or not): about 1.03 bytes per byte scanned.
// The copy is capped at [`SNAPSHOT_CAP_BYTES`] in total; regions past the cap are left out.
// Each `next` re-reads the regions, compares slot by slot against the copy and keeps the new bytes
// as the next step's copy. Once [`CANDIDATE_CAP`] or fewer survive, the label turns into an
// ordinary candidate list and the copies are dropped.

/// The most game memory an unknown scan copies (1 GiB).
pub const SNAPSHOT_CAP_BYTES: usize = 1 << 30;
/// Snapshot pages are 4 KB; a slot touching an unreadable page is dead.
pub const SNAP_PAGE: usize = 4096;

/// One region of an unknown scan.
#[derive(Clone, Debug)]
pub struct SnapRegion {
    pub base: u64,
    pub bytes: Vec<u8>,
    /// One bit per aligned slot (slot i sits at byte offset i * ALIGN).
    pub mask: Vec<u64>,
    pub alive: usize,
}

/// How many aligned slots of `size` bytes fit in `len` bytes.
pub fn slot_count(len: usize, size: usize) -> usize {
    if len >= size {
        (len - size) / ALIGN + 1
    } else {
        0
    }
}

fn slot_readable(off: usize, size: usize, ok_pages: &[bool]) -> bool {
    let first = off / SNAP_PAGE;
    let last = (off + size - 1) / SNAP_PAGE;
    ok_pages.get(first).copied().unwrap_or(false) && ok_pages.get(last).copied().unwrap_or(false)
}

impl SnapRegion {
    /// A first snapshot: every readable slot is alive. `base` must be 4-byte aligned (regions are
    /// page aligned).
    pub fn new(base: u64, bytes: Vec<u8>, ok_pages: &[bool], ty: ValueType) -> SnapRegion {
        let n = slot_count(bytes.len(), ty.size());
        let mut mask = vec![0u64; n.div_ceil(64)];
        let mut alive = 0;
        for i in 0..n {
            if slot_readable(i * ALIGN, ty.size(), ok_pages) {
                mask[i / 64] |= 1 << (i % 64);
                alive += 1;
            }
        }
        SnapRegion { base, bytes, mask, alive }
    }

    pub fn is_alive(&self, i: usize) -> bool {
        self.mask[i / 64] & (1 << (i % 64)) != 0
    }

    /// Narrow against freshly read bytes (same length) and keep them as the next copy.
    pub fn narrow(
        &mut self,
        new: Vec<u8>,
        ok_pages: &[bool],
        ty: ValueType,
        rule: &NextRule,
        target: Option<&Target>,
    ) {
        let size = ty.size();
        let n = slot_count(self.bytes.len().min(new.len()), size);
        let mut alive = 0;
        for w in 0..self.mask.len() {
            let mut word = self.mask[w];
            if word == 0 {
                continue;
            }
            let mut bits = word;
            while bits != 0 {
                let b = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let i = w * 64 + b;
                let off = i * ALIGN;
                let survives = i < n
                    && slot_readable(off, size, ok_pages)
                    && keep(rule, target, ty, &new[off..off + size], &self.bytes[off..off + size]);
                if survives {
                    alive += 1;
                } else {
                    word &= !(1 << b);
                }
            }
            self.mask[w] = word;
        }
        self.bytes = new;
        self.alive = alive;
    }

    /// The alive slots as candidates.
    pub fn push_alive(&self, ty: ValueType, out: &mut CandList) {
        let size = ty.size();
        for i in 0..slot_count(self.bytes.len(), size) {
            if self.is_alive(i) {
                let off = i * ALIGN;
                out.push(self.base + off as u64, ty, pack(ty, &self.bytes[off..off + size]));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_regions_narrow_like_a_list() {
        let ty = ValueType::I32;
        let mut old = vec![0u8; 2 * SNAP_PAGE];
        old[8..12].copy_from_slice(&100i32.to_le_bytes());
        old[16..20].copy_from_slice(&100i32.to_le_bytes());
        let mut r = SnapRegion::new(0x20000, old.clone(), &[true, true], ty);
        assert_eq!(r.alive, slot_count(2 * SNAP_PAGE, 4));
        assert_eq!(r.alive, 2048);

        // offset 8 increases, offset 16 decreases, everything else stays
        let mut new = old.clone();
        new[8..12].copy_from_slice(&150i32.to_le_bytes());
        new[16..20].copy_from_slice(&50i32.to_le_bytes());
        r.narrow(new.clone(), &[true, true], ty, &NextRule::Changed, None);
        assert_eq!(r.alive, 2);

        let mut newer = new.clone();
        newer[8..12].copy_from_slice(&151i32.to_le_bytes());
        newer[16..20].copy_from_slice(&40i32.to_le_bytes());
        r.narrow(newer.clone(), &[true, true], ty, &NextRule::Increased, None);
        assert_eq!(r.alive, 1);
        let mut list = CandList::default();
        r.push_alive(ty, &mut list);
        assert_eq!(list.addrs, vec![0x20008]);
        assert_eq!(read_i32(&unpack(list.olds[0])), 151);
    }

    #[test]
    fn snapshot_unchanged_and_decreased_and_value() {
        let ty = ValueType::F32;
        let mut old = vec![0u8; 64];
        old[0..4].copy_from_slice(&5.0f32.to_le_bytes());
        old[4..8].copy_from_slice(&5.0f32.to_le_bytes());
        let mut r = SnapRegion::new(0x1000, old.clone(), &[true], ty);
        let mut new = old.clone();
        new[4..8].copy_from_slice(&4.0f32.to_le_bytes());
        r.narrow(new.clone(), &[true], ty, &NextRule::Decreased, None);
        assert_eq!(r.alive, 1);
        let mut r2 = SnapRegion::new(0x1000, old.clone(), &[true], ty);
        r2.narrow(new.clone(), &[true], ty, &NextRule::Unchanged, None);
        assert_eq!(r2.alive, 15);
        let t = Target::new("4", Transform::None).unwrap();
        let mut r3 = SnapRegion::new(0x1000, old, &[true], ty);
        r3.narrow(new, &[true], ty, &NextRule::Value("4".into()), Some(&t));
        assert_eq!(r3.alive, 1);
    }

    #[test]
    fn snapshot_slots_on_bad_pages_are_dead() {
        let ty = ValueType::I64;
        let bytes = vec![0u8; 2 * SNAP_PAGE];
        let r = SnapRegion::new(0x1000, bytes.clone(), &[true, false], ty);
        // 8-byte slots every 4 bytes in page 0, minus the one straddling into page 1
        assert_eq!(r.alive, (SNAP_PAGE - 8) / 4 + 1);
        let mut r = SnapRegion::new(0x1000, bytes.clone(), &[true, true], ty);
        r.narrow(bytes, &[false, true], ty, &NextRule::Unchanged, None);
        assert_eq!(r.alive, (SNAP_PAGE - 8) / 4 + 1);
        assert_eq!(slot_count(3, 4), 0);
        assert_eq!(slot_count(8, 8), 1);
        assert_eq!(slot_count(12, 8), 2);
    }

    #[test]
    fn transforms_apply_before_comparison() {
        let t = Target::new("25", Transform::X10).unwrap();
        assert_eq!(t.int, Some(250));
        assert!(t.matches(ValueType::I32, &250i32.to_le_bytes()));
        let t = Target::new("2.5", Transform::X100).unwrap();
        assert_eq!(t.int, Some(250));
        let t = Target::new("40", Transform::Neg).unwrap();
        assert!(t.matches(ValueType::I32, &(-40i32).to_le_bytes()));
        assert!(!t.matches(ValueType::I32, &40i32.to_le_bytes()));
        let t = Target::new("2.55", Transform::X10).unwrap();
        assert_eq!(t.int, None);
        assert!(!t.matches(ValueType::I32, &25i32.to_le_bytes()));
    }

    #[test]
    fn floats_have_a_tolerance() {
        let t = Target::new("250", Transform::None).unwrap();
        assert!(t.matches(ValueType::F32, &249.7f32.to_le_bytes()));
        assert!(t.matches(ValueType::F64, &250.4f64.to_le_bytes()));
        assert!(!t.matches(ValueType::F32, &249.0f32.to_le_bytes()));
        assert!(!t.matches(ValueType::F32, &f32::NAN.to_le_bytes()));
        // relative tolerance for big values
        let t = Target::new("1000000", Transform::None).unwrap();
        assert!(t.matches(ValueType::F32, &1_000_800f32.to_le_bytes()));
        // the absolute tolerance scales with the transform
        let t = Target::new("25", Transform::X10).unwrap();
        assert!(t.matches(ValueType::F32, &253.0f32.to_le_bytes()));
        // ints are exact
        let t = Target::new("250", Transform::None).unwrap();
        assert!(!t.matches(ValueType::I32, &251i32.to_le_bytes()));
    }

    #[test]
    fn big_ints_stay_exact() {
        let t = Target::new("9007199254740993", Transform::None).unwrap();
        assert!(t.matches(ValueType::I64, &9007199254740993i64.to_le_bytes()));
        assert!(!t.matches(ValueType::I64, &9007199254740992i64.to_le_bytes()));
        assert!(!t.matches(ValueType::I32, &0i32.to_le_bytes()));
    }

    #[test]
    fn bad_values_are_refused() {
        assert!(Target::new("abc", Transform::None).is_err());
        assert!(Target::new("", Transform::None).is_err());
        assert!(Target::new("inf", Transform::None).is_err());
        assert!(encode(ValueType::I32, "3000000000").is_err());
        assert!(encode(ValueType::I32, "1.5").is_err());
        assert_eq!(encode(ValueType::I32, "1500.0").unwrap(), 1500i32.to_le_bytes());
        assert_eq!(encode(ValueType::F32, "2.5").unwrap(), 2.5f32.to_le_bytes());
        assert_eq!(encode(ValueType::I64, "-7").unwrap(), (-7i64).to_le_bytes());
        assert!(encode(ValueType::F64, "nan").is_err());
    }

    #[test]
    fn display_and_pack_round_trip() {
        let b = 250.0f32.to_le_bytes();
        assert_eq!(display(ValueType::F32, &b), "250");
        assert_eq!(display(ValueType::F32, &unpack(pack(ValueType::F32, &b))), "250");
        assert_eq!(display(ValueType::I32, &(-5i32).to_le_bytes()), "-5");
        assert_eq!(display(ValueType::F64, &30.5f64.to_le_bytes()), "30.5");
    }

    #[test]
    fn types_parse() {
        assert_eq!(ScanType::parse("AUTO"), Some(ScanType::Auto));
        assert_eq!(ScanType::parse("f64"), Some(ScanType::F64));
        assert_eq!(ScanType::parse("x"), None);
        assert_eq!(ValueType::parse("auto"), None);
        assert_eq!(ScanType::Auto.types(), vec![ValueType::I32, ValueType::F32, ValueType::F64]);
        assert_eq!(Transform::parse("x100"), Some(Transform::X100));
        assert_eq!(Transform::parse("x3"), None);
    }

    #[test]
    fn region_filter() {
        assert!(is_scannable(MEM_COMMIT, PAGE_READWRITE, MEM_PRIVATE));
        assert!(is_scannable(MEM_COMMIT, PAGE_WRITECOPY, MEM_IMAGE));
        assert!(is_scannable(MEM_COMMIT, PAGE_EXECUTE_READWRITE, MEM_PRIVATE));
        assert!(!is_scannable(MEM_COMMIT, PAGE_READWRITE | PAGE_GUARD, MEM_PRIVATE));
        assert!(!is_scannable(MEM_COMMIT, 0x02, MEM_PRIVATE)); // read-only
        assert!(!is_scannable(MEM_COMMIT, PAGE_NOACCESS, MEM_PRIVATE));
        assert!(!is_scannable(0x2000, PAGE_READWRITE, MEM_PRIVATE)); // reserved
        assert!(!is_scannable(MEM_COMMIT, PAGE_READWRITE, 0x40000)); // mapped
    }

    #[test]
    fn scan_buffer_finds_aligned_values_and_respects_limit_and_cap() {
        let mut buf = vec![0u8; 64];
        buf[8..12].copy_from_slice(&1234i32.to_le_bytes());
        buf[13..17].copy_from_slice(&1234i32.to_le_bytes()); // unaligned: skipped
        buf[40..44].copy_from_slice(&1234.2f32.to_le_bytes());
        buf[48..56].copy_from_slice(&1234.0f64.to_le_bytes());
        buf[60..64].copy_from_slice(&1234i32.to_le_bytes());
        let t = Target::new("1234", Transform::None).unwrap();

        let mut out = CandList::default();
        assert!(scan_buffer(&buf, 0x1000, 64, &[ValueType::I32], &t, &mut out, 100));
        assert_eq!(out.addrs, vec![0x1008, 0x103C]);

        let mut out = CandList::default();
        scan_buffer(&buf, 0x1000, 60, &[ValueType::I32], &t, &mut out, 100);
        assert_eq!(out.addrs, vec![0x1008], "the limit keeps overlap bytes for the next chunk");

        let mut out = CandList::default();
        scan_buffer(&buf, 0x1000, 64, &ScanType::Auto.types(), &t, &mut out, 100);
        let found: Vec<(u64, ValueType)> =
            out.addrs.iter().copied().zip(out.types.iter().copied()).collect();
        assert!(found.contains(&(0x1008, ValueType::I32)));
        assert!(found.contains(&(0x1028, ValueType::F32)));
        assert!(found.contains(&(0x1030, ValueType::F64)));

        let mut out = CandList::default();
        assert!(!scan_buffer(&buf, 0x1000, 64, &[ValueType::I32], &t, &mut out, 1));
        assert_eq!(out.len(), 1);

        // an unaligned base starts at the next aligned address
        let mut out = CandList::default();
        scan_buffer(&buf[2..], 0x1002, 62, &[ValueType::I32], &t, &mut out, 100);
        assert_eq!(out.addrs, vec![0x1008, 0x103C]);
    }

    #[test]
    fn keep_rules() {
        let a = 5i32.to_le_bytes();
        let b = 7i32.to_le_bytes();
        let t = Target::new("7", Transform::None).unwrap();
        assert!(keep(&NextRule::Changed, None, ValueType::I32, &b, &a));
        assert!(!keep(&NextRule::Unchanged, None, ValueType::I32, &b, &a));
        assert!(keep(&NextRule::Increased, None, ValueType::I32, &b, &a));
        assert!(!keep(&NextRule::Decreased, None, ValueType::I32, &b, &a));
        assert!(keep(&NextRule::Decreased, None, ValueType::I32, &(-1i32).to_le_bytes(), &a));
        assert!(keep(&NextRule::Value("7".into()), Some(&t), ValueType::I32, &b, &a));
        let fa = 1.5f32.to_le_bytes();
        let fb = 1.25f32.to_le_bytes();
        assert!(keep(&NextRule::Decreased, None, ValueType::F32, &fb, &fa));
        assert!(!keep(&NextRule::Increased, None, ValueType::F32, &f32::NAN.to_le_bytes(), &fa));
    }

    fn fake_memory() -> (Vec<u8>, u64) {
        (vec![0u8; 3 * 4096], 0x10000)
    }

    #[test]
    fn narrow_reads_in_blocks_and_updates_olds() {
        let (mut mem, base) = fake_memory();
        let mut list = CandList::default();
        for off in [16u64, 4096 + 8, 8192 + 4092] {
            mem[off as usize..off as usize + 4].copy_from_slice(&10i32.to_le_bytes());
            list.push(base + off, ValueType::I32, pack(ValueType::I32, &10i32.to_le_bytes()));
        }
        // the middle one changes to 11
        mem[4096 + 8..4096 + 12].copy_from_slice(&11i32.to_le_bytes());
        let mut reads = 0;
        let read = |addr: u64, buf: &mut [u8]| {
            reads += 1;
            if addr < base || addr + buf.len() as u64 > base + mem.len() as u64 {
                return false;
            }
            let o = (addr - base) as usize;
            buf.copy_from_slice(&mem[o..o + buf.len()]);
            true
        };
        let t = Target::new("11", Transform::None).unwrap();
        let out = narrow(&list, &NextRule::Value("11".into()), Some(&t), read);
        assert_eq!(out.addrs, vec![base + 4096 + 8]);
        assert_eq!(read_i32(&unpack(out.olds[0])), 11);
        assert!(reads <= 6, "block reads, not one read per candidate ({reads})");

        let read = |addr: u64, buf: &mut [u8]| {
            if addr < base || addr + buf.len() as u64 > base + mem.len() as u64 {
                return false;
            }
            let o = (addr - base) as usize;
            buf.copy_from_slice(&mem[o..o + buf.len()]);
            true
        };
        let out = narrow(&list, &NextRule::Unchanged, None, read);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn unreadable_candidates_drop() {
        let mut list = CandList::default();
        list.push(0x5000, ValueType::I32, 0);
        let out = narrow(&list, &NextRule::Unchanged, None, |_, _| false);
        assert!(out.is_empty());
    }
}
