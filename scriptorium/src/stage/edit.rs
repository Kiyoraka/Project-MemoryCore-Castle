//! Edits: each one patches a part with `opc::patch_xml` semantics (everything it did not touch is
//! copied byte for byte) and hands back the inverse. An inverse restores the exact original bytes of
//! what it replaced, so undo gives back the part as it was.

use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Command};
use crate::ground::opc::{self, name_key};
use crate::ground::xml::{self, Element, Event, Node, Reader};

use super::blank;
use super::model::{shape_id, shape_name, Deck, Ph, CT_NOTES_MASTER, CT_NOTES_SLIDE, REL_NOTES_MASTER, REL_NOTES_SLIDE};

/// Which shape a command means: its name, or its id (a number, or a string of digits that is not a
/// shape's name).
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeKey {
    Name(String),
    Id(u32),
}

impl ShapeKey {
    pub fn from_json(v: &Json) -> Option<ShapeKey> {
        if let Some(n) = v.as_i64() {
            return u32::try_from(n).ok().map(ShapeKey::Id);
        }
        v.as_str().map(|s| ShapeKey::Name(s.to_string()))
    }
}

fn is_shape(e: &Element) -> bool {
    matches!(e.local(), "sp" | "pic" | "grpSp" | "graphicFrame" | "cxnSp")
}

fn search(tree: &Element, pred: &dyn Fn(&Element) -> bool, path: &mut Vec<usize>) -> Option<Vec<usize>> {
    for (i, e) in tree.elements().enumerate() {
        path.push(i);
        if is_shape(e) && pred(e) {
            return Some(path.clone());
        }
        if e.local() == "grpSp" {
            if let Some(p) = search(e, pred, path) {
                return Some(p);
            }
        }
        path.pop();
    }
    None
}

/// The element path (child-element indices from the root) of the first shape `pred` accepts.
pub fn find_shape_path(root: &Element, pred: &dyn Fn(&Element) -> bool) -> Option<Vec<usize>> {
    let (ci, csld) = root.elements().enumerate().find(|(_, e)| e.local() == "cSld")?;
    let (ti, tree) = csld.elements().enumerate().find(|(_, e)| e.local() == "spTree")?;
    let mut path = vec![ci, ti];
    search(tree, pred, &mut path)
}

/// A shape by key: names win over ids.
pub fn find_shape(root: &Element, key: &ShapeKey) -> Option<Vec<usize>> {
    match key {
        ShapeKey::Name(n) => find_shape_path(root, &|e| shape_name(e) == *n).or_else(|| {
            let id: u32 = n.trim().parse().ok()?;
            find_shape_path(root, &|e| shape_id(e) == Some(id))
        }),
        ShapeKey::Id(id) => find_shape_path(root, &|e| shape_id(e) == Some(*id)),
    }
}

pub fn el_at<'a>(root: &'a Element, path: &[usize]) -> Option<&'a Element> {
    let mut cur = root;
    for &i in path {
        cur = cur.elements().nth(i)?;
    }
    Some(cur)
}

pub fn el_at_mut<'a>(root: &'a mut Element, path: &[usize]) -> Option<&'a mut Element> {
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

/// The exact source bytes of the element at `path`.
pub fn raw_element(src: &[u8], path: &[usize]) -> Option<Vec<u8>> {
    let utf8 = xml::to_utf8(src).ok()?;
    let src: &[u8] = &utf8;
    let mut r = Reader::new(src).ok()?;
    let mut counts: Vec<usize> = Vec::new();
    let mut now: Vec<usize> = Vec::new();
    let mut seen_root = false;
    while let Ok(Some(ev)) = r.next() {
        let empty = matches!(ev, Event::Empty { .. });
        match ev {
            Event::Start { span, .. } | Event::Empty { span, .. } => {
                let is_root = !seen_root;
                if is_root {
                    seen_root = true;
                } else {
                    let c = counts.last_mut()?;
                    now.push(*c);
                    *c += 1;
                }
                if now == path {
                    if empty {
                        return Some(src[span].to_vec());
                    }
                    let full = r.subtree_span().ok()?;
                    return Some(src[full].to_vec());
                }
                if empty {
                    if !is_root {
                        now.pop();
                    }
                } else {
                    counts.push(0);
                }
            }
            Event::End { .. } => {
                counts.pop();
                now.pop();
            }
            _ => {}
        }
    }
    None
}

fn child_index(e: &Element, local: &str) -> Option<usize> {
    e.elements().position(|c| c.local() == local)
}

/// Put `node` in place of the shape's text body (or remove it, or add one before `extLst`).
fn set_tx_body(sp: &mut Element, node: Option<Node>) {
    let pos = sp.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "txBody"));
    match (pos, node) {
        (Some(i), Some(n)) => sp.children[i] = n,
        (Some(i), None) => {
            sp.children.remove(i);
        }
        (None, Some(n)) => {
            let at = sp.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "extLst")).unwrap_or(sp.children.len());
            sp.children.insert(at, n);
        }
        (None, None) => {}
    }
}

/// A text body holding `text` (lines become paragraphs), keeping the old body's properties, list
/// style, first paragraph's properties and first run's properties.
pub fn build_tx_body(old: Option<&Element>, text: &str, p_prefix: &str) -> Element {
    let a = old
        .and_then(|t| t.elements().next())
        .and_then(|e| e.prefix())
        .unwrap_or("a")
        .to_string();
    let q = |local: &str| format!("{}:{}", a, local);
    let mut tx = match old {
        Some(o) => o.clone(),
        None => {
            let mut t = Element::new(format!("{}:txBody", p_prefix));
            t.children.push(Node::Element(Element::new(q("bodyPr"))));
            t.children.push(Node::Element(Element::new(q("lstStyle"))));
            t
        }
    };
    let paras: Vec<Element> = tx.children_named("p").cloned().collect();
    let ppr = paras.first().and_then(|p| p.child("pPr")).cloned();
    let rpr = paras
        .iter()
        .flat_map(|p| p.children_named("r"))
        .find_map(|r| r.child("rPr"))
        .cloned()
        .or_else(|| {
            paras.first().and_then(|p| p.child("endParaRPr")).map(|e| {
                let mut r = e.clone();
                r.name = q("rPr");
                r
            })
        });
    let end = paras.last().and_then(|p| p.child("endParaRPr")).cloned();
    // Drop the old paragraphs, keep everything else (body and list properties, extensions).
    tx.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "p"));
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut new_paras = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut p = Element::new(q("p"));
        if let Some(pp) = &ppr {
            p.children.push(Node::Element(pp.clone()));
        }
        if !line.is_empty() {
            let mut r = Element::new(q("r"));
            if let Some(rp) = &rpr {
                r.children.push(Node::Element(rp.clone()));
            }
            let mut t = Element::new(q("t"));
            t.children.push(Node::Text(line.to_string()));
            r.children.push(Node::Element(t));
            p.children.push(Node::Element(r));
        }
        if i + 1 == lines.len() {
            if let Some(e) = &end {
                p.children.push(Node::Element(e.clone()));
            }
        }
        new_paras.push(Node::Element(p));
    }
    // Paragraphs go after bodyPr / lstStyle, before an extLst.
    let at = tx
        .children
        .iter()
        .position(|c| matches!(c, Node::Element(e) if e.local() == "extLst"))
        .unwrap_or(tx.children.len());
    for (k, n) in new_paras.into_iter().enumerate() {
        tx.children.insert(at + k, n);
    }
    tx
}

/// Which part a text edit targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Slide,
    Notes,
}

impl Target {
    fn as_str(self) -> &'static str {
        match self {
            Target::Slide => "slide",
            Target::Notes => "notes",
        }
    }
}

fn part_for(deck: &Deck, slide: usize, target: Target) -> Result<String, BusError> {
    let e = deck
        .slides
        .get(slide)
        .ok_or_else(|| BusError::failed(format!("no slide {} (the deck has {} slide{})", slide, deck.slides.len(), if deck.slides.len() == 1 { "" } else { "s" })))?;
    match target {
        Target::Slide => Ok(e.part.clone()),
        Target::Notes => e.notes.clone().ok_or_else(|| BusError::failed(format!("slide {} has no notes page", slide))),
    }
}

fn restore_command(slide: usize, target: Target, id: u32, xml: Option<String>) -> Command {
    Command::new(
        "stage.restore_text",
        Json::obj()
            .with("slide", slide)
            .with("target", target.as_str())
            .with("shape_id", id)
            .with("xml", xml.map(Json::from).unwrap_or(Json::Null)),
    )
}

/// Replace the text of the shape at `path` in `part`; returns the inverse command.
fn replace_text(deck: &mut Deck, slide: usize, target: Target, part: &str, path: &[usize], text: &str) -> Result<Command, BusError> {
    let src = deck.pkg.part_bytes(part).map_err(|e| BusError::failed(e.to_string()))?;
    let root = xml::parse_dom(&src).map_err(|e| BusError::failed(e.to_string()))?;
    let sp = el_at(&root, path).ok_or_else(|| BusError::failed("shape not found"))?;
    if sp.local() != "sp" {
        return Err(BusError::failed(format!("shape '{}' is a {} and holds no text of its own", shape_name(sp), super::model::shape_kind(sp))));
    }
    let id = shape_id(sp).ok_or_else(|| BusError::failed("the shape has no id"))?;
    let old_raw = child_index(sp, "txBody").and_then(|i| {
        let mut p = path.to_vec();
        p.push(i);
        raw_element(&src, &p)
    });
    let old_xml = match old_raw {
        Some(b) => Some(String::from_utf8(b).map_err(|_| BusError::failed("the text body is not UTF-8"))?),
        None => None,
    };
    let p_prefix = sp.prefix().unwrap_or("p").to_string();
    let new_tx = build_tx_body(sp.child("txBody"), text, &p_prefix);
    let path = path.to_vec();
    deck.pkg
        .edit_xml(part, move |root| {
            if let Some(sp) = el_at_mut(root, &path) {
                set_tx_body(sp, Some(Node::Element(new_tx)));
            }
        })
        .map_err(|e| BusError::failed(e.to_string()))?;
    deck.reload_part(part);
    Ok(restore_command(slide, target, id, old_xml))
}

/// `stage.set_shape_text`.
pub fn set_shape_text(deck: &mut Deck, slide: usize, key: &ShapeKey, text: &str) -> Result<Command, BusError> {
    let part = part_for(deck, slide, Target::Slide)?;
    let root = deck.sheet(&part).ok_or_else(|| BusError::failed("the slide part cannot be read"))?;
    let path = find_shape(root, key).ok_or_else(|| {
        let names: Vec<String> = super::model::sp_tree(root).map(|t| super::model::all_shapes(t).into_iter().map(shape_name).collect()).unwrap_or_default();
        BusError::failed(format!("slide {} has no shape {:?}; its shapes are: {}", slide, key, names.join(", ")))
    })?;
    replace_text(deck, slide, Target::Slide, &part, &path, text)
}

/// `stage.restore_text` (the inverse of a text edit): put back the exact old text body.
pub fn restore_text(deck: &mut Deck, slide: usize, target: Target, id: u32, xml: Option<&str>) -> Result<Command, BusError> {
    let part = part_for(deck, slide, target)?;
    let src = deck.pkg.part_bytes(&part).map_err(|e| BusError::failed(e.to_string()))?;
    let root = xml::parse_dom(&src).map_err(|e| BusError::failed(e.to_string()))?;
    let path = find_shape(&root, &ShapeKey::Id(id)).ok_or_else(|| BusError::failed(format!("no shape with id {}", id)))?;
    let sp = el_at(&root, &path).ok_or_else(|| BusError::failed("shape not found"))?;
    let current = child_index(sp, "txBody")
        .and_then(|i| {
            let mut p = path.clone();
            p.push(i);
            raw_element(&src, &p)
        })
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    let node = xml.map(|x| Node::Raw(x.as_bytes().to_vec()));
    deck.pkg
        .edit_xml(&part, move |root| {
            if let Some(sp) = el_at_mut(root, &path) {
                set_tx_body(sp, node);
            }
        })
        .map_err(|e| BusError::failed(e.to_string()))?;
    deck.reload_part(&part);
    Ok(restore_command(slide, target, id, current))
}

/// `stage.set_notes`: the text of a slide's notes, creating the notes page (and a notes master, when
/// the deck has none) if needed.
pub fn set_notes(deck: &mut Deck, slide: usize, text: &str) -> Result<Command, BusError> {
    if slide >= deck.slides.len() {
        part_for(deck, slide, Target::Slide)?;
    }
    if deck.slides[slide].notes.is_none() {
        let created_master = create_notes(deck, slide, text)?;
        return Ok(Command::new("stage.drop_notes", Json::obj().with("slide", slide).with("master", created_master)));
    }
    let part = part_for(deck, slide, Target::Notes)?;
    let root = deck.sheet(&part).ok_or_else(|| BusError::failed("the notes part cannot be read"))?;
    let path = find_shape_path(root, &|e| Ph::of(e).is_some_and(|p| p.ty == "body"))
        .ok_or_else(|| BusError::failed(format!("slide {}'s notes page has no notes placeholder", slide)))?;
    replace_text(deck, slide, Target::Notes, &part, &path, text)
}

/// A part name like `/ppt/notesSlides/notesSlide{n}.xml` that is not taken yet.
fn free_name(deck: &Deck, pattern: &str) -> String {
    (1..)
        .map(|n| pattern.replace("{n}", &n.to_string()))
        .find(|name| deck.pkg.part(name).is_none())
        .unwrap_or_else(|| pattern.replace("{n}", "x"))
}

/// The prefix a root element binds a namespace to (`r` for relationships, usually).
fn prefix_for(root: &Element, ns: &str, fallback: &str) -> String {
    root.attrs
        .iter()
        .find(|(k, v)| v == ns && k.starts_with("xmlns:"))
        .map(|(k, _)| k["xmlns:".len()..].to_string())
        .unwrap_or_else(|| fallback.to_string())
}

const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn create_notes(deck: &mut Deck, slide: usize, text: &str) -> Result<bool, BusError> {
    let fail = |e: opc::OpcError| BusError::failed(e.to_string());
    let pres = deck.pres_part.clone();
    let slide_part = deck.slides[slide].part.clone();
    let mut master = deck.pkg.related(&pres, REL_NOTES_MASTER).into_iter().find(|p| deck.pkg.part(p).is_some());
    let created = master.is_none();
    if created {
        let theme_bytes = deck.slides[slide]
            .master
            .clone()
            .and_then(|m| deck.pkg.related(&m, opc::REL_THEME).into_iter().next())
            .and_then(|t| deck.pkg.part_bytes(&t).ok())
            .unwrap_or_else(|| blank::THEME.as_bytes().to_vec());
        let theme_part = free_name(deck, "/ppt/theme/theme{n}.xml");
        deck.pkg.add_part(&theme_part, opc::CT_THEME, theme_bytes).map_err(fail)?;
        let nm = free_name(deck, "/ppt/notesMasters/notesMaster{n}.xml");
        deck.pkg.add_part(&nm, CT_NOTES_MASTER, blank::NOTES_MASTER.as_bytes().to_vec()).map_err(fail)?;
        deck.pkg.add_relationship(&nm, opc::REL_THEME, &theme_part, false);
        let rid = deck.pkg.add_relationship(&pres, REL_NOTES_MASTER, &nm, false);
        deck.pkg
            .edit_xml(&pres, |root| {
                let p = root.prefix().unwrap_or("p").to_string();
                let r = prefix_for(root, NS_R, "r");
                let mut id = Element::new(format!("{}:notesMasterId", p));
                id.attrs.push((format!("{}:id", r), rid.clone()));
                let mut list = Element::new(format!("{}:notesMasterIdLst", p));
                list.children.push(Node::Element(id));
                let at = root
                    .children
                    .iter()
                    .position(|c| matches!(c, Node::Element(e) if e.local() == "sldMasterIdLst"))
                    .map(|i| i + 1)
                    .unwrap_or(0);
                root.children.insert(at, Node::Element(list));
            })
            .map_err(fail)?;
        deck.reload_part(&nm);
        master = Some(nm);
    }
    let master = master.unwrap_or_default();
    let notes = free_name(deck, "/ppt/notesSlides/notesSlide{n}.xml");
    deck.pkg.add_part(&notes, CT_NOTES_SLIDE, blank::notes_slide(text).into_bytes()).map_err(fail)?;
    deck.pkg.add_relationship(&notes, REL_NOTES_MASTER, &master, false);
    deck.pkg.add_relationship(&notes, opc::REL_SLIDE, &slide_part, false);
    deck.pkg.add_relationship(&slide_part, REL_NOTES_SLIDE, &notes, false);
    deck.slides[slide].notes = Some(notes.clone());
    deck.reload_part(&notes);
    Ok(created)
}

/// `stage.drop_notes` (the inverse of creating a notes page): remove it, and the notes master too
/// when that was created with it.
pub fn drop_notes(deck: &mut Deck, slide: usize, master: bool) -> Result<(), BusError> {
    let notes = part_for(deck, slide, Target::Notes)?;
    deck.pkg.remove_part(&notes);
    deck.sheets.remove(&name_key(&notes));
    deck.slides[slide].notes = None;
    if master {
        let pres = deck.pres_part.clone();
        if let Some(nm) = deck.pkg.related(&pres, REL_NOTES_MASTER).into_iter().next() {
            let themes = deck.pkg.related(&nm, opc::REL_THEME);
            deck.pkg.remove_part(&nm);
            deck.sheets.remove(&name_key(&nm));
            for t in themes {
                deck.pkg.remove_part(&t);
            }
        }
        deck.pkg
            .edit_xml(&pres, |root| {
                root.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "notesMasterIdLst"));
            })
            .map_err(|e| BusError::failed(e.to_string()))?;
    }
    Ok(())
}
