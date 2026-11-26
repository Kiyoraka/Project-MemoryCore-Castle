//! Ledger by hand: the editing ground's seam for a workbook. A place ([`Pos`]) is a cell,
//! `{"sheet": 0, "row": 6, "col": 1}` (0-based), with `"edit": "typed text"` while the member is
//! typing into it; a selection is the rectangle between its anchor and its focus (the focus is
//! the active cell). The page is the sheet's grid as [`super::grid`] draws it, so hit tests and
//! boxes use the same plan of rows and columns (frozen panes, hidden rows, the scrolled view).
//!
//! The caret is the active cell's left edge (after the typed text while typing); the selection's
//! boxes cover the range, split at the frozen panes.

use super::cellref::{cell_name, CellRange, MAX_COLS, MAX_ROWS};
use super::entry::input_text;
use super::grid::{row_header_width, Axis, HEADER_H};
use super::model::{CellValue, Sheet};
use super::numfmt::NumberFormat;
use super::styles::{HAlign, VAlign};
use super::LedgerDoc;
use crate::ground::bus::json::Json;
use crate::ground::bus::Command;
use crate::ground::display::Rect;
use crate::ground::edit::{CaretRect, Clip, Editing, Pos, Selection};
use crate::ground::text::{Measurer, TextStyle};

/// The most cells a copy carries in its rich form.
pub const COPY_LIMIT: u64 = 200_000;

/// A cell place.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub sheet: usize,
    pub row: u32,
    pub col: u32,
    /// The text typed so far, while typing into the cell.
    pub edit: Option<String>,
}

impl Place {
    pub fn new(sheet: usize, row: u32, col: u32) -> Place {
        Place { sheet, row, col, edit: None }
    }

    pub fn pos(&self) -> Pos {
        let mut j = Json::obj().with("sheet", self.sheet).with("row", self.row as u64).with("col", self.col as u64);
        if let Some(e) = &self.edit {
            j.set("edit", e.as_str());
        }
        Pos(j)
    }

    /// Reads `{"sheet","row","col"}` (or `{"sheet","cell":"B7"}`; the sheet may be a name).
    pub fn of(doc: &LedgerDoc, v: &Json) -> Option<Place> {
        let sheet = match v.get("sheet") {
            None | Some(Json::Null) => doc.active.min(doc.sheets.len().saturating_sub(1)),
            Some(Json::Str(s)) => doc.sheet_by_name(s)?,
            Some(j) => j.as_usize().filter(|&i| i < doc.sheets.len())?,
        };
        let (row, col) = match (v.get("row").and_then(Json::as_usize), v.get("col").and_then(Json::as_usize)) {
            (Some(r), Some(c)) => (r.min(MAX_ROWS as usize - 1) as u32, c.min(MAX_COLS as usize - 1) as u32),
            _ => super::parse_cell(v.get("cell")?.as_str()?)?,
        };
        Some(Place { sheet, row, col, edit: v.get("edit").and_then(Json::as_str).map(str::to_string) })
    }
}

/// A selection read: its sheet, the range (grown over merged cells), the anchor and the active cell.
#[derive(Debug, Clone)]
pub struct Sel {
    pub sheet: usize,
    pub range: CellRange,
    pub anchor: (u32, u32),
    pub active: Place,
}

impl Sel {
    pub fn selection(&self) -> Selection {
        Selection::new(Place::new(self.sheet, self.anchor.0, self.anchor.1).pos(), self.active.pos())
    }

    pub fn is_cell(&self) -> bool {
        self.range.is_single()
    }
}

/// A range grown until no merged range sticks out of it.
pub fn grow_over_merges(sheet: &Sheet, mut r: CellRange) -> CellRange {
    loop {
        let mut grown = r;
        for m in &sheet.merges {
            if m.intersects(&grown) {
                grown = grown.union(m);
            }
        }
        if grown == r {
            return r;
        }
        r = grown;
    }
}

/// The rows and columns the page shows: the same plan the grid draws with.
pub struct Geo {
    pub rows: Axis,
    pub cols: Axis,
    pub header_w: f64,
}

pub fn geo(sheet: &Sheet) -> Geo {
    let view = sheet.view;
    let (w, h) = (view.width.max(1.0), view.height.max(1.0));
    let (fr, fc) = sheet.frozen();
    let row_size = |r: u32| sheet.row_height(r);
    let col_size = |c: u32| sheet.col_width(c);
    let rows = Axis::plan(&row_size, HEADER_H, h, fr, view.row, MAX_ROWS);
    let last_row = rows.items.last().map(|t| t.0).unwrap_or(0);
    let header_w = row_header_width(last_row);
    let cols = Axis::plan(&col_size, header_w, w, fc, view.col, MAX_COLS);
    Geo { rows, cols, header_w }
}

impl Geo {
    /// The box of a range on the page (may lie outside the view).
    pub fn rect(&self, sheet: &Sheet, r: &CellRange) -> Rect {
        let row_size = |i: u32| sheet.row_height(i);
        let col_size = |i: u32| sheet.col_width(i);
        let x1 = self.cols.pos(&col_size, r.c1);
        let x2 = self.cols.pos(&col_size, r.c2) + col_size(r.c2);
        let y1 = self.rows.pos(&row_size, r.r1);
        let y2 = self.rows.pos(&row_size, r.r2) + row_size(r.r2);
        Rect::from_ltrb(x1, y1, x2.max(x1), y2.max(y1))
    }

    /// The boxes that paint a range, one per pane it shows in, clipped to that pane.
    pub fn rects(&self, sheet: &Sheet, r: &CellRange) -> Vec<Rect> {
        let mut out = Vec::new();
        for rf in [true, false] {
            for cf in [true, false] {
                let rspan = span(&self.rows, rf, r.r1, r.r2);
                let cspan = span(&self.cols, cf, r.c1, r.c2);
                let (Some((a, b)), Some((c, d))) = (rspan, cspan) else { continue };
                let part = CellRange::new(a, c, b, d);
                let rect = self.rect(sheet, &part);
                let (y1, y2) = extent(&self.rows, rf);
                let (x1, x2) = extent(&self.cols, cf);
                if let Some(clip) = rect.intersection(&Rect::from_ltrb(x1, y1, x2, y2)) {
                    if clip.w > 0.0 && clip.h > 0.0 {
                        out.push(clip);
                    }
                }
            }
        }
        out
    }

    /// The cell under a page point; header bands answer the first cell shown in that row or column.
    pub fn hit(&self, x: f64, y: f64) -> Option<(u32, u32)> {
        let find = |axis: &Axis, p: f64| axis.items.iter().find(|t| p >= t.1 && p < t.1 + t.2).map(|t| t.0);
        let first_row = self.rows.items.iter().find(|t| t.0 >= self.rows.top).or(self.rows.items.first()).map(|t| t.0)?;
        let first_col = self.cols.items.iter().find(|t| t.0 >= self.cols.top).or(self.cols.items.first()).map(|t| t.0)?;
        let r = if y < HEADER_H { Some(first_row) } else { find(&self.rows, y) };
        let c = if x < self.header_w { Some(first_col) } else { find(&self.cols, x) };
        Some((r?, c?))
    }
}

fn span(axis: &Axis, frozen: bool, lo: u32, hi: u32) -> Option<(u32, u32)> {
    if frozen {
        if axis.frozen_n == 0 || lo >= axis.frozen_n {
            return None;
        }
        Some((lo, hi.min(axis.frozen_n - 1)))
    } else {
        let a = lo.max(axis.top);
        (a <= hi).then_some((a, hi))
    }
}

fn extent(axis: &Axis, frozen: bool) -> (f64, f64) {
    if frozen {
        (axis.start, axis.frozen_end)
    } else {
        (axis.frozen_end, axis.end)
    }
}

/// The number format family a code belongs to (for the ribbon's state).
pub fn format_kind(code: &str) -> &'static str {
    let c = code.trim();
    if c.eq_ignore_ascii_case("general") || c.is_empty() {
        return "general";
    }
    if c == "@" {
        return "text";
    }
    if super::numfmt::is_date_format(c) {
        let l = c.to_ascii_lowercase();
        let has_date = l.contains('y') || l.contains('d') || (l.contains('m') && !l.contains('h') && !l.contains('s'));
        return if has_date { "date" } else { "time" };
    }
    if c.contains('%') {
        return "percent";
    }
    if c.contains("E+") || c.contains("E-") || c.contains("e+") {
        return "scientific";
    }
    if c.contains('?') && c.contains('/') {
        return "fraction";
    }
    if c.contains('*') && (c.contains('$') || c.contains('€') || c.contains('£')) {
        return "accounting";
    }
    if c.contains('$') || c.contains('€') || c.contains('£') || c.contains('¥') {
        return "currency";
    }
    "number"
}

/// Tab-separated text of rows of fields, quoted where a field holds a tab, a newline or a quote.
pub fn tsv(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        for (j, f) in row.iter().enumerate() {
            if j > 0 {
                out.push('\t');
            }
            if f.contains(['\t', '\n', '\r', '"']) {
                out.push('"');
                out.push_str(&f.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(f);
            }
        }
    }
    out
}

impl LedgerDoc {
    /// The place `v` names, if it is one.
    pub(super) fn place(&self, v: &Json) -> Option<Place> {
        Place::of(self, v)
    }

    /// A selection as the commands read it.
    pub(super) fn sel_of(&self, s: &Selection) -> Option<Sel> {
        let a = self.place(s.anchor.json())?;
        let f = self.place(s.focus.json())?;
        let sheet = f.sheet;
        let anchor = if a.sheet == sheet { (a.row, a.col) } else { (f.row, f.col) };
        let base = CellRange::new(anchor.0, anchor.1, f.row, f.col);
        let range = match self.sheets.get(sheet) {
            Some(sh) => grow_over_merges(sh, base),
            None => base,
        };
        Some(Sel { sheet, range, anchor, active: f })
    }

    /// The text style a cell draws with.
    fn cell_style(&self, sheet: &Sheet, r: u32, c: u32) -> TextStyle {
        let xf = self.styles.xf(sheet.effective_style(r, c));
        let font = self.styles.font(xf.font);
        TextStyle {
            family: font.name.clone(),
            size_pt: if font.size > 0.0 { font.size } else { 11.0 },
            weight: if font.bold { 700 } else { 400 },
            italic: font.italic,
            ..TextStyle::default()
        }
    }

    /// The selection a command lands on (worked out from its args, after it was applied).
    pub(super) fn landing(&self, cmd: &Command) -> Option<Selection> {
        let a = &cmd.args;
        let sel = a.get("sel").and_then(Selection::from_json);
        let cur = sel.as_ref().and_then(|s| self.sel_of(s));
        let caret = |p: Place| Some(Selection::caret(p.pos()));
        match cmd.name.as_str() {
            "ledger.move" => {
                let s = sel?;
                let motion = a.get("motion").and_then(Json::as_str)?;
                let to = self.step_place(&self.place(s.focus.json())?, motion)?;
                if a.get("extend").and_then(Json::as_bool) == Some(true) {
                    Some(Selection::new(s.anchor.clone(), to.pos()))
                } else {
                    caret(to)
                }
            }
            "ledger.enter" | "ledger.tab" => {
                let c = cur?;
                if cmd.name == "ledger.enter" && a.get("ctrl").and_then(Json::as_bool) == Some(true) {
                    let mut s = c.selection();
                    s.focus = Place::new(c.sheet, c.active.row, c.active.col).pos();
                    return Some(s);
                }
                let motion = match (cmd.name.as_str(), a.get("shift").and_then(Json::as_bool) == Some(true) || a.get("back").and_then(Json::as_bool) == Some(true)) {
                    ("ledger.enter", false) => "down",
                    ("ledger.enter", true) => "up",
                    (_, false) => "right",
                    (_, true) => "left",
                };
                let from = Place::new(c.sheet, c.active.row, c.active.col);
                caret(self.step_place(&from, motion).unwrap_or(from))
            }
            "ledger.select_all" => {
                let c = cur?;
                let sheet = self.sheets.get(c.sheet)?;
                let used = sheet.used_range();
                let r = match used {
                    Some(u) if !u.contains(c.active.row, c.active.col) || c.range != u => u,
                    _ => CellRange::new(0, 0, MAX_ROWS - 1, MAX_COLS - 1),
                };
                Some(Selection::new(Place::new(c.sheet, r.r1, r.c1).pos(), Place::new(c.sheet, r.r2, r.c2).pos()))
            }
            "ledger.insert_text" | "ledger.backspace" => {
                let c = cur?;
                let draft = self.draft_after(cmd, &c);
                let mut p = Place::new(c.sheet, c.active.row, c.active.col);
                p.edit = Some(draft);
                let mut s = c.selection();
                s.focus = p.pos();
                if c.is_cell() {
                    s.anchor = s.focus.clone();
                }
                Some(s)
            }
            "ledger.find" | "ledger.replace" => {
                let c = cur?;
                if cmd.name == "ledger.replace" && a.get("all").and_then(Json::as_bool) == Some(true) {
                    return caret(Place::new(c.sheet, c.active.row, c.active.col));
                }
                let next = a.get("next").and_then(Json::as_bool) != Some(false);
                let query = a.get("query").and_then(Json::as_str)?;
                let (si, r, col) = self.find_from(&c, query, next, cmd.name == "ledger.find", &a)?;
                caret(Place::new(si, r, col))
            }
            "ledger.paste" | "ledger.fill" | "ledger.autosum" | "ledger.sort" | "ledger.autofill" => {
                let c = cur?;
                let r = self.landed_range(cmd, &c)?;
                Some(Selection::new(Place::new(c.sheet, r.r1, r.c1).pos(), Place::new(c.sheet, r.r2, r.c2).pos()))
            }
            "ledger.add_sheet" => {
                let i = match a.get("at").and_then(Json::as_usize) {
                    Some(i) => i.min(self.sheets.len().saturating_sub(1)),
                    None => self.sheets.len().saturating_sub(1),
                };
                caret(Place::new(i, 0, 0))
            }
            "ledger.clear" | "ledger.format" | "ledger.toggle" | "ledger.borders" | "ledger.merge" | "ledger.unmerge" | "ledger.copy_format" | "ledger.format_painter" => {
                let c = cur?;
                let mut s = c.selection();
                if let Some(e) = &c.active.edit {
                    let mut p = Place::new(c.sheet, c.active.row, c.active.col);
                    p.edit = Some(e.clone());
                    s.focus = p.pos();
                }
                Some(s)
            }
            "ledger.insert" | "ledger.delete" | "ledger.freeze" | "ledger.row_height" | "ledger.col_width" | "ledger.hide" => {
                let c = cur?;
                Some(Selection::new(Place::new(c.sheet, c.anchor.0, c.anchor.1).pos(), Place::new(c.sheet, c.active.row, c.active.col).pos()))
            }
            _ => None,
        }
    }

    /// The draft text after typing (`insert_text`) or a Backspace.
    pub(super) fn draft_after(&self, cmd: &Command, c: &Sel) -> String {
        let old = c.active.edit.clone();
        if cmd.name == "ledger.backspace" {
            let mut d = old.unwrap_or_default();
            d.pop();
            return d;
        }
        let text = cmd.args.get("text").and_then(Json::as_str).unwrap_or("");
        format!("{}{}", old.unwrap_or_default(), text)
    }

    /// One cell move from a place; skips hidden rows and columns and steps over merged areas.
    pub(super) fn step_place(&self, p: &Place, motion: &str) -> Option<Place> {
        let sheet = self.sheets.get(p.sheet)?;
        let (r, c) = (p.row, p.col);
        let merge = sheet.merge_at(r, c).copied().unwrap_or(CellRange::cell(r, c));
        let vis_r = |r: u32| sheet.row_height(r) > 0.0;
        let vis_c = |c: u32| sheet.col_width(c) > 0.0;
        let next_r = |from: u32, down: bool| -> u32 {
            let mut x = from;
            loop {
                let n = if down { x.checked_add(1).filter(|&v| v < MAX_ROWS) } else { x.checked_sub(1) };
                match n {
                    Some(v) => {
                        x = v;
                        if vis_r(x) {
                            return x;
                        }
                    }
                    None => return from,
                }
            }
        };
        let next_c = |from: u32, right: bool| -> u32 {
            let mut x = from;
            loop {
                let n = if right { x.checked_add(1).filter(|&v| v < MAX_COLS) } else { x.checked_sub(1) };
                match n {
                    Some(v) => {
                        x = v;
                        if vis_c(x) {
                            return x;
                        }
                    }
                    None => return from,
                }
            }
        };
        let filled = |r: u32, c: u32| sheet.cell(r, c).is_some_and(|x| !x.value.is_empty() || x.formula.is_some());
        let (nr, nc) = match motion {
            "left" => (r, next_c(merge.c1, false)),
            "right" => (r, next_c(merge.c2, true)),
            "up" => (next_r(merge.r1, false), c),
            "down" => (next_r(merge.r2, true), c),
            "word_left" | "word_right" => {
                // To the edge of the data, as Ctrl+arrow does.
                let right = motion == "word_right";
                let step = |x: u32| -> Option<u32> {
                    if right {
                        x.checked_add(1).filter(|&v| v < MAX_COLS)
                    } else {
                        x.checked_sub(1)
                    }
                };
                let mut x = c;
                match step(x) {
                    None => (r, c),
                    Some(n) if filled(r, x) && filled(r, n) => {
                        x = n;
                        while let Some(m) = step(x) {
                            if !filled(r, m) {
                                break;
                            }
                            x = m;
                        }
                        (r, x)
                    }
                    Some(n) => {
                        x = n;
                        let row = sheet.rows.get(&r);
                        let found = row.and_then(|row| {
                            if right {
                                row.cells.range(x..).find(|(_, cl)| !cl.value.is_empty() || cl.formula.is_some()).map(|(k, _)| *k)
                            } else {
                                row.cells.range(..=x).rev().find(|(_, cl)| !cl.value.is_empty() || cl.formula.is_some()).map(|(k, _)| *k)
                            }
                        });
                        (r, found.unwrap_or(if right { MAX_COLS - 1 } else { 0 }))
                    }
                }
            }
            "line_start" => (r, sheet.frozen().1.min(c)),
            "line_end" => (r, sheet.rows.get(&r).and_then(|row| row.cells.keys().next_back().copied()).unwrap_or(c)),
            "doc_start" => {
                let (fr, fc) = sheet.frozen();
                (fr.min(r), fc.min(c))
            }
            "doc_end" => match sheet.used_range() {
                Some(u) => (u.r2, u.c2),
                None => (0, 0),
            },
            _ => return None,
        };
        // Landing inside a merged area lands on its top-left cell.
        let (nr, nc) = match sheet.merge_at(nr, nc) {
            Some(m) if !(m.contains(r, c)) => (m.r1, m.c1),
            _ => (nr, nc),
        };
        Some(Place::new(p.sheet, nr, nc))
    }

    /// The cell a find goes to from the selection: (sheet, row, col).
    pub(super) fn find_from(&self, c: &Sel, query: &str, next: bool, skip_current: bool, args: &Json) -> Option<(usize, u32, u32)> {
        if query.is_empty() {
            return None;
        }
        let sheet = self.sheets.get(c.sheet)?;
        let opts = super::ops::FindOpts::of(args);
        let mut hits: Vec<(u32, u32)> = Vec::new();
        for (&r, row) in &sheet.rows {
            for (&col, cell) in &row.cells {
                if opts.matches(&self.find_text(cell, opts.formulas), query) {
                    hits.push((r, col));
                }
            }
        }
        if hits.is_empty() {
            return None;
        }
        let here = (c.active.row, c.active.col);
        let pick = if next {
            hits.iter().find(|&&h| if skip_current { h > here } else { h >= here }).or(hits.first())
        } else {
            hits.iter().rev().find(|&&h| h < here).or(hits.last())
        };
        pick.map(|&(r, col)| (c.sheet, r, col))
    }

    /// What a find looks in: the formula (when asked and there is one) or the shown text.
    pub(super) fn find_text(&self, cell: &super::model::Cell, formulas: bool) -> String {
        match (&cell.formula, formulas) {
            (Some(f), true) => format!("={f}"),
            _ => self.formatted(cell),
        }
    }

    /// The editing state of a selection (the ribbon and a formula bar read it).
    pub(super) fn edit_state(&self, s: &Selection) -> Json {
        let Some(c) = self.sel_of(s) else { return Json::obj() };
        let Some(sheet) = self.sheets.get(c.sheet) else { return Json::obj() };
        let (r, col) = (c.active.row, c.active.col);
        let style = sheet.effective_style(r, col);
        let xf = self.styles.xf(style);
        let font = self.styles.font(xf.font);
        let code = self.styles.format_code(xf.num_fmt).to_string();
        let cell = sheet.cell(r, col);
        let input = match &c.active.edit {
            Some(e) => e.clone(),
            None => cell.map(|x| input_text(&x.value, x.formula.as_deref())).unwrap_or_default(),
        };
        let kind = format_kind(&code);
        let mut fmt = Json::obj();
        for k in ["general", "number", "currency", "accounting", "date", "time", "percent", "fraction", "scientific", "text"] {
            fmt.set(k, k == kind);
        }
        let h = xf.h_align;
        let align = Json::obj()
            .with("left", h == HAlign::Left)
            .with("center", h == HAlign::Center || h == HAlign::CenterContinuous)
            .with("right", h == HAlign::Right)
            .with("top", xf.v_align == VAlign::Top)
            .with("middle", xf.v_align == VAlign::Center)
            .with("bottom", xf.v_align == VAlign::Bottom);
        let hex = |c: Option<crate::ground::display::Color>| c.map(|c| format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b));
        let (fr, fc) = sheet.frozen();
        let mut o = Json::obj()
            .with("sheet", sheet.name.as_str())
            .with("cell", cell_name(r, col))
            .with("range", c.range.to_string())
            .with("input", input)
            .with("value", cell.map(|x| self.formatted(x)).unwrap_or_default())
            .with("formula", cell.is_some_and(|x| x.formula.is_some()))
            .with("editing", c.active.edit.is_some())
            .with("bold", font.bold)
            .with("italic", font.italic)
            .with("underline", font.underline)
            .with("strike", font.strike)
            .with("font", font.name.as_str())
            .with("size", font.size)
            .with("color", hex(font.color))
            .with("fill", hex(self.styles.fill(xf.fill)))
            .with("align", align)
            .with("wrap", xf.wrap)
            .with("merged", sheet.merge_at(r, col).is_some())
            .with("number_format", code.as_str())
            .with("format", fmt)
            .with("frozen", fr > 0 || fc > 0)
            .with("filter", self.filter_range(c.sheet).is_some())
            .with("painter", self.painter.is_some())
            .with("cut", self.cut.is_some());
        if !c.is_cell() {
            // The status bar's numbers over the selection (the stored cells only).
            let (mut sum, mut count, mut nums) = (0.0, 0u64, 0u64);
            let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
            for (_, row) in sheet.rows.range(c.range.r1..=c.range.r2) {
                for (_, cell) in row.cells.range(c.range.c1..=c.range.c2) {
                    if cell.value.is_empty() {
                        continue;
                    }
                    count += 1;
                    if let CellValue::Number(n) = cell.value {
                        nums += 1;
                        sum += n;
                        min = min.min(n);
                        max = max.max(n);
                    }
                }
            }
            let mut st = Json::obj().with("count", count).with("numbers", nums);
            if nums > 0 {
                st.set("sum", sum);
                st.set("average", sum / nums as f64);
                st.set("min", min);
                st.set("max", max);
            }
            o.set("stats", st);
        }
        o
    }

    /// The cells of a selection as tab-separated shown text and our own rich form.
    pub(super) fn copy_of(&self, s: &Selection) -> Clip {
        let Some(c) = self.sel_of(s) else { return Clip::default() };
        let Some(sheet) = self.sheets.get(c.sheet) else { return Clip::default() };
        let mut range = c.range;
        if let Some(u) = sheet.used_range() {
            // A whole row or column copies as far as the cells go.
            range = CellRange::new(range.r1, range.c1, range.r2.min(u.r2.max(range.r1)), range.c2.min(u.c2.max(range.c1)));
        }
        if range.rows() * range.cols() > COPY_LIMIT {
            return Clip::default();
        }
        let mut text_rows = Vec::new();
        let mut rich_rows = Vec::new();
        for r in range.r1..=range.r2 {
            let mut t = Vec::new();
            let mut j = Vec::new();
            for col in range.c1..=range.c2 {
                match sheet.cell(r, col) {
                    Some(cell) => {
                        t.push(self.formatted(cell));
                        let mut o = Json::obj().with("v", cell.value.to_json()).with("s", cell.style);
                        if matches!(cell.value, CellValue::Error(_)) {
                            o.set("e", true);
                        }
                        if let Some(f) = &cell.formula {
                            o.set("f", f.as_str());
                        }
                        j.push(o);
                    }
                    None => {
                        t.push(String::new());
                        let s = sheet.effective_style(r, col);
                        j.push(if s == 0 { Json::Null } else { Json::obj().with("v", Json::Null).with("s", s) });
                    }
                }
            }
            text_rows.push(t);
            rich_rows.push(Json::Arr(j));
        }
        let merges: Vec<Json> = sheet
            .merges
            .iter()
            .filter(|m| range.r1 <= m.r1 && m.r2 <= range.r2 && range.c1 <= m.c1 && m.c2 <= range.c2)
            .map(|m| Json::obj().with("r1", (m.r1 - range.r1) as u64).with("c1", (m.c1 - range.c1) as u64).with("r2", (m.r2 - range.r1) as u64).with("c2", (m.c2 - range.c1) as u64))
            .collect();
        let rich = Json::obj()
            .with("ledger", self.id as f64)
            .with("sheet", c.sheet)
            .with("row", range.r1 as u64)
            .with("col", range.c1 as u64)
            .with("rows", range.rows())
            .with("cols", range.cols())
            .with("cells", Json::Arr(rich_rows))
            .with("merges", Json::Arr(merges));
        Clip { text: tsv(&text_rows), rich: Some(rich) }
    }
}

impl Editing for LedgerDoc {
    fn hit(&self, page: usize, x: f64, y: f64) -> Option<Pos> {
        let sheet = self.sheets.get(page)?;
        let g = geo(sheet);
        let (r, c) = g.hit(x, y)?;
        let (r, c) = match sheet.merge_at(r, c) {
            Some(m) => (m.r1, m.c1),
            None => (r, c),
        };
        Some(Place::new(page, r, c).pos())
    }

    fn caret(&self, pos: &Pos) -> Option<CaretRect> {
        let p = self.place(pos.json())?;
        let sheet = self.sheets.get(p.sheet)?;
        let g = geo(sheet);
        let area = sheet.merge_at(p.row, p.col).copied().unwrap_or(CellRange::cell(p.row, p.col));
        let rect = g.rect(sheet, &area);
        let mut x = rect.x;
        if let Some(text) = &p.edit {
            let style = self.cell_style(sheet, p.row, p.col);
            let mut fonts = self.fonts.lock().unwrap_or_else(|e| e.into_inner());
            x += 2.0 + fonts.measure(text, &style);
            x = x.min(rect.right().max(rect.x));
        }
        Some(CaretRect { page: p.sheet, x, y: rect.y, height: rect.h })
    }

    fn selection_rects(&self, sel: &Selection) -> Vec<(usize, Rect)> {
        let Some(c) = self.sel_of(sel) else { return Vec::new() };
        let Some(sheet) = self.sheets.get(c.sheet) else { return Vec::new() };
        if c.range.is_single() || sheet.merge_at(c.range.r1, c.range.c1).is_some_and(|m| *m == c.range) {
            return Vec::new();
        }
        let g = geo(sheet);
        g.rects(sheet, &c.range).into_iter().map(|r| (c.sheet, r)).collect()
    }

    fn state(&self, sel: &Selection) -> Json {
        self.edit_state(sel)
    }

    fn copy(&self, sel: &Selection) -> Clip {
        self.copy_of(sel)
    }

    fn step(&self, pos: &Pos, motion: &str) -> Option<Pos> {
        let p = self.place(pos.json())?;
        self.step_place(&Place::new(p.sheet, p.row, p.col), motion).map(|q| q.pos())
    }

    fn landed(&self, cmd: &Command) -> Option<Selection> {
        self.landing(cmd)
    }
}

/// A number format's text for a value (for tests and the find).
pub fn shown(value: &CellValue, code: &str, date1904: bool) -> String {
    NumberFormat::parse(code).format(value, date1904).text
}
