//! Scroll's writing commands: typing, deleting, Enter and Tab, character and paragraph formatting,
//! styles, lists, find and replace, paste. Each is an ordinary bus command carrying the selection
//! it acts on (`"sel"`), edits the package's XML surgically ([`super::dom`]) and answers its undo as
//! one `scroll.restore` that puts back exactly the XML it replaced (top-level body blocks for the
//! main part, the whole part for headers, footers, styles and numbering).
//!
//! After every command the model is read again from the package and laid out, and the pages whose
//! display changed are reported touched.

use super::dom::{self, el_at, el_at_mut, prefix_of, set_child, wel, PPR_ORDER, RPR_ORDER};
use super::model::{self, Block, FlatPara, Para, Run, RunKind};
use super::pos::{self, atom_count, atoms, Flow, SPos, Span};
use super::props::{ParaProps, RunProps};
use super::ScrollDoc;
use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command};
use crate::ground::display::DisplayList;
use crate::ground::edit::Selection;
use crate::ground::opc::{self, Package};
use crate::ground::xml::{self, Element, Event, Node, Reader};

// ---------------------------------------------------------------------------------------------------
// Finding paragraphs
// ---------------------------------------------------------------------------------------------------

/// A paragraph found by flow and index: its part, its XML path there and the model's reading.
pub(crate) struct ParaRef<'a> {
    pub part: String,
    pub flat: &'a FlatPara,
    pub para: &'a Para,
}

fn walk_para<'a>(blocks: &'a [Block], pred: &dyn Fn(&Para) -> bool) -> Option<&'a Para> {
    for b in blocks {
        match b {
            Block::Para(p) if pred(p) => return Some(p),
            Block::Para(_) => {}
            Block::Table(t) => {
                for r in &t.rows {
                    for c in &r.cells {
                        if let Some(p) = walk_para(&c.blocks, pred) {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Everything an edit closure may read while the part's XML is being changed.
pub(crate) struct Cx<'a> {
    pub pkg: &'a Package,
    pub model: &'a model::Model,
    pub part: &'a str,
}

impl Cx<'_> {
    /// The runs of a paragraph element as it is now.
    pub fn runs(&self, p: &Element, fields_in: &[model::FieldState]) -> Vec<Run> {
        model::read_runs(self.pkg, self.part, self.model, p, fields_in)
    }
}

/// The parts an edit changed, as they were before it.
#[derive(Default)]
pub(crate) struct Tx {
    parts: Vec<(String, Vec<u8>)>,
    /// Something outside the snapshotted parts changed (a part or relationship was added).
    pub dirty: bool,
}

impl ScrollDoc {
    /// The part a flow lives in.
    pub(crate) fn part_name(&self, flow: &Flow) -> String {
        match flow {
            Flow::Body => self.main.clone(),
            Flow::Part(p) => p.clone(),
        }
    }

    /// How many paragraphs a flow has.
    pub(crate) fn flow_len(&self, flow: &Flow) -> usize {
        match flow {
            Flow::Body => self.model.flat.len(),
            Flow::Part(p) => self.model.hflat.get(p).map(|v| v.len()).unwrap_or(0),
        }
    }

    pub(crate) fn para_ref(&self, flow: &Flow, p: usize) -> Option<ParaRef<'_>> {
        match flow {
            Flow::Body => {
                let flat = self.model.flat.get(p)?;
                let para = walk_para(&self.model.body, &|x| x.flat == p)?;
                Some(ParaRef { part: self.main.clone(), flat, para })
            }
            Flow::Part(name) => {
                let flat = self.model.hflat.get(name)?.get(p)?;
                let para = walk_para(self.model.headers.get(name)?, &|x| x.hidx == p)?;
                Some(ParaRef { part: name.clone(), flat, para })
            }
        }
    }

    /// A paragraph's characters.
    pub(crate) fn para_atoms(&self, flow: &Flow, p: usize) -> Vec<pos::Atom> {
        self.para_ref(flow, p).map(|r| atoms(&r.para.runs)).unwrap_or_default()
    }

    pub(crate) fn para_chars(&self, flow: &Flow, p: usize) -> Vec<char> {
        self.para_atoms(flow, p).iter().map(|a| a.ch).collect()
    }

    pub(crate) fn para_len(&self, flow: &Flow, p: usize) -> usize {
        self.para_ref(flow, p).map(|r| atom_count(&r.para.runs)).unwrap_or(0)
    }

    /// A place made valid (a paragraph that exists, an offset inside it; an unknown part falls back to
    /// the body's start).
    pub(crate) fn clamp(&self, p: &SPos) -> SPos {
        let mut q = p.clone();
        let n = self.flow_len(&q.flow);
        if n == 0 {
            if q.flow != Flow::Body {
                return SPos::body(0, 0);
            }
            return SPos::body(0, 0);
        }
        q.p = q.p.min(n - 1);
        q.o = q.o.min(self.para_len(&q.flow, q.p));
        q
    }

    /// The selection a command carries, in reading order and made valid.
    pub(crate) fn sel_span(&self, cmd: &Command) -> Result<Span, BusError> {
        let sel = cmd
            .args
            .get("sel")
            .and_then(Selection::from_json)
            .ok_or_else(|| BusError::bad_args(&cmd.name, "takes \"sel\": { \"anchor\": pos, \"focus\": pos }"))?;
        let a = SPos::from_pos(&sel.anchor).ok_or_else(|| BusError::bad_args(&cmd.name, "\"sel\" positions are { \"p\", \"o\" }"))?;
        let f = SPos::from_pos(&sel.focus).ok_or_else(|| BusError::bad_args(&cmd.name, "\"sel\" positions are { \"p\", \"o\" }"))?;
        Ok(Span::new(self.clamp(&a), self.clamp(&f)))
    }

    /// The selection, or the whole document's start when a command was sent without one (the AI's
    /// convenience).
    pub(crate) fn sel_or_start(&self, cmd: &Command) -> Span {
        self.sel_span(cmd).unwrap_or_else(|_| Span::caret(SPos::body(0, 0)))
    }

    /// The paragraphs a selection touches (a selection ending at a paragraph's very start leaves it out).
    pub(crate) fn span_paras(&self, s: &Span) -> Vec<usize> {
        let mut last = s.end.p;
        if s.end.p > s.start.p && s.end.o == 0 {
            last -= 1;
        }
        (s.start.p..=last).collect()
    }

    pub(crate) fn set_landed(&mut self, cmd: &Command, sel: Selection) {
        self.landed = Some((fingerprint(cmd), sel));
    }

    pub(crate) fn land_caret(&mut self, cmd: &Command, p: &SPos) {
        let p = self.clamp(p);
        self.set_landed(cmd, Selection::caret(p.to_pos()));
    }

    pub(crate) fn land_span(&mut self, cmd: &Command, a: &SPos, b: &SPos) {
        let (a, b) = (self.clamp(a), self.clamp(b));
        self.set_landed(cmd, Selection::new(a.to_pos(), b.to_pos()));
    }

    // --- the transaction ---------------------------------------------------------------------------

    /// Edit one part's XML with the DOM, keeping every untouched byte. The closure sees the model as
    /// it was when the command started.
    pub(crate) fn tx_part<R>(&mut self, tx: &mut Tx, part: &str, f: impl FnOnce(&mut Element, &Cx) -> R) -> Result<R, BusError> {
        let bytes = self.pkg.part_bytes(part).map_err(|e| BusError::failed(e.to_string()))?;
        if !tx.parts.iter().any(|(n, _)| opc::same_part(n, part)) {
            tx.parts.push((part.to_string(), bytes.clone()));
        }
        let mut out = None;
        let new = {
            let cx = Cx { pkg: &self.pkg, model: &self.model, part };
            opc::try_patch_xml(&bytes, |root| out = Some(f(root, &cx))).map_err(|e| BusError::failed(e.to_string()))?
        };
        if new != bytes {
            self.pkg.set_part_bytes(part, new).map_err(|e| BusError::failed(e.to_string()))?;
        }
        out.ok_or_else(|| BusError::failed(format!("{part} could not be edited")))
    }

    /// Replace a whole part's bytes inside a transaction.
    pub(crate) fn tx_set(&mut self, tx: &mut Tx, part: &str, bytes: Vec<u8>) -> Result<(), BusError> {
        let before = self.pkg.part_bytes(part).map_err(|e| BusError::failed(e.to_string()))?;
        if !tx.parts.iter().any(|(n, _)| opc::same_part(n, part)) {
            tx.parts.push((part.to_string(), before));
        }
        self.pkg.set_part_bytes(part, bytes).map_err(|e| BusError::failed(e.to_string()))
    }

    /// Finish an edit: read the model again, lay out, and answer the undo (`scroll.restore`) and the
    /// pages whose display changed. Nothing changed answers an empty change (no undo step).
    pub(crate) fn tx_commit(&mut self, tx: Tx) -> Result<Change, BusError> {
        let mut changed = Vec::new();
        for (name, before) in &tx.parts {
            let now = self.pkg.part_bytes(name).map_err(|e| BusError::failed(e.to_string()))?;
            if &now != before {
                changed.push((name.clone(), before.clone(), now));
            }
        }
        if changed.is_empty() && !tx.dirty {
            return Ok(Change::default());
        }
        let old_pages = std::mem::take(&mut self.pages);
        let old_laid = std::mem::take(&mut self.laid);
        if let Err(e) = self.rebuild() {
            for (name, before, _) in &changed {
                let _ = self.pkg.set_part_bytes(name, before.clone());
            }
            let _ = self.rebuild();
            return Err(BusError::failed(format!("the edit left the document unreadable and was taken back: {e}")));
        }
        let mut entries = Vec::new();
        for (name, before, now) in &changed {
            if opc::same_part(name, &self.main) {
                if let (Some(old), Some(new)) = (body_children(before), body_children(now)) {
                    let mut a = 0;
                    while a < old.len() && a < new.len() && old[a] == new[a] {
                        a += 1;
                    }
                    let mut k = 0;
                    while k < old.len() - a && k < new.len() - a && old[old.len() - 1 - k] == new[new.len() - 1 - k] {
                        k += 1;
                    }
                    let xml: Vec<Json> = old[a..old.len() - k].iter().map(|s| Json::Str(s.clone())).collect();
                    entries.push(
                        Json::obj()
                            .with("part", name.as_str())
                            .with("start", a)
                            .with("count", new.len() - k - a)
                            .with("xml", Json::Arr(xml)),
                    );
                    continue;
                }
            }
            entries.push(Json::obj().with("part", name.as_str()).with("whole", String::from_utf8_lossy(before).into_owned()));
        }
        let mut touched = Vec::new();
        for i in 0..old_pages.len().max(self.pages.len()) {
            let same = match (old_pages.get(i), self.pages.get(i), old_laid.pages.get(i), self.laid.pages.get(i)) {
                (Some(a), Some(b), Some(la), Some(lb)) => {
                    same_page(a, b) && la.body_text == lb.body_text && la.header_text == lb.header_text && la.footer_text == lb.footer_text
                }
                _ => false,
            };
            if !same {
                touched.push(i);
            }
        }
        let undo = if entries.is_empty() { Vec::new() } else { vec![Command::new("scroll.restore", Json::obj().with("parts", Json::Arr(entries)))] };
        Ok(Change { undo, pages_touched: touched })
    }

    /// `scroll.restore` (an undo): put the XML an edit replaced back, answering its own inverse.
    pub(crate) fn restore(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let list = cmd.args.get("parts").and_then(Json::as_arr).ok_or_else(|| BusError::bad_args(&cmd.name, "takes { \"parts\": [..] }"))?.to_vec();
        let mut tx = Tx::default();
        for e in &list {
            let part = e.get("part").and_then(Json::as_str).ok_or_else(|| BusError::bad_args(&cmd.name, "each part names its \"part\""))?.to_string();
            if let Some(whole) = e.get("whole").and_then(Json::as_str) {
                self.tx_set(&mut tx, &part, whole.as_bytes().to_vec())?;
                continue;
            }
            let start = e.get("start").and_then(Json::as_usize).unwrap_or(0);
            let count = e.get("count").and_then(Json::as_usize).unwrap_or(0);
            let xml: Vec<String> = e.get("xml").and_then(Json::as_arr).unwrap_or(&[]).iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
            self.tx_part(&mut tx, &part, |root, _| {
                let Some(body) = root.child_mut("body") else { return };
                let n = dom::element_count(body);
                let start = start.min(n);
                let count = count.min(n - start);
                for _ in 0..count {
                    dom::remove_at(body, start);
                }
                let nodes: Vec<Node> = xml.into_iter().map(|s| Node::Raw(s.into_bytes())).collect();
                dom::insert_at(body, start, nodes);
            })?;
        }
        self.pending = None;
        self.tx_commit(tx)
    }
}

/// Two display lists that paint the same.
fn same_page(a: &DisplayList, b: &DisplayList) -> bool {
    a == b
}

/// The command's identity for [`ScrollDoc::landed`].
pub(crate) fn fingerprint(cmd: &Command) -> String {
    format!("{}{}", cmd.name, cmd.args)
}

/// The exact source of each element child of `w:body`.
pub(crate) fn body_children(bytes: &[u8]) -> Option<Vec<String>> {
    let src = xml::to_utf8(bytes).ok()?;
    let mut r = Reader::new(&src).ok()?;
    let mut depth = 0usize;
    let mut in_body = false;
    let mut out = Vec::new();
    while let Some(ev) = r.next().ok()? {
        match ev {
            Event::Start { name, span, .. } => {
                if in_body {
                    let _ = span;
                    let range = r.subtree_span().ok()?;
                    out.push(String::from_utf8(src[range].to_vec()).ok()?);
                    continue;
                }
                if depth == 1 && name.local == "body" {
                    in_body = true;
                    depth += 1;
                    continue;
                }
                if depth == 1 {
                    r.subtree_span().ok()?;
                    continue;
                }
                depth += 1;
            }
            Event::Empty { span, .. } => {
                if in_body {
                    out.push(String::from_utf8(src[span].to_vec()).ok()?);
                }
            }
            Event::End { .. } => {
                if in_body {
                    return Some(out);
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    if in_body {
        Some(out)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------------------------------

fn twips(pt: f64) -> String {
    format!("{}", (pt * 20.0).round() as i64)
}

fn half_points(pt: f64) -> String {
    format!("{}", (pt * 2.0).round().max(1.0) as i64)
}

/// `"#1f4e79"`, `"1F4E79"` or a few names -> `1F4E79`; `"auto"` stays.
pub(crate) fn hex_arg(v: &str) -> Option<String> {
    let v = v.trim().trim_start_matches('#');
    if v.eq_ignore_ascii_case("auto") {
        return Some("auto".into());
    }
    let named = match v.to_ascii_lowercase().as_str() {
        "black" => Some("000000"),
        "white" => Some("FFFFFF"),
        "red" => Some("FF0000"),
        "green" => Some("00B050"),
        "blue" => Some("0070C0"),
        "darkblue" | "dark blue" => Some("1F3864"),
        "yellow" => Some("FFFF00"),
        "orange" => Some("FFC000"),
        "purple" => Some("7030A0"),
        "gray" | "grey" => Some("808080"),
        _ => None,
    };
    if let Some(n) = named {
        return Some(n.into());
    }
    (v.len() == 6 && v.chars().all(|c| c.is_ascii_hexdigit())).then(|| v.to_ascii_uppercase())
}

const HIGHLIGHTS: &[&str] = &[
    "yellow", "green", "cyan", "magenta", "blue", "red", "darkBlue", "darkCyan", "darkGreen", "darkMagenta", "darkRed", "darkYellow", "darkGray",
    "lightGray", "black", "white",
];

const UNDERLINES: &[&str] = &[
    "single", "words", "double", "thick", "dotted", "dottedHeavy", "dash", "dashedHeavy", "dashLong", "dashLongHeavy", "dotDash", "dashDotHeavy",
    "dotDotDash", "dashDotDotHeavy", "wave", "wavyHeavy", "wavyDouble", "none",
];

/// One character property written into a `w:rPr`. Errors name what the value should be.
pub(crate) fn apply_rpr(rpr: &mut Element, w: &str, key: &str, v: &Json) -> Result<(), String> {
    let onoff = |rpr: &mut Element, local: &str, on: bool| {
        set_child(rpr, local, Some(if on { wel(w, local, &[]) } else { wel(w, local, &[("val", "0")]) }), RPR_ORDER);
    };
    let flag = |v: &Json| v.as_bool().ok_or_else(|| format!("\"{key}\" is true or false"));
    match key {
        "bold" => {
            let on = flag(v)?;
            onoff(rpr, "b", on);
            onoff(rpr, "bCs", on);
        }
        "italic" => {
            let on = flag(v)?;
            onoff(rpr, "i", on);
            onoff(rpr, "iCs", on);
        }
        "strike" => {
            set_child(rpr, "dstrike", None, RPR_ORDER);
            onoff(rpr, "strike", flag(v)?);
        }
        "double_strike" => {
            set_child(rpr, "strike", None, RPR_ORDER);
            onoff(rpr, "dstrike", flag(v)?);
        }
        "caps" => onoff(rpr, "caps", flag(v)?),
        "small_caps" => onoff(rpr, "smallCaps", flag(v)?),
        "hidden" => onoff(rpr, "vanish", flag(v)?),
        "underline" => {
            let style = match v {
                Json::Bool(true) => "single".to_string(),
                Json::Bool(false) | Json::Null => "none".to_string(),
                Json::Str(s) => {
                    let s = s.trim();
                    UNDERLINES.iter().find(|u| u.eq_ignore_ascii_case(s)).map(|u| u.to_string()).ok_or_else(|| format!("\"underline\" is true, false or one of {}", UNDERLINES.join(", ")))?
                }
                _ => return Err("\"underline\" is true, false or a style name".into()),
            };
            let mut u = rpr.child("u").cloned().unwrap_or_else(|| wel(w, "u", &[]));
            dom::set_attr_local(&mut u, w, "val", &style);
            set_child(rpr, "u", Some(u), RPR_ORDER);
        }
        "underline_color" => {
            let c = v.as_str().and_then(hex_arg).ok_or("\"underline_color\" is a hex colour like \"C00000\"")?;
            let mut u = rpr.child("u").cloned().unwrap_or_else(|| wel(w, "u", &[("val", "single")]));
            dom::set_attr_local(&mut u, w, "color", &c);
            dom::remove_attr_local(&mut u, "themeColor");
            set_child(rpr, "u", Some(u), RPR_ORDER);
        }
        "color" => match v {
            Json::Null => set_child(rpr, "color", None, RPR_ORDER),
            Json::Str(s) => {
                let c = hex_arg(s).ok_or("\"color\" is a hex colour like \"1F4E79\", \"auto\" or null")?;
                set_child(rpr, "color", Some(wel(w, "color", &[("val", &c)])), RPR_ORDER);
            }
            _ => return Err("\"color\" is a hex colour, \"auto\" or null".into()),
        },
        "highlight" => match v.as_str().map(str::trim) {
            None | Some("none") | Some("") => set_child(rpr, "highlight", None, RPR_ORDER),
            Some(h) => {
                let name = HIGHLIGHTS.iter().find(|x| x.eq_ignore_ascii_case(h)).ok_or_else(|| format!("\"highlight\" is one of {} or null", HIGHLIGHTS.join(", ")))?;
                set_child(rpr, "highlight", Some(wel(w, "highlight", &[("val", name)])), RPR_ORDER);
            }
        },
        "size" => {
            let pt = v.as_f64().filter(|x| *x >= 1.0 && *x <= 1638.0).ok_or("\"size\" is points, 1 to 1638")?;
            let hp = half_points(pt);
            set_child(rpr, "sz", Some(wel(w, "sz", &[("val", &hp)])), RPR_ORDER);
            set_child(rpr, "szCs", Some(wel(w, "szCs", &[("val", &hp)])), RPR_ORDER);
        }
        "font" => {
            let name = v.as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or("\"font\" is a family name, \"+Body\" or \"+Headings\"")?;
            let mut f = rpr.child("rFonts").cloned().unwrap_or_else(|| wel(w, "rFonts", &[]));
            let theme = match name.to_ascii_lowercase().as_str() {
                "+body" | "+minor" | "body" => Some("minor"),
                "+headings" | "+heading" | "+major" | "headings" => Some("major"),
                _ => None,
            };
            for a in ["ascii", "hAnsi", "asciiTheme", "hAnsiTheme", "cs", "cstheme"] {
                dom::remove_attr_local(&mut f, a);
            }
            match theme {
                Some(t) => {
                    dom::set_attr_local(&mut f, w, "asciiTheme", &format!("{t}HAnsi"));
                    dom::set_attr_local(&mut f, w, "hAnsiTheme", &format!("{t}HAnsi"));
                }
                None => {
                    dom::set_attr_local(&mut f, w, "ascii", name);
                    dom::set_attr_local(&mut f, w, "hAnsi", name);
                    dom::set_attr_local(&mut f, w, "cs", name);
                }
            }
            set_child(rpr, "rFonts", Some(f), RPR_ORDER);
        }
        "font_east_asian" => {
            let name = v.as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or("\"font_east_asian\" is a family name")?;
            let mut f = rpr.child("rFonts").cloned().unwrap_or_else(|| wel(w, "rFonts", &[]));
            dom::remove_attr_local(&mut f, "eastAsiaTheme");
            dom::set_attr_local(&mut f, w, "eastAsia", name);
            set_child(rpr, "rFonts", Some(f), RPR_ORDER);
        }
        "vert" | "superscript" | "subscript" => {
            let val = match (key, v) {
                ("vert", Json::Str(s)) => match s.trim() {
                    "superscript" | "super" => Some("superscript"),
                    "subscript" | "sub" => Some("subscript"),
                    "baseline" | "none" => None,
                    _ => return Err("\"vert\" is superscript, subscript or baseline".into()),
                },
                ("vert", Json::Null) => None,
                ("superscript", v) => flag(v)?.then_some("superscript"),
                ("subscript", v) => flag(v)?.then_some("subscript"),
                _ => return Err("\"vert\" is superscript, subscript or baseline".into()),
            };
            set_child(rpr, "vertAlign", val.map(|x| wel(w, "vertAlign", &[("val", x)])), RPR_ORDER);
        }
        "char_style" => match v.as_str() {
            Some(s) if !s.trim().is_empty() => set_child(rpr, "rStyle", Some(wel(w, "rStyle", &[("val", s.trim())])), RPR_ORDER),
            _ => set_child(rpr, "rStyle", None, RPR_ORDER),
        },
        "spacing" => {
            let pt = v.as_f64().ok_or("\"spacing\" is points (character spacing)")?;
            set_child(rpr, "spacing", Some(wel(w, "spacing", &[("val", &twips(pt))])), RPR_ORDER);
        }
        "clear" => {
            if v.as_bool() == Some(true) {
                rpr.children.clear();
            }
        }
        other => return Err(format!("unknown character property '{other}'")),
    }
    Ok(())
}

pub(crate) const RPR_KEYS: &[&str] = &[
    "bold", "italic", "strike", "double_strike", "caps", "small_caps", "hidden", "underline", "underline_color", "color", "highlight", "size", "font",
    "font_east_asian", "vert", "superscript", "subscript", "char_style", "spacing", "clear",
];

pub(crate) const PPR_KEYS: &[&str] = &[
    "align", "line", "line_rule", "before", "after", "ind_left", "ind_right", "first_line", "hanging", "keep_next", "keep_lines", "widow",
    "page_break_before", "contextual", "bidi", "tabs", "add_tab", "clear_tabs", "clear_tab", "style", "outline", "shading",
];

fn tab_el(w: &str, t: &Json) -> Result<Element, String> {
    let pos = t.get("pos").and_then(Json::as_f64).ok_or("a tab stop is { \"pos\": points, \"kind\"?: left|center|right|decimal|bar, \"leader\"?: none|dot|hyphen|underscore|middleDot }")?;
    let kind = match t.get("kind").and_then(Json::as_str).unwrap_or("left") {
        k @ ("left" | "center" | "right" | "decimal" | "bar" | "clear") => k,
        _ => return Err("a tab stop's \"kind\" is left, center, right, decimal or bar".into()),
    };
    let mut e = wel(w, "tab", &[("val", kind), ("pos", &twips(pos))]);
    if let Some(l) = t.get("leader").and_then(Json::as_str) {
        let l = match l {
            "dot" | "dots" => "dot",
            "hyphen" | "dash" => "hyphen",
            "underscore" | "line" => "underscore",
            "middleDot" => "middleDot",
            "none" => "none",
            _ => return Err("a tab's \"leader\" is none, dot, hyphen, underscore or middleDot".into()),
        };
        if l != "none" {
            e.set_attr(format!("{w}leader"), l);
        }
    }
    Ok(e)
}

fn tab_pos(e: &Element) -> f64 {
    dom::attr_local(e, "pos").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
}

/// One paragraph property written into a `w:pPr`.
pub(crate) fn apply_ppr(ppr: &mut Element, w: &str, key: &str, v: &Json, all: &Json) -> Result<(), String> {
    let onoff = |ppr: &mut Element, local: &str, on: bool| {
        set_child(ppr, local, Some(if on { wel(w, local, &[]) } else { wel(w, local, &[("val", "0")]) }), PPR_ORDER);
    };
    let flag = |v: &Json| v.as_bool().ok_or_else(|| format!("\"{key}\" is true or false"));
    let pts = |v: &Json| v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("\"{key}\" is points"));
    match key {
        "align" => {
            let a = match v.as_str().map(str::trim) {
                Some("left") | Some("start") => "left",
                Some("center") | Some("centre") => "center",
                Some("right") | Some("end") => "right",
                Some("justify") | Some("both") | Some("justified") => "both",
                Some("distribute") => "distribute",
                _ => return Err("\"align\" is left, center, right, justify or distribute".into()),
            };
            set_child(ppr, "jc", Some(wel(w, "jc", &[("val", a)])), PPR_ORDER);
        }
        "line" => {
            let rule = all.get("line_rule").and_then(Json::as_str).unwrap_or("auto");
            let x = v.as_f64().filter(|x| *x > 0.0).ok_or("\"line\" is a multiple (1, 1.15, 1.5, 2) or, with \"line_rule\" exact/atLeast, points")?;
            let sp = dom::ensure_child(ppr, w, "spacing", PPR_ORDER);
            match rule {
                "exact" | "exactly" => {
                    dom::set_attr_local(sp, w, "line", &twips(x));
                    dom::set_attr_local(sp, w, "lineRule", "exact");
                }
                "atLeast" | "at_least" => {
                    dom::set_attr_local(sp, w, "line", &twips(x));
                    dom::set_attr_local(sp, w, "lineRule", "atLeast");
                }
                _ => {
                    dom::set_attr_local(sp, w, "line", &format!("{}", (x * 240.0).round() as i64));
                    dom::set_attr_local(sp, w, "lineRule", "auto");
                }
            }
        }
        "line_rule" => {}
        "before" | "after" => {
            let x = pts(v)?.max(0.0);
            let sp = dom::ensure_child(ppr, w, "spacing", PPR_ORDER);
            dom::set_attr_local(sp, w, key, &twips(x));
            dom::remove_attr_local(sp, &format!("{key}Autospacing"));
            dom::remove_attr_local(sp, &format!("{key}Lines"));
        }
        "ind_left" | "ind_right" => {
            let x = pts(v)?;
            let ind = dom::ensure_child(ppr, w, "ind", PPR_ORDER);
            let (a, b) = if key == "ind_left" { ("left", "start") } else { ("right", "end") };
            dom::remove_attr_local(ind, b);
            dom::remove_attr_local(ind, &format!("{b}Chars"));
            dom::set_attr_local(ind, w, a, &twips(x));
        }
        "first_line" | "hanging" => {
            let x = pts(v)?.max(0.0);
            let ind = dom::ensure_child(ppr, w, "ind", PPR_ORDER);
            for a in ["firstLine", "hanging", "firstLineChars", "hangingChars"] {
                dom::remove_attr_local(ind, a);
            }
            dom::set_attr_local(ind, w, if key == "first_line" { "firstLine" } else { "hanging" }, &twips(x));
        }
        "keep_next" => onoff(ppr, "keepNext", flag(v)?),
        "keep_lines" => onoff(ppr, "keepLines", flag(v)?),
        "widow" => onoff(ppr, "widowControl", flag(v)?),
        "page_break_before" => onoff(ppr, "pageBreakBefore", flag(v)?),
        "contextual" => onoff(ppr, "contextualSpacing", flag(v)?),
        "bidi" => onoff(ppr, "bidi", flag(v)?),
        "tabs" => {
            let list = v.as_arr().ok_or("\"tabs\" is a list of tab stops")?;
            let mut stops: Vec<Element> = list.iter().map(|t| tab_el(w, t)).collect::<Result<_, _>>()?;
            stops.sort_by(|a, b| tab_pos(a).partial_cmp(&tab_pos(b)).unwrap_or(std::cmp::Ordering::Equal));
            if stops.is_empty() {
                set_child(ppr, "tabs", None, PPR_ORDER);
            } else {
                let mut tabs = wel(w, "tabs", &[]);
                tabs.children = stops.into_iter().map(Node::Element).collect();
                set_child(ppr, "tabs", Some(tabs), PPR_ORDER);
            }
        }
        "add_tab" | "clear_tab" => {
            let t = if key == "clear_tab" { Json::obj().with("pos", pts(v)?).with("kind", "clear") } else { v.clone() };
            let stop = tab_el(w, &t)?;
            let p = tab_pos(&stop);
            let tabs = dom::ensure_child(ppr, w, "tabs", PPR_ORDER);
            tabs.children.retain(|c| !matches!(c, Node::Element(e) if (tab_pos(e) - p).abs() < 1.0));
            let at = tabs.children.iter().position(|c| matches!(c, Node::Element(e) if tab_pos(e) > p)).unwrap_or(tabs.children.len());
            tabs.children.insert(at, Node::Element(stop));
        }
        "clear_tabs" => {
            if flag(v)? {
                set_child(ppr, "tabs", None, PPR_ORDER);
            }
        }
        "style" => match v.as_str() {
            Some(s) if !s.trim().is_empty() => set_child(ppr, "pStyle", Some(wel(w, "pStyle", &[("val", s.trim())])), PPR_ORDER),
            _ => set_child(ppr, "pStyle", None, PPR_ORDER),
        },
        "outline" => match v.as_usize() {
            Some(n) if n <= 9 => set_child(ppr, "outlineLvl", Some(wel(w, "outlineLvl", &[("val", &n.to_string())])), PPR_ORDER),
            _ => set_child(ppr, "outlineLvl", None, PPR_ORDER),
        },
        "shading" => match v.as_str().and_then(hex_arg) {
            Some(c) => set_child(ppr, "shd", Some(wel(w, "shd", &[("val", "clear"), ("color", "auto"), ("fill", &c)])), PPR_ORDER),
            None => set_child(ppr, "shd", None, PPR_ORDER),
        },
        other => return Err(format!("unknown paragraph property '{other}'")),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------
// Effective formatting
// ---------------------------------------------------------------------------------------------------

impl ScrollDoc {
    /// A run's effective character properties (document defaults, paragraph style, character style,
    /// direct formatting); `None` for the paragraph mark.
    pub(crate) fn eff_run(&self, para: &Para, ri: Option<usize>) -> RunProps {
        let st = &self.model.styles;
        let mut rp = st.doc_rpr.clone();
        if let Some(s) = para.ppr.style.clone().or_else(|| st.default_para.clone()) {
            rp.over(&st.folded(&s).1);
        }
        let direct = ri.and_then(|i| para.runs.get(i)).map(|r| &r.rpr).unwrap_or(&para.mark);
        if let Some(cs) = &direct.style {
            rp.over_toggling(&st.char_rpr(cs));
        }
        rp.over(direct);
        rp
    }

    /// A paragraph's effective paragraph properties (numbering level indents not included).
    pub(crate) fn eff_para(&self, para: &Para) -> ParaProps {
        let st = &self.model.styles;
        let mut pp = st.doc_ppr.clone();
        if let Some(s) = para.ppr.style.clone().or_else(|| st.default_para.clone()) {
            pp.over(&st.folded(&s).0);
        }
        let mut d = para.ppr.clone();
        d.style = None;
        pp.over(&d);
        pp
    }

    /// The runs (by paragraph) a selection covers, for the ribbon and the toggles: a caret looks at
    /// the character before it (the one after at a paragraph's start, the mark when empty).
    pub(crate) fn covered_runs(&self, s: &Span) -> Vec<(usize, Option<usize>)> {
        let mut out = Vec::new();
        let flow = s.flow().clone();
        if s.is_caret() {
            if let Some(r) = self.para_ref(&flow, s.start.p) {
                let ats = atoms(&r.para.runs);
                let o = s.start.o.min(ats.len());
                let ri = if o > 0 { Some(ats[o - 1].run) } else { ats.first().map(|a| a.run) };
                out.push((s.start.p, ri));
            }
            return out;
        }
        for p in s.start.p..=s.end.p {
            let Some(r) = self.para_ref(&flow, p) else { continue };
            let ats = atoms(&r.para.runs);
            let a = if p == s.start.p { s.start.o } else { 0 };
            let b = if p == s.end.p { s.end.o } else { ats.len() };
            let mut seen = Vec::new();
            for at in ats.iter().take(b.min(ats.len())).skip(a) {
                if !seen.contains(&at.run) {
                    seen.push(at.run);
                }
            }
            if seen.is_empty() {
                if ats.is_empty() {
                    out.push((p, None));
                }
                continue;
            }
            out.extend(seen.into_iter().map(|ri| (p, Some(ri))));
            if out.len() > 2000 {
                break;
            }
        }
        out
    }

    /// True when every covered run has the property on.
    fn all_on(&self, s: &Span, key: &str) -> bool {
        let flow = s.flow().clone();
        let runs = self.covered_runs(s);
        if runs.is_empty() {
            return false;
        }
        runs.iter().all(|(p, ri)| {
            let Some(r) = self.para_ref(&flow, *p) else { return false };
            let e = self.eff_run(r.para, *ri);
            match key {
                "bold" => e.bold == Some(true),
                "italic" => e.italic == Some(true),
                "underline" => e.underline == Some(true),
                "strike" => e.strike == Some(true),
                "caps" => e.caps == Some(true),
                "small_caps" => e.small_caps == Some(true),
                "superscript" => e.vert == Some(super::props::VertAlign::Super),
                "subscript" => e.vert == Some(super::props::VertAlign::Sub),
                "hidden" => e.hidden == Some(true),
                _ => false,
            }
        })
    }
}

// ---------------------------------------------------------------------------------------------------
// Text editing
// ---------------------------------------------------------------------------------------------------

/// Paragraph paths: the container path and the paragraph's index in it.
fn split_path(path: &[usize]) -> (Vec<usize>, usize) {
    let (c, last) = path.split_at(path.len().saturating_sub(1));
    (c.to_vec(), last.first().copied().unwrap_or(0))
}

impl ScrollDoc {
    /// Delete what a selection covers; the caret lands at its start.
    pub(crate) fn delete_span(&mut self, tx: &mut Tx, s: &Span) -> Result<SPos, BusError> {
        if s.is_caret() {
            return Ok(s.start.clone());
        }
        let flow = s.flow().clone();
        let (r1, r2) = match (self.para_ref(&flow, s.start.p), self.para_ref(&flow, s.end.p)) {
            (Some(a), Some(b)) => (
                (a.part.clone(), a.flat.path.clone(), a.para.fields_in.clone()),
                (b.part.clone(), b.flat.path.clone(), b.para.fields_in.clone()),
            ),
            _ => return Ok(s.start.clone()),
        };
        let part = r1.0.clone();
        let (o1, o2) = (s.start.o, s.end.o);
        if s.start.p == s.end.p {
            let path = r1.1.clone();
            let fi = r1.2.clone();
            self.tx_part(tx, &part, |root, cx| {
                if let Some(p) = el_at_mut(root, &path) {
                    let runs = cx.runs(p, &fi);
                    dom::delete_range(p, &runs, o1, o2);
                }
            })?;
            return Ok(s.start.clone());
        }
        let (c1, i1) = split_path(&r1.1);
        let (c2, i2) = split_path(&r2.1);
        if c1 == c2 && i2 > i1 {
            let (f1, f2) = (r1.2.clone(), r2.2.clone());
            self.tx_part(tx, &part, |root, cx| {
                let Some(c) = el_at_mut(root, &c1) else { return };
                if let Some(p2) = dom::child_at_mut(c, i2) {
                    let runs = cx.runs(p2, &f2);
                    dom::delete_range(p2, &runs, 0, o2);
                }
                let mut first_empty = false;
                if let Some(p1) = dom::child_at_mut(c, i1) {
                    let runs = cx.runs(p1, &f1);
                    let n = atom_count(&runs);
                    dom::delete_range(p1, &runs, o1, n);
                    first_empty = atom_count(&cx.runs(p1, &f1)) == 0;
                }
                for k in (i1 + 1..i2).rev() {
                    dom::remove_at(c, k);
                }
                if let Some(second) = dom::remove_at(c, i1 + 1) {
                    if let Some(p1) = dom::child_at_mut(c, i1) {
                        dom::join_paras(p1, second, first_empty);
                    }
                }
            })?;
            return Ok(s.start.clone());
        }
        // Across containers (into or out of a table): each paragraph loses its covered characters.
        let mut work = Vec::new();
        for p in s.start.p..=s.end.p {
            let Some(r) = self.para_ref(&flow, p) else { continue };
            let n = atom_count(&r.para.runs);
            let a = if p == s.start.p { o1 } else { 0 };
            let b = if p == s.end.p { o2 } else { n };
            work.push((r.flat.path.clone(), r.para.fields_in.clone(), a, b));
        }
        self.tx_part(tx, &part, |root, cx| {
            for (path, fi, a, b) in work {
                if let Some(p) = el_at_mut(root, &path) {
                    let runs = cx.runs(p, &fi);
                    dom::delete_range(p, &runs, a, b);
                }
            }
        })?;
        Ok(s.start.clone())
    }

    /// Insert text at a caret: `\n` starts a new paragraph, `\u{b}` is a line break, `\t` a tab,
    /// `\u{c}` a page break. `rpr` formats the text as a run of its own. Answers the caret after it.
    pub(crate) fn insert_at(&mut self, tx: &mut Tx, at: &SPos, text: &str, rpr: Option<Element>) -> Result<SPos, BusError> {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let Some(r) = self.para_ref(&at.flow, at.p) else { return Ok(at.clone()) };
        let (part, path, fi) = (r.part.clone(), r.flat.path.clone(), r.para.fields_in.clone());
        let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        let o = at.o;
        let last_len = self.tx_part(tx, &part, |root, cx| {
            let (cpath, idx) = split_path(&path);
            let Some(c) = el_at_mut(root, &cpath) else { return 0 };
            let mut cur = idx;
            let mut off = o;
            let mut last = 0;
            for (k, line) in lines.iter().enumerate() {
                let Some(p) = dom::child_at_mut(c, cur) else { break };
                let runs = cx.runs(p, &fi);
                let n = dom::insert_text(p, &runs, off, line, rpr.clone());
                last = off + n;
                if k + 1 < lines.len() {
                    let runs = cx.runs(p, &fi);
                    let right = dom::split_para(p, &runs, off + n);
                    dom::insert_at(c, cur + 1, vec![Node::Element(right)]);
                    cur += 1;
                    off = 0;
                }
            }
            last
        })?;
        Ok(at.para(at.p + lines.len() - 1, last_len))
    }

    /// Split the paragraph at a caret (Enter). An empty list item leaves the list instead; at the
    /// end of a paragraph whose style names a next style, the new paragraph takes it.
    pub(crate) fn split_at(&mut self, tx: &mut Tx, at: &SPos) -> Result<SPos, BusError> {
        let Some(r) = self.para_ref(&at.flow, at.p) else { return Ok(at.clone()) };
        let (part, path, fi) = (r.part.clone(), r.flat.path.clone(), r.para.fields_in.clone());
        let n = atom_count(&r.para.runs);
        if n == 0 && r.para.ppr.num_id.is_some_and(|x| x != 0) {
            self.tx_part(tx, &part, |root, _| {
                if let Some(ppr) = el_at_mut(root, &path).and_then(|p| p.child_mut("pPr")) {
                    set_child(ppr, "numPr", None, PPR_ORDER);
                }
            })?;
            return Ok(at.clone());
        }
        let next_style = if at.o >= n {
            r.para.ppr.style.as_ref().and_then(|s| self.model.styles.get(s)).and_then(|s| s.next.clone()).filter(|nx| Some(nx) != r.para.ppr.style.as_ref())
        } else {
            None
        };
        let o = at.o;
        self.tx_part(tx, &part, |root, cx| {
            let (cpath, idx) = split_path(&path);
            let Some(c) = el_at_mut(root, &cpath) else { return };
            let Some(p) = dom::child_at_mut(c, idx) else { return };
            let runs = cx.runs(p, &fi);
            let mut right = dom::split_para(p, &runs, o);
            if let Some(nx) = &next_style {
                let w = prefix_of(&right);
                let ppr = dom::ppr_mut(&mut right, &w);
                set_child(ppr, "pStyle", Some(wel(&w, "pStyle", &[("val", nx)])), PPR_ORDER);
            }
            dom::insert_at(c, idx + 1, vec![Node::Element(right)]);
        })?;
        Ok(at.para(at.p + 1, 0))
    }

    /// Can paragraph `p` be joined with `p + 1` (siblings in one container)?
    fn joinable(&self, flow: &Flow, p: usize) -> bool {
        match (self.para_ref(flow, p), self.para_ref(flow, p + 1)) {
            (Some(a), Some(b)) => {
                let (c1, i1) = split_path(&a.flat.path);
                let (c2, i2) = split_path(&b.flat.path);
                c1 == c2 && i2 == i1 + 1
            }
            _ => false,
        }
    }

    // --- the commands ------------------------------------------------------------------------------

    pub(crate) fn cmd_insert_text(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let text = cmd.args.get("text").and_then(Json::as_str).ok_or_else(|| BusError::bad_args(&cmd.name, "takes { \"sel\", \"text\": string }"))?.to_string();
        let pending = self.pending.take();
        // Typing over a selection takes the formatting of its first character.
        let over = if s.is_caret() || text.is_empty() { None } else { self.rpr_at(&s.start) };
        let mut tx = Tx::default();
        let at = self.delete_span(&mut tx, &s)?;
        let at = self.clamp_after(&at);
        let rpr = match pending {
            Some((pp, edits)) if pp == s.start && !text.is_empty() => Some(self.pending_rpr(&at, &edits)?),
            _ => over,
        };
        let end = if text.is_empty() { at.clone() } else { self.insert_at(&mut tx, &at, &text, rpr)? };
        let ch = self.tx_commit(tx)?;
        self.land_caret(cmd, &end);
        Ok(ch)
    }

    /// The model is stale inside a transaction; a place computed before stays valid for the edits
    /// that follow in the same part (they work from the XML as it is).
    fn clamp_after(&self, p: &SPos) -> SPos {
        p.clone()
    }

    /// The `w:rPr` of the run holding character `at.o` (none for a run without one).
    fn rpr_at(&self, at: &SPos) -> Option<Element> {
        let r = self.para_ref(&at.flow, at.p)?;
        let ats = atoms(&r.para.runs);
        let a = ats.get(at.o)?;
        if r.para.runs[a.run].kind != RunKind::R {
            return None;
        }
        let root = self.part_dom(&r.part)?;
        let p = el_at(&root, &r.flat.path)?;
        let runs = model::read_runs(&self.pkg, &r.part, &self.model, p, &r.para.fields_in);
        dom::run_rpr(p, &runs, a.run)
    }

    /// The run properties typed text takes after formatting was chosen at a caret.
    fn pending_rpr(&self, at: &SPos, edits: &[(String, Json)]) -> Result<Element, BusError> {
        let r = self.para_ref(&at.flow, at.p).ok_or_else(|| BusError::failed("no paragraph there"))?;
        let bytes = self.pkg.part_bytes(&r.part).map_err(|e| BusError::failed(e.to_string()))?;
        let root = xml::parse_dom(&bytes).map_err(|e| BusError::failed(e.to_string()))?;
        let p = el_at(&root, &r.flat.path).ok_or_else(|| BusError::failed("no paragraph there"))?;
        let w = prefix_of(p);
        let runs = model::read_runs(&self.pkg, &r.part, &self.model, p, &r.para.fields_in);
        let pt = dom::point_at(p, &runs, at.o);
        let mut rpr = pt.rpr_from.and_then(|ri| dom::run_rpr(p, &runs, ri)).or_else(|| dom::mark_rpr(p)).unwrap_or_else(|| wel(&w, "rPr", &[]));
        for (k, v) in edits {
            apply_rpr(&mut rpr, &w, k, v).map_err(BusError::failed)?;
        }
        Ok(rpr)
    }

    pub(crate) fn cmd_delete(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let back = cmd.args.get("back").and_then(Json::as_bool).unwrap_or(true);
        let word = cmd.args.get("word").and_then(Json::as_bool).unwrap_or(false);
        let mut tx = Tx::default();
        if !s.is_caret() {
            let at = self.delete_span(&mut tx, &s)?;
            let ch = self.tx_commit(tx)?;
            self.land_caret(cmd, &at);
            return Ok(ch);
        }
        let at = s.start.clone();
        let flow = at.flow.clone();
        let chars = self.para_chars(&flow, at.p);
        if back {
            if at.o > 0 {
                let from = if word { pos::word_left(&chars, at.o) } else { at.o - 1 };
                let land = self.delete_span(&mut tx, &Span::new(at.at(from), at.clone()))?;
                let ch = self.tx_commit(tx)?;
                self.land_caret(cmd, &land);
                return Ok(ch);
            }
            // At a paragraph's start: a list item leaves its list first.
            if let Some(r) = self.para_ref(&flow, at.p) {
                if r.para.ppr.num_id.is_some_and(|n| n != 0) {
                    let (part, path) = (r.part.clone(), r.flat.path.clone());
                    self.tx_part(&mut tx, &part, |root, _| {
                        if let Some(ppr) = el_at_mut(root, &path).and_then(|p| p.child_mut("pPr")) {
                            set_child(ppr, "numPr", None, PPR_ORDER);
                        }
                    })?;
                    let ch = self.tx_commit(tx)?;
                    self.land_caret(cmd, &at);
                    return Ok(ch);
                }
            }
            if at.p == 0 {
                self.land_caret(cmd, &at);
                return Ok(Change::default());
            }
            let prev_len = self.para_len(&flow, at.p - 1);
            let prev = at.para(at.p - 1, prev_len);
            if !self.joinable(&flow, at.p - 1) {
                self.land_caret(cmd, &prev);
                return Ok(Change::default());
            }
            let land = self.delete_span(&mut tx, &Span::new(prev.clone(), at.clone()))?;
            let ch = self.tx_commit(tx)?;
            self.land_caret(cmd, &land);
            return Ok(ch);
        }
        if at.o < chars.len() {
            let to = if word { pos::word_right(&chars, at.o).max(at.o + 1) } else { at.o + 1 };
            let land = self.delete_span(&mut tx, &Span::new(at.clone(), at.at(to)))?;
            let ch = self.tx_commit(tx)?;
            self.land_caret(cmd, &land);
            return Ok(ch);
        }
        if at.p + 1 >= self.flow_len(&flow) || !self.joinable(&flow, at.p) {
            self.land_caret(cmd, &at);
            return Ok(Change::default());
        }
        let land = self.delete_span(&mut tx, &Span::new(at.clone(), at.para(at.p + 1, 0)))?;
        let ch = self.tx_commit(tx)?;
        self.land_caret(cmd, &land);
        Ok(ch)
    }

    pub(crate) fn cmd_enter(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let shift = cmd.args.get("shift").and_then(Json::as_bool).unwrap_or(false);
        let ctrl = cmd.args.get("ctrl").and_then(Json::as_bool).unwrap_or(false);
        self.pending = None;
        let mut tx = Tx::default();
        let at = self.delete_span(&mut tx, &s)?;
        let land = if ctrl {
            self.insert_at(&mut tx, &at, "\u{c}", None)?
        } else if shift {
            self.insert_at(&mut tx, &at, "\u{b}", None)?
        } else if tx.parts.is_empty() {
            self.split_at(&mut tx, &at)?
        } else {
            // The selection was deleted first: split the edited XML at the same place.
            let ch_split = self.insert_at(&mut tx, &at, "\n", None)?;
            ch_split
        };
        let ch = self.tx_commit(tx)?;
        self.land_caret(cmd, &land);
        Ok(ch)
    }

    pub(crate) fn cmd_tab(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let back = cmd.args.get("back").and_then(Json::as_bool).unwrap_or(false);
        let flow = s.flow().clone();
        let (in_cell, in_list) = match self.para_ref(&flow, s.start.p) {
            Some(r) => (self.cell_of(&r.part, &r.flat.path).is_some(), self.eff_para(r.para).num_id.is_some_and(|n| n != 0)),
            None => (false, false),
        };
        let multi = s.start.p != s.end.p;
        if in_list && (s.start.o == 0 || multi) {
            return self.change_level(cmd, &s, if back { -1 } else { 1 });
        }
        if in_cell {
            return self.cell_step(cmd, &s, back);
        }
        if back || multi {
            // Shift+Tab outside a list and a table steps the indent back; Tab over several
            // paragraphs indents them.
            return self.indent_by(cmd, &s, if back { -36.0 } else { 36.0 });
        }
        let mut tx = Tx::default();
        let at = self.delete_span(&mut tx, &s)?;
        let land = self.insert_at(&mut tx, &at, "\t", None)?;
        let ch = self.tx_commit(tx)?;
        self.land_caret(cmd, &land);
        Ok(ch)
    }

    /// Move the left indent of the selected paragraphs by `delta` points (never below zero).
    pub(crate) fn indent_by(&mut self, cmd: &Command, s: &Span, delta: f64) -> Result<Change, BusError> {
        let flow = s.flow().clone();
        let mut work = Vec::new();
        for p in self.span_paras(s) {
            let Some(r) = self.para_ref(&flow, p) else { continue };
            let cur = self.eff_para(r.para).ind_left.unwrap_or(0.0);
            let next = ((cur + delta) / 36.0).round().max(0.0) * 36.0;
            if (next - cur).abs() > 0.01 {
                work.push((r.part.clone(), r.flat.path.clone(), next));
            }
        }
        let mut tx = Tx::default();
        for (part, path, next) in work {
            self.tx_part(&mut tx, &part, |root, _| {
                if let Some(p) = el_at_mut(root, &path) {
                    let w = prefix_of(p);
                    let ppr = dom::ppr_mut(p, &w);
                    let _ = apply_ppr(ppr, &w, "ind_left", &Json::Num(next), &Json::Null);
                }
            })?;
        }
        let ch = self.tx_commit(tx)?;
        self.set_landed(cmd, Selection::new(s.start.to_pos(), s.end.to_pos()));
        Ok(ch)
    }

    // --- formatting --------------------------------------------------------------------------------

    /// Apply character property edits to a selection. A caret inside a word formats the word; a
    /// caret elsewhere keeps them for the text typed next.
    pub(crate) fn format_span(&mut self, cmd: &Command, s: &Span, edits: Vec<(String, Json)>) -> Result<Change, BusError> {
        let flow = s.flow().clone();
        let mut s = s.clone();
        if s.is_caret() {
            let chars = self.para_chars(&flow, s.start.p);
            let (a, b) = pos::word_at(&chars, s.start.o);
            if a < b && a < s.start.o && s.start.o < b {
                s = Span::new(s.start.at(a), s.start.at(b));
            } else if !chars.is_empty() {
                let mut list = match self.pending.take() {
                    Some((pp, l)) if pp == s.start => l,
                    _ => Vec::new(),
                };
                for (k, v) in edits {
                    list.retain(|(x, _)| *x != k);
                    list.push((k, v));
                }
                self.pending = Some((s.start.clone(), list));
                self.set_landed(cmd, Selection::caret(s.start.to_pos()));
                return Ok(Change::default());
            }
        }
        // Validate before touching anything.
        let mut probe = wel("w:", "rPr", &[]);
        for (k, v) in &edits {
            apply_rpr(&mut probe, "w:", k, v).map_err(|e| BusError::bad_args(&cmd.name, e))?;
        }
        let mut work = Vec::new();
        for p in s.start.p..=s.end.p {
            let Some(r) = self.para_ref(&flow, p) else { continue };
            let n = atom_count(&r.para.runs);
            let a = if p == s.start.p { s.start.o } else { 0 };
            let b = if p == s.end.p { s.end.o } else { n };
            if a >= b && n > 0 {
                continue;
            }
            work.push((r.part.clone(), r.flat.path.clone(), r.para.fields_in.clone(), a, b));
        }
        let mut tx = Tx::default();
        let part = self.part_name(&flow);
        let edits2 = edits.clone();
        self.tx_part(&mut tx, &part, |root, cx| {
            for (_, path, fi, a, b) in work {
                let Some(p) = el_at_mut(root, &path) else { continue };
                let w = prefix_of(p);
                let runs = cx.runs(p, &fi);
                let f = |rpr: &mut Element| {
                    for (k, v) in &edits2 {
                        let _ = apply_rpr(rpr, &w, k, v);
                    }
                };
                dom::format_range(p, &runs, a, b, &f);
            }
        })?;
        self.pending = None;
        let ch = self.tx_commit(tx)?;
        self.set_landed(cmd, Selection::new(if s.backward { s.end.to_pos() } else { s.start.to_pos() }, if s.backward { s.start.to_pos() } else { s.end.to_pos() }));
        Ok(ch)
    }

    pub(crate) fn cmd_format(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let mut edits = Vec::new();
        if let Some(obj) = cmd.args.as_obj() {
            for (k, v) in obj {
                if k == "sel" {
                    continue;
                }
                if !RPR_KEYS.contains(&k.as_str()) {
                    return Err(BusError::bad_args(&cmd.name, format!("unknown property '{k}'; the properties are: {}", RPR_KEYS.join(", "))));
                }
                edits.push((k.clone(), v.clone()));
            }
        }
        if edits.is_empty() {
            return Err(BusError::bad_args(&cmd.name, format!("give at least one of: {}", RPR_KEYS.join(", "))));
        }
        self.format_span(cmd, &s, edits)
    }

    pub(crate) fn cmd_toggle(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let prop = cmd.args.get("prop").and_then(Json::as_str).unwrap_or("");
        const TOGGLES: [&str; 8] = ["bold", "italic", "underline", "strike", "caps", "small_caps", "superscript", "subscript"];
        if !TOGGLES.contains(&prop) {
            return Err(BusError::bad_args(&cmd.name, format!("\"prop\" is one of: {}", TOGGLES.join(", "))));
        }
        let pending_on = match &self.pending {
            Some((pp, l)) if *pp == s.start && s.is_caret() => l.iter().find(|(k, _)| k == prop).and_then(|(_, v)| match v {
                Json::Bool(b) => Some(*b),
                Json::Str(x) => Some(x != "none"),
                _ => None,
            }),
            _ => None,
        };
        let on = !pending_on.unwrap_or_else(|| self.all_on(&s, prop));
        self.format_span(cmd, &s, vec![(prop.to_string(), Json::Bool(on))])
    }

    /// Grow or shrink the font size one step of the usual list.
    pub(crate) fn cmd_font_step(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let up = cmd.args.get("delta").and_then(Json::as_f64).unwrap_or(1.0) >= 0.0;
        const SIZES: [f64; 16] = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 28.0, 36.0, 48.0, 72.0, 96.0];
        let flow = s.flow().clone();
        let cur = self
            .covered_runs(&s)
            .first()
            .and_then(|(p, ri)| self.para_ref(&flow, *p).map(|r| self.eff_run(r.para, *ri).size.unwrap_or(10.0)))
            .unwrap_or(11.0);
        let next = if up { SIZES.iter().copied().find(|x| *x > cur + 0.01).unwrap_or(cur + 12.0) } else { SIZES.iter().rev().copied().find(|x| *x < cur - 0.01).unwrap_or(1.0f64.max(cur - 1.0)) };
        self.format_span(cmd, &s, vec![("size".into(), Json::Num(next))])
    }

    pub(crate) fn cmd_paragraph(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let mut edits = Vec::new();
        if let Some(obj) = cmd.args.as_obj() {
            for (k, v) in obj {
                if k == "sel" {
                    continue;
                }
                if !PPR_KEYS.contains(&k.as_str()) {
                    return Err(BusError::bad_args(&cmd.name, format!("unknown property '{k}'; the properties are: {}", PPR_KEYS.join(", "))));
                }
                edits.push((k.clone(), v.clone()));
            }
        }
        if edits.is_empty() {
            return Err(BusError::bad_args(&cmd.name, format!("give at least one of: {}", PPR_KEYS.join(", "))));
        }
        let mut probe = wel("w:", "pPr", &[]);
        for (k, v) in &edits {
            apply_ppr(&mut probe, "w:", k, v, &cmd.args).map_err(|e| BusError::bad_args(&cmd.name, e))?;
        }
        self.para_edit(cmd, &s, |ppr, w| {
            for (k, v) in &edits {
                let _ = apply_ppr(ppr, w, k, v, &cmd.args);
            }
        })
    }

    /// Apply `f` to the `w:pPr` of every paragraph the selection touches.
    pub(crate) fn para_edit(&mut self, cmd: &Command, s: &Span, f: impl Fn(&mut Element, &str)) -> Result<Change, BusError> {
        let flow = s.flow().clone();
        let paths: Vec<Vec<usize>> = self.span_paras(s).into_iter().filter_map(|p| self.para_ref(&flow, p).map(|r| r.flat.path.clone())).collect();
        let part = self.part_name(&flow);
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &part, |root, _| {
            for path in &paths {
                let Some(p) = el_at_mut(root, path) else { continue };
                let w = prefix_of(p);
                let ppr = dom::ppr_mut(p, &w);
                f(ppr, &w);
                if ppr.children.is_empty() && ppr.attrs.is_empty() {
                    p.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "pPr"));
                }
            }
        })?;
        let ch = self.tx_commit(tx)?;
        let (a, b) = if s.backward { (s.end.clone(), s.start.clone()) } else { (s.start.clone(), s.end.clone()) };
        self.land_span(cmd, &a, &b);
        Ok(ch)
    }

    pub(crate) fn cmd_align(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let v = cmd.args.get("value").cloned().unwrap_or(Json::Null);
        let mut probe = wel("w:", "pPr", &[]);
        apply_ppr(&mut probe, "w:", "align", &v, &Json::Null).map_err(|e| BusError::bad_args(&cmd.name, e))?;
        self.para_edit(cmd, &s, |ppr, w| {
            let _ = apply_ppr(ppr, w, "align", &v, &Json::Null);
        })
    }

    pub(crate) fn cmd_indent(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let up = cmd.args.get("delta").and_then(Json::as_f64).unwrap_or(1.0) >= 0.0;
        let flow = s.flow().clone();
        let in_list = self.para_ref(&flow, s.start.p).is_some_and(|r| r.para.ppr.num_id.is_some_and(|n| n != 0));
        if in_list {
            return self.change_level(cmd, &s, if up { 1 } else { -1 });
        }
        self.indent_by(cmd, &s, if up { 36.0 } else { -36.0 })
    }

    /// Remove direct formatting: character formatting of the selection, and the paragraphs back to
    /// the default style with no direct paragraph properties (lists and section breaks kept).
    pub(crate) fn cmd_clear_format(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let flow = s.flow().clone();
        let mut work = Vec::new();
        for p in s.start.p..=s.end.p {
            let Some(r) = self.para_ref(&flow, p) else { continue };
            let n = atom_count(&r.para.runs);
            let (a, b) = if s.is_caret() { (0, n) } else { (if p == s.start.p { s.start.o } else { 0 }, if p == s.end.p { s.end.o } else { n }) };
            work.push((r.flat.path.clone(), r.para.fields_in.clone(), a, b));
        }
        let part = self.part_name(&flow);
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &part, |root, cx| {
            for (path, fi, a, b) in &work {
                let Some(p) = el_at_mut(root, path) else { continue };
                let runs = cx.runs(p, fi);
                dom::format_range(p, &runs, *a, *b, &|rpr: &mut Element| rpr.children.clear());
                if let Some(ppr) = p.child_mut("pPr") {
                    ppr.children.retain(|c| matches!(c, Node::Element(e) if matches!(e.local(), "numPr" | "sectPr" | "pPrChange" | "rPr")));
                }
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &s.start, &s.end);
        Ok(ch)
    }

    // --- styles ------------------------------------------------------------------------------------

    /// A style id from an id or a display name ("Heading 2", "heading2", "Heading2").
    pub(crate) fn style_id(&self, name: &str) -> Option<String> {
        let st = &self.model.styles;
        if st.get(name).is_some() {
            return Some(name.to_string());
        }
        let norm = |s: &str| s.to_ascii_lowercase().replace([' ', '-', '_'], "");
        let want = norm(name);
        st.map.values().find(|s| norm(&s.name) == want || norm(&s.id) == want).map(|s| s.id.clone())
    }

    pub(crate) fn cmd_style(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let name = cmd.args.get("style").and_then(Json::as_str).map(str::trim).filter(|x| !x.is_empty()).ok_or_else(|| BusError::bad_args(&cmd.name, "takes { \"sel\", \"style\": id or name }"))?.to_string();
        let mut tx = Tx::default();
        let id = match self.style_id(&name) {
            Some(id) => id,
            None => {
                let id = builtin_style_id(&name).ok_or_else(|| {
                    let mut names: Vec<&str> = self.model.styles.map.values().filter(|s| s.kind != super::styles::StyleKind::Numbering).map(|s| s.name.as_str()).collect();
                    names.sort();
                    BusError::failed(format!("no style '{name}'; the document's styles are: {}", names.join(", ")))
                })?;
                self.ensure_style(&mut tx, &id)?;
                id
            }
        };
        let kind = self.model.styles.get(&id).map(|s| s.kind).unwrap_or_else(|| if builtin_is_char(&id) { super::styles::StyleKind::Character } else { super::styles::StyleKind::Paragraph });
        if kind == super::styles::StyleKind::Character {
            if !tx.parts.is_empty() {
                self.tx_commit_silent(tx)?;
            }
            return self.format_span(cmd, &s, vec![("char_style".into(), Json::Str(id))]);
        }
        let flow = s.flow().clone();
        let paths: Vec<Vec<usize>> = self.span_paras(&s).into_iter().filter_map(|p| self.para_ref(&flow, p).map(|r| r.flat.path.clone())).collect();
        let part = self.part_name(&flow);
        let default = self.model.styles.default_para.clone();
        self.tx_part(&mut tx, &part, |root, _| {
            for path in &paths {
                let Some(p) = el_at_mut(root, path) else { continue };
                let w = prefix_of(p);
                let ppr = dom::ppr_mut(p, &w);
                let val = if default.as_deref() == Some(id.as_str()) { None } else { Some(wel(&w, "pStyle", &[("val", &id)])) };
                set_child(ppr, "pStyle", val, PPR_ORDER);
                if ppr.children.is_empty() && ppr.attrs.is_empty() {
                    p.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "pPr"));
                }
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &s.start, &s.end);
        Ok(ch)
    }

    /// Commit a transaction whose undo is folded into the next one (a style the next edit needs).
    fn tx_commit_silent(&mut self, tx: Tx) -> Result<(), BusError> {
        self.tx_commit(tx).map(|_| ())
    }

    /// Make sure the styles part holds style `id` (a built-in one is added when missing).
    pub(crate) fn ensure_style(&mut self, tx: &mut Tx, id: &str) -> Result<(), BusError> {
        if self.model.styles.get(id).is_some() {
            return Ok(());
        }
        let Some(xml_text) = builtin_style(id) else { return Ok(()) };
        let part = match self.pkg.related(&self.main, opc::REL_STYLES).first().cloned() {
            Some(p) => p,
            None => {
                let name = "/word/styles.xml".to_string();
                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:styles xmlns:w=\"{}\"></w:styles>",
                    W_NS
                );
                if self.pkg.part(&name).is_none() {
                    self.pkg.add_part(&name, opc::CT_STYLES, body.into_bytes()).map_err(|e| BusError::failed(e.to_string()))?;
                }
                self.pkg.add_relationship(&self.main, opc::REL_STYLES, "styles.xml", false);
                tx.dirty = true;
                name
            }
        };
        let based = builtin_based_on(id);
        let mut needed: Vec<&str> = Vec::new();
        if let Some(b) = based {
            if self.model.styles.get(b).is_none() && b != id {
                needed.push(b);
            }
        }
        needed.push(id);
        let default = self.model.styles.default_para.clone();
        let snippets: Vec<String> = needed
            .iter()
            .filter(|i| !(**i == "Normal" && default.is_some()))
            .filter_map(|i| builtin_style(i).map(str::to_string))
            .map(|x| match &default {
                Some(d) => x.replace("<w:basedOn w:val=\"Normal\"/>", &format!("<w:basedOn w:val=\"{d}\"/>")).replace("<w:next w:val=\"Normal\"/>", &format!("<w:next w:val=\"{d}\"/>")),
                None => x,
            })
            .collect();
        let _ = xml_text;
        self.tx_part(tx, &part, |root, _| {
            for s in snippets {
                root.children.push(Node::Raw(s.into_bytes()));
            }
        })?;
        Ok(())
    }

    // --- lists -------------------------------------------------------------------------------------

    /// The numbering part, created (empty) when the document has none.
    pub(crate) fn numbering_part(&mut self, tx: &mut Tx) -> Result<String, BusError> {
        if let Some(p) = self.pkg.related(&self.main, opc::REL_NUMBERING).first() {
            return Ok(p.clone());
        }
        let mut name = "/word/numbering.xml".to_string();
        let mut k = 2;
        while self.pkg.part(&name).is_some() {
            name = format!("/word/numbering{k}.xml");
            k += 1;
        }
        let body = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:numbering xmlns:w=\"{W_NS}\"></w:numbering>");
        self.pkg.add_part(&name, opc::CT_NUMBERING, body.into_bytes()).map_err(|e| BusError::failed(e.to_string()))?;
        let target = name.trim_start_matches("/word/").to_string();
        self.pkg.add_relationship(&self.main, opc::REL_NUMBERING, &target, false);
        tx.dirty = true;
        Ok(name)
    }

    /// Add a list definition (an abstract definition and a `w:num` for it); answers the new numId.
    fn add_list(&mut self, tx: &mut Tx, kind: &str, fmt: Option<&str>, text: Option<&str>) -> Result<i64, BusError> {
        let part = self.numbering_part(tx)?;
        let kind = kind.to_string();
        let (fmt, text) = (fmt.map(str::to_string), text.map(str::to_string));
        self.tx_part(tx, &part, move |root, _| {
            let w = prefix_of(root);
            let max_abs = root.children_named("abstractNum").filter_map(|a| dom::attr_local(a, "abstractNumId")?.parse::<i64>().ok()).max().unwrap_or(-1);
            let max_num = root.children_named("num").filter_map(|a| dom::attr_local(a, "numId")?.parse::<i64>().ok()).max().unwrap_or(0);
            let abs_id = max_abs + 1;
            let num_id = max_num + 1;
            let abs = abstract_list(&w, abs_id, &kind, fmt.as_deref(), text.as_deref());
            // Schema order: every abstractNum before the first num.
            let first_num = root.elements().position(|e| e.local() == "num" || e.local() == "numIdMacAtCleanup").unwrap_or(dom::element_count(root));
            dom::insert_at(root, first_num, vec![Node::Element(abs)]);
            let mut num = wel(&w, "num", &[("numId", &num_id.to_string())]);
            num.children.push(Node::Element(wel(&w, "abstractNumId", &[("val", &abs_id.to_string())])));
            let cleanup = root.elements().position(|e| e.local() == "numIdMacAtCleanup").unwrap_or(dom::element_count(root));
            dom::insert_at(root, cleanup, vec![Node::Element(num)]);
            num_id
        })
    }

    /// What kind of list a numId is (`bullet`, `number`), from its first level.
    pub(crate) fn list_kind(&self, num_id: i64) -> Option<&'static str> {
        let lvl = self.model.numbering.level(num_id, 0, &self.model.styles)?;
        Some(if lvl.fmt == "bullet" { "bullet" } else { "number" })
    }

    pub(crate) fn cmd_list(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let kind = cmd.args.get("kind").and_then(Json::as_str).unwrap_or("bullet").to_string();
        if !matches!(kind.as_str(), "bullet" | "number" | "multilevel" | "none") {
            return Err(BusError::bad_args(&cmd.name, "\"kind\" is bullet, number, multilevel or none"));
        }
        let fmt = cmd.args.get("format").and_then(Json::as_str).map(str::to_string);
        let text = cmd.args.get("text").and_then(Json::as_str).map(str::to_string);
        let flow = s.flow().clone();
        let paras = self.span_paras(&s);
        let current: Vec<Option<i64>> = paras.iter().map(|p| self.para_ref(&flow, *p).and_then(|r| self.eff_para(r.para).num_id).filter(|n| *n != 0)).collect();
        let want = if kind == "multilevel" { "number" } else { kind.as_str() };
        // Toggling off: every paragraph already in a list of this kind.
        let all_same = fmt.is_none() && kind != "none" && current.iter().all(|n| n.and_then(|n| self.list_kind(n)) == Some(want));
        let mut tx = Tx::default();
        let num_id: Option<i64> = if kind == "none" || all_same {
            None
        } else {
            // Continue the list just above when it is the same kind.
            let above = s.start.p.checked_sub(1).and_then(|p| self.para_ref(&flow, p)).and_then(|r| r.para.ppr.num_id).filter(|n| *n != 0);
            match above {
                Some(n) if fmt.is_none() && kind != "multilevel" && self.list_kind(n) == Some(want) => Some(n),
                _ => Some(self.add_list(&mut tx, &kind, fmt.as_deref(), text.as_deref())?),
            }
        };
        let paths: Vec<(Vec<usize>, u8)> = paras.iter().filter_map(|p| self.para_ref(&flow, *p).map(|r| (r.flat.path.clone(), r.para.ppr.ilvl.unwrap_or(0)))).collect();
        let part = self.part_name(&flow);
        let style_numbered: Vec<bool> = paras.iter().map(|p| self.para_ref(&flow, *p).is_some_and(|r| r.para.ppr.num_id.is_none() && self.eff_para(r.para).num_id.is_some_and(|n| n != 0))).collect();
        self.tx_part(&mut tx, &part, |root, _| {
            for (k, (path, lvl)) in paths.iter().enumerate() {
                let Some(p) = el_at_mut(root, path) else { continue };
                let w = prefix_of(p);
                let ppr = dom::ppr_mut(p, &w);
                match num_id {
                    Some(n) => {
                        let mut np = wel(&w, "numPr", &[]);
                        np.children.push(Node::Element(wel(&w, "ilvl", &[("val", &lvl.to_string())])));
                        np.children.push(Node::Element(wel(&w, "numId", &[("val", &n.to_string())])));
                        set_child(ppr, "numPr", Some(np), PPR_ORDER);
                    }
                    None => {
                        if style_numbered[k] {
                            // A style's numbering is switched off by numId 0.
                            let mut np = wel(&w, "numPr", &[]);
                            np.children.push(Node::Element(wel(&w, "numId", &[("val", "0")])));
                            set_child(ppr, "numPr", Some(np), PPR_ORDER);
                        } else {
                            set_child(ppr, "numPr", None, PPR_ORDER);
                        }
                    }
                }
                if ppr.children.is_empty() && ppr.attrs.is_empty() {
                    p.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "pPr"));
                }
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &s.start, &s.end);
        Ok(ch)
    }

    /// Demote (`delta` > 0) or promote list paragraphs one level.
    pub(crate) fn change_level(&mut self, cmd: &Command, s: &Span, delta: i32) -> Result<Change, BusError> {
        let flow = s.flow().clone();
        let mut work = Vec::new();
        for p in self.span_paras(s) {
            let Some(r) = self.para_ref(&flow, p) else { continue };
            let pp = self.eff_para(r.para);
            let Some(n) = pp.num_id.filter(|n| *n != 0) else { continue };
            let lvl = (pp.ilvl.unwrap_or(0) as i32 + delta).clamp(0, 8);
            work.push((r.flat.path.clone(), n, lvl));
        }
        let part = self.part_name(&flow);
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &part, |root, _| {
            for (path, n, lvl) in &work {
                let Some(p) = el_at_mut(root, path) else { continue };
                let w = prefix_of(p);
                let ppr = dom::ppr_mut(p, &w);
                let mut np = wel(&w, "numPr", &[]);
                np.children.push(Node::Element(wel(&w, "ilvl", &[("val", &lvl.to_string())])));
                np.children.push(Node::Element(wel(&w, "numId", &[("val", &n.to_string())])));
                set_child(ppr, "numPr", Some(np), PPR_ORDER);
            }
        })?;
        let ch = self.tx_commit(tx)?;
        let (a, b) = if s.backward { (s.end.clone(), s.start.clone()) } else { (s.start.clone(), s.end.clone()) };
        self.land_span(cmd, &a, &b);
        Ok(ch)
    }

    pub(crate) fn cmd_list_level(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let d = cmd.args.get("delta").and_then(Json::as_f64).unwrap_or(1.0);
        self.change_level(cmd, &s, if d >= 0.0 { 1 } else { -1 })
    }

    /// Restart numbering at this paragraph: a new `w:num` of the same definition with a start
    /// override, given to this paragraph and the rest of its list.
    pub(crate) fn cmd_list_restart(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let flow = s.flow().clone();
        let start = cmd.args.get("start").and_then(Json::as_i64).unwrap_or(1).max(0);
        let (num, lvl) = {
            let r = self.para_ref(&flow, s.start.p).ok_or_else(|| BusError::failed("no paragraph there"))?;
            let pp = self.eff_para(r.para);
            (pp.num_id.filter(|n| *n != 0).ok_or_else(|| BusError::failed("the paragraph is not in a list"))?, pp.ilvl.unwrap_or(0))
        };
        let mut tx = Tx::default();
        let part = self.numbering_part(&mut tx)?;
        let new_id = self.tx_part(&mut tx, &part, |root, _| {
            let w = prefix_of(root);
            let abs = root
                .children_named("num")
                .find(|e| dom::attr_local(e, "numId").and_then(|v| v.parse::<i64>().ok()) == Some(num))
                .and_then(|e| e.child("abstractNumId"))
                .and_then(|a| dom::attr_local(a, "val").map(str::to_string))
                .unwrap_or_else(|| "0".into());
            let max_num = root.children_named("num").filter_map(|a| dom::attr_local(a, "numId")?.parse::<i64>().ok()).max().unwrap_or(0);
            let id = max_num + 1;
            let mut n = wel(&w, "num", &[("numId", &id.to_string())]);
            n.children.push(Node::Element(wel(&w, "abstractNumId", &[("val", &abs)])));
            let mut o = wel(&w, "lvlOverride", &[("ilvl", &lvl.to_string())]);
            o.children.push(Node::Element(wel(&w, "startOverride", &[("val", &start.to_string())])));
            n.children.push(Node::Element(o));
            let cleanup = root.elements().position(|e| e.local() == "numIdMacAtCleanup").unwrap_or(dom::element_count(root));
            dom::insert_at(root, cleanup, vec![Node::Element(n)]);
            id
        })?;
        let mut paths = Vec::new();
        for p in s.start.p..self.flow_len(&flow) {
            let Some(r) = self.para_ref(&flow, p) else { break };
            if r.para.ppr.num_id == Some(num) {
                paths.push((r.flat.path.clone(), r.para.ppr.ilvl.unwrap_or(0)));
            }
        }
        let mpart = self.part_name(&flow);
        self.tx_part(&mut tx, &mpart, |root, _| {
            for (path, l) in &paths {
                let Some(p) = el_at_mut(root, path) else { continue };
                let w = prefix_of(p);
                let ppr = dom::ppr_mut(p, &w);
                let mut np = wel(&w, "numPr", &[]);
                np.children.push(Node::Element(wel(&w, "ilvl", &[("val", &l.to_string())])));
                np.children.push(Node::Element(wel(&w, "numId", &[("val", &new_id.to_string())])));
                set_child(ppr, "numPr", Some(np), PPR_ORDER);
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &s.start, &s.end);
        Ok(ch)
    }

    // --- find and replace --------------------------------------------------------------------------

    /// Every match of `query` in body paragraph `p`: `(start, end)` character offsets.
    pub(crate) fn matches_in(&self, p: usize, query: &[char], case: bool, whole: bool) -> Vec<(usize, usize)> {
        let fold = |c: char| if case { c } else { c.to_lowercase().next().unwrap_or(c) };
        let q: Vec<char> = query.iter().map(|c| fold(*c)).collect();
        let chars: Vec<char> = self.para_chars(&Flow::Body, p).into_iter().map(fold).collect();
        let mut out = Vec::new();
        if q.is_empty() || chars.len() < q.len() {
            return out;
        }
        let mut i = 0;
        while i + q.len() <= chars.len() {
            if chars[i..i + q.len()] == q[..] {
                let ok = !whole || ((i == 0 || !pos::is_word_char(chars[i - 1])) && (i + q.len() == chars.len() || !pos::is_word_char(chars[i + q.len()])));
                if ok {
                    out.push((i, i + q.len()));
                    i += q.len();
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    /// The next match of `query` after `from` (forward) or before it (backward), wrapping around the
    /// body; `(paragraph, start, end)`.
    pub(crate) fn find_from(&self, query: &[char], from: &SPos, forward: bool, case: bool, whole: bool) -> Option<(usize, usize, usize)> {
        let n = self.model.flat.len();
        if query.is_empty() || n == 0 {
            return None;
        }
        let (sp, so) = if from.flow == Flow::Body { (from.p.min(n - 1), from.o) } else { (0, 0) };
        for step in 0..=n {
            let p = if forward { (sp + step) % n } else { (sp + n * 2 - step) % n };
            let ms = self.matches_in(p, query, case, whole);
            let pick = match (forward, step) {
                (true, 0) => ms.into_iter().find(|(a, _)| *a >= so),
                (true, k) if k == n => ms.into_iter().find(|(a, _)| *a < so),
                (true, _) => ms.into_iter().next(),
                (false, 0) => ms.into_iter().rev().find(|(_, b)| *b <= so),
                (false, k) if k == n => ms.into_iter().rev().find(|(_, b)| *b > so),
                (false, _) => ms.into_iter().next_back(),
            };
            if let Some((a, b)) = pick {
                return Some((p, a, b));
            }
        }
        None
    }

    pub(crate) fn cmd_find(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let query: Vec<char> = cmd.args.get("query").and_then(Json::as_str).unwrap_or("").chars().collect();
        let forward = cmd.args.get("next").and_then(Json::as_bool).unwrap_or(true);
        let case = cmd.args.get("case").and_then(Json::as_bool).unwrap_or(false);
        let whole = cmd.args.get("whole").and_then(Json::as_bool).unwrap_or(false);
        let s = self.sel_or_start(cmd);
        let from = if forward { s.end.clone() } else { s.start.clone() };
        self.landed = None;
        if let Some((p, a, b)) = self.find_from(&query, &from, forward, case, whole) {
            self.land_span(cmd, &SPos::body(p, a), &SPos::body(p, b));
        }
        Ok(Change::default())
    }

    pub(crate) fn cmd_replace(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let query: Vec<char> = cmd.args.get("query").and_then(Json::as_str).unwrap_or("").chars().collect();
        if query.is_empty() {
            return Err(BusError::bad_args(&cmd.name, "takes { \"query\": string, \"with\": string, \"all\": bool, \"sel\" }"));
        }
        let with = cmd.args.get("with").and_then(Json::as_str).unwrap_or("").to_string();
        let all = cmd.args.get("all").and_then(Json::as_bool).unwrap_or(false);
        let case = cmd.args.get("case").and_then(Json::as_bool).unwrap_or(false);
        let whole = cmd.args.get("whole").and_then(Json::as_bool).unwrap_or(false);
        let s = self.sel_or_start(cmd);
        let fold = |c: char| if case { c } else { c.to_lowercase().next().unwrap_or(c) };
        let mut tx = Tx::default();
        self.landed = None;
        if all {
            // Every match, paragraph by paragraph, last first so offsets stay put.
            let mut per_para: Vec<(usize, Vec<(usize, usize)>)> = Vec::new();
            for p in 0..self.model.flat.len() {
                let ms = self.matches_in(p, &query, case, whole);
                if !ms.is_empty() {
                    per_para.push((p, ms));
                }
            }
            if per_para.is_empty() {
                return Ok(Change::default());
            }
            let mut work = Vec::new();
            for (p, list) in per_para {
                if let Some(r) = self.para_ref(&Flow::Body, p) {
                    work.push((r.flat.path.clone(), r.para.fields_in.clone(), list));
                }
            }
            let part = self.main.clone();
            let w2 = with.clone();
            self.tx_part(&mut tx, &part, |root, cx| {
                for (path, fi, mut list) in work {
                    list.sort();
                    let Some(p) = el_at_mut(root, &path) else { continue };
                    for (a, b) in list.into_iter().rev() {
                        replace_in(p, cx, &fi, a, b, &w2);
                    }
                }
            })?;
            let ch = self.tx_commit(tx)?;
            return Ok(ch);
        }
        // One: the selection when it is a match, then on to the next.
        let mut next_from = s.end.clone();
        if !s.is_caret() && s.start.p == s.end.p && s.start.flow == Flow::Body {
            let chars = self.para_chars(&Flow::Body, s.start.p);
            let sel: Vec<char> = chars[s.start.o.min(chars.len())..s.end.o.min(chars.len())].iter().map(|c| fold(*c)).collect();
            let q: Vec<char> = query.iter().map(|c| fold(*c)).collect();
            if sel == q {
                let r = self.para_ref(&Flow::Body, s.start.p).ok_or_else(|| BusError::failed("no paragraph there"))?;
                let (path, fi) = (r.flat.path.clone(), r.para.fields_in.clone());
                let (a, b) = (s.start.o, s.end.o);
                let part = self.main.clone();
                let w2 = with.clone();
                self.tx_part(&mut tx, &part, |root, cx| {
                    if let Some(p) = el_at_mut(root, &path) {
                        replace_in(p, cx, &fi, a, b, &w2);
                    }
                })?;
                next_from = s.start.at(a + dom::content_len(&with));
            }
        }
        let ch = self.tx_commit(tx)?;
        match self.find_from(&query, &next_from, true, case, whole) {
            Some((p, a, b)) => self.land_span(cmd, &SPos::body(p, a), &SPos::body(p, b)),
            None => self.land_caret(cmd, &next_from),
        }
        Ok(ch)
    }

    // --- paste -------------------------------------------------------------------------------------

    pub(crate) fn cmd_paste(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let rich = cmd.args.get("rich").filter(|r| r.get("scroll").is_some()).cloned();
        let text = cmd.args.get("text").and_then(Json::as_str).unwrap_or("").to_string();
        self.pending = None;
        let mut tx = Tx::default();
        let at = self.delete_span(&mut tx, &s)?;
        let land = match rich.as_ref().and_then(|r| r.get("paras")).and_then(Json::as_arr) {
            Some(paras) if !paras.is_empty() => self.paste_rich(&mut tx, &at, paras)?,
            _ => {
                let text = if text.is_empty() { cmd.args.get("html").and_then(Json::as_str).map(html_to_text).unwrap_or_default() } else { text };
                if text.is_empty() {
                    at.clone()
                } else {
                    self.insert_at(&mut tx, &at, &text, None)?
                }
            }
        };
        let ch = self.tx_commit(tx)?;
        self.land_caret(cmd, &land);
        Ok(ch)
    }

    /// Paste our own copy: runs keep their character formatting, whole paragraphs their paragraph
    /// properties.
    fn paste_rich(&mut self, tx: &mut Tx, at: &SPos, paras: &[Json]) -> Result<SPos, BusError> {
        let Some(r) = self.para_ref(&at.flow, at.p) else { return Ok(at.clone()) };
        let (part, path, fi) = (r.part.clone(), r.flat.path.clone(), r.para.fields_in.clone());
        let o = at.o;
        let count = paras.len();
        let parsed: Vec<(Option<Element>, Vec<(Option<Element>, String)>)> = paras
            .iter()
            .map(|p| {
                let ppr = p.get("ppr").and_then(Json::as_str).and_then(parse_snippet);
                let runs = p
                    .get("runs")
                    .and_then(Json::as_arr)
                    .unwrap_or(&[])
                    .iter()
                    .map(|r| (r.get("rpr").and_then(Json::as_str).and_then(parse_snippet), r.get("text").and_then(Json::as_str).unwrap_or("").to_string()))
                    .collect();
                (ppr, runs)
            })
            .collect();
        let last_len = self.tx_part(tx, &part, |root, cx| {
            let (cpath, idx) = split_path(&path);
            let Some(c) = el_at_mut(root, &cpath) else { return 0 };
            let mut cur = idx;
            let mut off = o;
            let mut last = 0;
            for (k, (ppr, runs)) in parsed.iter().enumerate() {
                let Some(p) = dom::child_at_mut(c, cur) else { break };
                let w = prefix_of(p);
                // Middle paragraphs (and a first one pasted at an empty paragraph) take their own
                // paragraph properties.
                if k > 0 && k + 1 < count || (count > 1 && k == 0 && off == 0) {
                    if let Some(pp) = ppr {
                        let sect = p.child("pPr").and_then(|x| x.child("sectPr")).cloned();
                        let mut pp = pp.clone();
                        set_child(&mut pp, "sectPr", sect, PPR_ORDER);
                        match p.child_mut("pPr") {
                            Some(x) => *x = pp,
                            None => p.children.insert(0, Node::Element(pp)),
                        }
                    }
                }
                for (rpr, text) in runs {
                    let rr = cx.runs(p, &fi);
                    let n = dom::insert_text(p, &rr, off, text, Some(rpr.clone().unwrap_or_else(|| wel(&w, "rPr", &[]))));
                    off += n;
                }
                last = off;
                if k + 1 < count {
                    let rr = cx.runs(p, &fi);
                    let right = dom::split_para(p, &rr, off);
                    dom::insert_at(c, cur + 1, vec![Node::Element(right)]);
                    cur += 1;
                    off = 0;
                }
            }
            last
        })?;
        Ok(at.para(at.p + count - 1, last_len))
    }
}

/// Replace characters `a..b` of a paragraph with text that takes the first replaced character's
/// formatting.
fn replace_in(p: &mut Element, cx: &Cx, fi: &[model::FieldState], a: usize, b: usize, with: &str) {
    let runs = cx.runs(p, fi);
    let n = dom::insert_text(p, &runs, a + 1, with, None);
    let runs = cx.runs(p, fi);
    dom::delete_range(p, &runs, a + 1 + n, b + n);
    let runs = cx.runs(p, fi);
    dom::delete_range(p, &runs, a, a + 1);
}

/// A clipboard `w:rPr` / `w:pPr` snippet (prefixes as written) read back.
pub(crate) fn parse_snippet(s: &str) -> Option<Element> {
    let wrapped = format!(
        "<x xmlns:w=\"{W_NS}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\">{s}</x>"
    );
    let root = xml::parse_dom(wrapped.as_bytes()).ok()?;
    let el = root.elements().next()?.clone();
    matches!(el.local(), "rPr" | "pPr").then_some(el)
}

/// The text of a pasted HTML fragment (tags dropped, block ends as new paragraphs, entities read).
pub(crate) fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let b = html.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            let end = html[i..].find('>').map(|k| i + k).unwrap_or(b.len() - 1);
            let tag = html[i + 1..end].trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("").to_ascii_lowercase();
            let closing = html[i + 1..].starts_with('/');
            if matches!(tag.as_str(), "br") || (closing && matches!(tag.as_str(), "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "tr")) {
                out.push('\n');
            } else if closing && matches!(tag.as_str(), "td" | "th") {
                out.push('\t');
            }
            if tag == "style" || tag == "script" {
                if !closing {
                    let close = format!("</{tag}");
                    if let Some(k) = html[end..].to_ascii_lowercase().find(&close) {
                        i = end + k;
                        continue;
                    }
                }
            }
            i = end + 1;
            continue;
        }
        if b[i] == b'&' {
            if let Some(k) = html[i..].find(';').filter(|k| *k < 10) {
                let ent = &html[i + 1..i + k];
                let ch = match ent {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    _ => ent.strip_prefix('#').and_then(|n| if let Some(h) = n.strip_prefix('x') { u32::from_str_radix(h, 16).ok() } else { n.parse().ok() }).and_then(char::from_u32),
                };
                if let Some(c) = ch {
                    out.push(c);
                    i += k + 1;
                    continue;
                }
            }
        }
        let ch = html[i..].chars().next().unwrap_or(' ');
        if ch == '\n' || ch == '\r' {
            out.push(' ');
        } else {
            out.push(ch);
        }
        i += ch.len_utf8();
    }
    let mut lines: Vec<String> = out.split('\n').map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

pub(crate) const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// A list definition with nine levels.
fn abstract_list(w: &str, id: i64, kind: &str, fmt: Option<&str>, text: Option<&str>) -> Element {
    let mut a = wel(w, "abstractNum", &[("abstractNumId", &id.to_string())]);
    a.children.push(Node::Element(wel(w, "multiLevelType", &[("val", if kind == "multilevel" { "multilevel" } else { "hybridMultilevel" })])));
    const BULLETS: [&str; 3] = ["\u{2022}", "\u{25E6}", "\u{25AA}"];
    const NUMS: [&str; 3] = ["decimal", "lowerLetter", "lowerRoman"];
    for l in 0..9usize {
        let mut lvl = wel(w, "lvl", &[("ilvl", &l.to_string())]);
        lvl.children.push(Node::Element(wel(w, "start", &[("val", "1")])));
        let (nf, lt) = match kind {
            "bullet" => ("bullet".to_string(), BULLETS[l % 3].to_string()),
            "multilevel" => ("decimal".to_string(), (1..=l + 1).map(|k| format!("%{k}")).collect::<Vec<_>>().join(".") + "."),
            _ => (NUMS[l % 3].to_string(), format!("%{}.", l + 1)),
        };
        let (nf, lt) = if l == 0 { (fmt.map(str::to_string).unwrap_or(nf), text.map(str::to_string).unwrap_or(lt)) } else { (nf, lt) };
        lvl.children.push(Node::Element(wel(w, "numFmt", &[("val", &nf)])));
        lvl.children.push(Node::Element(wel(w, "lvlText", &[("val", &lt)])));
        lvl.children.push(Node::Element(wel(w, "lvlJc", &[("val", "left")])));
        let mut ppr = wel(w, "pPr", &[]);
        let left = 720 * (l + 1);
        let hang = if kind == "multilevel" { 360 + 72 * l } else { 360 };
        ppr.children.push(Node::Element(wel(w, "ind", &[("left", &left.to_string()), ("hanging", &hang.to_string())])));
        lvl.children.push(Node::Element(ppr));
        a.children.push(Node::Element(lvl));
    }
    a
}

/// A built-in style id for a name the document does not have yet.
fn builtin_style_id(name: &str) -> Option<String> {
    let n = name.to_ascii_lowercase().replace([' ', '-', '_'], "");
    let id = match n.as_str() {
        "normal" => "Normal",
        "title" => "Title",
        "subtitle" => "Subtitle",
        "heading1" => "Heading1",
        "heading2" => "Heading2",
        "heading3" => "Heading3",
        "heading4" => "Heading4",
        "quote" => "Quote",
        "listparagraph" => "ListParagraph",
        "hyperlink" => "Hyperlink",
        "strong" => "Strong",
        "emphasis" => "Emphasis",
        "header" => "Header",
        "footer" => "Footer",
        "tablegrid" => "TableGrid",
        _ => return None,
    };
    Some(id.to_string())
}

fn builtin_is_char(id: &str) -> bool {
    matches!(id, "Hyperlink" | "Strong" | "Emphasis")
}

fn builtin_based_on(id: &str) -> Option<&'static str> {
    match id {
        "Hyperlink" | "Strong" | "Emphasis" => None,
        "Normal" | "TableGrid" => None,
        _ => Some("Normal"),
    }
}

/// The XML of a built-in style.
pub(crate) fn builtin_style(id: &str) -> Option<&'static str> {
    Some(match id {
        "Normal" => "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>",
        "Title" => "<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/><w:contextualSpacing/></w:pPr><w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/><w:kern w:val=\"28\"/><w:sz w:val=\"56\"/><w:szCs w:val=\"56\"/></w:rPr></w:style>",
        "Subtitle" => "<w:style w:type=\"paragraph\" w:styleId=\"Subtitle\"><w:name w:val=\"Subtitle\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:rPr><w:color w:val=\"5A5A5A\"/><w:spacing w:val=\"15\"/><w:sz w:val=\"28\"/></w:rPr></w:style>",
        "Heading1" => "<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"240\" w:after=\"0\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/><w:color w:val=\"2F5496\"/><w:sz w:val=\"32\"/><w:szCs w:val=\"32\"/></w:rPr></w:style>",
        "Heading2" => "<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"40\" w:after=\"0\"/><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/><w:color w:val=\"2F5496\"/><w:sz w:val=\"26\"/><w:szCs w:val=\"26\"/></w:rPr></w:style>",
        "Heading3" => "<w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"40\" w:after=\"0\"/><w:outlineLvl w:val=\"2\"/></w:pPr><w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/><w:color w:val=\"1F3763\"/><w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/></w:rPr></w:style>",
        "Heading4" => "<w:style w:type=\"paragraph\" w:styleId=\"Heading4\"><w:name w:val=\"heading 4\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"40\" w:after=\"0\"/><w:outlineLvl w:val=\"3\"/></w:pPr><w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/><w:i/><w:color w:val=\"2F5496\"/></w:rPr></w:style>",
        "Quote" => "<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:before=\"200\"/><w:ind w:left=\"864\" w:right=\"864\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr><w:i/><w:color w:val=\"404040\"/></w:rPr></w:style>",
        "ListParagraph" => "<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:pPr><w:ind w:left=\"720\"/><w:contextualSpacing/></w:pPr></w:style>",
        "Header" => "<w:style w:type=\"paragraph\" w:styleId=\"Header\"><w:name w:val=\"header\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:tabs><w:tab w:val=\"center\" w:pos=\"4680\"/><w:tab w:val=\"right\" w:pos=\"9360\"/></w:tabs><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:style>",
        "Footer" => "<w:style w:type=\"paragraph\" w:styleId=\"Footer\"><w:name w:val=\"footer\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:tabs><w:tab w:val=\"center\" w:pos=\"4680\"/><w:tab w:val=\"right\" w:pos=\"9360\"/></w:tabs><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:style>",
        "Hyperlink" => "<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>",
        "Strong" => "<w:style w:type=\"character\" w:styleId=\"Strong\"><w:name w:val=\"Strong\"/><w:qFormat/><w:rPr><w:b/><w:bCs/></w:rPr></w:style>",
        "Emphasis" => "<w:style w:type=\"character\" w:styleId=\"Emphasis\"><w:name w:val=\"Emphasis\"/><w:qFormat/><w:rPr><w:i/><w:iCs/></w:rPr></w:style>",
        "TableGrid" => "<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tblBorders></w:tblPr></w:style>",
        _ => return None,
    })
}

/// Read an rPr from the clipboard into a run's format quickly (for tests and the AI).
#[allow(dead_code)]
pub(crate) fn rpr_of(model: &model::Model, el: &Element) -> RunProps {
    model::parse_rpr(model, el)
}

