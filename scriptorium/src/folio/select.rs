//! Folio's editing ground: the caret, the selection, copy, find and the ways around the document
//! (pages, links, bookmarks, form fields).
//!
//! A place ([`Pos`]) is `{"page": p, "index": i}`: the i-th character of page p's text in reading
//! order (the same text `read {page}` answers), or `{"page": p, "field": "name", "widget": n}` for a
//! form field the member clicked (typing there fills the field). Every character has a box on the
//! page: its glyph's, or for a space or line break the layout put in, the gap it stands for.
//!
//! The ways around change nothing in the document: `folio.go_to_page`, `folio.follow_link` and
//! `folio.bookmark` land the caret at the top of the target page, and the room turns to the caret's
//! page. A web link is never opened here: its address is in `state` (`link.uri`) and in
//! `read {links: page}`, for the page to ask the member and the host to open.

use crate::ground::bus::json::Json;
use crate::ground::bus::Command;
use crate::ground::display::Rect;
use crate::ground::edit::{CaretRect, Clip, Editing, Pos, Selection};

use super::forms::{self, Kind};
use super::render::TextGlyph;
use super::text;
use super::FolioDoc;

/// One page's text laid out for the caret.
#[derive(Debug, Clone, Default)]
pub struct PageText {
    pub chars: Vec<char>,
    /// Each character's box in display points.
    pub boxes: Vec<Rect>,
}

/// Where a link goes.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    /// A page of this document (from 0).
    Page(usize),
    /// A web (or other) address, opened by the host after asking the member.
    Uri(String),
}

/// A link's area on the page (display points) and its target.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLink {
    pub rect: Rect,
    pub target: LinkTarget,
}

impl PageLink {
    pub fn target_json(&self) -> Json {
        match &self.target {
            LinkTarget::Page(p) => Json::obj().with("page", *p),
            LinkTarget::Uri(u) => Json::obj().with("uri", u.as_str()),
        }
    }
}

/// A glyph's box: from a little below the baseline to the ascent, along its advance.
fn glyph_box(g: &TextGlyph) -> Rect {
    let s = if g.size.is_finite() && g.size > 0.0 { g.size } else { 10.0 };
    let len = (g.dx * g.dx + g.dy * g.dy).sqrt();
    let (dx, dy) = if len > 1e-6 && len.is_finite() { (g.dx / len, g.dy / len) } else { (1.0, 0.0) };
    // "Up" on the page (display y grows downwards).
    let (ux, uy) = (dy, -dx);
    let w = if g.w.is_finite() && g.w.abs() > 1e-6 { g.w } else { s * 0.5 };
    let pts = [
        (g.x - ux * 0.2 * s, g.y - uy * 0.2 * s),
        (g.x + ux * 0.8 * s, g.y + uy * 0.8 * s),
        (g.x - ux * 0.2 * s + dx * w, g.y - uy * 0.2 * s + dy * w),
        (g.x + ux * 0.8 * s + dx * w, g.y + uy * 0.8 * s + dy * w),
    ];
    let (mut l, mut t, mut r, mut b) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in pts {
        l = l.min(x);
        t = t.min(y);
        r = r.max(x);
        b = b.max(y);
    }
    Rect::from_ltrb(l, t, r, b)
}

/// Two boxes on one line: they share most of their height.
fn same_line(a: &Rect, b: &Rect) -> bool {
    let overlap = a.bottom().min(b.bottom()) - a.y.max(b.y);
    overlap > 0.5 * a.h.min(b.h).max(0.1)
}

/// Lay a page's glyphs out as text with a box for every character.
pub fn lay_page(glyphs: &[TextGlyph]) -> PageText {
    let laid = text::lay_out(glyphs);
    let n = laid.chars.len();
    let own: Vec<Option<Rect>> = laid.glyphs.iter().map(|g| g.and_then(|i| glyphs.get(i)).map(glyph_box)).collect();
    let mut boxes: Vec<Rect> = Vec::with_capacity(n);
    for i in 0..n {
        if let Some(b) = own[i] {
            boxes.push(b);
            continue;
        }
        let prev = if i > 0 { Some(boxes[i - 1]) } else { None };
        let next = own[i + 1..].iter().take(4).flatten().next().copied();
        let b = match (prev, laid.chars[i]) {
            (Some(p), '\n') => Rect::new(p.right(), p.y, 0.0, p.h),
            (Some(p), _) => match next {
                Some(nx) if same_line(&p, &nx) && nx.x >= p.right() => Rect::new(p.right(), p.y, nx.x - p.right(), p.h),
                _ => Rect::new(p.right(), p.y, p.h * 0.25, p.h),
            },
            (None, _) => match next {
                Some(nx) => Rect::new(nx.x, nx.y, 0.0, nx.h),
                None => Rect::new(0.0, 0.0, 0.0, 0.0),
            },
        };
        boxes.push(b);
    }
    PageText { chars: laid.chars, boxes }
}

/// A place in Folio.
#[derive(Debug, Clone, PartialEq)]
pub enum At {
    /// A character of a page's text.
    Text { page: usize, index: usize },
    /// A form field's widget.
    Field { page: usize, name: String, widget: u32 },
}

impl At {
    pub fn page(&self) -> usize {
        match self {
            At::Text { page, .. } | At::Field { page, .. } => *page,
        }
    }

    pub fn pos(&self) -> Pos {
        match self {
            At::Text { page, index } => Pos::new(Json::obj().with("page", *page).with("index", *index)),
            At::Field { page, name, widget } => Pos::new(Json::obj().with("page", *page).with("field", name.as_str()).with("widget", *widget)),
        }
    }

    pub fn read(p: &Pos) -> Option<At> {
        let v = p.json();
        let page = v.get("page")?.as_usize()?;
        if let Some(name) = v.get("field").and_then(Json::as_str) {
            let widget = v.get("widget").and_then(Json::as_usize).unwrap_or(0).min(u32::MAX as usize) as u32;
            return Some(At::Field { page, name: name.to_string(), widget });
        }
        Some(At::Text { page, index: v.get("index").and_then(Json::as_usize).unwrap_or(0) })
    }
}

pub fn text_pos(page: usize, index: usize) -> Pos {
    At::Text { page, index }.pos()
}

fn norm(c: char) -> char {
    if c.is_whitespace() {
        ' '
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

impl FolioDoc {
    fn page_text_of(&self, page: usize) -> Option<&PageText> {
        self.laid.get(page)
    }

    /// The text place `at`, clamped into its page (None when the page is gone).
    fn clamp_text(&self, page: usize, index: usize) -> Option<(usize, usize)> {
        let t = self.page_text_of(page)?;
        Some((page, index.min(t.chars.len())))
    }

    /// The sorted text range a selection covers (fields cover nothing).
    fn text_range(&self, sel: &Selection) -> Option<((usize, usize), (usize, usize))> {
        let a = At::read(&sel.anchor)?;
        let b = At::read(&sel.focus)?;
        let (At::Text { page: pa, index: ia }, At::Text { page: pb, index: ib }) = (&a, &b) else { return None };
        let a = self.clamp_text(*pa, *ia)?;
        let b = self.clamp_text(*pb, *ib)?;
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    /// The text from `a` to `b` (pages joined by a blank line).
    pub(crate) fn range_text(&self, a: (usize, usize), b: (usize, usize)) -> String {
        let mut out = String::new();
        for page in a.0..=b.0 {
            let Some(t) = self.page_text_of(page) else { break };
            let s = if page == a.0 { a.1 } else { 0 };
            let e = if page == b.0 { b.1 } else { t.chars.len() };
            if page > a.0 {
                out.push_str("\n\n");
            }
            out.extend(t.chars[s.min(t.chars.len())..e.min(t.chars.len()).max(s.min(t.chars.len()))].iter());
        }
        out
    }

    fn line_start(t: &PageText, i: usize) -> usize {
        let mut j = i.min(t.chars.len());
        while j > 0 && t.chars[j - 1] != '\n' {
            j -= 1;
        }
        j
    }

    fn line_end(t: &PageText, i: usize) -> usize {
        let mut j = i.min(t.chars.len());
        while j < t.chars.len() && t.chars[j] != '\n' {
            j += 1;
        }
        j
    }

    fn text_caret(&self, page: usize, index: usize) -> Option<CaretRect> {
        let t = self.page_text_of(page)?;
        let n = t.chars.len();
        if n == 0 {
            return Some(CaretRect { page, x: 0.0, y: 0.0, height: 0.0 });
        }
        let (b, x) = if index < n {
            let b = t.boxes[index];
            (b, b.x)
        } else {
            let b = t.boxes[n - 1];
            (b, b.right())
        };
        Some(CaretRect { page, x, y: b.y, height: b.h })
    }

    /// Within line [start, end] of a page, the index whose caret is nearest `x`.
    fn nearest_in_line(&self, page: usize, start: usize, end: usize, x: f64) -> usize {
        let mut best = (f64::MAX, start);
        for i in start..=end {
            if let Some(c) = self.text_caret(page, i) {
                let d = (c.x - x).abs();
                if d < best.0 {
                    best = (d, i);
                }
            }
        }
        best.1
    }

    fn vertical(&self, page: usize, index: usize, down: bool) -> Option<(usize, usize)> {
        let t = self.page_text_of(page)?;
        let x = self.text_caret(page, index)?.x;
        let n = t.chars.len();
        let (tp, start, end) = if down {
            let le = Self::line_end(t, index);
            if le >= n {
                let np = page + 1;
                let nt = self.page_text_of(np)?;
                (np, 0, Self::line_end(nt, 0))
            } else {
                (page, le + 1, Self::line_end(t, le + 1))
            }
        } else {
            let ls = Self::line_start(t, index);
            if ls == 0 {
                let pp = page.checked_sub(1)?;
                let pt = self.page_text_of(pp)?;
                let pn = pt.chars.len();
                (pp, Self::line_start(pt, pn), pn)
            } else {
                (page, Self::line_start(t, ls - 1), ls - 1)
            }
        };
        Some((tp, self.nearest_in_line(tp, start, end, x)))
    }

    fn step_text(&self, page: usize, index: usize, motion: &str) -> Option<(usize, usize)> {
        let t = self.page_text_of(page)?;
        let n = t.chars.len();
        let i = index.min(n);
        let last = self.laid.len().checked_sub(1)?;
        match motion {
            "left" => {
                if i > 0 {
                    Some((page, i - 1))
                } else if page > 0 {
                    Some((page - 1, self.laid[page - 1].chars.len()))
                } else {
                    None
                }
            }
            "right" => {
                if i < n {
                    Some((page, i + 1))
                } else if page < last {
                    Some((page + 1, 0))
                } else {
                    None
                }
            }
            "word_right" => {
                if i >= n {
                    return if page < last { Some((page + 1, 0)) } else { None };
                }
                let mut j = i;
                while j < n && is_word(t.chars[j]) {
                    j += 1;
                }
                while j < n && !is_word(t.chars[j]) {
                    j += 1;
                }
                Some((page, j))
            }
            "word_left" => {
                if i == 0 {
                    return if page > 0 { Some((page - 1, self.laid[page - 1].chars.len())) } else { None };
                }
                let mut j = i;
                while j > 0 && !is_word(t.chars[j - 1]) {
                    j -= 1;
                }
                while j > 0 && is_word(t.chars[j - 1]) {
                    j -= 1;
                }
                Some((page, j))
            }
            "line_start" => Some((page, Self::line_start(t, i))),
            "line_end" => Some((page, Self::line_end(t, i))),
            "up" => self.vertical(page, i, false),
            "down" => self.vertical(page, i, true),
            "doc_start" => Some((0, 0)),
            "doc_end" => Some((last, self.laid[last].chars.len())),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Find
    // -----------------------------------------------------------------------------------------------

    fn matches_on(&self, page: usize, q: &[char]) -> Vec<usize> {
        let Some(t) = self.page_text_of(page) else { return Vec::new() };
        let hay: Vec<char> = t.chars.iter().map(|&c| norm(c)).collect();
        let mut out = Vec::new();
        if q.is_empty() || hay.len() < q.len() {
            return out;
        }
        for s in 0..=hay.len() - q.len() {
            if hay[s..s + q.len()] == *q {
                out.push(s);
            }
        }
        out
    }

    /// The next (or previous) match of `query` from the selection, wrapping round the document:
    /// (page, start, end).
    pub fn find_match(&self, query: &str, forward: bool, sel: Option<&Selection>) -> Option<(usize, usize, usize)> {
        let q: Vec<char> = query.chars().map(norm).collect();
        let np = self.laid.len();
        if q.iter().all(|c| *c == ' ') || np == 0 {
            return None;
        }
        let range = sel.and_then(|s| self.text_range(s));
        let field_page = sel.and_then(|s| At::read(&s.focus)).map(|a| a.page());
        let (sp, si) = match (range, forward) {
            (Some((_, b)), true) => b,
            (Some((a, _)), false) => a,
            (None, _) => match field_page {
                Some(p) if p < np => (p, 0),
                _ if forward => (0, 0),
                _ => (np - 1, self.laid[np - 1].chars.len()),
            },
        };
        let sp = sp.min(np - 1);
        for k in 0..=np {
            let p = if forward { (sp + k) % np } else { (sp + np - k % np) % np };
            let ms = self.matches_on(p, &q);
            if forward {
                if let Some(&m) = ms.iter().find(|&&m| k != 0 || m >= si) {
                    return Some((p, m, m + q.len()));
                }
            } else if let Some(&m) = ms.iter().rev().find(|&&m| k != 0 || m < si) {
                return Some((p, m, m + q.len()));
            }
        }
        None
    }

    // -----------------------------------------------------------------------------------------------
    // Ways around
    // -----------------------------------------------------------------------------------------------

    /// The page a command's selection is on (`from` wins, then the focus, then 0).
    fn current_page(&self, cmd: &Command) -> usize {
        cmd.args
            .get("from")
            .and_then(Json::as_usize)
            .or_else(|| cmd.args.get("sel").and_then(Selection::from_json).and_then(|s| At::read(&s.focus)).map(|a| a.page()))
            .unwrap_or(0)
            .min(self.pages.len().saturating_sub(1))
    }

    /// `folio.go_to_page`'s target: `page` (from 0), or `to` = first, last, next, previous.
    pub(crate) fn go_to_target(&self, cmd: &Command) -> Result<usize, String> {
        let n = self.pages.len();
        if n == 0 {
            return Err("the document has no pages".into());
        }
        if let Some(p) = cmd.args.get("page") {
            let p = p.as_usize().ok_or("\"page\" is a page number from 0")?;
            if p >= n {
                return Err(format!("no page {} (the document has {} pages, numbered from 0)", p, n));
            }
            return Ok(p);
        }
        let cur = self.current_page(cmd);
        match cmd.args.get("to").and_then(Json::as_str).unwrap_or("next") {
            "first" => Ok(0),
            "last" => Ok(n - 1),
            "next" => Ok((cur + 1).min(n - 1)),
            "previous" | "prev" => Ok(cur.saturating_sub(1)),
            other => Err(format!("\"to\" is first, last, next or previous (not '{}')", other)),
        }
    }

    /// The link under a page point.
    pub fn link_at(&self, page: usize, x: f64, y: f64) -> Option<&PageLink> {
        self.links.get(page)?.iter().rev().find(|l| l.rect.contains(x, y))
    }

    /// The link under the selection: under the caret, or crossing the selected boxes.
    pub fn link_for(&self, sel: &Selection) -> Option<&PageLink> {
        if let Some(At::Text { page, index }) = At::read(&sel.focus) {
            let c = self.text_caret(page, index)?;
            let probe = |x: f64| self.link_at(page, x, c.y + c.height * 0.5);
            if let Some(l) = probe(c.x + 0.5).or_else(|| probe(c.x - 0.5)) {
                return Some(l);
            }
        }
        for (page, r) in self.selection_rects(sel) {
            if let Some(l) = self.links.get(page).and_then(|ls| ls.iter().find(|l| l.rect.intersects(&r))) {
                return Some(l);
            }
        }
        None
    }

    /// `folio.follow_link`'s link: at { page, x, y } or under { sel }.
    pub(crate) fn link_of(&self, cmd: &Command) -> Option<&PageLink> {
        let num = |k: &str| cmd.args.get(k).and_then(Json::as_f64);
        if let (Some(page), Some(x), Some(y)) = (cmd.args.get("page").and_then(Json::as_usize), num("x"), num("y")) {
            return self.link_at(page, x, y);
        }
        let sel = cmd.args.get("sel").and_then(Selection::from_json)?;
        self.link_for(&sel)
    }

    /// `folio.bookmark`'s target page: bookmark `index` (in `read {outline}` order), or `to` = next /
    /// previous from the selection's page.
    pub(crate) fn bookmark_target(&self, cmd: &Command) -> Result<usize, String> {
        if self.outlines.is_empty() {
            return Err("this document has no bookmarks".into());
        }
        if let Some(i) = cmd.args.get("index") {
            let i = i.as_usize().ok_or("\"index\" is a bookmark number from 0")?;
            let o = self.outlines.get(i).ok_or_else(|| format!("no bookmark {} (the document has {})", i, self.outlines.len()))?;
            return o.page.ok_or_else(|| format!("bookmark '{}' does not lead to a page", o.title));
        }
        let cur = self.current_page(cmd);
        let pages: Vec<usize> = self.outlines.iter().filter_map(|o| o.page).collect();
        match cmd.args.get("to").and_then(Json::as_str).unwrap_or("next") {
            "next" => pages.iter().copied().filter(|&p| p > cur).min().ok_or_else(|| "no bookmark after this page".into()),
            "previous" | "prev" => pages.iter().copied().filter(|&p| p < cur).max().ok_or_else(|| "no bookmark before this page".into()),
            other => Err(format!("\"to\" is next or previous (not '{}')", other)),
        }
    }

    /// The outline as a tree: `[{ "index", "title", "page", "children": [..] }]`.
    pub fn outline_tree(&self) -> Json {
        fn build(entries: &[super::OutlineEntry], i: &mut usize, level: usize) -> Json {
            let mut out = Json::arr();
            while *i < entries.len() && entries[*i].level >= level {
                if entries[*i].level > level {
                    // A deeper entry with no parent at this level: hang it here.
                    let kids = build(entries, i, entries[*i].level);
                    if let Json::Arr(k) = kids {
                        for item in k {
                            out.push(item);
                        }
                    }
                    continue;
                }
                let e = &entries[*i];
                let index = *i;
                *i += 1;
                let children = if *i < entries.len() && entries[*i].level > level { build(entries, i, level + 1) } else { Json::arr() };
                out.push(
                    Json::obj()
                        .with("index", index)
                        .with("title", e.title.as_str())
                        .with("page", e.page.map(Json::from).unwrap_or(Json::Null))
                        .with("children", children),
                );
            }
            out
        }
        let mut i = 0;
        build(&self.outlines, &mut i, 0)
    }

    // -----------------------------------------------------------------------------------------------
    // Fields as places
    // -----------------------------------------------------------------------------------------------

    fn field_at(&self, page: usize, x: f64, y: f64) -> Option<At> {
        let p = self.pages.get(page)?;
        let base = p.base_matrix();
        for f in self.fields.iter().rev() {
            if !f.fillable() {
                continue;
            }
            for w in &f.widgets {
                if w.page != Some(page) {
                    continue;
                }
                let r = base.apply_rect(&Rect::from_ltrb(w.rect[0], w.rect[1], w.rect[2], w.rect[3]));
                if r.contains(x, y) {
                    return Some(At::Field { page, name: f.name.clone(), widget: w.num });
                }
            }
        }
        None
    }

    /// A widget's box in display points.
    pub(crate) fn widget_rect(&self, page: usize, rect: &[f64; 4]) -> Option<Rect> {
        let p = self.pages.get(page)?;
        Some(p.base_matrix().apply_rect(&Rect::from_ltrb(rect[0], rect[1], rect[2], rect[3])))
    }

    fn field_caret(&self, page: usize, name: &str, widget: u32) -> Option<CaretRect> {
        let f = self.fields.iter().find(|f| f.name == name)?;
        let w = f.widgets.iter().find(|w| w.num == widget && w.page == Some(page)).or_else(|| f.widgets.iter().find(|w| w.page == Some(page)))?;
        let r = self.widget_rect(page, &w.rect)?;
        let h = r.h.max(1.0);
        match f.kind {
            Kind::Text | Kind::Combo => {
                let size = forms::shown_size(f, &w.rect);
                let last_line = f.value.rsplit('\n').next().unwrap_or("");
                let shown: String = if f.password() { "*".repeat(last_line.chars().count()) } else { last_line.to_string() };
                let tw = forms::text_width(&shown, size);
                let x = (r.x + 2.0 + tw).min(r.right() - 1.0).max(r.x + 1.0);
                let ch = (size * 1.15).min(h - 2.0).max(1.0);
                let y = if f.multiline() { r.y + 2.0 + (f.value.matches('\n').count() as f64 * size * 1.15).min((h - ch - 2.0).max(0.0)) } else { r.y + (h - ch) / 2.0 };
                Some(CaretRect { page, x, y, height: ch })
            }
            _ => Some(CaretRect { page, x: r.x, y: r.y, height: h }),
        }
    }

    /// Fields in tab order (page, then top to bottom, then left to right): (page, name, widget).
    pub(crate) fn field_order(&self) -> Vec<(usize, String, u32)> {
        let mut v: Vec<(usize, f64, f64, String, u32)> = Vec::new();
        for f in &self.fields {
            if !f.fillable() {
                continue;
            }
            for w in &f.widgets {
                if let Some(p) = w.page {
                    let r = self.widget_rect(p, &w.rect).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
                    v.push((p, (r.y / 4.0).round(), r.x, f.name.clone(), w.num));
                    if f.kind == Kind::Check || f.kind == Kind::Radio {
                        continue;
                    }
                    break;
                }
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)).then(a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)));
        v.into_iter().map(|(p, _, _, n, w)| (p, n, w)).collect()
    }

    /// The field after (or before) the selection's place.
    pub(crate) fn next_field(&self, sel: Option<&Selection>, back: bool) -> Option<At> {
        let order = self.field_order();
        if order.is_empty() {
            return None;
        }
        let here = sel.and_then(|s| At::read(&s.focus));
        let idx = match &here {
            Some(At::Field { name, widget, .. }) => order.iter().position(|(_, n, w)| n == name && w == widget).or_else(|| order.iter().position(|(_, n, _)| n == name)),
            _ => None,
        };
        let pick = match (idx, back) {
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
            (None, false) => {
                let page = here.map(|h| h.page()).unwrap_or(0);
                order.iter().position(|(p, _, _)| *p >= page).unwrap_or(0)
            }
            (None, true) => {
                let page = here.map(|h| h.page()).unwrap_or(usize::MAX);
                order.iter().rposition(|(p, _, _)| *p <= page).unwrap_or(order.len() - 1)
            }
        };
        let (page, name, widget) = order[pick].clone();
        Some(At::Field { page, name, widget })
    }

    /// The field place a command acts on (its selection's focus).
    pub(crate) fn field_of(&self, cmd: &Command) -> Option<At> {
        let sel = cmd.args.get("sel").and_then(Selection::from_json)?;
        match At::read(&sel.focus)? {
            a @ At::Field { .. } => Some(a),
            _ => None,
        }
    }
}

impl Editing for FolioDoc {
    fn hit(&self, page: usize, x: f64, y: f64) -> Option<Pos> {
        if let Some(f) = self.field_at(page, x, y) {
            return Some(f.pos());
        }
        let t = self.page_text_of(page)?;
        let mut best: Option<(f64, usize)> = None;
        for (i, b) in t.boxes.iter().enumerate() {
            if t.chars[i] == '\n' {
                continue;
            }
            let dy = if y < b.y { b.y - y } else if y > b.bottom() { y - b.bottom() } else { 0.0 };
            let dx = if x < b.x { b.x - x } else if x > b.right() { x - b.right() } else { 0.0 };
            let d = dy * 4.0 + dx;
            if best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        let (d, i) = best?;
        if d > 60.0 {
            return None;
        }
        let b = t.boxes[i];
        let index = if x > b.x + b.w / 2.0 { i + 1 } else { i };
        Some(text_pos(page, index))
    }

    fn caret(&self, pos: &Pos) -> Option<CaretRect> {
        match At::read(pos)? {
            At::Text { page, index } => self.text_caret(page, index),
            At::Field { page, name, widget } => self.field_caret(page, &name, widget),
        }
    }

    fn selection_rects(&self, sel: &Selection) -> Vec<(usize, Rect)> {
        let mut out = Vec::new();
        if sel.is_collapsed() {
            return out;
        }
        let Some((a, b)) = self.text_range(sel) else { return out };
        for page in a.0..=b.0 {
            let Some(t) = self.page_text_of(page) else { break };
            let s = if page == a.0 { a.1 } else { 0 };
            let e = if page == b.0 { b.1 } else { t.chars.len() };
            let mut cur: Option<Rect> = None;
            for i in s..e.min(t.chars.len()) {
                if t.chars[i] == '\n' {
                    continue;
                }
                let bx = t.boxes[i];
                cur = match cur {
                    Some(c) if same_line(&c, &bx) && bx.x >= c.x - 1.0 => Some(c.union(&bx)),
                    Some(c) => {
                        out.push((page, c));
                        Some(bx)
                    }
                    None => Some(bx),
                };
            }
            if let Some(c) = cur {
                out.push((page, c));
            }
            if out.len() > 20_000 {
                break;
            }
        }
        out
    }

    fn state(&self, sel: &Selection) -> Json {
        let at = At::read(&sel.focus);
        let page = at.as_ref().map(|a| a.page()).unwrap_or(0).min(self.pages.len().saturating_sub(1));
        let (w, h) = self.page_size(page).unwrap_or((612.0, 792.0));
        let round = |v: f64| (v * 100.0).round() / 100.0;
        let selected = self.text_range(sel).map(|(a, b)| self.range_text(a, b).chars().count()).unwrap_or(0);
        let link = self.link_for(sel).map(|l| l.target_json()).unwrap_or(Json::Null);
        let field = match &at {
            Some(At::Field { name, .. }) => self.fields.iter().find(|f| &f.name == name).map(|f| f.to_json(self)).unwrap_or(Json::Null),
            _ => Json::Null,
        };
        let mut s = Json::obj()
            .with("page", page)
            .with("page_count", self.pages.len())
            .with("width", round(w))
            .with("height", round(h))
            .with("rotate", self.pages.get(page).map_or(0, |p| p.rotate as i64))
            .with("collapsed", sel.is_collapsed())
            .with("selected", selected)
            .with("link", link)
            .with("in_field", !field.is_null())
            .with("field", field)
            .with("fields", self.fields.iter().filter(|f| f.fillable()).count())
            .with("bookmarks", self.outlines.len())
            .with("first_page", page == 0)
            .with("last_page", page + 1 >= self.pages.len())
            .with("can_copy", self.permits(5))
            .with("locked", self.file.security.is_some());
        if let Some(t) = self.page_text_of(page) {
            s.set("page_chars", t.chars.len());
        }
        s
    }

    fn copy(&self, sel: &Selection) -> Clip {
        if !self.permits(5) {
            return Clip::default();
        }
        match self.text_range(sel) {
            Some((a, b)) => Clip::text(self.range_text(a, b)),
            None => match At::read(&sel.focus) {
                Some(At::Field { name, .. }) => self.fields.iter().find(|f| f.name == name && f.kind == Kind::Text && !f.password()).map(|f| Clip::text(f.value.clone())).unwrap_or_default(),
                _ => Clip::default(),
            },
        }
    }

    fn step(&self, pos: &Pos, motion: &str) -> Option<Pos> {
        match At::read(pos)? {
            At::Text { page, index } => self.step_text(page, index, motion).map(|(p, i)| text_pos(p, i)),
            At::Field { page, .. } => match motion {
                // Leaving a field by the keys: to its page's text.
                "doc_start" | "doc_end" | "up" | "down" => self.step_text(page, 0, motion).map(|(p, i)| text_pos(p, i)),
                _ => Some(pos.clone()),
            },
        }
    }

    fn landed(&self, cmd: &Command) -> Option<Selection> {
        let caret = |page: usize| Some(Selection::caret(text_pos(page, 0)));
        match cmd.name.as_str() {
            "folio.find" => {
                let q = cmd.args.get("query").and_then(Json::as_str)?;
                let forward = cmd.args.get("next").and_then(Json::as_bool).unwrap_or(true);
                let sel = cmd.args.get("sel").and_then(Selection::from_json);
                let (p, s, e) = self.find_match(q, forward, sel.as_ref())?;
                Some(Selection::new(text_pos(p, s), text_pos(p, e)))
            }
            "folio.go_to_page" => caret(self.go_to_target(cmd).ok()?),
            "folio.bookmark" => caret(self.bookmark_target(cmd).ok()?),
            "folio.follow_link" => match self.link_of(cmd)?.target {
                LinkTarget::Page(p) if p < self.pages.len() => caret(p),
                _ => None,
            },
            "folio.delete_pages" | "folio.rotate_pages" => {
                let first = super::pages_arg(cmd).ok()?.into_iter().min()?;
                caret(first.min(self.pages.len().saturating_sub(1)))
            }
            "folio.next_field" => {
                let sel = cmd.args.get("sel").and_then(Selection::from_json);
                let back = cmd.args.get("back").and_then(Json::as_bool).unwrap_or(false);
                Some(Selection::caret(self.next_field(sel.as_ref(), back)?.pos()))
            }
            "folio.insert_text" | "folio.backspace" => Some(Selection::caret(self.field_of(cmd)?.pos())),
            "folio.set_field" => {
                let name = cmd.args.get("name").and_then(Json::as_str)?;
                let f = self.fields.iter().find(|f| f.name == name)?;
                let w = f.widgets.iter().find(|w| w.page.is_some())?;
                Some(Selection::caret(At::Field { page: w.page?, name: f.name.clone(), widget: w.num }.pos()))
            }
            _ => None,
        }
    }
}
