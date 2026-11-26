//! The `CFF ` table (OpenType with PostScript outlines): INDEX and DICT structures, plain and
//! CID-keyed fonts (FDArray/FDSelect), and a Type 2 charstring interpreter producing cubic paths.
//! Offsets are relative to the start of the CFF table. Not handled: CFF2 (variable fonts), Type 1
//! charstrings, and the deprecated `seac` accent form of endchar.

use super::read::{rd16, rd8, rdi16, sub};
use super::PathOp;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Index {
    count: usize,
    off_size: u8,
    offsets: usize,
    base: usize,
}

impl Index {
    /// Parse an INDEX at `pos`; returns it and the position just past it.
    pub(crate) fn parse(d: &[u8], pos: usize) -> Option<(Index, usize)> {
        let count = rd16(d, pos)? as usize;
        if count == 0 {
            return Some((Index::default(), pos + 2));
        }
        let off_size = rd8(d, pos + 2)?;
        if !(1..=4).contains(&off_size) {
            return None;
        }
        let offsets = pos + 3;
        let base = offsets + (count + 1) * off_size as usize - 1;
        let idx = Index { count, off_size, offsets, base };
        let last = idx.offset(d, count)?;
        Some((idx, base.checked_add(last)?))
    }

    fn offset(&self, d: &[u8], i: usize) -> Option<usize> {
        let p = self.offsets + i * self.off_size as usize;
        let mut v = 0usize;
        for k in 0..self.off_size as usize {
            v = (v << 8) | rd8(d, p + k)? as usize;
        }
        Some(v)
    }

    pub(crate) fn len(&self) -> usize {
        self.count
    }

    pub(crate) fn get<'a>(&self, d: &'a [u8], i: usize) -> Option<&'a [u8]> {
        if i >= self.count {
            return None;
        }
        let a = self.offset(d, i)?;
        let b = self.offset(d, i + 1)?;
        if a == 0 || b < a {
            return None;
        }
        d.get(self.base.checked_add(a)?..self.base.checked_add(b)?)
    }
}

type Dict = Vec<(u16, Vec<f64>)>;

fn parse_dict(d: &[u8]) -> Dict {
    let mut out = Vec::new();
    let mut ops: Vec<f64> = Vec::new();
    let mut i = 0usize;
    while i < d.len() {
        let b0 = d[i];
        i += 1;
        match b0 {
            0..=21 => {
                let op = if b0 == 12 {
                    let Some(&b1) = d.get(i) else { break };
                    i += 1;
                    1200 + b1 as u16
                } else {
                    b0 as u16
                };
                out.push((op, std::mem::take(&mut ops)));
            }
            28 => {
                let Some(v) = rdi16(d, i) else { break };
                i += 2;
                ops.push(v as f64);
            }
            29 => {
                let Some(b) = sub(d, i, 4) else { break };
                i += 4;
                ops.push(i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64);
            }
            30 => {
                let mut s = String::new();
                'real: while let Some(&b) = d.get(i) {
                    i += 1;
                    for nib in [b >> 4, b & 0xF] {
                        match nib {
                            0..=9 => s.push((b'0' + nib) as char),
                            0xA => s.push('.'),
                            0xB => s.push('E'),
                            0xC => s.push_str("E-"),
                            0xE => s.push('-'),
                            0xF => break 'real,
                            _ => {}
                        }
                    }
                }
                ops.push(s.parse::<f64>().unwrap_or(0.0));
            }
            32..=246 => ops.push(b0 as f64 - 139.0),
            247..=250 => {
                let Some(&b1) = d.get(i) else { break };
                i += 1;
                ops.push(((b0 as f64 - 247.0) * 256.0) + b1 as f64 + 108.0);
            }
            251..=254 => {
                let Some(&b1) = d.get(i) else { break };
                i += 1;
                ops.push(-((b0 as f64 - 251.0) * 256.0) - b1 as f64 - 108.0);
            }
            _ => {}
        }
        if ops.len() > 513 {
            ops.clear();
        }
    }
    out
}

fn dict_get(dict: &Dict, op: u16) -> Option<&Vec<f64>> {
    dict.iter().find(|(o, _)| *o == op).map(|(_, v)| v)
}

fn as_offset(v: f64) -> Option<usize> {
    if v.is_finite() && v >= 0.0 && v < 1e12 {
        Some(v as usize)
    } else {
        None
    }
}

fn private_subrs(d: &[u8], dict: &Dict) -> Option<Index> {
    let p = dict_get(dict, 18)?;
    if p.len() < 2 {
        return None;
    }
    let size = as_offset(p[0])?;
    let off = as_offset(p[1])?;
    let pd = parse_dict(sub(d, off, size)?);
    let s = as_offset(*dict_get(&pd, 19)?.first()?)?;
    Index::parse(d, off.checked_add(s)?).map(|x| x.0)
}

enum Locals {
    Single(Option<Index>),
    Cid { fd_select: usize, fd_subrs: Vec<Option<Index>> },
}

pub(crate) struct Cff {
    charstrings: Index,
    gsubrs: Index,
    locals: Locals,
}

impl Cff {
    pub(crate) fn parse(d: &[u8]) -> Option<Cff> {
        if rd8(d, 0)? != 1 {
            return None; // CFF2 and unknown majors are not handled
        }
        let hdr = rd8(d, 2)? as usize;
        let (_names, p) = Index::parse(d, hdr)?;
        let (tops, p) = Index::parse(d, p)?;
        let (_strings, p) = Index::parse(d, p)?;
        let (gsubrs, _) = Index::parse(d, p)?;
        let top = parse_dict(tops.get(d, 0)?);
        if let Some(t) = dict_get(&top, 1206) {
            if t.first().copied().unwrap_or(2.0) != 2.0 {
                return None;
            }
        }
        let cs_off = as_offset(*dict_get(&top, 17)?.first()?)?;
        let (charstrings, _) = Index::parse(d, cs_off)?;
        let locals = if dict_get(&top, 1230).is_some() {
            let fda = as_offset(*dict_get(&top, 1236)?.first()?)?;
            let fd_select = as_offset(*dict_get(&top, 1237)?.first()?)?;
            let (fdarray, _) = Index::parse(d, fda)?;
            let mut fd_subrs = Vec::new();
            for i in 0..fdarray.len().min(256) {
                let fd = parse_dict(fdarray.get(d, i).unwrap_or(&[]));
                fd_subrs.push(private_subrs(d, &fd));
            }
            Locals::Cid { fd_select, fd_subrs }
        } else {
            Locals::Single(private_subrs(d, &top))
        };
        Some(Cff { charstrings, gsubrs, locals })
    }

    pub(crate) fn outline(&self, d: &[u8], gid: u16) -> Option<Vec<PathOp>> {
        let cs = self.charstrings.get(d, gid as usize)?;
        let local = match &self.locals {
            Locals::Single(s) => *s,
            Locals::Cid { fd_select, fd_subrs } => {
                fd_index(d, *fd_select, gid).and_then(|i| fd_subrs.get(i).copied().flatten())
            }
        };
        charstring_outline(d, cs, self.gsubrs, local)
    }
}

fn fd_index(d: &[u8], pos: usize, gid: u16) -> Option<usize> {
    match rd8(d, pos)? {
        0 => rd8(d, pos + 1 + gid as usize).map(|v| v as usize),
        3 => {
            let n = rd16(d, pos + 1)? as usize;
            let mut fd = None;
            for i in 0..n {
                let r = pos + 3 + 3 * i;
                let first = rd16(d, r)?;
                let next = rd16(d, r + 3)?; // the next range's first, or the sentinel
                if gid >= first && gid < next {
                    fd = Some(rd8(d, r + 2)? as usize);
                    break;
                }
            }
            fd
        }
        _ => None,
    }
}

fn bias(n: usize) -> i32 {
    if n < 1240 {
        107
    } else if n < 33900 {
        1131
    } else {
        32768
    }
}

/// Run one Type 2 charstring (with its subroutine INDEXes) and return the path.
pub(crate) fn charstring_outline(d: &[u8], cs: &[u8], gsubrs: Index, lsubrs: Option<Index>) -> Option<Vec<PathOp>> {
    let mut it = Interp {
        d,
        gsubrs,
        gbias: bias(gsubrs.len()),
        lsubrs,
        lbias: lsubrs.map_or(107, |l| bias(l.len())),
        stack: Vec::new(),
        x: 0.0,
        y: 0.0,
        nstems: 0,
        seen_width: false,
        open: false,
        out: Vec::new(),
        budget: 200_000,
        trans: [0.0; 32],
    };
    it.run(cs, 0).ok()?;
    it.close();
    Some(it.out)
}

const MAX_STACK: usize = 513;

struct Interp<'a> {
    d: &'a [u8],
    gsubrs: Index,
    gbias: i32,
    lsubrs: Option<Index>,
    lbias: i32,
    stack: Vec<f32>,
    x: f32,
    y: f32,
    nstems: usize,
    seen_width: bool,
    open: bool,
    out: Vec<PathOp>,
    budget: u32,
    trans: [f32; 32],
}

impl<'a> Interp<'a> {
    fn push(&mut self, v: f32) -> Result<(), ()> {
        if self.stack.len() >= MAX_STACK {
            return Err(());
        }
        self.stack.push(v);
        Ok(())
    }

    /// Take the operands of a stack-clearing operator, dropping a leading width if present.
    fn args(&mut self, has_width: bool) -> Vec<f32> {
        let mut a = std::mem::take(&mut self.stack);
        if !self.seen_width {
            self.seen_width = true;
            if has_width && !a.is_empty() {
                a.remove(0);
            }
        }
        a
    }

    fn close(&mut self) {
        if self.open {
            self.out.push(PathOp::Close);
            self.open = false;
        }
    }

    fn move_to(&mut self, dx: f32, dy: f32) {
        self.close();
        self.x += dx;
        self.y += dy;
        self.out.push(PathOp::MoveTo(self.x, self.y));
        self.open = true;
    }

    fn ensure_open(&mut self) {
        if !self.open {
            self.out.push(PathOp::MoveTo(self.x, self.y));
            self.open = true;
        }
    }

    fn line(&mut self, dx: f32, dy: f32) {
        self.ensure_open();
        self.x += dx;
        self.y += dy;
        self.out.push(PathOp::LineTo(self.x, self.y));
    }

    fn curve(&mut self, dx1: f32, dy1: f32, dx2: f32, dy2: f32, dx3: f32, dy3: f32) {
        self.ensure_open();
        let x1 = self.x + dx1;
        let y1 = self.y + dy1;
        let x2 = x1 + dx2;
        let y2 = y1 + dy2;
        self.x = x2 + dx3;
        self.y = y2 + dy3;
        self.out.push(PathOp::CubicTo(x1, y1, x2, y2, self.x, self.y));
    }

    fn pop(&mut self) -> Result<f32, ()> {
        self.stack.pop().ok_or(())
    }

    /// Returns Ok(true) when endchar was reached.
    fn run(&mut self, cs: &[u8], depth: u8) -> Result<bool, ()> {
        if depth > 10 {
            return Err(());
        }
        let mut i = 0usize;
        while i < cs.len() {
            if self.budget == 0 {
                return Err(());
            }
            self.budget -= 1;
            let b0 = cs[i];
            i += 1;
            match b0 {
                32..=246 => self.push(b0 as f32 - 139.0)?,
                247..=250 => {
                    let b1 = *cs.get(i).ok_or(())? as f32;
                    i += 1;
                    self.push((b0 as f32 - 247.0) * 256.0 + b1 + 108.0)?;
                }
                251..=254 => {
                    let b1 = *cs.get(i).ok_or(())? as f32;
                    i += 1;
                    self.push(-(b0 as f32 - 251.0) * 256.0 - b1 - 108.0)?;
                }
                28 => {
                    let v = rdi16(cs, i).ok_or(())?;
                    i += 2;
                    self.push(v as f32)?;
                }
                255 => {
                    let b = sub(cs, i, 4).ok_or(())?;
                    i += 4;
                    self.push(i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f32 / 65536.0)?;
                }
                1 | 3 | 18 | 23 => {
                    let w = self.stack.len() % 2 == 1;
                    let a = self.args(w);
                    self.nstems += a.len() / 2;
                }
                19 | 20 => {
                    let w = self.stack.len() % 2 == 1;
                    let a = self.args(w);
                    self.nstems += a.len() / 2;
                    i += (self.nstems + 7) / 8;
                }
                21 => {
                    let w = self.stack.len() > 2;
                    let a = self.args(w);
                    if a.len() < 2 {
                        return Err(());
                    }
                    self.move_to(a[0], a[1]);
                }
                22 => {
                    let w = self.stack.len() > 1;
                    let a = self.args(w);
                    self.move_to(*a.first().ok_or(())?, 0.0);
                }
                4 => {
                    let w = self.stack.len() > 1;
                    let a = self.args(w);
                    self.move_to(0.0, *a.first().ok_or(())?);
                }
                5 => {
                    let a = self.args(false);
                    for p in a.chunks_exact(2) {
                        self.line(p[0], p[1]);
                    }
                }
                6 | 7 => {
                    let a = self.args(false);
                    let mut horiz = b0 == 6;
                    for &v in &a {
                        if horiz {
                            self.line(v, 0.0);
                        } else {
                            self.line(0.0, v);
                        }
                        horiz = !horiz;
                    }
                }
                8 => {
                    let a = self.args(false);
                    for c in a.chunks_exact(6) {
                        self.curve(c[0], c[1], c[2], c[3], c[4], c[5]);
                    }
                }
                24 => {
                    let a = self.args(false);
                    let mut k = 0;
                    while a.len() - k >= 8 {
                        self.curve(a[k], a[k + 1], a[k + 2], a[k + 3], a[k + 4], a[k + 5]);
                        k += 6;
                    }
                    if k + 2 <= a.len() {
                        self.line(a[k], a[k + 1]);
                    }
                }
                25 => {
                    let a = self.args(false);
                    let mut k = 0;
                    while a.len() - k >= 8 {
                        self.line(a[k], a[k + 1]);
                        k += 2;
                    }
                    if k + 6 <= a.len() {
                        self.curve(a[k], a[k + 1], a[k + 2], a[k + 3], a[k + 4], a[k + 5]);
                    }
                }
                26 => {
                    let a = self.args(false);
                    let mut k = 0;
                    let mut dx1 = 0.0;
                    if a.len() % 2 == 1 {
                        dx1 = a[0];
                        k = 1;
                    }
                    while k + 4 <= a.len() {
                        self.curve(dx1, a[k], a[k + 1], a[k + 2], 0.0, a[k + 3]);
                        dx1 = 0.0;
                        k += 4;
                    }
                }
                27 => {
                    let a = self.args(false);
                    let mut k = 0;
                    let mut dy1 = 0.0;
                    if a.len() % 2 == 1 {
                        dy1 = a[0];
                        k = 1;
                    }
                    while k + 4 <= a.len() {
                        self.curve(a[k], dy1, a[k + 1], a[k + 2], a[k + 3], 0.0);
                        dy1 = 0.0;
                        k += 4;
                    }
                }
                30 | 31 => {
                    let a = self.args(false);
                    let mut horiz = b0 == 31;
                    let mut k = 0;
                    while k + 4 <= a.len() {
                        let last = if a.len() - k == 5 { a[k + 4] } else { 0.0 };
                        if horiz {
                            self.curve(a[k], 0.0, a[k + 1], a[k + 2], last, a[k + 3]);
                        } else {
                            self.curve(0.0, a[k], a[k + 1], a[k + 2], a[k + 3], last);
                        }
                        k += if a.len() - k == 5 { 5 } else { 4 };
                        horiz = !horiz;
                    }
                }
                10 | 29 => {
                    let idx = self.pop()? as i32;
                    let (subrs, b) = if b0 == 10 { (self.lsubrs.ok_or(())?, self.lbias) } else { (self.gsubrs, self.gbias) };
                    let n = idx + b;
                    if n < 0 {
                        return Err(());
                    }
                    let d = self.d;
                    let s = subrs.get(d, n as usize).ok_or(())?;
                    if self.run(s, depth + 1)? {
                        return Ok(true);
                    }
                }
                11 => return Ok(false),
                14 => {
                    let w = self.stack.len() == 1 || self.stack.len() == 5;
                    let _ = self.args(w);
                    self.close();
                    return Ok(true);
                }
                12 => {
                    let b1 = *cs.get(i).ok_or(())?;
                    i += 1;
                    self.escape(b1)?;
                }
                _ => self.stack.clear(),
            }
        }
        Ok(false)
    }

    fn escape(&mut self, op: u8) -> Result<(), ()> {
        match op {
            35 => {
                let a = self.args(false);
                if a.len() < 12 {
                    return Err(());
                }
                self.curve(a[0], a[1], a[2], a[3], a[4], a[5]);
                self.curve(a[6], a[7], a[8], a[9], a[10], a[11]);
            }
            34 => {
                let a = self.args(false);
                if a.len() < 7 {
                    return Err(());
                }
                self.curve(a[0], 0.0, a[1], a[2], a[3], 0.0);
                self.curve(a[4], 0.0, a[5], -a[2], a[6], 0.0);
            }
            36 => {
                let a = self.args(false);
                if a.len() < 9 {
                    return Err(());
                }
                self.curve(a[0], a[1], a[2], a[3], a[4], 0.0);
                self.curve(a[5], 0.0, a[6], a[7], a[8], -(a[1] + a[3] + a[7]));
            }
            37 => {
                let a = self.args(false);
                if a.len() < 11 {
                    return Err(());
                }
                let dx: f32 = a[0] + a[2] + a[4] + a[6] + a[8];
                let dy: f32 = a[1] + a[3] + a[5] + a[7] + a[9];
                let (sx, sy) = (self.x, self.y);
                self.curve(a[0], a[1], a[2], a[3], a[4], a[5]);
                let x1 = self.x + a[6];
                let y1 = self.y + a[7];
                let x2 = x1 + a[8];
                let y2 = y1 + a[9];
                let (ex, ey) = if dx.abs() > dy.abs() { (x2 + a[10], sy) } else { (sx, y2 + a[10]) };
                self.ensure_open();
                self.x = ex;
                self.y = ey;
                self.out.push(PathOp::CubicTo(x1, y1, x2, y2, ex, ey));
            }
            // Arithmetic and storage operators (rare in fonts, cheap to support).
            9 => {
                let v = self.pop()?;
                self.push(v.abs())?;
            }
            10 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(a + b)?;
            }
            11 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(a - b)?;
            }
            12 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(if b == 0.0 { 0.0 } else { a / b })?;
            }
            14 => {
                let v = self.pop()?;
                self.push(-v)?;
            }
            18 => {
                self.pop()?;
            }
            24 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(a * b)?;
            }
            26 => {
                let v = self.pop()?;
                self.push(v.max(0.0).sqrt())?;
            }
            27 => {
                let v = *self.stack.last().ok_or(())?;
                self.push(v)?;
            }
            28 => {
                let n = self.stack.len();
                if n < 2 {
                    return Err(());
                }
                self.stack.swap(n - 1, n - 2);
            }
            3 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(if a != 0.0 && b != 0.0 { 1.0 } else { 0.0 })?;
            }
            4 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(if a != 0.0 || b != 0.0 { 1.0 } else { 0.0 })?;
            }
            5 => {
                let a = self.pop()?;
                self.push(if a == 0.0 { 1.0 } else { 0.0 })?;
            }
            15 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(if a == b { 1.0 } else { 0.0 })?;
            }
            22 => {
                let v2 = self.pop()?;
                let v1 = self.pop()?;
                let s2 = self.pop()?;
                let s1 = self.pop()?;
                self.push(if v1 <= v2 { s1 } else { s2 })?;
            }
            20 => {
                let i = self.pop()? as i32;
                let v = self.pop()?;
                if (0..32).contains(&i) {
                    self.trans[i as usize] = v;
                }
            }
            21 => {
                let i = self.pop()? as i32;
                let v = if (0..32).contains(&i) { self.trans[i as usize] } else { 0.0 };
                self.push(v)?;
            }
            29 => {
                let i = self.pop()?;
                let n = self.stack.len();
                if n == 0 {
                    return Err(());
                }
                let k = if i < 0.0 { 0 } else { (i as usize).min(n - 1) };
                let v = self.stack[n - 1 - k];
                self.push(v)?;
            }
            30 => {
                let j = self.pop()? as i64;
                let n = self.pop()? as i64;
                let len = self.stack.len() as i64;
                if n <= 0 || n > len {
                    return Err(());
                }
                let start = (len - n) as usize;
                let shift = j.rem_euclid(n) as usize;
                self.stack[start..].rotate_right(shift);
            }
            23 => self.push(0.5)?, // random: deterministic stand-in
            _ => self.stack.clear(),
        }
        Ok(())
    }
}
