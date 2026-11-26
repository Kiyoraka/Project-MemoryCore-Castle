//! Ledger: our spreadsheet. M1 is "open and see": an `.xlsx` opens into sparse sheets, every sheet
//! draws as a virtualised grid (headers, gridlines, fills, borders, fonts, alignment, overflow,
//! merged cells, frozen panes), number formats are interpreted by our own engine, the AI's door
//! can describe the workbook and read ranges, and a cell can be set (and undone) with the rest of
//! the file left byte for byte as it was.
//!
//! - [`cellref`]: A1 references. [`model`]: sheets, rows, cells. [`numfmt`]: format codes and serial
//!   dates. [`styles`]: `styles.xml` and colours.
//! - Reading streams the worksheet XML (no DOM) into per-row `BTreeMap`s; writing patches only the
//!   rows an edit touches through [`crate::ground::opc`], so unknown XML survives.
//! - Formulas: the text is kept (shared formulas expanded per cell for the formula bar); after
//!   every change [`calc::recalc`] works the values out and the results are written back.
//!
//! M2 "write": Ledger is edited by hand and by chat. [`editing`] is the caret and the selection
//! (a place is a cell), [`entry`] reads what is typed, [`ops`] holds the commands (cells, formats,
//! structure, clipboard, sort, filter, find, freeze), [`book`] the sheets and CSV, [`refs`] rewrites
//! references when rows, columns, cells or sheets move, [`stylesedit`] appends formats to
//! `styles.xml`, and [`commit`] turns each change into its undo.

mod book;
pub mod calc;
pub mod cellref;
mod commit;
pub mod csv;
mod edit;
pub mod editing;
pub mod entry;
mod formula;
mod grid;
pub mod model;
pub mod numfmt;
mod ops;
mod read;
pub mod refs;
mod structure;
pub mod styles;
mod stylesedit;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_edit;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub use cellref::{cell_name, col_index, col_name, parse_cell, CellRange, MAX_COLS, MAX_ROWS};
pub use formula::shift_formula;
pub use grid::{row_header_width, HEADER_H};
pub use model::{Cell, CellValue, ColInfo, DefinedName, Pane, Row, Sheet, SheetKind, SheetState, View};
pub use numfmt::{builtin_format, format_value, format_value_full, general, is_date_format, serial_to_ymd, Formatted, NumberFormat};
pub use styles::Styles;

use self::edit::RowSnap;
use self::editing::Place;
use self::read::{parse_shared_strings, parse_workbook, parse_worksheet, ReadCtx};
use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command, CommandSpec, Document};
use crate::ground::display::{DisplayList, Resources};
use crate::ground::edit::Editing;
use crate::ground::draw::Theme;
use crate::ground::font::FontIndex;
use crate::ground::opc::{self, Package};
use crate::ground::text::FontCache;
use crate::ground::zip::{Method, ZipWriter};

/// The most cells one `read` hands back.
pub const READ_LIMIT: u64 = 20_000;

/// A font cache over this machine's fonts; the system index is scanned once per process.
pub fn system_fonts() -> FontCache {
    static IDX: OnceLock<Arc<FontIndex>> = OnceLock::new();
    FontCache::new(IDX.get_or_init(|| Arc::new(FontIndex::system())).clone())
}

/// An open workbook.
pub struct LedgerDoc {
    pkg: Package,
    workbook_part: String,
    sheets: Vec<Sheet>,
    sst: Vec<String>,
    styles: Styles,
    theme: Theme,
    date1904: bool,
    names: Vec<DefinedName>,
    active: usize,
    fonts: Mutex<FontCache>,
    resources: Resources,
    /// This workbook's identity for its own clipboard (a paste from it keeps formats).
    id: u64,
    /// The cells a cut marked (moved on the next paste).
    cut: Option<(usize, CellRange)>,
    /// The formats Format Painter took (painted on its next press).
    painter: Option<(usize, CellRange)>,
    sst_part: Option<String>,
    styles_part: Option<String>,
    /// Plain shared strings -> index, built on the first write of a string.
    sst_plain: Option<HashMap<String, usize>>,
    full_calc_marked: bool,
}

/// What reading a package gives.
struct Loaded {
    workbook_part: String,
    sheets: Vec<Sheet>,
    sst: Vec<String>,
    styles: Styles,
    theme: Theme,
    date1904: bool,
    names: Vec<DefinedName>,
    active: usize,
    sst_part: Option<String>,
    styles_part: Option<String>,
}

fn load(pkg: &Package) -> Result<Loaded, String> {
    let wb = pkg.main_part().ok_or("not a workbook: the package names no main part")?;
    if let Some(ct) = pkg.content_type(&wb) {
        if !ct.contains("spreadsheetml") && !ct.contains("ms-excel") {
            return Err(format!("not a workbook: the main part is {ct}"));
        }
    }
    let wb_bytes = pkg.part_bytes(&wb).map_err(|e| e.0)?;
    let info = parse_workbook(&wb_bytes)?;
    let first = |rel: &str| pkg.related(&wb, rel).into_iter().find(|p| pkg.part(p).is_some());
    let theme = first(opc::REL_THEME)
        .and_then(|p| pkg.part_bytes(&p).ok())
        .and_then(|b| Theme::parse(&b).ok())
        .unwrap_or_else(Theme::office_default);
    let styles_part = first(opc::REL_STYLES);
    let styles = match styles_part.as_ref().and_then(|p| pkg.part_bytes(p).ok()) {
        Some(b) => Styles::parse(&b, &theme).unwrap_or_default(),
        None => Styles::default(),
    };
    let sst_part = first(opc::REL_SHARED_STRINGS);
    let sst = match sst_part.as_ref().and_then(|p| pkg.part_bytes(p).ok()) {
        Some(b) => parse_shared_strings(&b)?,
        None => Vec::new(),
    };
    let mut sheets = Vec::new();
    for (name, id, rid, state) in &info.sheets {
        let mut sheet = Sheet::new(name);
        sheet.sheet_id = *id;
        sheet.rel_id = rid.clone();
        sheet.state = *state;
        sheet.kind = SheetKind::Other;
        if let Some(rel) = pkg.rel_by_id(&wb, rid).filter(|r| !r.target_mode_external) {
            let part = pkg.resolve(&wb, rel);
            if opc::rel_type_matches(&rel.rel_type, opc::REL_WORKSHEET) {
                sheet.kind = SheetKind::Worksheet;
                if let Ok(bytes) = pkg.part_bytes(&part) {
                    let ctx = ReadCtx { sst: &sst, date1904: info.date1904 };
                    parse_worksheet(&bytes, &ctx, &mut sheet, true)?;
                    sheet.part = Some(part);
                }
            } else if rel.rel_type.ends_with("/chartsheet") {
                sheet.kind = SheetKind::Chartsheet;
            }
        }
        sheets.push(sheet);
    }
    let active = info.active_tab.min(sheets.len().saturating_sub(1));
    Ok(Loaded { workbook_part: wb, sheets, sst, styles, theme, date1904: info.date1904, names: info.names, active, sst_part, styles_part })
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl std::fmt::Debug for LedgerDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LedgerDoc").field("workbook", &self.workbook_part).field("sheets", &self.sheets.len()).finish()
    }
}

fn bad(cmd: &str, msg: impl Into<String>) -> BusError {
    BusError::bad_args(cmd, msg)
}

impl LedgerDoc {
    /// Opens an `.xlsx` (or `.xlsm`, `.xltx`) with this machine's fonts.
    pub fn open(bytes: &[u8]) -> Result<LedgerDoc, String> {
        LedgerDoc::open_with_fonts(bytes, system_fonts())
    }

    /// Opens with a given font cache (an empty one draws no glyphs, for tests and headless use).
    pub fn open_with_fonts(bytes: &[u8], fonts: FontCache) -> Result<LedgerDoc, String> {
        let pkg = Package::open(bytes).map_err(|e| format!("not an Office file: {}", e.0))?;
        let l = load(&pkg)?;
        let mut doc = LedgerDoc {
            pkg,
            workbook_part: l.workbook_part,
            sheets: l.sheets,
            sst: l.sst,
            styles: l.styles,
            theme: l.theme,
            date1904: l.date1904,
            names: l.names,
            active: l.active,
            fonts: Mutex::new(fonts),
            resources: Resources::new(),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            cut: None,
            painter: None,
            sst_part: l.sst_part,
            styles_part: l.styles_part,
            sst_plain: None,
            full_calc_marked: false,
        };
        for i in 0..doc.sheets.len() {
            doc.refresh(i);
        }
        Ok(doc)
    }

    /// Reads the model again from the package (after a change to whole parts); each sheet keeps
    /// its view, the active sheet stays where it was.
    fn reload(&mut self) -> Result<(), String> {
        let l = load(&self.pkg)?;
        let active_name = self.sheets.get(self.active).map(|s| s.name.clone());
        let mut sheets = l.sheets;
        for s in sheets.iter_mut() {
            if let Some(old) = self.sheets.iter().find(|o| o.name == s.name || (o.part.is_some() && o.part == s.part)) {
                let (fr, fc) = s.frozen();
                s.view = old.view;
                s.view.row = s.view.row.max(fr);
                s.view.col = s.view.col.max(fc);
            }
        }
        self.active = active_name.and_then(|n| sheets.iter().position(|s| s.name == n)).unwrap_or(self.active).min(sheets.len().saturating_sub(1));
        self.sheets = sheets;
        self.workbook_part = l.workbook_part;
        self.sst = l.sst;
        self.styles = l.styles;
        self.theme = l.theme;
        self.date1904 = l.date1904;
        self.names = l.names;
        self.sst_part = l.sst_part;
        self.styles_part = l.styles_part;
        self.sst_plain = None;
        if let Some((s, _)) = self.cut {
            if s >= self.sheets.len() {
                self.cut = None;
            }
        }
        if let Some((s, _)) = self.painter {
            if s >= self.sheets.len() {
                self.painter = None;
            }
        }
        for i in 0..self.sheets.len() {
            self.refresh(i);
        }
        Ok(())
    }

    /// A shared string's index (appended to the table when new; the table part is edited once
    /// per call). Only plain entries are reused, so a rich string never gives its runs away.
    fn intern(&mut self, texts: &[String]) -> Vec<usize> {
        let Some(part) = self.sst_part.clone() else { return Vec::new() };
        if self.sst_plain.is_none() {
            let mut map = HashMap::new();
            if let Ok(bytes) = self.pkg.part_bytes(&part) {
                if let Ok(root) = crate::ground::xml::parse_dom(&bytes) {
                    for (i, si) in root.children_named("si").enumerate() {
                        let kids: Vec<_> = si.elements().collect();
                        if kids.len() == 1 && kids[0].local() == "t" {
                            if let Some(t) = self.sst.get(i) {
                                map.entry(t.clone()).or_insert(i);
                            }
                        }
                    }
                }
            }
            self.sst_plain = Some(map);
        }
        let mut out = Vec::with_capacity(texts.len());
        let mut added: Vec<String> = Vec::new();
        for t in texts {
            let map = self.sst_plain.as_mut().expect("built");
            match map.get(t) {
                Some(&i) => out.push(i),
                None => {
                    let i = self.sst.len();
                    self.sst.push(t.clone());
                    map.insert(t.clone(), i);
                    added.push(t.clone());
                    out.push(i);
                }
            }
        }
        if !added.is_empty() {
            let total = self.sst.len();
            let _ = self.pkg.edit_xml(&part, |root| {
                let prefix = root.prefix().map(str::to_string);
                for t in &added {
                    let mut si = crate::ground::xml::Element::new(edit::qualify(&prefix, "si"));
                    let mut te = crate::ground::xml::Element::new(edit::qualify(&prefix, "t"));
                    if t.starts_with(char::is_whitespace) || t.ends_with(char::is_whitespace) || t.contains('\n') {
                        te.attrs.push(("xml:space".into(), "preserve".into()));
                    }
                    te.children.push(crate::ground::xml::Node::Text(edit::encode_escapes(t)));
                    si.children.push(crate::ground::xml::Node::Element(te));
                    root.children.push(crate::ground::xml::Node::Element(si));
                }
                if root.attr("uniqueCount").is_some() || root.attr("count").is_some() {
                    root.set_attr("uniqueCount", total.to_string());
                    let count = root.attr("count").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0) + added.len();
                    root.set_attr("count", count.max(total).to_string());
                }
            });
        }
        out
    }

    /// Gives the text values of `writes` their shared-string index (when the workbook keeps a
    /// string table); text a cell already holds stays as it is.
    fn intern_writes(&mut self, si: usize, writes: &mut [edit::CellWrite]) {
        if self.sst_part.is_none() {
            return;
        }
        let mut want: Vec<(usize, String)> = Vec::new();
        for (k, w) in writes.iter().enumerate() {
            let Some(spec) = &w.spec else { continue };
            if spec.formula.is_some() || spec.sst.is_some() {
                continue;
            }
            let CellValue::Text(t) = &spec.value else { continue };
            let same = self.sheets[si].cell(w.row, w.col).is_some_and(|c| c.formula.is_none() && c.value == spec.value);
            if !same {
                want.push((k, t.clone()));
            }
        }
        if want.is_empty() {
            return;
        }
        let texts: Vec<String> = want.iter().map(|w| w.1.clone()).collect();
        let ids = self.intern(&texts);
        for ((k, _), id) in want.into_iter().zip(ids) {
            if let Some(spec) = writes[k].spec.as_mut() {
                spec.sst = Some(id);
            }
        }
    }

    /// A new workbook with one empty sheet, `Sheet1`.
    pub fn new_blank() -> LedgerDoc {
        LedgerDoc::open(&blank_xlsx()).expect("the blank workbook opens")
    }

    /// [`LedgerDoc::new_blank`] with a given font cache.
    pub fn new_blank_with_fonts(fonts: FontCache) -> LedgerDoc {
        LedgerDoc::open_with_fonts(&blank_xlsx(), fonts).expect("the blank workbook opens")
    }

    /// The file's bytes: the input unchanged when nothing was edited; otherwise every part the
    /// edits did not touch is copied byte for byte.
    pub fn save(&self) -> Vec<u8> {
        self.pkg.save()
    }

    pub fn package(&self) -> &Package {
        &self.pkg
    }

    pub fn sheets(&self) -> &[Sheet] {
        &self.sheets
    }

    pub fn sheet(&self, i: usize) -> Option<&Sheet> {
        self.sheets.get(i)
    }

    pub fn sheet_by_name(&self, name: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.name.eq_ignore_ascii_case(name.trim()))
    }

    pub fn styles(&self) -> &Styles {
        &self.styles
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn date1904(&self) -> bool {
        self.date1904
    }

    pub fn defined_names(&self) -> &[DefinedName] {
        &self.names
    }

    pub fn shared_strings(&self) -> &[String] {
        &self.sst
    }

    /// The tab that was active when the file was saved.
    pub fn active_sheet(&self) -> usize {
        self.active
    }

    /// A cell's text as the grid shows it (number format applied; `*` fill left out).
    pub fn formatted(&self, cell: &Cell) -> String {
        let code = self.styles.format_code(self.styles.xf(cell.style).num_fmt);
        NumberFormat::parse(code).format(&cell.value, self.date1904).text
    }

    /// The formatted text of a cell by position ("" when empty).
    pub fn cell_text(&self, sheet: usize, row: u32, col: u32) -> String {
        self.sheets.get(sheet).and_then(|s| s.cell(row, col)).map(|c| self.formatted(c)).unwrap_or_default()
    }

    /// Re-renders a sheet's view and keeps the outlines of the glyphs it uses.
    fn refresh(&mut self, i: usize) {
        let dl = self.display(i);
        let fonts = self.fonts.lock().unwrap_or_else(|e| e.into_inner());
        fonts.fill_resources(&mut self.resources, &dl.items);
    }

    fn sheet_index(&self, cmd: &str, v: Option<&Json>) -> Result<usize, BusError> {
        let n = self.sheets.len();
        if n == 0 {
            return Err(BusError::failed("the workbook has no sheets"));
        }
        match v {
            None | Some(Json::Null) => Ok(self.active.min(n - 1)),
            Some(Json::Str(s)) => self.sheet_by_name(s).ok_or_else(|| {
                BusError::failed(format!("no sheet named '{}'; the sheets are: {}", s, self.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))
            }),
            Some(j) => match j.as_usize() {
                Some(i) if i < n => Ok(i),
                _ => Err(bad(cmd, format!("\"sheet\" is a sheet name or an index from 0 to {}", n - 1))),
            },
        }
    }

    /// Reads rows written as XML back into the model (rows without XML are removed).
    fn reparse_rows(&mut self, si: usize, rows: &[RowSnap]) -> Result<(), String> {
        let sheet = &mut self.sheets[si];
        for s in rows {
            sheet.rows.remove(&s.row);
        }
        let xml: String = rows.iter().filter_map(|s| s.xml.clone()).collect();
        if xml.is_empty() {
            return Ok(());
        }
        let mut frag = String::from("<wrap");
        for (k, v) in &sheet.ns_decls {
            frag.push_str(&format!(" {}=\"{}\"", k, crate::ground::xml::escape_attr(v)));
        }
        frag.push_str("><sheetData>");
        frag.push_str(&xml);
        frag.push_str("</sheetData></wrap>");
        let ctx = ReadCtx { sst: &self.sst, date1904: self.date1904 };
        parse_worksheet(frag.as_bytes(), &ctx, sheet, false)
    }

    fn worksheet_part(&self, si: usize) -> Result<String, BusError> {
        self.sheets[si].part.clone().ok_or_else(|| BusError::failed(format!("'{}' is not a worksheet", self.sheets[si].name)))
    }

    fn scroll_to(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let cell = cmd.args.get("cell").and_then(Json::as_str).unwrap_or("A1");
        let (r, c) = parse_cell(cell).ok_or_else(|| bad(name, format!("'{cell}' is not a cell reference")))?;
        let size = |k: &str| -> Result<Option<f64>, BusError> {
            match cmd.args.get(k) {
                None | Some(Json::Null) => Ok(None),
                Some(j) => match j.as_f64() {
                    Some(v) if v > 0.0 && v <= 100_000.0 => Ok(Some(v)),
                    _ => Err(bad(name, format!("\"{k}\" is a size in points (above 0, at most 100000)"))),
                },
            }
        };
        let (w, h) = (size("width")?, size("height")?);
        let sheet = &mut self.sheets[si];
        let (fr, fc) = sheet.frozen();
        sheet.view.row = r.max(fr);
        sheet.view.col = c.max(fc);
        if let Some(w) = w {
            sheet.view.width = w;
        }
        if let Some(h) = h {
            sheet.view.height = h;
        }
        self.refresh(si);
        Ok(Change { undo: Vec::new(), pages_touched: vec![si] })
    }

    fn cell_json(&self, sheet: &Sheet, r: u32, c: u32) -> Json {
        match sheet.cell(r, c) {
            Some(cell) => {
                let mut o = Json::obj().with("v", cell.value.to_json());
                if let Some(f) = &cell.formula {
                    o.set("f", f.as_str());
                }
                o.set("t", self.formatted(cell));
                o
            }
            None => Json::obj().with("v", Json::Null).with("t", ""),
        }
    }
}

impl LedgerDoc {
    /// Applies one command (the bus's `apply`).
    fn apply_inner(&mut self, cmd: &Command) -> Result<Change, BusError> {
        match cmd.name.as_str() {
            "ledger.set_cell" => self.cmd_set_cell(cmd),
            "ledger.scroll_to" => self.scroll_to(cmd),
            "ledger.restore_rows" => self.restore(cmd),
            other => match self.apply_edit(cmd) {
                Some(r) => r,
                None => Err(BusError::unknown(other)),
            },
        }
    }

    /// The bytes of one sheet as CSV (`read {"csv": true}` answers the text).
    fn read_csv(&self, what: &Json) -> Result<Json, BusError> {
        let si = self.sheet_index("read", what.get("sheet"))?;
        let rows = self.sheet_rows(si);
        let text = String::from_utf8(csv::write(&rows, ',', false)).unwrap_or_default();
        Ok(Json::obj().with("sheet", self.sheets[si].name.as_str()).with("csv", text))
    }
}

impl Document for LedgerDoc {
    fn kind(&self) -> &'static str {
        "ledger"
    }

    fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
        self.apply_inner(cmd)
    }

    fn editing(&self) -> Option<&dyn Editing> {
        Some(self)
    }

    fn describe(&self) -> Json {
        let sheets: Vec<Json> = self
            .sheets
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let used = s.used_range().map(|r| r.to_string());
                let mut o = Json::obj()
                    .with("index", i)
                    .with("name", s.name.as_str())
                    .with("state", s.state.as_str())
                    .with(
                        "kind",
                        match s.kind {
                            SheetKind::Worksheet => "worksheet",
                            SheetKind::Chartsheet => "chartsheet",
                            SheetKind::Other => "other",
                        },
                    )
                    .with("used_range", used)
                    .with("dimension", s.dimension.clone());
                let (fr, fc) = s.frozen();
                o.set("frozen", if fr > 0 || fc > 0 { Json::obj().with("rows", fr).with("cols", fc) } else { Json::Null });
                o.set("merged_ranges", s.merges.len());
                o.set("auto_filter", s.auto_filter.map(|r| r.to_string()));
                o.set("active_cell", s.active_cell.map(|(r, c)| cell_name(r, c)));
                o.set(
                    "view",
                    Json::obj().with("top_left", cell_name(s.view.row, s.view.col)).with("width", s.view.width).with("height", s.view.height),
                );
                o
            })
            .collect();
        let names: Vec<Json> = self
            .names
            .iter()
            .map(|n| {
                Json::obj()
                    .with("name", n.name.as_str())
                    .with("value", n.value.as_str())
                    .with("scope", n.local_sheet.and_then(|i| self.sheets.get(i)).map(|s| s.name.clone()))
                    .with("hidden", n.hidden)
            })
            .collect();
        Json::obj()
            .with("sheets", Json::Arr(sheets))
            .with("active_sheet", self.active)
            .with("defined_names", Json::Arr(names))
            .with("date1904", self.date1904)
    }

    fn read(&self, what: &Json) -> Result<Json, BusError> {
        if what.get("csv").and_then(Json::as_bool) == Some(true) {
            return self.read_csv(what);
        }
        let si = self.sheet_index("read", what.get("sheet"))?;
        let sheet = &self.sheets[si];
        let text = what.get("range").or_else(|| what.get("cell")).and_then(Json::as_str);
        let mut range = match text {
            Some(t) => CellRange::parse(t).ok_or_else(|| bad("read", format!("'{t}' is not a range like \"A1:D10\"")))?,
            None => sheet.used_range().unwrap_or(CellRange::cell(0, 0)),
        };
        if range.rows() * range.cols() > READ_LIMIT {
            // A whole column or row: only as far as the cells go.
            if let Some(u) = sheet.used_range() {
                range = CellRange::new(range.r1, range.c1, range.r2.min(u.r2.max(range.r1)), range.c2.min(u.c2.max(range.c1)));
            }
        }
        if range.rows() * range.cols() > READ_LIMIT {
            return Err(BusError::failed(format!(
                "{} is {} cells; read at most {} at once",
                range,
                range.rows() * range.cols(),
                READ_LIMIT
            )));
        }
        let mut out = Vec::with_capacity(range.rows() as usize);
        for r in range.r1..=range.r2 {
            let mut line = Vec::with_capacity(range.cols() as usize);
            for c in range.c1..=range.c2 {
                line.push(self.cell_json(sheet, r, c));
            }
            out.push(Json::Arr(line));
        }
        Ok(Json::Arr(out))
    }

    fn display(&self, page: usize) -> DisplayList {
        match self.sheets.get(page) {
            Some(sheet) => {
                let mut fonts = self.fonts.lock().unwrap_or_else(|e| e.into_inner());
                grid::render(sheet, &self.styles, self.date1904, &mut fonts)
            }
            None => DisplayList::new(View::default().width, View::default().height),
        }
    }

    fn page_count(&self) -> usize {
        self.sheets.len()
    }

    fn commands(&self) -> Vec<CommandSpec> {
        command_specs()
    }

    fn resources(&self) -> Option<&Resources> {
        Some(&self.resources)
    }
}

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

/// The default `styles.xml` of a new workbook.
pub fn blank_styles() -> String {
    format!(
        "{DECL}<styleSheet xmlns=\"{NS_MAIN}\"><fonts count=\"1\"><font><sz val=\"11\"/><color theme=\"1\"/><name val=\"Calibri\"/>\
<family val=\"2\"/><scheme val=\"minor\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill>\
<fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\
<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\
<cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs>\
<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>"
    )
}

/// The bytes of a blank workbook: one empty sheet, the default styles.
pub fn blank_xlsx() -> Vec<u8> {
    let content_types = format!(
        "{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"{}\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"{}\"/>\
<Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"{}\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"{}\"/></Types>",
        opc::CT_RELATIONSHIPS,
        opc::CT_SPREADSHEETML_MAIN,
        opc::CT_WORKSHEET,
        opc::CT_SPREADSHEETML_STYLES
    );
    let root_rels = format!(
        "{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"{}\" Target=\"xl/workbook.xml\"/></Relationships>",
        opc::REL_OFFICE_DOCUMENT
    );
    let workbook = format!(
        "{DECL}<workbook xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\"><bookViews><workbookView/></bookViews>\
<sheets><sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>"
    );
    let wb_rels = format!(
        "{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"{}\" Target=\"worksheets/sheet1.xml\"/>\
<Relationship Id=\"rId2\" Type=\"{}\" Target=\"styles.xml\"/></Relationships>",
        opc::REL_WORKSHEET,
        opc::REL_STYLES
    );
    let sheet = format!(
        "{DECL}<worksheet xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_R}\"><dimension ref=\"A1\"/>\
<sheetViews><sheetView tabSelected=\"1\" workbookViewId=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/>\
<sheetData/><pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>"
    );
    let styles = blank_styles();
    let mut z = ZipWriter::new();
    let parts: [(&str, &str); 6] = [
        ("[Content_Types].xml", &content_types),
        ("_rels/.rels", &root_rels),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", &wb_rels),
        ("xl/worksheets/sheet1.xml", &sheet),
        ("xl/styles.xml", &styles),
    ];
    for (name, body) in parts {
        z.add(name, body.as_bytes(), Method::Deflated(6)).expect("zip entry");
    }
    z.finish()
}

/// Every command Ledger takes (the AI's door lists these). Commands that act on cells take the
/// page's selection as `"sel"`, or a `"range"`/`"cell"` (and `"sheet"`) from the companion.
pub fn command_specs() -> Vec<CommandSpec> {
    const T: &str = r#""sel"?: selection | "sheet"?: name|index, "range"?: "A1:C9""#;
    let s = |name: &str, summary: &str, args: String| CommandSpec::new(name, summary, args);
    vec![
        s("ledger.set_cell", "Set one cell: a raw \"value\" (number, text, boolean, null clears; the cell keeps its format), or an \"input\" read as if typed (\"=A1*2\" is a formula, \"10%\" a percentage, \"2026-11-26\" a date)", r#"{ "sheet"?: name|index, "cell": "B2", "value"?: number|string|boolean|null, "input"?: string }"#.into()),
        s("ledger.set_cells", "Fill a block from its top-left cell with rows of values; strings are read as typed (formulas, dates, percentages), numbers and booleans as they are", r#"{ "sheet"?: name|index, "cell": "A1", "values": [[..], [..]] }"#.into()),
        s("ledger.scroll_to", "Show a sheet from this cell at the top-left (optionally with a new viewport size in points)", r#"{ "sheet"?: name|index, "cell": "A1", "width"?: points, "height"?: points }"#.into()),
        s("ledger.move", "Move the active cell (arrows; word_left/right go to the edge of the data, line_start/end along the row, doc_start/end to A1 and the last cell); changes nothing", r#"{ "sel", "motion": "left"|"right"|"up"|"down"|"word_left"|"word_right"|"line_start"|"line_end"|"doc_start"|"doc_end", "extend"?: bool }"#.into()),
        s("ledger.enter", "Enter: move down (up with shift); with ctrl, put the active cell's content into every selected cell", r#"{ "sel", "shift"?: bool, "ctrl"?: bool }"#.into()),
        s("ledger.tab", "Tab: move right (left with back)", r#"{ "sel", "back"?: bool }"#.into()),
        s("ledger.select_all", "Select the data region, then the whole sheet", r#"{ "sel" }"#.into()),
        s("ledger.insert_text", "Type into the active cell (the typed text so far is read as an entry)", r#"{ "sel", "text": string }"#.into()),
        s("ledger.backspace", "Take the last typed character back (or empty the cell and start typing)", r#"{ "sel" }"#.into()),
        s("ledger.clear", "Clear the cells' contents, formats or both", format!("{{ {T}, \"what\"?: \"contents\"|\"formats\"|\"all\" }}")),
        s("ledger.fill", "Fill down, right, up or left: the edge row or column copied across the range (formulas move as copies)", format!("{{ {T}, \"direction\": \"down\"|\"right\"|\"up\"|\"left\" }}")),
        s("ledger.autofill", "AutoFill the source range out to \"to\": number and date series continue, \"Item 1\" counts on, formulas are copied; \"series\": true makes a single number count up", format!("{{ {T}, \"to\": \"A1:A20\", \"series\"?: bool }}")),
        s("ledger.cut", "Mark the cells to move on the next paste", format!("{{ {T} }}")),
        s("ledger.paste", "Paste at the selection: our own copy (\"rich\") keeps formats and moves formulas; text from outside is tab-separated and read as typed; \"mode\" pastes values, formats or formulas only", format!("{{ {T}, \"text\"?: string, \"rich\"?: object, \"mode\"?: \"all\"|\"values\"|\"formats\"|\"formulas\" }}")),
        s("ledger.move_range", "Move cells to a new top-left; formulas that point at them follow", r#"{ "sheet"?: name|index, "from": "A1:B5", "to": "D1" }"#.into()),
        s("ledger.autosum", "AutoSum: a SUM (or AVERAGE, COUNT, MAX, MIN) of the numbers above or left of the active cell, or below each column of a selected block", format!("{{ {T}, \"fn\"?: \"SUM\"|\"AVERAGE\"|\"COUNT\"|\"MAX\"|\"MIN\" }}")),
        s("ledger.format", "Format cells: font family and size (size_delta grows or shrinks a step), bold, italic, underline, strike, font colour, fill colour (null: none), horizontal and vertical alignment, wrap, indent, number format (general, number, currency, accounting, date, long_date, time, percent, fraction, scientific, text, comma, or a format code) and decimals (+1 / -1)", format!("{{ {T}, \"family\"?: string, \"size\"?: points, \"size_delta\"?: 1|-1, \"bold\"?: bool, \"italic\"?: bool, \"underline\"?: bool, \"strike\"?: bool, \"color\"?: \"#RRGGBB\"|null, \"fill\"?: \"#RRGGBB\"|null, \"h_align\"?: \"general\"|\"left\"|\"center\"|\"right\"|\"justify\", \"v_align\"?: \"top\"|\"middle\"|\"bottom\", \"wrap\"?: bool, \"indent\"?: n, \"number_format\"?: name|code, \"decimals\"?: 1|-1 }}")),
        s("ledger.toggle", "Switch bold, italic, underline, strike or wrap on or off (by the active cell)", format!("{{ {T}, \"what\": \"bold\"|\"italic\"|\"underline\"|\"strike\"|\"wrap\" }}")),
        s("ledger.borders", "Borders: all, outside, thick_box, inside, top, bottom, left, right, top_bottom, bottom_double or none", format!("{{ {T}, \"preset\": string, \"style\"?: \"thin\"|\"medium\"|\"thick\"|\"double\"|\"dashed\"|\"dotted\", \"color\"?: \"#RRGGBB\" }}")),
        s("ledger.format_painter", "Format Painter: the first press takes the selection's formats, the next press paints them onto the selection", format!("{{ {T} }}")),
        s("ledger.copy_format", "Copy the formats of \"from\" onto the cells (tiled)", format!("{{ {T}, \"from\": \"A1:B2\", \"from_sheet\"?: name|index }}")),
        s("ledger.merge", "Merge the cells (center: merge and centre; across: one merge per row); only the top-left value stays", format!("{{ {T}, \"mode\"?: \"center\"|\"across\"|\"merge\" }}")),
        s("ledger.unmerge", "Unmerge every merged area in the range", format!("{{ {T} }}")),
        s("ledger.insert", "Insert rows or columns (as many as selected, or \"count\"), or cells shifting down or right; every formula, merge, name and conditional format follows", format!("{{ {T}, \"what\": \"rows\"|\"cols\"|\"cells_down\"|\"cells_right\", \"count\"?: n }}")),
        s("ledger.delete", "Delete rows or columns, or cells shifting up or left; references into deleted cells become #REF!", format!("{{ {T}, \"what\": \"rows\"|\"cols\"|\"cells_up\"|\"cells_left\", \"count\"?: n }}")),
        s("ledger.row_height", "Set the rows' height in points (null: default), or fit it to the contents (auto)", format!("{{ {T}, \"height\"?: points|null, \"auto\"?: bool }}")),
        s("ledger.col_width", "Set the columns' width in characters (null: default), or fit it to the contents (auto)", format!("{{ {T}, \"width\"?: chars|null, \"auto\"?: bool }}")),
        s("ledger.hide", "Hide or unhide rows or columns", format!("{{ {T}, \"what\": \"rows\"|\"cols\", \"hidden\": bool }}")),
        s("ledger.sort", "Sort the selected block (or the data around the active cell) by a column, ascending or descending; a header row is found or told", format!("{{ {T}, \"by\"?: \"B\", \"descending\"?: bool, \"header\"?: bool }}")),
        s("ledger.filter", "AutoFilter: set it on the data (the button again takes it off), or show only the rows whose column holds one of \"values\"", format!("{{ {T}, \"column\"?: \"B\", \"values\"?: [string] }}")),
        s("ledger.clear_filter", "Clear the AutoFilter's criteria and show every row", r#"{ "sel"? | "sheet"? }"#.into()),
        s("ledger.remove_filter", "Take the AutoFilter off", r#"{ "sel"? | "sheet"? }"#.into()),
        s("ledger.find", "Find text (in formulas, or in shown values with \"in\": \"values\"); the selection goes to the next (or previous) match", r#"{ "sel", "query": string, "next": bool, "in"?: "formulas"|"values", "match_case"?: bool, "whole"?: bool }"#.into()),
        s("ledger.replace", "Replace in the active cell (then go to the next match) or everywhere on the sheet (\"all\"; \"workbook\": every sheet)", r#"{ "sel", "query": string, "with": string, "all": bool, "workbook"?: bool, "in"?: "formulas"|"values", "match_case"?: bool, "whole"?: bool }"#.into()),
        s("ledger.freeze", "Freeze panes: the rows above and the columns left of the active cell, or \"rows\" and \"cols\" (0, 0 unfreezes)", format!("{{ {T}, \"rows\"?: n, \"cols\"?: n }}")),
        s("ledger.unfreeze", "Unfreeze the panes", r#"{ "sel"? | "sheet"? }"#.into()),
        s("ledger.add_sheet", "Add a sheet (named, at a place)", r#"{ "name"?: string, "at"?: index }"#.into()),
        s("ledger.rename_sheet", "Rename a sheet; formulas that name it follow", r#"{ "sheet": name|index, "name": string }"#.into()),
        s("ledger.delete_sheet", "Delete a sheet; formulas that name it become #REF!", r#"{ "sheet": name|index }"#.into()),
        s("ledger.move_sheet", "Move a sheet to another place among the tabs", r#"{ "sheet": name|index, "to": index }"#.into()),
        s("ledger.import_csv", "Put CSV text into a sheet from a cell (the separator is found; leading zeros and long codes stay text)", r#"{ "text": string, "sheet"?: name|index, "cell"?: "A1", "separator"?: "," | ";" | "tab" }"#.into()),
    ]
}

/// A place in a workbook as the editing ground's pos.
pub fn pos(sheet: usize, cell: &str) -> Option<crate::ground::edit::Pos> {
    let (r, c) = parse_cell(cell)?;
    Some(Place::new(sheet, r, c).pos())
}
