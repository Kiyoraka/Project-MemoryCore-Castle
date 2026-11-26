//! The workbook as Ledger holds it: sheets of sparse rows (a `BTreeMap` of rows, each a `BTreeMap` of
//! cells), so an empty cell costs nothing and a sheet of 1,048,576 x 16,384 is as cheap as its
//! filled cells.

use std::collections::{BTreeMap, HashMap};

use super::cellref::{CellRange, MAX_COLS, MAX_ROWS};
use crate::ground::bus::json::Json;

/// What a cell holds (a formula's cached result for formula cells).
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    /// `#DIV/0!`, `#N/A`, ...
    Error(String),
}

impl CellValue {
    pub fn is_empty(&self) -> bool {
        matches!(self, CellValue::Empty)
    }

    /// The raw value for the AI's door: a number, a string, a boolean, the error text, or null.
    pub fn to_json(&self) -> Json {
        match self {
            CellValue::Empty => Json::Null,
            CellValue::Number(n) => Json::Num(*n),
            CellValue::Text(s) => Json::Str(s.clone()),
            CellValue::Bool(b) => Json::Bool(*b),
            CellValue::Error(e) => Json::Str(e.clone()),
        }
    }
}

/// One stored cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub value: CellValue,
    /// The formula text without `=` (a shared formula's child carries its expanded text).
    pub formula: Option<String>,
    /// Index into `cellXfs`.
    pub style: u32,
}

/// One stored row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub cells: BTreeMap<u32, Cell>,
    /// Custom height in points.
    pub height: Option<f64>,
    pub hidden: bool,
    /// The row's own style (`s` with `customFormat`), used by its empty cells.
    pub style: Option<u32>,
}

/// A `<col>` run (0-based, inclusive).
#[derive(Debug, Clone, PartialEq)]
pub struct ColInfo {
    pub min: u32,
    pub max: u32,
    /// Width in Excel's character units, as stored.
    pub width: Option<f64>,
    pub hidden: bool,
    pub style: Option<u32>,
}

/// `<pane>`: a split; frozen panes keep the first `y_split` rows and `x_split` columns in place.
#[derive(Debug, Clone, PartialEq)]
pub struct Pane {
    pub x_split: u32,
    pub y_split: u32,
    pub top_left: Option<(u32, u32)>,
    pub frozen: bool,
}

/// What part of a sheet the page shows: the top-left cell of the scrolling area and the size of
/// the viewport in points (headers included).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub row: u32,
    pub col: u32,
    pub width: f64,
    pub height: f64,
}

impl Default for View {
    fn default() -> View {
        View { row: 0, col: 0, width: 1200.0, height: 800.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetState {
    Visible,
    Hidden,
    VeryHidden,
}

impl SheetState {
    pub fn parse(s: Option<&str>) -> SheetState {
        match s {
            Some("hidden") => SheetState::Hidden,
            Some("veryHidden") => SheetState::VeryHidden,
            _ => SheetState::Visible,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SheetState::Visible => "visible",
            SheetState::Hidden => "hidden",
            SheetState::VeryHidden => "veryHidden",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetKind {
    Worksheet,
    Chartsheet,
    Other,
}

/// The master of a shared formula: where it sits and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct SharedFormula {
    pub row: u32,
    pub col: u32,
    pub text: String,
}

/// One sheet.
#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: String,
    pub sheet_id: u32,
    pub rel_id: String,
    pub state: SheetState,
    pub kind: SheetKind,
    /// The worksheet part (`/xl/worksheets/sheet1.xml`), when there is one.
    pub part: Option<String>,
    pub rows: BTreeMap<u32, Row>,
    pub cols: Vec<ColInfo>,
    pub default_row_height: f64,
    /// Default column width in points.
    pub default_col_width: f64,
    pub merges: Vec<CellRange>,
    pub pane: Option<Pane>,
    pub active_cell: Option<(u32, u32)>,
    pub selection: Option<String>,
    pub dimension: Option<String>,
    pub show_grid: bool,
    pub view: View,
    pub shared: HashMap<u32, SharedFormula>,
    /// The worksheet root's namespace declarations (to read fragments of it again).
    pub ns_decls: Vec<(String, String)>,
    /// The AutoFilter's range (`<autoFilter ref>`), when the sheet has one.
    pub auto_filter: Option<CellRange>,
}

/// Excel's width conversion for Calibri 11 (maximum digit width 7 px): character units to points.
pub fn chars_to_pt(width: f64) -> f64 {
    let mdw = 7.0;
    let px = (((256.0 * width + (128.0f64 / mdw).trunc()) / 256.0) * mdw).trunc();
    px.max(0.0) * 0.75
}

/// The default column width from `baseColWidth` (characters): padded and rounded up to 8 px.
pub fn base_width_to_pt(base: f64) -> f64 {
    let px = base * 7.0 + 5.0;
    let px = (px / 8.0).ceil() * 8.0;
    px * 0.75
}

impl Sheet {
    pub fn new(name: &str) -> Sheet {
        Sheet {
            name: name.to_string(),
            sheet_id: 1,
            rel_id: String::new(),
            state: SheetState::Visible,
            kind: SheetKind::Worksheet,
            part: None,
            rows: BTreeMap::new(),
            cols: Vec::new(),
            default_row_height: 15.0,
            default_col_width: base_width_to_pt(8.0),
            merges: Vec::new(),
            pane: None,
            active_cell: None,
            selection: None,
            dimension: None,
            show_grid: true,
            view: View::default(),
            shared: HashMap::new(),
            ns_decls: Vec::new(),
            auto_filter: None,
        }
    }

    pub fn cell(&self, row: u32, col: u32) -> Option<&Cell> {
        self.rows.get(&row)?.cells.get(&col)
    }

    /// The `<col>` run holding a column.
    pub fn col_info(&self, col: u32) -> Option<&ColInfo> {
        let i = self.cols.partition_point(|c| c.max < col);
        self.cols.get(i).filter(|c| c.min <= col && col <= c.max)
    }

    /// A row's height in points (0 when hidden).
    pub fn row_height(&self, row: u32) -> f64 {
        match self.rows.get(&row) {
            Some(r) if r.hidden => 0.0,
            Some(r) => r.height.unwrap_or(self.default_row_height),
            None => self.default_row_height,
        }
    }

    /// A column's width in points (0 when hidden).
    pub fn col_width(&self, col: u32) -> f64 {
        match self.col_info(col) {
            Some(c) if c.hidden => 0.0,
            Some(c) => c.width.map(chars_to_pt).unwrap_or(self.default_col_width),
            None => self.default_col_width,
        }
    }

    /// The style a cell shows with: its own, else its row's, else its column's, else 0.
    pub fn effective_style(&self, row: u32, col: u32) -> u32 {
        if let Some(r) = self.rows.get(&row) {
            if let Some(c) = r.cells.get(&col) {
                return c.style;
            }
            if let Some(s) = r.style {
                return s;
            }
        }
        self.col_info(col).and_then(|c| c.style).unwrap_or(0)
    }

    /// The range the stored cells cover (computed from the cells, not the `dimension` element).
    pub fn used_range(&self) -> Option<CellRange> {
        let mut out: Option<CellRange> = None;
        for (&r, row) in &self.rows {
            let (Some((&c1, _)), Some((&c2, _))) = (row.cells.iter().next(), row.cells.iter().next_back()) else { continue };
            let here = CellRange::new(r, c1, r, c2);
            out = Some(match out {
                Some(o) => o.union(&here),
                None => here,
            });
        }
        out
    }

    /// The merged range whose top-left cell this is, or that covers this cell.
    pub fn merge_at(&self, row: u32, col: u32) -> Option<&CellRange> {
        self.merges.iter().find(|m| m.contains(row, col))
    }

    pub fn frozen(&self) -> (u32, u32) {
        match &self.pane {
            Some(p) if p.frozen => (p.y_split.min(MAX_ROWS), p.x_split.min(MAX_COLS)),
            _ => (0, 0),
        }
    }
}

/// A `<definedName>`.
#[derive(Debug, Clone, PartialEq)]
pub struct DefinedName {
    pub name: String,
    pub value: String,
    /// `localSheetId`: the sheet the name belongs to (None: the whole workbook).
    pub local_sheet: Option<usize>,
    pub hidden: bool,
}
