//! Ledger's commands: what the member does by hand and the companion does by chat, each an
//! ordinary bus command with its undo. Cell writes go through [`LedgerDoc::commit_cells`], wider
//! changes through [`LedgerDoc::commit_parts`]; caret moves (`ledger.move`, `ledger.enter`,
//! `ledger.tab`, `ledger.select_all`, `ledger.find`) change nothing in the file and leave no undo
//! step (they may scroll the view so the active cell shows).

use std::collections::{BTreeMap, HashMap};

use super::cellref::{cell_name, col_index, col_name, CellRange, MAX_COLS, MAX_ROWS};
use super::commit::PartSnap;
use super::edit::{self, CellSpec, CellWrite};
use super::editing::{Place, Sel};
use super::entry::{input_text, parse_input, Entry};
use super::model::{Cell, CellValue, Sheet};
use super::refs::{self, Axis, Shift};
use super::stylesedit::{self, argb, FormatChange, Side};
use super::LedgerDoc;
use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command};
use crate::ground::edit::Selection;
use crate::ground::xml::{Element, Node};

/// The most cells one command writes.
pub const WRITE_LIMIT: u64 = 1_000_000;

fn bad(cmd: &str, msg: impl Into<String>) -> BusError {
    BusError::bad_args(cmd, msg)
}

/// How a find looks.
#[derive(Debug, Clone, Copy)]
pub struct FindOpts {
    pub formulas: bool,
    pub match_case: bool,
    pub whole: bool,
}

impl FindOpts {
    pub fn of(a: &Json) -> FindOpts {
        FindOpts {
            formulas: a.get("in").and_then(Json::as_str) != Some("values"),
            match_case: a.get("match_case").and_then(Json::as_bool) == Some(true),
            whole: a.get("whole").and_then(Json::as_bool) == Some(true),
        }
    }

    pub fn matches(&self, text: &str, query: &str) -> bool {
        if self.match_case {
            if self.whole {
                text == query
            } else {
                text.contains(query)
            }
        } else {
            let (t, q) = (text.to_lowercase(), query.to_lowercase());
            if self.whole {
                t == q
            } else {
                t.contains(&q)
            }
        }
    }

    /// `text` with `query` replaced by `with`.
    pub fn replace(&self, text: &str, query: &str, with: &str) -> String {
        if self.whole {
            return if self.matches(text, query) { with.to_string() } else { text.to_string() };
        }
        if self.match_case {
            return text.replace(query, with);
        }
        let lower = text.to_lowercase();
        let q = query.to_lowercase();
        if q.is_empty() || lower.len() != text.len() {
            return text.replace(query, with);
        }
        let mut out = String::new();
        let mut i = 0;
        while let Some(k) = lower[i..].find(&q) {
            out.push_str(&text[i..i + k]);
            out.push_str(with);
            i += k + q.len();
        }
        out.push_str(&text[i..]);
        out
    }
}

/// The number format codes behind the ribbon's names.
pub fn format_code_for(name: &str) -> Option<&'static str> {
    Some(match name {
        "general" => "General",
        "number" => "0.00",
        "comma" => r#"_(* #,##0.00_);_(* \(#,##0.00\);_(* "-"??_);_(@_)"#,
        "currency" => "$#,##0.00_);($#,##0.00)",
        "accounting" => r#"_("$"* #,##0.00_);_("$"* \(#,##0.00\);_("$"* "-"??_);_(@_)"#,
        "date" | "short_date" => "m/d/yyyy",
        "long_date" => "dddd, mmmm d, yyyy",
        "time" => "h:mm:ss AM/PM",
        "percent" | "percentage" => "0.00%",
        "fraction" => "# ?/?",
        "scientific" => "0.00E+00",
        "text" => "@",
        _ => return None,
    })
}

fn json_value(v: &Json, error: bool) -> CellValue {
    match v {
        Json::Num(n) if n.is_finite() => CellValue::Number(*n),
        Json::Str(s) if error => CellValue::Error(s.clone()),
        Json::Str(s) if s.is_empty() => CellValue::Empty,
        Json::Str(s) => CellValue::Text(s.clone()),
        Json::Bool(b) => CellValue::Bool(*b),
        _ => CellValue::Empty,
    }
}

fn whole_cols(r: &CellRange) -> bool {
    r.r1 == 0 && r.r2 == MAX_ROWS - 1
}

fn whole_rows(r: &CellRange) -> bool {
    r.c1 == 0 && r.c2 == MAX_COLS - 1
}

/// The worksheet elements in their schema order (new elements go in their place).
const SHEET_ORDER: [&str; 38] = [
    "sheetPr", "dimension", "sheetViews", "sheetFormatPr", "cols", "sheetData", "sheetCalcPr", "sheetProtection", "protectedRanges", "scenarios", "autoFilter", "sortState", "dataConsolidate", "customSheetViews", "mergeCells", "phoneticPr", "conditionalFormatting", "dataValidations", "hyperlinks", "printOptions", "pageMargins", "pageSetup", "headerFooter", "rowBreaks", "colBreaks", "customProperties", "cellWatches", "ignoredErrors", "smartTags", "drawing", "legacyDrawing", "legacyDrawingHF", "drawingHF", "picture", "oleObjects", "controls", "webPublishItems", "tableParts",
];

/// Puts `el` among the root's children where the schema wants it (after its predecessors).
pub fn insert_in_order(root: &mut Element, el: Element, order: &[&str]) {
    let me = order.iter().position(|n| *n == el.local()).unwrap_or(order.len());
    let at = root
        .children
        .iter()
        .position(|n| match n {
            Node::Element(e) => order.iter().position(|o| *o == e.local()).map_or(e.local() == "extLst", |k| k > me),
            _ => false,
        })
        .unwrap_or(root.children.len());
    root.children.insert(at, Node::Element(el));
}

pub fn child_index(root: &Element, local: &str) -> Option<usize> {
    root.children.iter().position(|n| matches!(n, Node::Element(e) if e.local() == local))
}

pub fn child_at(root: &mut Element, i: usize) -> &mut Element {
    match &mut root.children[i] {
        Node::Element(e) => e,
        _ => unreachable!("an element index"),
    }
}

/// The child `local` of `root`, made in its schema place when missing.
pub fn ensure_child<'a>(root: &'a mut Element, local: &str, order: &[&str]) -> &'a mut Element {
    if child_index(root, local).is_none() {
        let prefix = root.prefix().map(str::to_string);
        insert_in_order(root, Element::new(edit::qualify(&prefix, local)), order);
    }
    let i = child_index(root, local).expect("just made");
    child_at(root, i)
}

impl LedgerDoc {
    /// The cells a command acts on: its `sel`, else its `range`/`cell` (and `sheet`), else the
    /// sheet's active cell.
    pub(super) fn target(&self, cmd: &Command) -> Result<Sel, BusError> {
        let name = cmd.name.as_str();
        if let Some(s) = cmd.args.get("sel").and_then(Selection::from_json) {
            return self.sel_of(&s).ok_or_else(|| bad(name, "\"sel\" names no cell of this workbook"));
        }
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let sheet = &self.sheets[si];
        match cmd.args.get("range").or_else(|| cmd.args.get("cell")).and_then(Json::as_str) {
            Some(t) => {
                let r = CellRange::parse(t).ok_or_else(|| bad(name, format!("'{t}' is not a range like \"A1:D10\"")))?;
                let r = super::editing::grow_over_merges(sheet, r);
                Ok(Sel { sheet: si, range: r, anchor: (r.r1, r.c1), active: Place::new(si, r.r1, r.c1) })
            }
            None => {
                let (r, c) = sheet.active_cell.unwrap_or((0, 0));
                Ok(Sel { sheet: si, range: CellRange::cell(r, c), anchor: (r, c), active: Place::new(si, r, c) })
            }
        }
    }

    /// The range clipped to where the sheet's cells are (whole rows and columns, select all).
    fn clipped(&self, si: usize, r: &CellRange) -> CellRange {
        match self.sheets[si].used_range() {
            Some(u) if whole_cols(r) || whole_rows(r) || r.rows() * r.cols() > WRITE_LIMIT => {
                CellRange::new(r.r1, r.c1, r.r2.min(u.r2.max(r.r1)), r.c2.min(u.c2.max(r.c1)))
            }
            Some(_) => *r,
            None if whole_cols(r) || whole_rows(r) => CellRange::new(r.r1, r.c1, r.r1, r.c1),
            None => *r,
        }
    }

    fn check_size(name: &str, r: &CellRange) -> Result<(), BusError> {
        if r.rows() * r.cols() > WRITE_LIMIT {
            return Err(bad(name, format!("{} is {} cells; one change writes at most {}", r, r.rows() * r.cols(), WRITE_LIMIT)));
        }
        Ok(())
    }

    fn is_text_cell(&self, sheet: &Sheet, r: u32, c: u32) -> bool {
        self.styles.format_code(self.styles.xf(sheet.effective_style(r, c)).num_fmt) == "@"
    }

    fn is_general(&self, style: u32) -> bool {
        self.styles.format_code(self.styles.xf(style).num_fmt).eq_ignore_ascii_case("general")
    }

    // --- styles -----------------------------------------------------------------------------

    pub(super) fn styles_snapshot(&self) -> Vec<PartSnap> {
        match &self.styles_part {
            Some(p) => self.snapshot(&[p.clone()]),
            None => vec![("/xl/styles.xml".into(), None)],
        }
    }

    /// The styles part, made (with the usual defaults) when the workbook has none.
    fn ensure_styles(&mut self) -> Result<String, BusError> {
        if let Some(p) = &self.styles_part {
            return Ok(p.clone());
        }
        let name = "/xl/styles.xml".to_string();
        let blank = super::blank_styles();
        self.pkg.add_part(&name, crate::ground::opc::CT_SPREADSHEETML_STYLES, blank.into_bytes()).map_err(|e| BusError::failed(e.0))?;
        let wb = self.workbook_part.clone();
        self.pkg.add_relationship(&wb, crate::ground::opc::REL_STYLES, &name, false);
        self.styles_part = Some(name.clone());
        Ok(name)
    }

    /// The `cellXfs` index for each (base, change), in one edit of the styles part.
    pub(super) fn derive_many(&mut self, reqs: &[(u32, FormatChange)]) -> Result<Vec<u32>, BusError> {
        if reqs.is_empty() {
            return Ok(Vec::new());
        }
        let part = self.ensure_styles()?;
        let mut out = Vec::new();
        self.pkg
            .edit_xml(&part, |root| {
                for (base, ch) in reqs {
                    out.push(if ch.is_empty() { *base } else { stylesedit::derive(root, *base, ch) });
                }
            })
            .map_err(|e| BusError::failed(e.0))?;
        let bytes = self.pkg.part_bytes(&part).map_err(|e| BusError::failed(e.0))?;
        self.styles = super::Styles::parse(&bytes, &self.theme).unwrap_or_default();
        Ok(out)
    }

    /// The new style of each base style under a change worked out per base (`f` gives the change
    /// for a base). Answers base -> new.
    fn restyle(&mut self, bases: &[u32], f: &dyn Fn(&LedgerDoc, u32) -> FormatChange) -> Result<HashMap<u32, u32>, BusError> {
        let mut uniq: Vec<u32> = bases.to_vec();
        uniq.sort_unstable();
        uniq.dedup();
        let reqs: Vec<(u32, FormatChange)> = uniq.iter().map(|&b| (b, f(self, b))).collect();
        let ids = self.derive_many(&reqs)?;
        Ok(uniq.into_iter().zip(ids).collect())
    }

    // --- writing entries ----------------------------------------------------------------------

    /// Writes typed entries (each may set a General cell's number format).
    pub(super) fn put_entries(&mut self, si: usize, list: Vec<(u32, u32, Entry)>) -> Result<Change, BusError> {
        let snap = self.styles_snapshot();
        let mut reqs: Vec<(u32, FormatChange)> = Vec::new();
        let mut plan = Vec::with_capacity(list.len());
        for (r, c, e) in list {
            let style = self.sheets[si].effective_style(r, c);
            let mut want = None;
            if let Some(code) = e.format {
                if self.is_general(style) {
                    let ch = FormatChange { num_fmt: Some(code.to_string()), ..Default::default() };
                    let k = match reqs.iter().position(|q| q.0 == style && q.1 == ch) {
                        Some(k) => k,
                        None => {
                            reqs.push((style, ch));
                            reqs.len() - 1
                        }
                    };
                    want = Some(k);
                }
            }
            plan.push((r, c, e, style, want));
        }
        let ids = self.derive_many(&reqs)?;
        let writes = plan
            .into_iter()
            .map(|(r, c, e, style, want)| {
                let style = want.map(|k| ids[k]).unwrap_or(style);
                CellWrite { row: r, col: c, spec: Some(CellSpec { value: e.value, formula: e.formula, style, sst: None }) }
            })
            .collect();
        self.commit_cells(si, writes, snap)
    }

    /// Scrolls a sheet's view so cell (r, c) shows whole (frozen rows and columns stay).
    pub(super) fn ensure_visible(&mut self, si: usize, r: u32, c: u32) {
        let Some(sheet) = self.sheets.get_mut(si) else { return };
        let (fr, fc) = sheet.frozen();
        let g = super::editing::geo(sheet);
        if r >= fr {
            let avail = (g.rows.end - g.rows.frozen_end).max(1.0);
            if r < sheet.view.row {
                sheet.view.row = r;
            } else {
                let mut used = 0.0;
                let mut top = r;
                loop {
                    used += sheet.row_height(top);
                    if used > avail || top == fr {
                        if used > avail && top < r {
                            top += 1;
                        }
                        break;
                    }
                    top -= 1;
                }
                if top > sheet.view.row {
                    sheet.view.row = top;
                }
            }
        }
        if c >= fc {
            let avail = (g.cols.end - g.cols.frozen_end).max(1.0);
            if c < sheet.view.col {
                sheet.view.col = c;
            } else {
                let mut used = 0.0;
                let mut left = c;
                loop {
                    used += sheet.col_width(left);
                    if used > avail || left == fc {
                        if used > avail && left < c {
                            left += 1;
                        }
                        break;
                    }
                    left -= 1;
                }
                if left > sheet.view.col {
                    sheet.view.col = left;
                }
            }
        }
        self.refresh(si);
    }

    // --- caret moves (no step) ---------------------------------------------------------------

    fn cmd_move(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let motion = cmd.args.get("motion").and_then(Json::as_str).unwrap_or("");
        if !crate::ground::edit::MOTIONS.contains(&motion) {
            return Err(bad(name, format!("\"motion\" is one of: {}", crate::ground::edit::MOTIONS.join(", "))));
        }
        self.target(cmd)?;
        if let Some(s) = self.landing(cmd) {
            if let Some(p) = self.place(s.focus.json()) {
                self.ensure_visible(p.sheet, p.row, p.col);
            }
        }
        Ok(Change::default())
    }

    fn cmd_enter(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        if cmd.name == "ledger.enter" && cmd.args.get("ctrl").and_then(Json::as_bool) == Some(true) && !t.is_cell() {
            return self.fill_selection(cmd, &t);
        }
        if let Some(s) = self.landing(cmd) {
            if let Some(p) = self.place(s.focus.json()) {
                self.ensure_visible(p.sheet, p.row, p.col);
            }
        }
        Ok(Change::default())
    }

    /// Ctrl+Enter: the active cell's content into every cell of the selection.
    fn fill_selection(&mut self, cmd: &Command, t: &Sel) -> Result<Change, BusError> {
        let r = self.clipped(t.sheet, &t.range);
        Self::check_size(&cmd.name, &r)?;
        let (ar, ac) = (t.active.row, t.active.col);
        let sheet = &self.sheets[t.sheet];
        let src = sheet.cell(ar, ac).cloned();
        let mut writes = Vec::new();
        for row in r.r1..=r.r2 {
            for col in r.c1..=r.c2 {
                if (row, col) == (ar, ac) {
                    continue;
                }
                let style = sheet.effective_style(row, col);
                let spec = src.as_ref().map(|s| CellSpec {
                    value: if s.formula.is_some() { CellValue::Empty } else { s.value.clone() },
                    formula: s.formula.as_ref().map(|f| refs::copy_shift(f, row as i64 - ar as i64, col as i64 - ac as i64)),
                    style,
                    sst: None,
                });
                writes.push(CellWrite { row, col, spec: spec.or_else(|| (style != 0).then(|| CellSpec { value: CellValue::Empty, formula: None, style, sst: None })) });
            }
        }
        self.commit_cells(t.sheet, writes, Vec::new())
    }

    // --- typing ------------------------------------------------------------------------------

    fn cmd_type(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        let draft = self.draft_after(cmd, &t);
        let (r, c) = (t.active.row, t.active.col);
        let text_cell = self.is_text_cell(&self.sheets[t.sheet], r, c);
        let e = parse_input(&draft, self.date1904, text_cell);
        self.cut = None;
        let ch = self.put_entries(t.sheet, vec![(r, c, e)])?;
        self.ensure_visible(t.sheet, r, c);
        Ok(ch)
    }

    pub(super) fn cmd_set_cell(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let cell = cmd.args.get("cell").and_then(Json::as_str).ok_or_else(|| bad(name, "\"cell\" is an A1 reference like \"B2\""))?;
        let (r, c) = super::parse_cell(cell).ok_or_else(|| bad(name, format!("'{cell}' is not a cell reference (A1 to XFD1048576)")))?;
        let e = match (cmd.args.get("input"), cmd.args.get("value")) {
            (Some(Json::Str(s)), _) => parse_input(s, self.date1904, self.is_text_cell(&self.sheets[si], r, c)),
            (_, v) => Entry {
                value: match v {
                    None | Some(Json::Null) => CellValue::Empty,
                    Some(Json::Num(n)) if n.is_finite() => CellValue::Number(*n),
                    Some(Json::Str(s)) if s.is_empty() => CellValue::Empty,
                    Some(Json::Str(s)) => CellValue::Text(s.clone()),
                    Some(Json::Bool(b)) => CellValue::Bool(*b),
                    _ => return Err(bad(name, "\"value\" is a number, a string, a boolean or null")),
                },
                formula: None,
                format: None,
            },
        };
        if let CellValue::Text(s) = &e.value {
            if s.chars().count() > 32_767 {
                return Err(bad(name, "a cell holds at most 32,767 characters"));
            }
        }
        self.worksheet_part(si)?;
        self.put_entries(si, vec![(r, c, e)])
    }

    fn cmd_set_cells(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let rows = cmd.args.get("values").and_then(Json::as_arr).ok_or_else(|| bad(name, "\"values\" is a list of rows, each a list of values"))?;
        let mut list = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let items: Vec<Json> = match row {
                Json::Arr(a) => a.clone(),
                other => vec![other.clone()],
            };
            for (j, v) in items.iter().enumerate() {
                let (r, c) = (t.range.r1 as u64 + i as u64, t.range.c1 as u64 + j as u64);
                if r >= MAX_ROWS as u64 || c >= MAX_COLS as u64 {
                    return Err(bad(name, "the values run off the sheet"));
                }
                let (r, c) = (r as u32, c as u32);
                let e = match v {
                    Json::Str(s) => parse_input(s, self.date1904, self.is_text_cell(&self.sheets[t.sheet], r, c)),
                    other => Entry { value: json_value(other, false), formula: None, format: None },
                };
                list.push((r, c, e));
            }
        }
        if list.len() as u64 > WRITE_LIMIT {
            return Err(bad(name, "too many values for one change"));
        }
        self.worksheet_part(t.sheet)?;
        self.put_entries(t.sheet, list)
    }

    // --- clear, fill -------------------------------------------------------------------------

    fn cmd_clear(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        let what = cmd.args.get("what").and_then(Json::as_str).unwrap_or("contents");
        if !matches!(what, "contents" | "formats" | "all") {
            return Err(bad(&cmd.name, "\"what\" is contents, formats or all"));
        }
        let sheet = &self.sheets[t.sheet];
        let r = t.range;
        let mut writes = Vec::new();
        for (&row, rw) in sheet.rows.range(r.r1..=r.r2) {
            for (&col, cell) in rw.cells.range(r.c1..=r.c2) {
                let spec = match what {
                    "contents" => (cell.style != 0).then(|| CellSpec { value: CellValue::Empty, formula: None, style: cell.style, sst: None }),
                    "formats" => Some(CellSpec { value: cell.value.clone(), formula: cell.formula.clone(), style: 0, sst: None }),
                    _ => None,
                };
                writes.push(CellWrite { row, col, spec });
            }
        }
        if writes.is_empty() {
            return Ok(Change::default());
        }
        self.commit_cells(t.sheet, writes, Vec::new())
    }

    fn copy_spec(src: Option<&Cell>, dr: i64, dc: i64, style: Option<u32>) -> Option<CellSpec> {
        match src {
            Some(s) => Some(CellSpec {
                value: if s.formula.is_some() { s.value.clone() } else { s.value.clone() },
                formula: s.formula.as_ref().map(|f| refs::copy_shift(f, dr, dc)),
                style: style.unwrap_or(s.style),
                sst: None,
            }),
            None => style.filter(|&s| s != 0).map(|s| CellSpec { value: CellValue::Empty, formula: None, style: s, sst: None }),
        }
    }

    fn cmd_fill(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        let dir = cmd.args.get("direction").and_then(Json::as_str).unwrap_or("down");
        let r = self.clipped(t.sheet, &t.range);
        Self::check_size(&cmd.name, &r)?;
        let sheet = &self.sheets[t.sheet];
        let mut writes = Vec::new();
        match dir {
            "down" | "up" => {
                let src_row = if dir == "down" { r.r1 } else { r.r2 };
                for col in r.c1..=r.c2 {
                    let src = sheet.cell(src_row, col);
                    for row in r.r1..=r.r2 {
                        if row != src_row {
                            writes.push(CellWrite { row, col, spec: Self::copy_spec(src, row as i64 - src_row as i64, 0, None) });
                        }
                    }
                }
            }
            "right" | "left" => {
                let src_col = if dir == "right" { r.c1 } else { r.c2 };
                for row in r.r1..=r.r2 {
                    let src = sheet.cell(row, src_col);
                    for col in r.c1..=r.c2 {
                        if col != src_col {
                            writes.push(CellWrite { row, col, spec: Self::copy_spec(src, 0, col as i64 - src_col as i64, None) });
                        }
                    }
                }
            }
            _ => return Err(bad(&cmd.name, "\"direction\" is down, right, up or left")),
        }
        self.commit_cells(t.sheet, writes, Vec::new())
    }

    /// AutoFill: the source range extended to `to`, continuing number and date series, text with a
    /// trailing number, and copying the rest (formulas move as copies do).
    fn cmd_autofill(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let to_text = cmd.args.get("to").and_then(Json::as_str).ok_or_else(|| bad(name, "\"to\" is the range to fill, like \"A1:A20\""))?;
        let to = CellRange::parse(to_text).ok_or_else(|| bad(name, format!("'{to_text}' is not a range")))?;
        let src = t.range;
        let series = cmd.args.get("series").and_then(Json::as_bool) == Some(true);
        let vertical = to.c1 == src.c1 && to.c2 == src.c2 && (to.r2 > src.r2 || to.r1 < src.r1);
        let horizontal = to.r1 == src.r1 && to.r2 == src.r2 && (to.c2 > src.c2 || to.c1 < src.c1);
        if !(vertical || horizontal) || !(to.r1 <= src.r1 && src.r2 <= to.r2 && to.c1 <= src.c1 && src.c2 <= to.c2) {
            return Err(bad(name, "\"to\" must contain the source and reach past it in one direction"));
        }
        Self::check_size(name, &to)?;
        let sheet = &self.sheets[t.sheet];
        let mut writes = Vec::new();
        let lines: Vec<u32> = if vertical { (src.c1..=src.c2).collect() } else { (src.r1..=src.r2).collect() };
        for line in lines {
            let cells: Vec<(u32, u32)> = if vertical { (src.r1..=src.r2).map(|r| (r, line)).collect() } else { (src.c1..=src.c2).map(|c| (line, c)).collect() };
            let n = cells.len() as i64;
            let vals: Vec<Option<&Cell>> = cells.iter().map(|&(r, c)| sheet.cell(r, c)).collect();
            // A number or date series.
            let nums: Option<Vec<f64>> = vals.iter().map(|v| v.filter(|c| c.formula.is_none()).and_then(|c| if let CellValue::Number(x) = c.value { Some(x) } else { None })).collect();
            let is_date = vals.first().and_then(|v| *v).is_some_and(|c| super::numfmt::is_date_format(self.styles.format_code(self.styles.xf(c.style).num_fmt)));
            let step: Option<(f64, f64)> = match &nums {
                Some(v) if v.len() >= 2 => {
                    // Least squares, as the reference product continues a trend.
                    let k = v.len() as f64;
                    let mx = (k - 1.0) / 2.0;
                    let my = v.iter().sum::<f64>() / k;
                    let sxy: f64 = v.iter().enumerate().map(|(i, y)| (i as f64 - mx) * (y - my)).sum();
                    let sxx: f64 = (0..v.len()).map(|i| (i as f64 - mx).powi(2)).sum();
                    let b = if sxx == 0.0 { 0.0 } else { sxy / sxx };
                    Some((my - b * mx, b))
                }
                Some(v) if v.len() == 1 && (is_date || series) => Some((v[0], 1.0)),
                _ => None,
            };
            // Text with a trailing number ("Item 1", "Q1").
            let text_num: Option<Vec<(String, i64, usize)>> = vals
                .iter()
                .map(|v| {
                    let c = (*v)?;
                    if c.formula.is_some() {
                        return None;
                    }
                    let CellValue::Text(s) = &c.value else { return None };
                    let digits = s.chars().rev().take_while(|ch| ch.is_ascii_digit()).count();
                    if digits == 0 || digits == s.len() || digits > 15 {
                        return None;
                    }
                    let (head, tail) = s.split_at(s.len() - digits);
                    Some((head.to_string(), tail.parse().ok()?, digits))
                })
                .collect();
            let targets: Vec<(u32, u32, i64)> = if vertical {
                (to.r1..=to.r2).filter(|r| *r < src.r1 || *r > src.r2).map(|r| (r, line, r as i64 - src.r1 as i64)).collect()
            } else {
                (to.c1..=to.c2).filter(|c| *c < src.c1 || *c > src.c2).map(|c| (line, c, c as i64 - src.c1 as i64)).collect()
            };
            for (r, c, k) in targets {
                let idx = k.rem_euclid(n) as usize;
                let (sr, sc) = cells[idx];
                let base = vals[idx];
                let style = base.map(|b| b.style).unwrap_or_else(|| sheet.effective_style(sr, sc));
                let spec = if let Some((a, b)) = step {
                    let v = super::entry::round15(a + b * k as f64);
                    Some(CellSpec { value: CellValue::Number(v), formula: None, style, sst: None })
                } else if let Some(tn) = text_num.as_ref().filter(|tn| tn.iter().all(|x| x.0 == tn[0].0)) {
                    let step = if tn.len() >= 2 { tn[tn.len() - 1].1 - tn[tn.len() - 2].1 } else { 1 };
                    let v = tn[0].1 + step * k;
                    let num = if v < 0 { v.to_string() } else { format!("{:0width$}", v, width = tn[0].2) };
                    Some(CellSpec { value: CellValue::Text(format!("{}{}", tn[0].0, num)), formula: None, style, sst: None })
                } else {
                    Self::copy_spec(base, r as i64 - sr as i64, c as i64 - sc as i64, Some(style))
                };
                writes.push(CellWrite { row: r, col: c, spec });
            }
        }
        self.commit_cells(t.sheet, writes, Vec::new())
    }

    // --- clipboard ---------------------------------------------------------------------------

    /// Where a paste, fill, sum or sort lands.
    pub(super) fn landed_range(&self, cmd: &Command, c: &Sel) -> Option<CellRange> {
        match cmd.name.as_str() {
            "ledger.paste" => {
                let (h, w) = match cmd.args.get("rich").filter(|r| r.get("ledger").is_some()) {
                    Some(rich) => (rich.get("rows").and_then(Json::as_usize)? as u32, rich.get("cols").and_then(Json::as_usize)? as u32),
                    None => {
                        let rows = super::csv::parse_delimited(cmd.args.get("text").and_then(Json::as_str).unwrap_or(""), '\t');
                        (rows.len().max(1) as u32, rows.iter().map(Vec::len).max().unwrap_or(1).max(1) as u32)
                    }
                };
                let (rh, rw) = (c.range.rows() as u32, c.range.cols() as u32);
                let tile = rh >= h && rw >= w && rh % h == 0 && rw % w == 0 && !(h == 1 && w == 1 && c.is_cell());
                let (h, w) = if tile { (rh, rw) } else { (h, w) };
                Some(CellRange::new(c.range.r1, c.range.c1, (c.range.r1 + h - 1).min(MAX_ROWS - 1), (c.range.c1 + w - 1).min(MAX_COLS - 1)))
            }
            "ledger.autofill" => CellRange::parse(cmd.args.get("to").and_then(Json::as_str)?),
            "ledger.autosum" => {
                if c.is_cell() {
                    Some(CellRange::cell(c.active.row, c.active.col))
                } else {
                    let r = (c.range.r2 + 1).min(MAX_ROWS - 1);
                    Some(CellRange::new(r, c.range.c1, r, c.range.c2))
                }
            }
            "ledger.sort" => Some(self.sort_range(c)),
            _ => Some(c.range),
        }
    }

    fn cmd_cut(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        self.cut = Some((t.sheet, t.range));
        Ok(Change::default())
    }

    fn cmd_paste(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let mode = cmd.args.get("mode").and_then(Json::as_str).unwrap_or("all");
        if !matches!(mode, "all" | "values" | "formats" | "formulas") {
            return Err(bad(name, "\"mode\" is all, values, formats or formulas"));
        }
        let dest = self.landed_range(cmd, &t).ok_or_else(|| bad(name, "nothing to paste"))?;
        Self::check_size(name, &dest)?;
        let rich = cmd.args.get("rich").filter(|r| r.get("ledger").is_some()).cloned();
        if let Some(rich) = rich {
            let ours = rich.get("ledger").and_then(Json::as_f64) == Some(self.id as f64);
            let (sr, sc) = (rich.get("row").and_then(Json::as_usize).unwrap_or(0) as u32, rich.get("col").and_then(Json::as_usize).unwrap_or(0) as u32);
            let (h, w) = (rich.get("rows").and_then(Json::as_usize).unwrap_or(1) as u32, rich.get("cols").and_then(Json::as_usize).unwrap_or(1) as u32);
            let src_sheet = rich.get("sheet").and_then(Json::as_usize).unwrap_or(t.sheet);
            // A cut pasted: the cells move, and what pointed at them follows.
            if ours && mode == "all" {
                if let Some((cs, cr)) = self.cut {
                    if cs == src_sheet && cs == t.sheet && cr.r1 == sr && cr.c1 == sc && cr.rows() == h as u64 && cr.cols() == w as u64 {
                        self.cut = None;
                        return self.move_range(t.sheet, cr, dest.r1, dest.c1);
                    }
                }
            }
            let cells: Vec<Json> = rich.get("cells").and_then(Json::as_arr).map(|a| a.to_vec()).unwrap_or_default();
            let sheet = &self.sheets[t.sheet];
            let mut writes = Vec::new();
            for row in dest.r1..=dest.r2 {
                for col in dest.c1..=dest.c2 {
                    let (i, j) = ((row - dest.r1) % h, (col - dest.c1) % w);
                    let src = cells.get(i as usize).and_then(|r| r.at(j as usize)).cloned().unwrap_or(Json::Null);
                    let (dr, dc) = (row as i64 - (sr + i) as i64, col as i64 - (sc + j) as i64);
                    let old = sheet.cell(row, col);
                    let dest_style = sheet.effective_style(row, col);
                    let s_style = src.get("s").and_then(Json::as_usize).map(|s| s as u32).filter(|_| ours);
                    let value = src.get("v").map(|v| json_value(v, src.get("e").and_then(Json::as_bool) == Some(true))).unwrap_or(CellValue::Empty);
                    let formula = src.get("f").and_then(Json::as_str).map(|f| refs::copy_shift(f, dr, dc));
                    let spec = match mode {
                        "values" => CellSpec { value, formula: None, style: dest_style, sst: None },
                        "formats" => CellSpec {
                            value: old.map(|o| o.value.clone()).unwrap_or(CellValue::Empty),
                            formula: old.and_then(|o| o.formula.clone()),
                            style: s_style.unwrap_or(dest_style),
                            sst: None,
                        },
                        "formulas" => CellSpec { value, formula, style: dest_style, sst: None },
                        _ => CellSpec { value, formula, style: s_style.unwrap_or(if ours { 0 } else { dest_style }), sst: None },
                    };
                    writes.push(CellWrite { row, col, spec: (!spec.is_blank()).then_some(spec) });
                }
            }
            return self.commit_cells(t.sheet, writes, Vec::new());
        }
        // Text from outside: tab-separated rows, each field read as if typed.
        let text = cmd.args.get("text").and_then(Json::as_str).unwrap_or("");
        let rows = super::csv::parse_delimited(text, '\t');
        if rows.is_empty() {
            return Ok(Change::default());
        }
        let (h, w) = (rows.len() as u32, rows.iter().map(Vec::len).max().unwrap_or(1).max(1) as u32);
        let mut list = Vec::new();
        for row in dest.r1..=dest.r2 {
            for col in dest.c1..=dest.c2 {
                let (i, j) = ((row - dest.r1) % h, (col - dest.c1) % w);
                let field = rows[i as usize].get(j as usize).map(String::as_str).unwrap_or("");
                let tc = self.is_text_cell(&self.sheets[t.sheet], row, col);
                let mut e = parse_input(field, self.date1904, tc);
                if mode == "values" {
                    e.formula = None;
                }
                list.push((row, col, e));
            }
        }
        self.put_entries(t.sheet, list)
    }

    /// Moves cells `from` so their top-left lands on (tr, tc): references to them follow.
    pub(super) fn move_range(&mut self, si: usize, from: CellRange, tr: u32, tc: u32) -> Result<Change, BusError> {
        let (dr, dc) = (tr as i64 - from.r1 as i64, tc as i64 - from.c1 as i64);
        if dr == 0 && dc == 0 {
            return Ok(Change::default());
        }
        if from.r2 as i64 + dr >= MAX_ROWS as i64 || from.c2 as i64 + dc >= MAX_COLS as i64 {
            return Err(BusError::failed("the cells would move off the sheet"));
        }
        let sheet_name = self.sheets[si].name.clone();
        let names = self.book_parts();
        self.commit_parts(names, move |doc| {
            for i in 0..doc.sheets.len() {
                let Some(part) = doc.sheets[i].part.clone() else { continue };
                let home = doc.sheets[i].name.clone();
                let (sn, h2) = (sheet_name.clone(), home.clone());
                doc.pkg
                    .edit_xml(&part, |root| {
                        super::structure::rewrite_formulas(
                            root,
                            &|f| refs::move_refs(f, &home, &sn, &from, dr, dc),
                            &|f| refs::mentions_sheet(f, &h2, &sheet_name),
                            None,
                        )
                    })
                    .map_err(|e| BusError::failed(e.0))?;
            }
            let wb = doc.workbook_part.clone();
            let sn = sheet_name.clone();
            doc.pkg.edit_xml(&wb, |root| super::structure::rewrite_names(root, &|f| refs::move_refs(f, "", &sn, &from, dr, dc))).map_err(|e| BusError::failed(e.0))?;
            doc.reload().map_err(BusError::failed)?;
            // The cells themselves: the block written at its new place, its old place emptied.
            let sheet = &doc.sheets[si];
            let to = CellRange::new(tr, tc, (from.r2 as i64 + dr) as u32, (from.c2 as i64 + dc) as u32);
            let mut writes = Vec::new();
            for (&r, row) in sheet.rows.range(from.r1..=from.r2) {
                for (&c, _) in row.cells.range(from.c1..=from.c2) {
                    if !to.contains(r, c) {
                        writes.push(CellWrite { row: r, col: c, spec: None });
                    }
                }
            }
            for r in to.r1..=to.r2 {
                for c in to.c1..=to.c2 {
                    let src = sheet.cell((r as i64 - dr) as u32, (c as i64 - dc) as u32);
                    let spec = src.map(|s| CellSpec { value: s.value.clone(), formula: s.formula.clone(), style: s.style, sst: None });
                    if spec.is_some() || sheet.cell(r, c).is_some() {
                        writes.push(CellWrite { row: r, col: c, spec });
                    }
                }
            }
            let part = doc.worksheet_part(si)?;
            let sst = doc.sst.clone();
            let mut res = Ok(());
            doc.pkg
                .edit_xml(&part, |root| {
                    res = edit::write_cells(root, &writes, &sst).map(|_| ());
                    // Merged areas inside the block go with it.
                    if let Some(mc) = root.child_mut("mergeCells") {
                        for n in mc.children.iter_mut() {
                            let Node::Element(m) = n else { continue };
                            if let Some(r) = m.attr("ref").and_then(CellRange::parse) {
                                if from.r1 <= r.r1 && r.r2 <= from.r2 && from.c1 <= r.c1 && r.c2 <= from.c2 {
                                    let nr = CellRange::new((r.r1 as i64 + dr) as u32, (r.c1 as i64 + dc) as u32, (r.r2 as i64 + dr) as u32, (r.c2 as i64 + dc) as u32);
                                    m.set_attr("ref", nr.to_string());
                                }
                            }
                        }
                    }
                })
                .map_err(|e| BusError::failed(e.0))?;
            res.map_err(BusError::failed)?;
            doc.drop_calc_chain();
            Ok(())
        })
    }

    fn cmd_move_range(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let from = cmd.args.get("from").and_then(Json::as_str).and_then(CellRange::parse).ok_or_else(|| bad(name, "\"from\" is a range like \"A1:B5\""))?;
        let to = cmd.args.get("to").and_then(Json::as_str).and_then(CellRange::parse).ok_or_else(|| bad(name, "\"to\" is the top-left cell of the new place"))?;
        Self::check_size(name, &from)?;
        self.move_range(si, from, to.r1, to.c1)
    }

    // --- AutoSum -----------------------------------------------------------------------------

    fn cmd_autosum(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        let func = cmd.args.get("fn").and_then(Json::as_str).unwrap_or("SUM").to_ascii_uppercase();
        if !matches!(func.as_str(), "SUM" | "AVERAGE" | "COUNT" | "MAX" | "MIN") {
            return Err(bad(&cmd.name, "\"fn\" is SUM, AVERAGE, COUNT, MAX or MIN"));
        }
        let sheet = &self.sheets[t.sheet];
        let is_num = |r: u32, c: u32| sheet.cell(r, c).is_some_and(|x| matches!(x.value, CellValue::Number(_)) || x.formula.is_some());
        let mut list = Vec::new();
        if t.is_cell() {
            let (r, c) = (t.active.row, t.active.col);
            // Up first, then left: the run of numbers next to the cell.
            let up = {
                let mut k = r;
                while k > 0 && is_num(k - 1, c) {
                    k -= 1;
                }
                (k < r).then(|| CellRange::new(k, c, r - 1, c))
            };
            let left = {
                let mut k = c;
                while k > 0 && is_num(r, k - 1) {
                    k -= 1;
                }
                (k < c).then(|| CellRange::new(r, k, r, c - 1))
            };
            let f = match up.or(left) {
                Some(rg) => format!("{func}({})", range_text(&rg)),
                None => format!("{func}()"),
            };
            list.push((r, c, Entry { value: CellValue::Empty, formula: Some(f), format: None }));
        } else {
            let row = t.range.r2 + 1;
            if row >= MAX_ROWS {
                return Err(BusError::failed("there is no row below the selection for the totals"));
            }
            for c in t.range.c1..=t.range.c2 {
                let rg = CellRange::new(t.range.r1, c, t.range.r2, c);
                list.push((row, c, Entry { value: CellValue::Empty, formula: Some(format!("{func}({})", range_text(&rg))), format: None }));
            }
        }
        self.put_entries(t.sheet, list)
    }

    // --- formatting --------------------------------------------------------------------------

    /// The format change a command's args ask for, worked out for one base style.
    fn change_for(&self, a: &Json, base: u32) -> Result<FormatChange, String> {
        let xf = self.styles.xf(base);
        let font = self.styles.font(xf.font);
        let mut ch = FormatChange::default();
        if let Some(f) = a.get("family").and_then(Json::as_str) {
            ch.family = Some(f.to_string());
        }
        if let Some(s) = a.get("size").and_then(Json::as_f64) {
            if !(1.0..=409.0).contains(&s) {
                return Err("\"size\" is from 1 to 409 points".into());
            }
            ch.size = Some(s);
        }
        if let Some(d) = a.get("size_delta").and_then(Json::as_f64) {
            // The ribbon's grow and shrink steps.
            const STEPS: [f64; 16] = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0];
            let cur = font.size;
            let next = if d > 0.0 { STEPS.iter().copied().find(|&s| s > cur).unwrap_or(cur + 12.0) } else { STEPS.iter().rev().copied().find(|&s| s < cur).unwrap_or((cur - 1.0).max(1.0)) };
            ch.size = Some(next.min(409.0));
        }
        for (k, slot) in [("bold", &mut ch.bold), ("italic", &mut ch.italic), ("underline", &mut ch.underline), ("strike", &mut ch.strike)] {
            if let Some(b) = a.get(k).and_then(Json::as_bool) {
                *slot = Some(b);
            }
        }
        if let Some(c) = a.get("color") {
            ch.color = Some(match c {
                Json::Null => None,
                Json::Str(s) => Some(argb(s).ok_or_else(|| format!("'{s}' is not a colour like \"#FF0000\""))?),
                _ => return Err("\"color\" is a colour like \"#FF0000\" or null".into()),
            });
        }
        if let Some(c) = a.get("fill") {
            ch.fill = Some(match c {
                Json::Null => None,
                Json::Str(s) if s == "none" => None,
                Json::Str(s) => Some(argb(s).ok_or_else(|| format!("'{s}' is not a colour like \"#FFFF00\""))?),
                _ => return Err("\"fill\" is a colour like \"#FFFF00\" or null".into()),
            });
        }
        if let Some(h) = a.get("h_align") {
            let v = h.as_str().unwrap_or("general");
            if !matches!(v, "general" | "left" | "center" | "right" | "fill" | "justify" | "centerContinuous" | "distributed") {
                return Err("\"h_align\" is general, left, center, right, justify, fill, centerContinuous or distributed".into());
            }
            ch.h_align = Some(Some(v.to_string()));
        }
        if let Some(h) = a.get("v_align") {
            let v = match h.as_str().unwrap_or("bottom") {
                "middle" => "center",
                x => x,
            };
            if !matches!(v, "top" | "center" | "bottom" | "justify" | "distributed") {
                return Err("\"v_align\" is top, middle, bottom, justify or distributed".into());
            }
            ch.v_align = Some(Some(v.to_string()));
        }
        if let Some(w) = a.get("wrap").and_then(Json::as_bool) {
            ch.wrap = Some(w);
        }
        if let Some(n) = a.get("indent").and_then(Json::as_usize) {
            ch.indent = Some(n.min(250) as u32);
        }
        let mut code = None;
        if let Some(nf) = a.get("number_format").and_then(Json::as_str) {
            code = Some(format_code_for(nf).map(str::to_string).unwrap_or_else(|| nf.to_string()));
        }
        if let Some(d) = a.get("decimals").and_then(Json::as_i64) {
            let cur = code.clone().unwrap_or_else(|| self.styles.format_code(xf.num_fmt).to_string());
            let cur = if cur.eq_ignore_ascii_case("general") { "0".to_string() } else { cur };
            let mut c = cur;
            if d > 0 {
                c = stylesedit::change_decimals(&c, d as i32);
            } else if d < 0 {
                c = stylesedit::change_decimals(&c, d as i32);
            }
            code = Some(c);
        }
        ch.num_fmt = code;
        Ok(ch)
    }

    /// Applies a style map (old style -> new style) to every cell of a range, whole columns and
    /// rows by their own formats.
    fn apply_styles(&mut self, cmd: &Command, t: &Sel, per_cell: &dyn Fn(&LedgerDoc, u32, u32, u32) -> FormatChange) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let snap = self.styles_snapshot();
        let r = t.range;
        if whole_cols(&r) || whole_rows(&r) {
            return self.apply_styles_wide(cmd, t, per_cell);
        }
        Self::check_size(name, &r)?;
        // Each cell's change (they differ for borders), deduplicated.
        let mut reqs: Vec<(u32, FormatChange)> = Vec::new();
        let mut plan = Vec::new();
        for row in r.r1..=r.r2 {
            for col in r.c1..=r.c2 {
                let base = self.sheets[t.sheet].effective_style(row, col);
                let ch = per_cell(self, base, row, col);
                let k = match reqs.iter().position(|q| q.0 == base && q.1 == ch) {
                    Some(k) => k,
                    None => {
                        reqs.push((base, ch));
                        reqs.len() - 1
                    }
                };
                plan.push((row, col, k));
            }
        }
        let ids = self.derive_many(&reqs)?;
        let sheet = &self.sheets[t.sheet];
        let mut writes = Vec::new();
        for (row, col, k) in plan {
            let style = ids[k];
            match sheet.cell(row, col) {
                Some(c) if c.style == style => {}
                Some(c) => writes.push(CellWrite { row, col, spec: Some(CellSpec { value: c.value.clone(), formula: c.formula.clone(), style, sst: None }) }),
                None if style != sheet.effective_style(row, col) => {
                    writes.push(CellWrite { row, col, spec: Some(CellSpec { value: CellValue::Empty, formula: None, style, sst: None }) })
                }
                None => {}
            }
        }
        self.commit_cells(t.sheet, writes, snap)
    }

    /// Whole columns or rows: their own formats change, and every stored cell in them.
    fn apply_styles_wide(&mut self, cmd: &Command, t: &Sel, per_cell: &dyn Fn(&LedgerDoc, u32, u32, u32) -> FormatChange) -> Result<Change, BusError> {
        let _ = cmd;
        let si = t.sheet;
        let r = t.range;
        let part = self.worksheet_part(si)?;
        let mut names = vec![part.clone()];
        names.push(self.styles_part.clone().unwrap_or_else(|| "/xl/styles.xml".into()));
        let cols_mode = whole_cols(&r);
        self.commit_parts(names, move |doc| {
            let sheet = doc.sheets[si].clone();
            // The bases: each column's (or row's) own style and each stored cell's.
            let mut reqs: Vec<(u32, FormatChange)> = Vec::new();
            let idx = |base: u32, ch: FormatChange, reqs: &mut Vec<(u32, FormatChange)>| -> usize {
                match reqs.iter().position(|q| q.0 == base && q.1 == ch) {
                    Some(k) => k,
                    None => {
                        reqs.push((base, ch));
                        reqs.len() - 1
                    }
                }
            };
            let mut line_plan = Vec::new();
            if cols_mode {
                for c in r.c1..=r.c2.min(MAX_COLS - 1) {
                    let base = sheet.col_info(c).and_then(|ci| ci.style).unwrap_or(0);
                    let k = idx(base, per_cell(doc, base, 0, c), &mut reqs);
                    line_plan.push((c, k));
                }
            } else {
                for row in r.r1..=r.r2.min(r.r1 + 100_000) {
                    let base = sheet.rows.get(&row).and_then(|x| x.style).unwrap_or(0);
                    let k = idx(base, per_cell(doc, base, row, 0), &mut reqs);
                    line_plan.push((row, k));
                }
            }
            let mut cell_plan = Vec::new();
            for (&row, rw) in sheet.rows.range(r.r1..=r.r2) {
                for (&col, cell) in rw.cells.range(r.c1..=r.c2) {
                    let k = idx(cell.style, per_cell(doc, cell.style, row, col), &mut reqs);
                    cell_plan.push((row, col, k));
                }
            }
            let ids = doc.derive_many(&reqs)?;
            let writes: Vec<CellWrite> = cell_plan
                .into_iter()
                .filter_map(|(row, col, k)| {
                    let c = sheet.cell(row, col)?;
                    (c.style != ids[k]).then(|| CellWrite { row, col, spec: Some(CellSpec { value: c.value.clone(), formula: c.formula.clone(), style: ids[k], sst: None }) })
                })
                .collect();
            let sst = doc.sst.clone();
            let mut res = Ok(());
            doc.pkg
                .edit_xml(&part, |root| {
                    if cols_mode {
                        let spans: Vec<(u32, u32, u32)> = line_plan.iter().map(|&(c, k)| (c, c, ids[k])).collect();
                        set_cols(root, &spans, &|col, style| {
                            if style == 0 {
                                col.attrs.retain(|(k, _)| k != "style");
                            } else {
                                col.set_attr("style", style.to_string());
                            }
                        });
                    } else {
                        let map: BTreeMap<u32, u32> = line_plan.iter().map(|&(row, k)| (row, ids[k])).collect();
                        let rows: Vec<u32> = map.keys().copied().collect();
                        let _ = edit::edit_rows(root, &rows, &|e| {
                            let rn = e.attr("r").and_then(|v| v.parse::<u32>().ok()).map(|v| v - 1).unwrap_or(0);
                            let s = map.get(&rn).copied().unwrap_or(0);
                            if s == 0 {
                                e.attrs.retain(|(k, _)| k != "s" && k != "customFormat");
                            } else {
                                e.set_attr("s", s.to_string());
                                e.set_attr("customFormat", "1");
                            }
                        });
                    }
                    res = edit::write_cells(root, &writes, &sst).map(|_| ());
                })
                .map_err(|e| BusError::failed(e.0))?;
            res.map_err(BusError::failed)
        })
    }

    fn cmd_format(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        // Check the args once.
        self.change_for(&cmd.args, 0).map_err(|e| bad(&cmd.name, e))?;
        let args = cmd.args.clone();
        self.apply_styles(cmd, &t, &|doc, base, _, _| doc.change_for(&args, base).unwrap_or_default())
    }

    fn cmd_toggle(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        let what = cmd.args.get("what").and_then(Json::as_str).unwrap_or("");
        let xf = self.styles.xf(self.sheets[t.sheet].effective_style(t.active.row, t.active.col)).clone();
        let font = self.styles.font(xf.font).clone();
        let on = match what {
            "bold" => !font.bold,
            "italic" => !font.italic,
            "underline" => !font.underline,
            "strike" => !font.strike,
            "wrap" => !xf.wrap,
            _ => return Err(bad(&cmd.name, "\"what\" is bold, italic, underline, strike or wrap")),
        };
        let mut a = Json::obj();
        a.set(what, on);
        self.apply_styles(cmd, &t, &|doc, base, _, _| doc.change_for(&a, base).unwrap_or_default())
    }

    fn cmd_borders(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let preset = cmd.args.get("preset").and_then(Json::as_str).unwrap_or("all").to_string();
        let mut style = cmd.args.get("style").and_then(Json::as_str).unwrap_or("thin").to_string();
        if !matches!(style.as_str(), "thin" | "medium" | "thick" | "double" | "dashed" | "dotted" | "hair" | "mediumDashed" | "dashDot") {
            return Err(bad(name, "\"style\" is thin, medium, thick, double, dashed, dotted or hair"));
        }
        if !matches!(preset.as_str(), "all" | "outside" | "thick_box" | "inside" | "top" | "bottom" | "left" | "right" | "none" | "bottom_double" | "top_bottom") {
            return Err(bad(name, "\"preset\" is all, outside, thick_box, inside, top, bottom, left, right, top_bottom, bottom_double or none"));
        }
        let color = match cmd.args.get("color").and_then(Json::as_str) {
            Some(c) => Some(argb(c).ok_or_else(|| bad(name, format!("'{c}' is not a colour")))?),
            None => None,
        };
        if preset == "thick_box" {
            style = "medium".into();
        }
        let r = t.range;
        let side: Side = Some((style.clone(), color.clone()));
        let dbl: Side = Some(("double".into(), color.clone()));
        self.apply_styles(cmd, &t, &move |_, _, row, col| {
            let (top, bottom, left, right) = (row == r.r1, row == r.r2, col == r.c1, col == r.c2);
            let mut ch = FormatChange::default();
            let s = || side.clone();
            match preset.as_str() {
                "all" => {
                    ch.top = Some(s());
                    ch.bottom = Some(s());
                    ch.left = Some(s());
                    ch.right = Some(s());
                }
                "outside" | "thick_box" => {
                    if top {
                        ch.top = Some(s());
                    }
                    if bottom {
                        ch.bottom = Some(s());
                    }
                    if left {
                        ch.left = Some(s());
                    }
                    if right {
                        ch.right = Some(s());
                    }
                }
                "inside" => {
                    if !bottom {
                        ch.bottom = Some(s());
                    }
                    if !right {
                        ch.right = Some(s());
                    }
                }
                "top" if top => ch.top = Some(s()),
                "bottom" if bottom => ch.bottom = Some(s()),
                "left" if left => ch.left = Some(s()),
                "right" if right => ch.right = Some(s()),
                "bottom_double" if bottom => ch.bottom = Some(dbl.clone()),
                "top_bottom" => {
                    if top {
                        ch.top = Some(s());
                    }
                    if bottom {
                        ch.bottom = Some(s());
                    }
                }
                "none" => {
                    ch.top = Some(None);
                    ch.bottom = Some(None);
                    ch.left = Some(None);
                    ch.right = Some(None);
                }
                _ => {}
            }
            ch
        })
    }

    /// Format Painter: the first press takes the selection's formats, the next paints them.
    fn cmd_painter(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        match self.painter.take() {
            None => {
                self.painter = Some((t.sheet, t.range));
                Ok(Change::default())
            }
            Some((ps, pr)) => self.paint_formats(ps, pr, &t),
        }
    }

    fn cmd_copy_format(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let from = cmd.args.get("from").and_then(Json::as_str).and_then(CellRange::parse).ok_or_else(|| bad(name, "\"from\" is the range whose formats to copy"))?;
        let fs = match cmd.args.get("from_sheet") {
            Some(v) => self.sheet_index(name, Some(v))?,
            None => t.sheet,
        };
        self.paint_formats(fs, from, &t)
    }

    /// The styles of `from` (tiled) onto the target's cells.
    fn paint_formats(&mut self, fs: usize, from: CellRange, t: &Sel) -> Result<Change, BusError> {
        let src = &self.sheets[fs];
        let (h, w) = (from.rows() as u32, from.cols() as u32);
        let r = if t.is_cell() { CellRange::new(t.range.r1, t.range.c1, (t.range.r1 + h - 1).min(MAX_ROWS - 1), (t.range.c1 + w - 1).min(MAX_COLS - 1)) } else { self.clipped(t.sheet, &t.range) };
        Self::check_size("ledger.copy_format", &r)?;
        let styles: Vec<Vec<u32>> = (0..h).map(|i| (0..w).map(|j| src.effective_style(from.r1 + i, from.c1 + j)).collect()).collect();
        let sheet = &self.sheets[t.sheet];
        let mut writes = Vec::new();
        for row in r.r1..=r.r2 {
            for col in r.c1..=r.c2 {
                let s = styles[((row - r.r1) % h) as usize][((col - r.c1) % w) as usize];
                match sheet.cell(row, col) {
                    Some(c) if c.style == s => {}
                    Some(c) => writes.push(CellWrite { row, col, spec: Some(CellSpec { value: c.value.clone(), formula: c.formula.clone(), style: s, sst: None }) }),
                    None if s != sheet.effective_style(row, col) => writes.push(CellWrite { row, col, spec: Some(CellSpec { value: CellValue::Empty, formula: None, style: s, sst: None }) }),
                    None => {}
                }
            }
        }
        self.commit_cells(t.sheet, writes, Vec::new())
    }

    // --- merge -------------------------------------------------------------------------------

    fn cmd_merge(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let mode = cmd.args.get("mode").and_then(Json::as_str).unwrap_or("center").to_string();
        if !matches!(mode.as_str(), "center" | "across" | "merge") {
            return Err(bad(name, "\"mode\" is center, across or merge"));
        }
        let r = t.range;
        if r.is_single() || whole_cols(&r) || whole_rows(&r) {
            return Err(bad(name, "select at least two cells (and not whole rows or columns) to merge"));
        }
        Self::check_size(name, &r)?;
        let si = t.sheet;
        let part = self.worksheet_part(si)?;
        let names = vec![part.clone(), self.styles_part.clone().unwrap_or_else(|| "/xl/styles.xml".into())];
        self.commit_parts(names, move |doc| {
            let sheet = doc.sheets[si].clone();
            let areas: Vec<CellRange> = if mode == "across" { (r.r1..=r.r2).map(|row| CellRange::new(row, r.c1, row, r.c2)).collect() } else { vec![r] };
            // The top-left keeps its value (centred for Merge & Center); the others are emptied.
            let mut center_ids = HashMap::new();
            if mode == "center" {
                let bases: Vec<u32> = areas.iter().map(|a| sheet.effective_style(a.r1, a.c1)).collect();
                center_ids = doc.restyle(&bases, &|_, _| FormatChange { h_align: Some(Some("center".into())), ..Default::default() })?;
            }
            let mut writes = Vec::new();
            for a in &areas {
                for (&row, rw) in sheet.rows.range(a.r1..=a.r2) {
                    for (&col, c) in rw.cells.range(a.c1..=a.c2) {
                        if (row, col) == (a.r1, a.c1) {
                            continue;
                        }
                        if !c.value.is_empty() || c.formula.is_some() {
                            writes.push(CellWrite { row, col, spec: Some(CellSpec { value: CellValue::Empty, formula: None, style: c.style, sst: None }) });
                        }
                    }
                }
                if let Some(&s) = center_ids.get(&sheet.effective_style(a.r1, a.c1)) {
                    let c = sheet.cell(a.r1, a.c1);
                    writes.push(CellWrite {
                        row: a.r1,
                        col: a.c1,
                        spec: Some(CellSpec { value: c.map(|c| c.value.clone()).unwrap_or(CellValue::Empty), formula: c.and_then(|c| c.formula.clone()), style: s, sst: None }),
                    });
                }
            }
            let sst = doc.sst.clone();
            let mut res = Ok(());
            doc.pkg
                .edit_xml(&part, |root| {
                    res = edit::write_cells(root, &writes, &sst).map(|_| ());
                    let prefix = root.prefix().map(str::to_string);
                    let mc = ensure_child(root, "mergeCells", &SHEET_ORDER);
                    // Merges the new ones overlap are taken in.
                    mc.children.retain(|n| match n {
                        Node::Element(m) => !m.attr("ref").and_then(CellRange::parse).is_some_and(|x| areas.iter().any(|a| a.intersects(&x))),
                        _ => true,
                    });
                    for a in &areas {
                        let mut m = Element::new(edit::qualify(&prefix, "mergeCell"));
                        m.attrs.push(("ref".into(), a.to_string()));
                        mc.children.push(Node::Element(m));
                    }
                    let n = mc.children_named("mergeCell").count();
                    mc.set_attr("count", n.to_string());
                })
                .map_err(|e| BusError::failed(e.0))?;
            res.map_err(BusError::failed)
        })
    }

    fn cmd_unmerge(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        let r = t.range;
        if !self.sheets[t.sheet].merges.iter().any(|m| m.intersects(&r)) {
            return Ok(Change::default());
        }
        let part = self.worksheet_part(t.sheet)?;
        self.commit_parts(vec![part.clone()], move |doc| {
            doc.pkg
                .edit_xml(&part, |root| {
                    if let Some(mc) = root.child_mut("mergeCells") {
                        mc.children.retain(|n| match n {
                            Node::Element(m) => !m.attr("ref").and_then(CellRange::parse).is_some_and(|x| x.intersects(&r)),
                            _ => true,
                        });
                        let n = mc.children_named("mergeCell").count();
                        mc.set_attr("count", n.to_string());
                    }
                    if root.child("mergeCells").is_some_and(|m| m.children_named("mergeCell").next().is_none()) {
                        root.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "mergeCells"));
                    }
                })
                .map_err(|e| BusError::failed(e.0))?;
            Ok(())
        })
    }

    // --- structure ---------------------------------------------------------------------------

    /// Rows or columns inserted or deleted (or cells shifted) everywhere they are named.
    pub(super) fn structural(&mut self, si: usize, op: Shift) -> Result<Change, BusError> {
        let target = self.sheets[si].name.clone();
        let tpart = self.worksheet_part(si)?;
        let names = self.book_parts();
        self.commit_parts(names, move |doc| {
            for i in 0..doc.sheets.len() {
                let Some(part) = doc.sheets[i].part.clone() else { continue };
                let home = doc.sheets[i].name.clone();
                let is_target = part == tpart;
                let op2 = op.clone();
                let tname = target.clone();
                doc.pkg
                    .edit_xml(&part, |root| {
                        if is_target {
                            if let Some(sdi) = edit::sheet_data_index(root) {
                                if let Node::Element(sd) = &mut root.children[sdi] {
                                    let all = edit::shared_groups(sd, None);
                                    edit::explode_shared(sd, &all);
                                }
                            }
                        }
                        let range_map = |r: &CellRange| op2.range(r);
                        super::structure::rewrite_formulas(
                            root,
                            &|f| refs::shift_structure(f, &home, &op2),
                            &|f| refs::mentions_sheet(f, &home, &tname),
                            if is_target { Some(&range_map) } else { None },
                        );
                        if is_target {
                            super::structure::remap_sheet_data(root, &op2);
                            super::structure::shift_sheet_elements(root, &op2, &home);
                            super::structure::fix_dimension(root);
                        }
                    })
                    .map_err(|e| BusError::failed(e.0))?;
            }
            let wb = doc.workbook_part.clone();
            let op3 = op.clone();
            doc.pkg.edit_xml(&wb, |root| super::structure::rewrite_names(root, &|f| refs::shift_structure(f, "", &op3))).map_err(|e| BusError::failed(e.0))?;
            doc.drop_calc_chain();
            Ok(())
        })
    }

    fn cmd_insert_delete(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let insert = name == "ledger.insert";
        let default = if insert { "rows" } else { "rows" };
        let what = cmd.args.get("what").and_then(Json::as_str).unwrap_or(default);
        let r = t.range;
        let count_arg = cmd.args.get("count").and_then(Json::as_usize);
        let sign = if insert { 1i64 } else { -1i64 };
        let op = match (insert, what) {
            (_, "rows") => Shift { sheet: String::new(), axis: Axis::Row, at: r.r1, count: sign * count_arg.unwrap_or(r.rows() as usize) as i64, band: None },
            (_, "cols") => Shift { sheet: String::new(), axis: Axis::Col, at: r.c1, count: sign * count_arg.unwrap_or(r.cols() as usize) as i64, band: None },
            (true, "cells_down") | (false, "cells_up") => Shift { sheet: String::new(), axis: Axis::Row, at: r.r1, count: sign * r.rows() as i64, band: Some((r.c1, r.c2)) },
            (true, "cells_right") | (false, "cells_left") => Shift { sheet: String::new(), axis: Axis::Col, at: r.c1, count: sign * r.cols() as i64, band: Some((r.r1, r.r2)) },
            _ => {
                return Err(bad(
                    name,
                    if insert { "\"what\" is rows, cols, cells_down or cells_right" } else { "\"what\" is rows, cols, cells_up or cells_left" },
                ))
            }
        };
        if op.count == 0 {
            return Ok(Change::default());
        }
        let max = if op.axis == Axis::Row { MAX_ROWS } else { MAX_COLS };
        if insert {
            if let Some(u) = self.sheets[t.sheet].used_range() {
                let last = if op.axis == Axis::Row { u.r2 } else { u.c2 };
                if last as i64 + op.count >= max as i64 && last >= op.at {
                    return Err(BusError::failed("there is no room: the cells at the end of the sheet would be pushed off"));
                }
            }
        } else if op.at as i64 - op.count > max as i64 {
            return Err(bad(name, "that reaches past the end of the sheet"));
        }
        let mut op = op;
        op.sheet = self.sheets[t.sheet].name.clone();
        self.structural(t.sheet, op)
    }

    /// Changes rows' own elements (`edit_rows`) as one change with row snapshots.
    fn commit_rows(&mut self, si: usize, rows: Vec<u32>, f: &dyn Fn(&mut Element)) -> Result<Change, BusError> {
        let part = self.worksheet_part(si)?;
        let mut outcome = None;
        self.pkg.edit_xml(&part, |root| outcome = Some(edit::edit_rows(root, &rows, f))).map_err(|e| BusError::failed(e.0))?;
        let outcome = outcome.ok_or_else(|| BusError::failed("the worksheet could not be read"))?.map_err(BusError::failed)?;
        self.reparse_rows(si, &outcome.new_rows).map_err(BusError::failed)?;
        self.refresh(si);
        if outcome.old_rows.iter().zip(&outcome.new_rows).all(|(a, b)| a.xml == b.xml) {
            return Ok(Change::default());
        }
        Ok(Change { undo: vec![Self::restore_cmd(Some(si), &outcome.old_rows, None, &[])], pages_touched: vec![si] })
    }

    /// The text width of a cell in points, with its font.
    fn text_width(&self, sheet: &Sheet, r: u32, c: u32) -> f64 {
        use crate::ground::text::{Measurer, TextStyle};
        let Some(cell) = sheet.cell(r, c) else { return 0.0 };
        let text = self.formatted(cell);
        if text.is_empty() {
            return 0.0;
        }
        let xf = self.styles.xf(cell.style);
        let font = self.styles.font(xf.font);
        let style = TextStyle { family: font.name.clone(), size_pt: font.size.max(1.0), weight: if font.bold { 700 } else { 400 }, italic: font.italic, ..TextStyle::default() };
        let mut fonts = self.fonts.lock().unwrap_or_else(|e| e.into_inner());
        let w = fonts.measure(&text, &style);
        if w > 0.0 {
            w
        } else {
            // No fonts (headless): about one digit's width per character.
            text.chars().count() as f64 * font.size.max(1.0) * 0.5
        }
    }

    fn cmd_row_height(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let r = if whole_cols(&t.range) { self.clipped(t.sheet, &t.range) } else { t.range };
        if r.rows() > 100_000 {
            return Err(bad(name, "select at most 100,000 rows"));
        }
        let auto = cmd.args.get("auto").and_then(Json::as_bool) == Some(true);
        let height = match cmd.args.get("height") {
            Some(Json::Null) | None => None,
            Some(j) => Some(j.as_f64().filter(|h| (0.0..=409.0).contains(h)).ok_or_else(|| bad(name, "\"height\" is from 0 to 409 points"))?),
        };
        let sheet = &self.sheets[t.sheet];
        let mut heights: BTreeMap<u32, Option<f64>> = BTreeMap::new();
        for row in r.r1..=r.r2 {
            let h = if auto {
                // The tallest font in the row (wrapped text by its lines).
                let mut best: f64 = 0.0;
                if let Some(rw) = sheet.rows.get(&row) {
                    for (&c, cell) in &rw.cells {
                        let xf = self.styles.xf(cell.style);
                        let size = self.styles.font(xf.font).size.max(1.0);
                        let mut lines = 1.0;
                        if xf.wrap {
                            let w = self.text_width(sheet, row, c);
                            let avail = (sheet.col_width(c) - 4.0).max(1.0);
                            lines = (w / avail).ceil().max(1.0) + self.formatted(cell).matches('\n').count() as f64;
                        }
                        best = best.max(lines * size * 15.0 / 11.0);
                    }
                }
                let h = ((best / 0.75).round() * 0.75).min(409.0);
                if best == 0.0 || (h - sheet.default_row_height).abs() < 0.01 {
                    None
                } else {
                    Some(h)
                }
            } else {
                height
            };
            heights.insert(row, h);
        }
        let rows: Vec<u32> = heights.keys().copied().collect();
        self.commit_rows(t.sheet, rows, &move |e| {
            let rn = e.attr("r").and_then(|v| v.parse::<u32>().ok()).map(|v| v - 1).unwrap_or(0);
            match heights.get(&rn).copied().flatten() {
                Some(h) => {
                    e.set_attr("ht", fmt_num(h));
                    e.set_attr("customHeight", "1");
                }
                None => e.attrs.retain(|(k, _)| k != "ht" && k != "customHeight"),
            }
        })
    }

    fn cmd_hide(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let what = cmd.args.get("what").and_then(Json::as_str).unwrap_or("rows");
        let hidden = cmd.args.get("hidden").and_then(Json::as_bool).unwrap_or(true);
        match what {
            "rows" => {
                let r = t.range;
                if r.rows() > 100_000 {
                    return Err(bad(name, "select at most 100,000 rows"));
                }
                let rows: Vec<u32> = (r.r1..=r.r2).collect();
                self.commit_rows(t.sheet, rows, &move |e| {
                    if hidden {
                        e.set_attr("hidden", "1");
                    } else {
                        e.attrs.retain(|(k, _)| k != "hidden");
                    }
                })
            }
            "cols" => {
                let r = t.range;
                let part = self.worksheet_part(t.sheet)?;
                self.commit_parts(vec![part.clone()], move |doc| {
                    doc.pkg
                        .edit_xml(&part, |root| {
                            set_cols(root, &[(r.c1, r.c2, 0)], &|col, _| {
                                if hidden {
                                    col.set_attr("hidden", "1");
                                } else {
                                    col.attrs.retain(|(k, _)| k != "hidden");
                                }
                            })
                        })
                        .map_err(|e| BusError::failed(e.0))?;
                    Ok(())
                })
            }
            _ => Err(bad(name, "\"what\" is rows or cols")),
        }
    }

    fn cmd_col_width(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let r = t.range;
        let auto = cmd.args.get("auto").and_then(Json::as_bool) == Some(true);
        let width = match cmd.args.get("width") {
            Some(Json::Null) | None => None,
            Some(j) => Some(j.as_f64().filter(|w| (0.0..=255.0).contains(w)).ok_or_else(|| bad(name, "\"width\" is from 0 to 255 characters"))?),
        };
        if !auto && width.is_none() && cmd.args.get("width").is_none() {
            return Err(bad(name, "give \"width\" (characters, or null for the default) or \"auto\": true"));
        }
        let sheet = &self.sheets[t.sheet];
        let mut spans: Vec<(u32, u32, u32)> = Vec::new();
        let mut widths: HashMap<u32, Option<f64>> = HashMap::new();
        let cols_hi = if auto { r.c2.min(sheet.used_range().map_or(r.c1, |u| u.c2.max(r.c1))) } else { r.c2 };
        for c in r.c1..=cols_hi {
            let w = if auto {
                let mut best: f64 = 0.0;
                let rows = self.clipped(t.sheet, &CellRange::new(r.r1, c, r.r2, c));
                for (&row, rw) in sheet.rows.range(rows.r1..=rows.r2) {
                    if rw.cells.contains_key(&c) && sheet.merge_at(row, c).is_none() {
                        best = best.max(self.text_width(sheet, row, c));
                    }
                }
                if best == 0.0 {
                    None
                } else {
                    let px = best / 0.75 + 5.0;
                    Some((((px / 7.0) * 256.0).trunc() / 256.0 * 100.0).round() / 100.0)
                }
            } else {
                width
            };
            widths.insert(c, w.map(|w| w.min(255.0)));
            spans.push((c, c, 0));
        }
        let part = self.worksheet_part(t.sheet)?;
        self.commit_parts(vec![part.clone()], move |doc| {
            doc.pkg
                .edit_xml(&part, |root| {
                    set_cols(root, &spans, &|col, _| {
                        let c = col.attr("min").and_then(|v| v.parse::<u32>().ok()).map(|v| v - 1).unwrap_or(0);
                        match widths.get(&c).copied().flatten() {
                            Some(w) => {
                                col.set_attr("width", fmt_num(w));
                                col.set_attr("customWidth", "1");
                            }
                            None => col.attrs.retain(|(k, _)| k != "width" && k != "customWidth"),
                        }
                    })
                })
                .map_err(|e| BusError::failed(e.0))?;
            Ok(())
        })
    }

    // --- sort and filter ---------------------------------------------------------------------

    /// The data region around a cell: grown while a row or column next to it has cells.
    pub(super) fn current_region(&self, si: usize, r: u32, c: u32) -> CellRange {
        let sheet = &self.sheets[si];
        let filled = |r: u32, c: u32| sheet.cell(r, c).is_some_and(|x| !x.value.is_empty() || x.formula.is_some());
        let mut g = CellRange::cell(r, c);
        loop {
            let mut n = g;
            let any_row = |row: u32, a: u32, b: u32| (a..=b).any(|col| filled(row, col));
            let any_col = |col: u32, a: u32, b: u32| (a..=b).any(|row| filled(row, col));
            let (lo_c, hi_c) = (g.c1.saturating_sub(1), (g.c2 + 1).min(MAX_COLS - 1));
            let (lo_r, hi_r) = (g.r1.saturating_sub(1), (g.r2 + 1).min(MAX_ROWS - 1));
            if g.r1 > 0 && any_row(g.r1 - 1, lo_c, hi_c) {
                n.r1 = g.r1 - 1;
            }
            if g.r2 + 1 < MAX_ROWS && any_row(g.r2 + 1, lo_c, hi_c) {
                n.r2 = g.r2 + 1;
            }
            if g.c1 > 0 && any_col(g.c1 - 1, lo_r, hi_r) {
                n.c1 = g.c1 - 1;
            }
            if g.c2 + 1 < MAX_COLS && any_col(g.c2 + 1, lo_r, hi_r) {
                n.c2 = g.c2 + 1;
            }
            if n == g {
                return g;
            }
            g = n;
        }
    }

    pub(super) fn sort_range(&self, c: &Sel) -> CellRange {
        if c.is_cell() {
            self.current_region(c.sheet, c.active.row, c.active.col)
        } else {
            self.clipped(c.sheet, &c.range)
        }
    }

    fn has_header(&self, si: usize, r: &CellRange) -> bool {
        if r.rows() < 2 {
            return false;
        }
        let sheet = &self.sheets[si];
        let first_text = (r.c1..=r.c2).all(|c| matches!(sheet.cell(r.r1, c).map(|x| &x.value), Some(CellValue::Text(_)) | None));
        let below_other = (r.c1..=r.c2).any(|c| matches!(sheet.cell(r.r1 + 1, c).map(|x| &x.value), Some(CellValue::Number(_)) | Some(CellValue::Bool(_))));
        first_text && below_other
    }

    fn cmd_sort(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let r = self.sort_range(&t);
        Self::check_size(name, &r)?;
        let header = cmd.args.get("header").and_then(Json::as_bool).unwrap_or_else(|| self.has_header(t.sheet, &r));
        let desc = cmd.args.get("descending").and_then(Json::as_bool) == Some(true) || cmd.args.get("order").and_then(Json::as_str) == Some("descending");
        let by = match cmd.args.get("by") {
            None | Some(Json::Null) => t.active.col.clamp(r.c1, r.c2),
            Some(Json::Str(s)) => col_index(s.trim().trim_start_matches('$')).ok_or_else(|| bad(name, format!("'{s}' is not a column letter")))?,
            Some(j) => j.as_usize().map(|i| i as u32).ok_or_else(|| bad(name, "\"by\" is a column letter like \"B\""))?,
        };
        if by < r.c1 || by > r.c2 {
            return Err(bad(name, format!("column {} is outside {}", col_name(by), r)));
        }
        let first = if header { r.r1 + 1 } else { r.r1 };
        if first > r.r2 {
            return Ok(Change::default());
        }
        let sheet = &self.sheets[t.sheet];
        let rank = |v: Option<&CellValue>| match v {
            Some(CellValue::Number(_)) => 0,
            Some(CellValue::Text(_)) => 1,
            Some(CellValue::Bool(_)) => 2,
            Some(CellValue::Error(_)) => 3,
            _ => 4,
        };
        let mut order: Vec<u32> = (first..=r.r2).collect();
        order.sort_by(|&a, &b| {
            let (va, vb) = (sheet.cell(a, by).map(|c| &c.value), sheet.cell(b, by).map(|c| &c.value));
            let (ra, rb) = (rank(va), rank(vb));
            if ra == 4 || rb == 4 {
                return ra.cmp(&rb);
            }
            let o = ra.cmp(&rb).then_with(|| match (va, vb) {
                (Some(CellValue::Number(x)), Some(CellValue::Number(y))) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
                (Some(CellValue::Text(x)), Some(CellValue::Text(y))) => x.to_lowercase().cmp(&y.to_lowercase()),
                (Some(CellValue::Bool(x)), Some(CellValue::Bool(y))) => x.cmp(y),
                _ => std::cmp::Ordering::Equal,
            });
            if desc {
                o.reverse()
            } else {
                o
            }
        });
        let mut writes = Vec::new();
        for (i, &src) in order.iter().enumerate() {
            let dest = first + i as u32;
            if dest == src {
                continue;
            }
            for col in r.c1..=r.c2 {
                let s = sheet.cell(src, col);
                let spec = s.map(|s| CellSpec { value: s.value.clone(), formula: s.formula.as_ref().map(|f| refs::copy_shift(f, dest as i64 - src as i64, 0)), style: s.style, sst: None });
                if spec.is_some() || sheet.cell(dest, col).is_some() {
                    writes.push(CellWrite { row: dest, col, spec });
                }
            }
        }
        if writes.is_empty() {
            return Ok(Change::default());
        }
        self.commit_cells(t.sheet, writes, Vec::new())
    }

    /// The AutoFilter range of a sheet, if it has one.
    pub(super) fn filter_range(&self, si: usize) -> Option<CellRange> {
        self.sheets.get(si)?.auto_filter
    }

    fn cmd_filter(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let si = t.sheet;
        let existing = self.filter_range(si);
        let values: Option<Vec<String>> = cmd.args.get("values").and_then(Json::as_arr).map(|a| a.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect());
        let column = match cmd.args.get("column") {
            None | Some(Json::Null) => None,
            Some(Json::Str(s)) => Some(col_index(s.trim()).ok_or_else(|| bad(name, format!("'{s}' is not a column letter")))?),
            Some(j) => Some(j.as_usize().ok_or_else(|| bad(name, "\"column\" is a column letter"))? as u32),
        };
        // The Filter button without values switches the AutoFilter off when it is on.
        if values.is_none() && existing.is_some() && cmd.args.get("range").is_none() {
            return self.remove_filter(si, true);
        }
        let range = match (existing, cmd.args.get("range").is_some() || !t.is_cell()) {
            (Some(e), false) => e,
            _ => {
                if t.is_cell() {
                    self.current_region(si, t.active.row, t.active.col)
                } else {
                    self.clipped(si, &t.range)
                }
            }
        };
        if range.rows() < 1 {
            return Err(BusError::failed("there is no data to filter here"));
        }
        let hide: Vec<(u32, bool)> = match (&values, column) {
            (Some(vals), Some(col)) => {
                if col < range.c1 || col > range.c2 {
                    return Err(bad(name, format!("column {} is outside the filter {}", col_name(col), range)));
                }
                let sheet = &self.sheets[si];
                ((range.r1 + 1)..=range.r2)
                    .map(|row| {
                        let shown = sheet.cell(row, col).map(|c| self.formatted(c)).unwrap_or_default();
                        (row, !vals.iter().any(|v| v.eq_ignore_ascii_case(&shown)))
                    })
                    .collect()
            }
            (Some(_), None) => return Err(bad(name, "\"values\" needs the \"column\" they are for")),
            _ => Vec::new(),
        };
        let part = self.worksheet_part(si)?;
        let wb = self.workbook_part.clone();
        let sheet_name = self.sheets[si].name.clone();
        let filter_col = column.map(|c| c - range.c1);
        self.commit_parts(vec![part.clone(), wb.clone()], move |doc| {
            doc.pkg
                .edit_xml(&part, |root| {
                    let prefix = root.prefix().map(str::to_string);
                    let q = |l: &str| edit::qualify(&prefix, l);
                    let af = ensure_child(root, "autoFilter", &SHEET_ORDER);
                    af.set_attr("ref", range.to_string());
                    if let (Some(col), Some(vals)) = (filter_col, &values) {
                        af.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "filterColumn" && e.attr("colId") == Some(&col.to_string())));
                        let mut fc = Element::new(q("filterColumn"));
                        fc.attrs.push(("colId".into(), col.to_string()));
                        let mut fs = Element::new(q("filters"));
                        for v in vals {
                            if v.is_empty() {
                                fs.attrs.push(("blank".into(), "1".into()));
                                continue;
                            }
                            let mut f = Element::new(q("filter"));
                            f.attrs.push(("val".into(), v.clone()));
                            fs.children.push(Node::Element(f));
                        }
                        fc.children.push(Node::Element(fs));
                        af.children.push(Node::Element(fc));
                    }
                    if !hide.is_empty() {
                        let rows: Vec<u32> = hide.iter().map(|h| h.0).collect();
                        let map: HashMap<u32, bool> = hide.iter().copied().collect();
                        let _ = edit::edit_rows(root, &rows, &|e| {
                            let rn = e.attr("r").and_then(|v| v.parse::<u32>().ok()).map(|v| v - 1).unwrap_or(0);
                            if map.get(&rn) == Some(&true) {
                                e.set_attr("hidden", "1");
                            } else {
                                e.attrs.retain(|(k, _)| k != "hidden");
                            }
                        });
                    }
                })
                .map_err(|e| BusError::failed(e.0))?;
            let value = format!("{}!{}", refs::quote_sheet(&sheet_name), abs_range(&range));
            doc.pkg.edit_xml(&wb, |root| set_defined_name(root, "_xlnm._FilterDatabase", Some(si), Some(&value), true)).map_err(|e| BusError::failed(e.0))?;
            Ok(())
        })
    }

    /// Takes the AutoFilter away (`remove`) or only its criteria, showing every row again.
    fn remove_filter(&mut self, si: usize, remove: bool) -> Result<Change, BusError> {
        let Some(range) = self.filter_range(si) else { return Ok(Change::default()) };
        let part = self.worksheet_part(si)?;
        let wb = self.workbook_part.clone();
        let hidden: Vec<u32> = ((range.r1 + 1)..=range.r2).filter(|r| self.sheets[si].rows.get(r).is_some_and(|x| x.hidden)).collect();
        let names = if remove { vec![part.clone(), wb.clone()] } else { vec![part.clone()] };
        self.commit_parts(names, move |doc| {
            doc.pkg
                .edit_xml(&part, |root| {
                    if remove {
                        root.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "autoFilter"));
                    } else if let Some(af) = root.child_mut("autoFilter") {
                        af.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "filterColumn" || e.local() == "sortState"));
                    }
                    if !hidden.is_empty() {
                        let _ = edit::edit_rows(root, &hidden, &|e| e.attrs.retain(|(k, _)| k != "hidden"));
                    }
                })
                .map_err(|e| BusError::failed(e.0))?;
            if remove {
                doc.pkg.edit_xml(&wb, |root| set_defined_name(root, "_xlnm._FilterDatabase", Some(si), None, true)).map_err(|e| BusError::failed(e.0))?;
            }
            Ok(())
        })
    }

    fn cmd_clear_filter(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let si = match cmd.args.get("sel").and_then(Selection::from_json).and_then(|s| self.sel_of(&s)) {
            Some(s) => s.sheet,
            None => self.sheet_index(&cmd.name, cmd.args.get("sheet"))?,
        };
        self.remove_filter(si, cmd.name == "ledger.remove_filter")
    }

    // --- find and replace --------------------------------------------------------------------

    fn cmd_find(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let t = self.target(cmd)?;
        if let Some(s) = self.landing(cmd) {
            if let Some(p) = self.place(s.focus.json()) {
                if (p.row, p.col) != (t.active.row, t.active.col) || p.sheet != t.sheet {
                    self.ensure_visible(p.sheet, p.row, p.col);
                }
            }
        }
        Ok(Change::default())
    }

    fn cmd_replace(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let query = cmd.args.get("query").and_then(Json::as_str).unwrap_or("");
        if query.is_empty() {
            return Err(bad(name, "\"query\" is the text to find"));
        }
        let with = cmd.args.get("with").and_then(Json::as_str).unwrap_or("");
        let all = cmd.args.get("all").and_then(Json::as_bool) == Some(true);
        let opts = FindOpts::of(&cmd.args);
        let sheets: Vec<usize> = if all && cmd.args.get("workbook").and_then(Json::as_bool) == Some(true) { (0..self.sheets.len()).collect() } else { vec![t.sheet] };
        let mut total = Change::default();
        for si in sheets {
            if self.sheets[si].part.is_none() {
                continue;
            }
            let sheet = &self.sheets[si];
            let mut list = Vec::new();
            let mut visit = |r: u32, c: u32, cell: &Cell| {
                let text = self.find_text(cell, opts.formulas);
                if !opts.matches(&text, query) {
                    return;
                }
                let input = input_text(&cell.value, cell.formula.as_deref());
                let source = if cell.formula.is_some() && !opts.formulas { return } else { input };
                let new = opts.replace(&source, query, with);
                if new != source {
                    list.push((r, c, parse_input(&new, self.date1904, self.is_text_cell(sheet, r, c))));
                }
            };
            if all {
                for (&r, row) in &sheet.rows {
                    for (&c, cell) in &row.cells {
                        visit(r, c, cell);
                    }
                }
            } else if let Some(cell) = sheet.cell(t.active.row, t.active.col) {
                visit(t.active.row, t.active.col, cell);
            }
            if list.is_empty() {
                continue;
            }
            let ch = self.put_entries(si, list)?;
            total.undo.extend(ch.undo);
            for p in ch.pages_touched {
                if !total.pages_touched.contains(&p) {
                    total.pages_touched.push(p);
                }
            }
        }
        Ok(total)
    }

    // --- freeze panes ------------------------------------------------------------------------

    fn cmd_freeze(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let t = self.target(cmd)?;
        let (rows, cols) = if cmd.name == "ledger.unfreeze" {
            (0, 0)
        } else {
            match (cmd.args.get("rows").and_then(Json::as_usize), cmd.args.get("cols").and_then(Json::as_usize)) {
                (None, None) => {
                    // At the active cell: the rows above it and the columns left of it (the
                    // top-left cell of the view counts as the first).
                    (t.active.row, t.active.col)
                }
                (r, c) => (r.unwrap_or(0).min(MAX_ROWS as usize - 1) as u32, c.unwrap_or(0).min(MAX_COLS as usize - 1) as u32),
            }
        };
        let _ = name;
        let si = t.sheet;
        let part = self.worksheet_part(si)?;
        let active = (t.active.row, t.active.col);
        let ch = self.commit_parts(vec![part.clone()], move |doc| {
            doc.pkg.edit_xml(&part, |root| set_pane(root, rows, cols, active)).map_err(|e| BusError::failed(e.0))?;
            Ok(())
        })?;
        let sheet = &mut self.sheets[si];
        sheet.view.row = sheet.view.row.max(rows);
        sheet.view.col = sheet.view.col.max(cols);
        self.refresh(si);
        Ok(ch)
    }

    // --- dispatch ----------------------------------------------------------------------------

    /// Runs one of the editing commands; `None` for a name that is not one of them.
    pub(super) fn apply_edit(&mut self, cmd: &Command) -> Option<Result<Change, BusError>> {
        Some(match cmd.name.as_str() {
            "ledger.move" => self.cmd_move(cmd),
            "ledger.enter" | "ledger.tab" => self.cmd_enter(cmd),
            "ledger.select_all" => self.target(cmd).map(|_| Change::default()),
            "ledger.insert_text" | "ledger.backspace" => self.cmd_type(cmd),
            "ledger.set_cells" => self.cmd_set_cells(cmd),
            "ledger.clear" => self.cmd_clear(cmd),
            "ledger.fill" => self.cmd_fill(cmd),
            "ledger.autofill" => self.cmd_autofill(cmd),
            "ledger.cut" => self.cmd_cut(cmd),
            "ledger.paste" => self.cmd_paste(cmd),
            "ledger.move_range" => self.cmd_move_range(cmd),
            "ledger.autosum" => self.cmd_autosum(cmd),
            "ledger.format" => self.cmd_format(cmd),
            "ledger.toggle" => self.cmd_toggle(cmd),
            "ledger.borders" => self.cmd_borders(cmd),
            "ledger.format_painter" => self.cmd_painter(cmd),
            "ledger.copy_format" => self.cmd_copy_format(cmd),
            "ledger.merge" => self.cmd_merge(cmd),
            "ledger.unmerge" => self.cmd_unmerge(cmd),
            "ledger.insert" | "ledger.delete" => self.cmd_insert_delete(cmd),
            "ledger.row_height" => self.cmd_row_height(cmd),
            "ledger.col_width" => self.cmd_col_width(cmd),
            "ledger.hide" => self.cmd_hide(cmd),
            "ledger.sort" => self.cmd_sort(cmd),
            "ledger.filter" => self.cmd_filter(cmd),
            "ledger.clear_filter" | "ledger.remove_filter" => self.cmd_clear_filter(cmd),
            "ledger.find" => self.cmd_find(cmd),
            "ledger.replace" => self.cmd_replace(cmd),
            "ledger.freeze" | "ledger.unfreeze" => self.cmd_freeze(cmd),
            "ledger.add_sheet" => self.cmd_add_sheet(cmd),
            "ledger.rename_sheet" => self.cmd_rename_sheet(cmd),
            "ledger.delete_sheet" => self.cmd_delete_sheet(cmd),
            "ledger.move_sheet" => self.cmd_move_sheet(cmd),
            "ledger.import_csv" => self.cmd_import_csv(cmd),
            _ => return None,
        })
    }
}

fn range_text(r: &CellRange) -> String {
    r.to_string()
}

/// `$A$1:$C$9`.
pub fn abs_range(r: &CellRange) -> String {
    format!("${}${}:${}${}", col_name(r.c1), r.r1 + 1, col_name(r.c2), r.r2 + 1)
}

/// A number as an attribute (no trailing `.0`).
pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{}", (v * 10_000.0).round() / 10_000.0)
    }
}

/// Sets (or with `value: None` removes) a defined name in a workbook tree.
pub fn set_defined_name(root: &mut Element, name: &str, local: Option<usize>, value: Option<&str>, hidden: bool) {
    const WB_ORDER: [&str; 17] = [
        "fileVersion", "fileSharing", "workbookPr", "workbookProtection", "bookViews", "sheets", "functionGroups", "externalReferences", "definedNames", "calcPr", "oleSize", "customWorkbookViews", "pivotCaches", "smartTagPr", "smartTagTypes", "webPublishing", "fileRecoveryPr",
    ];
    let same = |e: &Element| e.local() == "definedName" && e.attr("name").is_some_and(|n| n.eq_ignore_ascii_case(name)) && e.attr("localSheetId").and_then(|v| v.parse::<usize>().ok()) == local;
    match value {
        None => {
            if let Some(dn) = root.child_mut("definedNames") {
                dn.children.retain(|n| !matches!(n, Node::Element(e) if same(e)));
            }
            if root.child("definedNames").is_some_and(|d| d.elements().next().is_none()) {
                root.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "definedNames"));
            }
        }
        Some(v) => {
            let prefix = root.prefix().map(str::to_string);
            let dn = ensure_child(root, "definedNames", &WB_ORDER);
            if let Some(e) = dn.children.iter_mut().find_map(|n| match n {
                Node::Element(e) if same(e) => Some(e),
                _ => None,
            }) {
                if e.text() != v {
                    e.children = vec![Node::Text(v.to_string())];
                }
                return;
            }
            let mut e = Element::new(edit::qualify(&prefix, "definedName"));
            e.attrs.push(("name".into(), name.to_string()));
            if let Some(l) = local {
                e.attrs.push(("localSheetId".into(), l.to_string()));
            }
            if hidden {
                e.attrs.push(("hidden".into(), "1".into()));
            }
            e.children.push(Node::Text(v.to_string()));
            dn.children.push(Node::Element(e));
        }
    }
}

/// Changes columns `spans` (0-based, inclusive; each with a value for `f`) in `<cols>`: runs that
/// straddle a span are split so only the span's columns change; columns without a run get one.
/// Runs left with nothing to say are dropped.
pub fn set_cols(root: &mut Element, spans: &[(u32, u32, u32)], f: &dyn Fn(&mut Element, u32)) {
    let prefix = root.prefix().map(str::to_string);
    let cols = ensure_child(root, "cols", &SHEET_ORDER);
    // Every run as (min, max, element), 0-based.
    let mut runs: Vec<(u32, u32, Element)> = Vec::new();
    let mut others = Vec::new();
    for n in std::mem::take(&mut cols.children) {
        match n {
            Node::Element(e) if e.local() == "col" => {
                let min = e.attr("min").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).max(1) - 1;
                let max = e.attr("max").and_then(|v| v.parse::<u32>().ok()).unwrap_or(min + 1).max(1) - 1;
                runs.push((min, max.max(min), e));
            }
            Node::Element(e) => others.push(Node::Element(e)),
            _ => {}
        }
    }
    for &(a, b, v) in spans {
        let mut next = Vec::new();
        let mut covered: Vec<(u32, u32)> = Vec::new();
        for (min, max, e) in runs {
            if max < a || min > b {
                next.push((min, max, e));
                continue;
            }
            if min < a {
                next.push((min, a - 1, e.clone()));
            }
            let (lo, hi) = (min.max(a), max.min(b));
            let mut mid = e.clone();
            f(&mut mid, v);
            next.push((lo, hi, mid));
            covered.push((lo, hi));
            if max > b {
                next.push((b + 1, max, e));
            }
        }
        // Gaps inside the span get new runs.
        covered.sort_unstable();
        let mut at = a;
        let mut gaps = Vec::new();
        for (lo, hi) in covered {
            if lo > at {
                gaps.push((at, lo - 1));
            }
            at = at.max(hi + 1);
        }
        if at <= b {
            gaps.push((at, b));
        }
        for (lo, hi) in gaps {
            let mut e = Element::new(edit::qualify(&prefix, "col"));
            e.attrs.push(("min".into(), (lo + 1).to_string()));
            e.attrs.push(("max".into(), (hi + 1).to_string()));
            f(&mut e, v);
            next.push((lo, hi, e));
        }
        runs = next;
    }
    runs.sort_by_key(|r| r.0);
    // Neighbouring runs that say the same are one run.
    let same = |a: &Element, b: &Element| {
        let strip = |e: &Element| e.attrs.iter().filter(|(k, _)| k != "min" && k != "max").cloned().collect::<Vec<_>>();
        strip(a) == strip(b) && a.children == b.children
    };
    let mut joined: Vec<(u32, u32, Element)> = Vec::new();
    for run in runs {
        match joined.last_mut() {
            Some(last) if last.1 + 1 == run.0 && same(&last.2, &run.2) => last.1 = run.1,
            _ => joined.push(run),
        }
    }
    let runs = joined;
    let mut out = Vec::new();
    for (min, max, mut e) in runs {
        if e.attr("min") != Some(&(min + 1).to_string()) {
            e.set_attr("min", (min + 1).to_string());
        }
        if e.attr("max") != Some(&(max + 1).to_string()) {
            e.set_attr("max", (max + 1).to_string());
        }
        let says = e.attrs.iter().any(|(k, _)| !matches!(k.as_str(), "min" | "max" | "customWidth" | "bestFit")) || !e.children.is_empty();
        if says {
            out.push(Node::Element(e));
        }
    }
    out.extend(others);
    cols.children = out;
    if cols.children.is_empty() {
        root.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "cols"));
    }
}

/// Freezes the first `rows` rows and `cols` columns of the first sheet view (0, 0 unfreezes).
pub fn set_pane(root: &mut Element, rows: u32, cols: u32, active: (u32, u32)) {
    let prefix = root.prefix().map(str::to_string);
    let q = |l: &str| edit::qualify(&prefix, l);
    let views = ensure_child(root, "sheetViews", &SHEET_ORDER);
    if views.children_named("sheetView").next().is_none() {
        let mut v = Element::new(q("sheetView"));
        v.attrs.push(("workbookViewId".into(), "0".into()));
        views.children.push(Node::Element(v));
    }
    let vi = views.children.iter().position(|n| matches!(n, Node::Element(e) if e.local() == "sheetView")).expect("a view");
    let view = child_at(views, vi);
    view.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "pane" || (e.local() == "selection" && e.attr("pane").is_some())));
    let has_sel = view.children.iter().any(|n| matches!(n, Node::Element(e) if e.local() == "selection"));
    if rows == 0 && cols == 0 {
        if !has_sel {
            let mut s = Element::new(q("selection"));
            s.attrs.push(("activeCell".into(), cell_name(active.0, active.1)));
            s.attrs.push(("sqref".into(), cell_name(active.0, active.1)));
            view.children.push(Node::Element(s));
        }
        return;
    }
    let pane_name = match (rows > 0, cols > 0) {
        (true, true) => "bottomRight",
        (true, false) => "bottomLeft",
        _ => "topRight",
    };
    let mut p = Element::new(q("pane"));
    if cols > 0 {
        p.attrs.push(("xSplit".into(), cols.to_string()));
    }
    if rows > 0 {
        p.attrs.push(("ySplit".into(), rows.to_string()));
    }
    p.attrs.push(("topLeftCell".into(), cell_name(rows, cols)));
    p.attrs.push(("activePane".into(), pane_name.into()));
    p.attrs.push(("state".into(), "frozen".into()));
    // The pane comes first, then the selections.
    let at = view.children.iter().position(|n| matches!(n, Node::Element(_))).unwrap_or(view.children.len());
    view.children.insert(at, Node::Element(p));
    let (ar, ac) = (active.0.max(rows), active.1.max(cols));
    let mut s = Element::new(q("selection"));
    s.attrs.push(("pane".into(), pane_name.into()));
    s.attrs.push(("activeCell".into(), cell_name(ar, ac)));
    s.attrs.push(("sqref".into(), cell_name(ar, ac)));
    let after = view.children.iter().rposition(|n| matches!(n, Node::Element(e) if e.local() == "selection" || e.local() == "pane")).map(|i| i + 1).unwrap_or(view.children.len());
    view.children.insert(after, Node::Element(s));
}
