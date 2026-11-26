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
//! - Formulas: the text is kept (shared formulas expanded per cell for the formula bar); nothing is
//!   calculated yet: the cached values are shown.

pub mod cellref;
mod edit;
mod formula;
mod grid;
pub mod model;
pub mod numfmt;
mod read;
pub mod styles;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex, OnceLock};

pub use cellref::{cell_name, col_index, col_name, parse_cell, CellRange, MAX_COLS, MAX_ROWS};
pub use formula::shift_formula;
pub use grid::{row_header_width, HEADER_H};
pub use model::{Cell, CellValue, ColInfo, DefinedName, Pane, Row, Sheet, SheetKind, SheetState, View};
pub use numfmt::{builtin_format, format_value, format_value_full, general, is_date_format, serial_to_ymd, Formatted, NumberFormat};
pub use styles::Styles;

use self::edit::RowSnap;
use self::read::{parse_shared_strings, parse_workbook, parse_worksheet, ReadCtx};
use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command, CommandSpec, Document};
use crate::ground::display::{DisplayList, Resources};
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
}

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
        let styles = match first(opc::REL_STYLES).and_then(|p| pkg.part_bytes(&p).ok()) {
            Some(b) => Styles::parse(&b, &theme).unwrap_or_default(),
            None => Styles::default(),
        };
        let sst = match first(opc::REL_SHARED_STRINGS).and_then(|p| pkg.part_bytes(&p).ok()) {
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
        let mut doc = LedgerDoc {
            pkg,
            workbook_part: wb,
            sheets,
            sst,
            styles,
            theme,
            date1904: info.date1904,
            names: info.names,
            active,
            fonts: Mutex::new(fonts),
            resources: Resources::new(),
        };
        for i in 0..doc.sheets.len() {
            doc.refresh(i);
        }
        Ok(doc)
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

    fn rows_json(rows: &[RowSnap]) -> Json {
        Json::Arr(rows.iter().map(|s| Json::obj().with("row", s.row as u64 + 1).with("xml", s.xml.clone())).collect())
    }

    fn set_cell(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let cell = cmd.args.get("cell").and_then(Json::as_str).ok_or_else(|| bad(name, "\"cell\" is an A1 reference like \"B2\""))?;
        let (r, c) = parse_cell(cell).ok_or_else(|| bad(name, format!("'{cell}' is not a cell reference (A1 to XFD1048576)")))?;
        let value = match cmd.args.get("value") {
            None | Some(Json::Null) => CellValue::Empty,
            Some(Json::Num(n)) if n.is_finite() => CellValue::Number(*n),
            Some(Json::Str(s)) if s.is_empty() => CellValue::Empty,
            Some(Json::Str(s)) => CellValue::Text(s.clone()),
            Some(Json::Bool(b)) => CellValue::Bool(*b),
            _ => return Err(bad(name, "\"value\" is a number, a string, a boolean or null")),
        };
        if let CellValue::Text(s) = &value {
            if s.chars().count() > 32_767 {
                return Err(bad(name, "a cell holds at most 32,767 characters"));
            }
        }
        let part = self.worksheet_part(si)?;
        let mut outcome = None;
        self.pkg
            .edit_xml(&part, |root| outcome = Some(edit::set_cell(root, r, c, &value)))
            .map_err(|e| BusError::failed(e.0))?;
        let outcome = outcome.ok_or_else(|| BusError::failed("the worksheet could not be read"))?.map_err(BusError::failed)?;
        self.reparse_rows(si, &outcome.new_rows).map_err(BusError::failed)?;
        if let Some(d) = &outcome.new_dimension {
            self.sheets[si].dimension = Some(d.clone());
        }
        let mut args = Json::obj().with("sheet", si).with("rows", Self::rows_json(&outcome.old_rows));
        if let Some(d) = outcome.old_dimension {
            args.set("dimension", d);
        }
        self.refresh(si);
        Ok(Change { undo: vec![Command::new("ledger.restore_rows", args)], pages_touched: vec![si] })
    }

    fn restore_rows(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let list = cmd.args.get("rows").and_then(Json::as_arr).ok_or_else(|| bad(name, "\"rows\" is a list"))?;
        let mut snaps = Vec::new();
        for item in list {
            let row = item.get("row").and_then(Json::as_usize).filter(|&r| r >= 1 && r <= MAX_ROWS as usize).ok_or_else(|| bad(name, "each row has a \"row\" number"))?;
            let xml = item.get("xml").and_then(Json::as_str).map(str::to_string);
            snaps.push(RowSnap { row: row as u32 - 1, xml });
        }
        let dim = cmd.args.get("dimension").and_then(Json::as_str).map(str::to_string);
        let part = self.worksheet_part(si)?;
        let ns = self.sheets[si].ns_decls.clone();
        let mut outcome = None;
        self.pkg
            .edit_xml(&part, |root| outcome = Some(edit::restore_rows(root, &snaps, dim.as_deref(), &ns)))
            .map_err(|e| BusError::failed(e.0))?;
        let outcome = outcome.ok_or_else(|| BusError::failed("the worksheet could not be read"))?.map_err(BusError::failed)?;
        self.reparse_rows(si, &outcome.new_rows).map_err(BusError::failed)?;
        if let Some(d) = &outcome.new_dimension {
            self.sheets[si].dimension = Some(d.clone());
        }
        let mut args = Json::obj().with("sheet", si).with("rows", Self::rows_json(&outcome.old_rows));
        if let Some(d) = outcome.old_dimension {
            args.set("dimension", d);
        }
        self.refresh(si);
        Ok(Change { undo: vec![Command::new("ledger.restore_rows", args)], pages_touched: vec![si] })
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

impl Document for LedgerDoc {
    fn kind(&self) -> &'static str {
        "ledger"
    }

    fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
        match cmd.name.as_str() {
            "ledger.set_cell" => self.set_cell(cmd),
            "ledger.scroll_to" => self.scroll_to(cmd),
            "ledger.restore_rows" => self.restore_rows(cmd),
            other => Err(BusError::unknown(other)),
        }
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
        vec![
            CommandSpec::new(
                "ledger.set_cell",
                "Set one cell to a number, text, a boolean, or clear it with null (the cell keeps its format)",
                r#"{ "sheet"?: name|index, "cell": "B2", "value": number|string|boolean|null }"#,
            ),
            CommandSpec::new(
                "ledger.scroll_to",
                "Show a sheet from this cell at the top-left (optionally with a new viewport size in points)",
                r#"{ "sheet"?: name|index, "cell": "A1", "width"?: points, "height"?: points }"#,
            ),
        ]
    }

    fn resources(&self) -> Option<&Resources> {
        Some(&self.resources)
    }
}

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

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
    let styles = format!(
        "{DECL}<styleSheet xmlns=\"{NS_MAIN}\"><fonts count=\"1\"><font><sz val=\"11\"/><color theme=\"1\"/><name val=\"Calibri\"/>\
<family val=\"2\"/><scheme val=\"minor\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill>\
<fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\
<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\
<cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs>\
<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>"
    );
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
