//! The deck as read from the package: the presentation part, the slides in their order, and every
//! slide, layout, master and notes part parsed once (re-parsed when a command patches one). Nothing is
//! copied out of the XML that the renderer can read from it directly; the XML stays the truth.

use std::collections::HashMap;

use crate::ground::draw::{ColorMap, Theme};
use crate::ground::opc::{self, name_key, Package, REL_SLIDE, REL_SLIDE_LAYOUT, REL_SLIDE_MASTER, REL_THEME};
use crate::ground::xml::{parse_dom, Element};

use super::style::{num, ListStyle};
use super::StageError;

/// Relationship type: a slide's notes page.
pub const REL_NOTES_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
/// Relationship type: the notes master.
pub const REL_NOTES_MASTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
/// Content type: a notes slide.
pub const CT_NOTES_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
/// Content type: the notes master.
pub const CT_NOTES_MASTER: &str = "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
/// Main content type of a macro-enabled deck (`.pptm`).
pub const CT_PRESENTATION_MACRO: &str = "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml";

/// An attribute by local name with a prefix (`r:id`, `r:embed`), whatever the prefix is.
pub(crate) fn rel_attr<'a>(el: &'a Element, local: &str) -> Option<&'a str> {
    el.attrs
        .iter()
        .find(|(k, _)| k.contains(':') && k.rsplit(':').next() == Some(local))
        .map(|(_, v)| v.as_str())
}

/// A placeholder reference (`p:ph`).
#[derive(Debug, Clone, PartialEq)]
pub struct Ph {
    /// The type as written, `obj` when absent (the schema default).
    pub ty: String,
    pub idx: Option<u32>,
}

impl Ph {
    /// The `p:ph` of a shape (`p:sp`, `p:pic`, `p:graphicFrame`, ...), if it is a placeholder.
    pub fn of(shape: &Element) -> Option<Ph> {
        let ph = nv_props(shape)?.child("nvPr")?.child("ph")?;
        Some(Ph {
            ty: ph.attr("type").unwrap_or("obj").trim().to_string(),
            idx: ph.attr("idx").and_then(|s| s.trim().parse().ok()),
        })
    }

    /// The master placeholder type this one inherits from.
    pub fn master_type(&self) -> &str {
        master_type(&self.ty)
    }
}

pub(crate) fn master_type(ty: &str) -> &str {
    match ty {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" | "sldImg" => ty,
        _ => "body",
    }
}

/// Which master text style a placeholder type's text uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCat {
    Title,
    Body,
    Other,
}

impl TextCat {
    pub fn of(ph: Option<&Ph>) -> TextCat {
        match ph.map(|p| p.ty.as_str()) {
            Some("title") | Some("ctrTitle") => TextCat::Title,
            Some("dt") | Some("ftr") | Some("sldNum") | Some("hdr") | None => TextCat::Other,
            Some(_) => TextCat::Body,
        }
    }
}

/// The `p:nv*Pr` child of a shape.
pub fn nv_props(shape: &Element) -> Option<&Element> {
    shape.elements().find(|e| e.local().starts_with("nv") && e.local().ends_with("Pr"))
}

/// The `p:cNvPr` of a shape.
pub fn c_nv_pr(shape: &Element) -> Option<&Element> {
    nv_props(shape)?.child("cNvPr")
}

pub fn shape_name(shape: &Element) -> String {
    c_nv_pr(shape).and_then(|c| c.attr("name")).unwrap_or("").to_string()
}

pub fn shape_id(shape: &Element) -> Option<u32> {
    c_nv_pr(shape).and_then(|c| c.attr("id")).and_then(|s| s.trim().parse().ok())
}

pub fn is_hidden(shape: &Element) -> bool {
    c_nv_pr(shape).and_then(|c| c.attr("hidden")).map(super::style::on).unwrap_or(false)
}

/// The shape tree of a slide, layout, master or notes page.
pub fn sp_tree(root: &Element) -> Option<&Element> {
    root.child("cSld")?.child("spTree")
}

/// The drawable children of a shape tree or group, `mc:AlternateContent` resolved to its fallback
/// (or its first choice when there is none).
pub fn shape_children(tree: &Element) -> Vec<&Element> {
    let mut out = Vec::new();
    for e in tree.elements() {
        match e.local() {
            "sp" | "pic" | "grpSp" | "graphicFrame" | "cxnSp" | "contentPart" => out.push(e),
            "AlternateContent" => {
                let branch = e.child("Fallback").or_else(|| e.child("Choice"));
                if let Some(b) = branch {
                    out.extend(shape_children(b));
                }
            }
            _ => {}
        }
    }
    out
}

/// Every shape of a tree, groups entered, in document order.
pub fn all_shapes(tree: &Element) -> Vec<&Element> {
    let mut out = Vec::new();
    for s in shape_children(tree) {
        out.push(s);
        if s.local() == "grpSp" {
            out.extend(all_shapes(s));
        }
    }
    out
}

/// The layout placeholder a slide placeholder inherits from: by `idx` first, then by type.
pub fn find_layout_ph<'a>(tree: &'a Element, ph: &Ph) -> Option<&'a Element> {
    let phs: Vec<(&Element, Ph)> = all_shapes(tree).into_iter().filter_map(|s| Ph::of(s).map(|p| (s, p))).collect();
    if let Some(idx) = ph.idx {
        if let Some((s, _)) = phs.iter().find(|(_, p)| p.idx == Some(idx)) {
            return Some(s);
        }
    }
    if let Some((s, _)) = phs.iter().find(|(_, p)| p.ty == ph.ty) {
        return Some(s);
    }
    // Type-only placeholders (title, dt, ftr, sldNum) and their equivalents.
    if ph.idx.is_none() || matches!(ph.ty.as_str(), "title" | "ctrTitle" | "dt" | "ftr" | "sldNum") {
        return phs.iter().find(|(_, p)| p.master_type() == ph.master_type()).map(|(s, _)| *s);
    }
    None
}

/// The master placeholder of a type (title, body, dt, ftr, sldNum).
pub fn find_master_ph<'a>(tree: &'a Element, ph: &Ph) -> Option<&'a Element> {
    let want = ph.master_type();
    all_shapes(tree).into_iter().find(|s| Ph::of(s).is_some_and(|p| p.master_type() == want))
}

/// The plain text of a text body: paragraphs joined by `\n`, line breaks as `\n`, the slide number
/// field as `slide_num` when given.
pub fn text_of(tx_body: &Element, slide_num: Option<usize>) -> String {
    let mut paras = Vec::new();
    for p in tx_body.children_named("p") {
        let mut s = String::new();
        for r in p.elements() {
            match r.local() {
                "r" => s.push_str(&r.child("t").map(|t| t.text()).unwrap_or_default()),
                "br" => s.push('\n'),
                "fld" => {
                    let slidenum = r.attr("type").is_some_and(|t| t == "slidenum");
                    match (slidenum, slide_num) {
                        (true, Some(n)) => s.push_str(&n.to_string()),
                        _ => s.push_str(&r.child("t").map(|t| t.text()).unwrap_or_default()),
                    }
                }
                _ => {}
            }
        }
        paras.push(s);
    }
    paras.join("\n")
}

/// The text of a shape (`p:sp` text body, or a table's cells joined by tabs and new lines).
pub fn shape_text(shape: &Element, slide_num: Option<usize>) -> String {
    if let Some(tx) = shape.child("txBody") {
        return text_of(tx, slide_num);
    }
    if let Some(tbl) = table_of(shape) {
        let rows: Vec<String> = tbl
            .children_named("tr")
            .map(|tr| tr.children_named("tc").map(|tc| tc.child("txBody").map(|t| text_of(t, slide_num)).unwrap_or_default()).collect::<Vec<_>>().join("\t"))
            .collect();
        return rows.join("\n");
    }
    String::new()
}

/// The `a:tbl` of a graphic frame.
pub fn table_of(frame: &Element) -> Option<&Element> {
    if frame.local() != "graphicFrame" {
        return None;
    }
    frame.child("graphic")?.child("graphicData")?.child("tbl")
}

/// What kind of shape this is, for the AI's outline.
pub fn shape_kind(shape: &Element) -> &'static str {
    match shape.local() {
        "sp" => {
            let tx_box = nv_props(shape).and_then(|n| n.child("cNvSpPr")).and_then(|c| c.attr("txBox")).map(super::style::on).unwrap_or(false);
            if tx_box {
                "text box"
            } else {
                "shape"
            }
        }
        "pic" => "picture",
        "grpSp" => "group",
        "cxnSp" => "connector",
        "graphicFrame" => {
            if table_of(shape).is_some() {
                "table"
            } else {
                let uri = shape.child("graphic").and_then(|g| g.child("graphicData")).and_then(|d| d.attr("uri")).unwrap_or("");
                if uri.ends_with("/chart") {
                    "chart"
                } else if uri.ends_with("/diagram") {
                    "smartart"
                } else {
                    "graphic"
                }
            }
        }
        _ => "other",
    }
}

/// One slide in presentation order.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideEntry {
    pub part: String,
    pub layout: Option<String>,
    pub master: Option<String>,
    pub notes: Option<String>,
}

/// The master text styles: title, body, other.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MasterStyles {
    pub title: ListStyle,
    pub body: ListStyle,
    pub other: ListStyle,
}

impl MasterStyles {
    pub fn get(&self, cat: TextCat) -> &ListStyle {
        match cat {
            TextCat::Title => &self.title,
            TextCat::Body => &self.body,
            TextCat::Other => &self.other,
        }
    }
}

/// The deck as read.
#[derive(Debug, Clone)]
pub struct Deck {
    pub pkg: Package,
    pub pres_part: String,
    /// Slide size, EMU.
    pub size: (i64, i64),
    /// Notes page size, EMU.
    pub notes_size: (i64, i64),
    pub first_slide_num: usize,
    pub default_style: ListStyle,
    pub slides: Vec<SlideEntry>,
    /// Parsed slide, layout, master and notes parts, by part-name key.
    pub sheets: HashMap<String, Element>,
    /// Themes by master (or notes master) part key.
    pub themes: HashMap<String, Theme>,
    pub master_styles: HashMap<String, MasterStyles>,
    pub default_theme: Theme,
}

impl Deck {
    pub fn load(pkg: Package) -> Result<Deck, StageError> {
        let pres_part = pkg
            .main_part()
            .or_else(|| pkg.parts_of_type(opc::CT_PRESENTATIONML_MAIN).into_iter().next())
            .or_else(|| pkg.parts_of_type(CT_PRESENTATION_MACRO).into_iter().next())
            .ok_or_else(|| StageError("not a presentation: the package has no main part".into()))?;
        let pres_bytes = pkg.part_bytes(&pres_part).map_err(|e| StageError(e.to_string()))?;
        let pres = parse_dom(&pres_bytes).map_err(|e| StageError(format!("presentation.xml: {}", e)))?;
        if pres.local() != "presentation" {
            return Err(StageError(format!("not a presentation: the main part is <{}>", pres.name)));
        }
        let size_of = |name: &str, dw: i64, dh: i64| {
            pres.child(name)
                .map(|s| {
                    (
                        s.attr("cx").and_then(num).map(|v| v as i64).unwrap_or(dw),
                        s.attr("cy").and_then(num).map(|v| v as i64).unwrap_or(dh),
                    )
                })
                .unwrap_or((dw, dh))
        };
        let size = size_of("sldSz", 9144000, 6858000);
        let notes_size = size_of("notesSz", 6858000, 9144000);
        let first_slide_num = pres.attr("firstSlideNum").and_then(|s| s.trim().parse().ok()).unwrap_or(1);
        let default_style = pres.child("defaultTextStyle").map(ListStyle::parse).unwrap_or_default();

        let mut deck = Deck {
            pkg,
            pres_part,
            size,
            notes_size,
            first_slide_num,
            default_style,
            slides: Vec::new(),
            sheets: HashMap::new(),
            themes: HashMap::new(),
            master_styles: HashMap::new(),
            default_theme: Theme::office_default(),
        };

        // The slides, in sldIdLst order (falling back on the relationship order).
        let mut slide_parts = Vec::new();
        if let Some(list) = pres.child("sldIdLst") {
            for id in list.children_named("sldId") {
                if let Some(rid) = rel_attr(id, "id") {
                    if let Some(rel) = deck.pkg.rel_by_id(&deck.pres_part, rid) {
                        let name = deck.pkg.resolve(&deck.pres_part, rel);
                        if deck.pkg.part(&name).is_some() {
                            slide_parts.push(name);
                        }
                    }
                }
            }
        } else {
            slide_parts = deck.pkg.related(&deck.pres_part, REL_SLIDE);
        }
        for part in slide_parts {
            let layout = deck.pkg.related(&part, REL_SLIDE_LAYOUT).into_iter().find(|p| deck.pkg.part(p).is_some());
            let master = layout.as_ref().and_then(|l| deck.pkg.related(l, REL_SLIDE_MASTER).into_iter().find(|p| deck.pkg.part(p).is_some()));
            let notes = deck.pkg.related(&part, REL_NOTES_SLIDE).into_iter().find(|p| deck.pkg.part(p).is_some());
            deck.slides.push(SlideEntry { part, layout, master, notes });
        }
        // Masters listed in the presentation (also those no slide uses).
        let masters = deck.pkg.related(&deck.pres_part, REL_SLIDE_MASTER);
        let mut to_parse: Vec<String> = Vec::new();
        for s in &deck.slides {
            to_parse.push(s.part.clone());
            to_parse.extend(s.layout.iter().cloned());
            to_parse.extend(s.master.iter().cloned());
            to_parse.extend(s.notes.iter().cloned());
        }
        to_parse.extend(masters.iter().cloned());
        for part in to_parse {
            deck.reload_part(&part);
        }
        Ok(deck)
    }

    /// (Re)parse one part into the sheet cache; a master also gets its theme and text styles.
    pub fn reload_part(&mut self, part: &str) {
        let key = name_key(part);
        let Ok(bytes) = self.pkg.part_bytes(part) else {
            self.sheets.remove(&key);
            return;
        };
        let Ok(root) = parse_dom(&bytes) else {
            self.sheets.remove(&key);
            return;
        };
        if matches!(root.local(), "sldMaster" | "notesMaster") {
            if let Some(theme_part) = self.pkg.related(part, REL_THEME).into_iter().next() {
                if let Some(t) = self.pkg.part_bytes(&theme_part).ok().and_then(|b| Theme::parse(&b).ok()) {
                    self.themes.insert(key.clone(), t);
                }
            }
            if let Some(tx) = root.child("txStyles") {
                let ls = |n: &str| tx.child(n).map(ListStyle::parse).unwrap_or_default();
                self.master_styles.insert(key.clone(), MasterStyles { title: ls("titleStyle"), body: ls("bodyStyle"), other: ls("otherStyle") });
            }
        }
        self.sheets.insert(key, root);
    }

    pub fn sheet(&self, part: &str) -> Option<&Element> {
        self.sheets.get(&name_key(part))
    }

    pub fn theme_of(&self, master: Option<&str>) -> &Theme {
        master.and_then(|m| self.themes.get(&name_key(m))).unwrap_or(&self.default_theme)
    }

    pub fn styles_of(&self, master: Option<&str>) -> Option<&MasterStyles> {
        master.and_then(|m| self.master_styles.get(&name_key(m)))
    }

    /// The colour map a slide uses: its own override, else its layout's, else the master's.
    pub fn color_map(&self, slide: usize) -> ColorMap {
        let Some(e) = self.slides.get(slide) else { return ColorMap::default() };
        let ovr = |part: Option<&String>| -> Option<ColorMap> {
            let o = self.sheet(part?)?.child("clrMapOvr")?;
            o.child("overrideClrMapping").map(|_| ColorMap::parse(o))
        };
        ovr(Some(&e.part))
            .or_else(|| ovr(e.layout.as_ref()))
            .or_else(|| e.master.as_ref().and_then(|m| self.sheet(m)).and_then(|m| m.child("clrMap")).map(ColorMap::parse))
            .unwrap_or_default()
    }

    /// The slide number shown on slide `index`.
    pub fn slide_number(&self, index: usize) -> usize {
        index + self.first_slide_num
    }

    /// The layout's name (`p:cSld name`).
    pub fn layout_name(&self, slide: usize) -> String {
        self.slides
            .get(slide)
            .and_then(|s| s.layout.as_ref())
            .and_then(|l| self.sheet(l))
            .and_then(|r| r.child("cSld"))
            .and_then(|c| c.attr("name"))
            .unwrap_or("")
            .to_string()
    }

    /// The text of a slide's notes (its body placeholder).
    pub fn notes_text(&self, slide: usize) -> Option<String> {
        let part = self.slides.get(slide)?.notes.as_ref()?;
        let root = self.sheet(part)?;
        let tree = sp_tree(root)?;
        let body = all_shapes(tree).into_iter().find(|s| Ph::of(s).is_some_and(|p| p.ty == "body"))?;
        body.child("txBody").map(|t| text_of(t, None))
    }
}
