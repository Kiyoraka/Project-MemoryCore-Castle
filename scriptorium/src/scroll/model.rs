//! The document model: `word/document.xml`'s body (paragraphs, runs, tables, inline pictures,
//! sections), the header and footer parts, and everything the layout needs alongside (styles,
//! numbering, theme, settings, decoded pictures).
//!
//! The model is read-only and rebuilt from the package after every change: the package's XML stays
//! the one truth, so whatever the model does not understand is never lost.

use super::numbering::Numbering;
use super::props::{attr, num_attr, on_off, twips_attr, val, Borders, Ctx, Margins, ParaProps, RunProps};
use super::styles::Styles;
use crate::ground::display::{Color, ImageRes};
use crate::ground::draw::{emu_to_pt, ColorMap, Theme};
use crate::ground::opc::{self, Package};
use crate::ground::picture::{jpeg, png};
use crate::ground::text::Align;
use crate::ground::xml::{parse_dom, Element};
use std::collections::HashMap;

// ---------------------------------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------------------------------

/// A field whose result the layout computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Page,
    NumPages,
}

/// An inline picture: its decoded image (None when the format is not one we decode) and its size in
/// points (from `wp:extent`).
#[derive(Debug, Clone, PartialEq)]
pub struct Pic {
    pub image: Option<u32>,
    pub w: f64,
    pub h: f64,
}

/// One piece of a run.
#[derive(Debug, Clone, PartialEq)]
pub enum RunItem {
    Text(String),
    Tab,
    /// A line break (`w:br`, `w:cr`).
    Break,
    /// `w:br w:type="page"` (and column breaks, which act as page breaks in one column).
    PageBreak,
    /// `w:sym`: a character in its own font.
    Sym(char, Option<String>),
    Pic(Pic),
    Field(FieldKind),
}

/// What XML a run was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// A `w:r`.
    R,
    /// A `w:fldSimple` (its one field item).
    FldSimple,
    /// An `m:oMath` / `m:oMathPara` (its text, read only).
    Math,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub rpr: RunProps,
    pub items: Vec<RunItem>,
    /// The hyperlink target when the run sits in a `w:hyperlink`.
    pub link: Option<String>,
    /// Element-only child indices from the paragraph (`w:p`) down to the run's element.
    pub src: Vec<usize>,
    pub kind: RunKind,
    /// For each item, the element index (among the run element's element children) it was read
    /// from (0 for a field simple or math run).
    pub item_src: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Para {
    pub ppr: ParaProps,
    /// The paragraph mark's run properties (`w:pPr/w:rPr`).
    pub mark: RunProps,
    pub runs: Vec<Run>,
    /// `w:pPr/w:sectPr`: this paragraph ends a section.
    pub sect: Option<SectPr>,
    /// Index among all body paragraphs in reading order (`usize::MAX` in headers and footers).
    pub flat: usize,
    /// Index among the paragraphs of its header or footer part (`usize::MAX` in the body).
    pub hidx: usize,
    /// The complex fields still open when the paragraph starts (a field may span paragraphs).
    pub fields_in: Vec<FieldState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMerge {
    None,
    Restart,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub span: usize,
    pub vmerge: VMerge,
    pub borders: Borders,
    pub fill: Option<Color>,
    pub mar: Margins,
    pub valign: VAlign,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Points; `true` for an exact height, else at least.
    pub height: Option<(f64, bool)>,
    /// `w:tblHeader`: repeated at the top of each page the table continues on.
    pub header: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub style: Option<String>,
    /// Grid column widths in points.
    pub grid: Vec<f64>,
    pub rows: Vec<Row>,
    pub borders: Borders,
    pub cell_mar: Margins,
    pub ind: f64,
    pub align: Align,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Para(Para),
    Table(Table),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectKind {
    NextPage,
    Continuous,
    EvenPage,
    OddPage,
}

/// `w:sectPr`, in points. Header and footer references are resolved to part names
/// (`[default, first, even]`).
#[derive(Debug, Clone, PartialEq)]
pub struct SectPr {
    pub page_w: f64,
    pub page_h: f64,
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
    pub header: f64,
    pub footer: f64,
    pub landscape: bool,
    pub headers: [Option<String>; 3],
    pub footers: [Option<String>; 3],
    pub title_pg: bool,
    pub kind: SectKind,
    pub pg_start: Option<i64>,
    pub pg_fmt: Option<String>,
}

impl Default for SectPr {
    /// US Letter with one-inch margins (Word's default when a document says nothing).
    fn default() -> Self {
        SectPr {
            page_w: 612.0,
            page_h: 792.0,
            top: 72.0,
            bottom: 72.0,
            left: 72.0,
            right: 72.0,
            header: 36.0,
            footer: 36.0,
            landscape: false,
            headers: [None, None, None],
            footers: [None, None, None],
            title_pg: false,
            kind: SectKind::NextPage,
            pg_start: None,
            pg_fmt: None,
        }
    }
}

/// What `describe` and `read` know of each body paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatPara {
    pub text: String,
    /// Effective paragraph style id (the default paragraph style when none is set).
    pub style: Option<String>,
    /// Element-only child indices from `w:document` down to the `w:p`.
    pub path: Vec<usize>,
}

/// `word/settings.xml`, the parts that change layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub default_tab: f64,
    pub even_odd_headers: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { default_tab: 36.0, even_odd_headers: false }
    }
}

/// Decoded pictures, by part name, shared across rebuilds of the model.
#[derive(Debug, Clone, Default)]
pub struct ImageStore {
    by_part: HashMap<String, Option<u32>>,
    pub images: Vec<ImageRes>,
}

impl ImageStore {
    /// The image id of a picture part, decoding it on first use (PNG and JPEG; others give None).
    pub fn get(&mut self, pkg: &Package, part: &str) -> Option<u32> {
        let key = opc::name_key(part);
        if let Some(v) = self.by_part.get(&key) {
            return *v;
        }
        let decoded = pkg.part_bytes(part).ok().and_then(|bytes| {
            if bytes.starts_with(b"\x89PNG") {
                png::decode(&bytes).ok()
            } else if bytes.starts_with(&[0xFF, 0xD8]) {
                jpeg::decode(&bytes).ok()
            } else {
                None
            }
        });
        let id = decoded.map(|img| {
            let id = self.images.len() as u32 + 1;
            self.images.push(ImageRes { id, width: img.width, height: img.height, rgba: img.pixels });
            id
        });
        self.by_part.insert(key, id);
        id
    }
}

/// Everything the layout and the door need.
#[derive(Debug, Clone)]
pub struct Model {
    pub body: Vec<Block>,
    /// The body's final `w:sectPr` (the last section).
    pub final_sect: SectPr,
    pub headers: HashMap<String, Vec<Block>>,
    pub styles: Styles,
    pub numbering: Numbering,
    pub theme: Theme,
    pub settings: Settings,
    pub flat: Vec<FlatPara>,
    /// The paragraphs of each header and footer part, by part name (paths from the part's root).
    pub hflat: HashMap<String, Vec<FlatPara>>,
    /// `w:clrSchemeMapping` (theme colours resolve through it).
    pub cmap: ColorMap,
    pub tables: usize,
    pub pictures: usize,
    /// Anchored (floating) drawings: counted, not shown yet.
    pub floating: usize,
    pub has_numpages: bool,
}

// ---------------------------------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------------------------------

impl Model {
    /// Read the model out of a package whose main part is `main`.
    pub fn load(pkg: &Package, main: &str, images: &mut ImageStore) -> Result<Model, String> {
        let theme = pkg
            .related(main, opc::REL_THEME)
            .first()
            .and_then(|t| pkg.part_bytes(t).ok())
            .and_then(|b| Theme::parse(&b).ok())
            .unwrap_or_else(Theme::office_default);
        let mut settings = Settings::default();
        let mut cmap = ColorMap::default();
        if let Some(root) = pkg
            .related(main, opc::REL_SETTINGS)
            .first()
            .and_then(|s| pkg.part_bytes(s).ok())
            .and_then(|b| parse_dom(&b).ok())
        {
            if let Some(t) = root.child("defaultTabStop").and_then(|e| twips_attr(e, "val")) {
                if t > 0.0 {
                    settings.default_tab = t;
                }
            }
            settings.even_odd_headers = root.child("evenAndOddHeaders").is_some_and(on_off);
            if let Some(m) = root.child("clrSchemeMapping") {
                cmap = ColorMap::parse(m);
            }
        }
        let ctx = Ctx { theme: &theme, cmap: &cmap };
        let styles = pkg
            .related(main, opc::REL_STYLES)
            .first()
            .and_then(|s| pkg.part_bytes(s).ok())
            .map(|b| Styles::parse(&b, &ctx))
            .unwrap_or_default();
        let numbering = pkg
            .related(main, opc::REL_NUMBERING)
            .first()
            .and_then(|s| pkg.part_bytes(s).ok())
            .map(|b| Numbering::parse(&b, &ctx))
            .unwrap_or_default();
        let bytes = pkg.part_bytes(main).map_err(|e| e.to_string())?;
        let root = parse_dom(&bytes).map_err(|e| format!("{main}: {e}"))?;
        let (body_index, body_el) = root
            .elements()
            .enumerate()
            .find(|(_, e)| e.local() == "body")
            .ok_or_else(|| format!("{main} has no w:body"))?;

        let mut flat = Vec::new();
        let mut counts = Counts::default();
        let mut p = PartReader { pkg, part: main.to_string(), ctx: &ctx, images, flat: Some(&mut flat), fields: Vec::new(), counts: &mut counts, styles: &styles, light: false, body: true };
        let mut final_sect = SectPr::default();
        let body = p.blocks(body_el, &[body_index], Some(&mut final_sect));

        let mut headers = HashMap::new();
        let mut hflat = HashMap::new();
        let mut sects: Vec<SectPr> = Vec::new();
        collect_sects(&body, &mut sects);
        sects.push(final_sect.clone());
        for s in &sects {
            for name in s.headers.iter().chain(s.footers.iter()).flatten() {
                if headers.contains_key(name) {
                    continue;
                }
                let mut hf = Vec::new();
                let blocks = match pkg.part_bytes(name).ok().and_then(|b| parse_dom(&b).ok()) {
                    Some(hroot) => {
                        let mut hp = PartReader {
                            pkg,
                            part: name.clone(),
                            ctx: &ctx,
                            images,
                            flat: Some(&mut hf),
                            fields: Vec::new(),
                            counts: &mut counts,
                            styles: &styles,
                            light: false,
                            body: false,
                        };
                        hp.blocks(&hroot, &[], None)
                    }
                    None => Vec::new(),
                };
                headers.insert(name.clone(), blocks);
                hflat.insert(name.clone(), hf);
            }
        }
        Ok(Model {
            body,
            final_sect,
            headers,
            styles,
            numbering,
            theme,
            settings,
            flat,
            hflat,
            cmap,
            tables: counts.tables,
            pictures: counts.pictures,
            floating: counts.floating,
            has_numpages: counts.numpages,
        })
    }
}

fn collect_sects(blocks: &[Block], out: &mut Vec<SectPr>) {
    for b in blocks {
        if let Block::Para(p) = b {
            if let Some(s) = &p.sect {
                out.push(s.clone());
            }
        }
    }
}

#[derive(Default)]
struct Counts {
    tables: usize,
    pictures: usize,
    floating: usize,
    numpages: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Instr,
    Result,
}

/// One complex field open while reading (its instruction so far, which half we are in).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldState {
    pub instr: String,
    pub phase: Phase,
    pub dynamic: Option<FieldKind>,
    pub emitted: bool,
}

fn field_kind(instr: &str) -> Option<FieldKind> {
    let word = instr.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    match word.as_str() {
        "PAGE" => Some(FieldKind::Page),
        "NUMPAGES" => Some(FieldKind::NumPages),
        _ => None,
    }
}

/// Reads one part's blocks.
struct PartReader<'a, 'b> {
    pkg: &'a Package,
    part: String,
    ctx: &'a Ctx<'a>,
    images: &'b mut ImageStore,
    flat: Option<&'b mut Vec<FlatPara>>,
    fields: Vec<FieldState>,
    counts: &'b mut Counts,
    styles: &'a Styles,
    /// Structure only: pictures are not decoded.
    light: bool,
    /// Reading the body (paragraphs get `flat`), else a header or footer (`hidx`).
    body: bool,
}

/// A paragraph element's runs read again from (possibly edited) XML, with the fields open at its
/// start: the same reading the model does, so the editing code and the layout agree on every
/// character. Pictures are not decoded.
pub fn read_runs(pkg: &Package, part: &str, model: &Model, p: &Element, fields_in: &[FieldState]) -> Vec<Run> {
    let ctx = Ctx { theme: &model.theme, cmap: &model.cmap };
    let mut images = ImageStore::default();
    let mut counts = Counts::default();
    let mut r = PartReader {
        pkg,
        part: part.to_string(),
        ctx: &ctx,
        images: &mut images,
        flat: None,
        fields: fields_in.to_vec(),
        counts: &mut counts,
        styles: &model.styles,
        light: true,
        body: false,
    };
    let mut runs = Vec::new();
    r.inline_children(p, None, &[], &mut runs);
    runs
}

/// A `w:rPr` element read as properties (for the editing code).
pub fn parse_rpr(model: &Model, el: &Element) -> RunProps {
    RunProps::parse(el, &Ctx { theme: &model.theme, cmap: &model.cmap })
}

/// A `w:pPr` element read as properties (for the editing code).
pub fn parse_ppr(model: &Model, el: &Element) -> ParaProps {
    ParaProps::parse(el, &Ctx { theme: &model.theme, cmap: &model.cmap })
}

impl<'a, 'b> PartReader<'a, 'b> {
    /// The block-level children of a body, cell, header, footer or content control.
    fn blocks(&mut self, parent: &Element, path: &[usize], mut final_sect: Option<&mut SectPr>) -> Vec<Block> {
        let mut out = Vec::new();
        for (i, el) in parent.elements().enumerate() {
            let mut here = path.to_vec();
            here.push(i);
            match el.local() {
                "p" => out.push(Block::Para(self.para(el, &here))),
                "tbl" => out.push(Block::Table(self.table(el, &here))),
                "sdt" => {
                    if let Some((ci, content)) = el.elements().enumerate().find(|(_, e)| e.local() == "sdtContent") {
                        here.push(ci);
                        out.extend(self.blocks(content, &here, None));
                    }
                }
                "customXml" | "ins" | "moveTo" => out.extend(self.blocks(el, &here, None)),
                "sectPr" => {
                    if let Some(fs) = final_sect.as_deref_mut() {
                        *fs = self.sect(el);
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn para(&mut self, el: &Element, path: &[usize]) -> Para {
        let mut para = Para {
            ppr: ParaProps::default(),
            mark: RunProps::default(),
            runs: Vec::new(),
            sect: None,
            flat: usize::MAX,
            hidx: usize::MAX,
            fields_in: self.fields.clone(),
        };
        if let Some(ppr) = el.child("pPr") {
            para.ppr = ParaProps::parse(ppr, self.ctx);
            if let Some(r) = ppr.child("rPr") {
                para.mark = RunProps::parse(r, self.ctx);
            }
            if let Some(s) = ppr.child("sectPr") {
                para.sect = Some(self.sect(s));
            }
        }
        self.inline_children(el, None, &[], &mut para.runs);
        if let Some(flat) = self.flat.as_deref_mut() {
            if self.body {
                para.flat = flat.len();
            } else {
                para.hidx = flat.len();
            }
            let style = para.ppr.style.clone().or_else(|| self.styles.default_para.clone());
            flat.push(FlatPara { text: plain_text(&para.runs), style, path: path.to_vec() });
        }
        para
    }

    /// The run-level content of a paragraph (or of a hyperlink, field, content control...).
    fn inline_children(&mut self, parent: &Element, link: Option<&str>, path: &[usize], runs: &mut Vec<Run>) {
        for (ci, c) in parent.elements().enumerate() {
            let mut here = path.to_vec();
            here.push(ci);
            match c.local() {
                "r" => {
                    if let Some(r) = self.run(c, link, &here) {
                        runs.push(r);
                    }
                }
                "hyperlink" => {
                    let target = attr(c, "id")
                        .and_then(|id| self.pkg.rel_by_id(&self.part, id))
                        .map(|r| r.target.clone())
                        .or_else(|| attr(c, "anchor").map(|a| format!("#{a}")));
                    self.inline_children(c, target.as_deref().or(link), &here, runs);
                }
                "fldSimple" => {
                    let instr = attr(c, "instr").unwrap_or("");
                    match field_kind(instr) {
                        Some(kind) => {
                            if kind == FieldKind::NumPages {
                                self.counts.numpages = true;
                            }
                            let rpr = c
                                .child("r")
                                .and_then(|r| r.child("rPr"))
                                .map(|r| RunProps::parse(r, self.ctx))
                                .unwrap_or_default();
                            runs.push(Run {
                                rpr,
                                items: vec![RunItem::Field(kind)],
                                link: link.map(str::to_string),
                                src: here,
                                kind: RunKind::FldSimple,
                                item_src: vec![0],
                            });
                        }
                        None => self.inline_children(c, link, &here, runs),
                    }
                }
                "ins" | "smartTag" | "customXml" | "dir" | "bdo" | "moveTo" => self.inline_children(c, link, &here, runs),
                "sdt" => {
                    if let Some((si, content)) = c.elements().enumerate().find(|(_, e)| e.local() == "sdtContent") {
                        here.push(si);
                        self.inline_children(content, link, &here, runs);
                    }
                }
                "oMathPara" | "oMath" => {
                    let mut text = String::new();
                    math_text(c, &mut text);
                    if !text.is_empty() {
                        runs.push(Run { rpr: RunProps::default(), items: vec![RunItem::Text(text)], link: None, src: here, kind: RunKind::Math, item_src: vec![0] });
                    }
                }
                _ => {}
            }
        }
    }

    /// True while run content is hidden by a field (its instruction, or a cached result the layout
    /// computes itself).
    fn suppressed(&self) -> bool {
        self.fields.iter().any(|f| f.phase == Phase::Instr || f.dynamic.is_some())
    }

    fn run(&mut self, el: &Element, link: Option<&str>, path: &[usize]) -> Option<Run> {
        let rpr = el.child("rPr").map(|r| RunProps::parse(r, self.ctx)).unwrap_or_default();
        let mut items: Vec<RunItem> = Vec::new();
        let mut item_src: Vec<usize> = Vec::new();
        for (ei, c) in el.elements().enumerate() {
            self.run_child(c, &mut items);
            item_src.resize(items.len(), ei);
        }
        if items.is_empty() {
            return None;
        }
        Some(Run { rpr, items, link: link.map(str::to_string), src: path.to_vec(), kind: RunKind::R, item_src })
    }

    /// One child element of a `w:r`, read into items.
    fn run_child(&mut self, c: &Element, items: &mut Vec<RunItem>) {
        {
            match c.local() {
                "fldChar" => match attr(c, "fldCharType").map(str::trim) {
                    Some("begin") => self.fields.push(FieldState { instr: String::new(), phase: Phase::Instr, dynamic: None, emitted: false }),
                    Some("separate") => {
                        if let Some(f) = self.fields.last_mut() {
                            f.phase = Phase::Result;
                            f.dynamic = field_kind(&f.instr);
                        }
                        let outer_hidden = self.fields.len() > 1 && {
                            let n = self.fields.len();
                            self.fields[..n - 1].iter().any(|f| f.phase == Phase::Instr || f.dynamic.is_some())
                        };
                        if let Some(f) = self.fields.last_mut() {
                            if let Some(k) = f.dynamic {
                                if !outer_hidden {
                                    items.push(RunItem::Field(k));
                                }
                                f.emitted = true;
                                if k == FieldKind::NumPages {
                                    self.counts.numpages = true;
                                }
                            }
                        }
                    }
                    Some("end") => {
                        if let Some(f) = self.fields.pop() {
                            let kind = if f.phase == Phase::Instr { field_kind(&f.instr) } else { f.dynamic };
                            if let (Some(k), false) = (kind, f.emitted) {
                                if !self.suppressed() {
                                    items.push(RunItem::Field(k));
                                }
                                if k == FieldKind::NumPages {
                                    self.counts.numpages = true;
                                }
                            }
                        }
                    }
                    _ => {}
                },
                "instrText" => {
                    if let Some(f) = self.fields.last_mut() {
                        if f.phase == Phase::Instr {
                            f.instr.push_str(&c.text());
                        }
                    }
                }
                _ if self.suppressed() => {}
                "t" => {
                    let t = c.text();
                    if !t.is_empty() {
                        items.push(RunItem::Text(t));
                    }
                }
                "tab" | "ptab" => items.push(RunItem::Tab),
                "br" => match attr(c, "type").map(str::trim) {
                    Some("page") | Some("column") => items.push(RunItem::PageBreak),
                    _ => items.push(RunItem::Break),
                },
                "cr" => items.push(RunItem::Break),
                "noBreakHyphen" => items.push(RunItem::Text("\u{2011}".into())),
                "softHyphen" => items.push(RunItem::Text("\u{AD}".into())),
                "sym" => {
                    let ch = attr(c, "char").and_then(|h| u32::from_str_radix(h.trim(), 16).ok()).and_then(char::from_u32);
                    if let Some(ch) = ch {
                        items.push(RunItem::Sym(ch, attr(c, "font").map(str::to_string)));
                    }
                }
                "drawing" => self.drawing(c, items),
                "AlternateContent" => {
                    // Prefer a choice that holds a DrawingML picture; VML fallbacks are not drawn yet.
                    for alt in c.elements() {
                        if let Some(d) = alt.elements().find(|e| e.local() == "drawing") {
                            self.drawing(d, items);
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn drawing(&mut self, el: &Element, items: &mut Vec<RunItem>) {
        for d in el.elements() {
            match d.local() {
                "inline" => {
                    self.counts.pictures += 1;
                    let (w, h) = d
                        .child("extent")
                        .map(|e| (emu_to_pt(num_attr(e, "cx").unwrap_or(0.0)), emu_to_pt(num_attr(e, "cy").unwrap_or(0.0))))
                        .unwrap_or((0.0, 0.0));
                    let image = find_blip(d)
                        .filter(|_| !self.light)
                        .and_then(|id| self.pkg.rel_by_id(&self.part, id))
                        .filter(|r| !r.target_mode_external)
                        .map(|r| self.pkg.resolve(&self.part, r))
                        .and_then(|name| self.images.get(self.pkg, &name));
                    items.push(RunItem::Pic(Pic { image, w: w.max(0.0), h: h.max(0.0) }));
                }
                "anchor" => {
                    self.counts.pictures += 1;
                    self.counts.floating += 1;
                }
                _ => {}
            }
        }
    }

    fn table(&mut self, el: &Element, path: &[usize]) -> Table {
        self.counts.tables += 1;
        let mut t = Table { style: None, grid: Vec::new(), rows: Vec::new(), borders: Borders::default(), cell_mar: Margins::default(), ind: 0.0, align: Align::Left };
        if let Some(tpr) = el.child("tblPr") {
            t.style = tpr.child("tblStyle").and_then(val).map(str::to_string);
            if let Some(b) = tpr.child("tblBorders") {
                t.borders = Borders::parse(b, self.ctx);
            }
            if let Some(m) = tpr.child("tblCellMar") {
                t.cell_mar = Margins::parse(m);
            }
            if let Some(i) = tpr.child("tblInd").and_then(|e| twips_attr(e, "w")) {
                t.ind = i;
            }
            t.align = match tpr.child("jc").and_then(val).map(str::trim) {
                Some("center") => Align::Center,
                Some("right") | Some("end") => Align::Right,
                _ => Align::Left,
            };
        }
        if let Some(g) = el.child("tblGrid") {
            t.grid = g.children_named("gridCol").map(|c| twips_attr(c, "w").unwrap_or(0.0).max(0.0)).collect();
        }
        for (ri, row) in el.elements().enumerate() {
            if row.local() != "tr" {
                continue;
            }
            let mut r = Row { cells: Vec::new(), height: None, header: false };
            if let Some(trpr) = row.child("trPr") {
                if let Some(h) = trpr.child("trHeight") {
                    if let Some(v) = twips_attr(h, "val") {
                        r.height = Some((v, attr(h, "hRule").is_some_and(|x| x == "exact")));
                    }
                }
                r.header = trpr.child("tblHeader").is_some_and(on_off);
            }
            let mut cells_path = path.to_vec();
            cells_path.push(ri);
            self.cells(row, &cells_path, &mut r.cells);
            t.rows.push(r);
        }
        t
    }

    fn cells(&mut self, row: &Element, path: &[usize], out: &mut Vec<Cell>) {
        for (ci, tc) in row.elements().enumerate() {
            let mut here = path.to_vec();
            here.push(ci);
            match tc.local() {
                "tc" => {}
                "sdt" | "customXml" => {
                    if let Some((si, content)) = tc.elements().enumerate().find(|(_, e)| e.local() == "sdtContent") {
                        here.push(si);
                        self.cells(content, &here, out);
                    } else if tc.local() == "customXml" {
                        self.cells(tc, &here, out);
                    }
                    continue;
                }
                _ => continue,
            }
            let mut cell = Cell { span: 1, vmerge: VMerge::None, borders: Borders::default(), fill: None, mar: Margins::default(), valign: VAlign::Top, blocks: Vec::new() };
            if let Some(tcpr) = tc.child("tcPr") {
                if let Some(s) = tcpr.child("gridSpan").and_then(|e| num_attr(e, "val")) {
                    cell.span = (s as usize).max(1);
                }
                if let Some(v) = tcpr.child("vMerge") {
                    cell.vmerge = if val(v).is_some_and(|x| x == "restart") { VMerge::Restart } else { VMerge::Continue };
                }
                if let Some(b) = tcpr.child("tcBorders") {
                    cell.borders = Borders::parse(b, self.ctx);
                }
                if let Some(s) = tcpr.child("shd") {
                    cell.fill = super::props::word_color(s, self.ctx, "fill", "themeFill", "themeFillTint", "themeFillShade");
                }
                if let Some(m) = tcpr.child("tcMar") {
                    cell.mar = Margins::parse(m);
                }
                cell.valign = match tcpr.child("vAlign").and_then(val).map(str::trim) {
                    Some("center") => VAlign::Center,
                    Some("bottom") => VAlign::Bottom,
                    _ => VAlign::Top,
                };
            }
            cell.blocks = self.blocks(tc, &here, None);
            out.push(cell);
        }
    }

    fn sect(&mut self, el: &Element) -> SectPr {
        let mut s = SectPr::default();
        if let Some(pg) = el.child("pgSz") {
            if let Some(w) = twips_attr(pg, "w") {
                s.page_w = w;
            }
            if let Some(h) = twips_attr(pg, "h") {
                s.page_h = h;
            }
            s.landscape = attr(pg, "orient").is_some_and(|o| o == "landscape");
            // A landscape page written with portrait numbers is turned.
            if s.landscape && s.page_w < s.page_h {
                std::mem::swap(&mut s.page_w, &mut s.page_h);
            }
        }
        if let Some(m) = el.child("pgMar") {
            let g = |k: &str, d: f64| twips_attr(m, k).unwrap_or(d);
            s.top = g("top", s.top);
            s.bottom = g("bottom", s.bottom);
            s.left = g("left", s.left).max(0.0) + twips_attr(m, "gutter").unwrap_or(0.0).max(0.0);
            s.right = g("right", s.right).max(0.0);
            s.header = g("header", s.header).max(0.0);
            s.footer = g("footer", s.footer).max(0.0);
        }
        for (local, slot) in [("headerReference", 0usize), ("footerReference", 1)] {
            for r in el.children_named(local) {
                let which = match attr(r, "type").map(str::trim) {
                    Some("first") => 1,
                    Some("even") => 2,
                    _ => 0,
                };
                let name = attr(r, "id")
                    .and_then(|id| self.pkg.rel_by_id(&self.part, id))
                    .map(|rel| self.pkg.resolve(&self.part, rel));
                if slot == 0 {
                    s.headers[which] = name;
                } else {
                    s.footers[which] = name;
                }
            }
        }
        s.title_pg = el.child("titlePg").is_some_and(on_off);
        s.kind = match el.child("type").and_then(val).map(str::trim) {
            Some("continuous") => SectKind::Continuous,
            Some("evenPage") => SectKind::EvenPage,
            Some("oddPage") => SectKind::OddPage,
            _ => SectKind::NextPage,
        };
        if let Some(n) = el.child("pgNumType") {
            s.pg_start = num_attr(n, "start").map(|v| v as i64);
            s.pg_fmt = attr(n, "fmt").map(str::to_string);
        }
        s
    }
}

/// The `r:embed` of the first `a:blip` inside a drawing.
fn find_blip(el: &Element) -> Option<&str> {
    if el.local() == "blip" {
        if let Some(v) = attr(el, "embed") {
            return Some(v);
        }
    }
    el.elements().find_map(find_blip)
}

fn math_text(el: &Element, out: &mut String) {
    for c in el.elements() {
        if c.local() == "t" {
            out.push_str(&c.text());
        } else {
            math_text(c, out);
        }
    }
}

/// A paragraph's plain text: text, tabs as `\t`, line breaks as `\n`, symbols; fields and pictures
/// leave nothing.
pub fn plain_text(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        if r.rpr.hidden == Some(true) {
            continue;
        }
        for it in &r.items {
            match it {
                RunItem::Text(t) => s.push_str(t),
                RunItem::Tab => s.push('\t'),
                RunItem::Break => s.push('\n'),
                RunItem::Sym(c, _) => s.push(*c),
                RunItem::PageBreak | RunItem::Pic(_) | RunItem::Field(_) => {}
            }
        }
    }
    s
}
