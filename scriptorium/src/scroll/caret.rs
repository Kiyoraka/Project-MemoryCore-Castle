//! Scroll on the editing ground: the caret, the selection boxes, the ribbon's state and the copy,
//! all read from the layout's position map ([`super::layout::LineBox`]).

use super::dom::el_at;
use super::layout::{LineBox, LineKey};
use super::model;
use super::pos::{self, AKind, Flow, SPos, Span};
use super::props::VertAlign;
use super::ScrollDoc;
use crate::ground::bus::json::Json;
use crate::ground::bus::Command;
use crate::ground::display::{Color, Rect};
use crate::ground::edit::{CaretRect, Clip, Editing, Pos, Selection};
use crate::ground::xml::{self, Element};

/// Does a line show paragraph `p` of `flow`?
fn shows(l: &LineBox, flow: &Flow, p: usize) -> bool {
    match (&l.key, flow) {
        (LineKey::Body(f), Flow::Body) => *f == p,
        (LineKey::Part(name, i), Flow::Part(part)) => *i == p && &**name == part.as_str(),
        _ => false,
    }
}

fn in_flow(l: &LineBox, flow: &Flow) -> bool {
    match (&l.key, flow) {
        (LineKey::Body(_), Flow::Body) => true,
        (LineKey::Part(name, _), Flow::Part(part)) => &**name == part.as_str(),
        _ => false,
    }
}

fn key_pos(l: &LineBox, o: usize) -> SPos {
    match &l.key {
        LineKey::Body(f) => SPos::body(*f, o),
        LineKey::Part(name, i) => SPos { flow: Flow::Part(name.to_string()), p: *i, o, pg: Some(l.page) },
    }
}

/// The x of offset `o` on a line.
fn x_at(l: &LineBox, o: usize) -> f64 {
    for c in &l.clusters {
        if o <= c.a {
            return c.x;
        }
        if o < c.b {
            return c.x;
        }
    }
    match l.clusters.last() {
        Some(c) if c.brk => c.x,
        Some(c) => c.x + c.w,
        None => l.x_start,
    }
}

/// The offset a point at `x` lands on in a line.
fn offset_at(l: &LineBox, x: f64) -> usize {
    for c in &l.clusters {
        if c.brk {
            return c.a;
        }
        if x < c.x + c.w / 2.0 {
            return c.a;
        }
        if c.b > c.a + 1 && x < c.x + c.w {
            return c.b;
        }
    }
    if l.para_last {
        return l.atoms.end;
    }
    l.clusters.last().map(|c| c.a).unwrap_or(l.atoms.start)
}

fn hex(c: Color) -> String {
    format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn xml_string(el: &Element) -> String {
    let mut w = xml::Writer::new();
    w.element(el);
    String::from_utf8_lossy(&w.into_bytes()).into_owned()
}

impl ScrollDoc {
    /// The lines that show a place's paragraph (on its page for a header or footer).
    fn lines_of(&self, at: &SPos) -> Vec<&LineBox> {
        let all: Vec<&LineBox> = self.laid.lines.iter().filter(|l| !l.repeat && shows(l, &at.flow, at.p)).collect();
        if let (Flow::Part(_), Some(pg)) = (&at.flow, at.pg) {
            let on: Vec<&LineBox> = all.iter().copied().filter(|l| l.page == pg).collect();
            if !on.is_empty() {
                return on;
            }
        }
        if let Flow::Part(_) = at.flow {
            // One page's copy of the header.
            let first = all.first().map(|l| l.page);
            return all.into_iter().filter(|l| Some(l.page) == first).collect();
        }
        all
    }

    /// The line a place's caret sits on.
    fn line_of(&self, at: &SPos) -> Option<&LineBox> {
        let ls = self.lines_of(at);
        ls.iter().copied().find(|l| l.atoms.start <= at.o && at.o < l.atoms.end).or_else(|| ls.iter().copied().filter(|l| l.atoms.start <= at.o).last()).or_else(|| ls.first().copied())
    }

    fn sel_of(&self, sel: &Selection) -> Option<Span> {
        let a = SPos::from_pos(&sel.anchor)?;
        let f = SPos::from_pos(&sel.focus)?;
        Some(Span::new(self.clamp(&a), self.clamp(&f)))
    }

    /// The ribbon's state for a selection.
    fn state_json(&self, s: &Span) -> Json {
        let flow = s.flow().clone();
        let mut j = Json::obj();
        let runs = self.covered_runs(s);
        let effs: Vec<super::props::RunProps> = runs.iter().filter_map(|(p, ri)| self.para_ref(&flow, *p).map(|r| self.eff_run(r.para, *ri))).collect();
        let all = |f: &dyn Fn(&super::props::RunProps) -> bool| !effs.is_empty() && effs.iter().all(f);
        let mut flags = vec![
            ("bold", all(&|e| e.bold == Some(true))),
            ("italic", all(&|e| e.italic == Some(true))),
            ("underline", all(&|e| e.underline == Some(true))),
            ("strike", all(&|e| e.strike == Some(true))),
            ("superscript", all(&|e| e.vert == Some(VertAlign::Super))),
            ("subscript", all(&|e| e.vert == Some(VertAlign::Sub))),
            ("caps", all(&|e| e.caps == Some(true))),
            ("small_caps", all(&|e| e.small_caps == Some(true))),
        ];
        // Formatting chosen at the caret for the next typed text.
        if let Some((pp, list)) = &self.pending {
            if s.is_caret() && *pp == s.start {
                for (k, v) in list {
                    if let Some(slot) = flags.iter_mut().find(|(n, _)| n == k) {
                        slot.1 = match v {
                            Json::Bool(b) => *b,
                            Json::Str(x) => x != "none",
                            _ => slot.1,
                        };
                    }
                }
            }
        }
        for (k, v) in flags {
            j.set(k, v);
        }
        let same = |f: &dyn Fn(&super::props::RunProps) -> Option<String>| -> Json {
            let mut vals = effs.iter().map(f);
            match vals.next().flatten() {
                Some(first) if effs.iter().map(f).all(|v| v.as_deref() == Some(first.as_str())) => Json::Str(first),
                _ => Json::Null,
            }
        };
        j.set("font", same(&|e| e.font.clone()));
        j.set("size", match same(&|e| e.size.map(|x| format!("{x}"))) {
            Json::Str(s) => s.parse::<f64>().map(Json::Num).unwrap_or(Json::Null),
            _ => Json::Null,
        });
        j.set("color", same(&|e| Some(e.color.flatten().map(hex).unwrap_or_else(|| "auto".into()))));
        j.set("underline_style", same(&|e| e.underline_style.clone()));
        j.set("highlight", same(&|e| e.highlight.flatten().map(hex)));
        if let Some(r) = self.para_ref(&flow, s.start.p) {
            let pp = self.eff_para(r.para);
            let style = r.para.ppr.style.clone().or_else(|| self.model.styles.default_para.clone());
            j.set("style", style.clone().map(Json::Str).unwrap_or(Json::Null));
            j.set("style_name", style.as_deref().map(|x| Json::Str(self.model.styles.name_of(x).to_string())).unwrap_or(Json::Null));
            let align = match pp.align {
                Some(crate::ground::text::Align::Center) => "center",
                Some(crate::ground::text::Align::Right) => "right",
                Some(crate::ground::text::Align::Justify) | Some(crate::ground::text::Align::Distribute) => "justify",
                _ => "left",
            };
            j.set("align", align);
            j.set("align_left", align == "left");
            j.set("align_center", align == "center");
            j.set("align_right", align == "right");
            j.set("align_justify", align == "justify");
            let list = pp.num_id.filter(|n| *n != 0).and_then(|n| self.list_kind(n));
            j.set("list", list.map(Json::from).unwrap_or(Json::Null));
            j.set("list_bullet", list == Some("bullet"));
            j.set("list_number", list == Some("number"));
            j.set("list_level", pp.ilvl.unwrap_or(0) as usize);
            j.set("line_spacing", match pp.line {
                Some(crate::ground::text::LineSpacing::Multiple(m)) => Json::Num(m),
                Some(crate::ground::text::LineSpacing::Exactly(x)) => Json::obj().with("exact", x),
                Some(crate::ground::text::LineSpacing::AtLeast(x)) => Json::obj().with("at_least", x),
                _ => Json::Num(1.0),
            });
            j.set("before", pp.before.unwrap_or(0.0));
            j.set("after", pp.after.unwrap_or(0.0));
            j.set("ind_left", pp.ind_left.unwrap_or(0.0));
            j.set("ind_right", pp.ind_right.unwrap_or(0.0));
            j.set("first_line", pp.ind_first.unwrap_or(0.0));
            j.set("keep_next", pp.keep_next == Some(true));
            j.set("keep_lines", pp.keep_lines == Some(true));
            j.set("widow", pp.widow != Some(false));
            // A link and a picture under the selection.
            let ats = pos::atoms(&r.para.runs);
            let o = if s.is_caret() { s.start.o.saturating_sub(1) } else { s.start.o };
            let link = ats.get(o).and_then(|a| r.para.runs[a.run].link.clone());
            j.set("is_link", link.is_some());
            j.set("link", link.map(Json::Str).unwrap_or(Json::Null));
            let pic = ats.get(s.start.o).is_some_and(|a| a.kind == AKind::Pic) && (!s.is_caret() && s.end.o == s.start.o + 1);
            j.set("picture", pic);
        }
        let tl = self.tloc(&s.start);
        j.set("in_table", tl.is_some());
        if let Some(t) = &tl {
            j.set("table", Json::obj().with("rows", t.grid.rows.len()).with("cols", t.grid.cols()).with("row", t.row).with("col", t.grid.rows[t.row].cells[t.cell].col));
        }
        j.set("in_header", matches!(flow, Flow::Part(_)));
        let landscape = self.laid.pages.get(self.page_of(&s.start)).is_some_and(|p| p.width > p.height);
        j.set("landscape", landscape);
        j.set("portrait", !landscape);
        j
    }
}

impl Editing for ScrollDoc {
    fn hit(&self, page: usize, x: f64, y: f64) -> Option<Pos> {
        let on: Vec<&LineBox> = self.laid.lines.iter().filter(|l| l.page == page && !l.repeat).collect();
        if on.is_empty() {
            return None;
        }
        let hdist = |l: &LineBox| {
            if x < l.col_x {
                l.col_x - x
            } else if x > l.col_x + l.col_w {
                x - l.col_x - l.col_w
            } else {
                0.0
            }
        };
        let vdist = |l: &LineBox| {
            if y < l.slot_top {
                l.slot_top - y
            } else if y >= l.slot_top + l.slot_h {
                y - (l.slot_top + l.slot_h) + 0.001
            } else {
                0.0
            }
        };
        let best = on
            .iter()
            .copied()
            .min_by(|a, b| {
                let ka = (vdist(a), hdist(a));
                let kb = (vdist(b), hdist(b));
                ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
            })?;
        Some(key_pos(best, offset_at(best, x)).to_pos())
    }

    fn caret(&self, pos: &Pos) -> Option<CaretRect> {
        let at = self.clamp(&SPos::from_pos(pos)?);
        let l = self.line_of(&at)?;
        let height = (l.ascent + l.descent).max(1.0);
        Some(CaretRect { page: l.page, x: x_at(l, at.o), y: l.baseline - l.ascent, height })
    }

    fn selection_rects(&self, sel: &Selection) -> Vec<(usize, Rect)> {
        let Some(s) = self.sel_of(sel) else { return Vec::new() };
        if s.is_caret() {
            return Vec::new();
        }
        let flow = s.flow().clone();
        let mut out = Vec::new();
        for l in self.laid.lines.iter().filter(|l| !l.repeat && in_flow(l, &flow)) {
            let p = match &l.key {
                LineKey::Body(f) => *f,
                LineKey::Part(_, i) => *i,
            };
            if p < s.start.p || p > s.end.p {
                continue;
            }
            let a = if p == s.start.p { s.start.o } else { 0 };
            let b = if p == s.end.p { s.end.o } else { usize::MAX };
            let lo = a.max(l.atoms.start);
            let hi = b.min(l.atoms.end);
            let past_end = l.para_last && p < s.end.p;
            if lo > hi || (lo == hi && !past_end) {
                continue;
            }
            let x1 = x_at(l, lo);
            let mut x2 = x_at(l, hi);
            if past_end {
                x2 += 4.0;
            } else if b > l.atoms.end && !l.para_last {
                x2 = x2.max(l.x_end);
            }
            if x2 > x1 {
                out.push((l.page, Rect::new(x1, l.top, x2 - x1, l.height)));
            }
        }
        out
    }

    fn state(&self, sel: &Selection) -> Json {
        match self.sel_of(sel) {
            Some(s) => self.state_json(&s),
            None => Json::obj(),
        }
    }

    fn copy(&self, sel: &Selection) -> Clip {
        let Some(s) = self.sel_of(sel) else { return Clip::default() };
        if s.is_caret() {
            return Clip::default();
        }
        let flow = s.flow().clone();
        let part = self.part_name(&flow);
        let root = self.part_dom(&part);
        let mut text = String::new();
        let mut paras = Vec::new();
        for p in s.start.p..=s.end.p {
            let Some(r) = self.para_ref(&flow, p) else { continue };
            let n = pos::atom_count(&r.para.runs);
            let a = if p == s.start.p { s.start.o } else { 0 };
            let b = if p == s.end.p { s.end.o } else { n };
            if p > s.start.p {
                text.push('\n');
            }
            let ats = pos::atoms(&r.para.runs);
            for at in ats.iter().take(b.min(n)).skip(a) {
                match at.ch {
                    pos::OBJECT => {}
                    '\u{c}' => text.push('\n'),
                    c => text.push(c),
                }
            }
            let Some(root) = root.as_ref() else { continue };
            let Some(el) = el_at(root, &r.flat.path) else { continue };
            let runs = model::read_runs(&self.pkg, &part, &self.model, el, &r.para.fields_in);
            let pieces = super::dom::runs_for_copy(el, &runs, a, b);
            let mut ppr = el.child("pPr").cloned();
            if let Some(pp) = ppr.as_mut() {
                super::dom::set_child(pp, "sectPr", None, super::dom::PPR_ORDER);
            }
            let runs_json: Vec<Json> = pieces.into_iter().map(|(rpr, t)| Json::obj().with("rpr", rpr.map(|e| Json::Str(xml_string(&e))).unwrap_or(Json::Null)).with("text", t)).collect();
            paras.push(Json::obj().with("ppr", ppr.map(|e| Json::Str(xml_string(&e))).unwrap_or(Json::Null)).with("runs", Json::Arr(runs_json)));
        }
        Clip { text, rich: Some(Json::obj().with("scroll", 1).with("paras", Json::Arr(paras))) }
    }

    fn step(&self, pos: &Pos, motion: &str) -> Option<Pos> {
        let at = self.clamp(&SPos::from_pos(pos)?);
        let flow = at.flow.clone();
        let len = self.para_len(&flow, at.p);
        let count = self.flow_len(&flow);
        let next = match motion {
            "left" => {
                if at.o > 0 {
                    at.at(at.o - 1)
                } else if at.p > 0 {
                    at.para(at.p - 1, self.para_len(&flow, at.p - 1))
                } else {
                    return None;
                }
            }
            "right" => {
                if at.o < len {
                    at.at(at.o + 1)
                } else if at.p + 1 < count {
                    at.para(at.p + 1, 0)
                } else {
                    return None;
                }
            }
            "word_left" => {
                if at.o == 0 {
                    if at.p == 0 {
                        return None;
                    }
                    at.para(at.p - 1, self.para_len(&flow, at.p - 1))
                } else {
                    at.at(pos::word_left(&self.para_chars(&flow, at.p), at.o))
                }
            }
            "word_right" => {
                if at.o >= len {
                    if at.p + 1 >= count {
                        return None;
                    }
                    at.para(at.p + 1, 0)
                } else {
                    at.at(pos::word_right(&self.para_chars(&flow, at.p), at.o))
                }
            }
            "line_start" => {
                let l = self.line_of(&at)?;
                at.at(l.atoms.start)
            }
            "line_end" => {
                let l = self.line_of(&at)?;
                let end = if l.para_last {
                    l.atoms.end
                } else {
                    match l.clusters.last() {
                        Some(c) => c.a,
                        None => l.atoms.end,
                    }
                };
                at.at(end)
            }
            "doc_start" => at.para(0, 0),
            "doc_end" => {
                let last = count.saturating_sub(1);
                at.para(last, self.para_len(&flow, last))
            }
            "up" | "down" => {
                let l = self.line_of(&at)?;
                let x = x_at(l, at.o);
                let down = motion == "down";
                let same_page: Vec<&LineBox> = self
                    .laid
                    .lines
                    .iter()
                    .filter(|m| !m.repeat && m.page == l.page && in_flow(m, &flow) && if down { m.top >= l.top + l.height - 0.5 } else { m.top + m.height <= l.top + 0.5 })
                    .collect();
                let pick = |cands: &[&LineBox], down: bool| -> Option<LineBox> {
                    let overlapping: Vec<&LineBox> = cands.iter().copied().filter(|m| x >= m.col_x - 1.0 && x <= m.col_x + m.col_w + 1.0).collect();
                    let pool = if overlapping.is_empty() { cands.to_vec() } else { overlapping };
                    let best = if down {
                        pool.into_iter().min_by(|a, b| a.top.partial_cmp(&b.top).unwrap_or(std::cmp::Ordering::Equal))
                    } else {
                        pool.into_iter().max_by(|a, b| (a.top + a.height).partial_cmp(&(b.top + b.height)).unwrap_or(std::cmp::Ordering::Equal))
                    };
                    best.cloned()
                };
                let target = match pick(&same_page, down) {
                    Some(t) => t,
                    None => {
                        if matches!(flow, Flow::Part(_)) {
                            return None;
                        }
                        let page = if down { l.page + 1 } else { l.page.checked_sub(1)? };
                        let other: Vec<&LineBox> = self.laid.lines.iter().filter(|m| !m.repeat && m.page == page && in_flow(m, &flow)).collect();
                        pick(&other, down)?
                    }
                };
                key_pos(&target, offset_at(&target, x))
            }
            _ => return None,
        };
        Some(next.to_pos())
    }

    fn landed(&self, cmd: &Command) -> Option<Selection> {
        match &self.landed {
            Some((fp, sel)) if *fp == super::edit::fingerprint(cmd) => Some(sel.clone()),
            _ => None,
        }
    }
}
