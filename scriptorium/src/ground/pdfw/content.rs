//! A display list as a page content stream (and the page's resource dictionary and links).
//!
//! The stream opens with `1 0 0 -1 0 H cm`, so the display list's y-down coordinates are used as
//! they are; text and images are flipped back upright by their own matrices.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::embed::glyph_width;
use super::obj::{num, nums, push_num};
use super::{LinkRec, LinkTarget, PageResources, PdfWriter};
use crate::ground::display::{path_bounds, Color, DisplayList, GradientStop, Item, LineCap, LineJoin, Matrix, Paint, PathOp, PlacedGlyph, Rect, Stroke};

/// What a `q` saves that we track to avoid repeating operators.
#[derive(Clone, PartialEq)]
struct GState {
    fill: Option<[u8; 3]>,
    stroke: Option<[u8; 3]>,
    /// Fill and stroke alpha in thousandths.
    alpha: (u16, u16),
    width: Option<f64>,
    cap: Option<u8>,
    join: Option<u8>,
    miter: Option<f64>,
    dash: Option<String>,
    /// The current transform in display coordinates (the page flip not included).
    m: Matrix,
}

impl GState {
    fn initial() -> GState {
        GState { fill: None, stroke: None, alpha: (1000, 1000), width: None, cap: None, join: None, miter: None, dash: None, m: Matrix::IDENTITY }
    }
}

#[derive(Default)]
struct ResDict {
    fonts: BTreeMap<u32, ()>,
    xobjects: BTreeMap<u32, ()>,
    gstates: BTreeMap<u32, ()>,
    shadings: BTreeMap<u32, ()>,
}

impl ResDict {
    fn to_pdf(&self) -> String {
        let mut s = String::from("<< /ProcSet [/PDF /Text /ImageB /ImageC]");
        let mut cat = |key: &str, prefix: &str, ids: &BTreeMap<u32, ()>| {
            if ids.is_empty() {
                return;
            }
            let _ = write!(s, " /{key} <<");
            for id in ids.keys() {
                let _ = write!(s, " /{prefix}{id} {id} 0 R");
            }
            s.push_str(" >>");
        };
        cat("Font", "F", &self.fonts);
        cat("XObject", "Im", &self.xobjects);
        cat("ExtGState", "GS", &self.gstates);
        cat("Shading", "Sh", &self.shadings);
        s.push_str(" >>");
        s
    }
}

struct Builder<'a> {
    res: &'a PageResources,
    out: String,
    gs: GState,
    stack: Vec<GState>,
    opacity: f64,
    opacity_stack: Vec<f64>,
    used: ResDict,
    links: Vec<LinkRec>,
    page_h: f64,
}

/// The content stream, the resource dictionary and the links of one page.
pub(super) fn page_content(w: &mut PdfWriter, list: &DisplayList, res: &PageResources, page_h: f64) -> (String, String, Vec<LinkRec>) {
    let mut b = Builder {
        res,
        out: String::new(),
        gs: GState::initial(),
        stack: Vec::new(),
        opacity: 1.0,
        opacity_stack: Vec::new(),
        used: ResDict::default(),
        links: Vec::new(),
        page_h,
    };
    let _ = writeln!(b.out, "1 0 0 -1 0 {} cm", num(page_h));
    for item in &list.items {
        b.item(w, item);
    }
    while !b.stack.is_empty() {
        b.pop_q();
    }
    (b.out, b.used.to_pdf(), b.links)
}

/// The path as operators (`m l c h`, or `re` for a plain axis-aligned rectangle), or empty.
pub(super) fn path_ops(ops: &[PathOp]) -> String {
    if let Some(r) = as_rect(ops) {
        return format!("{} re\n", nums(&r));
    }
    let mut s = String::new();
    let (mut cx, mut cy) = (0.0f64, 0.0f64);
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    let mut open = false;
    for op in ops {
        match *op {
            PathOp::MoveTo(x, y) => {
                let _ = writeln!(s, "{} m", nums(&[x, y]));
                (cx, cy, sx, sy) = (x, y, x, y);
                open = true;
            }
            PathOp::LineTo(x, y) => {
                if !open {
                    let _ = writeln!(s, "{} m", nums(&[x, y]));
                    (sx, sy) = (x, y);
                    open = true;
                } else {
                    let _ = writeln!(s, "{} l", nums(&[x, y]));
                }
                (cx, cy) = (x, y);
            }
            PathOp::QuadTo(qx, qy, x, y) => {
                if !open {
                    let _ = writeln!(s, "{} m", nums(&[cx, cy]));
                    (sx, sy) = (cx, cy);
                    open = true;
                }
                let c1 = (cx + 2.0 / 3.0 * (qx - cx), cy + 2.0 / 3.0 * (qy - cy));
                let c2 = (x + 2.0 / 3.0 * (qx - x), y + 2.0 / 3.0 * (qy - y));
                let _ = writeln!(s, "{} c", nums(&[c1.0, c1.1, c2.0, c2.1, x, y]));
                (cx, cy) = (x, y);
            }
            PathOp::CubicTo(x1, y1, x2, y2, x, y) => {
                if !open {
                    let _ = writeln!(s, "{} m", nums(&[cx, cy]));
                    (sx, sy) = (cx, cy);
                    open = true;
                }
                let _ = writeln!(s, "{} c", nums(&[x1, y1, x2, y2, x, y]));
                (cx, cy) = (x, y);
            }
            PathOp::Close => {
                if open {
                    s.push_str("h\n");
                    (cx, cy) = (sx, sy);
                    open = false;
                }
            }
        }
    }
    s
}

/// `x y w h` when the path is exactly one closed axis-aligned rectangle.
fn as_rect(ops: &[PathOp]) -> Option<[f64; 4]> {
    let mut pts = Vec::with_capacity(5);
    let (last, body) = ops.split_last()?;
    if *last != PathOp::Close {
        return None;
    }
    for (i, op) in body.iter().enumerate() {
        match (i, *op) {
            (0, PathOp::MoveTo(x, y)) => pts.push((x, y)),
            (1..=4, PathOp::LineTo(x, y)) => pts.push((x, y)),
            _ => return None,
        }
    }
    if pts.len() == 5 {
        if pts[4] != pts[0] {
            return None;
        }
        pts.pop();
    }
    if pts.len() != 4 {
        return None;
    }
    let (p0, p1, p2, p3) = (pts[0], pts[1], pts[2], pts[3]);
    let horiz_first = p0.1 == p1.1 && p1.0 == p2.0 && p2.1 == p3.1 && p3.0 == p0.0;
    if !horiz_first || p0.0 == p1.0 || p1.1 == p2.1 {
        return None;
    }
    // `re` draws (x,y) -> (x+w,y) -> (x+w,y+h) -> (x,y+h): the same order as M L L L with signed w, h.
    Some([p0.0, p0.1, p1.0 - p0.0, p2.1 - p1.1])
}

fn alpha_k(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 1000.0).round() as u16
}

impl<'a> Builder<'a> {
    fn item(&mut self, w: &mut PdfWriter, item: &Item) {
        match item {
            Item::Path { ops, fill, stroke } => self.path(w, ops, fill.as_ref(), stroke.as_ref()),
            Item::Glyphs { font, size, color, glyphs } => self.glyphs(w, *font, *size, *color, glyphs),
            Item::Image { image, rect } => {
                if self.opacity <= 0.0 {
                    return;
                }
                let Some(img) = self.res.image(*image).cloned() else { return };
                let Some(id) = w.image_obj(&img) else { return };
                self.used.xobjects.insert(id, ());
                self.push_q();
                self.set_alpha(w, Some(self.opacity), None);
                let _ = writeln!(self.out, "{} cm /Im{} Do", nums(&[rect.w, 0.0, 0.0, -rect.h, rect.x, rect.y + rect.h]), id);
                self.pop_q();
            }
            Item::PushClip(ops) => {
                self.push_q();
                let d = path_ops(ops);
                if d.is_empty() {
                    self.out.push_str("0 0 0 0 re W n\n");
                } else {
                    self.out.push_str(&d);
                    self.out.push_str("W n\n");
                }
            }
            Item::PushTransform(m) => {
                self.push_q();
                let _ = writeln!(self.out, "{} cm", nums(&[m.a, m.b, m.c, m.d, m.e, m.f]));
                self.gs.m = self.gs.m.multiply(m);
            }
            Item::PopClip | Item::PopTransform => {
                // Close the most recent `q`; an unmatched pop is ignored.
                if !self.stack.is_empty() {
                    self.pop_q();
                }
            }
            Item::PushOpacity(a) => {
                self.opacity_stack.push(self.opacity);
                self.opacity *= if a.is_finite() { a.clamp(0.0, 1.0) } else { 1.0 };
            }
            Item::PopOpacity => self.opacity = self.opacity_stack.pop().unwrap_or(1.0),
            Item::Link { rect, target } => {
                let Some(target) = parse_target(target) else { return };
                let r = self.gs.m.apply_rect(rect);
                self.links.push(LinkRec { rect: [r.x, self.page_h - r.bottom(), r.right(), self.page_h - r.y], target });
            }
        }
    }

    fn push_q(&mut self) {
        self.out.push_str("q\n");
        self.stack.push(self.gs.clone());
    }

    fn pop_q(&mut self) {
        if let Some(g) = self.stack.pop() {
            self.out.push_str("Q\n");
            self.gs = g;
        }
    }

    /// Set fill and/or stroke alpha (None keeps that one as it is) through an ExtGState.
    fn set_alpha(&mut self, w: &mut PdfWriter, fill: Option<f64>, stroke: Option<f64>) {
        let want = (fill.map_or(self.gs.alpha.0, alpha_k), stroke.map_or(self.gs.alpha.1, alpha_k));
        if want == self.gs.alpha {
            return;
        }
        let body = format!("<< /Type /ExtGState /ca {} /CA {} >>", num(want.0 as f64 / 1000.0), num(want.1 as f64 / 1000.0));
        let id = w.intern(body.into_bytes());
        self.used.gstates.insert(id, ());
        let _ = writeln!(self.out, "/GS{id} gs");
        self.gs.alpha = want;
    }

    fn set_fill(&mut self, c: Color) {
        let rgb = [c.r, c.g, c.b];
        if self.gs.fill != Some(rgb) {
            let _ = writeln!(self.out, "{} rg", rgb_nums(rgb));
            self.gs.fill = Some(rgb);
        }
    }

    fn set_stroke(&mut self, s: &Stroke) {
        let rgb = [s.color.r, s.color.g, s.color.b];
        if self.gs.stroke != Some(rgb) {
            let _ = writeln!(self.out, "{} RG", rgb_nums(rgb));
            self.gs.stroke = Some(rgb);
        }
        if self.gs.width != Some(s.width) {
            let _ = writeln!(self.out, "{} w", num(s.width));
            self.gs.width = Some(s.width);
        }
        let cap = match s.cap {
            LineCap::Butt => 0,
            LineCap::Round => 1,
            LineCap::Square => 2,
        };
        if self.gs.cap != Some(cap) {
            let _ = writeln!(self.out, "{cap} J");
            self.gs.cap = Some(cap);
        }
        let join = match s.join {
            LineJoin::Miter => 0,
            LineJoin::Round => 1,
            LineJoin::Bevel => 2,
        };
        if self.gs.join != Some(join) {
            let _ = writeln!(self.out, "{join} j");
            self.gs.join = Some(join);
        }
        let ml = if s.miter_limit.is_finite() { s.miter_limit.max(1.0) } else { 10.0 };
        if self.gs.miter != Some(ml) {
            let _ = writeln!(self.out, "{} M", num(ml));
            self.gs.miter = Some(ml);
        }
        let solid = s.dash.is_empty() || s.dash.iter().any(|d| !d.is_finite() || *d < 0.0) || s.dash.iter().all(|d| *d == 0.0);
        let dash = if solid { "[] 0 d".to_string() } else { format!("[{}] {} d", nums(&s.dash), num(s.dash_offset)) };
        let current = self.gs.dash.clone().unwrap_or_else(|| "[] 0 d".to_string());
        if current != dash {
            let _ = writeln!(self.out, "{dash}");
        }
        self.gs.dash = Some(dash);
    }

    fn path(&mut self, w: &mut PdfWriter, ops: &[PathOp], fill: Option<&Paint>, stroke: Option<&Stroke>) {
        let d = path_ops(ops);
        if d.is_empty() || self.opacity <= 0.0 {
            return;
        }
        let mut solid: Option<Color> = None;
        match fill {
            Some(Paint::Solid(c)) if c.a > 0 => solid = Some(*c),
            Some(p @ (Paint::Linear { .. } | Paint::Radial { .. })) => self.gradient(w, ops, &d, p),
            _ => {}
        }
        let stroke = stroke.filter(|s| s.width > 0.0 && s.width.is_finite() && s.color.a > 0);
        let fa = solid.map(|c| self.opacity * c.a as f64 / 255.0);
        let sa = stroke.map(|s| self.opacity * s.color.a as f64 / 255.0);
        if solid.is_none() && stroke.is_none() {
            return;
        }
        self.set_alpha(w, fa, sa);
        if let Some(c) = solid {
            self.set_fill(c);
        }
        if let Some(s) = stroke {
            self.set_stroke(s);
        }
        self.out.push_str(&d);
        self.out.push_str(match (solid.is_some(), stroke.is_some()) {
            (true, true) => "B\n",
            (true, false) => "f\n",
            _ => "S\n",
        });
    }

    /// A gradient fill: the path as a clip, the shading painted through it.
    fn gradient(&mut self, w: &mut PdfWriter, ops: &[PathOp], d: &str, paint: &Paint) {
        let (kind, coords, stops) = match paint {
            Paint::Linear { x0, y0, x1, y1, stops } => (2, vec![*x0, *y0, *x1, *y1], stops),
            Paint::Radial { fx, fy, fr, cx, cy, r, stops } => (3, vec![*fx, *fy, fr.max(0.0), *cx, *cy, r.max(0.0)], stops),
            Paint::Solid(_) => return,
        };
        let stops = normalize_stops(stops);
        if stops.is_empty() || coords.iter().any(|v| !v.is_finite()) {
            return;
        }
        let color_fn = function(&stops, |c| vec![c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0]);
        let sh = w.intern(
            format!("<< /ShadingType {kind} /ColorSpace /DeviceRGB /Coords [{}] /Function {color_fn} /Extend [true true] >>", nums(&coords)).into_bytes(),
        );
        self.used.shadings.insert(sh, ());
        let uniform = stops.iter().all(|s| s.color.a == stops[0].color.a);
        if uniform && stops[0].color.a == 0 {
            return;
        }
        self.push_q();
        self.out.push_str(d);
        self.out.push_str("W n\n");
        if uniform {
            self.set_alpha(w, Some(self.opacity * stops[0].color.a as f64 / 255.0), None);
        } else {
            // Alpha varies along the gradient: a luminosity soft mask painted with the same geometry.
            let alpha_fn = function(&stops, |c| vec![c.a as f64 / 255.0]);
            let ash = w.intern(
                format!("<< /ShadingType {kind} /ColorSpace /DeviceGray /Coords [{}] /Function {alpha_fn} /Extend [true true] >>", nums(&coords)).into_bytes(),
            );
            let bb = path_bounds(ops).map(|r| r.inflate(1.0)).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
            let form_content = format!("{d}W n\n/Sh{ash} sh\n");
            let form = w.new_stream(
                &format!(
                    "/Type /XObject /Subtype /Form /BBox [{}] /Group << /S /Transparency /CS /DeviceGray >> /Resources << /Shading << /Sh{ash} {ash} 0 R >> >>",
                    nums(&[bb.x, bb.y, bb.right(), bb.bottom()])
                ),
                form_content.as_bytes(),
                true,
            );
            let gs = w.intern(
                format!(
                    "<< /Type /ExtGState /ca {} /CA {} /SMask << /Type /Mask /S /Luminosity /G {form} 0 R >> >>",
                    num(alpha_k(self.opacity) as f64 / 1000.0),
                    num(self.gs.alpha.1 as f64 / 1000.0)
                )
                .into_bytes(),
            );
            self.used.gstates.insert(gs, ());
            let _ = writeln!(self.out, "/GS{gs} gs");
            // The soft mask is in force until the `Q`; the tracked alpha is restored there too.
            self.gs.alpha = (alpha_k(self.opacity), self.gs.alpha.1);
        }
        let _ = writeln!(self.out, "/Sh{sh} sh");
        self.pop_q();
    }

    fn glyphs(&mut self, w: &mut PdfWriter, font_id: u32, size: f64, color: Color, glyphs: &[PlacedGlyph]) {
        if glyphs.is_empty() || !size.is_finite() || size == 0.0 || color.a == 0 || self.opacity <= 0.0 {
            return;
        }
        let Some(font) = self.res.font(font_id).cloned() else { return };
        let fi = w.font_entry(&font);
        let type0 = w.fonts[fi].type0;
        for g in glyphs {
            w.fonts[fi].used.insert(g.gid);
            if let Some(t) = self.res.glyph_text.get(&(font_id, g.gid)) {
                w.fonts[fi].text.insert(g.gid, t.clone());
            }
        }
        self.used.fonts.insert(type0, ());
        self.set_alpha(w, Some(self.opacity * color.a as f64 / 255.0), None);
        self.set_fill(color);
        let _ = writeln!(self.out, "BT\n/F{type0} 1 Tf");
        let mut i = 0;
        while i < glyphs.len() {
            // One `Tm` per stretch of glyphs on the same baseline.
            let y = glyphs[i].y;
            let mut j = i + 1;
            while j < glyphs.len() && (glyphs[j].y - y).abs() <= 1e-6 {
                j += 1;
            }
            let run = &glyphs[i..j];
            let _ = write!(self.out, "{} Tm\n[", nums(&[size, 0.0, 0.0, -size, run[0].x, y]));
            let mut pen = run[0].x;
            let mut hex_open = false;
            for (k, g) in run.iter().enumerate() {
                if !hex_open {
                    self.out.push('<');
                    hex_open = true;
                }
                let _ = write!(self.out, "{:04X}", g.gid);
                let wdt = glyph_width(&font, g.gid);
                let expected = pen + size * wdt / 1000.0;
                if let Some(next) = run.get(k + 1) {
                    let adj = ((expected - next.x) * 1000.0 / size * 1000.0).round() / 1000.0;
                    if adj != 0.0 {
                        self.out.push('>');
                        hex_open = false;
                        push_num(&mut self.out, adj);
                    }
                    pen = expected - adj * size / 1000.0;
                }
            }
            if hex_open {
                self.out.push('>');
            }
            self.out.push_str("] TJ\n");
            i = j;
        }
        self.out.push_str("ET\n");
    }
}

fn rgb_nums(c: [u8; 3]) -> String {
    nums(&[c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0])
}

/// Stops sorted by offset, clamped to 0..=1, padded so they start at 0 and end at 1.
pub(super) fn normalize_stops(stops: &[GradientStop]) -> Vec<GradientStop> {
    let mut s: Vec<GradientStop> = stops.iter().filter(|s| s.offset.is_finite()).map(|s| GradientStop { offset: s.offset.clamp(0.0, 1.0), color: s.color }).collect();
    // A stable sort keeps the given order of stops at the same offset (a hard edge).
    s.sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap_or(std::cmp::Ordering::Equal));
    if s.is_empty() {
        return s;
    }
    if s[0].offset > 0.0 {
        s.insert(0, GradientStop { offset: 0.0, color: s[0].color });
    }
    let last = s[s.len() - 1];
    if last.offset < 1.0 {
        s.push(GradientStop { offset: 1.0, color: last.color });
    }
    s
}

/// The colour function over 0..=1: one Type 2 (exponential, N 1) between two colours, or a Type 3
/// stitching of one Type 2 per interval for more stops.
pub(super) fn function(stops: &[GradientStop], comps: impl Fn(Color) -> Vec<f64>) -> String {
    let t2 = |a: Color, b: Color| format!("<< /FunctionType 2 /Domain [0 1] /C0 [{}] /C1 [{}] /N 1 >>", nums(&comps(a)), nums(&comps(b)));
    if stops.len() == 1 {
        return t2(stops[0].color, stops[0].color);
    }
    // Intervals of positive width; a zero-width one (a hard edge) is the bound between its neighbours.
    let mut parts: Vec<(f64, f64, Color, Color)> = Vec::new();
    for p in stops.windows(2) {
        if p[1].offset > p[0].offset {
            parts.push((p[0].offset, p[1].offset, p[0].color, p[1].color));
        }
    }
    if parts.is_empty() {
        let c = stops[stops.len() - 1].color;
        return t2(c, c);
    }
    if parts.len() == 1 {
        return t2(parts[0].2, parts[0].3);
    }
    let fns: Vec<String> = parts.iter().map(|p| t2(p.2, p.3)).collect();
    let bounds: Vec<f64> = parts[..parts.len() - 1].iter().map(|p| p.1).collect();
    let encode: Vec<f64> = parts.iter().flat_map(|_| [0.0, 1.0]).collect();
    format!("<< /FunctionType 3 /Domain [0 1] /Functions [{}] /Bounds [{}] /Encode [{}] >>", fns.join(" "), nums(&bounds), nums(&encode))
}

/// A link target: `#page=N` (1-based, optionally `&y=` display points from the top) goes inside the
/// document; anything else not starting with `#` is a URI. Other fragments are left out.
fn parse_target(t: &str) -> Option<LinkTarget> {
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(frag) = t.strip_prefix('#') {
        let mut page = None;
        let mut y = None;
        for part in frag.split('&') {
            if let Some(v) = part.strip_prefix("page=") {
                page = v.trim().parse::<usize>().ok().filter(|&p| p >= 1);
            } else if let Some(v) = part.strip_prefix("y=") {
                y = v.trim().parse::<f64>().ok().filter(|v| v.is_finite());
            }
        }
        return page.map(|p| LinkTarget::Page(p - 1, y));
    }
    Some(LinkTarget::Uri(t.to_string()))
}
