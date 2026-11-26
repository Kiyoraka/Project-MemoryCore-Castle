//! Tables: insert, rows and columns in and out, merge, widths and AutoFit, row heights, cell
//! alignment, repeated header rows, borders and shading, Tab from cell to cell. Every change is
//! made on the `w:tbl` element in WordprocessingML's own shape (`w:tblGrid`, `w:tcW`, `w:gridSpan`,
//! `w:vMerge`), and the cells' `w:tcW` follow the grid.

use super::dom::{self, el_at, el_at_mut, prefix_of, set_child, wel, PPR_ORDER, TBLPR_ORDER, TCPR_ORDER, TRPR_ORDER};
use super::edit::{hex_arg, Tx};
use super::pos::{atom_count, Flow, SPos, Span};
use super::ScrollDoc;
use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command};
use crate::ground::edit::Selection;
use crate::ground::text::TextStyle;
use crate::ground::xml::{self, Element, Node};

/// One cell of a row: its element index in the `w:tr`, its first grid column and span.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CellG {
    pub el: usize,
    pub col: usize,
    pub span: usize,
    /// `w:vMerge` continue.
    pub cont: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowG {
    pub el: usize,
    pub cells: Vec<CellG>,
}

/// A table's rows and cells on its grid.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Grid {
    pub rows: Vec<RowG>,
    pub widths: Vec<f64>,
}

fn twips_of(e: &Element, local: &str) -> Option<f64> {
    dom::attr_local(e, local).and_then(|v| v.trim().parse::<f64>().ok())
}

pub(crate) fn grid_of(tbl: &Element) -> Grid {
    let widths = tbl.child("tblGrid").map(|g| g.children_named("gridCol").map(|c| twips_of(c, "w").unwrap_or(0.0) / 20.0).collect()).unwrap_or_default();
    let mut rows = Vec::new();
    for (ri, tr) in tbl.elements().enumerate() {
        if tr.local() != "tr" {
            continue;
        }
        let mut col = tr.child("trPr").and_then(|p| p.child("gridBefore")).and_then(|g| twips_of(g, "val")).unwrap_or(0.0) as usize;
        let mut cells = Vec::new();
        for (ci, tc) in tr.elements().enumerate() {
            if tc.local() != "tc" {
                continue;
            }
            let pr = tc.child("tcPr");
            let span = pr.and_then(|p| p.child("gridSpan")).and_then(|g| twips_of(g, "val")).unwrap_or(1.0).max(1.0) as usize;
            let cont = pr.and_then(|p| p.child("vMerge")).is_some_and(|v| dom::attr_local(v, "val").map(str::trim) != Some("restart"));
            cells.push(CellG { el: ci, col, span, cont });
            col += span;
        }
        rows.push(RowG { el: ri, cells });
    }
    Grid { rows, widths }
}

impl Grid {
    pub fn cols(&self) -> usize {
        self.rows.iter().map(|r| r.cells.last().map(|c| c.col + c.span).unwrap_or(0)).max().unwrap_or(0).max(self.widths.len())
    }

    /// The cell of row `r` covering grid column `col`.
    pub fn at(&self, r: usize, col: usize) -> Option<usize> {
        self.rows.get(r)?.cells.iter().position(|c| c.col <= col && col < c.col + c.span)
    }
}

/// Where a paragraph sits in a table.
#[derive(Debug, Clone)]
pub(crate) struct TLoc {
    pub part: String,
    pub tbl: Vec<usize>,
    pub row: usize,
    pub cell: usize,
    pub grid: Grid,
}

impl TLoc {
    pub fn tr_path(&self, r: usize) -> Vec<usize> {
        let mut p = self.tbl.clone();
        p.push(self.grid.rows[r].el);
        p
    }

    pub fn tc_path(&self, r: usize, c: usize) -> Vec<usize> {
        let mut p = self.tr_path(r);
        p.push(self.grid.rows[r].cells[c].el);
        p
    }
}

/// The innermost table cell on a paragraph path.
fn locate(root: &Element, path: &[usize]) -> Option<(Vec<usize>, usize, usize)> {
    let mut cur = root;
    let mut found = None;
    for (k, &i) in path.iter().enumerate() {
        let next = cur.elements().nth(i)?;
        if next.local() == "tc" && k >= 2 {
            let tr = el_at(root, &path[..k])?;
            if tr.local() == "tr" {
                found = Some((path[..k - 1].to_vec(), path[k - 1], i));
            }
        }
        cur = next;
    }
    found
}

/// The usual text column width when nothing better is known.
const COLUMN: f64 = 468.0;

impl ScrollDoc {
    /// The XML root of a part (parsed fresh).
    pub(crate) fn part_dom(&self, part: &str) -> Option<Element> {
        self.pkg.part_bytes(part).ok().and_then(|b| xml::parse_dom(&b).ok())
    }

    /// Is a paragraph (by its part and path) inside a table cell?
    pub(crate) fn cell_of(&self, part: &str, path: &[usize]) -> Option<TLoc> {
        let root = self.part_dom(part)?;
        self.cell_in(&root, part, path)
    }

    fn cell_in(&self, root: &Element, part: &str, path: &[usize]) -> Option<TLoc> {
        let (tbl, tr_el, tc_el) = locate(root, path)?;
        let grid = grid_of(el_at(root, &tbl)?);
        let row = grid.rows.iter().position(|r| r.el == tr_el)?;
        let cell = grid.rows[row].cells.iter().position(|c| c.el == tc_el)?;
        Some(TLoc { part: part.to_string(), tbl, row, cell, grid })
    }

    /// The table place of a selection end.
    pub(crate) fn tloc(&self, p: &SPos) -> Option<TLoc> {
        let r = self.para_ref(&p.flow, p.p)?;
        self.cell_of(&r.part, &r.flat.path)
    }

    /// The table, rows and grid columns a selection covers: `(loc, r0, r1, c0, c1)`.
    fn rect(&self, s: &Span) -> Option<(TLoc, usize, usize, usize, usize)> {
        let a = self.tloc(&s.start)?;
        let b = self.tloc(&s.end).filter(|b| b.tbl == a.tbl).unwrap_or_else(|| a.clone());
        let ca = &a.grid.rows[a.row].cells[a.cell];
        let cb = &b.grid.rows[b.row].cells[b.cell];
        let (r0, r1) = (a.row.min(b.row), a.row.max(b.row));
        let (c0, c1) = (ca.col.min(cb.col), (ca.col + ca.span).max(cb.col + cb.span) - 1);
        Some((a, r0, r1, c0, c1))
    }

    /// The first (or last) paragraph under an XML path, by flat index.
    pub(crate) fn para_under(&self, flow: &Flow, prefix: &[usize], last: bool) -> Option<usize> {
        let list = match flow {
            Flow::Body => &self.model.flat,
            Flow::Part(p) => self.model.hflat.get(p)?,
        };
        let it = list.iter().enumerate().filter(|(_, f)| f.path.starts_with(prefix)).map(|(i, _)| i);
        if last {
            it.last()
        } else {
            it.into_iter().next()
        }
    }

    /// Land on the whole content of the cell at `prefix`.
    fn land_cell(&mut self, cmd: &Command, flow: &Flow, prefix: &[usize]) {
        if let (Some(a), Some(b)) = (self.para_under(flow, prefix, false), self.para_under(flow, prefix, true)) {
            let n = self.para_len(flow, b);
            let base = SPos { flow: flow.clone(), p: a, o: 0, pg: None };
            self.set_landed(cmd, Selection::new(base.to_pos(), base.para(b, n).to_pos()));
        }
    }

    /// The text column width of the section a paragraph is in.
    pub(crate) fn column_width(&self, flow: &Flow, p: usize) -> f64 {
        if *flow == Flow::Body {
            if let Some(l) = self.laid.lines.iter().find(|l| matches!(l.key, super::layout::LineKey::Body(f) if f == p)) {
                if self.tloc(&SPos::body(p, 0)).is_none() {
                    return l.col_w;
                }
            }
        }
        let s = &self.model.final_sect;
        let w = s.page_w - s.left - s.right;
        if w > 36.0 {
            w
        } else {
            COLUMN
        }
    }

    // --- insert a table ----------------------------------------------------------------------------

    pub(crate) fn cmd_insert_table(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let rows = cmd.args.get("rows").and_then(Json::as_usize).unwrap_or(2).clamp(1, 500);
        let cols = cmd.args.get("cols").and_then(Json::as_usize).unwrap_or(2).clamp(1, 63);
        let mut tx = Tx::default();
        let at = self.delete_span(&mut tx, &s)?;
        self.ensure_style(&mut tx, "TableGrid")?;
        let style = self.style_id("TableGrid").unwrap_or_else(|| "TableGrid".into());
        let width = if let Some(loc) = self.tloc(&at) {
            loc.grid.widths.get(loc.grid.rows[loc.row].cells[loc.cell].col).copied().unwrap_or(COLUMN / 2.0) - 10.8
        } else {
            self.column_width(&at.flow, at.p)
        };
        let colw = (width / cols as f64).max(18.0);
        let r = self.para_ref(&at.flow, at.p).ok_or_else(|| BusError::failed("no paragraph there"))?;
        let (part, path, fi) = (r.part.clone(), r.flat.path.clone(), r.para.fields_in.clone());
        let o = at.o;
        let first_cell_p = self.tx_part(&mut tx, &part, |root, cx| {
            let (cpath, idx) = (path[..path.len() - 1].to_vec(), path[path.len() - 1]);
            let Some(c) = el_at_mut(root, &cpath) else { return at.p };
            let Some(p) = dom::child_at_mut(c, idx) else { return at.p };
            let w = prefix_of(p);
            let runs = cx.runs(p, &fi);
            let n = atom_count(&runs);
            let ppr = p.child("pPr").cloned().map(|mut x| {
                set_child(&mut x, "sectPr", None, PPR_ORDER);
                set_child(&mut x, "numPr", None, PPR_ORDER);
                x
            });
            let tbl = new_table(&w, rows, cols, colw, &style, ppr.as_ref());
            if n == 0 || o == 0 {
                dom::insert_at(c, idx, vec![Node::Element(tbl)]);
                return at.p;
            }
            let at_idx = if o >= n {
                idx + 1
            } else {
                let right = dom::split_para(p, &runs, o);
                dom::insert_at(c, idx + 1, vec![Node::Element(right)]);
                idx + 1
            };
            dom::insert_at(c, at_idx, vec![Node::Element(tbl)]);
            // A table is always followed by a paragraph.
            let next_is_p = c.elements().nth(at_idx + 1).is_some_and(|e| e.local() == "p");
            if !next_is_p {
                let mut np = wel(&w, "p", &[]);
                if let Some(pp) = &ppr {
                    np.children.push(Node::Element(pp.clone()));
                }
                dom::insert_at(c, at_idx + 1, vec![Node::Element(np)]);
            }
            at.p + 1
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_caret(cmd, &at.para(first_cell_p, 0));
        Ok(ch)
    }

    // --- rows and columns --------------------------------------------------------------------------

    pub(crate) fn cmd_table_insert(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let what = cmd.args.get("what").and_then(Json::as_str).unwrap_or("row_below").to_string();
        let (loc, r0, r1, c0, c1) = self.rect(&s).ok_or_else(|| BusError::failed("the selection is not in a table"))?;
        let count = cmd.args.get("count").and_then(Json::as_usize).unwrap_or(1).clamp(1, 100);
        let flow = s.flow().clone();
        let mut tx = Tx::default();
        let target: Vec<usize>;
        match what.as_str() {
            "row_above" | "row_below" => {
                let below = what == "row_below";
                let src_row = if below { r1 } else { r0 };
                let tr_path = loc.tr_path(src_row);
                let tbl_path = loc.tbl.clone();
                let at_el = if below { loc.grid.rows[src_row].el + 1 } else { loc.grid.rows[src_row].el };
                self.tx_part(&mut tx, &loc.part, |root, _| {
                    let Some(tr) = el_at(root, &tr_path).cloned() else { return };
                    let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
                    let new: Vec<Node> = (0..count).map(|_| Node::Element(blank_row(&tr))).collect();
                    dom::insert_at(tbl, at_el, new);
                })?;
                let mut t = loc.tbl.clone();
                t.push(at_el);
                t.push(loc.grid.rows[src_row].cells.first().map(|c| c.el).unwrap_or(0));
                target = t;
            }
            "col_left" | "col_right" => {
                let right = what == "col_right";
                let b = if right { c1 + 1 } else { c0 };
                let refc = if right { c1 } else { c0 };
                let tbl_path = loc.tbl.clone();
                self.tx_part(&mut tx, &loc.part, |root, _| {
                    let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
                    for _ in 0..count {
                        insert_column(tbl, b, refc);
                    }
                })?;
                target = loc.tc_path(loc.row, loc.cell);
            }
            _ => return Err(BusError::bad_args(&cmd.name, "\"what\" is row_above, row_below, col_left or col_right")),
        }
        let ch = self.tx_commit(tx)?;
        if let Some(p) = self.para_under(&flow, &target, false) {
            self.land_caret(cmd, &SPos { flow: flow.clone(), p, o: 0, pg: None });
        }
        Ok(ch)
    }

    pub(crate) fn cmd_table_delete(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let what = cmd.args.get("what").and_then(Json::as_str).unwrap_or("row").to_string();
        let (loc, r0, r1, c0, c1) = self.rect(&s).ok_or_else(|| BusError::failed("the selection is not in a table"))?;
        let flow = s.flow().clone();
        let before_p = self.para_under(&flow, &loc.tbl, false).unwrap_or(0);
        let mut tx = Tx::default();
        let tbl_path = loc.tbl.clone();
        let all_rows = r0 == 0 && r1 + 1 >= loc.grid.rows.len();
        let all_cols = c0 == 0 && c1 + 1 >= loc.grid.cols();
        let whole = what == "table" || (what == "row" && all_rows) || (what == "col" && all_cols);
        match what.as_str() {
            _ if whole => {
                self.tx_part(&mut tx, &loc.part, |root, _| {
                    let (c, i) = (tbl_path[..tbl_path.len() - 1].to_vec(), tbl_path[tbl_path.len() - 1]);
                    if let Some(cont) = el_at_mut(root, &c) {
                        dom::remove_at(cont, i);
                        // A cell or the body must keep a paragraph.
                        if cont.local() == "tc" && !cont.elements().any(|e| e.local() == "p" || e.local() == "tbl") {
                            let w = prefix_of(cont);
                            cont.children.push(Node::Element(wel(&w, "p", &[])));
                        }
                    }
                })?;
            }
            "row" => {
                let els: Vec<usize> = (r0..=r1).map(|r| loc.grid.rows[r].el).collect();
                self.tx_part(&mut tx, &loc.part, |root, _| {
                    let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
                    for e in els.into_iter().rev() {
                        dom::remove_at(tbl, e);
                    }
                    fix_vmerge_heads(tbl);
                })?;
            }
            "col" => {
                self.tx_part(&mut tx, &loc.part, |root, _| {
                    let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
                    for col in (c0..=c1).rev() {
                        delete_column(tbl, col);
                    }
                })?;
            }
            _ => return Err(BusError::bad_args(&cmd.name, "\"what\" is row, col or table")),
        }
        let ch = self.tx_commit(tx)?;
        let p = before_p.min(self.flow_len(&flow).saturating_sub(1));
        self.land_caret(cmd, &SPos { flow, p, o: 0, pg: None });
        Ok(ch)
    }

    pub(crate) fn cmd_merge_cells(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let (loc, r0, r1, mut c0, mut c1) = self.rect(&s).ok_or_else(|| BusError::failed("the selection is not in a table"))?;
        // Widen to whole cells.
        for _ in 0..4 {
            for r in r0..=r1 {
                for c in &loc.grid.rows[r].cells {
                    if c.col <= c1 && c.col + c.span > c0 {
                        c0 = c0.min(c.col);
                        c1 = c1.max(c.col + c.span - 1);
                    }
                }
            }
        }
        if r0 == r1 && loc.grid.at(r0, c0) == loc.grid.at(r0, c1) {
            self.land_caret(cmd, &s.start);
            return Ok(Change::default());
        }
        let flow = s.flow().clone();
        let tbl_path = loc.tbl.clone();
        let grid = loc.grid.clone();
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &loc.part, |root, _| {
            let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
            let w = prefix_of(tbl);
            let mut moved: Vec<Node> = Vec::new();
            for r in (r0..=r1).rev() {
                let row = &grid.rows[r];
                let in_range: Vec<&CellG> = row.cells.iter().filter(|c| c.col >= c0 && c.col + c.span - 1 <= c1).collect();
                let Some(first) = in_range.first() else { continue };
                let Some(tr) = dom::child_at_mut(tbl, row.el) else { continue };
                // Content of the other cells in the row joins the first.
                let mut extra: Vec<Node> = Vec::new();
                for c in in_range.iter().skip(1).rev() {
                    if let Some(tc) = dom::remove_at(tr, c.el) {
                        extra.splice(0..0, cell_content(tc));
                    }
                }
                let Some(tc) = dom::child_at_mut(tr, first.el) else { continue };
                append_content(tc, extra);
                let pr = dom::first_child_mut(tc, &w, "tcPr");
                let span = c1 - c0 + 1;
                set_child(pr, "gridSpan", if span > 1 { Some(wel(&w, "gridSpan", &[("val", &span.to_string())])) } else { None }, TCPR_ORDER);
                if r1 > r0 {
                    if r == r0 {
                        set_child(pr, "vMerge", Some(wel(&w, "vMerge", &[("val", "restart")])), TCPR_ORDER);
                        let rest = std::mem::take(&mut moved);
                        append_content(tc, rest);
                    } else {
                        set_child(pr, "vMerge", Some(wel(&w, "vMerge", &[])), TCPR_ORDER);
                        let content = take_content(tc);
                        moved.splice(0..0, content);
                        tc.children.push(Node::Element(wel(&w, "p", &[])));
                    }
                }
            }
            sync_widths(tbl);
        })?;
        let ch = self.tx_commit(tx)?;
        let target = loc.tc_path(r0, loc.grid.at(r0, c0).unwrap_or(0));
        self.land_cell(cmd, &flow, &target);
        Ok(ch)
    }

    // --- sizes and looks ---------------------------------------------------------------------------

    pub(crate) fn cmd_table_widths(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let loc = self.tloc(&s.start).ok_or_else(|| BusError::failed("the selection is not in a table"))?;
        let flow = s.flow().clone();
        let widths: Option<Vec<f64>> = cmd.args.get("widths").and_then(Json::as_arr).map(|a| a.iter().filter_map(Json::as_f64).map(|x| x.max(4.0)).collect());
        let autofit = cmd.args.get("autofit").and_then(Json::as_str).map(str::to_string);
        let avail = self.column_width(&flow, self.para_under(&flow, &loc.tbl, false).unwrap_or(0));
        let ncols = loc.grid.cols().max(1);
        let mut grid: Vec<f64> = loc.grid.widths.clone();
        grid.resize(ncols, avail / ncols as f64);
        let mut mode = "fixed";
        if let Some(ws) = &widths {
            for (i, w) in ws.iter().enumerate().take(ncols) {
                grid[i] = *w;
            }
        } else {
            match autofit.as_deref() {
                Some("window") => {
                    let sum: f64 = grid.iter().sum::<f64>().max(1.0);
                    for g in &mut grid {
                        *g *= avail / sum;
                    }
                    mode = "window";
                }
                Some("contents") => {
                    grid = self.fit_contents(&flow, &loc, ncols, avail);
                    mode = "contents";
                }
                Some("fixed") => {}
                _ => return Err(BusError::bad_args(&cmd.name, "takes \"widths\": [points] or \"autofit\": window | contents | fixed")),
            }
        }
        let tbl_path = loc.tbl.clone();
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &loc.part, |root, _| {
            let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
            let w = prefix_of(tbl);
            set_grid(tbl, &w, &grid);
            let pr = dom::first_child_mut(tbl, &w, "tblPr");
            let total: f64 = grid.iter().sum();
            match mode {
                "window" => {
                    set_child(pr, "tblW", Some(wel(&w, "tblW", &[("w", "5000"), ("type", "pct")])), TBLPR_ORDER);
                    set_child(pr, "tblLayout", None, TBLPR_ORDER);
                }
                "contents" => {
                    set_child(pr, "tblW", Some(wel(&w, "tblW", &[("w", "0"), ("type", "auto")])), TBLPR_ORDER);
                    set_child(pr, "tblLayout", None, TBLPR_ORDER);
                }
                _ => {
                    set_child(pr, "tblW", Some(wel(&w, "tblW", &[("w", &tw(total)), ("type", "dxa")])), TBLPR_ORDER);
                    set_child(pr, "tblLayout", Some(wel(&w, "tblLayout", &[("type", "fixed")])), TBLPR_ORDER);
                }
            }
            sync_widths(tbl);
        })?;
        let ch = self.tx_commit(tx)?;
        self.set_landed(cmd, Selection::new(s.start.to_pos(), s.end.to_pos()));
        Ok(ch)
    }

    /// Column widths that fit each column's widest line (cell margins included), within `avail`.
    fn fit_contents(&mut self, flow: &Flow, loc: &TLoc, ncols: usize, avail: f64) -> Vec<f64> {
        let mut need = vec![36.0f64; ncols];
        for (r, row) in loc.grid.rows.iter().enumerate() {
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.span != 1 || cell.col >= ncols {
                    continue;
                }
                let prefix = loc.tc_path(r, c);
                let list = match flow {
                    Flow::Body => self.model.flat.clone(),
                    Flow::Part(p) => self.model.hflat.get(p).cloned().unwrap_or_default(),
                };
                for (i, f) in list.iter().enumerate() {
                    if !f.path.starts_with(&prefix) {
                        continue;
                    }
                    let Some(pr) = self.para_ref(flow, i) else { continue };
                    let e = self.eff_run(pr.para, if pr.para.runs.is_empty() { None } else { Some(0) });
                    let style = TextStyle {
                        family: e.font.clone().unwrap_or_else(|| "Times New Roman".into()),
                        east_asian_family: e.font_ea.clone().unwrap_or_default(),
                        size_pt: e.size.unwrap_or(10.0),
                        weight: if e.bold == Some(true) { 700 } else { 400 },
                        italic: e.italic == Some(true),
                        ..TextStyle::new("Times New Roman", 10.0)
                    };
                    let text = f.text.clone();
                    let wdt: f64 = text.split('\n').map(|line| self.fonts.shape_text(line, &style).iter().map(|r| r.width).sum::<f64>()).fold(0.0, f64::max);
                    need[cell.col] = need[cell.col].max(wdt + 10.8 + 2.0);
                }
            }
        }
        let total: f64 = need.iter().sum();
        if total > avail {
            for n in &mut need {
                *n *= avail / total;
            }
        }
        need
    }

    /// Apply `f(tcPr)` to every cell in the selection's rectangle.
    fn cells_edit(&mut self, cmd: &Command, f: impl Fn(&mut Element, &str, CellEdge)) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let (loc, r0, r1, c0, c1) = self.rect(&s).ok_or_else(|| BusError::failed("the selection is not in a table"))?;
        let tbl_path = loc.tbl.clone();
        let grid = loc.grid.clone();
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &loc.part, |root, _| {
            let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
            let w = prefix_of(tbl);
            for r in r0..=r1 {
                let row = &grid.rows[r];
                let Some(tr) = dom::child_at_mut(tbl, row.el) else { continue };
                for cell in &row.cells {
                    if cell.col + cell.span - 1 < c0 || cell.col > c1 {
                        continue;
                    }
                    let Some(tc) = dom::child_at_mut(tr, cell.el) else { continue };
                    if tc.child("tcPr").is_none() {
                        tc.children.insert(0, Node::Element(wel(&w, "tcPr", &[])));
                    }
                    let edge = CellEdge { top: r == r0, bottom: r == r1, left: cell.col <= c0, right: cell.col + cell.span - 1 >= c1 };
                    f(tc, &w, edge);
                }
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.set_landed(cmd, Selection::new(s.start.to_pos(), s.end.to_pos()));
        Ok(ch)
    }

    /// Apply `f(trPr)` to the selection's rows.
    fn rows_edit(&mut self, cmd: &Command, f: impl Fn(&mut Element, &str, usize, usize)) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let (loc, r0, r1, _, _) = self.rect(&s).ok_or_else(|| BusError::failed("the selection is not in a table"))?;
        let tbl_path = loc.tbl.clone();
        let grid = loc.grid.clone();
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &loc.part, |root, _| {
            let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
            let w = prefix_of(tbl);
            for (r, row) in grid.rows.iter().enumerate() {
                let Some(tr) = dom::child_at_mut(tbl, row.el) else { continue };
                if tr.child("trPr").is_none() {
                    let at = tr.elements().take_while(|e| e.local() == "tblPrEx").count();
                    dom::insert_at(tr, at, vec![Node::Element(wel(&w, "trPr", &[]))]);
                }
                let Some(pr) = tr.child_mut("trPr") else { continue };
                let which = (if r >= r0 && r <= r1 { 1 } else { 0 }) + (if r < r0 { 2 } else { 0 });
                f(pr, &w, r, which);
                let empty = pr.children.is_empty() && pr.attrs.is_empty();
                if empty {
                    tr.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "trPr"));
                }
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.set_landed(cmd, Selection::new(s.start.to_pos(), s.end.to_pos()));
        Ok(ch)
    }

    pub(crate) fn cmd_row_height(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let h = cmd.args.get("height").and_then(Json::as_f64);
        let exact = cmd.args.get("exact").and_then(Json::as_bool).unwrap_or(false);
        self.rows_edit(cmd, |pr, w, _, which| {
            if which & 1 == 0 {
                return;
            }
            match h {
                Some(h) if h > 0.0 => {
                    let mut e = wel(w, "trHeight", &[("val", &tw(h))]);
                    if exact {
                        e.set_attr(format!("{w}hRule"), "exact");
                    }
                    set_child(pr, "trHeight", Some(e), TRPR_ORDER);
                }
                _ => set_child(pr, "trHeight", None, TRPR_ORDER),
            }
        })
    }

    pub(crate) fn cmd_header_rows(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let on = cmd.args.get("on").and_then(Json::as_bool).unwrap_or(true);
        self.rows_edit(cmd, |pr, w, _, which| {
            let selected = which & 1 == 1;
            let above = which & 2 == 2;
            if on && (selected || above) {
                set_child(pr, "tblHeader", Some(wel(w, "tblHeader", &[])), TRPR_ORDER);
            } else if !on && selected {
                set_child(pr, "tblHeader", None, TRPR_ORDER);
            }
        })
    }

    pub(crate) fn cmd_row_break(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let allow = cmd.args.get("allow").and_then(Json::as_bool).unwrap_or(true);
        self.rows_edit(cmd, |pr, w, _, which| {
            if which & 1 == 1 {
                set_child(pr, "cantSplit", if allow { None } else { Some(wel(w, "cantSplit", &[])) }, TRPR_ORDER);
            }
        })
    }

    pub(crate) fn cmd_cell_align(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let v = cmd.args.get("v").and_then(Json::as_str).map(str::to_string);
        let h = cmd.args.get("h").and_then(Json::as_str).map(str::to_string);
        if let Some(x) = &v {
            if !matches!(x.as_str(), "top" | "center" | "bottom") {
                return Err(BusError::bad_args(&cmd.name, "\"v\" is top, center or bottom"));
            }
        }
        let jc = match h.as_deref() {
            None => None,
            Some("left") => Some("left"),
            Some("center") | Some("centre") => Some("center"),
            Some("right") => Some("right"),
            Some("justify") => Some("both"),
            _ => return Err(BusError::bad_args(&cmd.name, "\"h\" is left, center, right or justify")),
        };
        self.cells_edit(cmd, |tc, w, _| {
            if let Some(x) = &v {
                if let Some(pr) = tc.child_mut("tcPr") {
                    set_child(pr, "vAlign", if x == "top" { None } else { Some(wel(w, "vAlign", &[("val", x)])) }, TCPR_ORDER);
                }
            }
            if let Some(j) = jc {
                for c in tc.children.iter_mut() {
                    if let Node::Element(p) = c {
                        if p.local() == "p" {
                            let ppr = dom::ppr_mut(p, w);
                            set_child(ppr, "jc", Some(wel(w, "jc", &[("val", j)])), PPR_ORDER);
                        }
                    }
                }
            }
        })
    }

    pub(crate) fn cmd_shading(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let color = match cmd.args.get("color") {
            None | Some(Json::Null) => None,
            Some(Json::Str(c)) => Some(hex_arg(c).ok_or_else(|| BusError::bad_args(&cmd.name, "\"color\" is a hex colour like \"D9D9D9\" or null"))?),
            _ => return Err(BusError::bad_args(&cmd.name, "\"color\" is a hex colour or null")),
        };
        self.cells_edit(cmd, |tc, w, _| {
            if let Some(pr) = tc.child_mut("tcPr") {
                set_child(pr, "shd", color.as_ref().map(|c| wel(w, "shd", &[("val", "clear"), ("color", "auto"), ("fill", c)])), TCPR_ORDER);
            }
        })
    }

    pub(crate) fn cmd_borders(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let which = cmd.args.get("which").and_then(Json::as_str).unwrap_or("all").to_string();
        const WHICH: [&str; 10] = ["all", "outside", "inside", "none", "top", "bottom", "left", "right", "inside_h", "inside_v"];
        if !WHICH.contains(&which.as_str()) {
            return Err(BusError::bad_args(&cmd.name, format!("\"which\" is one of {}", WHICH.join(", "))));
        }
        let style = match cmd.args.get("style").and_then(Json::as_str).unwrap_or("single") {
            "single" => "single",
            "double" => "double",
            "dashed" | "dash" => "dashed",
            "dotted" | "dot" => "dotted",
            "thick" => "thick",
            "none" | "nil" => "nil",
            _ => return Err(BusError::bad_args(&cmd.name, "\"style\" is single, double, dashed, dotted, thick or none")),
        }
        .to_string();
        let size = cmd.args.get("size").and_then(Json::as_f64).unwrap_or(0.5).clamp(0.25, 6.0);
        let color = cmd.args.get("color").and_then(Json::as_str).and_then(hex_arg).unwrap_or_else(|| "auto".into());
        let scope = cmd.args.get("scope").and_then(Json::as_str).map(str::to_string).unwrap_or_else(|| if s.is_caret() || which.starts_with("inside") { "table".into() } else { "cells".into() });
        let val = if which == "none" { "nil".to_string() } else { style };
        let border = move |w: &str, side: &str| wel(w, side, &[("val", &val), ("sz", &format!("{}", (size * 8.0).round() as i64)), ("space", "0"), ("color", &color)]);
        let sides_for = |which: &str| -> Vec<&'static str> {
            match which {
                "all" | "none" => vec!["top", "left", "bottom", "right", "insideH", "insideV"],
                "outside" => vec!["top", "left", "bottom", "right"],
                "inside" => vec!["insideH", "insideV"],
                "top" => vec!["top"],
                "bottom" => vec!["bottom"],
                "left" => vec!["left"],
                "right" => vec!["right"],
                "inside_h" => vec!["insideH"],
                _ => vec!["insideV"],
            }
        };
        if scope == "table" {
            let loc = self.tloc(&s.start).ok_or_else(|| BusError::failed("the selection is not in a table"))?;
            let tbl_path = loc.tbl.clone();
            let sides = sides_for(&which);
            let mut tx = Tx::default();
            self.tx_part(&mut tx, &loc.part, |root, _| {
                let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
                let w = prefix_of(tbl);
                let pr = dom::first_child_mut(tbl, &w, "tblPr");
                let b = dom::ensure_child(pr, &w, "tblBorders", TBLPR_ORDER);
                for side in &sides {
                    // `start`/`end` written by newer files mean left/right.
                    let alias = match *side {
                        "left" => Some("start"),
                        "right" => Some("end"),
                        _ => None,
                    };
                    if let Some(a) = alias {
                        set_child(b, a, None, dom::BORDER_ORDER);
                    }
                    set_child(b, side, Some(border(&w, side)), dom::BORDER_ORDER);
                }
            })?;
            let ch = self.tx_commit(tx)?;
            self.set_landed(cmd, Selection::new(s.start.to_pos(), s.end.to_pos()));
            return Ok(ch);
        }
        let which2 = which.clone();
        self.cells_edit(cmd, move |tc, w, edge| {
            let sides: Vec<&str> = match which2.as_str() {
                "all" | "none" => vec!["top", "left", "bottom", "right"],
                "outside" => {
                    let mut v = Vec::new();
                    if edge.top {
                        v.push("top");
                    }
                    if edge.left {
                        v.push("left");
                    }
                    if edge.bottom {
                        v.push("bottom");
                    }
                    if edge.right {
                        v.push("right");
                    }
                    v
                }
                "inside" | "inside_h" | "inside_v" => {
                    let mut v = Vec::new();
                    if which2 != "inside_v" {
                        if !edge.top {
                            v.push("top");
                        }
                        if !edge.bottom {
                            v.push("bottom");
                        }
                    }
                    if which2 != "inside_h" {
                        if !edge.left {
                            v.push("left");
                        }
                        if !edge.right {
                            v.push("right");
                        }
                    }
                    v
                }
                "top" if edge.top => vec!["top"],
                "bottom" if edge.bottom => vec!["bottom"],
                "left" if edge.left => vec!["left"],
                "right" if edge.right => vec!["right"],
                _ => vec![],
            };
            if sides.is_empty() {
                return;
            }
            let Some(pr) = tc.child_mut("tcPr") else { return };
            let b = dom::ensure_child(pr, w, "tcBorders", TCPR_ORDER);
            for side in sides {
                set_child(b, side, Some(border(w, side)), dom::BORDER_ORDER);
            }
        })
    }

    pub(crate) fn cmd_table_align(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let loc = self.tloc(&s.start).ok_or_else(|| BusError::failed("the selection is not in a table"))?;
        let align = match cmd.args.get("align").and_then(Json::as_str).unwrap_or("left") {
            "left" => None,
            "center" | "centre" => Some("center"),
            "right" => Some("right"),
            _ => return Err(BusError::bad_args(&cmd.name, "\"align\" is left, center or right")),
        };
        let indent = cmd.args.get("indent").and_then(Json::as_f64);
        let tbl_path = loc.tbl.clone();
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &loc.part, |root, _| {
            let Some(tbl) = el_at_mut(root, &tbl_path) else { return };
            let w = prefix_of(tbl);
            let pr = dom::first_child_mut(tbl, &w, "tblPr");
            set_child(pr, "jc", align.map(|a| wel(&w, "jc", &[("val", a)])), TBLPR_ORDER);
            if let Some(i) = indent {
                set_child(pr, "tblInd", Some(wel(&w, "tblInd", &[("w", &tw(i)), ("type", "dxa")])), TBLPR_ORDER);
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.set_landed(cmd, Selection::new(s.start.to_pos(), s.end.to_pos()));
        Ok(ch)
    }

    // --- moving and selecting ----------------------------------------------------------------------

    /// Tab (or Shift+Tab) from cell to cell; Tab in the last cell adds a row.
    pub(crate) fn cell_step(&mut self, cmd: &Command, s: &Span, back: bool) -> Result<Change, BusError> {
        let Some(loc) = self.tloc(&s.end) else { return Ok(Change::default()) };
        let flow = s.flow().clone();
        let order: Vec<(usize, usize)> = loc.grid.rows.iter().enumerate().flat_map(|(r, row)| (0..row.cells.len()).map(move |c| (r, c))).collect();
        let here = order.iter().position(|x| *x == (loc.row, loc.cell)).unwrap_or(0);
        if back {
            if here == 0 {
                self.land_caret(cmd, &s.end);
                return Ok(Change::default());
            }
            let (r, c) = order[here - 1];
            self.land_cell(cmd, &flow, &loc.tc_path(r, c));
            return Ok(Change::default());
        }
        if here + 1 < order.len() {
            let (r, c) = order[here + 1];
            self.land_cell(cmd, &flow, &loc.tc_path(r, c));
            return Ok(Change::default());
        }
        // The last cell: a new row like this one.
        let tr_path = loc.tr_path(loc.row);
        let tbl_path = loc.tbl.clone();
        let at_el = loc.grid.rows[loc.row].el + 1;
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &loc.part, |root, _| {
            let Some(tr) = el_at(root, &tr_path).cloned() else { return };
            if let Some(tbl) = el_at_mut(root, &tbl_path) {
                dom::insert_at(tbl, at_el, vec![Node::Element(blank_row(&tr))]);
            }
        })?;
        let ch = self.tx_commit(tx)?;
        let mut target = loc.tbl.clone();
        target.push(at_el);
        if let Some(p) = self.para_under(&flow, &target, false) {
            self.land_caret(cmd, &SPos { flow, p, o: 0, pg: None });
        }
        Ok(ch)
    }

    /// `scroll.select`: a selection of a word, a paragraph, a cell, a row, a column, a table or
    /// everything (changes nothing).
    pub(crate) fn cmd_select(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let what = cmd.args.get("what").and_then(Json::as_str).unwrap_or("word");
        let flow = s.flow().clone();
        let at = s.end.clone();
        match what {
            "word" => {
                let chars = self.para_chars(&flow, at.p);
                let (a, b) = super::pos::word_at(&chars, at.o);
                self.land_span(cmd, &at.at(a), &at.at(b));
            }
            "paragraph" => {
                let n = self.para_len(&flow, at.p);
                self.land_span(cmd, &at.at(0), &at.at(n));
            }
            "all" => {
                let last = self.flow_len(&flow).saturating_sub(1);
                let n = self.para_len(&flow, last);
                self.land_span(cmd, &at.para(0, 0), &at.para(last, n));
            }
            "cell" | "row" | "column" | "table" => {
                let loc = self.tloc(&at).ok_or_else(|| BusError::failed("the selection is not in a table"))?;
                let (first, last) = match what {
                    "cell" => (loc.tc_path(loc.row, loc.cell), loc.tc_path(loc.row, loc.cell)),
                    "row" => (loc.tc_path(loc.row, 0), loc.tc_path(loc.row, loc.grid.rows[loc.row].cells.len() - 1)),
                    "column" => {
                        let col = loc.grid.rows[loc.row].cells[loc.cell].col;
                        let top = (0..loc.grid.rows.len()).find_map(|r| loc.grid.at(r, col).map(|c| (r, c))).unwrap_or((loc.row, loc.cell));
                        let bot = (0..loc.grid.rows.len()).rev().find_map(|r| loc.grid.at(r, col).map(|c| (r, c))).unwrap_or((loc.row, loc.cell));
                        (loc.tc_path(top.0, top.1), loc.tc_path(bot.0, bot.1))
                    }
                    _ => (loc.tbl.clone(), loc.tbl.clone()),
                };
                if let (Some(a), Some(b)) = (self.para_under(&flow, &first, false), self.para_under(&flow, &last, true)) {
                    let n = self.para_len(&flow, b);
                    self.land_span(cmd, &at.para(a, 0), &at.para(b, n));
                }
            }
            _ => return Err(BusError::bad_args(&cmd.name, "\"what\" is word, paragraph, cell, row, column, table or all")),
        }
        Ok(Change::default())
    }
}

/// Which edges of the selection's rectangle a cell is on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellEdge {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

fn tw(pt: f64) -> String {
    format!("{}", (pt * 20.0).round() as i64)
}

/// A new table: the grid style, auto width, `rows` x `cols` empty cells.
fn new_table(w: &str, rows: usize, cols: usize, colw: f64, style: &str, ppr: Option<&Element>) -> Element {
    let mut tbl = wel(w, "tbl", &[]);
    let mut pr = wel(w, "tblPr", &[]);
    pr.children.push(Node::Element(wel(w, "tblStyle", &[("val", style)])));
    pr.children.push(Node::Element(wel(w, "tblW", &[("w", "0"), ("type", "auto")])));
    pr.children.push(Node::Element(wel(w, "tblLook", &[("val", "04A0"), ("firstRow", "1"), ("lastRow", "0"), ("firstColumn", "1"), ("lastColumn", "0"), ("noHBand", "0"), ("noVBand", "1")])));
    tbl.children.push(Node::Element(pr));
    let mut grid = wel(w, "tblGrid", &[]);
    for _ in 0..cols {
        grid.children.push(Node::Element(wel(w, "gridCol", &[("w", &tw(colw))])));
    }
    tbl.children.push(Node::Element(grid));
    for _ in 0..rows {
        let mut tr = wel(w, "tr", &[]);
        for _ in 0..cols {
            let mut tc = wel(w, "tc", &[]);
            let mut tcpr = wel(w, "tcPr", &[]);
            tcpr.children.push(Node::Element(wel(w, "tcW", &[("w", &tw(colw)), ("type", "dxa")])));
            tc.children.push(Node::Element(tcpr));
            let mut p = wel(w, "p", &[]);
            if let Some(pp) = ppr {
                p.children.push(Node::Element(pp.clone()));
            }
            tc.children.push(Node::Element(p));
            tr.children.push(Node::Element(tc));
        }
        tbl.children.push(Node::Element(tr));
    }
    tbl
}

/// An empty row shaped like `tr`: the same row and cell properties (merges reset), one empty
/// paragraph per cell keeping the first paragraph's properties.
fn blank_row(tr: &Element) -> Element {
    let w = prefix_of(tr);
    let mut out = Element::new(tr.name.clone());
    out.attrs = tr.attrs.iter().filter(|(k, _)| !k.ends_with("paraId") && !k.ends_with("textId")).cloned().collect();
    for c in tr.elements() {
        match c.local() {
            "trPr" | "tblPrEx" => {
                let mut x = c.clone();
                if x.local() == "trPr" {
                    set_child(&mut x, "tblHeader", None, TRPR_ORDER);
                }
                out.children.push(Node::Element(x));
            }
            "tc" => {
                let mut tc = Element::new(c.name.clone());
                if let Some(pr) = c.child("tcPr") {
                    let mut pr = pr.clone();
                    set_child(&mut pr, "vMerge", None, TCPR_ORDER);
                    tc.children.push(Node::Element(pr));
                }
                let mut p = wel(&w, "p", &[]);
                if let Some(pp) = c.child("p").and_then(|p| p.child("pPr")) {
                    let mut pp = pp.clone();
                    set_child(&mut pp, "sectPr", None, PPR_ORDER);
                    p.children.push(Node::Element(pp));
                }
                tc.children.push(Node::Element(p));
                out.children.push(Node::Element(tc));
            }
            _ => {}
        }
    }
    out
}

/// The block content of a cell (everything but its properties).
fn cell_content(tc: Element) -> Vec<Node> {
    tc.children.into_iter().filter(|c| !matches!(c, Node::Element(e) if e.local() == "tcPr")).collect()
}

fn take_content(tc: &mut Element) -> Vec<Node> {
    let mut keep = Vec::new();
    let mut out = Vec::new();
    for c in std::mem::take(&mut tc.children) {
        match &c {
            Node::Element(e) if e.local() == "tcPr" => keep.push(c),
            _ => out.push(c),
        }
    }
    tc.children = keep;
    out
}

fn is_empty_para(n: &Node) -> bool {
    match n {
        Node::Element(e) if e.local() == "p" => !e.elements().any(|c| c.local() != "pPr" && c.local() != "bookmarkStart" && c.local() != "bookmarkEnd"),
        Node::Element(_) => false,
        _ => true,
    }
}

/// Add moved cell content: empty paragraphs are dropped, and the cell's lone empty paragraph gives
/// way to real content.
fn append_content(tc: &mut Element, extra: Vec<Node>) {
    let real: Vec<Node> = extra.into_iter().filter(|n| !is_empty_para(n)).collect();
    if real.is_empty() {
        return;
    }
    let only_empty = tc.children.iter().filter(|c| !matches!(c, Node::Element(e) if e.local() == "tcPr")).all(is_empty_para);
    if only_empty {
        tc.children.retain(|c| matches!(c, Node::Element(e) if e.local() == "tcPr"));
    }
    tc.children.extend(real);
}

/// Write the grid's widths.
fn set_grid(tbl: &mut Element, w: &str, widths: &[f64]) {
    let mut g = wel(w, "tblGrid", &[]);
    for x in widths {
        g.children.push(Node::Element(wel(w, "gridCol", &[("w", &tw(*x))])));
    }
    match tbl.child_mut("tblGrid") {
        Some(old) => *old = g,
        None => {
            let at = tbl.elements().take_while(|e| e.local() == "tblPr").count();
            dom::insert_at(tbl, at, vec![Node::Element(g)]);
        }
    }
}

/// Every cell's `w:tcW` from the grid (dxa).
fn sync_widths(tbl: &mut Element) {
    let grid = grid_of(tbl);
    let w = prefix_of(tbl);
    for row in &grid.rows {
        let Some(tr) = dom::child_at_mut(tbl, row.el) else { continue };
        for c in &row.cells {
            let Some(tc) = dom::child_at_mut(tr, c.el) else { continue };
            let width: f64 = (c.col..c.col + c.span).map(|i| grid.widths.get(i).copied().unwrap_or(0.0)).sum();
            if width <= 0.0 {
                continue;
            }
            if tc.child("tcPr").is_none() {
                tc.children.insert(0, Node::Element(wel(&w, "tcPr", &[])));
            }
            if let Some(pr) = tc.child_mut("tcPr") {
                set_child(pr, "tcW", Some(wel(&w, "tcW", &[("w", &tw(width)), ("type", "dxa")])), TCPR_ORDER);
            }
        }
    }
}

/// Insert a grid column at boundary `b`, half as wide as column `refc` (which gives up the other
/// half), with an empty cell in every row.
fn insert_column(tbl: &mut Element, b: usize, refc: usize) {
    let grid = grid_of(tbl);
    let w = prefix_of(tbl);
    let mut widths = grid.widths.clone();
    let ncols = grid.cols().max(widths.len());
    widths.resize(ncols, widths.last().copied().unwrap_or(72.0));
    let half = widths.get(refc).copied().unwrap_or(72.0) / 2.0;
    if refc < widths.len() {
        widths[refc] = half;
    }
    widths.insert(b.min(widths.len()), half);
    for row in &grid.rows {
        let Some(tr) = dom::child_at_mut(tbl, row.el) else { continue };
        // A cell that starts at b: the new one goes before it; one that ends at b: after it; one that
        // spans across b just grows.
        if let Some(c) = row.cells.iter().find(|c| c.col < b && b < c.col + c.span) {
            if let Some(tc) = dom::child_at_mut(tr, c.el) {
                let pr = dom::first_child_mut(tc, &w, "tcPr");
                set_child(pr, "gridSpan", Some(wel(&w, "gridSpan", &[("val", &(c.span + 1).to_string())])), TCPR_ORDER);
            }
            continue;
        }
        let (at, like) = match row.cells.iter().position(|c| c.col == b) {
            Some(i) => (row.cells[i].el, row.cells[i].el),
            None => match row.cells.last() {
                Some(l) => (l.el + 1, l.el),
                None => continue,
            },
        };
        let template = tr.elements().nth(like).cloned();
        let mut tc = wel(&w, "tc", &[]);
        let mut pr = wel(&w, "tcPr", &[]);
        pr.children.push(Node::Element(wel(&w, "tcW", &[("w", &tw(half)), ("type", "dxa")])));
        if let Some(t) = &template {
            if let Some(shd) = t.child("tcPr").and_then(|p| p.child("shd")) {
                pr.children.push(Node::Element(shd.clone()));
            }
        }
        tc.children.push(Node::Element(pr));
        let mut p = wel(&w, "p", &[]);
        if let Some(pp) = template.as_ref().and_then(|t| t.child("p")).and_then(|p| p.child("pPr")) {
            p.children.push(Node::Element(pp.clone()));
        }
        tc.children.push(Node::Element(p));
        dom::insert_at(tr, at, vec![Node::Element(tc)]);
    }
    set_grid(tbl, &w, &widths);
    sync_widths(tbl);
}

/// Remove grid column `col`: a spanning cell shrinks, a single one goes.
fn delete_column(tbl: &mut Element, col: usize) {
    let grid = grid_of(tbl);
    let w = prefix_of(tbl);
    for row in grid.rows.iter().rev() {
        let Some(tr) = dom::child_at_mut(tbl, row.el) else { continue };
        let Some(c) = row.cells.iter().find(|c| c.col <= col && col < c.col + c.span) else { continue };
        if c.span > 1 {
            if let Some(tc) = dom::child_at_mut(tr, c.el) {
                let pr = dom::first_child_mut(tc, &w, "tcPr");
                let span = c.span - 1;
                set_child(pr, "gridSpan", if span > 1 { Some(wel(&w, "gridSpan", &[("val", &span.to_string())])) } else { None }, TCPR_ORDER);
            }
        } else {
            dom::remove_at(tr, c.el);
        }
        if !tr.elements().any(|e| e.local() == "tc") {
            dom::remove_at(tbl, row.el);
        }
    }
    let mut widths = grid.widths.clone();
    if col < widths.len() {
        widths.remove(col);
    }
    set_grid(tbl, &w, &widths);
    sync_widths(tbl);
}

/// After rows are removed, a merge whose first row went starts again at its next row.
fn fix_vmerge_heads(tbl: &mut Element) {
    let grid = grid_of(tbl);
    let w = prefix_of(tbl);
    let mut prev: Vec<usize> = Vec::new();
    for row in &grid.rows {
        let Some(tr) = dom::child_at_mut(tbl, row.el) else { continue };
        let mut now = Vec::new();
        for c in &row.cells {
            let Some(tc) = dom::child_at_mut(tr, c.el) else { continue };
            let has = tc.child("tcPr").and_then(|p| p.child("vMerge")).is_some();
            if c.cont && !prev.contains(&c.col) {
                if let Some(pr) = tc.child_mut("tcPr") {
                    set_child(pr, "vMerge", Some(wel(&w, "vMerge", &[("val", "restart")])), TCPR_ORDER);
                }
            }
            if has {
                now.push(c.col);
            }
        }
        prev = now;
    }
}
