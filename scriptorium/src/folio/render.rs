//! The content-stream renderer: a page's operators become a `display::DisplayList` (points, y down,
//! origin at the top-left of the crop box, page rotation applied), and every shown character is kept
//! as a positioned [`TextGlyph`] for text extraction.
//!
//! Covered: graphics state (q Q cm w J j M d gs ri i), paths (m l c v y h re) and painting
//! (S s f F f* B B* b b* n, W W* clipping), colour in every space `colour` knows, shadings (sh, and
//! shading patterns) as gradients, Form and Image XObjects, inline images, text (BT ET Tf Tc Tw Tz TL
//! Ts Td TD Tm T* Tj TJ ' " Tr) with glyph outlines registered in `display::Resources`, Type 3 glyph
//! procedures, ExtGState alpha. Unknown operators are skipped.

use std::collections::HashMap;
use std::sync::Arc;

use super::colour::{Cs, Func};
use super::fonts::{FontCache, Kind, PdfFont};
use super::image::decode_image;
use super::lexer::{find, is_white, Lexer, Parsed};
use super::obj::{Dict, Obj, Stream};
use super::parse::PdfFile;
use crate::ground::display::{Color, DisplayList, GradientStop, ImageRes, Item, LineCap, LineJoin, Matrix, Paint, PathOp, PlacedGlyph, Rect, Resources, Stroke};

/// The most operators one page may run (forms and Type 3 glyphs included).
const MAX_OPS: usize = 4_000_000;
/// How deep forms, Type 3 glyphs and annotation appearances may nest.
const MAX_DEPTH: u32 = 12;
/// The most image pixel bytes one document keeps decoded.
const IMAGE_BUDGET: usize = 768 * 1024 * 1024;

/// One shown character, for text extraction (display coordinates).
#[derive(Debug, Clone, PartialEq)]
pub struct TextGlyph {
    pub text: String,
    /// The glyph origin on the baseline.
    pub x: f64,
    pub y: f64,
    /// The advance along the baseline.
    pub w: f64,
    /// The font size as drawn.
    pub size: f64,
    /// The baseline direction (unit vector, display coordinates).
    pub dx: f64,
    pub dy: f64,
}

/// What renders share across pages: fonts and decoded images.
#[derive(Debug, Default)]
pub struct RenderCache {
    pub fonts: FontCache,
    images: HashMap<String, Option<u32>>,
    next_image: u32,
    image_bytes: usize,
}

impl RenderCache {
    pub fn new() -> RenderCache {
        RenderCache { fonts: FontCache::new(), images: HashMap::new(), next_image: 1, image_bytes: 0 }
    }
}

/// What a page needs to be drawn.
pub struct PageSource<'a> {
    pub width: f64,
    pub height: f64,
    /// PDF default user space to display space.
    pub base: Matrix,
    pub resources: &'a Dict,
    pub contents: Obj,
    /// Annotation dictionaries (their /AP /N appearance is drawn).
    pub annots: Vec<Dict>,
    /// Link areas (PDF user space rect) and targets.
    pub links: Vec<([f64; 4], String)>,
}

pub struct PageRender {
    pub list: DisplayList,
    pub text: Vec<TextGlyph>,
    pub image_count: usize,
}

#[derive(Clone)]
struct GState {
    ctm: Matrix,
    fill_cs: Cs,
    stroke_cs: Cs,
    fill: Vec<f64>,
    stroke: Vec<f64>,
    fill_paint: Option<Paint>,
    stroke_paint: Option<Paint>,
    lw: f64,
    cap: LineCap,
    join: LineJoin,
    miter: f64,
    dash: Vec<f64>,
    dash_phase: f64,
    fill_alpha: f64,
    stroke_alpha: f64,
    font: Option<Arc<PdfFont>>,
    size: f64,
    tc: f64,
    tw: f64,
    th: f64,
    tl: f64,
    rise: f64,
    mode: i64,
    clips: usize,
}

impl GState {
    fn new(ctm: Matrix) -> GState {
        GState {
            ctm,
            fill_cs: Cs::Gray,
            stroke_cs: Cs::Gray,
            fill: vec![0.0],
            stroke: vec![0.0],
            fill_paint: None,
            stroke_paint: None,
            lw: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter: 10.0,
            dash: Vec::new(),
            dash_phase: 0.0,
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            font: None,
            size: 1.0,
            tc: 0.0,
            tw: 0.0,
            th: 1.0,
            tl: 0.0,
            rise: 0.0,
            mode: 0,
            clips: 0,
        }
    }
}

struct R<'a> {
    file: &'a PdfFile,
    cache: &'a mut RenderCache,
    res: &'a mut Resources,
    out: Vec<Item>,
    text: Vec<TextGlyph>,
    base: Matrix,
    page: Rect,
    ops: usize,
    forms: Vec<u32>,
    images: usize,
}

fn num(ops: &[Obj], i: usize) -> f64 {
    ops.get(i).and_then(Obj::as_f64).unwrap_or(0.0)
}

fn nums(ops: &[Obj]) -> Vec<f64> {
    ops.iter().filter_map(Obj::as_f64).collect()
}

/// Draw a page.
pub fn render_page(file: &PdfFile, src: &PageSource, cache: &mut RenderCache, res: &mut Resources) -> PageRender {
    let mut r = R {
        file,
        cache,
        res,
        out: Vec::new(),
        text: Vec::new(),
        base: src.base,
        page: Rect::new(0.0, 0.0, src.width, src.height),
        ops: 0,
        forms: Vec::new(),
        images: 0,
    };
    let data = content_bytes(file, &src.contents);
    r.run(&data, src.resources, GState::new(Matrix::IDENTITY), 0);
    for a in &src.annots {
        r.annotation(a, src.resources);
    }
    for (rect, target) in &src.links {
        let dr = src.base.apply_rect(&Rect::from_ltrb(rect[0], rect[1], rect[2], rect[3]));
        r.out.push(Item::Link { rect: dr, target: target.clone() });
    }
    let mut list = DisplayList::new(src.width, src.height);
    list.items = r.out;
    PageRender { list, text: r.text, image_count: r.images }
}

/// A page's /Contents (a stream or an array of streams) as one byte string.
pub fn content_bytes(file: &PdfFile, contents: &Obj) -> Vec<u8> {
    match file.resolve(contents) {
        Obj::Stream(s) => file.stream_data(&s),
        Obj::Arr(a) => {
            let mut out = Vec::new();
            for part in a.iter().take(10_000) {
                if let Obj::Stream(s) = file.resolve(part) {
                    out.extend_from_slice(&file.stream_data(&s));
                    out.push(b'\n');
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

impl<'a> R<'a> {
    fn dev(&self, gs: &GState) -> Matrix {
        self.base.multiply(&gs.ctm)
    }

    fn fill_paint(&self, gs: &GState) -> Paint {
        if matches!(gs.fill_cs, Cs::Pattern) {
            if let Some(p) = &gs.fill_paint {
                return p.clone();
            }
        }
        Paint::Solid(gs.fill_cs.color(&gs.fill, gs.fill_alpha))
    }

    fn stroke_color(&self, gs: &GState) -> Color {
        gs.stroke_cs.color(&gs.stroke, gs.stroke_alpha)
    }

    fn fill_color(&self, gs: &GState) -> Color {
        gs.fill_cs.color(&gs.fill, gs.fill_alpha)
    }

    fn stroke(&self, gs: &GState) -> Stroke {
        let m = self.dev(gs);
        let scale = m.determinant().abs().sqrt();
        let mut s = Stroke::new((gs.lw * scale).max(0.25), self.stroke_color(gs));
        s.cap = gs.cap;
        s.join = gs.join;
        s.miter_limit = gs.miter.max(1.0);
        if gs.dash.iter().any(|&d| d > 0.0) {
            s.dash = gs.dash.iter().map(|d| d.abs() * scale).collect();
            s.dash_offset = gs.dash_phase * scale;
        }
        s
    }

    /// Run a content stream.
    fn run(&mut self, data: &[u8], resources: &Dict, mut gs: GState, depth: u32) {
        let start_clips = gs.clips;
        let mut stack: Vec<GState> = Vec::new();
        let mut path: Vec<PathOp> = Vec::new();
        let mut cur = (0.0f64, 0.0f64);
        let mut sub_start = (0.0f64, 0.0f64);
        let mut pending_clip = false;
        let mut tm = Matrix::IDENTITY;
        let mut tlm = Matrix::IDENTITY;
        let mut ops: Vec<Obj> = Vec::new();
        let mut lx = Lexer::new(data, 0);
        loop {
            if self.ops >= MAX_OPS {
                break;
            }
            let p = lx.parse();
            let op = match p {
                Parsed::End => break,
                Parsed::Obj(o) => {
                    if ops.len() < 64 {
                        ops.push(o);
                    }
                    continue;
                }
                Parsed::Keyword(k) => k,
            };
            self.ops += 1;
            let dev = self.dev(&gs);
            match op.as_slice() {
                b"q" => {
                    if stack.len() < 512 {
                        stack.push(gs.clone());
                    }
                }
                b"Q" => {
                    if let Some(prev) = stack.pop() {
                        for _ in prev.clips..gs.clips {
                            self.out.push(Item::PopClip);
                        }
                        gs = prev;
                    }
                }
                b"cm" if ops.len() >= 6 => {
                    let m = Matrix::new(num(&ops, 0), num(&ops, 1), num(&ops, 2), num(&ops, 3), num(&ops, 4), num(&ops, 5));
                    gs.ctm = gs.ctm.multiply(&m);
                }
                b"w" => gs.lw = num(&ops, 0).abs(),
                b"J" => {
                    gs.cap = match num(&ops, 0) as i64 {
                        1 => LineCap::Round,
                        2 => LineCap::Square,
                        _ => LineCap::Butt,
                    }
                }
                b"j" => {
                    gs.join = match num(&ops, 0) as i64 {
                        1 => LineJoin::Round,
                        2 => LineJoin::Bevel,
                        _ => LineJoin::Miter,
                    }
                }
                b"M" => gs.miter = num(&ops, 0),
                b"d" => {
                    gs.dash = ops.first().map(|a| a.nums()).unwrap_or_default();
                    gs.dash_phase = num(&ops, 1);
                }
                b"gs" => {
                    if let Some(name) = ops.first().and_then(Obj::as_name) {
                        if let Some(eg) = self.file.dict_of(resources, "ExtGState").and_then(|e| self.file.dict_of(&e, name)) {
                            self.ext_gstate(&eg, &mut gs, resources);
                        }
                    }
                }
                // Paths.
                b"m" if ops.len() >= 2 => {
                    cur = (num(&ops, 0), num(&ops, 1));
                    sub_start = cur;
                    let (x, y) = dev.apply(cur.0, cur.1);
                    path.push(PathOp::MoveTo(x, y));
                }
                b"l" if ops.len() >= 2 => {
                    cur = (num(&ops, 0), num(&ops, 1));
                    let (x, y) = dev.apply(cur.0, cur.1);
                    if path.is_empty() {
                        path.push(PathOp::MoveTo(x, y));
                    } else {
                        path.push(PathOp::LineTo(x, y));
                    }
                }
                b"c" | b"v" | b"y" => {
                    let v = nums(&ops);
                    let (c1, c2, end) = match op.as_slice() {
                        b"c" if v.len() >= 6 => ((v[0], v[1]), (v[2], v[3]), (v[4], v[5])),
                        b"v" if v.len() >= 4 => (cur, (v[0], v[1]), (v[2], v[3])),
                        b"y" if v.len() >= 4 => ((v[0], v[1]), (v[2], v[3]), (v[2], v[3])),
                        _ => {
                            ops.clear();
                            continue;
                        }
                    };
                    let a = dev.apply(c1.0, c1.1);
                    let b = dev.apply(c2.0, c2.1);
                    let e = dev.apply(end.0, end.1);
                    if path.is_empty() {
                        let s = dev.apply(cur.0, cur.1);
                        path.push(PathOp::MoveTo(s.0, s.1));
                    }
                    path.push(PathOp::CubicTo(a.0, a.1, b.0, b.1, e.0, e.1));
                    cur = end;
                }
                b"h" => {
                    if !path.is_empty() {
                        path.push(PathOp::Close);
                    }
                    cur = sub_start;
                }
                b"re" if ops.len() >= 4 => {
                    let (x, y, w, h) = (num(&ops, 0), num(&ops, 1), num(&ops, 2), num(&ops, 3));
                    for (i, (px, py)) in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].iter().enumerate() {
                        let (dx, dy) = dev.apply(*px, *py);
                        path.push(if i == 0 { PathOp::MoveTo(dx, dy) } else { PathOp::LineTo(dx, dy) });
                    }
                    path.push(PathOp::Close);
                    cur = (x, y);
                    sub_start = cur;
                }
                b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n" => {
                    let close = matches!(op.as_slice(), b"s" | b"b" | b"b*");
                    if close && !path.is_empty() {
                        path.push(PathOp::Close);
                    }
                    let fill = matches!(op.as_slice(), b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*");
                    let stroke = matches!(op.as_slice(), b"S" | b"s" | b"B" | b"B*" | b"b" | b"b*");
                    let drawable = path.iter().any(|p| !matches!(p, PathOp::MoveTo(..) | PathOp::Close));
                    if drawable && (fill || stroke) {
                        self.out.push(Item::Path {
                            ops: path.clone(),
                            fill: if fill { Some(self.fill_paint(&gs)) } else { None },
                            stroke: if stroke { Some(self.stroke(&gs)) } else { None },
                        });
                    }
                    if pending_clip {
                        self.out.push(Item::PushClip(path.clone()));
                        gs.clips += 1;
                        pending_clip = false;
                    }
                    path.clear();
                }
                b"W" | b"W*" => pending_clip = true,
                // Colour.
                b"CS" | b"cs" => {
                    let cs = match ops.first() {
                        Some(o) => Cs::parse(self.file, o, Some(resources), 0),
                        None => Cs::Gray,
                    };
                    let init = cs.initial();
                    if op == b"CS" {
                        gs.stroke_cs = cs;
                        gs.stroke = init;
                        gs.stroke_paint = None;
                    } else {
                        gs.fill_cs = cs;
                        gs.fill = init;
                        gs.fill_paint = None;
                    }
                }
                b"SC" | b"SCN" | b"sc" | b"scn" => {
                    let comps = nums(&ops);
                    let pattern = ops.last().and_then(Obj::as_name).map(str::to_string);
                    let stroke = op.starts_with(b"S");
                    let paint = pattern.and_then(|p| self.pattern_paint(&p, resources));
                    if stroke {
                        if !comps.is_empty() {
                            gs.stroke = comps;
                        }
                        gs.stroke_paint = paint;
                    } else {
                        if !comps.is_empty() {
                            gs.fill = comps;
                        }
                        gs.fill_paint = paint;
                    }
                }
                b"G" => {
                    gs.stroke_cs = Cs::Gray;
                    gs.stroke = vec![num(&ops, 0)];
                }
                b"g" => {
                    gs.fill_cs = Cs::Gray;
                    gs.fill = vec![num(&ops, 0)];
                }
                b"RG" => {
                    gs.stroke_cs = Cs::Rgb;
                    gs.stroke = vec![num(&ops, 0), num(&ops, 1), num(&ops, 2)];
                }
                b"rg" => {
                    gs.fill_cs = Cs::Rgb;
                    gs.fill = vec![num(&ops, 0), num(&ops, 1), num(&ops, 2)];
                }
                b"K" => {
                    gs.stroke_cs = Cs::Cmyk;
                    gs.stroke = vec![num(&ops, 0), num(&ops, 1), num(&ops, 2), num(&ops, 3)];
                }
                b"k" => {
                    gs.fill_cs = Cs::Cmyk;
                    gs.fill = vec![num(&ops, 0), num(&ops, 1), num(&ops, 2), num(&ops, 3)];
                }
                b"sh" => {
                    if let Some(name) = ops.first().and_then(Obj::as_name) {
                        if let Some(sh) = self.file.dict_of(resources, "Shading").map(|s| self.file.dget(&s, name)) {
                            if let Some(d) = sh.as_dict() {
                                let paint = self.shading_paint(d, &dev).unwrap_or(Paint::Solid(Color::rgb(200, 200, 200)));
                                let p = self.page;
                                self.out.push(Item::Path {
                                    ops: vec![
                                        PathOp::MoveTo(p.x, p.y),
                                        PathOp::LineTo(p.right(), p.y),
                                        PathOp::LineTo(p.right(), p.bottom()),
                                        PathOp::LineTo(p.x, p.bottom()),
                                        PathOp::Close,
                                    ],
                                    fill: Some(paint),
                                    stroke: None,
                                });
                            }
                        }
                    }
                }
                // XObjects and inline images.
                b"Do" => {
                    if let Some(name) = ops.first().and_then(Obj::as_name) {
                        if let Some(xo) = self.file.dict_of(resources, "XObject") {
                            let raw = xo.get(name).cloned().unwrap_or(Obj::Null);
                            let num = raw.as_ref().map(|r| r.0);
                            if let Obj::Stream(s) = self.file.resolve(&raw) {
                                match s.dict.name("Subtype") {
                                    Some("Form") => self.form(&s, num, &gs, resources, depth),
                                    Some("Image") => {
                                        let key = num.map(|n| format!("x{}", n));
                                        self.image(&s, key, &gs, resources);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                b"BI" => {
                    if let Some(s) = inline_image(&mut lx) {
                        self.image(&Arc::new(s), None, &gs, resources);
                    }
                }
                // Text.
                b"BT" => {
                    tm = Matrix::IDENTITY;
                    tlm = Matrix::IDENTITY;
                }
                b"ET" => {}
                b"Tc" => gs.tc = num(&ops, 0),
                b"Tw" => gs.tw = num(&ops, 0),
                b"Tz" => gs.th = num(&ops, 0) / 100.0,
                b"TL" => gs.tl = num(&ops, 0),
                b"Ts" => gs.rise = num(&ops, 0),
                b"Tr" => gs.mode = num(&ops, 0) as i64,
                b"Tf" => {
                    gs.size = num(&ops, 1);
                    if let Some(name) = ops.first().and_then(Obj::as_name) {
                        if let Some(fd) = self.file.dict_of(resources, "Font") {
                            if let Some(fo) = fd.get(name) {
                                let fo = fo.clone();
                                gs.font = self.cache.fonts.get(self.file, &fo);
                            }
                        }
                    }
                }
                b"Td" | b"TD" => {
                    let (tx, ty) = (num(&ops, 0), num(&ops, 1));
                    if op == b"TD" {
                        gs.tl = -ty;
                    }
                    tlm = tlm.multiply(&Matrix::translate(tx, ty));
                    tm = tlm;
                }
                b"Tm" if ops.len() >= 6 => {
                    tlm = Matrix::new(num(&ops, 0), num(&ops, 1), num(&ops, 2), num(&ops, 3), num(&ops, 4), num(&ops, 5));
                    tm = tlm;
                }
                b"T*" => {
                    tlm = tlm.multiply(&Matrix::translate(0.0, -gs.tl));
                    tm = tlm;
                }
                b"Tj" => {
                    if let Some(s) = ops.first().and_then(Obj::as_bytes) {
                        let s = s.to_vec();
                        self.show(&s, &gs, &mut tm, resources, depth);
                    }
                }
                b"'" | b"\"" => {
                    if op == b"\"" {
                        gs.tw = num(&ops, 0);
                        gs.tc = num(&ops, 1);
                    }
                    tlm = tlm.multiply(&Matrix::translate(0.0, -gs.tl));
                    tm = tlm;
                    if let Some(s) = ops.last().and_then(Obj::as_bytes) {
                        let s = s.to_vec();
                        self.show(&s, &gs, &mut tm, resources, depth);
                    }
                }
                b"TJ" => {
                    if let Some(arr) = ops.first().and_then(Obj::as_arr) {
                        let arr = arr.to_vec();
                        for item in &arr {
                            match item {
                                Obj::Str(s) => self.show(s, &gs, &mut tm, resources, depth),
                                other => {
                                    if let Some(n) = other.as_f64() {
                                        let tx = -n / 1000.0 * gs.size * gs.th;
                                        tm = tm.multiply(&Matrix::translate(tx, 0.0));
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            ops.clear();
        }
        for _ in start_clips..gs.clips {
            self.out.push(Item::PopClip);
        }
    }

    fn ext_gstate(&mut self, eg: &Dict, gs: &mut GState, _resources: &Dict) {
        let f = self.file;
        if let Some(v) = f.dget(eg, "LW").as_f64() {
            gs.lw = v.abs();
        }
        if let Some(v) = f.dget(eg, "LC").as_i64() {
            gs.cap = match v {
                1 => LineCap::Round,
                2 => LineCap::Square,
                _ => LineCap::Butt,
            };
        }
        if let Some(v) = f.dget(eg, "LJ").as_i64() {
            gs.join = match v {
                1 => LineJoin::Round,
                2 => LineJoin::Bevel,
                _ => LineJoin::Miter,
            };
        }
        if let Some(v) = f.dget(eg, "ML").as_f64() {
            gs.miter = v;
        }
        if let Obj::Arr(d) = f.dget(eg, "D") {
            gs.dash = d.first().map(|a| a.nums()).unwrap_or_default();
            gs.dash_phase = d.get(1).and_then(Obj::as_f64).unwrap_or(0.0);
        }
        if let Some(v) = f.dget(eg, "CA").as_f64() {
            gs.stroke_alpha = v.clamp(0.0, 1.0);
        }
        if let Some(v) = f.dget(eg, "ca").as_f64() {
            gs.fill_alpha = v.clamp(0.0, 1.0);
        }
        if let Obj::Arr(fa) = f.dget(eg, "Font") {
            if let Some(fo) = fa.first() {
                let fo = fo.clone();
                gs.font = self.cache.fonts.get(f, &fo);
            }
            if let Some(s) = fa.get(1).and_then(Obj::as_f64) {
                gs.size = s;
            }
        }
    }

    /// A shading as a gradient paint (axial and radial shadings; others give None).
    fn shading_paint(&self, sh: &Dict, m: &Matrix) -> Option<Paint> {
        let f = self.file;
        let kind = f.dget(sh, "ShadingType").as_i64()?;
        let cs = Cs::parse(f, sh.get("ColorSpace").unwrap_or(&Obj::name("DeviceRGB")), None, 0);
        let func = sh.get("Function").and_then(|o| Func::parse(f, o, 0));
        let dom = f.dget(sh, "Domain").nums();
        let (t0, t1) = (dom.first().copied().unwrap_or(0.0), dom.get(1).copied().unwrap_or(1.0));
        let coords = f.dget(sh, "Coords").nums();
        let stops: Vec<GradientStop> = (0..=10)
            .map(|i| {
                let k = i as f64 / 10.0;
                let t = t0 + k * (t1 - t0);
                let c = match &func {
                    Some(fu) => fu.eval(t),
                    None => vec![t],
                };
                GradientStop { offset: k, color: cs.color(&c, 1.0) }
            })
            .collect();
        match kind {
            2 if coords.len() >= 4 => {
                let (x0, y0) = m.apply(coords[0], coords[1]);
                let (x1, y1) = m.apply(coords[2], coords[3]);
                Some(Paint::Linear { x0, y0, x1, y1, stops })
            }
            3 if coords.len() >= 6 => {
                let s = m.determinant().abs().sqrt();
                let (fx, fy) = m.apply(coords[0], coords[1]);
                let (cx, cy) = m.apply(coords[3], coords[4]);
                Some(Paint::Radial { fx, fy, fr: coords[2] * s, cx, cy, r: coords[5] * s, stops })
            }
            _ => {
                // Function, free-form and mesh shadings: their middle colour.
                stops.get(5).map(|s| Paint::Solid(s.color))
            }
        }
    }

    /// A pattern colour as a paint (shading patterns as gradients, tiling patterns as a light gray).
    fn pattern_paint(&self, name: &str, resources: &Dict) -> Option<Paint> {
        let pat = self.file.dict_of(resources, "Pattern")?;
        let p = self.file.dget(&pat, name);
        let d = p.as_dict()?;
        let mv = self.file.dget(d, "Matrix").nums();
        let pm = if mv.len() >= 6 { Matrix::new(mv[0], mv[1], mv[2], mv[3], mv[4], mv[5]) } else { Matrix::IDENTITY };
        match self.file.dget(d, "PatternType").as_i64() {
            Some(2) => {
                let sh = self.file.dget(d, "Shading");
                let m = self.base.multiply(&pm);
                self.shading_paint(sh.as_dict()?, &m)
            }
            _ => Some(Paint::Solid(Color::rgba(160, 160, 160, 128))),
        }
    }

    fn form(&mut self, s: &Stream, num: Option<u32>, gs: &GState, resources: &Dict, depth: u32) {
        if depth >= MAX_DEPTH {
            return;
        }
        if let Some(n) = num {
            if self.forms.contains(&n) {
                return;
            }
        }
        let f = self.file;
        let mv = f.dget(&s.dict, "Matrix").nums();
        let m = if mv.len() >= 6 { Matrix::new(mv[0], mv[1], mv[2], mv[3], mv[4], mv[5]) } else { Matrix::IDENTITY };
        let mut g = gs.clone();
        g.ctm = g.ctm.multiply(&m);
        let bbox = f.dget(&s.dict, "BBox").nums();
        let mut clipped = false;
        if bbox.len() >= 4 {
            let dev = self.dev(&g);
            let pts = [(bbox[0], bbox[1]), (bbox[2], bbox[1]), (bbox[2], bbox[3]), (bbox[0], bbox[3])];
            let mut clip = Vec::with_capacity(5);
            for (i, (x, y)) in pts.iter().enumerate() {
                let (dx, dy) = dev.apply(*x, *y);
                clip.push(if i == 0 { PathOp::MoveTo(dx, dy) } else { PathOp::LineTo(dx, dy) });
            }
            clip.push(PathOp::Close);
            self.out.push(Item::PushClip(clip));
            clipped = true;
        }
        let own = f.dict_of(&s.dict, "Resources");
        let res = own.as_ref().unwrap_or(resources).clone();
        let data = f.stream_data(s);
        if let Some(n) = num {
            self.forms.push(n);
        }
        g.clips = 0;
        self.run(&data, &res, g, depth + 1);
        if num.is_some() {
            self.forms.pop();
        }
        if clipped {
            self.out.push(Item::PopClip);
        }
    }

    fn image(&mut self, s: &Arc<Stream>, key: Option<String>, gs: &GState, resources: &Dict) {
        let stencil = s.dict.get("ImageMask").or_else(|| s.dict.get("IM")).and_then(Obj::as_bool).unwrap_or(false);
        let fill = self.fill_color(gs);
        let key = key.map(|k| if stencil { format!("{}:{}", k, fill.to_hex()) } else { k });
        let cached = key.as_ref().and_then(|k| self.cache.images.get(k).copied());
        let id = match cached {
            Some(id) => id,
            None => {
                let id = if self.cache.image_bytes > IMAGE_BUDGET {
                    None
                } else {
                    match decode_image(self.file, s, Some(resources), Color { a: 255, ..fill }, 0) {
                        Ok(px) => {
                            let id = self.cache.next_image;
                            self.cache.next_image += 1;
                            self.cache.image_bytes += px.rgba.len();
                            self.res.add_image(ImageRes { id, width: px.width, height: px.height, rgba: px.rgba });
                            Some(id)
                        }
                        Err(_) => None,
                    }
                };
                if let Some(k) = key {
                    self.cache.images.insert(k, id);
                }
                id
            }
        };
        self.images += 1;
        let m = self.dev(gs).multiply(&Matrix::new(1.0, 0.0, 0.0, -1.0, 0.0, 1.0));
        let alpha = if stencil { 1.0 } else { gs.fill_alpha };
        if alpha < 1.0 {
            self.out.push(Item::PushOpacity(alpha));
        }
        self.out.push(Item::PushTransform(m));
        match id {
            Some(id) => self.out.push(Item::Image { image: id, rect: Rect::new(0.0, 0.0, 1.0, 1.0) }),
            None => self.out.push(Item::Path {
                ops: vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(1.0, 0.0), PathOp::LineTo(1.0, 1.0), PathOp::LineTo(0.0, 1.0), PathOp::Close],
                fill: Some(Paint::Solid(Color::rgb(220, 220, 220))),
                stroke: None,
            }),
        }
        self.out.push(Item::PopTransform);
        if alpha < 1.0 {
            self.out.push(Item::PopOpacity);
        }
    }

    fn ensure_glyph(&mut self, font: &PdfFont, gid: u16) {
        let f = self.res.add_font(font.id, font.upem);
        if f.glyphs.contains_key(&gid) {
            return;
        }
        let outline: Vec<PathOp> = font
            .face
            .as_ref()
            .and_then(|face| face.outline(gid))
            .map(|ops| {
                ops.iter()
                    .map(|o| match *o {
                        crate::ground::font::PathOp::MoveTo(x, y) => PathOp::MoveTo(x as f64, y as f64),
                        crate::ground::font::PathOp::LineTo(x, y) => PathOp::LineTo(x as f64, y as f64),
                        crate::ground::font::PathOp::QuadTo(a, b, x, y) => PathOp::QuadTo(a as f64, b as f64, x as f64, y as f64),
                        crate::ground::font::PathOp::CubicTo(a, b, c, d, x, y) => {
                            PathOp::CubicTo(a as f64, b as f64, c as f64, d as f64, x as f64, y as f64)
                        }
                        crate::ground::font::PathOp::Close => PathOp::Close,
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.res.set_glyph(font.id, gid, outline);
    }

    /// Show a string: glyphs drawn, text kept, the text matrix advanced.
    fn show(&mut self, s: &[u8], gs: &GState, tm: &mut Matrix, resources: &Dict, depth: u32) {
        let Some(font) = gs.font.clone() else { return };
        let glyphs = font.decode(s);
        let dev = self.dev(gs);
        let param = Matrix::new(gs.size * gs.th, 0.0, 0.0, gs.size, 0.0, gs.rise);
        let mode = gs.mode;
        let color = if matches!(mode, 1 | 5) { self.stroke_color(gs) } else { self.fill_color(gs) };
        let visible = !matches!(mode, 3 | 7);
        let mut placed: Vec<(u16, f64, f64)> = Vec::new();
        let first = dev.multiply(tm).multiply(&param);
        for g in &glyphs {
            let trm = dev.multiply(tm).multiply(&param);
            let (ox, oy) = trm.apply(0.0, 0.0);
            let (ex, ey) = trm.apply(g.width, 0.0);
            let adv = ((ex - ox).powi(2) + (ey - oy).powi(2)).sqrt();
            let size = trm.determinant().abs().sqrt();
            if !g.text.is_empty() && self.text.len() < 2_000_000 {
                let (dx, dy) = if adv > 1e-9 { ((ex - ox) / adv, (ey - oy) / adv) } else { (1.0, 0.0) };
                self.text.push(TextGlyph { text: g.text.clone(), x: ox, y: oy, w: adv, size, dx, dy });
            }
            if visible {
                if font.kind == Kind::Type3 {
                    self.type3_glyph(&font, g.code, gs, tm, &param, resources, depth);
                } else if g.gid != 0 {
                    placed.push((g.gid, ox, oy));
                }
            }
            let tx = (g.width * gs.size + gs.tc + if g.word_space { gs.tw } else { 0.0 }) * gs.th;
            *tm = tm.multiply(&Matrix::translate(tx, 0.0));
        }
        if placed.is_empty() {
            return;
        }
        for (gid, _, _) in &placed {
            self.ensure_glyph(&font, *gid);
        }
        let (a, b, c, d) = (first.a, first.b, first.c, first.d);
        let s = (a * d - b * c).abs().sqrt();
        if !(s > 1e-9) || !s.is_finite() {
            return;
        }
        let eps = 1e-6 * s;
        let upright = b.abs() < eps && c.abs() < eps && a > 0.0 && d < 0.0 && (a + d).abs() < eps;
        if upright {
            let gl = placed.iter().map(|&(gid, x, y)| PlacedGlyph { gid, x, y }).collect();
            self.out.push(Item::Glyphs { font: font.id, size: s, color, glyphs: gl });
        } else {
            let (x0, y0) = (placed[0].1, placed[0].2);
            let t = Matrix::new(a / s, b / s, -c / s, -d / s, x0, y0);
            let Some(inv) = t.invert() else { return };
            let gl = placed
                .iter()
                .map(|&(gid, x, y)| {
                    let (lx, ly) = inv.apply(x, y);
                    PlacedGlyph { gid, x: lx, y: ly }
                })
                .collect();
            self.out.push(Item::PushTransform(t));
            self.out.push(Item::Glyphs { font: font.id, size: s, color, glyphs: gl });
            self.out.push(Item::PopTransform);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn type3_glyph(&mut self, font: &PdfFont, code: u32, gs: &GState, tm: &Matrix, param: &Matrix, resources: &Dict, depth: u32) {
        if depth >= MAX_DEPTH {
            return;
        }
        let Some(t3) = &font.type3 else { return };
        let Some(proc_obj) = font.type3_proc(code) else { return };
        let Obj::Stream(s) = self.file.resolve(proc_obj) else { return };
        let data = self.file.stream_data(&s);
        let mut g = gs.clone();
        g.ctm = gs.ctm.multiply(tm).multiply(param).multiply(&t3.matrix);
        g.clips = 0;
        g.font = None;
        let res = t3.resources.clone().unwrap_or_else(|| resources.clone());
        self.run(&data, &res, g, depth + 1);
    }

    /// An annotation's normal appearance, fitted to its /Rect.
    fn annotation(&mut self, a: &Dict, resources: &Dict) {
        let f = self.file;
        let flags = f.dget(a, "F").as_i64().unwrap_or(0);
        if flags & 2 != 0 || flags & 32 != 0 || a.name("Subtype") == Some("Popup") {
            return;
        }
        let Some(ap) = f.dict_of(a, "AP") else { return };
        let n = f.dget(&ap, "N");
        let stream = match &n {
            Obj::Stream(s) => Some(s.clone()),
            Obj::Dict(states) => {
                let state = f.dget(a, "AS");
                state.as_name().and_then(|st| f.dget(states, st).as_stream().cloned())
            }
            _ => None,
        };
        let Some(s) = stream else { return };
        let rect = f.dget(a, "Rect").nums();
        if rect.len() < 4 {
            return;
        }
        let (rx0, ry0, rx1, ry1) = (rect[0].min(rect[2]), rect[1].min(rect[3]), rect[0].max(rect[2]), rect[1].max(rect[3]));
        let bbox = f.dget(&s.dict, "BBox").nums();
        let mv = f.dget(&s.dict, "Matrix").nums();
        let m = if mv.len() >= 6 { Matrix::new(mv[0], mv[1], mv[2], mv[3], mv[4], mv[5]) } else { Matrix::IDENTITY };
        let bb = if bbox.len() >= 4 { Rect::from_ltrb(bbox[0], bbox[1], bbox[2], bbox[3]) } else { Rect::from_ltrb(rx0, ry0, rx1, ry1) };
        let tb = m.apply_rect(&bb);
        let sx = if tb.w > 1e-9 { (rx1 - rx0) / tb.w } else { 1.0 };
        let sy = if tb.h > 1e-9 { (ry1 - ry0) / tb.h } else { 1.0 };
        let fit = Matrix::new(sx, 0.0, 0.0, sy, rx0 - tb.x * sx, ry0 - tb.y * sy);
        let gs = GState::new(fit);
        self.form(&s, None, &gs, resources, 1);
    }
}

/// Read an inline image after `BI`: its dictionary up to `ID`, then the data up to `EI`.
fn inline_image(lx: &mut Lexer) -> Option<Stream> {
    let mut d = Dict::new();
    loop {
        match lx.parse() {
            Parsed::Obj(Obj::Name(k)) => {
                let v = lx.object();
                let key = match k.as_str() {
                    "BPC" => "BitsPerComponent",
                    "CS" => "ColorSpace",
                    "D" => "Decode",
                    "DP" => "DecodeParms",
                    "F" => "Filter",
                    "H" => "Height",
                    "IM" => "ImageMask",
                    "I" => "Interpolate",
                    "W" => "Width",
                    other => other,
                }
                .to_string();
                d.set(&key, v);
            }
            Parsed::Keyword(k) if k == b"ID" => break,
            Parsed::End => return None,
            _ => {}
        }
        if d.0.len() > 64 {
            return None;
        }
    }
    let data = lx.d;
    let mut start = lx.pos;
    if start < data.len() && is_white(data[start]) {
        start += 1;
    }
    let mut end: Option<usize> = None;
    if !d.contains("Filter") {
        let w = d.get("Width").and_then(Obj::as_i64).unwrap_or(0).max(0) as usize;
        let h = d.get("Height").and_then(Obj::as_i64).unwrap_or(0).max(0) as usize;
        let im = d.get("ImageMask").and_then(Obj::as_bool).unwrap_or(false);
        let bpc = if im { 1 } else { d.get("BitsPerComponent").and_then(Obj::as_i64).unwrap_or(8).clamp(1, 16) as usize };
        let n = if im {
            1
        } else {
            match d.get("ColorSpace") {
                Some(Obj::Name(n)) if n == "RGB" || n == "DeviceRGB" => 3,
                Some(Obj::Name(n)) if n == "CMYK" || n == "DeviceCMYK" => 4,
                _ => 1,
            }
        };
        let len = (w.saturating_mul(n).saturating_mul(bpc) + 7) / 8 * h;
        let e = start.saturating_add(len);
        if e <= data.len() {
            let mut q = e;
            while q < data.len() && is_white(data[q]) {
                q += 1;
            }
            if data[q..].starts_with(b"EI") {
                end = Some(e);
                lx.pos = q + 2;
            }
        }
    }
    if end.is_none() {
        let mut from = start;
        loop {
            let Some(p) = find(data, b"EI", from) else {
                lx.pos = data.len();
                return None;
            };
            let before = p == 0 || is_white(data[p - 1]);
            let after = p + 2 >= data.len() || is_white(data[p + 2]);
            if before && after {
                let mut e = p;
                if e > start && is_white(data[e - 1]) {
                    e -= 1;
                }
                end = Some(e.max(start));
                lx.pos = p + 2;
                break;
            }
            from = p + 2;
        }
    }
    let end = end?;
    Some(Stream { dict: d, data: data[start..end].to_vec() })
}
