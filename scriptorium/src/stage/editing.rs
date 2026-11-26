//! Stage by hand: the caret in a shape's text, a shape (or several) selected as a whole, a slide
//! selected for a copy. Places are JSON the room never reads:
//!
//! - text: `{"slide","shape","para","char"}` (+ `"cell":[row,col]` in a table)
//! - shapes: `{"slide","shape"}` (+ `"shapes":[ids]` for more than one)
//! - a whole slide: `{"slide","whole":true}`
//!
//! The geometry comes from the last drawing of each slide (`render::ShapeGeom`), so the caret sits
//! exactly on the glyphs the page shows.

use crate::ground::bus::json::Json;
use crate::ground::bus::Command;
use crate::ground::display::{Matrix, Rect};
use crate::ground::edit::{CaretRect, Clip, Editing, Pos, Selection};
use crate::ground::xml::write_dom;

use super::edit::{el_at, find_shape, raw_element, ShapeKey};
use super::model::shape_text;
use super::render::{resolve_paras, resolve_shape, Owner, ParaGeom, Scene, ShapeGeom, TextGeom};
use super::style::{Autofit, BulletKind};
use super::text::{word_left, word_right, At, Body};
use super::StageDoc;

/// A caret (or one end of a text selection) in a shape's text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextPos {
    pub slide: usize,
    pub shape: u32,
    pub cell: Option<(usize, usize)>,
    pub at: At,
}

impl TextPos {
    pub fn pos(&self) -> Pos {
        let mut j = Json::obj().with("slide", self.slide).with("shape", self.shape).with("para", self.at.para).with("char", self.at.ch);
        if let Some((r, c)) = self.cell {
            j.set("cell", Json::Arr(vec![Json::from(r), Json::from(c)]));
        }
        Pos(j)
    }

    pub fn same_text(&self, o: &TextPos) -> bool {
        self.slide == o.slide && self.shape == o.shape && self.cell == o.cell
    }
}

/// What a selection means.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    /// A text range (anchor, focus) in one shape; equal ends are a caret.
    Text(TextPos, TextPos),
    /// Whole shapes on a slide.
    Shapes(usize, Vec<u32>),
    /// A slide as a whole.
    Slide(usize),
    /// Nothing on a slide (a click beside the shapes): the slide is known, nothing is selected.
    Blank(usize),
}

pub fn text_pos(p: &Json) -> Option<TextPos> {
    let slide = p.get("slide")?.as_usize()?;
    let shape = u32::try_from(p.get("shape")?.as_i64()?).ok()?;
    let para = p.get("para")?.as_usize()?;
    let ch = p.get("char")?.as_usize()?;
    let cell = p.get("cell").and_then(Json::as_arr).and_then(|a| Some((a.first()?.as_usize()?, a.get(1)?.as_usize()?)));
    Some(TextPos { slide, shape, cell, at: At::new(para, ch) })
}

fn ids_of(p: &Json, out: &mut Vec<u32>) {
    if let Some(i) = p.get("shape").and_then(Json::as_i64).and_then(|v| u32::try_from(v).ok()) {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    if let Some(list) = p.get("shapes").and_then(Json::as_arr) {
        for v in list {
            if let Some(i) = v.as_i64().and_then(|v| u32::try_from(v).ok()) {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
    }
}

/// A selection read as a [`Place`].
pub fn place_of(sel: &Json) -> Option<Place> {
    let s = Selection::from_json(sel)?;
    let (a, f) = (s.anchor.json(), s.focus.json());
    let slide = f.get("slide").and_then(Json::as_usize).or_else(|| a.get("slide").and_then(Json::as_usize))?;
    if f.get("whole").and_then(Json::as_bool) == Some(true) || a.get("whole").and_then(Json::as_bool) == Some(true) {
        return Some(Place::Slide(slide));
    }
    match (text_pos(a), text_pos(f)) {
        (Some(ta), Some(tf)) if ta.same_text(&tf) => return Some(Place::Text(ta, tf)),
        (_, Some(tf)) if a.get("para").is_some() => return Some(Place::Text(tf, tf)),
        (Some(ta), None) if f.get("shape").is_none() => return Some(Place::Text(ta, ta)),
        _ => {}
    }
    let mut ids = Vec::new();
    if a.get("slide").and_then(Json::as_usize) == Some(slide) {
        ids_of(a, &mut ids);
    }
    ids_of(f, &mut ids);
    if ids.is_empty() {
        return Some(Place::Blank(slide));
    }
    Some(Place::Shapes(slide, ids))
}

/// A shapes selection as JSON (non-collapsed, so the page paints its box).
pub fn shapes_sel(slide: usize, ids: &[u32]) -> Selection {
    let first = ids.first().copied().unwrap_or(0);
    let last = ids.last().copied().unwrap_or(first);
    let all = Json::Arr(ids.iter().map(|i| Json::from(*i)).collect());
    Selection::new(Pos(Json::obj().with("slide", slide).with("shape", first).with("shapes", all)), Pos(Json::obj().with("slide", slide).with("shape", last)))
}

pub fn slide_sel(slide: usize) -> Selection {
    let p = Pos(Json::obj().with("slide", slide).with("whole", true));
    Selection::new(p.clone(), Pos(Json::obj().with("slide", slide).with("whole", true).with("end", true)))
}

const BORDER: f64 = 4.0;
const HANDLE: f64 = 6.0;

fn local(m: &Matrix, x: f64, y: f64) -> Option<(f64, f64)> {
    m.invert().map(|i| i.apply(x, y))
}

fn inside(g: &ShapeGeom, x: f64, y: f64) -> bool {
    let Some((lx, ly)) = local(&g.m, x, y) else { return false };
    // Thin shapes (lines) get a few points of slack.
    let sx = if g.w < BORDER * 2.0 { BORDER } else { 0.5 };
    let sy = if g.h < BORDER * 2.0 { BORDER } else { 0.5 };
    lx >= -sx && ly >= -sy && lx <= g.w + sx && ly <= g.h + sy
}

fn on_border(g: &ShapeGeom, x: f64, y: f64) -> bool {
    let Some((lx, ly)) = local(&g.m, x, y) else { return false };
    lx < BORDER || ly < BORDER || lx > g.w - BORDER || ly > g.h - BORDER
}

/// The paragraph and character nearest a point in a text area.
fn hit_text(t: &TextGeom, x: f64, y: f64) -> Option<At> {
    let (lx, ly) = local(&t.m, x, y)?;
    if t.paras.is_empty() {
        return Some(At::new(0, 0));
    }
    let mut pi = t.paras.len() - 1;
    for (i, p) in t.paras.iter().enumerate() {
        if ly < p.y + p.lay.height {
            pi = i;
            break;
        }
    }
    let p = &t.paras[pi];
    let off = p.lay.hit_test(lx - p.x, ly - p.y);
    Some(At::new(pi, p.to_model(off).min(p.model_len)))
}

/// The caret's box in a paragraph, in the text's own space: (x, top, height).
fn caret_in(p: &ParaGeom, ch: usize) -> (f64, f64, f64) {
    let r = p.lay.caret_rect(p.to_layout(ch));
    let h = if r.h > 0.0 { r.h } else { p.lay.lines.first().map(|l| l.height).unwrap_or(12.0) };
    (p.x + r.x, p.y + r.y, h)
}

/// The line index of a character in a paragraph.
fn line_of(p: &ParaGeom, ch: usize) -> usize {
    let off = p.to_layout(ch);
    let n = p.lay.lines.len();
    p.lay.lines.iter().position(|l| off < l.range.end || (off == l.range.end && l.ends_with_break)).unwrap_or(n.saturating_sub(1)).min(n.saturating_sub(1))
}

impl StageDoc {
    pub(crate) fn geom(&self, slide: usize, id: u32) -> Option<&ShapeGeom> {
        self.geo.get(slide)?.iter().find(|g| g.id == id)
    }

    pub(crate) fn text_geom(&self, tp: &TextPos) -> Option<&TextGeom> {
        let g = self.geom(tp.slide, tp.shape)?;
        match tp.cell {
            Some((r, c)) => g.cells.iter().find(|x| x.0 == r && x.1 == c).map(|x| &x.3),
            None => g.text.as_ref(),
        }
    }

    pub(crate) fn body_of(&self, tp: &TextPos) -> Option<Body> {
        super::ops::read_body(&self.deck, tp.slide, tp.shape, tp.cell)
    }

    /// The slide-space box of a shape.
    pub(crate) fn shape_box(&self, slide: usize, id: u32) -> Option<Rect> {
        let g = self.geom(slide, id)?;
        Some(g.m.apply_rect(&Rect::new(0.0, 0.0, g.w, g.h)))
    }

    fn text_step(&self, tp: &TextPos, motion: &str) -> Option<TextPos> {
        let body = self.body_of(tp)?;
        let at = body.clamp(tp.at);
        let plen = |p: usize| body.paras.get(p).map(|x| x.len()).unwrap_or(0);
        let mv = |at: At| Some(TextPos { at, ..*tp });
        match motion {
            "left" => {
                if at.ch > 0 {
                    mv(At::new(at.para, at.ch - 1))
                } else if at.para > 0 {
                    mv(At::new(at.para - 1, plen(at.para - 1)))
                } else {
                    mv(at)
                }
            }
            "right" => {
                if at.ch < plen(at.para) {
                    mv(At::new(at.para, at.ch + 1))
                } else if at.para + 1 < body.paras.len() {
                    mv(At::new(at.para + 1, 0))
                } else {
                    mv(at)
                }
            }
            "word_left" => {
                if at.ch == 0 && at.para > 0 {
                    mv(At::new(at.para - 1, plen(at.para - 1)))
                } else {
                    mv(At::new(at.para, word_left(&body.chars(at.para), at.ch)))
                }
            }
            "word_right" => {
                if at.ch >= plen(at.para) && at.para + 1 < body.paras.len() {
                    mv(At::new(at.para + 1, 0))
                } else {
                    mv(At::new(at.para, word_right(&body.chars(at.para), at.ch)))
                }
            }
            "doc_start" => mv(At::new(0, 0)),
            "doc_end" => mv(body.end()),
            "line_start" | "line_end" | "up" | "down" => {
                let t = self.text_geom(tp)?;
                let p = t.paras.get(at.para)?;
                let li = line_of(p, at.ch);
                let line = p.lay.lines.get(li)?;
                match motion {
                    "line_start" => mv(At::new(at.para, p.to_model(line.range.start))),
                    "line_end" => {
                        let mut end = line.range.end;
                        if line.ends_with_break && end > line.range.start {
                            end -= p.lay.text[..end].chars().next_back().map(|c| c.len_utf8()).unwrap_or(1);
                        }
                        mv(At::new(at.para, p.to_model(end)))
                    }
                    _ => {
                        let (cx, _, _) = caret_in(p, at.ch);
                        let target = if motion == "up" {
                            if li > 0 {
                                Some((at.para, li - 1))
                            } else if at.para > 0 {
                                Some((at.para - 1, t.paras[at.para - 1].lay.lines.len().saturating_sub(1)))
                            } else {
                                None
                            }
                        } else if li + 1 < p.lay.lines.len() {
                            Some((at.para, li + 1))
                        } else if at.para + 1 < t.paras.len() {
                            Some((at.para + 1, 0))
                        } else {
                            None
                        };
                        match target {
                            None => mv(if motion == "up" { At::new(at.para, 0) } else { At::new(at.para, plen(at.para)) }),
                            Some((pi, l)) => {
                                let q = &t.paras[pi];
                                let ly = q.lay.lines.get(l).map(|ln| ln.top + ln.height / 2.0).unwrap_or(0.0);
                                let off = q.lay.hit_test(cx - q.x, ly);
                                mv(At::new(pi, q.to_model(off).min(plen(pi))))
                            }
                        }
                    }
                }
            }
            _ => None,
        }
    }

    /// The resolved run style and paragraph under a text place (for the ribbon's state).
    fn text_state(&self, tp: &TextPos) -> Json {
        let mut j = Json::obj().with("kind", "text");
        let body = self.body_of(tp);
        if let (Some(scene), None) = (Scene::new(&self.deck, tp.slide), tp.cell) {
            if let Some(path) = find_shape(scene.slide, &ShapeKey::Id(tp.shape)) {
                if let Some(el) = el_at(scene.slide, &path) {
                    let r = resolve_shape(&scene, el, Owner::Slide);
                    let b = r.body();
                    let (fs, lr) = b.shrink();
                    let empty = super::render::empty_tx();
                    let tx = el.child("txBody").unwrap_or(&empty);
                    let paras = resolve_paras(&scene, tx, &r.text_chain(&scene), fs, lr);
                    if let Some(p) = paras.get(tp.at.para.min(paras.len().saturating_sub(1))) {
                        let ch = tp.at.ch;
                        let k = p.segs.iter().position(|s| ch > s.model && ch <= s.model + s.mlen).or_else(|| p.segs.iter().position(|s| ch <= s.model));
                        let st = k.and_then(|k| p.runs.get(k)).map(|r| r.style.clone()).unwrap_or_else(|| p.end_style.clone());
                        j.set("bold", st.weight >= 600);
                        j.set("italic", st.italic);
                        j.set("underline", st.underline);
                        j.set("strike", st.strike);
                        j.set("font", st.family.as_str());
                        j.set("size", ((st.size_pt / if fs > 0.0 { fs } else { 1.0 }) * 10.0).round() / 10.0);
                        j.set("color", format!("{:02X}{:02X}{:02X}", st.color.r, st.color.g, st.color.b));
                        let al = match p.align {
                            crate::ground::text::Align::Center => "center",
                            crate::ground::text::Align::Right => "right",
                            crate::ground::text::Align::Justify | crate::ground::text::Align::Distribute => "justify",
                            _ => "left",
                        };
                        j.set("align", Json::obj().with("left", al == "left").with("center", al == "center").with("right", al == "right").with("justify", al == "justify"));
                        j.set("level", p.level);
                        let lst = r.text_chain(&scene).para(body.as_ref().and_then(|b| b.paras.get(tp.at.para)).and_then(|q| q.ppr.as_ref()), p.level);
                        j.set("bullets", matches!(lst.bullet, Some(BulletKind::Char(_))));
                        j.set("numbered", matches!(lst.bullet, Some(BulletKind::AutoNum { .. })));
                        j.set(
                            "line_spacing",
                            match p.line_spacing {
                                crate::ground::text::LineSpacing::Multiple(m) => Json::from((m * 100.0).round() / 100.0),
                                crate::ground::text::LineSpacing::Exactly(v) => Json::from(v),
                                _ => Json::from(1.0),
                            },
                        );
                    }
                    j.set(
                        "autofit",
                        match b.autofit {
                            Some(Autofit::Normal { .. }) => "shrink",
                            Some(Autofit::Shape) => "resize",
                            _ => "none",
                        },
                    );
                    j.set("autofit_shrink", matches!(b.autofit, Some(Autofit::Normal { .. })));
                    j.set("autofit_resize", matches!(b.autofit, Some(Autofit::Shape)));
                    j.set("wrap", b.wrap.unwrap_or(true));
                }
            }
        } else if let Some(b) = &body {
            let rpr = b.rpr_at(tp.at);
            let on = |k: &str| rpr.as_ref().and_then(|r| r.attr(k)).map(super::style::on).unwrap_or(false);
            j.set("bold", on("b"));
            j.set("italic", on("i"));
            j.set("underline", rpr.as_ref().and_then(|r| r.attr("u")).is_some_and(|u| u != "none"));
            j.set("table", true);
        }
        if tp.cell.is_some() {
            j.set("table", true);
            j.set("cell", Json::Arr(vec![Json::from(tp.cell.map(|c| c.0).unwrap_or(0)), Json::from(tp.cell.map(|c| c.1).unwrap_or(0))]));
        }
        j.set("shape", self.shape_state(tp.slide, tp.shape));
        j
    }

    fn shape_state(&self, slide: usize, id: u32) -> Json {
        let mut j = Json::obj().with("id", id);
        if let Some(g) = self.geom(slide, id) {
            j.set("kind", g.kind);
            if let Some(b) = self.shape_box(slide, id) {
                j.set("x", (b.x * 10.0).round() / 10.0);
                j.set("y", (b.y * 10.0).round() / 10.0);
            }
            j.set("w", (g.w * 10.0).round() / 10.0);
            j.set("h", (g.h * 10.0).round() / 10.0);
        }
        if let Some(scene) = Scene::new(&self.deck, slide) {
            if let Some(el) = find_shape(scene.slide, &ShapeKey::Id(id)).and_then(|p| el_at(scene.slide, &p)) {
                let r = resolve_shape(&scene, el, Owner::Slide);
                if let Some(x) = r.xfrm() {
                    j.set("rotation", x.rotation_deg());
                    j.set("flip_h", x.flip_h);
                    j.set("flip_v", x.flip_v);
                }
                match r.fill(&scene) {
                    Some(crate::ground::draw::Fill::Solid(c)) => j.set("fill", format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)),
                    Some(crate::ground::draw::Fill::None) | None => j.set("fill", Json::Null),
                    Some(_) => j.set("fill", "other"),
                }
                if let Some(l) = r.line(&scene) {
                    if let Some(s) = l.to_stroke() {
                        j.set("outline", format!("{:02X}{:02X}{:02X}", s.color.r, s.color.g, s.color.b));
                        j.set("outline_width", (s.width * 100.0).round() / 100.0);
                    }
                }
                j.set("name", super::model::shape_name(el));
                j.set("picture", el.local() == "pic");
                j.set("group", el.local() == "grpSp");
                j.set("table", super::model::table_of(el).is_some());
            }
        }
        j
    }

    /// The raw XML of a shape on a slide.
    pub(crate) fn shape_raw(&self, slide: usize, id: u32) -> Option<String> {
        let part = &self.deck.slides.get(slide)?.part;
        let src = self.deck.pkg.part_bytes(part).ok()?;
        let root = self.deck.sheet(part)?;
        let path = find_shape(root, &ShapeKey::Id(id))?;
        raw_element(&src, &path).and_then(|b| String::from_utf8(b).ok())
    }
}

impl Editing for StageDoc {
    fn hit(&self, page: usize, x: f64, y: f64) -> Option<Pos> {
        let geo = self.geo.get(page)?;
        let shape = |id: u32| Some(Pos(Json::obj().with("slide", page).with("shape", id)));
        for g in geo.iter().rev().filter(|g| g.group.is_none()) {
            if !inside(g, x, y) {
                continue;
            }
            if g.kind == "group" {
                for c in geo.iter().rev().filter(|c| c.top == g.id && c.id != g.id && c.kind != "group") {
                    if let Some(t) = &c.text {
                        let has = t.paras.iter().any(|p| p.model_len > 0);
                        if has && inside(c, x, y) && !on_border(c, x, y) {
                            let at = hit_text(t, x, y)?;
                            return Some(TextPos { slide: page, shape: c.id, cell: None, at }.pos());
                        }
                    }
                }
                return shape(g.id);
            }
            if !g.cells.is_empty() {
                if on_border(g, x, y) {
                    return shape(g.id);
                }
                let (lx, ly) = local(&g.m, x, y)?;
                let cell = g.cells.iter().find(|c| c.2.contains(lx, ly)).or_else(|| g.cells.last());
                if let Some((r, c, _, t)) = cell {
                    let at = hit_text(t, x, y)?;
                    return Some(TextPos { slide: page, shape: g.id, cell: Some((*r, *c)), at }.pos());
                }
                return shape(g.id);
            }
            if let Some(t) = &g.text {
                let has = t.paras.iter().any(|p| p.model_len > 0);
                if (has || g.placeholder || g.text_box) && !on_border(g, x, y) {
                    let at = hit_text(t, x, y)?;
                    return Some(TextPos { slide: page, shape: g.id, cell: None, at }.pos());
                }
            }
            return shape(g.id);
        }
        // Beside every shape: the slide itself, nothing selected.
        Some(Pos(Json::obj().with("slide", page)))
    }

    fn caret(&self, pos: &Pos) -> Option<CaretRect> {
        let p = pos.json();
        let slide = p.get("slide")?.as_usize()?;
        if slide >= self.deck.slides.len() {
            return None;
        }
        if p.get("whole").is_some() || p.get("shape").is_none() {
            return Some(CaretRect { page: slide, x: 0.0, y: 0.0, height: 0.0 });
        }
        if let Some(tp) = text_pos(p) {
            let t = self.text_geom(&tp)?;
            let para = t.paras.get(tp.at.para).or(t.paras.last())?;
            let ch = if tp.at.para < t.paras.len() { tp.at.ch.min(para.model_len) } else { para.model_len };
            let (cx, cy, h) = caret_in(para, ch);
            let top = t.m.apply(cx, cy);
            let bot = t.m.apply(cx, cy + h);
            let height = ((bot.0 - top.0).powi(2) + (bot.1 - top.1).powi(2)).sqrt();
            return Some(CaretRect { page: slide, x: top.0, y: top.1.min(bot.1), height });
        }
        // A shape: a bar along its left edge marks it (the selection box shows it when drawn).
        let id = u32::try_from(p.get("shape")?.as_i64()?).ok()?;
        let b = self.shape_box(slide, id)?;
        Some(CaretRect { page: slide, x: b.x, y: b.y, height: b.h.max(1.0) })
    }

    fn selection_rects(&self, sel: &Selection) -> Vec<(usize, Rect)> {
        let Some(place) = place_of(&sel.to_json()) else { return Vec::new() };
        match place {
            Place::Text(a, f) => {
                let Some(t) = self.text_geom(&f) else { return Vec::new() };
                let (s, e) = (a.at.min(f.at), a.at.max(f.at));
                if s == e {
                    return Vec::new();
                }
                let mut out = Vec::new();
                for pi in s.para..=e.para.min(t.paras.len().saturating_sub(1)) {
                    let p = &t.paras[pi];
                    let from = p.to_layout(if pi == s.para { s.ch } else { 0 });
                    let to = p.to_layout(if pi == e.para { e.ch } else { p.model_len });
                    for line in &p.lay.lines {
                        let (ls, le) = (line.range.start.max(from), line.range.end.min(to));
                        if ls > le || (ls == le && !(pi < e.para && le == line.range.end)) {
                            continue;
                        }
                        let x1 = p.lay.caret_rect(ls).x;
                        let mut x2 = p.lay.caret_rect(le).x;
                        if le == line.range.end && le <= to && (pi < e.para || to > line.range.end) {
                            x2 = x2.max(line.x + line.width) + 3.0;
                        }
                        if x2 <= x1 {
                            x2 = x1 + 3.0;
                        }
                        let r = Rect::new(p.x + x1, p.y + line.top, x2 - x1, line.height);
                        out.push((f.slide, t.m.apply_rect(&r)));
                    }
                }
                out
            }
            Place::Shapes(slide, ids) => {
                let mut out = Vec::new();
                for id in ids {
                    let Some(g) = self.geom(slide, id) else { continue };
                    let b = g.m.apply_rect(&Rect::new(0.0, 0.0, g.w, g.h));
                    // The box as four thin edges and eight handles.
                    let t = 1.0;
                    out.push((slide, Rect::new(b.x, b.y, b.w.max(t), t)));
                    out.push((slide, Rect::new(b.x, b.bottom() - t, b.w.max(t), t)));
                    out.push((slide, Rect::new(b.x, b.y, t, b.h.max(t))));
                    out.push((slide, Rect::new(b.right() - t, b.y, t, b.h.max(t))));
                    for (hx, hy) in [(b.x, b.y), (b.x + b.w / 2.0, b.y), (b.right(), b.y), (b.right(), b.y + b.h / 2.0), (b.right(), b.bottom()), (b.x + b.w / 2.0, b.bottom()), (b.x, b.bottom()), (b.x, b.y + b.h / 2.0)] {
                        out.push((slide, Rect::new(hx - HANDLE / 2.0, hy - HANDLE / 2.0, HANDLE, HANDLE)));
                    }
                }
                out
            }
            Place::Blank(_) => Vec::new(),
            Place::Slide(slide) => {
                let (w, h) = self.slide_size_pt();
                vec![(slide, Rect::new(0.0, 0.0, w, 3.0)), (slide, Rect::new(0.0, h - 3.0, w, 3.0)), (slide, Rect::new(0.0, 0.0, 3.0, h)), (slide, Rect::new(w - 3.0, 0.0, 3.0, h))]
            }
        }
    }

    fn state(&self, sel: &Selection) -> Json {
        match place_of(&sel.to_json()) {
            Some(Place::Text(a, f)) => {
                let s = if a.at <= f.at { a } else { f };
                let mut j = self.text_state(&TextPos { at: if a.at == f.at { f.at } else { At::new(s.at.para, s.at.ch + 1) }, ..s });
                j.set("collapsed", a.at == f.at);
                j.set("slide", f.slide);
                j
            }
            Some(Place::Shapes(slide, ids)) => {
                let mut j = self.shape_state(slide, ids[0]).with("kind", "shapes");
                j.set("count", ids.len());
                j.set("shapes", Json::Arr(ids.iter().map(|i| Json::from(*i)).collect()));
                j.set("slide", slide);
                j
            }
            Some(Place::Slide(slide)) => Json::obj().with("kind", "slide").with("slide", slide).with("whole", true),
            Some(Place::Blank(slide)) => Json::obj().with("kind", "blank").with("slide", slide),
            None => Json::obj(),
        }
    }

    fn copy(&self, sel: &Selection) -> Clip {
        match place_of(&sel.to_json()) {
            Some(Place::Text(a, f)) => {
                let Some(body) = self.body_of(&f) else { return Clip::default() };
                let part = body.slice(a.at, f.at);
                let text = part.text().replace('\u{b}', "\n");
                let mut tx = crate::ground::xml::Element::new("a:txBody");
                part.write_into(&mut tx);
                let xml = String::from_utf8_lossy(&write_dom(&tx)).into_owned();
                Clip { text, rich: Some(Json::obj().with("kind", "stage.text").with("xml", xml)) }
            }
            Some(Place::Shapes(slide, ids)) => {
                let part = self.deck.slides[slide].part.clone();
                let root = self.deck.sheet(&part);
                let mut texts = Vec::new();
                let mut shapes = Vec::new();
                for id in &ids {
                    let Some(raw) = self.shape_raw(slide, *id) else { continue };
                    let Some(el) = root.and_then(|r| find_shape(r, &ShapeKey::Id(*id)).and_then(|p| el_at(r, &p))) else { continue };
                    let t = shape_text(el, Some(self.deck.slide_number(slide)));
                    if !t.is_empty() {
                        texts.push(t);
                    }
                    shapes.push(super::ops::shape_package(&self.deck, &part, el, raw));
                }
                Clip { text: texts.join("\n"), rich: Some(Json::obj().with("kind", "stage.shapes").with("slide", slide).with("shapes", Json::Arr(shapes))) }
            }
            Some(Place::Blank(_)) => Clip::default(),
            Some(Place::Slide(slide)) => {
                let rich = self.slide_package(slide);
                let title = self.slide_json(slide, false).get("title").and_then(Json::as_str).unwrap_or("").to_string();
                Clip { text: title, rich: Some(Json::obj().with("kind", "stage.slides").with("slides", Json::Arr(rich.into_iter().collect()))) }
            }
            None => Clip::default(),
        }
    }

    fn step(&self, pos: &Pos, motion: &str) -> Option<Pos> {
        let tp = text_pos(pos.json())?;
        self.text_step(&tp, motion).map(|t| t.pos())
    }

    fn landed(&self, cmd: &Command) -> Option<Selection> {
        match &self.landed {
            Some((c, s)) if c == cmd => Some(s.clone()),
            _ => None,
        }
    }
}

impl StageDoc {
    /// Moves for the arrow commands (`stage.arrow`): the caret one step, or the selection grown.
    pub(crate) fn arrow_text(&self, a: TextPos, f: TextPos, dir: &str, extend: bool) -> Selection {
        if !extend && a.at != f.at && matches!(dir, "left" | "right") {
            let t = if (dir == "left") == (a.at < f.at) { a } else { f };
            return Selection::caret(t.pos());
        }
        let next = self.text_step(&f, dir).unwrap_or(f);
        if extend {
            Selection::new(a.pos(), next.pos())
        } else {
            Selection::caret(next.pos())
        }
    }
}
