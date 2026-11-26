//! A slide to a display list: background, the master's and the layout's own shapes, then the slide's
//! shapes in z-order, each resolved through the placeholder chain (slide -> layout -> master ->
//! master text styles -> presentation defaults -> theme).

use std::collections::HashMap;

use crate::ground::display::{Color, DisplayList, ImageRes, Item, Matrix, Paint, PathOp, Rect, Resources, Stroke};
use crate::ground::draw::{
    find_fill, parse_fill, parse_line, shape_geometry, shape_items, ColorMap, ColorMod, ColorSpec, Fill, Geometry, Line, PictureFill,
    PictureMode, Theme, Xfrm,
};
use crate::ground::opc::name_key;
use crate::ground::picture::{jpeg, png};
use crate::ground::text::{layout_paragraph, Align, FontCache, LineSpacing, ParaOptions, ParagraphLayout, StyledRun, TabKind, TabStop, TextStyle};
use crate::ground::xml::Element;

use super::model::{find_layout_ph, find_master_ph, shape_children, shape_id, sp_tree, table_of, Deck, MasterStyles, Ph, TextCat};
use super::style::{autonum_text, emu_attr_pt, is_invisible, on, BodyProps, BulletKind, BulletSize, ListStyle, ParaProps, RunProps, Spacing};

const EMU: f64 = 12700.0;

/// Who owns a shape being drawn on a slide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Slide,
    Layout,
    Master,
}

/// Everything one slide's drawing needs to know about its chain.
pub struct Scene<'a> {
    pub deck: &'a Deck,
    pub index: usize,
    pub theme: &'a Theme,
    pub cmap: ColorMap,
    pub slide: &'a Element,
    pub layout: Option<&'a Element>,
    pub master: Option<&'a Element>,
    pub slide_part: &'a str,
    pub layout_part: Option<&'a str>,
    pub master_part: Option<&'a str>,
    pub styles: Option<&'a MasterStyles>,
}

impl<'a> Scene<'a> {
    pub fn new(deck: &'a Deck, index: usize) -> Option<Scene<'a>> {
        let e = deck.slides.get(index)?;
        let slide = deck.sheet(&e.part)?;
        let master_part = e.master.as_deref();
        Some(Scene {
            deck,
            index,
            theme: deck.theme_of(master_part),
            cmap: deck.color_map(index),
            slide,
            layout: e.layout.as_deref().and_then(|l| deck.sheet(l)),
            master: master_part.and_then(|m| deck.sheet(m)),
            slide_part: &e.part,
            layout_part: e.layout.as_deref(),
            master_part,
            styles: deck.styles_of(master_part),
        })
    }

    fn part_of(&self, owner: Owner) -> &'a str {
        match owner {
            Owner::Slide => self.slide_part,
            Owner::Layout => self.layout_part.unwrap_or(""),
            Owner::Master => self.master_part.unwrap_or(""),
        }
    }

    pub fn color(&self, spec: &ColorSpec) -> Color {
        spec.resolve(self.theme, &self.cmap, None)
    }

    fn slide_number(&self) -> usize {
        self.deck.slide_number(self.index)
    }
}

/// A shape with the placeholders it inherits from.
pub struct ShapeRes<'a> {
    pub el: &'a Element,
    pub owner: Owner,
    pub ph: Option<Ph>,
    pub layout_el: Option<&'a Element>,
    pub master_el: Option<&'a Element>,
}

pub fn resolve_shape<'a>(scene: &Scene<'a>, el: &'a Element, owner: Owner) -> ShapeRes<'a> {
    let ph = Ph::of(el);
    let mut layout_el = None;
    let mut master_el = None;
    if let Some(p) = &ph {
        if owner == Owner::Slide {
            layout_el = scene.layout.and_then(sp_tree).and_then(|t| find_layout_ph(t, p));
        }
        if owner != Owner::Master {
            let via = layout_el.and_then(Ph::of).unwrap_or_else(|| p.clone());
            master_el = scene.master.and_then(sp_tree).and_then(|t| find_master_ph(t, &via));
        }
    }
    ShapeRes { el, owner, ph, layout_el, master_el }
}

impl<'a> ShapeRes<'a> {
    fn chain(&self) -> Vec<&'a Element> {
        let mut v = vec![self.el];
        v.extend(self.layout_el);
        v.extend(self.master_el);
        v
    }

    fn sp_prs(&self) -> Vec<&'a Element> {
        self.chain().into_iter().filter_map(|e| e.child("spPr").or_else(|| e.child("grpSpPr"))).collect()
    }

    /// The position: the first of the chain that has one (`p:xfrm` for graphic frames).
    pub fn xfrm(&self) -> Option<Xfrm> {
        for e in self.chain() {
            let x = if e.local() == "graphicFrame" { e.child("xfrm") } else { e.child("spPr").and_then(|s| s.child("xfrm")) };
            if let Some(x) = x {
                return Some(Xfrm::parse(x));
            }
        }
        None
    }

    pub fn geometry(&self) -> Geometry {
        self.sp_prs().into_iter().find_map(Geometry::parse).unwrap_or_else(|| Geometry::preset("rect"))
    }

    pub fn style(&self) -> Option<&'a Element> {
        self.chain().into_iter().find_map(|e| e.child("style"))
    }

    pub fn fill(&self, scene: &Scene) -> Option<Fill> {
        for sp in self.sp_prs() {
            if let Some(f) = find_fill(sp) {
                return Some(f.resolve(scene.theme, &scene.cmap, None));
            }
        }
        let r = self.style()?.child("fillRef")?;
        let idx: u32 = r.attr("idx")?.trim().parse().ok()?;
        let ph_color = ColorSpec::parse(r).map(|c| scene.color(&c));
        scene.theme.fill_ref(idx).map(|f| f.resolve(scene.theme, &scene.cmap, ph_color))
    }

    pub fn line(&self, scene: &Scene) -> Option<Line> {
        let own = self.sp_prs().into_iter().find_map(|s| s.child("ln")).map(parse_line).map(|l| l.resolve(scene.theme, &scene.cmap, None));
        let styled = self.style().and_then(|s| s.child("lnRef")).and_then(|r| {
            let idx: u32 = r.attr("idx")?.trim().parse().ok()?;
            let ph_color = ColorSpec::parse(r).map(|c| scene.color(&c));
            scene.theme.line_ref(idx).map(|l| l.resolve(scene.theme, &scene.cmap, ph_color))
        });
        match (own, styled) {
            (Some(o), Some(s)) => Some(o.over(&s)),
            (Some(o), None) => Some(o),
            (None, s) => s,
        }
    }

    pub fn body(&self) -> BodyProps {
        let mut b = BodyProps::default();
        for e in self.chain() {
            if let Some(bp) = e.child("txBody").and_then(|t| t.child("bodyPr")) {
                b.fill_from(&BodyProps::parse(bp));
            }
        }
        b
    }

    pub fn text_chain(&self, scene: &Scene) -> TextChain {
        let lists = self.chain().into_iter().filter_map(|e| e.child("txBody").and_then(|t| t.child("lstStyle"))).map(ListStyle::parse).collect();
        let mid = self.style().and_then(|s| s.child("fontRef")).map(|f| {
            let mut rp = RunProps::default();
            match f.attr("idx").map(str::trim) {
                Some("major") => {
                    rp.latin = Some("+mj-lt".into());
                    rp.ea = Some("+mj-ea".into());
                }
                Some("minor") => {
                    rp.latin = Some("+mn-lt".into());
                    rp.ea = Some("+mn-ea".into());
                }
                _ => {}
            }
            rp.color = ColorSpec::parse(f);
            rp
        });
        let cat = TextCat::of(self.ph.as_ref());
        let mut base = Vec::new();
        if let Some(st) = scene.styles {
            base.push(st.get(cat).clone());
        }
        base.push(scene.deck.default_style.clone());
        TextChain { lists, mid, base }
    }
}

/// The list styles a text body inherits, highest first, with a shape style's font reference between
/// the placeholder chain and the master's text styles.
#[derive(Debug, Clone, Default)]
pub struct TextChain {
    pub lists: Vec<ListStyle>,
    pub mid: Option<RunProps>,
    pub base: Vec<ListStyle>,
}

impl TextChain {
    pub fn para(&self, ppr: Option<&Element>, lvl: usize) -> ParaProps {
        let mut p = ppr.map(ParaProps::parse).unwrap_or_default();
        for ls in &self.lists {
            if let Some(l) = ls.level(lvl) {
                p.fill_from(l);
            }
        }
        if let Some(m) = &self.mid {
            p.run.fill_from(m);
        }
        for ls in &self.base {
            if let Some(l) = ls.level(lvl) {
                p.fill_from(l);
            }
        }
        p
    }
}

/// One paragraph with every property resolved, ready for layout.
#[derive(Debug, Clone, PartialEq)]
pub struct RPara {
    pub level: usize,
    pub align: Align,
    /// Points.
    pub mar_l: f64,
    pub mar_r: f64,
    pub indent: f64,
    pub line_spacing: LineSpacing,
    pub space_before: f64,
    pub space_after: f64,
    /// The bullet (or number) with its separator, when the paragraph shows one.
    pub bullet: Option<StyledRun>,
    pub runs: Vec<StyledRun>,
    /// The style of an empty paragraph (its end mark).
    pub end_style: TextStyle,
    pub def_tab: f64,
    pub tabs: Vec<(f64, String)>,
    /// Where each run's text comes from: one per entry of `runs`.
    pub segs: Vec<Seg>,
    /// The paragraph's length in characters (a line break and a field count one).
    pub model_len: usize,
}

/// One drawn run's place in its paragraph: the first character (counting as [`super::text`] does),
/// how many it covers there, and the text drawn for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Seg {
    pub model: usize,
    pub mlen: usize,
    pub text: String,
    /// A field: one character drawn as many.
    pub atomic: bool,
}

fn align_of(a: Option<&str>) -> Align {
    match a {
        Some("ctr") => Align::Center,
        Some("r") => Align::Right,
        Some("just") | Some("justLow") => Align::Justify,
        Some("dist") | Some("thaiDist") => Align::Distribute,
        _ => Align::Left,
    }
}

/// A run's style from its resolved properties (sizes scaled by an autofit's font scale).
pub fn text_style(rp: &RunProps, theme: &Theme, cmap: &ColorMap, scale: f64) -> TextStyle {
    let latin = rp.latin.as_deref().unwrap_or("+mn-lt");
    let mut family = theme.resolve_typeface(latin).to_string();
    if family.trim().is_empty() {
        family = theme.minor_font.latin.clone();
    }
    let east_asian_family = rp.ea.as_deref().map(|e| theme.resolve_typeface(e).to_string()).unwrap_or_default();
    let mut size = rp.size.unwrap_or(18.0) * scale;
    if rp.baseline.is_some_and(|b| b != 0.0) {
        size *= 2.0 / 3.0;
    }
    TextStyle {
        family,
        east_asian_family,
        size_pt: size.max(0.5),
        weight: if rp.bold.unwrap_or(false) { 700 } else { 400 },
        italic: rp.italic.unwrap_or(false),
        color: rp.color.as_ref().map(|c| c.resolve(theme, cmap, None)).unwrap_or_else(|| theme.scheme_color("tx1", cmap)),
        letter_spacing: rp.spacing.unwrap_or(0.0) * scale,
        underline: rp.underline.unwrap_or(false),
        strike: rp.strike.unwrap_or(false),
    }
}

/// Wingdings and Symbol bullets to the Unicode characters they draw.
fn symbol_bullet(face: &str, ch: &str) -> Option<&'static str> {
    let f = face.to_ascii_lowercase();
    if !(f.starts_with("wingdings") || f == "symbol") {
        return None;
    }
    Some(match ch {
        "§" | "n" => "\u{25AA}",
        "Ø" => "\u{27A2}",
        "ü" => "\u{2713}",
        "q" | "o" => "\u{25A1}",
        "v" => "\u{2756}",
        "l" | "·" => "\u{2022}",
        "Ÿ" => "\u{2022}",
        "à" | "è" => "\u{2192}",
        _ => "\u{2022}",
    })
}

/// The paragraphs of a text body, resolved through `chain`.
pub fn resolve_paras(scene: &Scene, tx: &Element, chain: &TextChain, font_scale: f64, line_reduction: f64) -> Vec<RPara> {
    let theme = scene.theme;
    let cmap = &scene.cmap;
    let mut counters: [(u32, String); 9] = Default::default();
    let mut out = Vec::new();
    for p in tx.children_named("p") {
        let ppr = p.child("pPr");
        let lvl = ppr.and_then(|e| e.attr("lvl")).and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(0).min(8);
        let props = chain.para(ppr, lvl);
        let style_of = |el: Option<&Element>| -> (TextStyle, RunProps) {
            let mut rp = el.map(RunProps::parse).unwrap_or_default();
            rp.fill_from(&props.run);
            (text_style(&rp, theme, cmap, font_scale), rp)
        };
        let mut runs: Vec<StyledRun> = Vec::new();
        let mut segs: Vec<Seg> = Vec::new();
        let mut model = 0usize;
        for r in p.elements() {
            let mlen = match r.local() {
                "r" => r.child("t").map(|t| t.text().chars().count()).unwrap_or(0),
                "br" | "fld" => 1,
                _ => 0,
            };
            let at = model;
            model += mlen;
            let text = match r.local() {
                "r" => r.child("t").map(|t| t.text()).unwrap_or_default(),
                "br" => "\n".to_string(),
                "fld" => {
                    if r.attr("type").is_some_and(|t| t == "slidenum") {
                        scene.slide_number().to_string()
                    } else {
                        r.child("t").map(|t| t.text()).unwrap_or_default()
                    }
                }
                _ => continue,
            };
            let (style, rp) = style_of(r.child("rPr"));
            let text = text.replace('\u{b}', "\n");
            let text = if rp.caps.as_deref() == Some("all") { text.to_uppercase() } else { text };
            if text.is_empty() || is_invisible(style.color) && text.trim().is_empty() {
                continue;
            }
            segs.push(Seg { model: at, mlen, text: text.clone(), atomic: r.local() == "fld" });
            runs.push(StyledRun { text, style });
        }
        let (end_style, _) = style_of(p.child("endParaRPr"));
        let has_text = runs.iter().any(|r| !r.text.trim().is_empty());

        // Numbering: deeper levels reset, an unnumbered paragraph at a level stops its count.
        for c in counters.iter_mut().skip(lvl + 1) {
            *c = (0, String::new());
        }
        let first = runs.first().map(|r| r.style.clone()).unwrap_or_else(|| end_style.clone());
        let bullet_text = match (&props.bullet, has_text) {
            (Some(BulletKind::Char(c)), true) => Some(c.clone()),
            (Some(BulletKind::AutoNum { scheme, start }), true) => {
                let c = &mut counters[lvl];
                if c.1 != *scheme || c.0 == 0 {
                    *c = (*start, scheme.clone());
                } else {
                    c.0 += 1;
                }
                Some(autonum_text(scheme, c.0))
            }
            (_, true) => {
                counters[lvl] = (0, String::new());
                None
            }
            _ => None,
        };
        let indent = props.indent.unwrap_or(0.0);
        let mar_l = props.mar_l.unwrap_or(0.0);
        let bullet = bullet_text.map(|b| {
            let mut st = first.clone();
            st.underline = false;
            st.strike = false;
            let mut text = b;
            match &props.bullet_font {
                Some(Some(face)) => {
                    let face = theme.resolve_typeface(face).to_string();
                    if let Some(u) = symbol_bullet(&face, &text) {
                        text = u.to_string();
                    } else if !face.trim().is_empty() {
                        st.family = face;
                    }
                }
                _ => {}
            }
            if let Some(Some(c)) = &props.bullet_color {
                st.color = c.resolve(theme, cmap, None);
            }
            match props.bullet_size {
                Some(BulletSize::Pct(p)) => st.size_pt = (first.size_pt * p).max(0.5),
                Some(BulletSize::Pts(v)) => st.size_pt = (v * font_scale).max(0.5),
                _ => {}
            }
            text.push(if indent < 0.0 { '\t' } else { ' ' });
            StyledRun { text, style: st }
        });
        let line_spacing = match props.line_spacing {
            Some(Spacing::Pct(p)) => LineSpacing::Multiple((p - line_reduction).max(0.1)),
            Some(Spacing::Pts(v)) => LineSpacing::Exactly(v * font_scale),
            None if line_reduction > 0.0 => LineSpacing::Multiple((1.0 - line_reduction).max(0.1)),
            None => LineSpacing::Single,
        };
        let space = |s: Option<Spacing>| match s {
            Some(Spacing::Pct(p)) => p * first.size_pt * 1.2,
            Some(Spacing::Pts(v)) => v,
            None => 0.0,
        };
        out.push(RPara {
            level: lvl,
            align: align_of(props.align.as_deref()),
            mar_l,
            mar_r: props.mar_r.unwrap_or(0.0),
            indent,
            line_spacing,
            space_before: space(props.space_before),
            space_after: space(props.space_after),
            bullet,
            runs,
            end_style,
            def_tab: props.def_tab.unwrap_or(72.0),
            tabs: props.tabs.clone().unwrap_or_default(),
            segs,
            model_len: model,
        });
    }
    out
}

/// Laid-out paragraphs.
#[derive(Debug, Clone)]
pub struct TextBlock {
    pub paras: Vec<ParagraphLayout>,
    /// Each paragraph's alignment and natural width (for unwrapped text).
    pub aligns: Vec<(Align, f64)>,
    pub height: f64,
    pub wrap: bool,
}

pub fn layout_block(fonts: &mut FontCache, paras: &[RPara], width: f64, wrap: bool) -> TextBlock {
    let mut out = Vec::new();
    let mut aligns = Vec::new();
    let mut height = 0.0;
    for p in paras {
        let mut runs: Vec<StyledRun> = Vec::new();
        runs.extend(p.bullet.iter().cloned());
        runs.extend(p.runs.iter().cloned());
        if runs.is_empty() {
            runs.push(StyledRun { text: String::new(), style: p.end_style.clone() });
        }
        let mut opts = ParaOptions::new(if wrap { width.max(1.0) } else { 1.0e6 });
        opts.align = if wrap { p.align } else { Align::Left };
        opts.left_indent = p.mar_l;
        opts.right_indent = if wrap { p.mar_r } else { 0.0 };
        opts.first_line_indent = p.indent;
        opts.line_spacing = p.line_spacing;
        opts.space_before = p.space_before;
        opts.space_after = p.space_after;
        opts.default_tab_width = if p.def_tab > 0.0 { p.def_tab } else { 72.0 };
        if p.bullet.is_some() && p.indent < 0.0 {
            opts.tab_stops.push(TabStop::new(p.mar_l, TabKind::Left));
        }
        for (pos, kind) in &p.tabs {
            let k = match kind.as_str() {
                "ctr" => TabKind::Center,
                "r" => TabKind::Right,
                "dec" => TabKind::Decimal,
                _ => TabKind::Left,
            };
            opts.tab_stops.push(TabStop::new(*pos, k));
        }
        let lay = layout_paragraph(fonts, &runs, &opts);
        let natural = lay.lines.iter().map(|l| l.x + l.width).fold(0.0, f64::max);
        height += lay.height;
        aligns.push((p.align, natural));
        out.push(lay);
    }
    TextBlock { paras: out, aligns, height, wrap }
}

/// The block's items placed in `inner` with a vertical anchor (t, ctr, b).
pub fn place_block(block: &TextBlock, inner: Rect, anchor: &str) -> Vec<Item> {
    let mut y = match anchor {
        "ctr" => inner.y + (inner.h - block.height) / 2.0,
        "b" => inner.bottom() - block.height,
        _ => inner.y,
    };
    let mut items = Vec::new();
    for (lay, (align, natural)) in block.paras.iter().zip(&block.aligns) {
        let dx = if block.wrap {
            0.0
        } else {
            match align {
                Align::Center => (inner.w - natural) / 2.0,
                Align::Right => inner.w - natural,
                _ => 0.0,
            }
        };
        items.extend(lay.to_items(inner.x + dx, y));
        y += lay.height;
    }
    items
}

/// Where each laid-out paragraph sits (the same places [`place_block`] draws them at).
pub fn place_geom(block: &TextBlock, paras: &[RPara], inner: Rect, anchor: &str) -> Vec<ParaGeom> {
    let mut y = match anchor {
        "ctr" => inner.y + (inner.h - block.height) / 2.0,
        "b" => inner.bottom() - block.height,
        _ => inner.y,
    };
    let mut out = Vec::new();
    for ((lay, (align, natural)), rp) in block.paras.iter().zip(&block.aligns).zip(paras) {
        let dx = if block.wrap {
            0.0
        } else {
            match align {
                Align::Center => (inner.w - natural) / 2.0,
                Align::Right => inner.w - natural,
                _ => 0.0,
            }
        };
        out.push(ParaGeom {
            lay: lay.clone(),
            x: inner.x + dx,
            y,
            bullet: rp.bullet.as_ref().map(|b| b.text.len()).unwrap_or(0),
            segs: rp.segs.clone(),
            model_len: rp.model_len,
        });
        y += lay.height;
    }
    out
}

/// One paragraph as laid out on a slide, with the way back from its drawn text to its characters.
#[derive(Debug, Clone)]
pub struct ParaGeom {
    pub lay: ParagraphLayout,
    /// Top-left of the paragraph in the text's own space (points).
    pub x: f64,
    pub y: f64,
    /// Bytes of the bullet (or number) at the start of the drawn text.
    pub bullet: usize,
    pub segs: Vec<Seg>,
    pub model_len: usize,
}

impl ParaGeom {
    /// A character place to a byte offset in the drawn text.
    pub fn to_layout(&self, ch: usize) -> usize {
        let mut l = self.bullet;
        for s in &self.segs {
            if ch < s.model {
                return l;
            }
            if ch <= s.model + s.mlen {
                let k = ch - s.model;
                if s.atomic {
                    return if k == 0 { l } else { l + s.text.len() };
                }
                return l + s.text.char_indices().nth(k).map(|(b, _)| b).unwrap_or(s.text.len());
            }
            l += s.text.len();
        }
        l
    }

    /// A byte offset in the drawn text to a character place.
    pub fn to_model(&self, off: usize) -> usize {
        if off <= self.bullet {
            return self.segs.first().map(|s| s.model).unwrap_or(0).min(self.model_len);
        }
        let mut l = self.bullet;
        for s in &self.segs {
            if off <= l + s.text.len() {
                let part = &s.text[..(off - l).min(s.text.len())];
                let k = part.chars().count();
                if s.atomic {
                    return s.model + usize::from(k > 0);
                }
                return s.model + k.min(s.mlen);
            }
            l += s.text.len();
        }
        self.model_len
    }
}

/// A text area of a shape: its paragraphs and the transform from its own space to the slide.
#[derive(Debug, Clone)]
pub struct TextGeom {
    pub m: Matrix,
    pub inner: Rect,
    pub paras: Vec<ParaGeom>,
}

/// A slide's shape as drawn, for the caret, the hit test and the selection handles.
#[derive(Debug, Clone)]
pub struct ShapeGeom {
    pub id: u32,
    /// The top-level shape on the slide (itself, or the outermost group around it).
    pub top: u32,
    /// The group directly around it.
    pub group: Option<u32>,
    pub kind: &'static str,
    /// The shape's own box (0,0)-(w,h) to slide points.
    pub m: Matrix,
    pub w: f64,
    pub h: f64,
    pub text: Option<TextGeom>,
    /// Table cells: (row, column, the cell's box in the table's space, its text).
    pub cells: Vec<(usize, usize, Rect, TextGeom)>,
    pub placeholder: bool,
    /// A text box (typing is what it is for, even when empty).
    pub text_box: bool,
}

/// What an empty placeholder shows until it is typed in (never in the show or the file).
pub fn prompt_text(ph: &Ph, layout_el: Option<&Element>) -> Option<String> {
    if let Some(l) = layout_el {
        let custom = l
            .elements()
            .find(|e| e.local().starts_with("nv"))
            .and_then(|n| n.child("nvPr"))
            .and_then(|n| n.child("ph"))
            .and_then(|p| p.attr("hasCustomPrompt"))
            .map(on)
            .unwrap_or(false);
        if custom {
            if let Some(t) = l.child("txBody").map(|t| super::model::text_of(t, None)).filter(|t| !t.trim().is_empty()) {
                return Some(t);
            }
        }
    }
    Some(
        match ph.ty.as_str() {
            "title" | "ctrTitle" => "Click to add title",
            "subTitle" => "Click to add subtitle",
            "pic" => "Click icon to add picture",
            "dt" | "ftr" | "sldNum" | "hdr" | "sldImg" => return None,
            _ => "Click to add text",
        }
        .to_string(),
    )
}

/// The alpha prompt text is drawn with: the slide show leaves out glyph runs of this alpha.
pub const PROMPT_ALPHA: u8 = 0xFE;

/// Does a text body have anything to draw?
fn has_text(tx: &Element) -> bool {
    tx.children_named("p").any(|p| p.elements().any(|r| matches!(r.local(), "r" | "fld") && r.child("t").is_some_and(|t| !t.text().is_empty())))
}

fn rect_ops(r: Rect) -> Vec<PathOp> {
    vec![PathOp::MoveTo(r.x, r.y), PathOp::LineTo(r.right(), r.y), PathOp::LineTo(r.right(), r.bottom()), PathOp::LineTo(r.x, r.bottom()), PathOp::Close]
}

/// Decode a PNG or JPEG.
pub fn decode_image(bytes: &[u8]) -> Option<jpeg::Image> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        png::decode(bytes).ok()
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        jpeg::decode(bytes).ok()
    } else {
        None
    }
}

/// The default table style PowerPoint writes for every new table (Medium Style 2 - Accent 1).
pub const MEDIUM_STYLE_2_ACCENT_1: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

/// Draws slides. Holds the font cache, the resources the lists refer to, and the decoded images.
pub struct Painter<'a> {
    pub fonts: &'a mut FontCache,
    pub res: &'a mut Resources,
    /// Image part key -> resource id (None: could not be decoded).
    pub images: &'a mut HashMap<String, Option<u32>>,
    /// The slide's shapes as drawn (filled by `render_slide`).
    pub geo: Vec<ShapeGeom>,
    /// Draw the prompt text of empty placeholders.
    pub prompts: bool,
    cur: Matrix,
    top: Option<u32>,
    group: Option<u32>,
}

impl<'a> Painter<'a> {
    pub fn new(fonts: &'a mut FontCache, res: &'a mut Resources, images: &'a mut HashMap<String, Option<u32>>) -> Painter<'a> {
        Painter { fonts, res, images, geo: Vec::new(), prompts: true, cur: Matrix::IDENTITY, top: None, group: None }
    }

    #[allow(clippy::too_many_arguments)]
    fn record(&mut self, el: &Element, kind: &'static str, m: Matrix, w: f64, h: f64, text: Option<TextGeom>, cells: Vec<(usize, usize, Rect, TextGeom)>) {
        let Some(id) = shape_id(el) else { return };
        let text_box = super::model::shape_kind(el) == "text box";
        self.geo.push(ShapeGeom { id, top: self.top.unwrap_or(id), group: self.group, kind, m, w, h, text, cells, placeholder: Ph::of(el).is_some(), text_box });
    }

    pub fn render_slide(&mut self, deck: &Deck, index: usize) -> DisplayList {
        self.geo.clear();
        self.cur = Matrix::IDENTITY;
        self.top = None;
        self.group = None;
        let (w, h) = (deck.size.0 as f64 / EMU, deck.size.1 as f64 / EMU);
        let mut dl = DisplayList::new(w, h);
        let Some(scene) = Scene::new(deck, index) else { return dl };
        let mut out = Vec::new();
        self.background(&scene, Rect::new(0.0, 0.0, w, h), &mut out);
        let shows = |e: Option<&Element>| e.and_then(|e| e.attr("showMasterSp")).map(on).unwrap_or(true);
        let slide_shows = shows(Some(scene.slide));
        if slide_shows && shows(scene.layout) {
            if let Some(t) = scene.master.and_then(sp_tree) {
                self.tree(&scene, t, Owner::Master, &mut out, None, true);
            }
        }
        if slide_shows {
            if let Some(t) = scene.layout.and_then(sp_tree) {
                self.tree(&scene, t, Owner::Layout, &mut out, None, true);
            }
        }
        if let Some(t) = sp_tree(scene.slide) {
            self.tree(&scene, t, Owner::Slide, &mut out, None, false);
        }
        self.fonts.fill_resources(self.res, &out);
        dl.items = out;
        dl
    }

    fn background(&mut self, scene: &Scene, rect: Rect, out: &mut Vec<Item>) {
        let sources = [(Some(scene.slide), Owner::Slide), (scene.layout, Owner::Layout), (scene.master, Owner::Master)];
        for (el, owner) in sources {
            let Some(bg) = el.and_then(|e| e.child("cSld")).and_then(|c| c.child("bg")) else { continue };
            let fill = if let Some(pr) = bg.child("bgPr") {
                find_fill(pr).map(|f| f.resolve(scene.theme, &scene.cmap, None))
            } else if let Some(r) = bg.child("bgRef") {
                let idx: u32 = r.attr("idx").and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                let ph = ColorSpec::parse(r).map(|c| scene.color(&c));
                scene.theme.fill_ref(idx).map(|f| f.resolve(scene.theme, &scene.cmap, ph))
            } else {
                None
            };
            if let Some(f) = fill {
                self.fill_rect(scene.deck, scene.part_of(owner), &f, rect, out);
            }
            return;
        }
        let white = scene.theme.scheme_color("bg1", &scene.cmap);
        out.push(Item::Path { ops: rect_ops(rect), fill: Some(Paint::Solid(white)), stroke: None });
    }

    fn fill_rect(&mut self, deck: &Deck, part: &str, fill: &Fill, rect: Rect, out: &mut Vec<Item>) {
        match fill {
            Fill::Picture(pf) => self.picture(deck, part, pf, rect, None, out),
            f => {
                if let Some(p) = f.to_paint(rect) {
                    out.push(Item::Path { ops: rect_ops(rect), fill: Some(p), stroke: None });
                }
            }
        }
    }

    fn tree(&mut self, scene: &Scene, tree: &Element, owner: Owner, out: &mut Vec<Item>, group_fill: Option<&Fill>, skip_ph: bool) {
        for el in shape_children(tree) {
            if super::model::is_hidden(el) {
                continue;
            }
            if skip_ph && Ph::of(el).is_some() {
                continue;
            }
            match el.local() {
                "sp" | "cxnSp" => self.sp(scene, el, owner, out, group_fill),
                "pic" => self.pic(scene, el, owner, out),
                "grpSp" => self.group(scene, el, owner, out, group_fill),
                "graphicFrame" => self.frame(scene, el, owner, out),
                _ => {}
            }
        }
    }

    fn sp(&mut self, scene: &Scene, el: &Element, owner: Owner, out: &mut Vec<Item>, group_fill: Option<&Fill>) {
        let r = resolve_shape(scene, el, owner);
        let Some(xfrm) = r.xfrm() else { return };
        let geom = r.geometry();
        let sg = shape_geometry(&geom, xfrm.ext.0 as f64, xfrm.ext.1 as f64, &[]);
        let fill = match r.fill(scene) {
            Some(Fill::Group) => group_fill.cloned(),
            f => f,
        };
        let line = r.line(scene);
        if let Some(Fill::Picture(pf)) = &fill {
            out.push(Item::PushTransform(xfrm.matrix()));
            let clip = sg.paths.first().map(|p| p.ops.clone());
            let bx = Rect::new(0.0, 0.0, xfrm.ext.0 as f64 / EMU, xfrm.ext.1 as f64 / EMU);
            self.picture(scene.deck, scene.part_of(owner), pf, bx, clip, out);
            out.push(Item::PopTransform);
            out.extend(shape_items(&sg.paths, None, line.as_ref(), Some(&xfrm)));
        } else {
            out.extend(shape_items(&sg.paths, fill.as_ref(), line.as_ref(), Some(&xfrm)));
        }
        let slide = owner == Owner::Slide;
        let upright = Xfrm { flip_h: false, flip_v: false, ..xfrm };
        let tx = el.child("txBody");
        let mut tgeom = None;
        if el.local() == "sp" && (tx.is_some() || slide) {
            let (items, g) = self.text_items(scene, &r, tx, sg.text_rect, slide);
            if !items.is_empty() {
                out.push(Item::PushTransform(upright.matrix()));
                out.extend(items);
                out.push(Item::PopTransform);
            }
            tgeom = g.map(|mut g| {
                g.m = self.cur.multiply(&upright.matrix());
                g
            });
        }
        if slide {
            let kind = if el.local() == "cxnSp" { "connector" } else { "shape" };
            let m = self.cur.multiply(&xfrm.matrix());
            self.record(el, kind, m, xfrm.ext.0 as f64 / EMU, xfrm.ext.1 as f64 / EMU, tgeom, Vec::new());
        }
    }

    fn text_items(&mut self, scene: &Scene, r: &ShapeRes, tx: Option<&Element>, rect: Rect, want_geo: bool) -> (Vec<Item>, Option<TextGeom>) {
        let body = r.body();
        let chain = r.text_chain(scene);
        let (fs, lr) = body.shrink();
        let empty;
        let tx = match tx {
            Some(t) => t,
            None => {
                empty = empty_tx();
                &empty
            }
        };
        let shows = has_text(tx);
        if !shows && !want_geo {
            return (Vec::new(), None);
        }
        let paras = resolve_paras(scene, tx, &chain, fs, lr);
        let (l, t, rr, b) = body.insets();
        let inner = Rect::new(rect.x + l, rect.y + t, (rect.w - l - rr).max(0.0), (rect.h - t - b).max(0.0));
        let wrap = body.wrap.unwrap_or(true);
        let anchor = body.anchor.as_deref().unwrap_or("t");
        let block = layout_block(self.fonts, &paras, inner.w, wrap);
        let mut items = if shows { place_block(&block, inner, anchor) } else { Vec::new() };
        if !shows && self.prompts && r.owner == Owner::Slide {
            if let Some(prompt) = r.ph.as_ref().and_then(|p| prompt_text(p, r.layout_el)) {
                let mut pp: Vec<RPara> = paras.iter().take(1).cloned().collect();
                if let Some(first) = pp.first_mut() {
                    let mut st = first.end_style.clone();
                    let c = st.color;
                    let mix = |v: u8| ((v as f64) * 0.55 + 128.0 * 0.45).round() as u8;
                    st.color = Color::rgba(mix(c.r), mix(c.g), mix(c.b), PROMPT_ALPHA);
                    first.runs = vec![StyledRun { text: prompt, style: st }];
                    let pblock = layout_block(self.fonts, &pp, inner.w, wrap);
                    items = place_block(&pblock, inner, anchor);
                }
            }
        }
        let geo = want_geo.then(|| TextGeom { m: Matrix::IDENTITY, inner, paras: place_geom(&block, &paras, inner, anchor) });
        (items, geo)
    }

    fn pic(&mut self, scene: &Scene, el: &Element, owner: Owner, out: &mut Vec<Item>) {
        let r = resolve_shape(scene, el, owner);
        let Some(xfrm) = r.xfrm() else { return };
        let Some(Fill::Picture(pf)) = el.child("blipFill").and_then(parse_fill) else { return };
        let geom = r.geometry();
        let sg = shape_geometry(&geom, xfrm.ext.0 as f64, xfrm.ext.1 as f64, &[]);
        out.push(Item::PushTransform(xfrm.matrix()));
        let bx = Rect::new(0.0, 0.0, xfrm.ext.0 as f64 / EMU, xfrm.ext.1 as f64 / EMU);
        let clip = sg.paths.first().map(|p| p.ops.clone());
        self.picture(scene.deck, scene.part_of(owner), &pf, bx, clip, out);
        if let Some(line) = r.line(scene) {
            out.extend(shape_items(&sg.paths, None, Some(&line), None));
        }
        out.push(Item::PopTransform);
        if owner == Owner::Slide {
            let m = self.cur.multiply(&xfrm.matrix());
            self.record(el, "picture", m, xfrm.ext.0 as f64 / EMU, xfrm.ext.1 as f64 / EMU, None, Vec::new());
        }
    }

    /// A picture fill over `bx` (local points), cropped by its source rectangle, clipped to `clip`.
    fn picture(&mut self, deck: &Deck, part: &str, pf: &PictureFill, bx: Rect, clip: Option<Vec<PathOp>>, out: &mut Vec<Item>) {
        let dest = match &pf.mode {
            PictureMode::Stretch(i) => Rect::new(bx.x + i.l * bx.w, bx.y + i.t * bx.h, bx.w * (1.0 - i.l - i.r), bx.h * (1.0 - i.t - i.b)),
            PictureMode::Tile { .. } => bx,
        };
        let s = pf.src_rect;
        let (fw, fh) = (1.0 - s.l - s.r, 1.0 - s.t - s.b);
        if fw <= 1e-9 || fh <= 1e-9 || dest.w <= 0.0 || dest.h <= 0.0 {
            return;
        }
        let full = Rect::new(dest.x - s.l / fw * dest.w, dest.y - s.t / fh * dest.h, dest.w / fw, dest.h / fh);
        let id = pf.embed.as_deref().and_then(|rid| self.image_id(deck, part, rid));
        out.push(Item::PushClip(clip.unwrap_or_else(|| rect_ops(bx))));
        match id {
            Some(id) => {
                let fade = pf.alpha < 1.0;
                if fade {
                    out.push(Item::PushOpacity(pf.alpha.max(0.0)));
                }
                out.push(Item::Image { image: id, rect: full });
                if fade {
                    out.push(Item::PopOpacity);
                }
            }
            None => {
                // A picture we cannot decode yet (EMF, WMF, SVG, GIF, linked): a quiet grey frame.
                let mut st = Stroke::new(0.75, Color::rgb(0xA6, 0xA6, 0xA6));
                st.dash = vec![3.0, 2.0];
                out.push(Item::Path { ops: rect_ops(dest), fill: Some(Paint::Solid(Color::rgb(0xF2, 0xF2, 0xF2))), stroke: Some(st) });
            }
        }
        out.push(Item::PopClip);
    }

    fn image_id(&mut self, deck: &Deck, part: &str, rid: &str) -> Option<u32> {
        let rel = deck.pkg.rel_by_id(part, rid)?;
        if rel.target_mode_external {
            return None;
        }
        let name = deck.pkg.resolve(part, rel);
        let key = name_key(&name);
        if let Some(v) = self.images.get(&key) {
            return *v;
        }
        let img = deck.pkg.part_bytes(&name).ok().and_then(|b| decode_image(&b));
        let id = img.map(|img| {
            let id = self.res.images.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            self.res.add_image(ImageRes { id, width: img.width, height: img.height, rgba: img.pixels });
            id
        });
        self.images.insert(key, id);
        id
    }

    fn group(&mut self, scene: &Scene, el: &Element, owner: Owner, out: &mut Vec<Item>, group_fill: Option<&Fill>) {
        let gp = el.child("grpSpPr");
        let xfrm = gp.and_then(|g| g.child("xfrm")).map(Xfrm::parse);
        let gfill = match gp.and_then(find_fill).map(|f| f.resolve(scene.theme, &scene.cmap, None)) {
            Some(Fill::Group) | None => group_fill.cloned(),
            f => f,
        };
        if let Some(x) = &xfrm {
            out.push(Item::PushTransform(x.child_matrix()));
        }
        let saved = (self.cur, self.top, self.group);
        if owner == Owner::Slide {
            if let Some(x) = &xfrm {
                let m = self.cur.multiply(&x.matrix());
                self.record(el, "group", m, x.ext.0 as f64 / EMU, x.ext.1 as f64 / EMU, None, Vec::new());
                self.cur = self.cur.multiply(&x.child_matrix());
            }
            self.top = self.top.or(shape_id(el));
            self.group = shape_id(el);
        }
        self.tree(scene, el, owner, out, gfill.as_ref(), false);
        (self.cur, self.top, self.group) = saved;
        if xfrm.is_some() {
            out.push(Item::PopTransform);
        }
    }

    fn frame(&mut self, scene: &Scene, el: &Element, owner: Owner, out: &mut Vec<Item>) {
        let r = resolve_shape(scene, el, owner);
        let Some(x) = r.xfrm() else { return };
        let mut cells = Vec::new();
        let mut h = x.ext.1 as f64 / EMU;
        let mut w = x.ext.0 as f64 / EMU;
        let kind = if let Some(tbl) = table_of(el) {
            let (tw, th) = self.table(scene, tbl, &x, out, &mut cells);
            if tw > 0.0 {
                w = tw;
            }
            h = h.max(th);
            "table"
        } else {
            super::model::shape_kind(el)
        };
        if owner == Owner::Slide {
            let m = self.cur.multiply(&x.matrix());
            for c in &mut cells {
                c.3.m = m;
            }
            self.record(el, kind, m, w, h, None, cells);
        }
    }

    /// Draws a table; returns its drawn width and height, and fills `cells` with each cell's text.
    fn table(&mut self, scene: &Scene, tbl: &Element, xfrm: &Xfrm, out: &mut Vec<Item>, cells: &mut Vec<(usize, usize, Rect, TextGeom)>) -> (f64, f64) {
        let cols: Vec<f64> = tbl
            .child("tblGrid")
            .map(|g| g.children_named("gridCol").map(|c| emu_attr_pt(c, "w").unwrap_or(0.0)).collect())
            .unwrap_or_default();
        if cols.is_empty() {
            return (0.0, 0.0);
        }
        let rows: Vec<&Element> = tbl.children_named("tr").collect();
        let pr = tbl.child("tblPr");
        let flag = |n: &str| pr.and_then(|p| p.attr(n)).map(on).unwrap_or(false);
        let (first_row, band_row, last_row) = (flag("firstRow"), flag("bandRow"), flag("lastRow"));
        let style_id = pr.and_then(|p| p.child("tableStyleId")).map(|s| s.text().trim().to_string()).unwrap_or_default();
        let styled = style_id.eq_ignore_ascii_case(MEDIUM_STYLE_2_ACCENT_1);
        let mut heights: Vec<f64> = rows.iter().map(|r| emu_attr_pt(r, "h").unwrap_or(0.0)).collect();
        let header = |ri: usize| styled && ((first_row && ri == 0) || (last_row && ri + 1 == rows.len()));
        let mut base = Vec::new();
        if let Some(st) = scene.styles {
            base.push(st.other.clone());
        }
        base.push(scene.deck.default_style.clone());
        let chain_for = |tc: &Element, ri: usize| {
            let mut mid = RunProps::default();
            if header(ri) {
                mid.bold = Some(true);
                mid.color = Some(ColorSpec::scheme("lt1"));
            } else if styled {
                mid.color = Some(ColorSpec::scheme("dk1"));
            }
            TextChain {
                lists: tc.child("txBody").and_then(|t| t.child("lstStyle")).map(ListStyle::parse).into_iter().collect(),
                mid: Some(mid),
                base: base.clone(),
            }
        };
        let margins = |tc: &Element| {
            let p = tc.child("tcPr");
            let m = |n: &str, d: f64| p.and_then(|p| emu_attr_pt(p, n)).unwrap_or(d);
            (m("marL", 7.2), m("marT", 3.6), m("marR", 7.2), m("marB", 3.6))
        };
        let span = |tc: &Element, n: &str| tc.attr(n).and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1).max(1);
        let merged = |tc: &Element| tc.attr("hMerge").map(on).unwrap_or(false) || tc.attr("vMerge").map(on).unwrap_or(false);
        let width_of = |c: usize, n: usize| cols.iter().skip(c).take(n).sum::<f64>();

        // Rows grow to fit their text.
        let mut blocks: Vec<Vec<Option<(TextBlock, Vec<RPara>)>>> = Vec::new();
        let empty = empty_tx();
        for (ri, tr) in rows.iter().enumerate() {
            let mut row_blocks = Vec::new();
            for (ci, tc) in tr.children_named("tc").enumerate() {
                if merged(tc) || ci >= cols.len() {
                    row_blocks.push(None);
                    continue;
                }
                let (ml, mt, mr, mb) = margins(tc);
                let w = (width_of(ci, span(tc, "gridSpan")) - ml - mr).max(1.0);
                let tx = tc.child("txBody").unwrap_or(&empty);
                let paras = resolve_paras(scene, tx, &chain_for(tc, ri), 1.0, 0.0);
                let b = layout_block(self.fonts, &paras, w, true);
                if span(tc, "rowSpan") == 1 {
                    heights[ri] = heights[ri].max(b.height + mt + mb);
                }
                row_blocks.push(Some((b, paras)));
            }
            blocks.push(row_blocks);
        }

        let accent = |tint: Option<f64>| {
            let mut c = ColorSpec::scheme("accent1");
            if let Some(t) = tint {
                c = c.with(ColorMod::Tint(t));
            }
            Fill::Solid(scene.color(&c))
        };
        let border = Stroke::new(1.0, scene.color(&ColorSpec::scheme("lt1")));
        out.push(Item::PushTransform(xfrm.matrix()));
        let mut y = 0.0;
        for (ri, tr) in rows.iter().enumerate() {
            let mut x = 0.0;
            for (ci, tc) in tr.children_named("tc").enumerate() {
                if ci >= cols.len() {
                    break;
                }
                let cw = cols[ci];
                if merged(tc) {
                    x += cw;
                    continue;
                }
                let w = width_of(ci, span(tc, "gridSpan"));
                let h: f64 = heights.iter().skip(ri).take(span(tc, "rowSpan")).sum();
                let cell = Rect::new(x, y, w, h);
                let tcpr = tc.child("tcPr");
                let fill = tcpr.and_then(find_fill).map(|f| f.resolve(scene.theme, &scene.cmap, None)).or_else(|| {
                    if !styled {
                        None
                    } else if header(ri) {
                        Some(accent(None))
                    } else if band_row && (ri - usize::from(first_row)) % 2 == 0 {
                        Some(accent(Some(0.4)))
                    } else {
                        Some(accent(Some(0.2)))
                    }
                });
                if let Some(f) = &fill {
                    self.fill_rect(scene.deck, scene.slide_part, f, cell, out);
                }
                if let Some(Some((block, paras))) = blocks.get(ri).and_then(|b| b.get(ci)) {
                    let (ml, mt, mr, mb) = margins(tc);
                    let inner = Rect::new(cell.x + ml, cell.y + mt, (cell.w - ml - mr).max(0.0), (cell.h - mt - mb).max(0.0));
                    let anchor = tcpr.and_then(|p| p.attr("anchor")).unwrap_or("t");
                    if tc.child("txBody").is_some_and(has_text) {
                        out.extend(place_block(block, inner, anchor));
                    }
                    cells.push((ri, ci, cell, TextGeom { m: Matrix::IDENTITY, inner, paras: place_geom(block, paras, inner, anchor) }));
                }
                let edges = [
                    ("lnL", (cell.x, cell.y), (cell.x, cell.bottom())),
                    ("lnR", (cell.right(), cell.y), (cell.right(), cell.bottom())),
                    ("lnT", (cell.x, cell.y), (cell.right(), cell.y)),
                    ("lnB", (cell.x, cell.bottom()), (cell.right(), cell.bottom())),
                ];
                for (name, a, b) in edges {
                    let stroke = match tcpr.and_then(|p| p.child(name)) {
                        Some(ln) => parse_line(ln).resolve(scene.theme, &scene.cmap, None).to_stroke(),
                        None if styled => Some(border.clone()),
                        None => None,
                    };
                    if let Some(s) = stroke {
                        out.push(Item::Path { ops: vec![PathOp::MoveTo(a.0, a.1), PathOp::LineTo(b.0, b.1)], fill: None, stroke: Some(s) });
                    }
                }
                x += cw;
            }
            y += heights[ri];
        }
        out.push(Item::PopTransform);
        (cols.iter().sum(), y)
    }
}

/// An empty text body (one empty paragraph), for shapes that have none yet.
pub fn empty_tx() -> Element {
    let mut tx = Element::new("p:txBody");
    tx.children.push(crate::ground::xml::Node::Element(Element::new("a:bodyPr")));
    tx.children.push(crate::ground::xml::Node::Element(Element::new("a:p")));
    tx
}
