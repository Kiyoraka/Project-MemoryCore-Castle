//! The drawing vocabulary every editor emits. Rust thinks, the canvas paints: Rust lays out everything
//! and hands the host a [`DisplayList`]; the host's canvas only draws it. Units are points (1/72 in),
//! y down, origin at the page's top-left.
//!
//! Glyphs are drawn from outlines held in [`Resources`] (no browser font ever decides a shape), and
//! images from RGBA pixels held there too; the host fetches both on demand as JSON.

use std::collections::HashMap;

use super::bus::json::Json;

// ---------------------------------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------------------------------

/// One step of a path. Coordinates are in the space of whatever holds the path (points for items,
/// font units for glyph outlines).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathOp {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    /// Control point, end point.
    QuadTo(f64, f64, f64, f64),
    /// Two control points, end point.
    CubicTo(f64, f64, f64, f64, f64, f64),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    /// From left, top, right, bottom (in any order).
    pub fn from_ltrb(l: f64, t: f64, r: f64, b: f64) -> Rect {
        let (l, r) = if l <= r { (l, r) } else { (r, l) };
        let (t, b) = if t <= b { (t, b) } else { (b, t) };
        Rect { x: l, y: t, w: r - l, h: b - t }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// True when the rects overlap or touch (a zero-height line on a tile's edge still counts).
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x <= o.right() && o.x <= self.right() && self.y <= o.bottom() && o.y <= self.bottom()
    }

    /// The overlap, if any.
    pub fn intersection(&self, o: &Rect) -> Option<Rect> {
        if !self.intersects(o) {
            return None;
        }
        Some(Rect::from_ltrb(self.x.max(o.x), self.y.max(o.y), self.right().min(o.right()), self.bottom().min(o.bottom())))
    }

    pub fn union(&self, o: &Rect) -> Rect {
        Rect::from_ltrb(self.x.min(o.x), self.y.min(o.y), self.right().max(o.right()), self.bottom().max(o.bottom()))
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }

    /// Grown by `d` on every side.
    pub fn inflate(&self, d: f64) -> Rect {
        Rect { x: self.x - d, y: self.y - d, w: self.w + 2.0 * d, h: self.h + 2.0 * d }
    }
}

/// A 2x3 affine matrix, the canvas's `setTransform(a, b, c, d, e, f)`:
/// `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Matrix {
    fn default() -> Matrix {
        Matrix::IDENTITY
    }
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Matrix {
        Matrix { a, b, c, d, e, f }
    }

    pub fn translate(tx: f64, ty: f64) -> Matrix {
        Matrix { e: tx, f: ty, ..Matrix::IDENTITY }
    }

    pub fn scale(sx: f64, sy: f64) -> Matrix {
        Matrix { a: sx, d: sy, ..Matrix::IDENTITY }
    }

    /// Rotation by `radians` (clockwise on screen, since y is down).
    pub fn rotate(radians: f64) -> Matrix {
        let (s, c) = radians.sin_cos();
        Matrix { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    /// `self x other`: the result applies `other` first, then `self`
    /// (`a.multiply(&b).apply(p) == a.apply(b.apply(p))`), like the canvas's `ctx.transform(other)`
    /// on a context whose current transform is `self`.
    pub fn multiply(&self, o: &Matrix) -> Matrix {
        Matrix {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }

    /// `self` first, then `next`.
    pub fn then(&self, next: &Matrix) -> Matrix {
        next.multiply(self)
    }

    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// The inverse, or `None` for a singular matrix.
    pub fn invert(&self) -> Option<Matrix> {
        let det = self.determinant();
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let a = self.d / det;
        let b = -self.b / det;
        let c = -self.c / det;
        let d = self.a / det;
        Some(Matrix { a, b, c, d, e: -(a * self.e + c * self.f), f: -(b * self.e + d * self.f) })
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// The axis-aligned box around the transformed rect.
    pub fn apply_rect(&self, r: &Rect) -> Rect {
        let pts = [self.apply(r.x, r.y), self.apply(r.right(), r.y), self.apply(r.x, r.bottom()), self.apply(r.right(), r.bottom())];
        let mut out = Rect::from_ltrb(pts[0].0, pts[0].1, pts[0].0, pts[0].1);
        for p in &pts[1..] {
            out = out.union(&Rect::new(p.0, p.1, 0.0, 0.0));
        }
        out
    }

    pub fn is_identity(&self) -> bool {
        *self == Matrix::IDENTITY
    }
}

/// The box around a path's points, control points included (never smaller than the true curve).
pub fn path_bounds(ops: &[PathOp]) -> Option<Rect> {
    let mut acc: Option<(f64, f64, f64, f64)> = None;
    let mut add = |x: f64, y: f64| {
        acc = Some(match acc {
            None => (x, y, x, y),
            Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
        });
    };
    for op in ops {
        match *op {
            PathOp::MoveTo(x, y) | PathOp::LineTo(x, y) => add(x, y),
            PathOp::QuadTo(x1, y1, x, y) => {
                add(x1, y1);
                add(x, y);
            }
            PathOp::CubicTo(x1, y1, x2, y2, x, y) => {
                add(x1, y1);
                add(x2, y2);
                add(x, y);
            }
            PathOp::Close => {}
        }
    }
    acc.map(|(l, t, r, b)| Rect::from_ltrb(l, t, r, b))
}

// ---------------------------------------------------------------------------------------------------
// Paint
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };
    pub const TRANSPARENT: Color = Color { r: 0, g: 0, b: 0, a: 0 };

    pub fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque (both are canvas fill styles).
    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    /// 0..=1 along the gradient.
    pub offset: f64,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(Color),
    /// From (x0, y0) to (x1, y1), in the item's coordinates.
    Linear { x0: f64, y0: f64, x1: f64, y1: f64, stops: Vec<GradientStop> },
    /// From the focal circle (fx, fy, fr) to the outer circle (cx, cy, r), like the canvas's
    /// `createRadialGradient(fx, fy, fr, cx, cy, r)`.
    Radial { fx: f64, fy: f64, fr: f64, cx: f64, cy: f64, r: f64, stops: Vec<GradientStop> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub width: f64,
    pub color: Color,
    /// On/off lengths; empty for a solid line.
    pub dash: Vec<f64>,
    /// Where in the dash pattern the line starts (PDF's dash phase, the canvas's `lineDashOffset`).
    pub dash_offset: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f64,
}

impl Stroke {
    /// A solid line with butt caps, miter joins and the usual miter limit of 10.
    pub fn new(width: f64, color: Color) -> Stroke {
        Stroke { width, color, dash: Vec::new(), dash_offset: 0.0, cap: LineCap::Butt, join: LineJoin::Miter, miter_limit: 10.0 }
    }

    /// How far the painted line can reach beyond the path's points.
    pub fn reach(&self) -> f64 {
        let half = self.width.abs() / 2.0;
        let mut r = half;
        if self.cap == LineCap::Square {
            r = r.max(half * std::f64::consts::SQRT_2);
        }
        if self.join == LineJoin::Miter {
            r = r.max(half * self.miter_limit.max(1.0));
        }
        r
    }
}

// ---------------------------------------------------------------------------------------------------
// The display list
// ---------------------------------------------------------------------------------------------------

/// One glyph placed at its origin on the baseline (points, y down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedGlyph {
    pub gid: u16,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Path { ops: Vec<PathOp>, fill: Option<Paint>, stroke: Option<Stroke> },
    /// A run of glyphs of one font (an id in [`Resources::fonts`]) at one size and colour.
    Glyphs { font: u32, size: f64, color: Color, glyphs: Vec<PlacedGlyph> },
    /// An image (an id in [`Resources::images`]) stretched over `rect`.
    Image { image: u32, rect: Rect },
    PushClip(Vec<PathOp>),
    PopClip,
    /// Multiplied onto the current transform (the canvas's `ctx.transform`).
    PushTransform(Matrix),
    PopTransform,
    /// Multiplied onto the current opacity (0..=1) for everything up to the matching pop.
    PushOpacity(f64),
    PopOpacity,
    /// A clickable area (a URL, or an in-document target such as `#page=3`); not painted.
    Link { rect: Rect, target: String },
}

impl Item {
    /// The item's box in its own coordinates (before any pushed transform), or `None` for the
    /// state items (clip, transform, opacity pushes and pops) and empty runs. Glyph runs use a
    /// generous em box per glyph, since the outlines live in the resources.
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            Item::Path { ops, stroke, .. } => {
                let r = path_bounds(ops)?;
                Some(match stroke {
                    Some(s) => r.inflate(s.reach()),
                    None => r,
                })
            }
            Item::Glyphs { size, glyphs, .. } => {
                let s = size.abs();
                let mut out: Option<Rect> = None;
                for g in glyphs {
                    let r = Rect::from_ltrb(g.x - 0.5 * s, g.y - 1.2 * s, g.x + 1.5 * s, g.y + 0.5 * s);
                    out = Some(match out {
                        None => r,
                        Some(o) => o.union(&r),
                    });
                }
                out
            }
            Item::Image { rect, .. } | Item::Link { rect, .. } => Some(Rect::from_ltrb(rect.x, rect.y, rect.right(), rect.bottom())),
            Item::PushClip(_) | Item::PopClip | Item::PushTransform(_) | Item::PopTransform | Item::PushOpacity(_) | Item::PopOpacity => None,
        }
    }

    /// True for the push/pop items that change the painter's state rather than draw.
    pub fn is_state(&self) -> bool {
        matches!(self, Item::PushClip(_) | Item::PopClip | Item::PushTransform(_) | Item::PopTransform | Item::PushOpacity(_) | Item::PopOpacity)
    }

    /// The item as JSON for the host (see [`DisplayList::to_json`]).
    pub fn to_json_value(&self) -> Json {
        match self {
            Item::Path { ops, fill, stroke } => {
                let mut v = Json::obj().with("t", "path").with("d", path_data(ops));
                if let Some(p) = fill {
                    v.set("fill", paint_json(p));
                }
                if let Some(s) = stroke {
                    v.set("stroke", stroke_json(s));
                }
                v
            }
            Item::Glyphs { font, size, color, glyphs } => {
                let mut g = Vec::with_capacity(glyphs.len() * 3);
                for pg in glyphs {
                    g.push(Json::from(pg.gid as u32));
                    g.push(n2(pg.x));
                    g.push(n2(pg.y));
                }
                Json::obj().with("t", "glyphs").with("font", *font).with("size", n2(*size)).with("c", color.to_hex()).with("g", Json::Arr(g))
            }
            Item::Image { image, rect } => Json::obj().with("t", "image").with("image", *image).with("r", rect_json(rect)),
            Item::PushClip(ops) => Json::obj().with("t", "clip").with("d", path_data(ops)),
            Item::PopClip => Json::obj().with("t", "unclip"),
            Item::PushTransform(m) => Json::obj().with(
                "t",
                "transform",
            ).with("m", Json::Arr(vec![round(m.a, 6), round(m.b, 6), round(m.c, 6), round(m.d, 6), n2(m.e), n2(m.f)])),
            Item::PopTransform => Json::obj().with("t", "untransform"),
            Item::PushOpacity(a) => Json::obj().with("t", "opacity").with("a", round(a.clamp(0.0, 1.0), 4)),
            Item::PopOpacity => Json::obj().with("t", "unopacity"),
            Item::Link { rect, target } => Json::obj().with("t", "link").with("r", rect_json(rect)).with("target", target.as_str()),
        }
    }
}

/// A page (or a slide, a sheet view, a tile) as things to draw, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayList {
    pub width: f64,
    pub height: f64,
    pub items: Vec<Item>,
}

impl DisplayList {
    pub fn new(width: f64, height: f64) -> DisplayList {
        DisplayList { width, height, items: Vec::new() }
    }

    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }

    /// The list as compact JSON for the host's painter. Coordinates are rounded to 2 decimals
    /// (matrix scale/rotation parts to 6, opacity and gradient offsets to 4). Shape:
    ///
    /// ```text
    /// {"w":612,"h":792,"items":[
    ///   {"t":"path","d":"M0 0L10 0Z","fill":{"k":"solid","c":"#ff0000"},
    ///      "stroke":{"w":1,"c":"#000000","dash":[],"off":0,"cap":"butt","join":"miter","ml":10}},
    ///   {"t":"glyphs","font":1,"size":12,"c":"#000000","g":[gid,x,y, gid,x,y, ...]},
    ///   {"t":"image","image":3,"r":[x,y,w,h]},
    ///   {"t":"clip","d":".."}, {"t":"unclip"},
    ///   {"t":"transform","m":[a,b,c,d,e,f]}, {"t":"untransform"},
    ///   {"t":"opacity","a":0.5}, {"t":"unopacity"},
    ///   {"t":"link","r":[x,y,w,h],"target":"https://.."}]}
    /// ```
    ///
    /// `d` is SVG path data (`M L Q C Z`, absolute), ready for `new Path2D(d)`. Paints are
    /// `{"k":"solid","c"}`, `{"k":"linear","x0","y0","x1","y1","stops":[[offset,"#.."],..]}` and
    /// `{"k":"radial","fx","fy","fr","cx","cy","r","stops"}`. Colours are `#rrggbb` or `#rrggbbaa`.
    pub fn to_json(&self) -> String {
        self.to_json_value().to_string()
    }

    /// [`DisplayList::to_json`] as a JSON value (for the bus's `shot`).
    pub fn to_json_value(&self) -> Json {
        Json::obj()
            .with("w", n2(self.width))
            .with("h", n2(self.height))
            .with("items", Json::Arr(self.items.iter().map(Item::to_json_value).collect()))
    }

    /// Each item's box in page coordinates (pushed transforms applied, pushed clips intersected), or
    /// `None` for state items and items that cannot be seen (empty, or clipped away entirely).
    pub fn world_bounds(&self) -> Vec<Option<Rect>> {
        let mut out = Vec::with_capacity(self.items.len());
        let mut m = Matrix::IDENTITY;
        let mut m_stack: Vec<Matrix> = Vec::new();
        // The current clip box in page coordinates; `Some(None)` would mean clipped to nothing.
        let mut clip: Option<Option<Rect>> = None;
        let mut clip_stack: Vec<Option<Option<Rect>>> = Vec::new();
        for item in &self.items {
            match item {
                Item::PushTransform(t) => {
                    m_stack.push(m);
                    m = m.multiply(t);
                    out.push(None);
                }
                Item::PopTransform => {
                    m = m_stack.pop().unwrap_or(Matrix::IDENTITY);
                    out.push(None);
                }
                Item::PushClip(ops) => {
                    clip_stack.push(clip);
                    let r = path_bounds(ops).map(|r| m.apply_rect(&r));
                    clip = Some(match (clip, r) {
                        (None, r) => r,
                        (Some(Some(c)), Some(r)) => c.intersection(&r),
                        _ => None,
                    });
                    out.push(None);
                }
                Item::PopClip => {
                    clip = clip_stack.pop().unwrap_or(None);
                    out.push(None);
                }
                Item::PushOpacity(_) | Item::PopOpacity => out.push(None),
                _ => {
                    let b = item.bounds().map(|r| m.apply_rect(&r));
                    let b = match (b, clip) {
                        (Some(b), None) => Some(b),
                        (Some(b), Some(Some(c))) => b.intersection(&c),
                        _ => None,
                    };
                    out.push(b);
                }
            }
        }
        out
    }

    /// For the host's tiled painter: the indices of the items to paint for the `view` rect (page
    /// coordinates), in order. State items are always included so pushes and pops stay balanced.
    pub fn visible_items(&self, view: Rect) -> Vec<usize> {
        let bounds = self.world_bounds();
        self.items
            .iter()
            .enumerate()
            .filter(|(i, item)| item.is_state() || bounds[*i].map_or(false, |b| b.intersects(&view)))
            .map(|(i, _)| i)
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------------------------------

/// A font as the painter needs it: glyph outlines by glyph id, cached as they are first used.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FontRes {
    pub id: u32,
    /// Outlines in font units, y UP as in the font file. To paint a glyph placed at (x, y) at
    /// `size` points: translate(x, y), scale(size / units_per_em, -size / units_per_em).
    pub glyphs: HashMap<u16, Vec<PathOp>>,
    pub units_per_em: f64,
}

/// An image's pixels, RGBA 8-bit, row by row from the top, not premultiplied.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageRes {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The fonts and images a display list refers to by id.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Resources {
    pub fonts: Vec<FontRes>,
    pub images: Vec<ImageRes>,
}

impl Resources {
    pub fn new() -> Resources {
        Resources::default()
    }

    pub fn font(&self, id: u32) -> Option<&FontRes> {
        self.fonts.iter().find(|f| f.id == id)
    }

    pub fn font_mut(&mut self, id: u32) -> Option<&mut FontRes> {
        self.fonts.iter_mut().find(|f| f.id == id)
    }

    /// The font with this id, added (empty) if it is not here yet.
    pub fn add_font(&mut self, id: u32, units_per_em: f64) -> &mut FontRes {
        if let Some(i) = self.fonts.iter().position(|f| f.id == id) {
            return &mut self.fonts[i];
        }
        self.fonts.push(FontRes { id, glyphs: HashMap::new(), units_per_em });
        self.fonts.last_mut().expect("just pushed")
    }

    /// Cache one glyph outline (adding the font with 1000 units per em if missing).
    pub fn set_glyph(&mut self, font: u32, gid: u16, outline: Vec<PathOp>) {
        if self.font(font).is_none() {
            self.add_font(font, 1000.0);
        }
        if let Some(f) = self.font_mut(font) {
            f.glyphs.insert(gid, outline);
        }
    }

    pub fn glyph(&self, font: u32, gid: u16) -> Option<&[PathOp]> {
        self.font(font)?.glyphs.get(&gid).map(|v| v.as_slice())
    }

    pub fn image(&self, id: u32) -> Option<&ImageRes> {
        self.images.iter().find(|i| i.id == id)
    }

    /// Add or replace an image (by its id).
    pub fn add_image(&mut self, image: ImageRes) {
        match self.images.iter_mut().find(|i| i.id == image.id) {
            Some(slot) => *slot = image,
            None => self.images.push(image),
        }
    }

    /// Outlines on demand for the host: `{"font":1,"upem":1000,"glyphs":{"36":"M..Z",..},"missing":[..]}`.
    /// `d` strings are SVG path data in font units, y up; `missing` lists the ids not cached (the
    /// editor should load them and ask again). `None` for an unknown font.
    pub fn glyphs_json(&self, font: u32, gids: &[u16]) -> Option<String> {
        let f = self.font(font)?;
        let mut glyphs = Json::obj();
        let mut missing = Json::arr();
        for &gid in gids {
            match f.glyphs.get(&gid) {
                Some(ops) => glyphs.set(&gid.to_string(), path_data(ops)),
                None => missing.push(gid as u32),
            }
        }
        Some(
            Json::obj()
                .with("font", font)
                .with("upem", n2(f.units_per_em))
                .with("glyphs", glyphs)
                .with("missing", missing)
                .to_string(),
        )
    }

    /// An image's pixels for the host: `{"image":3,"width":w,"height":h,"rgba":"<base64>"}`.
    /// `None` for an unknown image.
    pub fn image_json(&self, id: u32) -> Option<String> {
        let img = self.image(id)?;
        Some(
            Json::obj()
                .with("image", id)
                .with("width", img.width)
                .with("height", img.height)
                .with("rgba", base64(&img.rgba))
                .to_string(),
        )
    }
}

// ---------------------------------------------------------------------------------------------------
// Helpers: numbers, path data, base64
// ---------------------------------------------------------------------------------------------------

/// Round to `places` decimals (negative zero becomes zero; non-finite becomes zero).
fn round_f(v: f64, places: i32) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    let k = 10f64.powi(places);
    let r = (v * k).round() / k;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

fn round(v: f64, places: i32) -> Json {
    Json::Num(round_f(v, places))
}

fn n2(v: f64) -> Json {
    round(v, 2)
}

fn rect_json(r: &Rect) -> Json {
    Json::Arr(vec![n2(r.x), n2(r.y), n2(r.w), n2(r.h)])
}

fn push_num(out: &mut String, v: f64) {
    super::bus::json::write_num(out, round_f(v, 2));
}

/// SVG path data for the ops (absolute commands, numbers to 2 decimals): `M10 20L30 40Z`.
pub fn path_data(ops: &[PathOp]) -> String {
    let mut out = String::new();
    let nums = |out: &mut String, letter: char, vs: &[f64]| {
        out.push(letter);
        for (i, v) in vs.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            push_num(out, *v);
        }
    };
    for op in ops {
        match *op {
            PathOp::MoveTo(x, y) => nums(&mut out, 'M', &[x, y]),
            PathOp::LineTo(x, y) => nums(&mut out, 'L', &[x, y]),
            PathOp::QuadTo(x1, y1, x, y) => nums(&mut out, 'Q', &[x1, y1, x, y]),
            PathOp::CubicTo(x1, y1, x2, y2, x, y) => nums(&mut out, 'C', &[x1, y1, x2, y2, x, y]),
            PathOp::Close => out.push('Z'),
        }
    }
    out
}

fn stops_json(stops: &[GradientStop]) -> Json {
    Json::Arr(stops.iter().map(|s| Json::Arr(vec![round(s.offset.clamp(0.0, 1.0), 4), Json::from(s.color.to_hex())])).collect())
}

fn paint_json(p: &Paint) -> Json {
    match p {
        Paint::Solid(c) => Json::obj().with("k", "solid").with("c", c.to_hex()),
        Paint::Linear { x0, y0, x1, y1, stops } => Json::obj()
            .with("k", "linear")
            .with("x0", n2(*x0))
            .with("y0", n2(*y0))
            .with("x1", n2(*x1))
            .with("y1", n2(*y1))
            .with("stops", stops_json(stops)),
        Paint::Radial { fx, fy, fr, cx, cy, r, stops } => Json::obj()
            .with("k", "radial")
            .with("fx", n2(*fx))
            .with("fy", n2(*fy))
            .with("fr", n2(*fr))
            .with("cx", n2(*cx))
            .with("cy", n2(*cy))
            .with("r", n2(*r))
            .with("stops", stops_json(stops)),
    }
}

fn stroke_json(s: &Stroke) -> Json {
    let cap = match s.cap {
        LineCap::Butt => "butt",
        LineCap::Round => "round",
        LineCap::Square => "square",
    };
    let join = match s.join {
        LineJoin::Miter => "miter",
        LineJoin::Round => "round",
        LineJoin::Bevel => "bevel",
    };
    Json::obj()
        .with("w", n2(s.width))
        .with("c", s.color.to_hex())
        .with("dash", Json::Arr(s.dash.iter().map(|d| n2(*d)).collect()))
        .with("off", n2(s.dash_offset))
        .with("cap", cap)
        .with("join", join)
        .with("ml", n2(s.miter_limit))
}

/// Standard base64 (RFC 4648, `+/`, `=` padding).
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ABC[(n >> 18) as usize & 63] as char);
        out.push(ABC[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ABC[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ABC[n as usize & 63] as char } else { '=' });
    }
    out
}

// ---------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground::bus::json;

    fn square(x: f64, y: f64, s: f64) -> Vec<PathOp> {
        vec![PathOp::MoveTo(x, y), PathOp::LineTo(x + s, y), PathOp::LineTo(x + s, y + s), PathOp::LineTo(x, y + s), PathOp::Close]
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn close_m(a: &Matrix, b: &Matrix) -> bool {
        close(a.a, b.a) && close(a.b, b.b) && close(a.c, b.c) && close(a.d, b.d) && close(a.e, b.e) && close(a.f, b.f)
    }

    #[test]
    fn base64_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd, 0x00]), "//79AA==");
    }

    #[test]
    fn matrix_maths() {
        let t = Matrix::translate(10.0, 20.0);
        let s = Matrix::scale(2.0, 3.0);
        assert_eq!(t.apply(1.0, 1.0), (11.0, 21.0));
        // multiply applies the right-hand matrix first.
        let ts = t.multiply(&s);
        assert_eq!(ts.apply(1.0, 1.0), (12.0, 23.0));
        assert_eq!(ts.apply(1.0, 1.0), t.apply(s.apply(1.0, 1.0).0, s.apply(1.0, 1.0).1));
        assert_eq!(s.then(&t), ts);
        assert_eq!(s.multiply(&t).apply(1.0, 1.0), (22.0, 63.0));
        let r = Matrix::rotate(std::f64::consts::FRAC_PI_2);
        let (x, y) = r.apply(1.0, 0.0);
        assert!(close(x, 0.0) && close(y, 1.0));
        let m = Matrix::new(2.0, 1.0, -1.0, 3.0, 5.0, -7.0);
        let inv = m.invert().unwrap();
        assert!(close_m(&m.multiply(&inv), &Matrix::IDENTITY));
        assert!(close_m(&inv.multiply(&m), &Matrix::IDENTITY));
        let (px, py) = m.apply(4.0, -2.0);
        let (bx, by) = inv.apply(px, py);
        assert!(close(bx, 4.0) && close(by, -2.0));
        assert!(Matrix::scale(0.0, 1.0).invert().is_none());
        assert!(Matrix::default().is_identity());
        let rr = Matrix::rotate(std::f64::consts::FRAC_PI_4).apply_rect(&Rect::new(0.0, 0.0, 2.0, 2.0));
        assert!(close(rr.w, 2.0 * 2f64.sqrt()) && close(rr.y, 0.0));
    }

    #[test]
    fn rect_basics() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 5.0, 5.0, 5.0);
        assert!(a.intersects(&b), "touching counts");
        assert!(!a.intersects(&Rect::new(10.1, 0.0, 1.0, 1.0)));
        assert_eq!(a.intersection(&Rect::new(5.0, 5.0, 10.0, 10.0)), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 15.0, 10.0));
        assert_eq!(Rect::from_ltrb(5.0, 6.0, 1.0, 2.0), Rect::new(1.0, 2.0, 4.0, 4.0));
        assert!(a.contains(10.0, 0.0) && !a.contains(-0.1, 0.0));
        assert_eq!(a.inflate(1.0), Rect::new(-1.0, -1.0, 12.0, 12.0));
    }

    #[test]
    fn bounds_of_each_item() {
        let p = Item::Path { ops: square(10.0, 20.0, 5.0), fill: Some(Paint::Solid(Color::BLACK)), stroke: None };
        assert_eq!(p.bounds(), Some(Rect::new(10.0, 20.0, 5.0, 5.0)));
        let mut st = Stroke::new(2.0, Color::BLACK);
        st.join = LineJoin::Round;
        let ps = Item::Path { ops: square(10.0, 20.0, 5.0), fill: None, stroke: Some(st.clone()) };
        assert_eq!(ps.bounds(), Some(Rect::new(9.0, 19.0, 7.0, 7.0)));
        st.join = LineJoin::Miter; // limit 10 -> reach 10
        let pm = Item::Path { ops: vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(10.0, 0.0)], fill: None, stroke: Some(st) };
        assert_eq!(pm.bounds(), Some(Rect::new(-10.0, -10.0, 30.0, 20.0)));
        let curve = Item::Path {
            ops: vec![PathOp::MoveTo(0.0, 0.0), PathOp::QuadTo(5.0, -10.0, 10.0, 0.0), PathOp::CubicTo(12.0, 4.0, 14.0, 9.0, 8.0, 3.0)],
            fill: None,
            stroke: None,
        };
        assert_eq!(curve.bounds(), Some(Rect::new(0.0, -10.0, 14.0, 19.0)));
        assert_eq!(Item::Path { ops: vec![], fill: None, stroke: None }.bounds(), None);

        let g = Item::Glyphs {
            font: 1,
            size: 10.0,
            color: Color::BLACK,
            glyphs: vec![PlacedGlyph { gid: 1, x: 100.0, y: 50.0 }, PlacedGlyph { gid: 2, x: 106.0, y: 50.0 }],
        };
        assert_eq!(g.bounds(), Some(Rect::from_ltrb(95.0, 38.0, 121.0, 55.0)));
        assert_eq!(Item::Glyphs { font: 1, size: 10.0, color: Color::BLACK, glyphs: vec![] }.bounds(), None);

        let img = Item::Image { image: 1, rect: Rect::new(1.0, 2.0, 3.0, 4.0) };
        assert_eq!(img.bounds(), Some(Rect::new(1.0, 2.0, 3.0, 4.0)));
        let flipped = Item::Image { image: 1, rect: Rect::new(10.0, 10.0, -4.0, -4.0) };
        assert_eq!(flipped.bounds(), Some(Rect::new(6.0, 6.0, 4.0, 4.0)));
        let link = Item::Link { rect: Rect::new(0.0, 0.0, 5.0, 5.0), target: "x".into() };
        assert_eq!(link.bounds(), Some(Rect::new(0.0, 0.0, 5.0, 5.0)));
        for s in [Item::PushClip(square(0.0, 0.0, 1.0)), Item::PopClip, Item::PushTransform(Matrix::IDENTITY), Item::PopTransform, Item::PushOpacity(0.5), Item::PopOpacity] {
            assert_eq!(s.bounds(), None);
            assert!(s.is_state());
        }
    }

    #[test]
    fn json_shape() {
        let mut dl = DisplayList::new(612.0, 792.0);
        let mut st = Stroke::new(1.5, Color::rgba(0, 0, 255, 128));
        st.dash = vec![3.0, 1.0];
        st.cap = LineCap::Round;
        dl.push(Item::Path {
            ops: vec![PathOp::MoveTo(1.234, -2.0), PathOp::LineTo(3.0, 4.005), PathOp::QuadTo(1.0, 2.0, 3.0, 4.0), PathOp::CubicTo(1.0, 2.0, 3.0, 4.0, 5.0, 6.0), PathOp::Close],
            fill: Some(Paint::Solid(Color::rgb(255, 0, 0))),
            stroke: Some(st),
        });
        dl.push(Item::Glyphs { font: 7, size: 12.0, color: Color::BLACK, glyphs: vec![PlacedGlyph { gid: 36, x: 72.0, y: 100.126 }] });
        dl.push(Item::Image { image: 3, rect: Rect::new(0.0, 0.0, 10.0, 20.0) });
        dl.push(Item::PushClip(vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(1.0, 1.0), PathOp::Close]));
        dl.push(Item::PopClip);
        dl.push(Item::PushTransform(Matrix::new(0.8660254037, 0.5, -0.5, 0.8660254037, 10.0, 20.0)));
        dl.push(Item::PopTransform);
        dl.push(Item::PushOpacity(0.5));
        dl.push(Item::PopOpacity);
        dl.push(Item::Link { rect: Rect::new(1.0, 2.0, 3.0, 4.0), target: "https://example.com/\"q\"".into() });
        dl.push(Item::Path {
            ops: square(0.0, 0.0, 1.0),
            fill: Some(Paint::Linear { x0: 0.0, y0: 0.0, x1: 1.0, y1: 0.0, stops: vec![GradientStop { offset: 0.0, color: Color::WHITE }, GradientStop { offset: 1.0, color: Color::BLACK }] }),
            stroke: None,
        });
        dl.push(Item::Path {
            ops: vec![],
            fill: Some(Paint::Radial { fx: 1.0, fy: 1.0, fr: 0.0, cx: 1.0, cy: 1.0, r: 5.0, stops: vec![GradientStop { offset: 0.33333, color: Color::TRANSPARENT }] }),
            stroke: None,
        });
        let expected = concat!(
            r#"{"w":612,"h":792,"items":["#,
            r##"{"t":"path","d":"M1.23 -2L3 4.01Q1 2 3 4C1 2 3 4 5 6Z","fill":{"k":"solid","c":"#ff0000"},"stroke":{"w":1.5,"c":"#0000ff80","dash":[3,1],"off":0,"cap":"round","join":"miter","ml":10}},"##,
            r##"{"t":"glyphs","font":7,"size":12,"c":"#000000","g":[36,72,100.13]},"##,
            r#"{"t":"image","image":3,"r":[0,0,10,20]},"#,
            r#"{"t":"clip","d":"M0 0L1 1Z"},{"t":"unclip"},"#,
            r#"{"t":"transform","m":[0.866025,0.5,-0.5,0.866025,10,20]},{"t":"untransform"},"#,
            r#"{"t":"opacity","a":0.5},{"t":"unopacity"},"#,
            r#"{"t":"link","r":[1,2,3,4],"target":"https://example.com/\"q\""},"#,
            r##"{"t":"path","d":"M0 0L1 0L1 1L0 1Z","fill":{"k":"linear","x0":0,"y0":0,"x1":1,"y1":0,"stops":[[0,"#ffffff"],[1,"#000000"]]}},"##,
            r##"{"t":"path","d":"","fill":{"k":"radial","fx":1,"fy":1,"fr":0,"cx":1,"cy":1,"r":5,"stops":[[0.3333,"#00000000"]]}}"##,
            "]}"
        );
        assert_eq!(dl.to_json(), expected);
        // It is valid JSON and parses back to the same shape.
        let v = json::parse(&dl.to_json()).unwrap();
        assert_eq!(v.get("items").unwrap().as_arr().unwrap().len(), 12);
        assert_eq!(v, dl.to_json_value());
    }

    #[test]
    fn visible_items_follow_transforms_and_clips() {
        let mut dl = DisplayList::new(200.0, 200.0);
        let fill = || Some(Paint::Solid(Color::BLACK));
        dl.push(Item::Path { ops: square(0.0, 0.0, 10.0), fill: fill(), stroke: None }); // 0: top-left
        dl.push(Item::PushTransform(Matrix::translate(100.0, 100.0))); // 1
        dl.push(Item::Path { ops: square(0.0, 0.0, 10.0), fill: fill(), stroke: None }); // 2: at 100,100
        dl.push(Item::PopTransform); // 3
        dl.push(Item::PushClip(square(0.0, 0.0, 50.0))); // 4
        dl.push(Item::Path { ops: square(150.0, 150.0, 10.0), fill: fill(), stroke: None }); // 5: clipped away
        dl.push(Item::Path { ops: square(40.0, 40.0, 20.0), fill: fill(), stroke: None }); // 6: partly inside
        dl.push(Item::PopClip); // 7
        dl.push(Item::Image { image: 1, rect: Rect::new(150.0, 150.0, 10.0, 10.0) }); // 8

        let wb = dl.world_bounds();
        assert_eq!(wb[2], Some(Rect::new(100.0, 100.0, 10.0, 10.0)));
        assert_eq!(wb[5], None);
        assert_eq!(wb[6], Some(Rect::new(40.0, 40.0, 10.0, 10.0)));

        assert_eq!(dl.visible_items(Rect::new(0.0, 0.0, 20.0, 20.0)), vec![0, 1, 3, 4, 7]);
        assert_eq!(dl.visible_items(Rect::new(95.0, 95.0, 10.0, 10.0)), vec![1, 2, 3, 4, 7]);
        assert_eq!(dl.visible_items(Rect::new(140.0, 140.0, 60.0, 60.0)), vec![1, 3, 4, 7, 8]);
        assert_eq!(dl.visible_items(Rect::new(45.0, 45.0, 2.0, 2.0)), vec![1, 3, 4, 6, 7]);
        // Unbalanced pops do not panic.
        let mut odd = DisplayList::new(10.0, 10.0);
        odd.push(Item::PopTransform);
        odd.push(Item::PopClip);
        odd.push(Item::Image { image: 1, rect: Rect::new(0.0, 0.0, 1.0, 1.0) });
        assert_eq!(odd.visible_items(Rect::new(0.0, 0.0, 10.0, 10.0)), vec![0, 1, 2]);
    }

    #[test]
    fn resources_json() {
        let mut res = Resources::new();
        res.add_font(1, 2048.0);
        res.set_glyph(1, 36, vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(500.0, 1400.0), PathOp::Close]);
        res.set_glyph(1, 3, vec![]);
        assert_eq!(res.glyph(1, 36).unwrap().len(), 3);
        assert_eq!(
            res.glyphs_json(1, &[36, 3, 99]).unwrap(),
            r#"{"font":1,"upem":2048,"glyphs":{"36":"M0 0L500 1400Z","3":""},"missing":[99]}"#
        );
        assert!(res.glyphs_json(2, &[1]).is_none());
        res.set_glyph(5, 1, vec![PathOp::MoveTo(1.0, 1.0)]);
        assert_eq!(res.font(5).unwrap().units_per_em, 1000.0);
        assert_eq!(res.add_font(1, 1.0).units_per_em, 2048.0, "add_font keeps an existing font");

        res.add_image(ImageRes { id: 4, width: 1, height: 1, rgba: b"Man!".to_vec() });
        assert_eq!(res.image_json(4).unwrap(), r#"{"image":4,"width":1,"height":1,"rgba":"TWFuIQ=="}"#);
        res.add_image(ImageRes { id: 4, width: 1, height: 1, rgba: vec![0, 0, 0, 0] });
        assert_eq!(res.images.len(), 1);
        assert!(res.image_json(9).is_none());
    }

    #[test]
    fn colors_and_path_data() {
        assert_eq!(Color::rgb(1, 2, 255).to_hex(), "#0102ff");
        assert_eq!(Color::rgba(1, 2, 3, 0).to_hex(), "#01020300");
        assert_eq!(path_data(&[PathOp::MoveTo(-0.001, 1e-9), PathOp::LineTo(-1.5, 1234567.891)]), "M0 0L-1.5 1234567.89");
    }
}
