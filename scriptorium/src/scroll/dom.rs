//! The XML side of editing: surgical changes to `w:p` elements that keep everything the editor does
//! not understand (bookmarks, comment ranges, content controls, tracked changes, extension
//! elements) exactly where it was. New XML is written in WordprocessingML's own shape (`w:r`,
//! `w:t`, `w:rPr` children in schema order).
//!
//! Every function here works from the runs read out of the element *as it is now*
//! ([`super::model::read_runs`]), so several changes can follow one another inside one edit.

use super::model::{Run, RunItem, RunKind};
use super::pos::{atoms, AKind, Atom};
use crate::ground::xml::{Element, Node};

// ---------------------------------------------------------------------------------------------------
// Element helpers
// ---------------------------------------------------------------------------------------------------

/// The element at an element-only child path below `root`.
pub fn el_at<'e>(root: &'e Element, path: &[usize]) -> Option<&'e Element> {
    let mut cur = root;
    for &i in path {
        cur = cur.elements().nth(i)?;
    }
    Some(cur)
}

/// Mutable [`el_at`].
pub fn el_at_mut<'e>(root: &'e mut Element, path: &[usize]) -> Option<&'e mut Element> {
    let mut cur = root;
    for &i in path {
        cur = cur
            .children
            .iter_mut()
            .filter_map(|c| match c {
                Node::Element(e) => Some(e),
                _ => None,
            })
            .nth(i)?;
    }
    Some(cur)
}

/// The position in `children` of the `i`-th element child (`children.len()` past the last).
pub fn node_index(el: &Element, i: usize) -> usize {
    let mut seen = 0;
    for (k, c) in el.children.iter().enumerate() {
        if matches!(c, Node::Element(_)) {
            if seen == i {
                return k;
            }
            seen += 1;
        }
    }
    el.children.len()
}

pub fn element_count(el: &Element) -> usize {
    el.elements().count()
}

/// Insert nodes before the `i`-th element child (at the end when there is none).
pub fn insert_at(el: &mut Element, i: usize, nodes: Vec<Node>) {
    let k = if i >= element_count(el) { el.children.len() } else { node_index(el, i) };
    for (j, n) in nodes.into_iter().enumerate() {
        el.children.insert(k + j, n);
    }
}

/// Remove the `i`-th element child.
pub fn remove_at(el: &mut Element, i: usize) -> Option<Element> {
    let k = node_index(el, i);
    if k >= el.children.len() {
        return None;
    }
    match el.children.remove(k) {
        Node::Element(e) => Some(e),
        other => {
            el.children.insert(k, other);
            None
        }
    }
}

/// The `i`-th element child, mutable.
pub fn child_at_mut(el: &mut Element, i: usize) -> Option<&mut Element> {
    el_at_mut(el, &[i])
}

/// The WordprocessingML prefix in use (`"w:"`, or `""` for a default namespace).
pub fn prefix_of(el: &Element) -> String {
    match el.prefix() {
        Some(p) => format!("{p}:"),
        None => "w:".to_string(),
    }
}

/// `<w:local w:k="v" ...>` with the prefix `w` (e.g. `"w:"`).
pub fn wel(w: &str, local: &str, attrs: &[(&str, &str)]) -> Element {
    let mut e = Element::new(format!("{w}{local}"));
    for (k, v) in attrs {
        if k.contains(':') {
            e.attrs.push((k.to_string(), v.to_string()));
        } else {
            e.attrs.push((format!("{w}{k}"), v.to_string()));
        }
    }
    e
}

/// The attribute with local name `local`, whatever its prefix.
pub fn attr_local<'e>(el: &'e Element, local: &str) -> Option<&'e str> {
    super::props::attr(el, local)
}

/// Set an attribute by local name, keeping its existing prefix (or using `w`).
pub fn set_attr_local(el: &mut Element, w: &str, local: &str, value: &str) {
    for (k, v) in el.attrs.iter_mut() {
        if !k.starts_with("xmlns") && k.rsplit(':').next() == Some(local) {
            *v = value.to_string();
            return;
        }
    }
    el.attrs.push((format!("{w}{local}"), value.to_string()));
}

/// Remove an attribute by local name.
pub fn remove_attr_local(el: &mut Element, local: &str) {
    el.attrs.retain(|(k, _)| k.starts_with("xmlns") || k.rsplit(':').next() != Some(local));
}

/// The leading property children a container keeps first (`w:pPr`, `w:rPr`, `w:sdtPr`...).
fn is_property(e: &Element) -> bool {
    matches!(e.local(), "pPr" | "rPr" | "sdtPr" | "sdtEndPr" | "smartTagPr" | "customXmlPr" | "tblPr" | "tblGrid" | "trPr" | "tcPr")
}

/// How many property children open the element.
pub fn property_count(el: &Element) -> usize {
    el.elements().take_while(|e| is_property(e)).count()
}

/// The element's name and attributes (ids that must stay unique left out) with its property
/// children cloned: the second half of something split in two.
fn shell_clone(el: &Element) -> Element {
    let mut e = Element::new(el.name.clone());
    e.attrs = el
        .attrs
        .iter()
        .filter(|(k, _)| {
            let l = k.rsplit(':').next().unwrap_or(k);
            !matches!(l, "paraId" | "textId")
        })
        .cloned()
        .collect();
    for c in el.elements().take_while(|c| is_property(c)) {
        e.children.push(Node::Element(c.clone()));
    }
    e
}

// ---------------------------------------------------------------------------------------------------
// Schema order
// ---------------------------------------------------------------------------------------------------

pub const RPR_ORDER: &[&str] = &[
    "rStyle", "rFonts", "b", "bCs", "i", "iCs", "caps", "smallCaps", "strike", "dstrike", "outline", "shadow", "emboss", "imprint", "noProof",
    "snapToGrid", "vanish", "webHidden", "color", "spacing", "w", "kern", "position", "sz", "szCs", "highlight", "u", "effect", "bdr", "shd",
    "fitText", "vertAlign", "rtl", "cs", "em", "lang", "eastAsianLayout", "specVanish", "oMath",
];

pub const PPR_ORDER: &[&str] = &[
    "pStyle", "keepNext", "keepLines", "pageBreakBefore", "framePr", "widowControl", "numPr", "suppressLineNumbers", "pBdr", "shd", "tabs",
    "suppressAutoHyphens", "kinsoku", "wordWrap", "overflowPunct", "topLinePunct", "autoSpaceDE", "autoSpaceDN", "bidi", "adjustRightInd",
    "snapToGrid", "spacing", "ind", "contextualSpacing", "mirrorIndents", "suppressOverlap", "jc", "textDirection", "textAlignment",
    "textboxTightWrap", "outlineLvl", "divId", "cnfStyle", "rPr", "sectPr", "pPrChange",
];

pub const SECT_ORDER: &[&str] = &[
    "headerReference", "footerReference", "footnotePr", "endnotePr", "type", "pgSz", "pgMar", "paperSrc", "pgBorders", "lnNumType",
    "pgNumType", "cols", "formProt", "vAlign", "noEndnote", "titlePg", "textDirection", "bidi", "rtlGutter", "docGrid", "printerSettings",
    "sectPrChange",
];

pub const TBLPR_ORDER: &[&str] = &[
    "tblStyle", "tblpPr", "tblOverlap", "bidiVisual", "tblStyleRowBandSize", "tblStyleColBandSize", "tblW", "jc", "tblCellSpacing", "tblInd",
    "tblBorders", "shd", "tblLayout", "tblCellMar", "tblLook", "tblCaption", "tblDescription", "tblPrChange",
];

pub const TCPR_ORDER: &[&str] =
    &["cnfStyle", "tcW", "gridSpan", "hMerge", "vMerge", "tcBorders", "shd", "noWrap", "tcMar", "textDirection", "tcFitText", "vAlign", "hideMark"];

pub const TRPR_ORDER: &[&str] =
    &["cnfStyle", "divId", "gridBefore", "gridAfter", "wBefore", "wAfter", "cantSplit", "trHeight", "tblHeader", "tblCellSpacing", "jc", "hidden"];

pub const BORDER_ORDER: &[&str] = &["top", "left", "start", "bottom", "right", "end", "insideH", "insideV", "tl2br", "tr2bl"];

/// Put `new` (or nothing) in place of the child named `local`, at its schema position by `order`.
pub fn set_child(el: &mut Element, local: &str, new: Option<Element>, order: &[&str]) {
    let existing = el.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == local));
    match (existing, new) {
        (Some(k), Some(n)) => el.children[k] = Node::Element(n),
        (Some(k), None) => {
            el.children.remove(k);
        }
        (None, Some(n)) => {
            let rank = order.iter().position(|o| *o == local).unwrap_or(order.len());
            let at = el
                .children
                .iter()
                .position(|c| match c {
                    Node::Element(e) => order.iter().position(|o| *o == e.local()).is_some_and(|r| r > rank),
                    _ => false,
                })
                .unwrap_or(el.children.len());
            el.children.insert(at, Node::Element(n));
        }
        (None, None) => {}
    }
}

/// The child named `local`, created empty at its schema position when missing.
pub fn ensure_child<'e>(el: &'e mut Element, w: &str, local: &str, order: &[&str]) -> &'e mut Element {
    if el.child(local).is_none() {
        set_child(el, local, Some(wel(w, local, &[])), order);
    }
    el.child_mut(local).expect("just made")
}

/// A paragraph's `w:pPr`, created when missing.
pub fn ppr_mut<'e>(p: &'e mut Element, w: &str) -> &'e mut Element {
    if p.child("pPr").is_none() {
        p.children.insert(0, Node::Element(wel(w, "pPr", &[])));
    }
    p.child_mut("pPr").expect("just made")
}

/// A run's `w:rPr`, created when missing.
pub fn rpr_mut<'e>(r: &'e mut Element, w: &str) -> &'e mut Element {
    if r.child("rPr").is_none() {
        r.children.insert(0, Node::Element(wel(w, "rPr", &[])));
    }
    r.child_mut("rPr").expect("just made")
}

/// The run properties a new run takes from the paragraph mark (`w:pPr/w:rPr`), without the
/// revision marks only a paragraph mark may carry.
pub fn mark_rpr(p: &Element) -> Option<Element> {
    let mut r = p.child("pPr")?.child("rPr")?.clone();
    r.children.retain(|c| !matches!(c, Node::Element(e) if matches!(e.local(), "ins" | "del" | "moveFrom" | "moveTo" | "rPrChange")));
    if r.children.is_empty() {
        None
    } else {
        Some(r)
    }
}

// ---------------------------------------------------------------------------------------------------
// Text content
// ---------------------------------------------------------------------------------------------------

/// Keep `xml:space="preserve"` on a `w:t` whose spaces would otherwise be lost.
pub fn fix_space(t: &mut Element) {
    let text = t.text();
    let needs = text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) || text.contains("  ");
    if needs && t.attr("xml:space").is_none() {
        t.attrs.push(("xml:space".into(), "preserve".into()));
    }
}

fn set_text(t: &mut Element, s: String) {
    t.children = vec![Node::Text(s)];
    fix_space(t);
}

fn new_t(w: &str, s: &str) -> Element {
    let mut t = wel(w, "t", &[]);
    set_text(&mut t, s.to_string());
    t
}

/// The run content for a piece of typed text: `w:t` for characters, `w:tab`, `w:br` (a line break
/// for `\n`, `\u{b}` and `\u{2028}`), `w:br w:type="page"` for `\u{c}`.
pub fn content_nodes(w: &str, text: &str) -> Vec<Node> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut Vec<Node>| {
        if !buf.is_empty() {
            out.push(Node::Element(new_t(w, buf)));
            buf.clear();
        }
    };
    for ch in text.chars() {
        match ch {
            '\t' => {
                flush(&mut buf, &mut out);
                out.push(Node::Element(wel(w, "tab", &[])));
            }
            '\n' | '\u{b}' | '\u{2028}' => {
                flush(&mut buf, &mut out);
                out.push(Node::Element(wel(w, "br", &[])));
            }
            '\u{c}' => {
                flush(&mut buf, &mut out);
                out.push(Node::Element(wel(w, "br", &[("type", "page")])));
            }
            '\r' => {}
            c => buf.push(c),
        }
    }
    flush(&mut buf, &mut out);
    out
}

/// How many editing characters [`content_nodes`] makes of `text`.
pub fn content_len(text: &str) -> usize {
    text.chars().filter(|c| *c != '\r').count()
}

/// A new `w:r` with the given properties and content.
pub fn new_run(w: &str, rpr: Option<Element>, content: Vec<Node>) -> Element {
    let mut r = wel(w, "r", &[]);
    if let Some(p) = rpr {
        r.children.push(Node::Element(p));
    }
    r.children.extend(content);
    r
}

// ---------------------------------------------------------------------------------------------------
// Points in a paragraph
// ---------------------------------------------------------------------------------------------------

/// Where something goes in a paragraph: before element `before` of the container at `path` (from
/// the `w:p`), after splitting the `w:t` at element `t` at character `sub` when given. A container
/// that is a `w:r` takes run content; any other container takes runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub path: Vec<usize>,
    pub before: usize,
    pub split: Option<(usize, usize)>,
    /// A `w:t` element of the run that typed text may simply be appended to.
    pub append: Option<usize>,
    pub in_run: bool,
    /// The run whose properties new runs copy.
    pub rpr_from: Option<usize>,
}

fn item_len(it: &RunItem) -> usize {
    super::layout::item_atoms(it)
}

/// Is the field atom a complex field (`w:fldChar`)?
fn complex_field(r: &Run) -> bool {
    r.kind == RunKind::R
}

/// The `w:fldChar` types of a run's direct children, with their element indexes.
fn field_chars(run: &Element) -> Vec<(usize, String)> {
    run.elements()
        .enumerate()
        .filter(|(_, e)| e.local() == "fldChar")
        .map(|(k, e)| (k, attr_local(e, "fldCharType").unwrap_or("").trim().to_string()))
        .collect()
}

/// Index (in container `c`) of the run holding the `end` that closes the field whose `separate` (or
/// `end`) is element `elem` of run `from`.
fn field_end(c: &Element, from: usize, elem: usize) -> usize {
    let mut depth = 0i32;
    for (i, e) in c.elements().enumerate().skip(from) {
        for (k, t) in field_chars(e) {
            if i == from && k < elem {
                continue;
            }
            match t.as_str() {
                "begin" => depth += 1,
                "end" => {
                    if depth == 0 {
                        return i;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    element_count(c).saturating_sub(1).max(from)
}

/// Index (in container `c`) of the run holding the `begin` that opens the field whose `separate` (or
/// `end`) is element `elem` of run `from`.
fn field_begin(c: &Element, from: usize, elem: usize) -> usize {
    let els: Vec<&Element> = c.elements().collect();
    if els.is_empty() {
        return 0;
    }
    let mut depth = 0i32;
    let mut i = from.min(els.len() - 1);
    loop {
        for (k, t) in field_chars(els[i]).into_iter().rev() {
            if i == from && k >= elem {
                continue;
            }
            match t.as_str() {
                "end" => depth += 1,
                "begin" => {
                    if depth == 0 {
                        return i;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        if i == 0 {
            return 0;
        }
        i -= 1;
    }
}

/// Where text typed at character `o` goes: into the run of the character before it when that is
/// plain run content (so it takes that run's formatting), else before the character after it, else
/// as a new run beside its neighbours.
pub fn point_at(p: &Element, runs: &[Run], o: usize) -> Point {
    let ats = atoms(runs);
    let o = o.min(ats.len());
    let simple = |a: &Atom| runs[a.run].kind == RunKind::R && a.kind != AKind::Field;
    if o > 0 {
        let l = ats[o - 1];
        if simple(&l) {
            let r = &runs[l.run];
            let e = r.item_src[l.item];
            if l.kind == AKind::Text && is_t(p, &r.src, e) {
                let n = item_len(&r.items[l.item]);
                if l.sub + 1 < n {
                    return Point { path: r.src.clone(), before: e, split: Some((e, l.sub + 1)), append: None, in_run: true, rpr_from: Some(l.run) };
                }
                return Point { path: r.src.clone(), before: e + 1, split: None, append: Some(e), in_run: true, rpr_from: Some(l.run) };
            }
            return Point { path: r.src.clone(), before: e + 1, split: None, append: None, in_run: true, rpr_from: Some(l.run) };
        }
    }
    if o < ats.len() {
        let rt = ats[o];
        if simple(&rt) {
            let r = &runs[rt.run];
            let e = r.item_src[rt.item];
            let split = if rt.kind == AKind::Text && rt.sub > 0 { Some((e, rt.sub)) } else { None };
            return Point { path: r.src.clone(), before: e, split, append: None, in_run: true, rpr_from: Some(rt.run) };
        }
    }
    // A run of its own beside the neighbours.
    if o > 0 {
        let l = ats[o - 1];
        let r = &runs[l.run];
        let (parent, last) = r.src.split_at(r.src.len() - 1);
        let mut before = last[0] + 1;
        if l.kind == AKind::Field && complex_field(r) {
            if let Some(c) = el_at(p, parent) {
                before = field_end(c, last[0], r.item_src[l.item]) + 1;
            }
        }
        return Point { path: parent.to_vec(), before, split: None, append: None, in_run: false, rpr_from: Some(l.run) };
    }
    if let Some(rt) = ats.first() {
        let r = &runs[rt.run];
        let (parent, last) = r.src.split_at(r.src.len() - 1);
        let mut before = last[0];
        if rt.kind == AKind::Field && complex_field(r) {
            if let Some(c) = el_at(p, parent) {
                before = field_begin(c, last[0], r.item_src[rt.item]);
            }
        }
        return Point { path: parent.to_vec(), before, split: None, append: None, in_run: false, rpr_from: Some(rt.run) };
    }
    Point { path: Vec::new(), before: element_count(p), split: None, append: None, in_run: false, rpr_from: None }
}

fn is_t(p: &Element, run_path: &[usize], e: usize) -> bool {
    el_at(p, run_path).and_then(|r| r.elements().nth(e)).is_some_and(|t| t.local() == "t")
}

/// The `w:rPr` of run `ri` as it is in the XML (a field simple's first run's).
pub fn run_rpr(p: &Element, runs: &[Run], ri: usize) -> Option<Element> {
    let r = runs.get(ri)?;
    let el = el_at(p, &r.src)?;
    match r.kind {
        RunKind::R => el.child("rPr").cloned(),
        RunKind::FldSimple => el.child("r").and_then(|x| x.child("rPr")).cloned(),
        RunKind::Math => None,
    }
}

/// Split the `w:t` at element `e` of `run` at character `sub`; the second half follows it.
fn split_t(run: &mut Element, e: usize, sub: usize) {
    let Some(t) = child_at_mut(run, e) else { return };
    if t.local() != "t" {
        return;
    }
    let text = t.text();
    let cut = text.char_indices().nth(sub).map(|(b, _)| b).unwrap_or(text.len());
    let (a, b) = (text[..cut].to_string(), text[cut..].to_string());
    let mut second = t.clone();
    set_text(t, a);
    set_text(&mut second, b);
    insert_at(run, e + 1, vec![Node::Element(second)]);
}

/// Insert typed text at character `o`; `rpr` (when given) formats it as a run of its own. Returns
/// the number of characters inserted.
pub fn insert_text(p: &mut Element, runs: &[Run], o: usize, text: &str, rpr: Option<Element>) -> usize {
    let w = prefix_of(p);
    let n = content_len(text);
    if n == 0 {
        return 0;
    }
    let pt = point_at(p, runs, o);
    if rpr.is_none() && pt.in_run {
        let pure = !text.chars().any(|c| matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\u{2028}'));
        let Some(run) = el_at_mut(p, &pt.path) else { return 0 };
        if pure {
            if let Some((e, sub)) = pt.split {
                if let Some(t) = child_at_mut(run, e) {
                    let cur = t.text();
                    let cut = cur.char_indices().nth(sub).map(|(b, _)| b).unwrap_or(cur.len());
                    let s = format!("{}{}{}", &cur[..cut], text.replace('\r', ""), &cur[cut..]);
                    set_text(t, s);
                    return n;
                }
            }
            if let Some(e) = pt.append {
                if let Some(t) = child_at_mut(run, e) {
                    let s = format!("{}{}", t.text(), text.replace('\r', ""));
                    set_text(t, s);
                    return n;
                }
            }
        }
        let mut before = pt.before;
        if let Some((e, sub)) = pt.split {
            split_t(run, e, sub);
            before = e + 1;
        }
        let before = before.max(property_count(run));
        insert_at(run, before, content_nodes(&w, text));
        return n;
    }
    // A run of its own: at the paragraph level of the point.
    let template = rpr.or_else(|| pt.rpr_from.and_then(|ri| run_rpr(p, runs, ri))).or_else(|| mark_rpr(p));
    let run = new_run(&w, template, content_nodes(&w, text));
    let (path, before) = level_point(p, runs, o, &pt);
    let Some(c) = el_at_mut(p, &path) else { return 0 };
    let before = before.max(property_count(c));
    insert_at(c, before, vec![Node::Element(run)]);
    n
}

/// The run-level place (a container that holds runs, and an index in it) for character `o`,
/// splitting the run the point falls inside.
pub fn level_point(p: &mut Element, _runs: &[Run], _o: usize, pt: &Point) -> (Vec<usize>, usize) {
    if !pt.in_run {
        return (pt.path.clone(), pt.before);
    }
    let (parent, last) = pt.path.split_at(pt.path.len() - 1);
    let ri = last[0];
    let Some(run) = el_at_mut(p, &pt.path) else { return (parent.to_vec(), ri + 1) };
    let mut before = pt.before;
    if let Some((e, sub)) = pt.split {
        split_t(run, e, sub);
        before = e + 1;
    }
    let props = property_count(run);
    let total = element_count(run);
    if before <= props {
        return (parent.to_vec(), ri);
    }
    if before >= total {
        return (parent.to_vec(), ri + 1);
    }
    let right = split_tree(run, &[], before);
    let Some(c) = el_at_mut(p, parent) else { return (parent.to_vec(), ri + 1) };
    insert_at(c, ri + 1, vec![Node::Element(right)]);
    (parent.to_vec(), ri + 1)
}

/// The run-level place for character `o` (see [`level_point`]).
pub fn level_at(p: &mut Element, runs: &[Run], o: usize) -> (Vec<usize>, usize) {
    let pt = point_at(p, runs, o);
    level_point(p, runs, o, &pt)
}

/// Split `el` at element `before` of the descendant container at `path`: everything from there on
/// moves into the returned copy (its name, attributes and property children cloned).
pub fn split_tree(el: &mut Element, path: &[usize], before: usize) -> Element {
    let mut right = shell_clone(el);
    let moved_from = if path.is_empty() {
        before.max(property_count(el))
    } else {
        let ci = path[0];
        if let Some(child) = child_at_mut(el, ci) {
            let r = split_tree(child, &path[1..], before);
            right.children.push(Node::Element(r));
        }
        ci + 1
    };
    let k = if moved_from >= element_count(el) { el.children.len() } else { node_index(el, moved_from) };
    let tail: Vec<Node> = el.children.drain(k..).collect();
    right.children.extend(tail);
    right
}

/// Drop `w:r` elements left with nothing but their properties.
pub fn prune_empty_runs(el: &mut Element) {
    el.children.retain(|c| match c {
        Node::Element(e) if e.local() == "r" => e.elements().any(|x| x.local() != "rPr") || e.children.iter().any(|n| matches!(n, Node::Raw(_))),
        _ => true,
    });
    for c in el.children.iter_mut() {
        if let Node::Element(e) = c {
            if matches!(e.local(), "hyperlink" | "smartTag" | "sdt" | "sdtContent" | "ins" | "customXml" | "dir" | "bdo") {
                prune_empty_runs(e);
            }
        }
    }
}

/// Split paragraph `p` at character `o`: `p` keeps what is before, the returned paragraph (its
/// properties cloned; the section break, if any, moves with it) holds the rest.
pub fn split_para(p: &mut Element, runs: &[Run], o: usize) -> Element {
    let n = super::pos::atom_count(runs);
    let (path, before) = if o == 0 {
        (Vec::new(), property_count(p))
    } else if o >= n {
        (Vec::new(), element_count(p))
    } else {
        let pt = point_at(p, runs, o);
        if pt.in_run {
            let mut before = pt.before;
            if let Some((e, sub)) = pt.split {
                if let Some(run) = el_at_mut(p, &pt.path) {
                    split_t(run, e, sub);
                }
                before = e + 1;
            }
            (pt.path.clone(), before)
        } else {
            (pt.path.clone(), pt.before)
        }
    };
    // At the very end, keep trailing range ends (bookmarkEnd...) with the first paragraph.
    let mut right = split_tree(p, &path, before);
    prune_empty_runs(p);
    prune_empty_runs(&mut right);
    // The section break ends the second paragraph now.
    if let Some(ppr) = p.child_mut("pPr") {
        set_child(ppr, "sectPr", None, PPR_ORDER);
    }
    right
}

/// Join paragraph `second` onto `first`: its content follows, and the joined paragraph keeps the
/// first's properties (the second's when the first is empty), with the second's section break.
pub fn join_paras(first: &mut Element, second: Element, first_empty: bool) {
    let sect = second.child("pPr").and_then(|p| p.child("sectPr")).cloned();
    if first_empty {
        if let Some(pp) = second.child("pPr") {
            match first.child_mut("pPr") {
                Some(x) => *x = pp.clone(),
                None => first.children.insert(0, Node::Element(pp.clone())),
            }
        }
    }
    if let Some(ppr) = first.child_mut("pPr") {
        set_child(ppr, "sectPr", sect.clone(), PPR_ORDER);
    } else if let Some(s) = sect {
        let w = prefix_of(first);
        let mut ppr = wel(&w, "pPr", &[]);
        ppr.children.push(Node::Element(s));
        first.children.insert(0, Node::Element(ppr));
    }
    let mut skip_ppr = true;
    for c in second.children {
        if skip_ppr {
            if let Node::Element(e) = &c {
                if e.local() == "pPr" {
                    skip_ppr = false;
                    continue;
                }
            }
        }
        first.children.push(c);
    }
}

// ---------------------------------------------------------------------------------------------------
// Deleting and formatting characters
// ---------------------------------------------------------------------------------------------------

/// The atom range each item of each run covers: `(run, item) -> start`.
fn run_ranges(runs: &[Run]) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(runs.len());
    let mut at = 0;
    for r in runs {
        let n: usize = r.items.iter().map(item_len).sum();
        out.push((at, at + n));
        at += n;
    }
    out
}

/// Delete characters `a..b` of the paragraph.
pub fn delete_range(p: &mut Element, runs: &[Run], a: usize, b: usize) {
    if a >= b {
        return;
    }
    let ranges = run_ranges(runs);
    for ri in (0..runs.len()).rev() {
        let (ra, rb) = ranges[ri];
        if rb <= a || ra >= b || ra == rb {
            continue;
        }
        let r = &runs[ri];
        let (parent, last) = r.src.split_at(r.src.len() - 1);
        let idx = last[0];
        match r.kind {
            RunKind::FldSimple | RunKind::Math => {
                if let Some(c) = el_at_mut(p, parent) {
                    remove_at(c, idx);
                }
                continue;
            }
            RunKind::R => {}
        }
        // Items in reverse.
        let mut starts = Vec::new();
        let mut at = ra;
        for it in &r.items {
            starts.push(at);
            at += item_len(it);
        }
        let mut removed_field = false;
        for ii in (0..r.items.len()).rev() {
            let ia = starts[ii];
            let ib = ia + item_len(&r.items[ii]);
            if ib <= a || ia >= b {
                continue;
            }
            let e = r.item_src[ii];
            match &r.items[ii] {
                RunItem::Field(_) => {
                    // The whole field goes: from its begin to its end.
                    if let Some(c) = el_at_mut(p, parent) {
                        let end = field_end(c, idx, e);
                        let begin = field_begin(c, idx, e);
                        for k in (begin..=end.max(begin)).rev() {
                            remove_at(c, k);
                        }
                    }
                    removed_field = true;
                    break;
                }
                RunItem::Text(t) => {
                    let Some(run) = el_at_mut(p, &r.src) else { continue };
                    let Some(te) = child_at_mut(run, e) else { continue };
                    if te.local() != "t" {
                        remove_at(run, e);
                        continue;
                    }
                    let s = a.max(ia) - ia;
                    let f = b.min(ib) - ia;
                    let kept: String = t.chars().enumerate().filter(|(k, _)| *k < s || *k >= f).map(|(_, c)| c).collect();
                    if kept.is_empty() {
                        remove_at(run, e);
                    } else {
                        set_text(te, kept);
                    }
                }
                _ => {
                    if let Some(run) = el_at_mut(p, &r.src) {
                        remove_at(run, e);
                    }
                }
            }
        }
        if removed_field {
            continue;
        }
        let empty = el_at(p, &r.src).is_some_and(|run| !run.elements().any(|x| x.local() != "rPr"));
        if empty {
            if let Some(c) = el_at_mut(p, parent) {
                remove_at(c, idx);
            }
        }
    }
}

/// Apply `edit` to the run properties of characters `a..b` (runs split at the edges). The
/// paragraph mark takes it too when the whole paragraph is covered.
pub fn format_range(p: &mut Element, runs: &[Run], a: usize, b: usize, edit: &dyn Fn(&mut Element)) {
    let w = prefix_of(p);
    let ranges = run_ranges(runs);
    let total = ranges.last().map(|r| r.1).unwrap_or(0);
    for ri in (0..runs.len()).rev() {
        let (ra, rb) = ranges[ri];
        if rb <= a || ra >= b || ra == rb {
            continue;
        }
        let r = &runs[ri];
        match r.kind {
            RunKind::Math => continue,
            RunKind::FldSimple => {
                if let Some(f) = el_at_mut(p, &r.src) {
                    for c in f.children.iter_mut() {
                        if let Node::Element(run) = c {
                            if run.local() == "r" {
                                edit(rpr_mut(run, &w));
                            }
                        }
                    }
                }
                continue;
            }
            RunKind::R => {}
        }
        let s = a.max(ra) - ra;
        let e = b.min(rb) - ra;
        let (parent, last) = r.src.split_at(r.src.len() - 1);
        let idx = last[0];
        let Some(run) = el_at(p, &r.src) else { continue };
        if s == 0 && e == rb - ra {
            if let Some(run) = el_at_mut(p, &r.src) {
                edit(rpr_mut(run, &w));
                tidy_rpr(run);
            }
            continue;
        }
        let pieces = split_run_by(run, r, s, e);
        let Some(c) = el_at_mut(p, parent) else { continue };
        remove_at(c, idx);
        let mut nodes = Vec::new();
        for (k, mut piece) in pieces.into_iter().enumerate() {
            if k == 1 {
                edit(rpr_mut(&mut piece, &w));
                tidy_rpr(&mut piece);
            }
            if piece.elements().any(|x| x.local() != "rPr") {
                nodes.push(Node::Element(piece));
            }
        }
        insert_at(c, idx, nodes);
    }
    if a == 0 && b >= total {
        let ppr = ppr_mut(p, &w);
        let mut rpr = ppr.child("rPr").cloned().unwrap_or_else(|| wel(&w, "rPr", &[]));
        edit(&mut rpr);
        let keep = !rpr.children.is_empty();
        set_child(ppr, "rPr", if keep { Some(rpr) } else { None }, PPR_ORDER);
        if ppr.children.is_empty() && ppr.attrs.is_empty() {
            p.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "pPr"));
        }
    }
}

/// An empty `w:rPr` left behind is removed.
fn tidy_rpr(run: &mut Element) {
    run.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "rPr" && e.children.is_empty() && e.attrs.is_empty()));
}

/// Cut a run in three by its characters `s..e` (run-local): before, inside, after (each a whole
/// `w:r` with the same properties; a piece may be left with nothing but them).
fn split_run_by(run: &Element, r: &Run, s: usize, e: usize) -> [Element; 3] {
    let mut parts = [shell_clone(run), shell_clone(run), shell_clone(run)];
    let mut item_at_elem: Vec<Option<(usize, usize)>> = vec![None; element_count(run)];
    let mut at = 0;
    for (ii, it) in r.items.iter().enumerate() {
        let n = item_len(it);
        if let Some(slot) = item_at_elem.get_mut(r.item_src[ii]) {
            *slot = Some((at, at + n));
        }
        at += n;
    }
    let mut side = 0usize;
    let mut ei = 0usize;
    for c in &run.children {
        let Node::Element(el) = c else {
            parts[side].children.push(c.clone());
            continue;
        };
        let this = ei;
        ei += 1;
        if is_property(el) {
            continue;
        }
        match item_at_elem.get(this).copied().flatten() {
            Some((ia, ib)) => {
                if ib <= s {
                    side = 0;
                    parts[0].children.push(c.clone());
                } else if ia >= e {
                    side = 2;
                    parts[2].children.push(c.clone());
                } else if el.local() == "t" && (ia < s || ib > e) {
                    let text: Vec<char> = el.text().chars().collect();
                    let cut = |x: usize, y: usize| -> String { text[x.min(text.len())..y.min(text.len())].iter().collect() };
                    let (x0, x1) = (s.saturating_sub(ia), (e - ia).min(text.len()));
                    for (k, piece) in [cut(0, x0), cut(x0, x1), cut(x1, text.len())].into_iter().enumerate() {
                        if !piece.is_empty() {
                            let mut t = el.clone();
                            set_text(&mut t, piece);
                            parts[k].children.push(Node::Element(t));
                        }
                    }
                    side = if ib > e { 2 } else { 1 };
                } else {
                    side = 1;
                    parts[1].children.push(c.clone());
                }
            }
            None => parts[side].children.push(c.clone()),
        }
    }
    parts
}

/// The XML of characters `a..b` as runs (for the clipboard): each run's `w:rPr` and its text.
pub fn runs_for_copy(p: &Element, runs: &[Run], a: usize, b: usize) -> Vec<(Option<Element>, String)> {
    let ranges = run_ranges(runs);
    let mut out = Vec::new();
    for (ri, r) in runs.iter().enumerate() {
        let (ra, rb) = ranges[ri];
        if rb <= a || ra >= b {
            continue;
        }
        let mut text = String::new();
        let mut at = ra;
        for it in &r.items {
            let n = item_len(it);
            let (ia, ib) = (at, at + n);
            at = ib;
            if ib <= a || ia >= b {
                continue;
            }
            match it {
                RunItem::Text(t) => {
                    let s = a.max(ia) - ia;
                    let f = b.min(ib) - ia;
                    text.extend(t.chars().skip(s).take(f - s));
                }
                RunItem::Tab => text.push('\t'),
                RunItem::Break => text.push('\u{b}'),
                RunItem::PageBreak => text.push('\u{c}'),
                RunItem::Sym(c, _) => text.push(*c),
                RunItem::Pic(_) | RunItem::Field(_) => {}
            }
        }
        if text.is_empty() {
            continue;
        }
        let rpr = if r.kind == RunKind::Math { None } else { run_rpr(p, runs, ri) };
        out.push((rpr, text));
    }
    out
}

/// The child named `local`, created empty as the element's first child when missing (`w:tcPr`,
/// `w:tblPr`, `w:trPr` open their element).
pub fn first_child_mut<'e>(el: &'e mut Element, w: &str, local: &str) -> &'e mut Element {
    if el.child(local).is_none() {
        el.children.insert(0, Node::Element(wel(w, local, &[])));
    }
    el.child_mut(local).expect("just made")
}
