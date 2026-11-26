//! Structural edits inside the XML: rows and columns inserted or deleted (or cells shifted in a
//! band), with everything that names a place following along: the cells and rows themselves,
//! `<cols>`, merged ranges, conditional formats (their ranges and formulas), data validations,
//! hyperlinks, the AutoFilter, the dimension, the selection and the pane; formulas everywhere
//! through [`rewrite_formulas`], defined names through [`rewrite_names`].

use std::collections::BTreeMap;

use super::cellref::{cell_name, parse_cell, CellRange, MAX_COLS, MAX_ROWS};
use super::edit::{cell_positions, elem, explode_shared, qualify, row_positions, sheet_data_index, shared_groups};
use super::refs::{shift_span, Axis, Shift};
use crate::ground::xml::{Element, Node};

/// Rewrites every formula of a worksheet with `f` (shared formula groups whose master `needs`
/// rewriting are turned into plain formulas first, so each cell gets its own text). Array formula
/// ranges move with `range` when given.
pub fn rewrite_formulas(root: &mut Element, f: &dyn Fn(&str) -> String, needs: &dyn Fn(&str) -> bool, range: Option<&dyn Fn(&CellRange) -> Option<CellRange>>) {
    let Some(sdi) = sheet_data_index(root) else { return };
    let Node::Element(sd) = &mut root.children[sdi] else { return };
    // Groups whose master mentions what changes.
    let mut sis = Vec::new();
    for si in shared_groups(sd, None) {
        let master = find_master(sd, &si);
        if master.as_deref().is_some_and(needs) {
            sis.push(si);
        }
    }
    explode_shared(sd, &sis);
    for n in sd.children.iter_mut() {
        let Node::Element(row) = n else { continue };
        if row.local() != "row" {
            continue;
        }
        for cn in row.children.iter_mut() {
            let Node::Element(c) = cn else { continue };
            if c.local() != "c" {
                continue;
            }
            let Some(fe) = c.child_mut("f") else { continue };
            // A shared child carries no text; its master was handled above or needs nothing.
            let text = fe.text();
            if !text.is_empty() && needs(&text) {
                let new = f(&text);
                if new != text {
                    fe.children = vec![Node::Text(new)];
                }
            }
            if let (Some(rf), Some(map)) = (fe.attr("ref").map(str::to_string), range) {
                if fe.attr("t") != Some("shared") {
                    if let Some(r) = CellRange::parse(&rf) {
                        if let Some(nr) = map(&r) {
                            if nr != r {
                                fe.set_attr("ref", nr.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
}

fn find_master(sd: &Element, si: &str) -> Option<String> {
    for (ri, _) in row_positions(sd) {
        let row = elem(sd, ri);
        for (ci, _) in cell_positions(row) {
            if let Some(f) = elem(row, ci).child("f") {
                if f.attr("t") == Some("shared") && f.attr("si") == Some(si) && f.attr("ref").is_some() {
                    return Some(f.text());
                }
            }
        }
    }
    None
}

/// Rewrites the text of every `<definedName>` of a workbook with `f`.
pub fn rewrite_names(root: &mut Element, f: &dyn Fn(&str) -> String) {
    let Some(dn) = root.child_mut("definedNames") else { return };
    for n in dn.children.iter_mut() {
        let Node::Element(d) = n else { continue };
        if d.local() != "definedName" {
            continue;
        }
        let text = d.text();
        let new = f(&text);
        if new != text {
            d.children = vec![Node::Text(new)];
        }
    }
}

/// A space-separated list of ranges after `map` (ranges that are gone are left out).
fn map_sqref(sqref: &str, map: &dyn Fn(&CellRange) -> Option<CellRange>) -> String {
    sqref.split_whitespace().filter_map(|t| CellRange::parse(t).and_then(|r| map(&r))).map(|r| r.to_string()).collect::<Vec<_>>().join(" ")
}

/// Moves the cells and rows of a worksheet's `sheetData` as `op` says (after every shared formula
/// group of the sheet was turned into plain formulas by the caller).
pub fn remap_sheet_data(root: &mut Element, op: &Shift) {
    let Some(sdi) = sheet_data_index(root) else { return };
    let Node::Element(sd) = &mut root.children[sdi] else { return };
    let prefix = sd.prefix().map(str::to_string);
    let in_band = |v: u32| op.band.map_or(true, |(lo, hi)| v >= lo && v <= hi);
    let moves_rows = op.axis == Axis::Row && op.band.is_none();
    let mut rows_out: BTreeMap<u32, Element> = BTreeMap::new();
    let mut cells_out: BTreeMap<u32, BTreeMap<u32, Element>> = BTreeMap::new();
    let mut others: Vec<Node> = Vec::new();
    let mut next = 0u32;
    for n in std::mem::take(&mut sd.children) {
        let Node::Element(row) = n else {
            others.push(n);
            continue;
        };
        if row.local() != "row" {
            others.push(Node::Element(row));
            continue;
        }
        let r = row.attr("r").and_then(|v| v.parse::<u32>().ok()).filter(|&v| v >= 1).map(|v| v - 1).unwrap_or(next);
        next = r + 1;
        let row_target = if moves_rows { shift_span(r, r, op.at, op.count, MAX_ROWS).map(|t| t.0) } else { Some(r) };
        let mut shell = Element { name: row.name.clone(), attrs: row.attrs.clone(), children: Vec::new() };
        let mut nextc = 0u32;
        for cn in row.children {
            let Node::Element(mut c) = cn else { continue };
            if c.local() != "c" {
                shell.children.push(Node::Element(c));
                continue;
            }
            let col = c.attr("r").and_then(parse_cell).map(|p| p.1).unwrap_or(nextc);
            nextc = col + 1;
            let target = match op.axis {
                Axis::Row if moves_rows => row_target.map(|nr| (nr, col)),
                Axis::Row => {
                    if in_band(col) {
                        shift_span(r, r, op.at, op.count, MAX_ROWS).map(|t| (t.0, col))
                    } else {
                        Some((r, col))
                    }
                }
                Axis::Col => {
                    if in_band(r) {
                        shift_span(col, col, op.at, op.count, MAX_COLS).map(|t| (r, t.0))
                    } else {
                        Some((r, col))
                    }
                }
            };
            if let Some((nr, nc)) = target {
                c.set_attr("r", cell_name(nr, nc));
                cells_out.entry(nr).or_default().insert(nc, c);
            }
        }
        if let Some(nr) = row_target {
            rows_out.insert(nr, shell);
        }
    }
    let mut keys: Vec<u32> = rows_out.keys().chain(cells_out.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    let mut children = Vec::with_capacity(keys.len());
    for r in keys {
        let mut row = rows_out.remove(&r).unwrap_or_else(|| Element::new(qualify(&prefix, "row")));
        row.set_attr("r", (r + 1).to_string());
        let cells = cells_out.remove(&r).unwrap_or_default();
        if let (Some(_), Some(first), Some(last)) = (row.attr("spans"), cells.keys().next(), cells.keys().next_back()) {
            let sp = format!("{}:{}", first + 1, last + 1);
            row.set_attr("spans", sp);
        }
        let had_own = row.attrs.iter().any(|(k, _)| k != "r" && k != "spans" && !k.ends_with("dyDescent"));
        if cells.is_empty() && !had_own && row.children.is_empty() {
            continue;
        }
        if cells.is_empty() {
            row.attrs.retain(|(k, _)| k != "spans");
        }
        // Keep the cells in order, before anything else the row carries.
        let rest = std::mem::take(&mut row.children);
        row.children = cells.into_values().map(Node::Element).collect();
        row.children.extend(rest);
        children.push(Node::Element(row));
    }
    children.extend(others);
    sd.children = children;
}

/// Shifts the sheet-level elements that name places (everything but the cells).
pub fn shift_sheet_elements(root: &mut Element, op: &Shift, home: &str) {
    let map = |r: &CellRange| op.range(r);
    let ff = |t: &str| super::refs::shift_structure(t, home, op);
    // Columns.
    if op.axis == Axis::Col && op.band.is_none() {
        if let Some(cols) = root.child_mut("cols") {
            let mut kept = Vec::new();
            for n in std::mem::take(&mut cols.children) {
                let Node::Element(mut c) = n else {
                    kept.push(n);
                    continue;
                };
                let min: u32 = c.attr("min").and_then(|v| v.parse().ok()).unwrap_or(1);
                let max: u32 = c.attr("max").and_then(|v| v.parse().ok()).unwrap_or(min);
                if min == 0 {
                    kept.push(Node::Element(c));
                    continue;
                }
                match shift_span(min - 1, max.max(min) - 1, op.at, op.count, MAX_COLS) {
                    Some((a, b)) => {
                        if (a + 1, b + 1) != (min, max) {
                            c.set_attr("min", (a + 1).to_string());
                            c.set_attr("max", (b + 1).to_string());
                        }
                        kept.push(Node::Element(c));
                    }
                    None => {}
                }
            }
            cols.children = kept;
        }
        if root.child("cols").is_some_and(|c| c.elements().next().is_none()) {
            root.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "cols"));
        }
    }
    // Merged ranges.
    if let Some(mc) = root.child_mut("mergeCells") {
        mc.children.retain_mut(|n| {
            let Node::Element(m) = n else { return true };
            let Some(r) = m.attr("ref").and_then(CellRange::parse) else { return true };
            match map(&r) {
                Some(nr) if !nr.is_single() => {
                    if nr != r {
                        m.set_attr("ref", nr.to_string());
                    }
                    true
                }
                _ => false,
            }
        });
        let n = mc.children_named("mergeCell").count();
        if mc.attr("count").is_some() {
            mc.set_attr("count", n.to_string());
        }
    }
    if root.child("mergeCells").is_some_and(|m| m.children_named("mergeCell").next().is_none()) {
        root.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "mergeCells"));
    }
    // Conditional formats: their ranges and formulas.
    root.children.retain_mut(|n| {
        let Node::Element(cf) = n else { return true };
        if cf.local() != "conditionalFormatting" {
            return true;
        }
        let Some(sq) = cf.attr("sqref").map(str::to_string) else { return true };
        let new = map_sqref(&sq, &map);
        if new.is_empty() {
            return false;
        }
        if new != sq {
            cf.set_attr("sqref", new);
        }
        for rule in cf.children.iter_mut() {
            let Node::Element(rule) = rule else { continue };
            for f in rule.children.iter_mut() {
                let Node::Element(f) = f else { continue };
                if f.local() == "formula" {
                    let t = f.text();
                    let nt = ff(&t);
                    if nt != t {
                        f.children = vec![Node::Text(nt)];
                    }
                }
            }
        }
        true
    });
    // Data validations.
    if let Some(dv) = root.child_mut("dataValidations") {
        dv.children.retain_mut(|n| {
            let Node::Element(v) = n else { return true };
            let Some(sq) = v.attr("sqref").map(str::to_string) else { return true };
            let new = map_sqref(&sq, &map);
            if new.is_empty() {
                return false;
            }
            if new != sq {
                v.set_attr("sqref", new);
            }
            for f in v.children.iter_mut() {
                let Node::Element(f) = f else { continue };
                if f.local() == "formula1" || f.local() == "formula2" {
                    let t = f.text();
                    let nt = ff(&t);
                    if nt != t {
                        f.children = vec![Node::Text(nt)];
                    }
                }
            }
            true
        });
        let n = dv.children_named("dataValidation").count();
        if dv.attr("count").is_some() {
            dv.set_attr("count", n.to_string());
        }
    }
    // Hyperlinks.
    if let Some(hl) = root.child_mut("hyperlinks") {
        hl.children.retain_mut(|n| {
            let Node::Element(h) = n else { return true };
            let Some(r) = h.attr("ref").and_then(CellRange::parse) else { return true };
            match map(&r) {
                Some(nr) => {
                    if nr != r {
                        h.set_attr("ref", nr.to_string());
                    }
                    true
                }
                None => false,
            }
        });
    }
    // The AutoFilter.
    if let Some(af) = root.child_mut("autoFilter") {
        if let Some(r) = af.attr("ref").and_then(CellRange::parse) {
            match map(&r) {
                Some(nr) if nr != r => af.set_attr("ref", nr.to_string()),
                Some(_) => {}
                None => {
                    root.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "autoFilter"));
                }
            }
        }
    }
    // The selection and the pane.
    if let Some(views) = root.child_mut("sheetViews") {
        for v in views.children.iter_mut() {
            let Node::Element(v) = v else { continue };
            for x in v.children.iter_mut() {
                let Node::Element(x) = x else { continue };
                if x.local() == "selection" {
                    if let Some((r, c)) = x.attr("activeCell").and_then(parse_cell) {
                        let nr = map(&CellRange::cell(r, c)).unwrap_or(CellRange::cell(r.min(MAX_ROWS - 1), c.min(MAX_COLS - 1)));
                        if (nr.r1, nr.c1) != (r, c) {
                            x.set_attr("activeCell", cell_name(nr.r1, nr.c1));
                        }
                    }
                    if let Some(sq) = x.attr("sqref").map(str::to_string) {
                        let new = map_sqref(&sq, &map);
                        let new = if new.is_empty() { x.attr("activeCell").unwrap_or("A1").to_string() } else { new };
                        if new != sq {
                            x.set_attr("sqref", new);
                        }
                    }
                }
            }
        }
    }
}

/// The dimension worked out from the cells (`A1` for an empty sheet).
pub fn fix_dimension(root: &mut Element) {
    let Some(sdi) = sheet_data_index(root) else { return };
    let sd = elem(root, sdi);
    let mut used: Option<CellRange> = None;
    for (ri, r) in row_positions(sd) {
        for (_, c) in cell_positions(elem(sd, ri)) {
            let here = CellRange::cell(r, c);
            used = Some(used.map_or(here, |u| u.union(&here)));
        }
    }
    let text = used.map(|u| u.to_string()).unwrap_or_else(|| "A1".into());
    if let Some(d) = root.child_mut("dimension") {
        if d.attr("ref") != Some(text.as_str()) {
            d.set_attr("ref", text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground::xml::parse_dom;
    use crate::ledger::edit::to_xml;

    const SHEET: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:C3"/><sheetViews><sheetView workbookViewId="0"><selection activeCell="B3" sqref="B3"/></sheetView></sheetViews><cols><col min="2" max="3" width="20" customWidth="1"/></cols><sheetData><row r="1" spans="1:3"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c></row><row r="2" ht="30" customHeight="1"><c r="B2"><v>5</v></c></row><row r="3"><c r="A3"><f>A1+B2</f><v>6</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A2:B2"/></mergeCells><conditionalFormatting sqref="A1:A3"><cfRule type="expression" priority="1"><formula>A1&gt;B2</formula></cfRule></conditionalFormatting></worksheet>"#;

    #[test]
    fn rows_inserted_move_everything() {
        let mut root = parse_dom(SHEET.as_bytes()).unwrap();
        let op = Shift { sheet: "S".into(), axis: Axis::Row, at: 1, count: 1, band: None };
        rewrite_formulas(&mut root, &|t| super::super::refs::shift_structure(t, "S", &op), &|_| true, None);
        remap_sheet_data(&mut root, &op);
        shift_sheet_elements(&mut root, &op, "S");
        fix_dimension(&mut root);
        let x = to_xml(&root);
        assert!(x.contains(r#"<row r="3" ht="30" customHeight="1"><c r="B3"><v>5</v></c></row>"#), "{x}");
        assert!(x.contains(r#"<row r="4"><c r="A4"><f>A1+B3</f><v>6</v></c></row>"#), "{x}");
        assert!(x.contains(r#"<mergeCell ref="A3:B3"/>"#));
        assert!(x.contains(r#"sqref="A1:A4""#) && x.contains("<formula>A1&gt;B3</formula>"));
        assert!(x.contains(r#"<dimension ref="A1:C4"/>"#));
        assert!(x.contains(r#"activeCell="B4" sqref="B4""#));
    }

    #[test]
    fn columns_deleted_and_cells_shifted() {
        let mut root = parse_dom(SHEET.as_bytes()).unwrap();
        let op = Shift { sheet: "S".into(), axis: Axis::Col, at: 1, count: -1, band: None };
        rewrite_formulas(&mut root, &|t| super::super::refs::shift_structure(t, "S", &op), &|_| true, None);
        remap_sheet_data(&mut root, &op);
        shift_sheet_elements(&mut root, &op, "S");
        let x = to_xml(&root);
        assert!(x.contains(r#"<row r="1" spans="1:2"><c r="A1"><v>1</v></c><c r="B1"><v>3</v></c></row>"#), "{x}");
        assert!(x.contains(r#"<row r="2" ht="30" customHeight="1"/>"#), "{x}");
        assert!(x.contains("<f>A1+#REF!</f>"));
        assert!(x.contains(r#"<col min="2" max="2" width="20" customWidth="1"/>"#));
        assert!(!x.contains("mergeCells"), "a merge squeezed to one cell goes");

        // Cells shifted down in column B only.
        let mut root = parse_dom(SHEET.as_bytes()).unwrap();
        let band = Shift { sheet: "S".into(), axis: Axis::Row, at: 0, count: 1, band: Some((1, 1)) };
        remap_sheet_data(&mut root, &band);
        let x = to_xml(&root);
        assert!(x.contains(r#"<row r="1" spans="1:3"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c></row>"#), "{x}");
        assert!(x.contains(r#"<row r="3"><c r="A3"><f>A1+B2</f><v>6</v></c><c r="B3"><v>5</v></c></row>"#), "{x}");
    }
}
