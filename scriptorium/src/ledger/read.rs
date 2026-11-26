//! Reading SpreadsheetML: the workbook (sheets, defined names, the date system), shared strings
//! and worksheets. Worksheets and the string table are read by streaming (pull events, no DOM), so
//! a large sheet costs its cells and nothing more.

use super::cellref::{parse_cell, CellRange, MAX_COLS, MAX_ROWS};
use super::formula::shift_formula;
use super::model::{base_width_to_pt, chars_to_pt, Cell, CellValue, ColInfo, DefinedName, Pane, Row, Sheet, SharedFormula, SheetState};
use super::numfmt::iso_to_serial;
use crate::ground::xml::{parse_dom, Attr, Event, Reader};

/// What `xl/workbook.xml` says.
#[derive(Debug, Clone, Default)]
pub struct WorkbookInfo {
    /// (name, sheetId, r:id, state) in tab order.
    pub sheets: Vec<(String, u32, String, SheetState)>,
    pub names: Vec<DefinedName>,
    pub date1904: bool,
    pub active_tab: usize,
}

pub fn parse_workbook(bytes: &[u8]) -> Result<WorkbookInfo, String> {
    let root = parse_dom(bytes).map_err(|e| format!("workbook.xml: {e}"))?;
    let mut info = WorkbookInfo::default();
    if let Some(pr) = root.child("workbookPr") {
        info.date1904 = matches!(pr.attr("date1904"), Some("1") | Some("true"));
    }
    if let Some(v) = root.child("bookViews").and_then(|b| b.child("workbookView")) {
        info.active_tab = v.attr("activeTab").and_then(|t| t.parse().ok()).unwrap_or(0);
    }
    if let Some(sheets) = root.child("sheets") {
        for s in sheets.children_named("sheet") {
            let rid = s.attrs.iter().find(|(k, _)| k.ends_with(":id")).map(|(_, v)| v.clone()).unwrap_or_default();
            info.sheets.push((
                s.attr("name").unwrap_or("Sheet").to_string(),
                s.attr("sheetId").and_then(|v| v.parse().ok()).unwrap_or(0),
                rid,
                SheetState::parse(s.attr("state")),
            ));
        }
    }
    if let Some(dn) = root.child("definedNames") {
        for d in dn.children_named("definedName") {
            info.names.push(DefinedName {
                name: d.attr("name").unwrap_or("").to_string(),
                value: d.text(),
                local_sheet: d.attr("localSheetId").and_then(|v| v.parse().ok()),
                hidden: matches!(d.attr("hidden"), Some("1") | Some("true")),
            });
        }
    }
    Ok(info)
}

/// `_x000D_`-style escapes of SpreadsheetML strings back to their characters.
pub fn decode_escapes(s: &str) -> String {
    if !s.contains("_x") {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'_' && i + 6 < b.len() && b[i + 1] == b'x' && b[i + 6] == b'_' {
            if let Ok(v) = u32::from_str_radix(&s[i + 2..i + 6], 16) {
                if let Some(c) = char::from_u32(v) {
                    out.push(c);
                    i += 7;
                    continue;
                }
            }
        }
        let c = s[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// The shared string table, rich runs flattened to plain text (phonetic runs left out).
pub fn parse_shared_strings(bytes: &[u8]) -> Result<Vec<String>, String> {
    let utf8 = crate::ground::xml::to_utf8(bytes).map_err(|e| format!("sharedStrings.xml: {e}"))?;
    let mut r = Reader::new(&utf8).map_err(|e| format!("sharedStrings.xml: {e}"))?;
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut in_t = false;
    let mut rph = 0usize;
    while let Some(ev) = r.next().map_err(|e| format!("sharedStrings.xml: {e}"))? {
        match ev {
            Event::Start { name, .. } => match name.local {
                "si" => cur = Some(String::new()),
                "rPh" => rph += 1,
                "t" if rph == 0 => in_t = true,
                _ => {}
            },
            Event::Empty { name, .. } => {
                if name.local == "si" {
                    out.push(String::new());
                }
            }
            Event::End { name, .. } => match name.local {
                "si" => {
                    if let Some(s) = cur.take() {
                        out.push(decode_escapes(&s));
                    }
                }
                "rPh" => rph = rph.saturating_sub(1),
                "t" => in_t = false,
                _ => {}
            },
            Event::Text { text, .. } => {
                if in_t {
                    if let Some(c) = cur.as_mut() {
                        c.push_str(&text);
                    }
                }
            }
            Event::CData { text, .. } => {
                if in_t {
                    if let Some(c) = cur.as_mut() {
                        c.push_str(text);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn attr<'a>(attrs: &'a [Attr<'_>], local: &str) -> Option<&'a str> {
    attrs.iter().find(|a| a.name.local == local && a.name.prefix.is_none()).map(|a| a.value.as_ref())
}

/// A cell being read.
#[derive(Default)]
struct CellBuild {
    row: u32,
    col: u32,
    t: String,
    s: u32,
    v: String,
    has_v: bool,
    is_text: String,
    f: Option<String>,
    f_shared_si: Option<u32>,
    f_master_ref: bool,
}

/// What the worksheet reader needs besides the sheet.
pub struct ReadCtx<'a> {
    pub sst: &'a [String],
    pub date1904: bool,
}

/// Reads a worksheet part (or a fragment of rows wrapped in any root) into `sheet`. Rows found
/// replace the sheet's rows of the same number; sheet-level elements (views, columns, merges) are
/// read only when `full`.
pub fn parse_worksheet(bytes: &[u8], ctx: &ReadCtx<'_>, sheet: &mut Sheet, full: bool) -> Result<(), String> {
    let utf8 = crate::ground::xml::to_utf8(bytes).map_err(|e| format!("{}: {e}", sheet.name))?;
    let mut r = Reader::new(&utf8).map_err(|e| format!("{}: {e}", sheet.name))?;
    let mut depth = 0usize;
    let mut first_view = true;
    let mut in_view = false;
    let mut cur_row: Option<u32> = None;
    let mut next_row: u32 = 0;
    let mut next_col: u32 = 0;
    let mut cell: Option<CellBuild> = None;
    let (mut in_v, mut in_f, mut in_is, mut in_t) = (false, false, false, false);
    let mut rph = 0usize;
    let mut pending: Vec<(u32, u32, u32)> = Vec::new();
    let mut base_col_width: Option<f64> = None;
    let mut default_col_width: Option<f64> = None;

    loop {
        let ev = match r.next().map_err(|e| format!("{}: {e}", sheet.name))? {
            Some(ev) => ev,
            None => break,
        };
        let (is_start, is_empty) = (matches!(ev, Event::Start { .. }), matches!(ev, Event::Empty { .. }));
        match &ev {
            Event::Start { name, attrs, .. } | Event::Empty { name, attrs, .. } => {
                if depth == 0 && full {
                    sheet.ns_decls = attrs
                        .iter()
                        .filter(|a| a.name.full == "xmlns" || a.name.prefix == Some("xmlns"))
                        .map(|a| (a.name.full.to_string(), a.value.to_string()))
                        .collect();
                }
                match name.local {
                    "dimension" if full => sheet.dimension = attr(attrs, "ref").map(str::to_string),
                    "sheetView" if full => {
                        in_view = first_view && is_start;
                        if first_view {
                            sheet.show_grid = !matches!(attr(attrs, "showGridLines"), Some("0") | Some("false"));
                            if let Some((r0, c0)) = attr(attrs, "topLeftCell").and_then(parse_cell) {
                                sheet.view.row = r0;
                                sheet.view.col = c0;
                            }
                        }
                        first_view = false;
                    }
                    "pane" if full && in_view => {
                        let num = |a: &str| attr(attrs, a).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
                        let state = attr(attrs, "state").unwrap_or("split");
                        let frozen = state == "frozen" || state == "frozenSplit";
                        let top_left = attr(attrs, "topLeftCell").and_then(parse_cell);
                        let pane = Pane { x_split: num("xSplit") as u32, y_split: num("ySplit") as u32, top_left, frozen };
                        if frozen {
                            let (tr, tc) = top_left.unwrap_or((pane.y_split, pane.x_split));
                            sheet.view.row = tr.max(pane.y_split);
                            sheet.view.col = tc.max(pane.x_split);
                        }
                        sheet.pane = Some(pane);
                    }
                    "selection" if full && in_view => {
                        if sheet.active_cell.is_none() || attr(attrs, "pane").is_some() {
                            sheet.active_cell = attr(attrs, "activeCell").and_then(parse_cell);
                            sheet.selection = attr(attrs, "sqref").map(str::to_string);
                        }
                    }
                    "sheetFormatPr" if full => {
                        if let Some(h) = attr(attrs, "defaultRowHeight").and_then(|v| v.parse().ok()) {
                            sheet.default_row_height = h;
                        }
                        default_col_width = attr(attrs, "defaultColWidth").and_then(|v| v.parse().ok());
                        base_col_width = attr(attrs, "baseColWidth").and_then(|v| v.parse().ok());
                    }
                    "col" if full => {
                        let min: u32 = attr(attrs, "min").and_then(|v| v.parse().ok()).unwrap_or(1);
                        let max: u32 = attr(attrs, "max").and_then(|v| v.parse().ok()).unwrap_or(min);
                        if min >= 1 {
                            sheet.cols.push(ColInfo {
                                min: min - 1,
                                max: (max.max(min) - 1).min(MAX_COLS - 1),
                                width: attr(attrs, "width").and_then(|v| v.parse().ok()),
                                hidden: matches!(attr(attrs, "hidden"), Some("1") | Some("true")),
                                style: attr(attrs, "style").and_then(|v| v.parse().ok()),
                            });
                        }
                    }
                    "row" => {
                        let rn = attr(attrs, "r").and_then(|v| v.parse::<u32>().ok()).filter(|&v| v >= 1 && v <= MAX_ROWS).map(|v| v - 1).unwrap_or(next_row);
                        next_row = rn + 1;
                        next_col = 0;
                        let custom_fmt = matches!(attr(attrs, "customFormat"), Some("1") | Some("true"));
                        let row = Row {
                            cells: Default::default(),
                            height: attr(attrs, "ht").and_then(|v| v.parse().ok()),
                            hidden: matches!(attr(attrs, "hidden"), Some("1") | Some("true")),
                            style: if custom_fmt { attr(attrs, "s").and_then(|v| v.parse().ok()) } else { None },
                        };
                        sheet.rows.insert(rn, row);
                        cur_row = if is_start { Some(rn) } else { None };
                    }
                    "c" if cur_row.is_some() => {
                        let (row, col) = match attr(attrs, "r").and_then(parse_cell) {
                            Some(p) => p,
                            None => (cur_row.unwrap_or(0), next_col.min(MAX_COLS - 1)),
                        };
                        next_col = col + 1;
                        let b = CellBuild {
                            row,
                            col,
                            t: attr(attrs, "t").unwrap_or("n").to_string(),
                            s: attr(attrs, "s").and_then(|v| v.parse().ok()).unwrap_or(0),
                            ..CellBuild::default()
                        };
                        if is_empty {
                            finish_cell(b, ctx, sheet, &mut pending);
                        } else {
                            cell = Some(b);
                        }
                    }
                    "v" if cell.is_some() => in_v = is_start,
                    "f" if cell.is_some() => {
                        if let Some(c) = cell.as_mut() {
                            let shared = attr(attrs, "t") == Some("shared");
                            if shared {
                                c.f_shared_si = attr(attrs, "si").and_then(|v| v.parse().ok());
                                c.f_master_ref = attr(attrs, "ref").is_some();
                            }
                            c.f = Some(String::new());
                        }
                        in_f = is_start;
                    }
                    "is" if cell.is_some() => in_is = is_start,
                    "rPh" if in_is => {
                        if is_start {
                            rph += 1;
                        }
                    }
                    "t" if in_is && rph == 0 => in_t = is_start,
                    "mergeCell" if full => {
                        if let Some(m) = attr(attrs, "ref").and_then(CellRange::parse) {
                            if !m.is_single() {
                                sheet.merges.push(m);
                            }
                        }
                    }
                    _ => {}
                }
                if is_start {
                    depth += 1;
                }
            }
            Event::End { name, .. } => {
                depth = depth.saturating_sub(1);
                match name.local {
                    "sheetView" => in_view = false,
                    "row" => cur_row = None,
                    "c" => {
                        if let Some(b) = cell.take() {
                            finish_cell(b, ctx, sheet, &mut pending);
                        }
                        in_v = false;
                        in_f = false;
                        in_is = false;
                        in_t = false;
                    }
                    "v" => in_v = false,
                    "f" => in_f = false,
                    "is" => in_is = false,
                    "rPh" => rph = rph.saturating_sub(1),
                    "t" => in_t = false,
                    _ => {}
                }
            }
            Event::Text { text, .. } => {
                if let Some(c) = cell.as_mut() {
                    if in_v {
                        c.v.push_str(text);
                        c.has_v = true;
                    } else if in_f {
                        if let Some(f) = c.f.as_mut() {
                            f.push_str(text);
                        }
                    } else if in_t {
                        c.is_text.push_str(text);
                    }
                }
            }
            Event::CData { text, .. } => {
                if let Some(c) = cell.as_mut() {
                    if in_v {
                        c.v.push_str(text);
                        c.has_v = true;
                    } else if in_t {
                        c.is_text.push_str(text);
                    }
                }
            }
            _ => {}
        }
    }
    if full {
        sheet.cols.sort_by_key(|c| c.min);
        if let Some(w) = default_col_width {
            sheet.default_col_width = chars_to_pt(w);
        } else if let Some(b) = base_col_width {
            sheet.default_col_width = base_width_to_pt(b);
        }
    }
    for (row, col, si) in pending {
        let Some(m) = sheet.shared.get(&si).cloned() else { continue };
        let text = shift_formula(&m.text, row as i64 - m.row as i64, col as i64 - m.col as i64);
        if let Some(c) = sheet.rows.get_mut(&row).and_then(|r| r.cells.get_mut(&col)) {
            c.formula = Some(text);
        }
    }
    Ok(())
}

fn finish_cell(b: CellBuild, ctx: &ReadCtx<'_>, sheet: &mut Sheet, pending: &mut Vec<(u32, u32, u32)>) {
    let v = b.v.as_str();
    let value = match b.t.as_str() {
        "s" => match v.trim().parse::<usize>() {
            Ok(i) => CellValue::Text(ctx.sst.get(i).cloned().unwrap_or_default()),
            Err(_) => CellValue::Empty,
        },
        "str" => {
            if b.has_v {
                CellValue::Text(decode_escapes(v))
            } else {
                CellValue::Empty
            }
        }
        "inlineStr" => {
            if !b.is_text.is_empty() || !b.has_v {
                CellValue::Text(decode_escapes(&b.is_text))
            } else {
                CellValue::Text(decode_escapes(v))
            }
        }
        "b" => {
            if b.has_v {
                CellValue::Bool(matches!(v.trim(), "1" | "true" | "TRUE"))
            } else {
                CellValue::Empty
            }
        }
        "e" => CellValue::Error(v.trim().to_string()),
        "d" => match iso_to_serial(v, ctx.date1904) {
            Some(n) => CellValue::Number(n),
            None if b.has_v => CellValue::Text(v.to_string()),
            None => CellValue::Empty,
        },
        _ => match v.trim().parse::<f64>() {
            Ok(n) if b.has_v => CellValue::Number(n),
            _ if b.has_v && !v.trim().is_empty() => CellValue::Text(v.to_string()),
            _ => CellValue::Empty,
        },
    };
    let mut formula = b.f.filter(|f| !f.is_empty());
    if let Some(si) = b.f_shared_si {
        match &formula {
            Some(text) if b.f_master_ref => {
                sheet.shared.insert(si, SharedFormula { row: b.row, col: b.col, text: text.clone() });
            }
            Some(_) => {}
            None => {
                pending.push((b.row, b.col, si));
                formula = None;
            }
        }
    }
    let row = sheet.rows.entry(b.row).or_default();
    row.cells.insert(b.col, Cell { value, formula, style: b.s });
}
