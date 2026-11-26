//! Writing cells back into a worksheet part. Edits work on the part's tree through
//! `opc::Package::edit_xml` (patch semantics: every row, cell and element the edit leaves equal is
//! copied byte for byte), touching only the rows they must. Each edit snapshots the rows it changes
//! as XML text, so its inverse can put them back exactly.
//!
//! A string is written as an inline string (`t="inlineStr"`), so the shared string table is never
//! touched. Overwriting the master cell of a shared formula hands the master text and range to the
//! next child, so the other cells keep their formulas.

use super::cellref::{cell_name, parse_cell, CellRange};
use super::formula::shift_formula;
use super::model::CellValue;
use crate::ground::xml::{parse_dom, Element, Node, Writer};

/// A row as it was (or is): its 0-based number and its XML (`None`: no such row).
#[derive(Debug, Clone, PartialEq)]
pub struct RowSnap {
    pub row: u32,
    pub xml: Option<String>,
}

/// What an edit did.
#[derive(Debug, Default)]
pub struct Outcome {
    pub old_rows: Vec<RowSnap>,
    pub new_rows: Vec<RowSnap>,
    /// The `dimension` before, when the edit changed it.
    pub old_dimension: Option<String>,
    pub new_dimension: Option<String>,
}

pub fn to_xml(e: &Element) -> String {
    let mut w = Writer::new();
    w.element(e);
    String::from_utf8(w.into_bytes()).unwrap_or_default()
}

/// Parses an element written by [`to_xml`] with the worksheet root's namespace declarations.
pub fn from_xml(xml: &str, ns: &[(String, String)]) -> Result<Element, String> {
    let mut wrap = String::from("<wrap");
    for (k, v) in ns {
        wrap.push(' ');
        wrap.push_str(k);
        wrap.push_str("=\"");
        wrap.push_str(&crate::ground::xml::escape_attr(v));
        wrap.push('"');
    }
    wrap.push('>');
    wrap.push_str(xml);
    wrap.push_str("</wrap>");
    let root = parse_dom(wrap.as_bytes()).map_err(|e| e.to_string())?;
    let first = root.elements().next().cloned();
    first.ok_or_else(|| "empty row XML".to_string())
}

fn qualify(prefix: &Option<String>, local: &str) -> String {
    match prefix {
        Some(p) => format!("{p}:{local}"),
        None => local.to_string(),
    }
}

fn sheet_data_index(root: &Element) -> Option<usize> {
    root.children.iter().position(|n| matches!(n, Node::Element(e) if e.local() == "sheetData"))
}

/// (child index, 0-based row) of each `<row>`, implicit numbers filled in.
fn row_positions(sd: &Element) -> Vec<(usize, u32)> {
    let mut out = Vec::new();
    let mut next = 0u32;
    for (i, n) in sd.children.iter().enumerate() {
        if let Node::Element(e) = n {
            if e.local() == "row" {
                let r = e.attr("r").and_then(|v| v.parse::<u32>().ok()).filter(|&v| v >= 1).map(|v| v - 1).unwrap_or(next);
                next = r + 1;
                out.push((i, r));
            }
        }
    }
    out
}

/// (child index, 0-based column) of each `<c>` of a row.
fn cell_positions(row: &Element) -> Vec<(usize, u32)> {
    let mut out = Vec::new();
    let mut next = 0u32;
    for (i, n) in row.children.iter().enumerate() {
        if let Node::Element(e) = n {
            if e.local() == "c" {
                let c = e.attr("r").and_then(parse_cell).map(|p| p.1).unwrap_or(next);
                next = c + 1;
                out.push((i, c));
            }
        }
    }
    out
}

fn elem_mut(parent: &mut Element, i: usize) -> &mut Element {
    match &mut parent.children[i] {
        Node::Element(e) => e,
        _ => unreachable!("index of an element"),
    }
}

fn elem(parent: &Element, i: usize) -> &Element {
    match &parent.children[i] {
        Node::Element(e) => e,
        _ => unreachable!("index of an element"),
    }
}

fn row_xml(sd: &Element, row: u32) -> Option<String> {
    row_positions(sd).into_iter().find(|p| p.1 == row).map(|(i, _)| to_xml(elem(sd, i)))
}

/// The new `<c>` for a value, keeping the style; `None` removes the cell.
fn new_cell(prefix: &Option<String>, row: u32, col: u32, style: Option<&str>, value: &CellValue) -> Option<Element> {
    let mut c = Element::new(qualify(prefix, "c"));
    c.attrs.push(("r".into(), cell_name(row, col)));
    if let Some(s) = style {
        c.attrs.push(("s".into(), s.to_string()));
    }
    let v = |text: String| {
        let mut v = Element::new(qualify(prefix, "v"));
        v.children.push(Node::Text(text));
        Node::Element(v)
    };
    match value {
        CellValue::Empty => {
            style?;
        }
        CellValue::Number(n) => c.children.push(v(format!("{}", n))),
        CellValue::Bool(b) => {
            c.attrs.push(("t".into(), "b".into()));
            c.children.push(v(if *b { "1" } else { "0" }.into()));
        }
        CellValue::Error(e) => {
            c.attrs.push(("t".into(), "e".into()));
            c.children.push(v(e.clone()));
        }
        CellValue::Text(s) => {
            c.attrs.push(("t".into(), "inlineStr".into()));
            let mut is = Element::new(qualify(prefix, "is"));
            let mut t = Element::new(qualify(prefix, "t"));
            if s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) || s.contains('\n') {
                t.attrs.push(("xml:space".into(), "preserve".into()));
            }
            t.children.push(Node::Text(s.clone()));
            is.children.push(Node::Element(t));
            c.children.push(Node::Element(is));
        }
    }
    Some(c)
}

/// The cells of the sheet holding a shared formula child of `si`, in document order (row, col),
/// skipping (`skip_r`, `skip_c`).
fn shared_children(sd: &Element, si: &str, skip: (u32, u32)) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for (ri, r) in row_positions(sd) {
        let row = elem(sd, ri);
        for (ci, c) in cell_positions(row) {
            if (r, c) == skip {
                continue;
            }
            let cell = elem(row, ci);
            if let Some(f) = cell.child("f") {
                if f.attr("t") == Some("shared") && f.attr("si") == Some(si) && f.attr("ref").is_none() {
                    out.push((r, c));
                }
            }
        }
    }
    out
}

/// Sets (or clears, with `CellValue::Empty`) one cell in a worksheet tree.
pub fn set_cell(root: &mut Element, row: u32, col: u32, value: &CellValue) -> Result<Outcome, String> {
    let sdi = sheet_data_index(root).ok_or("the worksheet has no sheetData")?;
    let mut out = Outcome::default();
    let (prefix, transfer) = {
        let sd = elem(root, sdi);
        let prefix = sd.prefix().map(str::to_string);
        // Is the cell being replaced the master of a shared formula?
        let mut transfer: Option<(u32, u32, String, CellRange)> = None;
        if let Some((ri, _)) = row_positions(sd).into_iter().find(|p| p.1 == row) {
            let r = elem(sd, ri);
            if let Some((ci, _)) = cell_positions(r).into_iter().find(|p| p.1 == col) {
                if let Some(f) = elem(r, ci).child("f") {
                    if let (Some("shared"), Some(si), Some(_)) = (f.attr("t"), f.attr("si"), f.attr("ref")) {
                        let text = f.text();
                        let kids = shared_children(sd, si, (row, col));
                        if let Some(&(nr, nc)) = kids.first() {
                            let mut bbox = CellRange::cell(nr, nc);
                            for &(kr, kc) in &kids {
                                bbox = bbox.union(&CellRange::cell(kr, kc));
                            }
                            let moved = shift_formula(&text, nr as i64 - row as i64, nc as i64 - col as i64);
                            transfer = Some((nr, nc, moved, bbox));
                        }
                    }
                }
            }
        }
        out.old_rows.push(RowSnap { row, xml: row_xml(sd, row) });
        if let Some((nr, ..)) = &transfer {
            if *nr != row {
                out.old_rows.push(RowSnap { row: *nr, xml: row_xml(sd, *nr) });
            }
        }
        (prefix, transfer)
    };

    let sd = elem_mut(root, sdi);
    // The master's text and range move to the first remaining child.
    if let Some((nr, nc, text, bbox)) = &transfer {
        if let Some((ri, _)) = row_positions(sd).into_iter().find(|p| p.1 == *nr) {
            let r = elem_mut(sd, ri);
            if let Some((ci, _)) = cell_positions(r).into_iter().find(|p| p.1 == *nc) {
                if let Some(f) = elem_mut(r, ci).child_mut("f") {
                    f.set_attr("ref", bbox.to_string());
                    f.children = vec![Node::Text(text.clone())];
                }
            }
        }
    }

    let positions = row_positions(sd);
    match positions.iter().find(|p| p.1 == row).copied() {
        Some((ri, _)) => {
            let r = elem_mut(sd, ri);
            if r.attr("r").is_none() {
                r.set_attr("r", (row + 1).to_string());
            }
            let cells = cell_positions(r);
            // Give every cell of the row its explicit reference before cells move around.
            for &(ci, c) in &cells {
                let e = elem_mut(r, ci);
                if e.attr("r").is_none() {
                    e.attrs.insert(0, ("r".into(), cell_name(row, c)));
                }
            }
            let old = cells.iter().find(|p| p.1 == col).copied();
            let style = old.and_then(|(ci, _)| elem(r, ci).attr("s").map(str::to_string));
            let new = new_cell(&prefix, row, col, style.as_deref(), value);
            match (old, new) {
                (Some((ci, _)), Some(n)) => r.children[ci] = Node::Element(n),
                (Some((ci, _)), None) => {
                    r.children.remove(ci);
                }
                (None, Some(n)) => {
                    let at = cells.iter().find(|p| p.1 > col).map(|p| p.0).unwrap_or(r.children.len());
                    r.children.insert(at, Node::Element(n));
                }
                (None, None) => {}
            }
            if let Some(sp) = r.attr("spans").map(str::to_string) {
                if let Some((a, b)) = sp.split_once(':') {
                    if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                        let (na, nb) = (a.min(col + 1), b.max(col + 1));
                        if (na, nb) != (a, b) && !value.is_empty() {
                            r.set_attr("spans", format!("{na}:{nb}"));
                        }
                    }
                }
            }
        }
        None => {
            if let Some(n) = new_cell(&prefix, row, col, None, value) {
                // Rows after the new one that rely on implicit numbering get theirs written down.
                for &(ri, rn) in &positions {
                    if rn > row {
                        let e = elem_mut(sd, ri);
                        if e.attr("r").is_none() {
                            e.attrs.insert(0, ("r".into(), (rn + 1).to_string()));
                        }
                    }
                }
                let mut nrow = Element::new(qualify(&prefix, "row"));
                nrow.attrs.push(("r".into(), (row + 1).to_string()));
                nrow.children.push(Node::Element(n));
                let at = positions.iter().find(|p| p.1 > row).map(|p| p.0).unwrap_or(sd.children.len());
                sd.children.insert(at, Node::Element(nrow));
            }
        }
    }

    for snap in out.old_rows.clone() {
        out.new_rows.push(RowSnap { row: snap.row, xml: row_xml(elem(root, sdi), snap.row) });
    }

    // The dimension grows to hold a new value.
    if !value.is_empty() {
        if let Some(d) = root.child_mut("dimension") {
            if let Some(old) = d.attr("ref").map(str::to_string) {
                if let Some(range) = CellRange::parse(&old) {
                    let grown = range.union(&CellRange::cell(row, col)).to_string();
                    if grown != old {
                        d.set_attr("ref", grown.clone());
                        out.old_dimension = Some(old);
                        out.new_dimension = Some(grown);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Puts rows back as snapshotted (`xml: None` removes the row), and the dimension if given.
pub fn restore_rows(root: &mut Element, rows: &[RowSnap], dimension: Option<&str>, ns: &[(String, String)]) -> Result<Outcome, String> {
    let sdi = sheet_data_index(root).ok_or("the worksheet has no sheetData")?;
    let mut parsed = Vec::new();
    for s in rows {
        parsed.push(match &s.xml {
            Some(x) => Some(from_xml(x, ns)?),
            None => None,
        });
    }
    let mut out = Outcome::default();
    for (s, el) in rows.iter().zip(parsed) {
        let sd = elem_mut(root, sdi);
        out.old_rows.push(RowSnap { row: s.row, xml: row_xml(sd, s.row) });
        let positions = row_positions(sd);
        match (positions.iter().find(|p| p.1 == s.row).copied(), el) {
            (Some((ri, _)), Some(e)) => sd.children[ri] = Node::Element(e),
            (Some((ri, _)), None) => {
                sd.children.remove(ri);
            }
            (None, Some(e)) => {
                let at = positions.iter().find(|p| p.1 > s.row).map(|p| p.0).unwrap_or(sd.children.len());
                sd.children.insert(at, Node::Element(e));
            }
            (None, None) => {}
        }
        out.new_rows.push(RowSnap { row: s.row, xml: s.xml.clone() });
    }
    if let Some(dim) = dimension {
        if let Some(d) = root.child_mut("dimension") {
            let old = d.attr("ref").map(str::to_string);
            if old.as_deref() != Some(dim) {
                d.set_attr("ref", dim);
                out.old_dimension = old;
                out.new_dimension = Some(dim.to_string());
            }
        }
    }
    Ok(out)
}
