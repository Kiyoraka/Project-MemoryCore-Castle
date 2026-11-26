//! TrueType outlines: `loca` + `glyf`, simple glyphs (quadratic contours with implied on-curve
//! points) and composite glyphs (components placed by offset and 2x2 transform).

use super::read::{rd16, rd32, rd8, rdi16};
use super::PathOp;

const MAX_DEPTH: u8 = 8;

#[derive(Clone, Copy)]
pub(crate) struct Glyf<'a> {
    pub glyf: &'a [u8],
    pub loca: &'a [u8],
    pub long: bool,
    pub num_glyphs: u16,
}

/// Affine transform: x' = a*x + c*y + e, y' = b*x + d*y + f.
#[derive(Clone, Copy)]
struct Xf {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Xf {
    const ID: Xf = Xf { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
    /// self after inner: p -> self(inner(p)).
    fn then_outer(inner: &Xf, outer: &Xf) -> Xf {
        Xf {
            a: outer.a * inner.a + outer.c * inner.b,
            b: outer.b * inner.a + outer.d * inner.b,
            c: outer.a * inner.c + outer.c * inner.d,
            d: outer.b * inner.c + outer.d * inner.d,
            e: outer.a * inner.e + outer.c * inner.f + outer.e,
            f: outer.b * inner.e + outer.d * inner.f + outer.f,
        }
    }
}

/// A parsed component reference inside a composite glyph.
pub(crate) struct Component {
    pub gid: u16,
    /// Byte position of the glyph-index field inside the glyph's data (for renumbering).
    pub gid_pos: usize,
}

impl<'a> Glyf<'a> {
    /// Byte range of a glyph inside `glyf` (empty range = empty glyph).
    pub(crate) fn range(&self, gid: u16) -> Option<(usize, usize)> {
        if gid >= self.num_glyphs {
            return None;
        }
        let i = gid as usize;
        let (a, b) = if self.long {
            (rd32(self.loca, 4 * i)? as usize, rd32(self.loca, 4 * i + 4)? as usize)
        } else {
            (rd16(self.loca, 2 * i)? as usize * 2, rd16(self.loca, 2 * i + 2)? as usize * 2)
        };
        if a > b || b > self.glyf.len() {
            // Tolerate a final offset slightly past the end by clamping; reject reversed ranges.
            if a <= b && a <= self.glyf.len() {
                return Some((a, self.glyf.len()));
            }
            return None;
        }
        Some((a, b))
    }

    pub(crate) fn data(&self, gid: u16) -> Option<&'a [u8]> {
        let (a, b) = self.range(gid)?;
        self.glyf.get(a..b)
    }

    pub(crate) fn outline(&self, gid: u16) -> Option<Vec<PathOp>> {
        let mut out = Vec::new();
        let mut budget: usize = 1 << 20;
        self.outline_into(gid, &Xf::ID, 0, &mut out, &mut budget)?;
        Some(out)
    }

    fn outline_into(&self, gid: u16, xf: &Xf, depth: u8, out: &mut Vec<PathOp>, budget: &mut usize) -> Option<()> {
        if depth > MAX_DEPTH || *budget == 0 {
            return None;
        }
        *budget -= 1;
        let g = self.data(gid)?;
        if g.is_empty() {
            return Some(());
        }
        let n = rdi16(g, 0)?;
        if n >= 0 {
            simple(g, n as usize, xf, out, budget)
        } else {
            let mut pos = 10usize;
            loop {
                let flags = rd16(g, pos)?;
                let child = rd16(g, pos + 2)?;
                pos += 4;
                let (a1, a2) = if flags & 0x0001 != 0 {
                    let v = (rdi16(g, pos)? as f32, rdi16(g, pos + 2)? as f32);
                    pos += 4;
                    v
                } else {
                    let v = if flags & 0x0002 != 0 {
                        (rd8(g, pos)? as i8 as f32, rd8(g, pos + 1)? as i8 as f32)
                    } else {
                        (rd8(g, pos)? as f32, rd8(g, pos + 1)? as f32)
                    };
                    pos += 2;
                    v
                };
                let f2 = |o: usize| rdi16(g, o).map(|v| v as f32 / 16384.0);
                let (mut a, mut b, mut c, mut d) = (1.0f32, 0.0f32, 0.0f32, 1.0f32);
                if flags & 0x0008 != 0 {
                    a = f2(pos)?;
                    d = a;
                    pos += 2;
                } else if flags & 0x0040 != 0 {
                    a = f2(pos)?;
                    d = f2(pos + 2)?;
                    pos += 4;
                } else if flags & 0x0080 != 0 {
                    a = f2(pos)?;
                    b = f2(pos + 2)?;
                    c = f2(pos + 4)?;
                    d = f2(pos + 6)?;
                    pos += 8;
                }
                // Point-matching placement (args are point numbers) is approximated as no offset.
                let (mut dx, mut dy) = if flags & 0x0002 != 0 { (a1, a2) } else { (0.0, 0.0) };
                if flags & 0x0800 != 0 && flags & 0x1000 == 0 {
                    let (sx, sy) = (a * dx + c * dy, b * dx + d * dy);
                    dx = sx;
                    dy = sy;
                }
                let local = Xf { a, b, c, d, e: dx, f: dy };
                let combined = Xf::then_outer(&local, xf);
                // A broken component is skipped rather than failing the whole glyph.
                let _ = self.outline_into(child, &combined, depth + 1, out, budget);
                if flags & 0x0020 == 0 {
                    break;
                }
            }
            Some(())
        }
    }
}

/// Components of a composite glyph (empty for simple or broken glyphs).
pub(crate) fn components(g: &[u8]) -> Vec<Component> {
    let mut out = Vec::new();
    if rdi16(g, 0).map_or(true, |n| n >= 0) {
        return out;
    }
    let mut pos = 10usize;
    for _ in 0..1024 {
        let (Some(flags), Some(gid)) = (rd16(g, pos), rd16(g, pos + 2)) else { break };
        out.push(Component { gid, gid_pos: pos + 2 });
        pos += 4;
        pos += if flags & 0x0001 != 0 { 4 } else { 2 };
        if flags & 0x0008 != 0 {
            pos += 2;
        } else if flags & 0x0040 != 0 {
            pos += 4;
        } else if flags & 0x0080 != 0 {
            pos += 8;
        }
        if flags & 0x0020 == 0 {
            break;
        }
    }
    out
}

fn simple(g: &[u8], ncont: usize, xf: &Xf, out: &mut Vec<PathOp>, budget: &mut usize) -> Option<()> {
    if ncont == 0 {
        return Some(());
    }
    let mut ends = Vec::with_capacity(ncont);
    let mut prev: i32 = -1;
    for i in 0..ncont {
        let e = rd16(g, 10 + 2 * i)? as i32;
        if e < prev {
            return None;
        }
        ends.push(e as usize);
        prev = e;
    }
    let npts = prev as usize + 1;
    if npts > *budget {
        return None;
    }
    *budget -= npts.min(*budget);
    let ilen = rd16(g, 10 + 2 * ncont)? as usize;
    let mut pos = 12 + 2 * ncont + ilen;
    let mut flags = Vec::with_capacity(npts);
    while flags.len() < npts {
        let f = rd8(g, pos)?;
        pos += 1;
        flags.push(f);
        if f & 0x08 != 0 {
            let r = rd8(g, pos)?;
            pos += 1;
            for _ in 0..r {
                if flags.len() >= npts {
                    break;
                }
                flags.push(f);
            }
        }
    }
    let mut xs = Vec::with_capacity(npts);
    let mut v: i32 = 0;
    for &f in &flags {
        if f & 0x02 != 0 {
            let d = rd8(g, pos)? as i32;
            pos += 1;
            v += if f & 0x10 != 0 { d } else { -d };
        } else if f & 0x10 == 0 {
            v += rdi16(g, pos)? as i32;
            pos += 2;
        }
        xs.push(v);
    }
    let mut ys = Vec::with_capacity(npts);
    v = 0;
    for &f in &flags {
        if f & 0x04 != 0 {
            let d = rd8(g, pos)? as i32;
            pos += 1;
            v += if f & 0x20 != 0 { d } else { -d };
        } else if f & 0x20 == 0 {
            v += rdi16(g, pos)? as i32;
            pos += 2;
        }
        ys.push(v);
    }
    let mut start = 0usize;
    let mut pts: Vec<(f32, f32, bool)> = Vec::new();
    for &end in &ends {
        if end < start {
            continue;
        }
        pts.clear();
        for i in start..=end {
            let (x, y) = xf.apply(xs[i] as f32, ys[i] as f32);
            pts.push((x, y, flags[i] & 0x01 != 0));
        }
        emit_contour(&pts, out);
        start = end + 1;
    }
    Some(())
}

/// Turn one quadratic contour into path ops, inserting the implied on-curve midpoints.
pub(crate) fn emit_contour(pts: &[(f32, f32, bool)], out: &mut Vec<PathOp>) {
    let n = pts.len();
    if n == 0 {
        return;
    }
    let mid = |a: (f32, f32), b: (f32, f32)| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
    let first_on = pts.iter().position(|p| p.2);
    let (start, order): ((f32, f32), Vec<usize>) = match first_on {
        Some(k) => ((pts[k].0, pts[k].1), (1..=n).map(|j| (k + j) % n).collect()),
        None => (mid((pts[n - 1].0, pts[n - 1].1), (pts[0].0, pts[0].1)), (0..n).collect()),
    };
    out.push(PathOp::MoveTo(start.0, start.1));
    let mut ctrl: Option<(f32, f32)> = None;
    for idx in order {
        let p = (pts[idx].0, pts[idx].1);
        if pts[idx].2 {
            match ctrl.take() {
                Some(c) => out.push(PathOp::QuadTo(c.0, c.1, p.0, p.1)),
                None => out.push(PathOp::LineTo(p.0, p.1)),
            }
        } else {
            if let Some(c) = ctrl {
                let m = mid(c, p);
                out.push(PathOp::QuadTo(c.0, c.1, m.0, m.1));
            }
            ctrl = Some(p);
        }
    }
    if let Some(c) = ctrl {
        out.push(PathOp::QuadTo(c.0, c.1, start.0, start.1));
    }
    if let Some(PathOp::LineTo(x, y)) = out.last() {
        if *x == start.0 && *y == start.1 {
            out.pop();
        }
    }
    out.push(PathOp::Close);
}
