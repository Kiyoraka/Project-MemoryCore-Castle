//! Kerning: the old `kern` table (format 0 pairs, Microsoft and Apple headers) and the `GPOS`
//! 'kern' feature's pair adjustment lookups (type 2, formats 1 and 2, also behind type 9
//! extensions). Offsets are relative to the start of each table.

use super::read::{rd16, rd32, sub};

/// Sum of horizontal format-0 pair values for (left, right) in a `kern` table.
pub(crate) fn kern_table(d: &[u8], l: u16, r: u16) -> i32 {
    let key = ((l as u32) << 16) | r as u32;
    let mut total = 0i32;
    let Some(version) = rd16(d, 0) else { return 0 };
    if version == 0 {
        let n = rd16(d, 2).unwrap_or(0) as usize;
        let mut pos = 4usize;
        for _ in 0..n {
            let (Some(len), Some(cov)) = (rd16(d, pos + 2), rd16(d, pos + 4)) else { break };
            let format = cov >> 8;
            let horizontal = cov & 1 != 0;
            let minimum = cov & 2 != 0;
            let cross = cov & 4 != 0;
            if format == 0 && horizontal && !minimum && !cross {
                let v = format0(d, pos + 6, key).unwrap_or(0);
                if cov & 8 != 0 {
                    total = v;
                } else {
                    total += v;
                }
            }
            if len < 6 {
                break;
            }
            pos += len as usize;
        }
    } else if rd32(d, 0) == Some(0x0001_0000) {
        let n = rd32(d, 4).unwrap_or(0).min(64) as usize;
        let mut pos = 8usize;
        for _ in 0..n {
            let (Some(len), Some(cov)) = (rd32(d, pos), rd16(d, pos + 4)) else { break };
            let vertical = cov & 0x8000 != 0;
            let cross = cov & 0x4000 != 0;
            let variation = cov & 0x2000 != 0;
            if cov & 0xFF == 0 && !vertical && !cross && !variation {
                total += format0(d, pos + 8, key).unwrap_or(0);
            }
            if len < 8 {
                break;
            }
            pos = pos.checked_add(len as usize).unwrap_or(usize::MAX);
        }
    }
    total
}

fn format0(d: &[u8], pos: usize, key: u32) -> Option<i32> {
    let n = rd16(d, pos)? as usize;
    let pairs = pos + 8;
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let p = pairs + 6 * mid;
        let k = rd32(d, p)?;
        if k < key {
            lo = mid + 1;
        } else if k > key {
            hi = mid;
        } else {
            return Some(rd16(d, p + 4)? as i16 as i32);
        }
    }
    Some(0)
}

/// The GPOS pair-adjustment subtables reachable from the 'kern' feature, per lookup in order.
pub(crate) struct Gpos {
    lookups: Vec<Vec<usize>>,
}

impl Gpos {
    pub(crate) fn parse(d: &[u8]) -> Option<Gpos> {
        let fl = rd16(d, 6)? as usize;
        let ll = rd16(d, 8)? as usize;
        let fcount = rd16(d, fl)? as usize;
        let mut idx: Vec<u16> = Vec::new();
        for i in 0..fcount {
            let r = fl + 2 + 6 * i;
            let Some(tag) = sub(d, r, 4) else { break };
            if tag != b"kern" {
                continue;
            }
            let Some(f) = rd16(d, r + 4) else { continue };
            let f = fl + f as usize;
            let n = rd16(d, f + 2).unwrap_or(0) as usize;
            for k in 0..n {
                if let Some(v) = rd16(d, f + 4 + 2 * k) {
                    idx.push(v);
                }
            }
        }
        idx.sort_unstable();
        idx.dedup();
        let lcount = rd16(d, ll)?;
        let mut lookups = Vec::new();
        for li in idx {
            if li >= lcount {
                continue;
            }
            let Some(lo) = rd16(d, ll + 2 + 2 * li as usize) else { continue };
            let lk = ll + lo as usize;
            let (Some(kind), Some(n)) = (rd16(d, lk), rd16(d, lk + 4)) else { continue };
            let mut subs = Vec::new();
            for s in 0..n as usize {
                let Some(so) = rd16(d, lk + 6 + 2 * s) else { break };
                let st = lk + so as usize;
                match kind {
                    2 => subs.push(st),
                    9 => {
                        if rd16(d, st) == Some(1) && rd16(d, st + 2) == Some(2) {
                            if let Some(eo) = rd32(d, st + 4) {
                                subs.push(st + eo as usize);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if !subs.is_empty() {
                lookups.push(subs);
            }
        }
        if lookups.is_empty() {
            None
        } else {
            Some(Gpos { lookups })
        }
    }

    /// Total x-advance adjustment; None when no subtable applied to this pair.
    pub(crate) fn kern(&self, d: &[u8], l: u16, r: u16) -> Option<i32> {
        let mut total = None;
        for subs in &self.lookups {
            for &st in subs {
                if let Some(v) = pair_pos(d, st, l, r) {
                    total = Some(total.unwrap_or(0) + v);
                    break; // first applicable subtable wins within a lookup
                }
            }
        }
        total
    }
}

fn value_size(vf: u16) -> usize {
    2 * (vf & 0xFF).count_ones() as usize
}

fn x_advance(d: &[u8], pos: usize, vf: u16) -> i32 {
    if vf & 4 == 0 {
        return 0;
    }
    rd16(d, pos + 2 * (vf & 3).count_ones() as usize).map_or(0, |v| v as i16 as i32)
}

fn pair_pos(d: &[u8], st: usize, l: u16, r: u16) -> Option<i32> {
    let fmt = rd16(d, st)?;
    let cov = st + rd16(d, st + 2)? as usize;
    let vf1 = rd16(d, st + 4)?;
    let vf2 = rd16(d, st + 6)?;
    let ci = coverage_index(d, cov, l)?;
    let (s1, s2) = (value_size(vf1), value_size(vf2));
    match fmt {
        1 => {
            let count = rd16(d, st + 8)? as usize;
            if ci >= count {
                return None;
            }
            let ps = st + rd16(d, st + 10 + 2 * ci)? as usize;
            let n = rd16(d, ps)? as usize;
            let rec = 2 + s1 + s2;
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let p = ps + 2 + rec * mid;
                let g = rd16(d, p)?;
                if g < r {
                    lo = mid + 1;
                } else if g > r {
                    hi = mid;
                } else {
                    return Some(x_advance(d, p + 2, vf1));
                }
            }
            None
        }
        2 => {
            let cd1 = st + rd16(d, st + 8)? as usize;
            let cd2 = st + rd16(d, st + 10)? as usize;
            let c1n = rd16(d, st + 12)? as usize;
            let c2n = rd16(d, st + 14)? as usize;
            let c1 = class_of(d, cd1, l) as usize;
            let c2 = class_of(d, cd2, r) as usize;
            if c1 >= c1n || c2 >= c2n {
                return None;
            }
            let rec = st + 16 + (c1 * c2n + c2) * (s1 + s2);
            Some(x_advance(d, rec, vf1))
        }
        _ => None,
    }
}

fn coverage_index(d: &[u8], cov: usize, g: u16) -> Option<usize> {
    match rd16(d, cov)? {
        1 => {
            let n = rd16(d, cov + 2)? as usize;
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let v = rd16(d, cov + 4 + 2 * mid)?;
                if v < g {
                    lo = mid + 1;
                } else if v > g {
                    hi = mid;
                } else {
                    return Some(mid);
                }
            }
            None
        }
        2 => {
            let n = rd16(d, cov + 2)? as usize;
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let r = cov + 4 + 6 * mid;
                let start = rd16(d, r)?;
                let end = rd16(d, r + 2)?;
                if g < start {
                    hi = mid;
                } else if g > end {
                    lo = mid + 1;
                } else {
                    return Some(rd16(d, r + 4)? as usize + (g - start) as usize);
                }
            }
            None
        }
        _ => None,
    }
}

fn class_of(d: &[u8], cd: usize, g: u16) -> u16 {
    match rd16(d, cd) {
        Some(1) => {
            let (Some(start), Some(n)) = (rd16(d, cd + 2), rd16(d, cd + 4)) else { return 0 };
            if g < start || g - start >= n {
                return 0;
            }
            rd16(d, cd + 6 + 2 * (g - start) as usize).unwrap_or(0)
        }
        Some(2) => {
            let Some(n) = rd16(d, cd + 2) else { return 0 };
            let (mut lo, mut hi) = (0usize, n as usize);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let r = cd + 4 + 6 * mid;
                let (Some(start), Some(end)) = (rd16(d, r), rd16(d, r + 2)) else { return 0 };
                if g < start {
                    hi = mid;
                } else if g > end {
                    lo = mid + 1;
                } else {
                    return rd16(d, r + 4).unwrap_or(0);
                }
            }
            0
        }
        _ => 0,
    }
}
