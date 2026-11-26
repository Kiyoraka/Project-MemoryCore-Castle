//! The hand edits' XML work: shapes found and patched by id, new shapes, pictures and tables written in
//! PresentationML's own shape, the stacking order, groups, alignment and the slide list. Each function
//! only changes the deck; `mod.rs` keeps the undo (a snapshot of the parts) and redraws.

use std::collections::HashSet;

use crate::ground::bus::json::Json;
use crate::ground::bus::BusError;
use crate::ground::display::{Matrix, Rect};
use crate::ground::draw::Xfrm;
use crate::ground::opc::{self, name_key};
use crate::ground::xml::{self, escape_attr, escape_text, Element, Node};

use super::edit::{el_at, el_at_mut, find_shape, ShapeKey};
use super::model::{rel_attr, shape_id, shape_name, sp_tree, Deck, Ph, REL_NOTES_SLIDE};
use super::render::{decode_image, MEDIUM_STYLE_2_ACCENT_1};
use super::text::{put_ordered, solid_fill, Body, FILLS, LN_ORDER, SPPR_ORDER, TCPR_ORDER};

pub const EMU: f64 = 12700.0;

pub fn emu(pt: f64) -> i64 {
    (pt * EMU).round() as i64
}

pub fn failed(m: impl Into<String>) -> BusError {
    BusError::failed(m)
}

const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";

/// The prefixes a slide binds PresentationML, DrawingML and relationships to.
#[derive(Debug, Clone)]
pub struct Ns {
    pub p: String,
    pub a: String,
    pub r: String,
}

impl Ns {
    pub fn of(root: &Element) -> Ns {
        let find = |ns: &str, d: &str| {
            root.attrs.iter().find(|(k, v)| v == ns && k.starts_with("xmlns:")).map(|(k, _)| k[6..].to_string()).unwrap_or_else(|| d.to_string())
        };
        Ns { p: root.prefix().map(str::to_string).unwrap_or_else(|| find(NS_P, "p")), a: find(NS_A, "a"), r: find(NS_R, "r") }
    }

    /// Make sure the root declares the three prefixes.
    pub fn declare(&self, root: &mut Element) {
        for (pre, ns) in [(&self.a, NS_A), (&self.r, NS_R), (&self.p, NS_P)] {
            let k = format!("xmlns:{pre}");
            let bound = root.attr(&k).is_some() || (root.prefix() == Some(pre.as_str()) && root.attr("xmlns").is_some());
            if !bound {
                root.attrs.push((k, ns.to_string()));
            }
        }
    }

    /// Write XML in the conventional prefixes (`p:`, `a:`, `r:`) into this slide's prefixes.
    pub fn fix(&self, xml: &str) -> String {
        if self.p == "p" && self.a == "a" && self.r == "r" {
            return xml.to_string();
        }
        xml.replace("<p:", &format!("<{}:", self.p))
            .replace("</p:", &format!("</{}:", self.p))
            .replace("<a:", &format!("<{}:", self.a))
            .replace("</a:", &format!("</{}:", self.a))
            .replace(" r:", &format!(" {}:", self.r))
    }
}

/// Parse an element written with the conventional prefixes.
pub fn fragment(xml: &str) -> Result<Element, BusError> {
    let doc = format!(r#"<w xmlns:a="{NS_A}" xmlns:r="{NS_R}" xmlns:p="{NS_P}">{xml}</w>"#);
    let root = xml::parse_dom(doc.as_bytes()).map_err(|e| failed(format!("bad shape XML: {e}")))?;
    let first = root.elements().next().cloned();
    first.ok_or_else(|| failed("empty shape XML"))
}

/// The part of slide `i`.
pub fn slide_part(deck: &Deck, slide: usize) -> Result<String, BusError> {
    deck.slides
        .get(slide)
        .map(|s| s.part.clone())
        .ok_or_else(|| failed(format!("no slide {} (the deck has {} slide{})", slide, deck.slides.len(), if deck.slides.len() == 1 { "" } else { "s" })))
}

/// Patch slide `i`'s XML. `f` checks first and changes after: an error leaves the part as it was.
pub fn with_slide<R>(deck: &mut Deck, slide: usize, f: impl FnOnce(&mut Element) -> Result<R, BusError>) -> Result<R, BusError> {
    let part = slide_part(deck, slide)?;
    let mut out: Option<Result<R, BusError>> = None;
    let mut keep = true;
    let before = deck.pkg.part_bytes(&part).map_err(|e| failed(e.to_string()))?;
    deck.pkg
        .edit_xml(&part, |root| {
            let r = f(root);
            keep = r.is_ok();
            out = Some(r);
        })
        .map_err(|e| failed(e.to_string()))?;
    if !keep {
        let _ = deck.pkg.set_part_bytes(&part, before);
    }
    deck.reload_part(&part);
    out.unwrap_or_else(|| Err(failed("the slide could not be read")))
}

/// The shape tree of a slide root, mutable.
pub fn tree_mut(root: &mut Element) -> Option<&mut Element> {
    root.child_mut("cSld")?.child_mut("spTree")
}

pub fn shape_path(root: &Element, id: u32) -> Result<Vec<usize>, BusError> {
    find_shape(root, &ShapeKey::Id(id)).ok_or_else(|| failed(format!("there is no shape {id} on the slide")))
}

pub fn shape_mut(root: &mut Element, id: u32) -> Result<&mut Element, BusError> {
    let path = shape_path(root, id)?;
    el_at_mut(root, &path).ok_or_else(|| failed("shape not found"))
}

pub fn is_shape(e: &Element) -> bool {
    matches!(e.local(), "sp" | "pic" | "grpSp" | "graphicFrame" | "cxnSp")
}

/// Every cNvPr id in a tree.
fn ids_in(e: &Element, out: &mut HashSet<u32>) {
    if e.local() == "cNvPr" {
        if let Some(id) = e.attr("id").and_then(|s| s.trim().parse().ok()) {
            out.insert(id);
        }
    }
    for c in e.elements() {
        ids_in(c, out);
    }
}

/// The next unused shape id on a slide.
pub fn next_id(root: &Element) -> u32 {
    let mut ids = HashSet::new();
    ids_in(root, &mut ids);
    ids.into_iter().max().unwrap_or(1) + 1
}

/// A name like "TextBox 4" that no shape on the slide has.
pub fn fresh_name(root: &Element, base: &str, id: u32) -> String {
    let mut names = HashSet::new();
    if let Some(t) = sp_tree(root) {
        for s in super::model::all_shapes(t) {
            names.insert(shape_name(s));
        }
    }
    let mut n = id.saturating_sub(1).max(1);
    loop {
        let cand = format!("{base} {n}");
        if !names.contains(&cand) {
            return cand;
        }
        n += 1;
    }
}

/// Insert a shape at the top of the slide's stacking order (before the tree's extension list).
pub fn push_shape(root: &mut Element, shape: Element) -> Result<(), BusError> {
    let tree = tree_mut(root).ok_or_else(|| failed("the slide has no shape tree"))?;
    let at = tree.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "extLst")).unwrap_or(tree.children.len());
    tree.children.insert(at, Node::Element(shape));
    Ok(())
}

fn xfrm_xml(x: f64, y: f64, w: f64, h: f64) -> String {
    format!(r#"<a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#, emu(x), emu(y), emu(w.max(0.0)), emu(h.max(0.0)))
}

fn paras_xml(text: &str, ppr: &str) -> String {
    let mut s = String::new();
    for line in text.replace("\r\n", "\n").split('\n') {
        s.push_str("<a:p>");
        s.push_str(ppr);
        if !line.is_empty() {
            s.push_str(&format!(r#"<a:r><a:rPr lang="en-US" dirty="0"/><a:t>{}</a:t></a:r>"#, escape_text(line)));
        } else {
            s.push_str(r#"<a:endParaRPr lang="en-US" dirty="0"/>"#);
        }
        s.push_str("</a:p>");
    }
    s
}

/// A new text box.
pub fn text_box_xml(id: u32, name: &str, r: Rect, text: &str) -> String {
    format!(
        concat!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr>"#,
            r#"<p:spPr>{xfrm}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>"#,
            r#"<p:txBody><a:bodyPr wrap="square" rtlCol="0"><a:spAutoFit/></a:bodyPr><a:lstStyle/>{paras}</p:txBody></p:sp>"#
        ),
        id = id,
        name = escape_attr(name),
        xfrm = xfrm_xml(r.x, r.y, r.w, r.h),
        paras = paras_xml(text, "")
    )
}

/// A new preset shape with the theme's look (accent 1 fill, a darker outline, light text).
pub fn preset_xml(id: u32, name: &str, preset: &str, r: Rect, text: &str) -> String {
    format!(
        concat!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>"#,
            r#"<p:spPr>{xfrm}<a:prstGeom prst="{preset}"><a:avLst/></a:prstGeom></p:spPr>"#,
            r#"<p:style><a:lnRef idx="2"><a:schemeClr val="accent1"><a:shade val="15000"/></a:schemeClr></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef>"#,
            r#"<a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></p:style>"#,
            r#"<p:txBody><a:bodyPr rtlCol="0" anchor="ctr"/><a:lstStyle/>{paras}</p:txBody></p:sp>"#
        ),
        id = id,
        name = escape_attr(name),
        xfrm = xfrm_xml(r.x, r.y, r.w, r.h),
        preset = escape_attr(preset),
        paras = paras_xml(text, r#"<a:pPr algn="ctr"/>"#)
    )
}

/// A line end type as PresentationML names it.
pub fn end_type(s: &str) -> Option<&'static str> {
    Some(match s {
        "none" => "none",
        "triangle" | "arrow_filled" => "triangle",
        "arrow" | "open" => "arrow",
        "stealth" => "stealth",
        "oval" | "circle" => "oval",
        "diamond" => "diamond",
        _ => return None,
    })
}

/// A new straight line from (x1, y1) to (x2, y2) with optional arrowheads.
pub fn line_xml(id: u32, name: &str, x1: f64, y1: f64, x2: f64, y2: f64, head: Option<&str>, tail: Option<&str>) -> String {
    let (x, y, w, h) = (x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs());
    let flips = format!("{}{}", if x2 < x1 { r#" flipH="1""# } else { "" }, if y2 < y1 { r#" flipV="1""# } else { "" });
    let ends = [("headEnd", head), ("tailEnd", tail)]
        .iter()
        .filter_map(|(n, t)| t.and_then(end_type).filter(|t| *t != "none").map(|t| format!(r#"<a:{n} type="{t}"/>"#)))
        .collect::<String>();
    let ln = if ends.is_empty() { String::new() } else { format!(r#"<a:ln>{ends}</a:ln>"#) };
    format!(
        concat!(
            r#"<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr>"#,
            r#"<p:spPr><a:xfrm{flips}><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom>{ln}</p:spPr>"#,
            r#"<p:style><a:lnRef idx="1"><a:schemeClr val="accent1"/></a:lnRef><a:fillRef idx="0"><a:schemeClr val="accent1"/></a:fillRef>"#,
            r#"<a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="tx1"/></a:fontRef></p:style></p:cxnSp>"#
        ),
        id = id,
        name = escape_attr(name),
        flips = flips,
        x = emu(x),
        y = emu(y),
        w = emu(w),
        h = emu(h),
        ln = ln
    )
}

/// A new picture shape over relationship `rid`.
pub fn picture_xml(id: u32, name: &str, descr: &str, rid: &str, r: Rect, crop: Option<(f64, f64, f64, f64)>) -> String {
    let src = match crop {
        Some((l, t, rr, b)) if l + t + rr + b > 1e-6 => {
            let p = |v: f64| (v * 100000.0).round() as i64;
            let mut s = String::from("<a:srcRect");
            for (k, v) in [("l", l), ("t", t), ("r", rr), ("b", b)] {
                if v.abs() > 1e-9 {
                    s.push_str(&format!(r#" {k}="{}""#, p(v)));
                }
            }
            s + "/>"
        }
        _ => String::new(),
    };
    format!(
        concat!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="{name}" descr="{descr}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>"#,
            r#"<p:blipFill><a:blip r:embed="{rid}"/>{src}<a:stretch><a:fillRect/></a:stretch></p:blipFill>"#,
            r#"<p:spPr>{xfrm}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#
        ),
        id = id,
        name = escape_attr(name),
        descr = escape_attr(descr),
        rid = rid,
        src = src,
        xfrm = xfrm_xml(r.x, r.y, r.w, r.h)
    )
}

const EMPTY_CELL: &str = r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US" dirty="0"/></a:p></a:txBody><a:tcPr/></a:tc>"#;

/// A new table of `rows` x `cols` empty cells in the default table style.
pub fn table_xml(id: u32, name: &str, rows: usize, cols: usize, r: Rect) -> String {
    let cw = emu(r.w / cols as f64);
    let rh = emu((r.h / rows as f64).max(1.0));
    let grid: String = (0..cols).map(|_| format!(r#"<a:gridCol w="{cw}"/>"#)).collect();
    let row = format!(r#"<a:tr h="{rh}">{}</a:tr>"#, EMPTY_CELL.repeat(cols));
    format!(
        concat!(
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="{name}"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>"#,
            r#"<p:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></p:xfrm>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="1" bandRow="1"><a:tableStyleId>{style}</a:tableStyleId></a:tblPr>"#,
            r#"<a:tblGrid>{grid}</a:tblGrid>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#
        ),
        id = id,
        name = escape_attr(name),
        x = emu(r.x),
        y = emu(r.y),
        w = cw * cols as i64,
        h = rh * rows as i64,
        style = MEDIUM_STYLE_2_ACCENT_1,
        grid = grid,
        rows = row.repeat(rows)
    )
}

// ---------------------------------------------------------------------------------------------------
// Text bodies
// ---------------------------------------------------------------------------------------------------

/// The `a:tbl` of a graphic frame, mutable.
pub fn table_mut(frame: &mut Element) -> Option<&mut Element> {
    frame.child_mut("graphic")?.child_mut("graphicData")?.child_mut("tbl")
}

/// A table cell (row, column), mutable.
pub fn cell_mut(frame: &mut Element, r: usize, c: usize) -> Option<&mut Element> {
    let tbl = table_mut(frame)?;
    let tr = tbl.children.iter_mut().filter_map(|n| match n {
        Node::Element(e) if e.local() == "tr" => Some(e),
        _ => None,
    });
    let tr = tr.into_iter().nth(r)?;
    tr.children
        .iter_mut()
        .filter_map(|n| match n {
            Node::Element(e) if e.local() == "tc" => Some(e),
            _ => None,
        })
        .nth(c)
}

/// The text body of a shape (or of one of its table cells), made when there is none.
pub fn tx_mut<'a>(shape: &'a mut Element, cell: Option<(usize, usize)>, ns: &Ns) -> Result<&'a mut Element, BusError> {
    let (host, name): (&mut Element, String) = match cell {
        Some((r, c)) => (cell_mut(shape, r, c).ok_or_else(|| failed(format!("the table has no cell ({r}, {c})")))?, format!("{}:txBody", ns.a)),
        None => {
            if shape.local() != "sp" {
                return Err(failed(format!("shape '{}' holds no text of its own", shape_name(shape))));
            }
            let p = shape.prefix().unwrap_or(&ns.p).to_string();
            (shape, format!("{p}:txBody"))
        }
    };
    if host.child("txBody").is_none() {
        let mut tx = Element::new(name);
        tx.children.push(Node::Element(Element::new(format!("{}:bodyPr", ns.a))));
        tx.children.push(Node::Element(Element::new(format!("{}:lstStyle", ns.a))));
        let mut p = Element::new(format!("{}:p", ns.a));
        let mut end = Element::new(format!("{}:endParaRPr", ns.a));
        end.attrs.push(("lang".into(), "en-US".into()));
        end.attrs.push(("dirty".into(), "0".into()));
        p.children.push(Node::Element(end));
        tx.children.push(Node::Element(p));
        // txBody goes before tcPr in a cell, before extLst in a shape.
        let at = host.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "tcPr" || e.local() == "extLst")).unwrap_or(host.children.len());
        host.children.insert(at, Node::Element(tx));
    }
    host.child_mut("txBody").ok_or_else(|| failed("no text body"))
}

/// Read, edit and write back a shape's text.
pub fn edit_body<R>(deck: &mut Deck, slide: usize, id: u32, cell: Option<(usize, usize)>, f: impl FnOnce(&mut Body) -> R) -> Result<R, BusError> {
    with_slide(deck, slide, |root| {
        let ns = Ns::of(root);
        let sp = shape_mut(root, id)?;
        let tx = tx_mut(sp, cell, &ns)?;
        let mut body = Body::read(tx);
        let r = f(&mut body);
        body.write_into(tx);
        Ok(r)
    })
}

/// A shape's text body as read (an empty one when it has none).
pub fn read_body(deck: &Deck, slide: usize, id: u32, cell: Option<(usize, usize)>) -> Option<Body> {
    let root = deck.sheet(&deck.slides.get(slide)?.part)?;
    let path = find_shape(root, &ShapeKey::Id(id))?;
    let sp = el_at(root, &path)?;
    let host = match cell {
        Some((r, c)) => super::model::table_of(sp)?.children_named("tr").nth(r)?.children_named("tc").nth(c)?,
        None => sp,
    };
    Some(match host.child("txBody") {
        Some(tx) => Body::read(tx),
        None => Body::read(&super::render::empty_tx()),
    })
}

/// The `a:bodyPr` of a shape, made when there is none.
pub fn body_pr_mut<'a>(shape: &'a mut Element, ns: &Ns) -> Result<&'a mut Element, BusError> {
    let tx = tx_mut(shape, None, ns)?;
    if tx.child("bodyPr").is_none() {
        tx.children.insert(0, Node::Element(Element::new(format!("{}:bodyPr", ns.a))));
    }
    tx.child_mut("bodyPr").ok_or_else(|| failed("no body properties"))
}

// ---------------------------------------------------------------------------------------------------
// Shape properties
// ---------------------------------------------------------------------------------------------------

/// The `p:spPr` (or `p:grpSpPr`) of a shape, made when there is none.
pub fn sp_pr_mut<'a>(shape: &'a mut Element, ns: &Ns) -> Result<&'a mut Element, BusError> {
    let local = if shape.local() == "grpSp" { "grpSpPr" } else { "spPr" };
    if shape.local() == "graphicFrame" {
        return Err(failed("a table or chart frame has no shape properties"));
    }
    if shape.child(local).is_none() {
        let p = shape.prefix().unwrap_or(&ns.p).to_string();
        let at = shape.children.iter().position(|c| matches!(c, Node::Element(e) if e.local().starts_with("nv"))).map(|i| i + 1).unwrap_or(0);
        let at = if shape.local() == "pic" {
            shape.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "blipFill")).map(|i| i + 1).unwrap_or(at)
        } else {
            at
        };
        shape.children.insert(at, Node::Element(Element::new(format!("{p}:{local}"))));
    }
    shape.child_mut(local).ok_or_else(|| failed("no shape properties"))
}

/// The shape's own transform element (`a:xfrm` in spPr, `p:xfrm` of a frame), if written.
pub fn own_xfrm(shape: &Element) -> Option<&Element> {
    if shape.local() == "graphicFrame" {
        shape.child("xfrm")
    } else {
        shape.child(if shape.local() == "grpSp" { "grpSpPr" } else { "spPr" }).and_then(|s| s.child("xfrm"))
    }
}

/// Write a transform into the shape (keeping a group's child offset and extent).
pub fn set_xfrm(shape: &mut Element, x: &Xfrm, ns: &Ns) -> Result<(), BusError> {
    let a = ns.a.clone();
    let host: &mut Element = if shape.local() == "graphicFrame" {
        shape
    } else {
        sp_pr_mut(shape, ns)?
    };
    let frame = host.local() == "graphicFrame";
    let name = if frame { format!("{}:xfrm", host.prefix().unwrap_or(&ns.p)) } else { format!("{a}:xfrm") };
    let mut e = Element::new(name);
    if x.rot != 0 {
        e.attrs.push(("rot".into(), x.rot.to_string()));
    }
    if x.flip_h {
        e.attrs.push(("flipH".into(), "1".into()));
    }
    if x.flip_v {
        e.attrs.push(("flipV".into(), "1".into()));
    }
    let pair = |n: &str, k1: &str, v1: i64, k2: &str, v2: i64| {
        let mut c = Element::new(format!("{a}:{n}"));
        c.attrs.push((k1.into(), v1.to_string()));
        c.attrs.push((k2.into(), v2.to_string()));
        Node::Element(c)
    };
    e.children.push(pair("off", "x", x.off.0, "y", x.off.1));
    e.children.push(pair("ext", "cx", x.ext.0.max(0), "cy", x.ext.1.max(0)));
    if let (Some(o), Some(c)) = (x.ch_off, x.ch_ext) {
        e.children.push(pair("chOff", "x", o.0, "y", o.1));
        e.children.push(pair("chExt", "cx", c.0, "cy", c.1));
    }
    // Keep the old element when it says the same (byte-exact).
    if let Some(old) = host.child("xfrm") {
        if Xfrm::parse(old) == *x {
            return Ok(());
        }
    }
    if frame {
        let at = host.children.iter().position(|c| matches!(c, Node::Element(el) if el.local() == "xfrm"));
        host.children.retain(|c| !matches!(c, Node::Element(el) if el.local() == "xfrm"));
        let at = at.unwrap_or_else(|| host.children.iter().position(|c| matches!(c, Node::Element(el) if el.local() == "graphic")).unwrap_or(host.children.len()));
        host.children.insert(at, Node::Element(e));
    } else {
        put_ordered(host, Some(e), &["xfrm"], SPPR_ORDER);
    }
    Ok(())
}

/// Set (or clear, with `None`) the solid fill of a shape.
pub fn set_fill(shape: &mut Element, hex: Option<&str>, alpha: Option<f64>, ns: &Ns) -> Result<(), BusError> {
    let a = ns.a.clone();
    let sp = sp_pr_mut(shape, ns)?;
    let fill = match hex {
        Some(h) => solid_fill(&a, h, alpha),
        None => Element::new(format!("{a}:noFill")),
    };
    put_ordered(sp, Some(fill), FILLS, SPPR_ORDER);
    Ok(())
}

/// The outline: colour (`Some(None)` for no line), width in points, dash, arrowheads.
pub struct Outline<'a> {
    pub color: Option<Option<&'a str>>,
    pub width: Option<f64>,
    pub dash: Option<&'a str>,
    pub head: Option<&'a str>,
    pub tail: Option<&'a str>,
}

pub fn set_outline(shape: &mut Element, o: &Outline, ns: &Ns) -> Result<(), BusError> {
    let a = ns.a.clone();
    let sp = sp_pr_mut(shape, ns)?;
    if sp.child("ln").is_none() {
        put_ordered(sp, Some(Element::new(format!("{a}:ln"))), &[], SPPR_ORDER);
    }
    let ln = sp.child_mut("ln").ok_or_else(|| failed("no line"))?;
    if let Some(w) = o.width {
        ln.set_attr("w", emu(w.clamp(0.0, 1584.0)).to_string());
    }
    if let Some(c) = o.color {
        let f = match c {
            Some(h) => solid_fill(&a, h, None),
            None => Element::new(format!("{a}:noFill")),
        };
        put_ordered(ln, Some(f), &["noFill", "solidFill", "gradFill", "pattFill"], LN_ORDER);
    }
    if let Some(d) = o.dash {
        let mut e = Element::new(format!("{a}:prstDash"));
        e.attrs.push(("val".into(), d.to_string()));
        put_ordered(ln, Some(e), &["prstDash", "custDash"], LN_ORDER);
    }
    for (n, v) in [("headEnd", o.head), ("tailEnd", o.tail)] {
        if let Some(t) = v.and_then(end_type) {
            let mut e = Element::new(format!("{a}:{n}"));
            e.attrs.push(("type".into(), t.to_string()));
            put_ordered(ln, Some(e), &[n], LN_ORDER);
        }
    }
    Ok(())
}

/// The fill of a table cell (`None`: no fill).
pub fn set_cell_fill(cell: &mut Element, hex: Option<&str>, ns: &Ns) {
    let a = ns.a.clone();
    if cell.child("tcPr").is_none() {
        cell.children.push(Node::Element(Element::new(format!("{a}:tcPr"))));
    }
    if let Some(pr) = cell.child_mut("tcPr") {
        let f = match hex {
            Some(h) => solid_fill(&a, h, None),
            None => Element::new(format!("{a}:noFill")),
        };
        put_ordered(pr, Some(f), FILLS, TCPR_ORDER);
    }
}

// ---------------------------------------------------------------------------------------------------
// Stacking order, groups, moving
// ---------------------------------------------------------------------------------------------------

/// The shape-element indices of a tree or group (child positions in `children`).
fn shape_slots(parent: &Element) -> Vec<usize> {
    parent.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(e) if is_shape(e) || e.local() == "AlternateContent")).map(|(i, _)| i).collect()
}

/// The parent of a shape (its path without the last step) and its element index there.
fn parent_and_slot(root: &mut Element, id: u32) -> Result<(&mut Element, usize), BusError> {
    let path = shape_path(root, id)?;
    let (last, up) = path.split_last().ok_or_else(|| failed("shape not found"))?;
    let parent = el_at_mut(root, up).ok_or_else(|| failed("shape not found"))?;
    let slot = parent.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(_))).nth(*last).map(|(i, _)| i).ok_or_else(|| failed("shape not found"))?;
    Ok((parent, slot))
}

/// Move a shape in the stacking order: `front`, `back`, `forward`, `backward`.
pub fn reorder(root: &mut Element, id: u32, to: &str) -> Result<(), BusError> {
    let (parent, slot) = parent_and_slot(root, id)?;
    let slots = shape_slots(parent);
    let k = slots.iter().position(|&s| s == slot).ok_or_else(|| failed("shape not found"))?;
    let target = match to {
        "front" => slots.len() - 1,
        "back" => 0,
        "forward" => (k + 1).min(slots.len() - 1),
        "backward" => k.saturating_sub(1),
        other => return Err(BusError::bad_args("stage.order", format!("\"to\" is front, back, forward or backward, not {other}"))),
    };
    if target == k {
        return Ok(());
    }
    let node = parent.children.remove(slot);
    let slots = shape_slots(parent);
    let at = if target >= slots.len() {
        slots.last().map(|s| s + 1).unwrap_or(parent.children.len())
    } else if target > k {
        slots[target - 1] + 1
    } else {
        slots[target]
    };
    parent.children.insert(at, node);
    Ok(())
}

/// Remove a shape; returns its element.
pub fn remove_shape(root: &mut Element, id: u32) -> Result<Element, BusError> {
    let (parent, slot) = parent_and_slot(root, id)?;
    match parent.children.remove(slot) {
        Node::Element(e) => Ok(e),
        _ => Err(failed("shape not found")),
    }
}

/// Give every cNvPr in `e` a new id from `next` (counting up); returns the first.
pub fn renumber(e: &mut Element, next: &mut u32) {
    if e.local() == "cNvPr" {
        e.set_attr("id", next.to_string());
        *next += 1;
    }
    for c in &mut e.children {
        if let Node::Element(el) = c {
            renumber(el, next);
        }
    }
}

/// The bounding box (slide points) of an unrotated transform.
pub fn xfrm_box(x: &Xfrm) -> Rect {
    x.rect()
}

/// Group shapes (all on the slide's tree): returns the group's id.
pub fn group(root: &mut Element, ids: &[u32], boxes: &[(u32, Xfrm)], name: Option<&str>, gid: Option<u32>) -> Result<u32, BusError> {
    if ids.len() < 2 {
        return Err(failed("select two or more shapes to group"));
    }
    let ns = Ns::of(root);
    let tree_path = {
        let mut p = None;
        for id in ids {
            let path = shape_path(root, *id)?;
            let up = path[..path.len() - 1].to_vec();
            if p.as_ref().is_some_and(|q: &Vec<usize>| *q != up) {
                return Err(failed("the shapes to group must sit side by side (not some inside a group)"));
            }
            p = Some(up);
        }
        p.unwrap_or_default()
    };
    // Materialise transforms first (placeholders may inherit theirs).
    for (id, x) in boxes {
        let sp = shape_mut(root, *id)?;
        if own_xfrm(sp).is_none() {
            set_xfrm(sp, x, &ns)?;
        }
    }
    let mut bb: Option<Rect> = None;
    for (_, x) in boxes {
        let r = x.rect();
        bb = Some(bb.map_or(r, |b| b.union(&r)));
    }
    let bb = bb.ok_or_else(|| failed("nothing to group"))?;
    let gid = gid.unwrap_or_else(|| next_id(root));
    let gname = name.map(str::to_string).unwrap_or_else(|| fresh_name(root, "Group", gid));
    let parent = el_at_mut(root, &tree_path).ok_or_else(|| failed("no shape tree"))?;
    // Members in their stacking order; the group takes the topmost member's place.
    let mut members = Vec::new();
    let mut place = None;
    let mut i = 0;
    while i < parent.children.len() {
        let hit = matches!(&parent.children[i], Node::Element(e) if is_shape(e) && shape_id(e).is_some_and(|s| ids.contains(&s)));
        if hit {
            if let Node::Element(e) = parent.children.remove(i) {
                members.push(e);
            }
            place = Some(i);
        } else {
            i += 1;
        }
    }
    let at = place.unwrap_or(parent.children.len());
    let (x, y, w, h) = (emu(bb.x), emu(bb.y), emu(bb.w), emu(bb.h));
    let shell = ns.fix(&format!(
        r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="{gid}" name="{}"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/><a:chOff x="{x}" y="{y}"/><a:chExt cx="{w}" cy="{h}"/></a:xfrm></p:grpSpPr></p:grpSp>"#,
        escape_attr(&gname)
    ));
    let mut g = fragment(&shell)?;
    for m in members {
        g.children.push(Node::Element(m));
    }
    parent.children.insert(at.min(parent.children.len()), Node::Element(g));
    Ok(gid)
}

/// Ungroup: the children take the group's place in the parent's space. Returns (name, member ids).
pub fn ungroup(root: &mut Element, id: u32) -> Result<(String, Vec<u32>), BusError> {
    let ns = Ns::of(root);
    let (parent, slot) = parent_and_slot(root, id)?;
    let g = match &parent.children[slot] {
        Node::Element(e) if e.local() == "grpSp" => e.clone(),
        _ => return Err(failed("that shape is not a group")),
    };
    let gx = g.child("grpSpPr").and_then(|p| p.child("xfrm")).map(Xfrm::parse).unwrap_or_default();
    let cm = gx.child_matrix();
    let grot = gx.rot;
    let mut kids = Vec::new();
    let mut ids = Vec::new();
    for c in g.elements().filter(|e| is_shape(e)) {
        let mut k = c.clone();
        if let Some(x) = own_xfrm(&k).map(Xfrm::parse) {
            let r = x.rect();
            let (cx, cy) = cm.apply(r.x + r.w / 2.0, r.y + r.h / 2.0);
            let sx = if let Some(ce) = gx.ch_ext { if ce.0 != 0 { gx.ext.0 as f64 / ce.0 as f64 } else { 1.0 } } else { 1.0 };
            let sy = if let Some(ce) = gx.ch_ext { if ce.1 != 0 { gx.ext.1 as f64 / ce.1 as f64 } else { 1.0 } } else { 1.0 };
            let (w, h) = (r.w * sx, r.h * sy);
            let mut nx = x;
            nx.off = (emu(cx - w / 2.0), emu(cy - h / 2.0));
            nx.ext = (emu(w), emu(h));
            nx.rot = (x.rot + grot).rem_euclid(21_600_000);
            if gx.flip_h {
                nx.flip_h = !nx.flip_h;
            }
            if gx.flip_v {
                nx.flip_v = !nx.flip_v;
            }
            if k.local() == "grpSp" {
                nx.ch_off = x.ch_off.or(Some(x.off));
                nx.ch_ext = x.ch_ext.or(Some(x.ext));
            }
            set_xfrm(&mut k, &nx, &ns)?;
        }
        if let Some(i) = shape_id(&k) {
            ids.push(i);
        }
        kids.push(Node::Element(k));
    }
    parent.children.remove(slot);
    for (j, k) in kids.into_iter().enumerate() {
        parent.children.insert(slot + j, k);
    }
    Ok((shape_name(&g), ids))
}

/// The matrix of a shape's parent space (slide points from the parent's coordinates), from its drawn
/// transform `m` and its own transform.
pub fn parent_matrix(m: &Matrix, own: &Xfrm) -> Matrix {
    match own.matrix().invert() {
        Some(inv) => m.multiply(&inv),
        None => Matrix::IDENTITY,
    }
}

/// Move a shape by (dx, dy) slide points, given its parent space.
pub fn move_by(x: &mut Xfrm, dx: f64, dy: f64, parent: &Matrix) {
    let lin = Matrix::new(parent.a, parent.b, parent.c, parent.d, 0.0, 0.0);
    let (lx, ly) = lin.invert().map(|i| i.apply(dx, dy)).unwrap_or((dx, dy));
    x.off.0 += emu(lx);
    x.off.1 += emu(ly);
}

// ---------------------------------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------------------------------

fn tbl_rows(tbl: &mut Element) -> Vec<usize> {
    tbl.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(e) if e.local() == "tr")).map(|(i, _)| i).collect()
}

/// An empty copy of a cell (its paragraph and run properties kept, the text gone, merges cleared).
fn emptied(tc: &Element) -> Element {
    let mut c = tc.clone();
    c.attrs.retain(|(k, _)| !matches!(k.as_str(), "gridSpan" | "rowSpan" | "hMerge" | "vMerge"));
    if let Some(tx) = c.child_mut("txBody") {
        let mut b = Body::read(tx);
        let end = b.end();
        b.delete(super::text::At::new(0, 0), end);
        b.write_into(tx);
    }
    c
}

/// Insert a row (`after`: below `row`, else above) or a column; delete a row or column.
pub fn table_change(frame: &mut Element, what: &str, index: usize, insert: bool, after: bool) -> Result<(), BusError> {
    let tbl = table_mut(frame).ok_or_else(|| failed("that shape is not a table"))?;
    match (what, insert) {
        ("row", true) => {
            let rows = tbl_rows(tbl);
            let src = *rows.get(index).ok_or_else(|| failed(format!("the table has no row {index}")))?;
            let Node::Element(tr) = &tbl.children[src] else { return Err(failed("bad row")) };
            let mut copy = tr.clone();
            copy.children = copy
                .children
                .iter()
                .map(|n| match n {
                    Node::Element(e) if e.local() == "tc" => Node::Element(emptied(e)),
                    other => other.clone(),
                })
                .collect();
            tbl.children.insert(if after { src + 1 } else { src }, Node::Element(copy));
        }
        ("row", false) => {
            let rows = tbl_rows(tbl);
            if rows.len() <= 1 {
                return Err(failed("a table keeps at least one row (delete the table instead)"));
            }
            let src = *rows.get(index).ok_or_else(|| failed(format!("the table has no row {index}")))?;
            tbl.children.remove(src);
        }
        ("col", true) | ("column", true) => {
            let grid = tbl.child_mut("tblGrid").ok_or_else(|| failed("the table has no grid"))?;
            let cols: Vec<usize> = grid.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(e) if e.local() == "gridCol")).map(|(i, _)| i).collect();
            let src = *cols.get(index).ok_or_else(|| failed(format!("the table has no column {index}")))?;
            let gc = grid.children[src].clone();
            grid.children.insert(if after { src + 1 } else { src }, gc);
            for ri in tbl_rows(tbl) {
                let Node::Element(tr) = &mut tbl.children[ri] else { continue };
                let tcs: Vec<usize> = tr.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(e) if e.local() == "tc")).map(|(i, _)| i).collect();
                if let Some(&s) = tcs.get(index) {
                    let Node::Element(tc) = &tr.children[s] else { continue };
                    let e = emptied(tc);
                    tr.children.insert(if after { s + 1 } else { s }, Node::Element(e));
                }
            }
        }
        ("col", false) | ("column", false) => {
            let grid = tbl.child_mut("tblGrid").ok_or_else(|| failed("the table has no grid"))?;
            let cols: Vec<usize> = grid.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(e) if e.local() == "gridCol")).map(|(i, _)| i).collect();
            if cols.len() <= 1 {
                return Err(failed("a table keeps at least one column (delete the table instead)"));
            }
            let src = *cols.get(index).ok_or_else(|| failed(format!("the table has no column {index}")))?;
            grid.children.remove(src);
            for ri in tbl_rows(tbl) {
                let Node::Element(tr) = &mut tbl.children[ri] else { continue };
                let tcs: Vec<usize> = tr.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(e) if e.local() == "tc")).map(|(i, _)| i).collect();
                if let Some(&s) = tcs.get(index) {
                    tr.children.remove(s);
                }
            }
        }
        _ => return Err(failed(format!("unknown table change {what}"))),
    }
    fit_frame(frame);
    Ok(())
}

/// Set a column's width or a row's height (points).
pub fn table_size(frame: &mut Element, col: Option<(usize, f64)>, row: Option<(usize, f64)>) -> Result<(), BusError> {
    let tbl = table_mut(frame).ok_or_else(|| failed("that shape is not a table"))?;
    if let Some((c, w)) = col {
        let grid = tbl.child_mut("tblGrid").ok_or_else(|| failed("the table has no grid"))?;
        let gc = grid
            .children
            .iter_mut()
            .filter_map(|n| match n {
                Node::Element(e) if e.local() == "gridCol" => Some(e),
                _ => None,
            })
            .nth(c)
            .ok_or_else(|| failed(format!("the table has no column {c}")))?;
        gc.set_attr("w", emu(w.max(1.0)).to_string());
    }
    if let Some((r, h)) = row {
        let tr = tbl
            .children
            .iter_mut()
            .filter_map(|n| match n {
                Node::Element(e) if e.local() == "tr" => Some(e),
                _ => None,
            })
            .nth(r)
            .ok_or_else(|| failed(format!("the table has no row {r}")))?;
        tr.set_attr("h", emu(h.max(1.0)).to_string());
    }
    fit_frame(frame);
    Ok(())
}

/// The frame's extent follows its grid and rows.
fn fit_frame(frame: &mut Element) {
    let Some(tbl) = table_mut(frame) else { return };
    let w: i64 = tbl.child("tblGrid").map(|g| g.children_named("gridCol").filter_map(|c| c.attr("w").and_then(|v| v.trim().parse::<i64>().ok())).sum()).unwrap_or(0);
    let h: i64 = tbl.children_named("tr").filter_map(|r| r.attr("h").and_then(|v| v.trim().parse::<i64>().ok())).sum();
    if let Some(ext) = frame.child_mut("xfrm").and_then(|x| x.child_mut("ext")) {
        if w > 0 {
            ext.set_attr("cx", w.to_string());
        }
        if h > 0 {
            ext.set_attr("cy", h.to_string());
        }
    }
}

/// (rows, columns) of a table frame.
pub fn table_dims(frame: &Element) -> (usize, usize) {
    let Some(tbl) = super::model::table_of(frame) else { return (0, 0) };
    let rows = tbl.children_named("tr").count();
    let cols = tbl.child("tblGrid").map(|g| g.children_named("gridCol").count()).unwrap_or(0);
    (rows, cols)
}

// ---------------------------------------------------------------------------------------------------
// Pictures and media
// ---------------------------------------------------------------------------------------------------

/// What a picture's bytes are: (extension, content type).
pub fn sniff_image(bytes: &[u8], name: &str) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some(("png", "image/png"));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return Some(("jpeg", "image/jpeg"));
    }
    if bytes.starts_with(b"GIF8") {
        return Some(("gif", "image/gif"));
    }
    if bytes.starts_with(b"BM") {
        return Some(("bmp", "image/bmp"));
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    if head.contains("<svg") || name.to_ascii_lowercase().ends_with(".svg") {
        return Some(("svg", "image/svg+xml"));
    }
    None
}

/// A picture's size in points (96 pixels to the inch), when we can read it.
pub fn image_size_pt(bytes: &[u8]) -> Option<(f64, f64)> {
    let img = decode_image(bytes)?;
    Some((img.width as f64 * 0.75, img.height as f64 * 0.75))
}

/// Add a media part (a fresh `/ppt/media/imageN.ext`) and a relationship to it from `part`; returns
/// the relationship id.
pub fn add_media(deck: &mut Deck, part: &str, bytes: Vec<u8>, ext: &str, ct: &str, rel_type: &str) -> Result<String, BusError> {
    let name = (1..).map(|n| format!("/ppt/media/image{n}.{ext}")).find(|n| deck.pkg.part(n).is_none()).unwrap_or_default();
    if deck.pkg.content_types().default_for(ext).is_none() {
        deck.pkg.set_default_content_type(ext, ct);
    }
    deck.pkg.add_part(&name, ct, bytes).map_err(|e| failed(e.to_string()))?;
    Ok(deck.pkg.add_relationship(part, rel_type, &name, false))
}

/// The relationship ids a shape's XML refers to (`r:embed`, `r:link`, `r:id`, ...).
pub fn rids_in(e: &Element, out: &mut Vec<String>) {
    for (k, v) in &e.attrs {
        if k.contains(':') && !k.starts_with("xmlns") && matches!(k.rsplit(':').next(), Some("embed" | "link" | "id" | "pict" | "dm" | "lo" | "qs" | "cs")) && v.starts_with("rId") && !out.contains(v) {
            out.push(v.clone());
        }
    }
    for c in e.elements() {
        rids_in(c, out);
    }
}

/// Rename relationship ids in a shape (`map` old -> new).
pub fn remap_rids(e: &mut Element, map: &[(String, String)]) {
    for (k, v) in &mut e.attrs {
        if k.contains(':') && !k.starts_with("xmlns") {
            if let Some((_, n)) = map.iter().find(|(o, _)| o == v) {
                *v = n.clone();
            }
        }
    }
    for c in &mut e.children {
        if let Node::Element(el) = c {
            remap_rids(el, map);
        }
    }
}

/// A shape and the parts its relationships point at, for a copy that may go to another deck:
/// `{"xml", "rels": [{"id","type","name","ct","b64"|"target","external"}]}`.
pub fn shape_package(deck: &Deck, part: &str, shape: &Element, raw: String) -> Json {
    let mut rids = Vec::new();
    rids_in(shape, &mut rids);
    let mut rels = Vec::new();
    for rid in rids {
        let Some(rel) = deck.pkg.rel_by_id(part, &rid) else { continue };
        let mut j = Json::obj().with("id", rid.as_str()).with("type", rel.rel_type.as_str());
        if rel.target_mode_external {
            j.set("external", true);
            j.set("target", rel.target.as_str());
        } else {
            let name = deck.pkg.resolve(part, rel);
            let Ok(b) = deck.pkg.part_bytes(&name) else { continue };
            j.set("name", name.as_str());
            j.set("ct", deck.pkg.content_type(&name).unwrap_or("application/octet-stream"));
            j.set("b64", super::snap::b64_encode(&b));
        }
        rels.push(j);
    }
    Json::obj().with("xml", raw).with("rels", Json::Arr(rels))
}

/// Bring a copied shape's related parts into this deck for `part`: returns the old -> new id map.
pub fn adopt_rels(deck: &mut Deck, part: &str, rels: &[Json]) -> Vec<(String, String)> {
    let mut map = Vec::new();
    for r in rels {
        let (Some(id), Some(ty)) = (r.get("id").and_then(Json::as_str), r.get("type").and_then(Json::as_str)) else { continue };
        let new = if r.get("external").and_then(Json::as_bool) == Some(true) {
            deck.pkg.add_relationship(part, ty, r.get("target").and_then(Json::as_str).unwrap_or(""), true)
        } else {
            let Some(bytes) = r.get("b64").and_then(Json::as_str).and_then(super::snap::b64_decode) else { continue };
            let ct = r.get("ct").and_then(Json::as_str).unwrap_or("application/octet-stream");
            let src = r.get("name").and_then(Json::as_str).unwrap_or("/ppt/media/image.bin");
            // The same bytes already in the deck are reused.
            let existing = deck.pkg.part_names().into_iter().find(|n| name_key(n) == name_key(src) && deck.pkg.part_bytes(n).ok().as_deref() == Some(&bytes[..]));
            match existing {
                Some(n) => deck.pkg.add_relationship(part, ty, &n, false),
                None => {
                    let ext = src.rsplit('.').next().unwrap_or("bin").to_ascii_lowercase();
                    match add_media(deck, part, bytes, &ext, ct, ty) {
                        Ok(id) => id,
                        Err(_) => continue,
                    }
                }
            }
        };
        map.push((id.to_string(), new));
    }
    map
}

// ---------------------------------------------------------------------------------------------------
// Slides
// ---------------------------------------------------------------------------------------------------

/// Every layout of every master: (layout part, name, master part).
pub fn layouts(deck: &Deck) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for m in deck.pkg.related(&deck.pres_part, opc::REL_SLIDE_MASTER) {
        for l in deck.pkg.related(&m, opc::REL_SLIDE_LAYOUT) {
            let name = deck
                .pkg
                .part_bytes(&l)
                .ok()
                .and_then(|b| xml::parse_dom(&b).ok())
                .and_then(|r| r.child("cSld").and_then(|c| c.attr("name")).map(str::to_string))
                .unwrap_or_default();
            out.push((l, name, m.clone()));
        }
    }
    out
}

/// A layout by name (case-insensitive) or index.
pub fn find_layout(deck: &Deck, want: &Json, fallback: Option<&str>) -> Result<String, BusError> {
    let all = layouts(deck);
    if let Some(i) = want.as_usize() {
        return all.get(i).map(|l| l.0.clone()).ok_or_else(|| failed(format!("there is no layout {i}; the layouts are: {}", names(&all))));
    }
    if let Some(n) = want.as_str() {
        return all
            .iter()
            .find(|l| l.1.eq_ignore_ascii_case(n.trim()))
            .map(|l| l.0.clone())
            .ok_or_else(|| failed(format!("there is no layout '{n}'; the layouts are: {}", names(&all))));
    }
    fallback.map(str::to_string).or_else(|| all.first().map(|l| l.0.clone())).ok_or_else(|| failed("the deck has no slide layouts"))
}

fn names(all: &[(String, String, String)]) -> String {
    all.iter().map(|l| l.1.clone()).collect::<Vec<_>>().join(", ")
}

const TREE_HEAD: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;

/// The placeholders a new slide on `layout` starts with (empty, inheriting everything).
pub fn placeholders_of(deck: &Deck, layout: &str) -> String {
    let Some(root) = deck.pkg.part_bytes(layout).ok().and_then(|b| xml::parse_dom(&b).ok()) else { return String::new() };
    let Some(tree) = sp_tree(&root) else { return String::new() };
    let mut s = String::new();
    for sh in super::model::shape_children(tree) {
        let Some(ph) = Ph::of(sh) else { continue };
        if matches!(ph.ty.as_str(), "dt" | "ftr" | "sldNum" | "hdr") || sh.local() != "sp" {
            continue;
        }
        let id = shape_id(sh).unwrap_or(2);
        let mut ph_attrs = String::new();
        if ph.ty != "obj" {
            ph_attrs.push_str(&format!(r#" type="{}""#, escape_attr(&ph.ty)));
        }
        if let Some(i) = ph.idx {
            ph_attrs.push_str(&format!(r#" idx="{i}""#));
        }
        s.push_str(&format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph{ph_attrs}/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US" dirty="0"/></a:p></p:txBody></p:sp>"#,
            escape_attr(&shape_name(sh))
        ));
    }
    s
}

/// A slide part's XML holding `tree` (shape XML).
pub fn slide_xml(tree: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<p:sld xmlns:a=\"{NS_A}\" xmlns:r=\"{NS_R}\" xmlns:p=\"{NS_P}\"><p:cSld><p:spTree>{TREE_HEAD}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
    )
}

/// Write a relationships part from a list (ids kept exactly).
pub fn rels_xml(list: &[(String, String, String, bool)]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for (id, ty, target, ext) in list {
        s.push_str(&format!(
            r#"<Relationship Id="{}" Type="{}" Target="{}"{}/>"#,
            escape_attr(id),
            escape_attr(ty),
            escape_attr(target),
            if *ext { r#" TargetMode="External""# } else { "" }
        ));
    }
    s + "</Relationships>"
}

/// Add a slide part (with its relationships written as given) to the presentation at `index`.
pub fn insert_slide_part(deck: &mut Deck, index: usize, xml_bytes: Vec<u8>, rels: Vec<(String, String, String, bool)>) -> Result<String, BusError> {
    let name = (1..).map(|n| format!("/ppt/slides/slide{n}.xml")).find(|n| deck.pkg.part(n).is_none()).unwrap_or_default();
    deck.pkg.add_part(&name, opc::CT_SLIDE, xml_bytes).map_err(|e| failed(e.to_string()))?;
    let rels_name = opc::rels_part_name(&name);
    if !rels.is_empty() {
        deck.pkg.add_part(&rels_name, opc::CT_RELATIONSHIPS, rels_xml(&rels).into_bytes()).map_err(|e| failed(e.to_string()))?;
    }
    let pres = deck.pres_part.clone();
    let rid = deck.pkg.add_relationship(&pres, opc::REL_SLIDE, &name, false);
    let mut err = None;
    deck.pkg
        .edit_xml(&pres, |root| {
            let ns = Ns::of(root);
            let p = root.prefix().unwrap_or("p").to_string();
            if root.child("sldIdLst").is_none() {
                let at = root.children.iter().position(|c| matches!(c, Node::Element(e) if matches!(e.local(), "sldSz" | "notesSz" | "smartTags" | "embeddedFontLst" | "custShowLst" | "photoAlbum" | "custDataLst" | "kinsoku" | "defaultTextStyle" | "modifyVerifier" | "extLst"))).unwrap_or(root.children.len());
                root.children.insert(at, Node::Element(Element::new(format!("{p}:sldIdLst"))));
            }
            let Some(list) = root.child_mut("sldIdLst") else {
                err = Some(failed("no slide list"));
                return;
            };
            let max = list.children_named("sldId").filter_map(|e| e.attr("id").and_then(|v| v.trim().parse::<u32>().ok())).max().unwrap_or(255);
            let mut id = Element::new(format!("{p}:sldId"));
            id.attrs.push(("id".into(), (max + 1).max(256).to_string()));
            id.attrs.push((format!("{}:id", ns.r), rid.clone()));
            let slots: Vec<usize> = list.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(e) if e.local() == "sldId")).map(|(i, _)| i).collect();
            let at = slots.get(index).copied().unwrap_or_else(|| slots.last().map(|s| s + 1).unwrap_or(0));
            list.children.insert(at, Node::Element(id));
        })
        .map_err(|e| failed(e.to_string()))?;
    if let Some(e) = err {
        return Err(e);
    }
    Ok(name)
}

/// Move slide `from` to position `to` in the slide list.
pub fn move_slide(deck: &mut Deck, from: usize, to: usize) -> Result<(), BusError> {
    let pres = deck.pres_part.clone();
    deck.pkg
        .edit_xml(&pres, |root| {
            let Some(list) = root.child_mut("sldIdLst") else { return };
            let slots: Vec<usize> = list.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(e) if e.local() == "sldId")).map(|(i, _)| i).collect();
            let (Some(&f), true) = (slots.get(from), to < slots.len()) else { return };
            let node = list.children.remove(f);
            let slots: Vec<usize> = list.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(e) if e.local() == "sldId")).map(|(i, _)| i).collect();
            let at = slots.get(to).copied().unwrap_or_else(|| slots.last().map(|s| s + 1).unwrap_or(0));
            list.children.insert(at, node);
        })
        .map_err(|e| failed(e.to_string()))?;
    Ok(())
}

/// Remove slide `i` (and its notes page) from the deck.
pub fn delete_slide(deck: &mut Deck, i: usize) -> Result<(), BusError> {
    let entry = deck.slides.get(i).cloned().ok_or_else(|| failed(format!("no slide {i}")))?;
    let pres = deck.pres_part.clone();
    let rid = deck.pkg.rels(&pres).iter().find(|r| name_key(&deck.pkg.resolve(&pres, r)) == name_key(&entry.part)).map(|r| r.id.clone());
    if let Some(rid) = &rid {
        deck.pkg
            .edit_xml(&pres, |root| {
                if let Some(list) = root.child_mut("sldIdLst") {
                    list.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "sldId" && rel_attr(e, "id") == Some(rid.as_str())));
                }
            })
            .map_err(|e| failed(e.to_string()))?;
    }
    if let Some(n) = &entry.notes {
        deck.pkg.remove_part(n);
    }
    deck.pkg.remove_part(&entry.part);
    Ok(())
}

/// The relationships of a part as (id, type, target as written, external).
pub fn rel_list(deck: &Deck, part: &str) -> Vec<(String, String, String, bool)> {
    deck.pkg.rels(part).iter().map(|r| (r.id.clone(), r.rel_type.clone(), r.target.clone(), r.target_mode_external)).collect()
}

/// Duplicate slide `i` right after itself (its notes page too); returns the new slide's index.
pub fn duplicate_slide(deck: &mut Deck, i: usize) -> Result<usize, BusError> {
    let entry = deck.slides.get(i).cloned().ok_or_else(|| failed(format!("no slide {i}")))?;
    let bytes = deck.pkg.part_bytes(&entry.part).map_err(|e| failed(e.to_string()))?;
    let mut rels: Vec<(String, String, String, bool)> = rel_list(deck, &entry.part).into_iter().filter(|r| !opc::rel_type_matches(&r.1, REL_NOTES_SLIDE)).collect();
    let name = insert_slide_part(deck, i + 1, bytes, Vec::new())?;
    if let Some(n) = &entry.notes {
        let nb = deck.pkg.part_bytes(n).map_err(|e| failed(e.to_string()))?;
        let nname = (1..).map(|k| format!("/ppt/notesSlides/notesSlide{k}.xml")).find(|k| deck.pkg.part(k).is_none()).unwrap_or_default();
        deck.pkg.add_part(&nname, super::model::CT_NOTES_SLIDE, nb).map_err(|e| failed(e.to_string()))?;
        let nrels: Vec<(String, String, String, bool)> = rel_list(deck, n)
            .into_iter()
            .map(|(id, ty, t, ext)| if opc::rel_type_matches(&ty, opc::REL_SLIDE) { (id, ty, opc::relative_target(&nname, &name), ext) } else { (id, ty, t, ext) })
            .collect();
        deck.pkg.add_part(&opc::rels_part_name(&nname), opc::CT_RELATIONSHIPS, rels_xml(&nrels).into_bytes()).map_err(|e| failed(e.to_string()))?;
        let used: HashSet<String> = rels.iter().map(|r| r.0.clone()).collect();
        let nid = (1..).map(|k| format!("rId{k}")).find(|k| !used.contains(k)).unwrap_or_default();
        rels.push((nid, REL_NOTES_SLIDE.to_string(), opc::relative_target(&name, &nname), false));
    }
    deck.pkg.add_part(&opc::rels_part_name(&name), opc::CT_RELATIONSHIPS, rels_xml(&rels).into_bytes()).map_err(|e| failed(e.to_string()))?;
    Ok(i + 1)
}

/// Point slide `i` at another layout; placeholders the new layout lacks keep their place (their
/// position written on the slide), and the new layout's missing placeholders are added empty.
pub fn set_layout(deck: &mut Deck, i: usize, layout: &str, boxes: &[(u32, Xfrm)]) -> Result<(), BusError> {
    let entry = deck.slides.get(i).cloned().ok_or_else(|| failed(format!("no slide {i}")))?;
    let lroot = deck.pkg.part_bytes(layout).ok().and_then(|b| xml::parse_dom(&b).ok()).ok_or_else(|| failed("the layout cannot be read"))?;
    let ltree = sp_tree(&lroot).ok_or_else(|| failed("the layout has no shapes"))?.clone();
    let new_phs = placeholders_of(deck, layout);
    with_slide(deck, i, |root| {
        let ns = Ns::of(root);
        let tree = sp_tree(root).ok_or_else(|| failed("no shape tree"))?;
        let mut keep_place = Vec::new();
        let mut have = Vec::new();
        for s in super::model::all_shapes(tree) {
            if let Some(ph) = Ph::of(s) {
                let matched = super::model::find_layout_ph(&ltree, &ph).is_some();
                if !matched && own_xfrm(s).is_none() {
                    if let Some(id) = shape_id(s) {
                        keep_place.push(id);
                    }
                }
                have.push(ph);
            }
        }
        for id in keep_place {
            if let Some((_, x)) = boxes.iter().find(|(b, _)| *b == id) {
                let sp = shape_mut(root, id)?;
                set_xfrm(sp, x, &ns)?;
            }
        }
        // Missing placeholders of the new layout.
        let wrapper = fragment(&format!("<p:spTree>{}</p:spTree>", new_phs))?;
        for sh in wrapper.elements() {
            let Some(ph) = Ph::of(sh) else { continue };
            let present = have.iter().any(|h| (h.idx.is_some() && h.idx == ph.idx) || (h.master_type() == ph.master_type() && matches!(ph.ty.as_str(), "title" | "ctrTitle")));
            if present {
                continue;
            }
            let mut e = sh.clone();
            let mut next = next_id(root);
            renumber(&mut e, &mut next);
            let e = fragment(&ns.fix(&String::from_utf8_lossy(&xml::write_dom(&e))))?;
            push_shape(root, e)?;
        }
        Ok(())
    })?;
    let old = deck.pkg.rels(&entry.part).iter().find(|r| opc::rel_type_matches(&r.rel_type, opc::REL_SLIDE_LAYOUT)).map(|r| r.id.clone());
    if let Some(id) = old {
        deck.pkg.remove_relationship(&entry.part, &id);
    }
    deck.pkg.add_relationship(&entry.part, opc::REL_SLIDE_LAYOUT, layout, false);
    Ok(())
}
