//! Scroll, the word processor. Milestone M1, *open and see*: a `.docx` opens and shows faithfully,
//! and saving changes nothing it did not touch. Milestone M2, *write*: a caret and a selection on
//! the pages ([`caret`], the layout's position map), and every writing command of the first tier
//! as an ordinary bus command ([`edit`], [`tables`], [`sections`], [`media`]) that edits the XML
//! surgically ([`dom`]) and undoes by putting back exactly the XML it replaced.
//!
//! - [`ScrollDoc::open`] reads the package ([`crate::ground::opc`]), builds the [`model`] (body,
//!   styles with `basedOn` chains, numbering, theme, settings, headers and footers, inline pictures)
//!   and lays it out into pages ([`layout`]).
//! - [`ScrollDoc::save`] hands back the package: untouched parts byte-identical, and an unedited
//!   document byte-identical as a whole.
//! - [`ScrollDoc`] is a [`Document`] for the command bus: `display(page)` gives the page's display
//!   list, `resources()` the glyph outlines and pictures it draws with, `describe` / `read` serve the
//!   AI's door, and the M1 commands (`scroll.set_title`, `scroll.set_paragraph_text`) edit the XML
//!   through [`opc::patch_xml`] so that everything the editor does not understand survives.
//!
//! Paragraph indexes count every paragraph of the body in reading order, the ones inside table
//! cells included (headers and footers are not counted).

mod caret;
pub mod dom;
pub mod edit;
pub mod layout;
mod media;
pub mod model;
pub mod numbering;
pub mod pos;
pub mod props;
mod sections;
pub mod styles;
mod tables;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod write_tests;

use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command, CommandSpec, Document};
use crate::ground::display::{Color, DisplayList, Item, Paint, PathOp, Resources};
use crate::ground::edit::{Editing, Selection};
use crate::ground::pdfw::PageResources;
use crate::ground::font::FontIndex;
use crate::ground::opc::{self, CoreProperties, Package};
use crate::ground::text::FontCache;
use crate::ground::xml::{self, Element, Event, Node, Reader};
use layout::Laid;
use model::{ImageStore, Model};
use std::sync::{Arc, OnceLock};

/// This machine's fonts, indexed once per process and shared by every document.
pub fn system_fonts() -> Arc<FontIndex> {
    static INDEX: OnceLock<Arc<FontIndex>> = OnceLock::new();
    INDEX.get_or_init(|| Arc::new(FontIndex::system())).clone()
}

/// An open word-processing document.
pub struct ScrollDoc {
    pkg: Package,
    main: String,
    model: Model,
    images: ImageStore,
    fonts: FontCache,
    laid: Laid,
    pages: Vec<DisplayList>,
    resources: Resources,
    /// Where the last command's selection landed (by the command's fingerprint).
    landed: Option<(String, Selection)>,
    /// Character formatting chosen at a caret, for the text typed there next.
    pending: Option<(pos::SPos, Vec<(String, Json)>)>,
}

impl std::fmt::Debug for ScrollDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollDoc").field("main", &self.main).field("pages", &self.pages.len()).field("paragraphs", &self.model.flat.len()).finish()
    }
}

impl ScrollDoc {
    /// Open `.docx` bytes, laying out with this machine's fonts.
    pub fn open(bytes: &[u8]) -> Result<ScrollDoc, String> {
        ScrollDoc::open_with_fonts(bytes, system_fonts())
    }

    /// Open `.docx` bytes with a given font index (an empty one lays out with synthetic metrics).
    pub fn open_with_fonts(bytes: &[u8], fonts: Arc<FontIndex>) -> Result<ScrollDoc, String> {
        let pkg = Package::open(bytes).map_err(|e| e.to_string())?;
        ScrollDoc::from_package(pkg, FontCache::new(fonts))
    }

    /// A document from an already opened package.
    pub fn from_package(pkg: Package, fonts: FontCache) -> Result<ScrollDoc, String> {
        let main = pkg.main_part().ok_or_else(|| "not a word-processing document: the package has no main part".to_string())?;
        if pkg.part(&main).is_none() {
            return Err(format!("the main part {main} is missing"));
        }
        let mut images = ImageStore::default();
        let model = Model::load(&pkg, &main, &mut images)?;
        let mut doc = ScrollDoc {
            pkg,
            main,
            model,
            images,
            fonts,
            laid: Laid::default(),
            pages: Vec::new(),
            resources: Resources::new(),
            landed: None,
            pending: None,
        };
        doc.relayout();
        Ok(doc)
    }

    /// A new, empty document: one empty paragraph on a Letter page with one-inch margins, Calibri
    /// 11 pt body text and the usual heading styles.
    pub fn new_blank() -> ScrollDoc {
        let mut pkg = Package::new();
        let main = "/word/document.xml";
        let _ = pkg.add_part(main, opc::CT_WORDPROCESSINGML_MAIN, BLANK_DOCUMENT.as_bytes().to_vec());
        let _ = pkg.add_part("/word/styles.xml", opc::CT_STYLES, BLANK_STYLES.as_bytes().to_vec());
        pkg.add_relationship("/", opc::REL_OFFICE_DOCUMENT, "word/document.xml", false);
        pkg.add_relationship(main, opc::REL_STYLES, "styles.xml", false);
        let _ = pkg.set_core(CoreProperties { creator: Some("Scriptorium".into()), ..CoreProperties::default() });
        ScrollDoc::from_package(pkg, FontCache::new(system_fonts())).expect("the blank document always opens")
    }

    /// The document as `.docx` bytes: the opened bytes exactly when nothing changed, otherwise every
    /// untouched part copied as it was.
    pub fn save(&self) -> Vec<u8> {
        self.pkg.save()
    }

    /// The package underneath.
    pub fn package(&self) -> &Package {
        &self.pkg
    }

    /// The main part's name (`/word/document.xml`).
    pub fn main_part(&self) -> &str {
        &self.main
    }

    /// How many body paragraphs there are (table cells included).
    pub fn paragraph_count(&self) -> usize {
        self.model.flat.len()
    }

    /// A body paragraph's plain text.
    pub fn paragraph_text(&self, index: usize) -> Option<&str> {
        self.model.flat.get(index).map(|p| p.text.as_str())
    }

    /// The text laid on a page (body only).
    pub fn page_text(&self, page: usize) -> Option<&str> {
        self.laid.pages.get(page).map(|p| p.body_text.as_str())
    }

    /// The model (read-only).
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The laid-out pages.
    pub fn laid(&self) -> &Laid {
        &self.laid
    }

    fn rebuild(&mut self) -> Result<(), String> {
        self.model = Model::load(&self.pkg, &self.main, &mut self.images)?;
        self.relayout();
        Ok(())
    }

    fn relayout(&mut self) {
        self.laid = layout::layout(&self.model, &mut self.fonts);
        let mut res = Resources::new();
        let mut lists = Vec::with_capacity(self.laid.pages.len());
        for p in &self.laid.pages {
            let mut dl = DisplayList::new(p.width, p.height);
            dl.push(Item::Path {
                ops: vec![
                    PathOp::MoveTo(0.0, 0.0),
                    PathOp::LineTo(p.width, 0.0),
                    PathOp::LineTo(p.width, p.height),
                    PathOp::LineTo(0.0, p.height),
                    PathOp::Close,
                ],
                fill: Some(Paint::Solid(Color::WHITE)),
                stroke: None,
            });
            dl.items.extend(p.header_items.iter().cloned());
            dl.items.extend(p.body_items.iter().cloned());
            dl.items.extend(p.footer_items.iter().cloned());
            self.fonts.fill_resources(&mut res, &dl.items);
            lists.push(dl);
        }
        for img in &self.images.images {
            res.add_image(img.clone());
        }
        self.pages = lists;
        self.resources = res;
    }

    fn para_page(&self, index: usize) -> usize {
        self.laid.para_page.get(index).copied().unwrap_or(0)
    }

    fn touched_from(&self, old_page: usize, old_count: usize) -> Vec<usize> {
        let end = old_count.max(self.pages.len());
        (old_page.min(end)..end).collect()
    }

    // --- commands ----------------------------------------------------------------------------------

    fn set_title(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let title = match cmd.args.get("title") {
            Some(Json::Str(s)) => Some(s.clone()),
            Some(Json::Null) => None,
            _ => return Err(BusError::bad_args(&cmd.name, "takes { \"title\": string | null }")),
        };
        let old = self.pkg.core().title.clone();
        let mut core = self.pkg.core().clone();
        core.title = title;
        self.pkg.set_core(core).map_err(|e| BusError::failed(e.to_string()))?;
        let inverse = Json::obj().with("title", old.map(Json::Str).unwrap_or(Json::Null));
        Ok(Change { undo: vec![Command::new("scroll.set_title", inverse)], pages_touched: Vec::new() })
    }

    fn para_index(&self, cmd: &Command) -> Result<usize, BusError> {
        let i = cmd
            .args
            .get("index")
            .and_then(Json::as_usize)
            .ok_or_else(|| BusError::bad_args(&cmd.name, "\"index\" is a paragraph number from 0"))?;
        if i >= self.model.flat.len() {
            return Err(BusError::failed(format!("no paragraph {i} (the document has {})", self.model.flat.len())));
        }
        Ok(i)
    }

    fn set_paragraph_text(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let i = self.para_index(cmd)?;
        let text = cmd
            .args
            .get("text")
            .and_then(Json::as_str)
            .ok_or_else(|| BusError::bad_args(&cmd.name, "takes { \"index\": n, \"text\": string }"))?
            .to_string();
        let path = self.model.flat[i].path.clone();
        let before = self.pkg.part_bytes(&self.main).map_err(|e| BusError::failed(e.to_string()))?;
        let old_xml = element_source(&before, &path).ok_or_else(|| BusError::failed(format!("paragraph {i} was not found in the XML")))?;
        let (old_page, old_count) = (self.para_page(i), self.pages.len());
        let main = self.main.clone();
        self.pkg
            .edit_xml(&main, |root| {
                if let Some(p) = element_at_mut(root, &path) {
                    rewrite_paragraph(p, &text);
                }
            })
            .map_err(|e| BusError::failed(e.to_string()))?;
        self.rebuild().map_err(BusError::failed)?;
        let inverse = Json::obj().with("index", i).with("xml", old_xml);
        Ok(Change { undo: vec![Command::new("scroll.restore_paragraph", inverse)], pages_touched: self.touched_from(old_page, old_count) })
    }

    /// The inverse of an edit: puts a paragraph's original XML back exactly as it was written.
    fn restore_paragraph(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let i = self.para_index(cmd)?;
        let xml_text = cmd
            .args
            .get("xml")
            .and_then(Json::as_str)
            .ok_or_else(|| BusError::bad_args(&cmd.name, "takes { \"index\": n, \"xml\": string }"))?
            .to_string();
        // Refuse anything that is not one paragraph element (the inverse is ours, but stay safe).
        let snippet_ok = xml::parse_dom(xml_text.as_bytes()).is_ok_and(|e| e.local() == "p")
            || wrap_and_check(&xml_text);
        if !snippet_ok {
            return Err(BusError::bad_args(&cmd.name, "\"xml\" must be one w:p element"));
        }
        let path = self.model.flat[i].path.clone();
        let before = self.pkg.part_bytes(&self.main).map_err(|e| BusError::failed(e.to_string()))?;
        let current = element_source(&before, &path).ok_or_else(|| BusError::failed(format!("paragraph {i} was not found in the XML")))?;
        let (old_page, old_count) = (self.para_page(i), self.pages.len());
        let main = self.main.clone();
        let bytes = xml_text.into_bytes();
        self.pkg
            .edit_xml(&main, |root| {
                replace_at(root, &path, Node::Raw(bytes));
            })
            .map_err(|e| BusError::failed(e.to_string()))?;
        self.rebuild().map_err(BusError::failed)?;
        let inverse = Json::obj().with("index", i).with("xml", current);
        Ok(Change { undo: vec![Command::new("scroll.restore_paragraph", inverse)], pages_touched: self.touched_from(old_page, old_count) })
    }
}

/// A paragraph snippet that uses undeclared prefixes (`w:`) does not parse alone: check it inside a
/// wrapper that declares the usual WordprocessingML prefixes.
fn wrap_and_check(snippet: &str) -> bool {
    let wrapped = format!(
        "<x xmlns:w=\"{W}\" xmlns:r=\"{R}\" xmlns:wp=\"{WP}\" xmlns:a=\"{A}\" xmlns:pic=\"{PIC}\" xmlns:mc=\"{MC}\" xmlns:w14=\"{W14}\" xmlns:wps=\"{WPS}\" xmlns:v=\"{V}\" xmlns:o=\"{O}\" xmlns:m=\"{M}\">{snippet}</x>",
        W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
        WP = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
        A = "http://schemas.openxmlformats.org/drawingml/2006/main",
        PIC = "http://schemas.openxmlformats.org/drawingml/2006/picture",
        MC = "http://schemas.openxmlformats.org/markup-compatibility/2006",
        W14 = "http://schemas.microsoft.com/office/word/2010/wordml",
        WPS = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
        V = "urn:schemas-microsoft-com:vml",
        O = "urn:schemas-microsoft-com:office:office",
        M = "http://schemas.openxmlformats.org/officeDocument/2006/math",
    );
    match xml::parse_dom(wrapped.as_bytes()) {
        Ok(root) => {
            let els: Vec<&Element> = root.elements().collect();
            els.len() == 1 && els[0].local() == "p"
        }
        Err(_) => {
            // Prefixes we did not declare: accept when it at least looks like one paragraph.
            let t = snippet.trim();
            (t.starts_with("<w:p>") || t.starts_with("<w:p ")) && t.ends_with("</w:p>") || t.starts_with("<w:p") && t.ends_with("/>")
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// XML paths
// ---------------------------------------------------------------------------------------------------

/// The element at an element-only child path below `root`.
fn element_at_mut<'e>(root: &'e mut Element, path: &[usize]) -> Option<&'e mut Element> {
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

/// Replace the element at `path` with `node`.
fn replace_at(root: &mut Element, path: &[usize], node: Node) -> bool {
    let Some((&last, parent_path)) = path.split_last() else { return false };
    let Some(parent) = element_at_mut(root, parent_path) else { return false };
    let mut seen = 0;
    for slot in parent.children.iter_mut() {
        if matches!(slot, Node::Element(_)) {
            if seen == last {
                *slot = node;
                return true;
            }
            seen += 1;
        }
    }
    false
}

/// The exact source text of the element at an element-only child path below the root.
fn element_source(bytes: &[u8], path: &[usize]) -> Option<String> {
    let src = xml::to_utf8(bytes).ok()?;
    let mut r = Reader::new(&src).ok()?;
    let mut counts: Vec<usize> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    while let Some(ev) = r.next().ok()? {
        match ev {
            Event::Start { span, .. } => {
                if counts.is_empty() {
                    counts.push(0);
                    continue;
                }
                let idx = *counts.last()?;
                *counts.last_mut()? += 1;
                cur.push(idx);
                if cur == path {
                    let range = r.subtree_span().ok()?;
                    let _ = span;
                    return String::from_utf8(src[range].to_vec()).ok();
                }
                counts.push(0);
            }
            Event::Empty { span, .. } => {
                if counts.is_empty() {
                    return None;
                }
                let idx = *counts.last()?;
                *counts.last_mut()? += 1;
                cur.push(idx);
                if cur == path {
                    return String::from_utf8(src[span].to_vec()).ok();
                }
                cur.pop();
            }
            Event::End { .. } => {
                counts.pop();
                cur.pop();
            }
            _ => {}
        }
    }
    None
}

/// Replace a paragraph's content with plain text in one run that keeps the first run's formatting.
/// The paragraph's properties and its range markers (bookmarks, comment ranges) stay.
fn rewrite_paragraph(p: &mut Element, text: &str) {
    let w = p.prefix().map(|s| format!("{s}:")).unwrap_or_default();
    fn first_rpr(el: &Element) -> Option<Element> {
        for c in el.elements() {
            if c.local() == "r" {
                return c.child("rPr").cloned();
            }
            if c.local() == "pPr" {
                continue;
            }
            if let Some(r) = first_rpr(c) {
                return Some(r);
            }
        }
        None
    }
    let rpr = first_rpr(p);
    let ppr = p.child("pPr").cloned();
    let markers: Vec<Node> = p
        .children
        .iter()
        .filter(|c| {
            matches!(c, Node::Element(e) if matches!(e.local(),
                "bookmarkStart" | "bookmarkEnd" | "commentRangeStart" | "commentRangeEnd" | "permStart" | "permEnd"))
        })
        .cloned()
        .collect();
    let mut run = Element::new(format!("{w}r"));
    if let Some(r) = rpr {
        run.children.push(Node::Element(r));
    }
    let mut buf = String::new();
    let flush = |buf: &mut String, run: &mut Element| {
        if buf.is_empty() {
            return;
        }
        let mut t = Element::new(format!("{w}t"));
        if buf.starts_with(' ') || buf.ends_with(' ') || buf.contains("  ") {
            t.attrs.push(("xml:space".into(), "preserve".into()));
        }
        t.children.push(Node::Text(std::mem::take(buf)));
        run.children.push(Node::Element(t));
    };
    for ch in text.chars() {
        match ch {
            '\t' => {
                flush(&mut buf, &mut run);
                run.children.push(Node::Element(Element::new(format!("{w}tab"))));
            }
            '\n' => {
                flush(&mut buf, &mut run);
                run.children.push(Node::Element(Element::new(format!("{w}br"))));
            }
            '\r' => {}
            c => buf.push(c),
        }
    }
    flush(&mut buf, &mut run);
    let mut kids = Vec::new();
    if let Some(pp) = ppr {
        kids.push(Node::Element(pp));
    }
    kids.extend(markers.iter().filter(|m| matches!(m, Node::Element(e) if e.local().ends_with("Start"))).cloned());
    if run.children.iter().any(|c| matches!(c, Node::Element(e) if e.local() != "rPr")) {
        kids.push(Node::Element(run));
    }
    kids.extend(markers.into_iter().filter(|m| matches!(m, Node::Element(e) if !e.local().ends_with("Start"))));
    p.children = kids;
}

// ---------------------------------------------------------------------------------------------------
// The bus
// ---------------------------------------------------------------------------------------------------

fn opt(s: &Option<String>) -> Json {
    match s {
        Some(v) => Json::Str(v.clone()),
        None => Json::Null,
    }
}

impl ScrollDoc {
    fn para_json(&self, i: usize) -> Json {
        let p = &self.model.flat[i];
        let mut j = Json::obj().with("index", i).with("text", p.text.as_str());
        match &p.style {
            Some(s) => {
                j.set("style", s.as_str());
                j.set("style_name", self.model.styles.name_of(s));
                if let Some(l) = self.model.styles.heading_level(s) {
                    j.set("heading_level", l as usize);
                }
            }
            None => j.set("style", Json::Null),
        }
        j.set("page", self.para_page(i));
        j
    }
}

impl Document for ScrollDoc {
    fn kind(&self) -> &'static str {
        "scroll"
    }

    fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
        self.landed = None;
        let keeps_pending = matches!(cmd.name.as_str(), "scroll.toggle" | "scroll.format" | "scroll.font_size_step" | "scroll.insert_text" | "scroll.find" | "scroll.select");
        if !keeps_pending {
            self.pending = None;
        }
        match cmd.name.as_str() {
            "scroll.set_title" => self.set_title(cmd),
            "scroll.set_paragraph_text" => self.set_paragraph_text(cmd),
            "scroll.restore_paragraph" => self.restore_paragraph(cmd),
            "scroll.restore" => self.restore(cmd),
            "scroll.insert_text" => self.cmd_insert_text(cmd),
            "scroll.delete" => self.cmd_delete(cmd),
            "scroll.enter" => self.cmd_enter(cmd),
            "scroll.tab" => self.cmd_tab(cmd),
            "scroll.paste" => self.cmd_paste(cmd),
            "scroll.format" => self.cmd_format(cmd),
            "scroll.toggle" => self.cmd_toggle(cmd),
            "scroll.font_size_step" => self.cmd_font_step(cmd),
            "scroll.clear_format" => self.cmd_clear_format(cmd),
            "scroll.paragraph" => self.cmd_paragraph(cmd),
            "scroll.align" => self.cmd_align(cmd),
            "scroll.indent" => self.cmd_indent(cmd),
            "scroll.style" => self.cmd_style(cmd),
            "scroll.list" => self.cmd_list(cmd),
            "scroll.list_level" => self.cmd_list_level(cmd),
            "scroll.list_restart" => self.cmd_list_restart(cmd),
            "scroll.find" => self.cmd_find(cmd),
            "scroll.replace" => self.cmd_replace(cmd),
            "scroll.select" => self.cmd_select(cmd),
            "scroll.insert_table" => self.cmd_insert_table(cmd),
            "scroll.table_insert" => self.cmd_table_insert(cmd),
            "scroll.table_delete" => self.cmd_table_delete(cmd),
            "scroll.merge_cells" => self.cmd_merge_cells(cmd),
            "scroll.table_widths" => self.cmd_table_widths(cmd),
            "scroll.row_height" => self.cmd_row_height(cmd),
            "scroll.row_break" => self.cmd_row_break(cmd),
            "scroll.header_rows" => self.cmd_header_rows(cmd),
            "scroll.cell_align" => self.cmd_cell_align(cmd),
            "scroll.shading" => self.cmd_shading(cmd),
            "scroll.borders" => self.cmd_borders(cmd),
            "scroll.table_align" => self.cmd_table_align(cmd),
            "scroll.page_break" => self.cmd_page_break(cmd),
            "scroll.section_break" => self.cmd_section_break(cmd),
            "scroll.page_setup" => self.cmd_page_setup(cmd),
            "scroll.header_footer" => self.cmd_header_footer(cmd),
            "scroll.page_number" => self.cmd_page_number(cmd),
            "scroll.page_numbering" => self.cmd_page_numbering(cmd),
            "scroll.first_page" => self.cmd_first_page(cmd),
            "scroll.insert_picture" => self.cmd_insert_picture(cmd),
            "scroll.picture_size" => self.cmd_picture_size(cmd),
            "scroll.link" => self.cmd_link(cmd),
            "scroll.unlink" => self.cmd_unlink(cmd),
            other => Err(BusError::unknown(other)),
        }
    }

    fn describe(&self) -> Json {
        let mut headings = Json::arr();
        for (i, p) in self.model.flat.iter().enumerate() {
            let Some(s) = &p.style else { continue };
            if let Some(level) = self.model.styles.heading_level(s) {
                headings.push(
                    Json::obj()
                        .with("index", i)
                        .with("level", level as usize)
                        .with("text", p.text.as_str())
                        .with("style", self.model.styles.name_of(s))
                        .with("page", self.para_page(i)),
                );
            }
        }
        let core = self.pkg.core();
        let props = Json::obj()
            .with("title", opt(&core.title))
            .with("subject", opt(&core.subject))
            .with("creator", opt(&core.creator))
            .with("keywords", opt(&core.keywords))
            .with("description", opt(&core.description))
            .with("last_modified_by", opt(&core.last_modified_by))
            .with("created", opt(&core.created))
            .with("modified", opt(&core.modified));
        let sections = 1 + self.model.body.iter().filter(|b| matches!(b, model::Block::Para(p) if p.sect.is_some())).count();
        let (w, h) = self.pages.first().map(|d| (d.width, d.height)).unwrap_or((612.0, 792.0));
        let mut styles: Vec<(&str, &str, &str)> = self
            .model
            .styles
            .map
            .values()
            .filter_map(|s| {
                let kind = match s.kind {
                    styles::StyleKind::Paragraph => "paragraph",
                    styles::StyleKind::Character => "character",
                    styles::StyleKind::Table => "table",
                    styles::StyleKind::Numbering => return None,
                };
                Some((s.id.as_str(), s.name.as_str(), kind))
            })
            .collect();
        styles.sort();
        let styles: Vec<Json> = styles.into_iter().map(|(id, name, kind)| Json::obj().with("id", id).with("name", name).with("type", kind)).collect();
        let parts: Vec<Json> = {
            let mut v: Vec<&String> = self.model.headers.keys().collect();
            v.sort();
            v.into_iter().map(|k| Json::Str(k.clone())).collect()
        };
        Json::obj()
            .with("pages", self.pages.len())
            .with("paragraphs", self.model.flat.len())
            .with("tables", self.model.tables)
            .with("pictures", self.model.pictures)
            .with("floating_pictures", self.model.floating)
            .with("sections", sections)
            .with("page_size", Json::obj().with("width", w).with("height", h))
            .with("headings", headings)
            .with("properties", props)
            .with("styles", Json::Arr(styles))
            .with("header_footer_parts", Json::Arr(parts))
            .with("positions", "a place is { \"p\": paragraph (flat, table cells included), \"o\": character offset }, or in a header or footer { \"h\": part, \"p\", \"o\" }; \"sel\" is { \"anchor\": place, \"focus\": place }")
    }

    fn read(&self, what: &Json) -> Result<Json, BusError> {
        if let Some(v) = what.get("paragraph") {
            let i = v.as_usize().ok_or_else(|| BusError::bad_args("", "\"paragraph\" is a number from 0"))?;
            if i >= self.model.flat.len() {
                return Err(BusError::failed(format!("no paragraph {i} (the document has {})", self.model.flat.len())));
            }
            return Ok(self.para_json(i));
        }
        if let Some(v) = what.get("range") {
            let (a, b) = match v.as_arr() {
                Some([a, b]) => (a.as_usize(), b.as_usize()),
                _ => (None, None),
            };
            let (Some(a), Some(b)) = (a, b) else {
                return Err(BusError::bad_args("", "\"range\" is [first, last] paragraph numbers (both included)"));
            };
            let n = self.model.flat.len();
            if n == 0 || a >= n || a > b {
                return Ok(Json::obj().with("paragraphs", Json::arr()));
            }
            let b = b.min(n - 1);
            let list: Vec<Json> = (a..=b).map(|i| self.para_json(i)).collect();
            return Ok(Json::obj().with("paragraphs", Json::Arr(list)));
        }
        if let Some(v) = what.get("page") {
            let n = v.as_usize().ok_or_else(|| BusError::bad_args("", "\"page\" is a number from 0"))?;
            let Some(p) = self.laid.pages.get(n) else {
                return Err(BusError::failed(format!("no page {n} (the document has {})", self.laid.pages.len())));
            };
            return Ok(Json::obj()
                .with("page", n)
                .with("number", p.number)
                .with("text", p.body_text.as_str())
                .with("header", p.header_text.as_str())
                .with("footer", p.footer_text.as_str()));
        }
        Err(BusError::bad_args("", "read takes { \"paragraph\": n }, { \"range\": [a, b] } or { \"page\": n }"))
    }

    fn display(&self, page: usize) -> DisplayList {
        self.pages.get(page).cloned().unwrap_or_else(|| DisplayList::new(612.0, 792.0))
    }

    fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn commands(&self) -> Vec<CommandSpec> {
        let mut v = vec![
            CommandSpec::new("scroll.set_title", "Set the document's title (a core property; null removes it).", "{ \"title\": string | null }"),
            CommandSpec::new(
                "scroll.set_paragraph_text",
                "Replace one paragraph's text, keeping its paragraph formatting and its first run's character formatting. \\t is a tab, \\n a line break.",
                "{ \"index\": number, \"text\": string }",
            ),
        ];
        v.extend(write_specs());
        v
    }

    fn resources(&self) -> Option<&Resources> {
        Some(&self.resources)
    }

    fn editing(&self) -> Option<&dyn Editing> {
        Some(self)
    }

    fn pdf_resources(&self) -> Option<PageResources> {
        let mut r = PageResources::new();
        for id in 1..=self.fonts.font_count() as u32 {
            if let Some(f) = self.fonts.font(id) {
                r.add_font(id, f.clone());
            }
        }
        Some(r)
    }
}

/// The writing commands, as the AI and the palette see them.
fn write_specs() -> Vec<CommandSpec> {
    const SEL: &str = "\"sel\": { \"anchor\": place, \"focus\": place }";
    let c = |name: &str, summary: &str, args: &str| CommandSpec::new(name, summary, args.replace("SEL", SEL));
    vec![
        c("scroll.insert_text", "Type text at the selection (replacing it). \\n starts a new paragraph, \\u000b is a line break, \\t a tab.", "{ SEL, \"text\": string }"),
        c("scroll.delete", "Delete the selection, or one character (a word with word:true) before (back:true) or after the caret; joins paragraphs at their edges.", "{ SEL, \"back\"?: bool, \"word\"?: bool }"),
        c("scroll.enter", "Enter: a new paragraph (shift: a line break, ctrl: a page break).", "{ SEL, \"shift\"?: bool, \"ctrl\"?: bool }"),
        c("scroll.tab", "Tab: a tab character, the next table cell (a new row after the last), or a list level down (back: up).", "{ SEL, \"back\"?: bool }"),
        c("scroll.paste", "Paste text (or our own rich copy, or HTML as text) at the selection.", "{ SEL, \"text\": string, \"html\"?: string, \"rich\"?: object }"),
        c("scroll.format", "Character formatting for the selection: bold, italic, underline (true/false or single, double, thick, dotted, dash, wave, words...), underline_color, strike, double_strike, caps, small_caps, hidden, color (hex or auto), highlight (yellow, green... or null), size (pt), font (family, +Body or +Headings), font_east_asian, vert (superscript, subscript, baseline), char_style (id), spacing (pt), clear (true removes all).", "{ SEL, ...properties }"),
        c("scroll.toggle", "Turn a character property on or off for the selection (bold, italic, underline, strike, caps, small_caps, superscript, subscript).", "{ SEL, \"prop\": string }"),
        c("scroll.font_size_step", "Grow (delta 1) or shrink (delta -1) the font size one step.", "{ SEL, \"delta\": 1 | -1 }"),
        c("scroll.clear_format", "Clear direct character and paragraph formatting (lists and section breaks stay).", "{ SEL }"),
        c("scroll.paragraph", "Paragraph formatting for the selected paragraphs: align (left, center, right, justify), line (multiple, or points with line_rule exact/atLeast), before, after, ind_left, ind_right, first_line, hanging (pt), keep_next, keep_lines, widow, page_break_before, contextual, bidi (bool), tabs [{pos, kind, leader}], add_tab, clear_tab (pt), clear_tabs, style (id), outline, shading (hex).", "{ SEL, ...properties }"),
        c("scroll.align", "Align the selected paragraphs.", "{ SEL, \"value\": \"left\" | \"center\" | \"right\" | \"justify\" }"),
        c("scroll.indent", "Increase (delta 1) or decrease (-1) the indent; in a list, the level.", "{ SEL, \"delta\": 1 | -1 }"),
        c("scroll.style", "Apply a paragraph or character style by id or name (Heading 1, Title, Quote, Strong... are added when missing).", "{ SEL, \"style\": string }"),
        c("scroll.list", "Make the selected paragraphs a list (again: back to plain), with an optional first-level number format (decimal, lowerLetter, upperRoman...) and level text (\"%1)\").", "{ SEL, \"kind\": \"bullet\" | \"number\" | \"multilevel\" | \"none\", \"format\"?: string, \"text\"?: string }"),
        c("scroll.list_level", "Demote (delta 1) or promote (-1) list items one level.", "{ SEL, \"delta\": 1 | -1 }"),
        c("scroll.list_restart", "Restart numbering at this list item (at start, default 1).", "{ SEL, \"start\"?: number }"),
        c("scroll.find", "Find text from the selection (forward, or back with next:false); the match comes back as the selection.", "{ SEL, \"query\": string, \"next\": bool, \"case\"?: bool, \"whole\"?: bool }"),
        c("scroll.replace", "Replace the selected match and go to the next, or every match with all:true (each keeps its first character's formatting).", "{ SEL, \"query\": string, \"with\": string, \"all\": bool, \"case\"?: bool, \"whole\"?: bool }"),
        c("scroll.select", "Select the word, paragraph, cell, row, column, table or everything at the selection (changes nothing).", "{ SEL, \"what\": string }"),
        c("scroll.insert_table", "Insert a table of rows x cols at the caret.", "{ SEL, \"rows\": number, \"cols\": number }"),
        c("scroll.table_insert", "Insert rows above or below, or columns left or right of the selection.", "{ SEL, \"what\": \"row_above\" | \"row_below\" | \"col_left\" | \"col_right\", \"count\"?: number }"),
        c("scroll.table_delete", "Delete the selected rows, columns or the whole table.", "{ SEL, \"what\": \"row\" | \"col\" | \"table\" }"),
        c("scroll.merge_cells", "Merge the selected cells (across and down).", "{ SEL }"),
        c("scroll.table_widths", "Set column widths (points), or AutoFit to the window or the contents, or fix them.", "{ SEL, \"widths\"?: [number], \"autofit\"?: \"window\" | \"contents\" | \"fixed\" }"),
        c("scroll.row_height", "Row height in points (null: automatic), exact or at least.", "{ SEL, \"height\": number | null, \"exact\"?: bool }"),
        c("scroll.row_break", "Allow or prevent the selected rows from breaking across pages.", "{ SEL, \"allow\": bool }"),
        c("scroll.header_rows", "Repeat the rows from the top through the selection at the top of each page (on:false stops).", "{ SEL, \"on\": bool }"),
        c("scroll.cell_align", "Align text in the selected cells: v (top, center, bottom) and h (left, center, right, justify).", "{ SEL, \"v\"?: string, \"h\"?: string }"),
        c("scroll.shading", "Fill the selected cells with a colour (null removes it).", "{ SEL, \"color\": string | null }"),
        c("scroll.borders", "Borders of the table or the selected cells: which (all, outside, inside, none, top, bottom, left, right, inside_h, inside_v), style (single, double, dashed, dotted, thick, none), size (pt), color, scope (table or cells).", "{ SEL, \"which\": string, \"style\"?: string, \"size\"?: number, \"color\"?: string, \"scope\"?: string }"),
        c("scroll.table_align", "Place the table left, center or right (indent in points).", "{ SEL, \"align\": string, \"indent\"?: number }"),
        c("scroll.page_break", "Insert a page break at the caret.", "{ SEL }"),
        c("scroll.section_break", "End the section at the caret; the next one starts on a new page, continuously, or on an even or odd page.", "{ SEL, \"kind\": \"nextPage\" | \"continuous\" | \"evenPage\" | \"oddPage\" }"),
        c("scroll.page_setup", "Margins (top, bottom, left, right, header, footer, gutter in pt), orientation (portrait, landscape) and size (Letter, A4, Legal... or width and height) of the selection's section, or every section with scope all.", "{ SEL, \"top\"?: number, ..., \"orientation\"?: string, \"size\"?: string, \"scope\"?: \"section\" | \"all\" }"),
        c("scroll.header_footer", "Open the section's header or footer (default, first or even; created when missing), optionally replacing its text; the caret lands in it.", "{ SEL, \"which\": \"header\" | \"footer\", \"kind\"?: \"default\" | \"first\" | \"even\", \"text\"?: string }"),
        c("scroll.page_number", "Insert a page number (plain, page_x, page_x_of_y, x_of_y) in the header, the footer (aligned) or here at the caret.", "{ SEL, \"where\": \"header\" | \"footer\" | \"here\", \"align\"?: string, \"format\"?: string }"),
        c("scroll.page_numbering", "Restart page numbers at start in the section (no start: continue), with a format (decimal, lowerRoman, upperRoman, lowerLetter, upperLetter).", "{ SEL, \"start\"?: number, \"format\"?: string }"),
        c("scroll.first_page", "A different first page header and footer for the section (on or off).", "{ SEL, \"on\": bool }"),
        c("scroll.insert_picture", "Insert a picture (PNG, JPEG, GIF, BMP bytes as base64) at the caret; width or height alone keeps its proportions.", "{ SEL, \"data\": base64, \"name\": string, \"width\"?: number, \"height\"?: number }"),
        c("scroll.picture_size", "Resize the selected picture (points); width or height alone keeps its proportions.", "{ SEL, \"width\"?: number, \"height\"?: number }"),
        c("scroll.link", "Link the selection to a web address, mailto: or #bookmark (a caret in a link changes it; a caret elsewhere inserts text as a new link).", "{ SEL, \"url\": string, \"text\"?: string }"),
        c("scroll.unlink", "Remove the links the selection touches (the text stays).", "{ SEL }"),
    ]
}

// ---------------------------------------------------------------------------------------------------
// The blank document
// ---------------------------------------------------------------------------------------------------

const BLANK_DOCUMENT: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
    "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">",
    "<w:body><w:p/>",
    "<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>",
    "<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/>",
    "</w:sectPr></w:body></w:document>"
);

const BLANK_STYLES: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
    "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme=\"minorHAnsi\" w:hAnsiTheme=\"minorHAnsi\" w:eastAsiaTheme=\"minorEastAsia\"/>",
    "<w:sz w:val=\"22\"/><w:lang w:val=\"en-US\"/></w:rPr></w:rPrDefault>",
    "<w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"259\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>",
    "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/><w:sz w:val=\"56\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/>",
    "<w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"0\"/><w:outlineLvl w:val=\"0\"/></w:pPr>",
    "<w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/><w:color w:val=\"2F5496\" w:themeColor=\"accent1\" w:themeShade=\"BF\"/><w:sz w:val=\"32\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/>",
    "<w:pPr><w:keepNext/><w:spacing w:before=\"40\" w:after=\"0\"/><w:outlineLvl w:val=\"1\"/></w:pPr>",
    "<w:rPr><w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/><w:color w:val=\"2F5496\" w:themeColor=\"accent1\" w:themeShade=\"BF\"/><w:sz w:val=\"26\"/></w:rPr></w:style>",
    "<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/></w:style>",
    "<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:basedOn w:val=\"DefaultParagraphFont\"/>",
    "<w:rPr><w:color w:val=\"0563C1\" w:themeColor=\"hyperlink\"/><w:u w:val=\"single\"/></w:rPr></w:style>",
    "<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/>",
    "<w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/>",
    "<w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>",
    "<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:basedOn w:val=\"TableNormal\"/>",
    "<w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:tblPr><w:tblBorders>",
    "<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>",
    "<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>",
    "<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>",
    "</w:tblBorders></w:tblPr></w:style>",
    "</w:styles>"
);
