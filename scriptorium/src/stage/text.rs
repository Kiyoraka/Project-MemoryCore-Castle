//! A text body (`p:txBody`, `a:txBody`) as paragraphs of runs, line breaks and fields, edited by
//! character position and written back into the same element. A paragraph nobody touched is written
//! back as the very element it was read from, so its bytes stay as they were.
//!
//! Positions are `(paragraph, char)`: a run counts its characters, a line break (`a:br`) and a field
//! (`a:fld`, the slide number say) count one each.

use crate::ground::xml::{Element, Node};

/// One thing in a paragraph.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Run { rpr: Option<Element>, text: String },
    Br { rpr: Option<Element> },
    Fld { el: Element },
}

impl Item {
    pub fn len(&self) -> usize {
        match self {
            Item::Run { text, .. } => text.chars().count(),
            _ => 1,
        }
    }

    pub fn rpr(&self) -> Option<&Element> {
        match self {
            Item::Run { rpr, .. } | Item::Br { rpr } => rpr.as_ref(),
            Item::Fld { el } => el.child("rPr"),
        }
    }

    fn rpr_mut(&mut self, a: &str) -> &mut Element {
        let slot = match self {
            Item::Run { rpr, .. } | Item::Br { rpr } => rpr,
            Item::Fld { el } => {
                if el.child("rPr").is_none() {
                    let at = el.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "pPr" || e.local() == "t")).unwrap_or(0);
                    el.children.insert(at, Node::Element(new_rpr(a)));
                }
                return el.child_mut("rPr").expect("just made");
            }
        };
        slot.get_or_insert_with(|| new_rpr(a))
    }
}

fn new_rpr(a: &str) -> Element {
    let mut r = Element::new(format!("{a}:rPr"));
    r.attrs.push(("lang".into(), "en-US".into()));
    r.attrs.push(("dirty".into(), "0".into()));
    r
}

/// One paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct Para {
    pub ppr: Option<Element>,
    pub items: Vec<Item>,
    pub end: Option<Element>,
    /// Children we do not edit (comments, extensions), written back after the runs.
    pub other: Vec<Node>,
    /// The element as read, when nothing changed it.
    orig: Option<Element>,
}

impl Para {
    pub fn empty(ppr: Option<Element>, end: Option<Element>) -> Para {
        Para { ppr, items: Vec::new(), end, other: Vec::new(), orig: None }
    }

    pub fn len(&self) -> usize {
        self.items.iter().map(Item::len).sum()
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        for it in &self.items {
            match it {
                Item::Run { text, .. } => s.push_str(text),
                Item::Br { .. } => s.push('\u{b}'),
                Item::Fld { el } => s.push_str(&el.child("t").map(|t| t.text()).unwrap_or_default()),
            }
        }
        s
    }

    /// The level (`a:pPr lvl`), 0 when unset.
    pub fn level(&self) -> usize {
        self.ppr.as_ref().and_then(|p| p.attr("lvl")).and_then(|s| s.trim().parse().ok()).unwrap_or(0)
    }

    fn parse(p: &Element) -> Para {
        let mut para = Para { ppr: None, items: Vec::new(), end: None, other: Vec::new(), orig: Some(p.clone()) };
        for c in &p.children {
            match c {
                Node::Element(e) => match e.local() {
                    "pPr" => para.ppr = Some(e.clone()),
                    "r" => para.items.push(Item::Run { rpr: e.child("rPr").cloned(), text: e.child("t").map(|t| t.text()).unwrap_or_default() }),
                    "br" => para.items.push(Item::Br { rpr: e.child("rPr").cloned() }),
                    "fld" => para.items.push(Item::Fld { el: e.clone() }),
                    "endParaRPr" => para.end = Some(e.clone()),
                    _ => para.other.push(c.clone()),
                },
                Node::Text(t) if t.trim().is_empty() => {}
                other => para.other.push(other.clone()),
            }
        }
        para
    }

    fn touch(&mut self) {
        self.orig = None;
    }

    fn write(&self, a: &str) -> Element {
        if let Some(o) = &self.orig {
            return o.clone();
        }
        let mut p = Element::new(format!("{a}:p"));
        if let Some(pp) = &self.ppr {
            p.children.push(Node::Element(pp.clone()));
        }
        // Neighbouring runs with the same formatting are written as one.
        let mut items: Vec<Item> = Vec::with_capacity(self.items.len());
        for it in &self.items {
            if let (Some(Item::Run { rpr: pr, text: pt }), Item::Run { rpr, text }) = (items.last_mut(), it) {
                if pr == rpr {
                    pt.push_str(text);
                    continue;
                }
            }
            items.push(it.clone());
        }
        for it in &items {
            match it {
                Item::Run { rpr, text } => {
                    let mut r = Element::new(format!("{a}:r"));
                    if let Some(rp) = rpr {
                        r.children.push(Node::Element(rp.clone()));
                    }
                    let mut t = Element::new(format!("{a}:t"));
                    if !text.is_empty() {
                        t.children.push(Node::Text(text.clone()));
                    }
                    r.children.push(Node::Element(t));
                    p.children.push(Node::Element(r));
                }
                Item::Br { rpr } => {
                    let mut b = Element::new(format!("{a}:br"));
                    if let Some(rp) = rpr {
                        b.children.push(Node::Element(rp.clone()));
                    }
                    p.children.push(Node::Element(b));
                }
                Item::Fld { el } => p.children.push(Node::Element(el.clone())),
            }
        }
        p.children.extend(self.other.iter().cloned());
        if let Some(e) = &self.end {
            p.children.push(Node::Element(e.clone()));
        }
        p
    }

    /// Split item boundaries so that `at` falls between items; returns the item index there.
    fn cut(&mut self, at: usize) -> usize {
        let mut pos = 0;
        for i in 0..self.items.len() {
            let len = self.items[i].len();
            if at == pos {
                return i;
            }
            if at < pos + len {
                if let Item::Run { rpr, text } = &self.items[i] {
                    let k = at - pos;
                    let byte = text.char_indices().nth(k).map(|(b, _)| b).unwrap_or(text.len());
                    let (left, right) = (text[..byte].to_string(), text[byte..].to_string());
                    let rpr = rpr.clone();
                    self.items[i] = Item::Run { rpr: rpr.clone(), text: left };
                    self.items.insert(i + 1, Item::Run { rpr, text: right });
                    self.touch();
                    return i + 1;
                }
                return i + 1;
            }
            pos += len;
        }
        self.items.len()
    }

    /// The run properties typing at `at` takes: the run before, else the run after, else the
    /// paragraph's end properties.
    pub fn rpr_at(&self, at: usize) -> Option<Element> {
        let mut pos = 0;
        let mut before: Option<&Item> = None;
        for it in &self.items {
            let len = it.len();
            if at > pos && at <= pos + len || (at == pos && len == 0 && matches!(it, Item::Run { .. })) {
                return it.rpr().cloned();
            }
            if pos + len <= at {
                before = Some(it);
            } else {
                return before.or(Some(it)).and_then(|i| i.rpr().cloned());
            }
            pos += len;
        }
        before.and_then(|i| i.rpr().cloned()).or_else(|| self.end.as_ref().map(|e| renamed(e, "rPr")))
    }
}

fn renamed(e: &Element, local: &str) -> Element {
    let mut r = e.clone();
    r.name = match e.prefix() {
        Some(p) => format!("{p}:{local}"),
        None => local.to_string(),
    };
    r
}

/// A text body's paragraphs.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub paras: Vec<Para>,
    /// The DrawingML prefix (`a`).
    pub a: String,
}

/// A place in a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct At {
    pub para: usize,
    pub ch: usize,
}

impl At {
    pub fn new(para: usize, ch: usize) -> At {
        At { para, ch }
    }
}

impl Body {
    /// The paragraphs of a text body element (an empty one when it has none).
    pub fn read(tx: &Element) -> Body {
        let a = tx.elements().find_map(|e| e.prefix().map(str::to_string)).unwrap_or_else(|| "a".into());
        let mut paras: Vec<Para> = tx.children_named("p").map(Para::parse).collect();
        if paras.is_empty() {
            let mut p = Para::empty(None, None);
            p.touch();
            paras.push(p);
        }
        Body { paras, a }
    }

    /// Write the paragraphs back into `tx` (the body and list properties and everything else kept).
    pub fn write_into(&self, tx: &mut Element) {
        let first = tx.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "p"));
        tx.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "p"));
        let at = first.unwrap_or_else(|| tx.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "extLst")).unwrap_or(tx.children.len()));
        let at = at.min(tx.children.len());
        for (k, p) in self.paras.iter().enumerate() {
            tx.children.insert(at + k, Node::Element(p.write(&self.a)));
        }
    }

    pub fn text(&self) -> String {
        self.paras.iter().map(Para::text).collect::<Vec<_>>().join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.paras.iter().all(|p| p.len() == 0)
    }

    /// The last place.
    pub fn end(&self) -> At {
        let p = self.paras.len().saturating_sub(1);
        At::new(p, self.paras.get(p).map(Para::len).unwrap_or(0))
    }

    /// `at` kept inside the body.
    pub fn clamp(&self, at: At) -> At {
        if self.paras.is_empty() {
            return At::new(0, 0);
        }
        let p = at.para.min(self.paras.len() - 1);
        At::new(p, at.ch.min(self.paras[p].len()))
    }

    /// Delete from `a` to `b` (any order); paragraphs between are joined. Returns the place left.
    pub fn delete(&mut self, a: At, b: At) -> At {
        let (s, e) = (self.clamp(a.min(b)), self.clamp(a.max(b)));
        if s == e {
            return s;
        }
        if s.para == e.para {
            let p = &mut self.paras[s.para];
            let i = p.cut(s.ch);
            let j = p.cut(e.ch);
            p.items.drain(i..j);
            p.touch();
            return s;
        }
        // Cut the tail of the first and the head of the last, then join.
        let tail = {
            let last = &mut self.paras[e.para];
            let j = last.cut(e.ch);
            last.items.split_off(j)
        };
        let end = self.paras[e.para].end.clone();
        let first = &mut self.paras[s.para];
        let i = first.cut(s.ch);
        first.items.truncate(i);
        first.items.extend(tail);
        if end.is_some() {
            first.end = end;
        }
        first.touch();
        self.paras.drain(s.para + 1..=e.para);
        s
    }

    /// Insert `text` at `at` (`\n` starts a new paragraph, `\u{b}` breaks the line); returns the place
    /// after it. New text takes `rpr` when given, else the formatting typing there takes.
    pub fn insert(&mut self, at: At, text: &str, rpr: Option<&Element>) -> At {
        let mut at = self.clamp(at);
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        for (k, piece) in text.split('\n').enumerate() {
            if k > 0 {
                at = self.split(at);
            }
            for (j, seg) in piece.split('\u{b}').enumerate() {
                if j > 0 {
                    at = self.line_break(at);
                }
                if !seg.is_empty() {
                    at = self.insert_plain(at, seg, rpr);
                }
            }
        }
        at
    }

    fn insert_plain(&mut self, at: At, seg: &str, rpr: Option<&Element>) -> At {
        let a = self.a.clone();
        let p = &mut self.paras[at.para];
        let n = seg.chars().count();
        let i = p.cut(at.ch);
        let is_run = |it: &Item| matches!(it, Item::Run { .. });
        let same = |it: &Item, r: &Element| matches!(it, Item::Run { rpr: Some(x), .. } if x == r);
        let pending = i < p.items.len() && p.items[i].len() == 0 && is_run(&p.items[i]);
        let spot: Option<(usize, bool)> = match rpr {
            None if pending => Some((i, true)),
            None if i > 0 && is_run(&p.items[i - 1]) => Some((i - 1, false)),
            None if i < p.items.len() && is_run(&p.items[i]) => Some((i, true)),
            Some(r) if i > 0 && same(&p.items[i - 1], r) => Some((i - 1, false)),
            Some(r) if i < p.items.len() && same(&p.items[i], r) => Some((i, true)),
            _ => None,
        };
        match spot {
            Some((k, front)) => {
                if let Item::Run { text, .. } = &mut p.items[k] {
                    if front {
                        text.insert_str(0, seg);
                    } else {
                        text.push_str(seg);
                    }
                }
            }
            None => {
                let want = rpr.cloned().or_else(|| p.rpr_at(at.ch));
                let r = want.map(|r| renamed(&r, "rPr")).unwrap_or_else(|| new_rpr(&a));
                p.items.insert(i, Item::Run { rpr: Some(r), text: seg.to_string() });
            }
        }
        p.touch();
        At::new(at.para, at.ch + n)
    }

    /// A new paragraph at `at` (the one after keeps the paragraph properties).
    pub fn split(&mut self, at: At) -> At {
        let at = self.clamp(at);
        let p = &mut self.paras[at.para];
        let i = p.cut(at.ch);
        let rest = p.items.split_off(i);
        let carried = p.rpr_at(at.ch).map(|r| renamed(&r, "endParaRPr"));
        let mut next = Para::empty(p.ppr.clone(), p.end.clone());
        next.items = rest;
        next.touch();
        if let Some(c) = carried {
            if p.items.is_empty() || p.end.is_none() {
                p.end = Some(c.clone());
            }
            if next.items.is_empty() {
                next.end = Some(c);
            }
        }
        p.touch();
        self.paras.insert(at.para + 1, next);
        At::new(at.para + 1, 0)
    }

    /// A line break at `at`.
    pub fn line_break(&mut self, at: At) -> At {
        let at = self.clamp(at);
        let p = &mut self.paras[at.para];
        let rpr = p.rpr_at(at.ch);
        let i = p.cut(at.ch);
        p.items.insert(i, Item::Br { rpr });
        p.touch();
        At::new(at.para, at.ch + 1)
    }

    /// Change the run properties of everything from `a` to `b`. A caret (`a == b`) leaves an empty run
    /// with the new properties there for the next typing (and an empty paragraph's end properties).
    pub fn format_runs(&mut self, a: At, b: At, f: &dyn Fn(&mut Element)) {
        let (s, e) = (self.clamp(a.min(b)), self.clamp(a.max(b)));
        let pre = self.a.clone();
        if s == e {
            let p = &mut self.paras[s.para];
            let mut r = p.rpr_at(s.ch).map(|r| renamed(&r, "rPr")).unwrap_or_else(|| new_rpr(&pre));
            f(&mut r);
            let i = p.cut(s.ch);
            let empty_here = i < p.items.len() && p.items[i].len() == 0 && matches!(p.items[i], Item::Run { .. });
            if empty_here {
                p.items[i] = Item::Run { rpr: Some(r), text: String::new() };
            } else {
                p.items.insert(i, Item::Run { rpr: Some(r), text: String::new() });
            }
            if p.len() == 0 {
                let end = p.end.get_or_insert_with(|| renamed(&new_rpr(&pre), "endParaRPr"));
                f(end);
            }
            p.touch();
            return;
        }
        for pi in s.para..=e.para {
            let p = &mut self.paras[pi];
            let from = if pi == s.para { s.ch } else { 0 };
            let to = if pi == e.para { e.ch } else { p.len() };
            let i = p.cut(from);
            let j = p.cut(to);
            for it in &mut p.items[i..j] {
                f(it.rpr_mut(&pre));
            }
            if from == 0 && to == p.len() {
                let end = p.end.get_or_insert_with(|| renamed(&new_rpr(&pre), "endParaRPr"));
                f(end);
            }
            p.touch();
        }
    }

    /// Change the paragraph properties of the paragraphs `a..=b`.
    pub fn format_paras(&mut self, a: usize, b: usize, f: &dyn Fn(&mut Element)) {
        let pre = self.a.clone();
        let (a, b) = (a.min(b), a.max(b).min(self.paras.len().saturating_sub(1)));
        for p in &mut self.paras[a..=b] {
            let ppr = p.ppr.get_or_insert_with(|| Element::new(format!("{pre}:pPr")));
            f(ppr);
            p.touch();
        }
    }

    /// The run properties at a place (for the ribbon's state).
    pub fn rpr_at(&self, at: At) -> Option<Element> {
        let at = self.clamp(at);
        self.paras.get(at.para).and_then(|p| p.rpr_at(at.ch))
    }

    /// The paragraphs and items between two places, as a body of its own (for a copy).
    pub fn slice(&self, a: At, b: At) -> Body {
        let mut copy = self.clone();
        let (s, e) = (copy.clamp(a.min(b)), copy.clamp(a.max(b)));
        // Drop everything after e, then everything before s.
        {
            let last = &mut copy.paras[e.para];
            let j = last.cut(e.ch);
            last.items.truncate(j);
            last.touch();
        }
        copy.paras.truncate(e.para + 1);
        {
            let first = &mut copy.paras[s.para];
            let i = first.cut(s.ch);
            first.items.drain(..i);
            first.touch();
        }
        copy.paras.drain(..s.para);
        copy
    }

    /// Insert another body's paragraphs at `at`, keeping their formatting; returns the place after.
    pub fn insert_body(&mut self, at: At, other: &Body) -> At {
        let mut at = self.clamp(at);
        for (k, op) in other.paras.iter().enumerate() {
            if k > 0 {
                at = self.split(at);
                if op.ppr.is_some() {
                    self.paras[at.para].ppr = op.ppr.clone();
                }
            }
            let p = &mut self.paras[at.para];
            let i = p.cut(at.ch);
            let mut n = 0;
            for (j, it) in op.items.iter().enumerate() {
                let mut it = it.clone();
                match &mut it {
                    Item::Run { rpr: Some(r), .. } | Item::Br { rpr: Some(r) } => *r = renamed(r, "rPr"),
                    _ => {}
                }
                n += it.len();
                p.items.insert(i + j, it);
            }
            p.touch();
            at = At::new(at.para, at.ch + n);
        }
        at
    }

    /// The characters of a paragraph as a vector (a break as `\u{b}`, a field as `\u{fffc}`).
    pub fn chars(&self, para: usize) -> Vec<char> {
        let mut v = Vec::new();
        if let Some(p) = self.paras.get(para) {
            for it in &p.items {
                match it {
                    Item::Run { text, .. } => v.extend(text.chars()),
                    Item::Br { .. } => v.push('\u{b}'),
                    Item::Fld { .. } => v.push('\u{fffc}'),
                }
            }
        }
        v
    }
}

/// The word boundaries around a place: the start of the word before and the end of the word after.
pub fn word_left(chars: &[char], at: usize) -> usize {
    let mut i = at.min(chars.len());
    while i > 0 && !chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    while i > 0 && chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    i
}

pub fn word_right(chars: &[char], at: usize) -> usize {
    let mut i = at.min(chars.len());
    while i < chars.len() && chars[i].is_alphanumeric() {
        i += 1;
    }
    while i < chars.len() && !chars[i].is_alphanumeric() {
        i += 1;
    }
    i
}

/// Put `child` into `el` in schema order: before the first existing child that `order` lists later,
/// replacing any existing child whose local name is in `replaces`.
pub fn put_ordered(el: &mut Element, child: Option<Element>, replaces: &[&str], order: &[&str]) {
    let idx = el.children.iter().position(|c| matches!(c, Node::Element(e) if replaces.contains(&e.local())));
    el.children.retain(|c| !matches!(c, Node::Element(e) if replaces.contains(&e.local())));
    let Some(child) = child else { return };
    let rank = |l: &str| order.iter().position(|o| *o == l);
    let mine = rank(child.local()).unwrap_or(order.len());
    let at = match idx {
        Some(i) => i.min(el.children.len()),
        None => el
            .children
            .iter()
            .position(|c| matches!(c, Node::Element(e) if rank(e.local()).is_some_and(|r| r > mine)))
            .unwrap_or_else(|| {
                // after the last known-earlier child, or at the end
                el.children.len()
            }),
    };
    el.children.insert(at, Node::Element(child));
}

/// Schema order of `a:rPr` children.
pub const RPR_ORDER: &[&str] = &[
    "ln", "noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill", "effectLst", "effectDag", "highlight", "uLnTx", "uLn", "uFillTx", "uFill", "latin", "ea", "cs", "sym",
    "hlinkClick", "hlinkMouseOver", "rtl", "extLst",
];
/// Schema order of `a:pPr` children.
pub const PPR_ORDER: &[&str] = &[
    "lnSpc", "spcBef", "spcAft", "buClrTx", "buClr", "buSzTx", "buSzPct", "buSzPts", "buFontTx", "buFont", "buNone", "buAutoNum", "buChar", "buBlip", "tabLst", "defRPr", "extLst",
];
/// Schema order of `p:spPr` children.
pub const SPPR_ORDER: &[&str] = &["xfrm", "custGeom", "prstGeom", "noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill", "ln", "effectLst", "effectDag", "scene3d", "sp3d", "extLst"];
/// Schema order of `a:bodyPr` children.
pub const BODYPR_ORDER: &[&str] = &["prstTxWarp", "noAutofit", "normAutofit", "spAutoFit", "scene3d", "sp3d", "flatTx", "extLst"];
/// Schema order of `a:tcPr` children.
pub const TCPR_ORDER: &[&str] = &["lnL", "lnR", "lnT", "lnB", "lnTlToBr", "lnBlToTr", "cell3D", "noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill", "headers", "extLst"];
/// Schema order of `a:ln` children.
pub const LN_ORDER: &[&str] = &["noFill", "solidFill", "gradFill", "pattFill", "prstDash", "custDash", "round", "bevel", "miter", "headEnd", "tailEnd", "extLst"];
/// The fill choices.
pub const FILLS: &[&str] = &["noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill"];

/// `<a:solidFill><a:srgbClr val=".."/></a:solidFill>` (with an alpha below 1).
pub fn solid_fill(a: &str, hex: &str, alpha: Option<f64>) -> Element {
    let mut c = Element::new(format!("{a}:srgbClr"));
    c.attrs.push(("val".into(), hex.trim_start_matches('#').to_ascii_uppercase()));
    if let Some(al) = alpha.filter(|v| *v < 1.0) {
        let mut m = Element::new(format!("{a}:alpha"));
        m.attrs.push(("val".into(), format!("{}", (al.clamp(0.0, 1.0) * 100000.0).round() as i64)));
        c.children.push(Node::Element(m));
    }
    let mut f = Element::new(format!("{a}:solidFill"));
    f.children.push(Node::Element(c));
    f
}

/// A colour argument: `RRGGBB` or `#RRGGBB`.
pub fn hex_arg(s: &str) -> Option<String> {
    let t = s.trim().trim_start_matches('#');
    (t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit())).then(|| t.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground::xml::{parse_dom, write_dom};

    fn body(xml: &str) -> (Element, Body) {
        let tx = parse_dom(format!(r#"<p:txBody xmlns:p="p" xmlns:a="a">{xml}</p:txBody>"#).as_bytes()).unwrap();
        let b = Body::read(&tx);
        (tx, b)
    }

    fn out(tx: &Element, b: &Body) -> String {
        let mut t = tx.clone();
        b.write_into(&mut t);
        String::from_utf8(write_dom(&t)).unwrap()
    }

    #[test]
    fn insert_split_delete_and_breaks() {
        let (tx, mut b) = body(r#"<a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr b="1"/><a:t>Hello</a:t></a:r><a:r><a:rPr/><a:t> world</a:t></a:r></a:p>"#);
        assert_eq!(b.text(), "Hello world");
        let at = b.insert(At::new(0, 5), ",", None);
        assert_eq!(at, At::new(0, 6));
        assert_eq!(b.text(), "Hello, world");
        let at = b.insert(At::new(0, 6), "\nnew", None);
        assert_eq!(at, At::new(1, 3));
        assert_eq!(b.text(), "Hello,\nnew world");
        let at = b.line_break(At::new(1, 3));
        assert_eq!(at, At::new(1, 4));
        assert_eq!(b.paras[1].len(), 10);
        let left = b.delete(At::new(0, 3), At::new(1, 4));
        assert_eq!(left, At::new(0, 3));
        assert_eq!(b.text(), "Hel world");
        let xml = out(&tx, &b);
        assert!(xml.contains(r#"<a:r><a:rPr b="1"/><a:t>Hel</a:t></a:r>"#), "{xml}");
        assert!(xml.contains(r#"<p:txBody xmlns:p="p" xmlns:a="a"><a:bodyPr/><a:lstStyle/><a:p>"#), "{xml}");
    }

    #[test]
    fn untouched_paragraphs_are_the_same_elements() {
        let (tx, mut b) = body(r#"<a:bodyPr/><a:p><a:r><a:t>one</a:t></a:r></a:p><a:p><a:r><a:t>two</a:t></a:r></a:p>"#);
        b.insert(At::new(1, 3), "!", None);
        let mut t = tx.clone();
        b.write_into(&mut t);
        assert_eq!(t.children_named("p").next(), tx.children_named("p").next());
    }

    #[test]
    fn formatting_a_range_and_a_caret() {
        let (tx, mut b) = body(r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>abcdef</a:t></a:r></a:p>"#);
        b.format_runs(At::new(0, 2), At::new(0, 4), &|r| r.set_attr("b", "1"));
        let xml = out(&tx, &b);
        assert!(xml.contains(r#"<a:t>ab</a:t></a:r><a:r><a:rPr lang="en-US" b="1"/><a:t>cd</a:t>"#), "{xml}");
        // A caret: the next typing takes the italic.
        b.format_runs(At::new(0, 6), At::new(0, 6), &|r| r.set_attr("i", "1"));
        b.insert(At::new(0, 6), "X", None);
        let xml = out(&tx, &b);
        assert!(xml.contains(r#"<a:rPr lang="en-US" i="1"/><a:t>X</a:t>"#), "{xml}");
        assert_eq!(b.text(), "abcdefX");
    }

    #[test]
    fn slices_and_bodies_paste_back() {
        let (_, b) = body(r#"<a:p><a:r><a:rPr b="1"/><a:t>bold</a:t></a:r><a:r><a:t>plain</a:t></a:r></a:p><a:p><a:r><a:t>second</a:t></a:r></a:p>"#);
        let s = b.slice(At::new(0, 2), At::new(1, 3));
        assert_eq!(s.text(), "ldplain\nsec");
        let (_, mut c) = body(r#"<a:p><a:r><a:t>XY</a:t></a:r></a:p>"#);
        let at = c.insert_body(At::new(0, 1), &s);
        assert_eq!(c.text(), "Xldplain\nsecY");
        assert_eq!(at, At::new(1, 3));
        assert_eq!(word_left(&c.chars(0), 5), 0);
        assert_eq!(word_right(&"ab cd".chars().collect::<Vec<_>>(), 0), 3);
    }

    #[test]
    fn ordered_children() {
        let mut rpr = parse_dom(br#"<a:rPr xmlns:a="a"><a:ln/><a:latin typeface="X"/></a:rPr>"#).unwrap();
        put_ordered(&mut rpr, Some(solid_fill("a", "ff0000", None)), FILLS, RPR_ORDER);
        let names: Vec<&str> = rpr.elements().map(|e| e.local()).collect();
        assert_eq!(names, vec!["ln", "solidFill", "latin"]);
        put_ordered(&mut rpr, None, &["latin"], RPR_ORDER);
        assert_eq!(rpr.elements().count(), 2);
        assert_eq!(hex_arg("#a0b1c2").as_deref(), Some("A0B1C2"));
        assert_eq!(hex_arg("zz"), None);
    }
}
