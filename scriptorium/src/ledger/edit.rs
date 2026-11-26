//! Writing cells back into a worksheet part. Edits work on the part's tree through
//! `opc::Package::edit_xml` (patch semantics: every row, cell and element the edit leaves equal is
//! copied byte for byte), touching only the rows they must. Each edit snapshots the rows it changes
//! as XML text, so its inverse can put them back exactly.
//!
//! A string goes into the shared string table when the workbook has one (the caller hands the
//! index), else it is written as an inline string. A write that touches a cell of a shared formula
//! first turns that group into plain formulas, so every other cell keeps its own formula.

use std::collections::{BTreeMap, BTreeSet, HashMap};

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

pub fn qualify(prefix: &Option<String>, local: &str) -> String {
    match prefix {
        Some(p) => format!("{p}:{local}"),
        None => local.to_string(),
    }
}

pub fn sheet_data_index(root: &Element) -> Option<usize> {
    root.children.iter().position(|n| matches!(n, Node::Element(e) if e.local() == "sheetData"))
}

/// (child index, 0-based row) of each `<row>`, implicit numbers filled in.
pub fn row_positions(sd: &Element) -> Vec<(usize, u32)> {
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
pub fn cell_positions(row: &Element) -> Vec<(usize, u32)> {
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

pub fn elem_mut(parent: &mut Element, i: usize) -> &mut Element {
    match &mut parent.children[i] {
        Node::Element(e) => e,
        _ => unreachable!("index of an element"),
    }
}

pub fn elem(parent: &Element, i: usize) -> &Element {
    match &parent.children[i] {
        Node::Element(e) => e,
        _ => unreachable!("index of an element"),
    }
}

pub fn row_xml(sd: &Element, row: u32) -> Option<String> {
    row_positions(sd).into_iter().find(|p| p.1 == row).map(|(i, _)| to_xml(elem(sd, i)))
}

/// What one cell becomes: its value, formula (text without `=`) and style. A text value with a
/// shared-string index is written as `t="s"`, without one as an inline string.
#[derive(Debug, Clone, PartialEq)]
pub struct CellSpec {
    pub value: CellValue,
    pub formula: Option<String>,
    pub style: u32,
    pub sst: Option<usize>,
}

impl CellSpec {
    /// True when the cell would hold nothing at all (then it is removed).
    pub fn is_blank(&self) -> bool {
        self.value.is_empty() && self.formula.is_none() && self.style == 0
    }
}

/// One cell to write; `spec: None` removes the cell.
#[derive(Debug, Clone, PartialEq)]
pub struct CellWrite {
    pub row: u32,
    pub col: u32,
    pub spec: Option<CellSpec>,
}

/// SpreadsheetML's `_xHHHH_` escapes for characters XML cannot hold (and a literal `_xHHHH_`).
pub fn encode_escapes(s: &str) -> String {
    let bad = |c: char| (c < ' ' && c != '\t' && c != '\n' && c != '\r') || c == '\u{FFFE}' || c == '\u{FFFF}';
    if !s.chars().any(bad) && !s.contains("_x") {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 8);
    for (i, c) in s.char_indices() {
        if bad(c) {
            out.push_str(&format!("_x{:04X}_", c as u32));
        } else if c == '_' && i + 6 < b.len() && b[i + 1] == b'x' && b[i + 6] == b'_' && s[i + 2..i + 6].bytes().all(|h| h.is_ascii_hexdigit()) {
            out.push_str("_x005F_");
        } else {
            out.push(c);
        }
    }
    out
}

/// A number as `<v>` text (shortest round trip).
pub fn number_text(n: f64) -> String {
    let a = n.abs();
    if a != 0.0 && !(1e-7..1e17).contains(&a) {
        let s = format!("{:E}", n);
        match s.find('E') {
            Some(i) if !s[i + 1..].starts_with('-') => format!("{}E+{}", &s[..i], &s[i + 1..]),
            _ => s,
        }
    } else {
        format!("{}", n)
    }
}

fn text_node(prefix: &Option<String>, local: &str, text: String) -> Element {
    let mut e = Element::new(qualify(prefix, local));
    if local == "t" && (text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) || text.contains('\n')) {
        e.attrs.push(("xml:space".into(), "preserve".into()));
    }
    e.children.push(Node::Text(text));
    e
}

/// The value children of a cell (`<v>` or `<is>`) and its `t`.
fn value_nodes(prefix: &Option<String>, value: &CellValue, has_formula: bool, sst: Option<usize>) -> (Option<&'static str>, Vec<Node>) {
    match value {
        CellValue::Empty => (None, Vec::new()),
        CellValue::Number(n) => (None, vec![Node::Element(text_node(prefix, "v", number_text(*n)))]),
        CellValue::Bool(b) => (Some("b"), vec![Node::Element(text_node(prefix, "v", if *b { "1" } else { "0" }.into()))]),
        CellValue::Error(e) => (Some("e"), vec![Node::Element(text_node(prefix, "v", e.clone()))]),
        CellValue::Text(s) if has_formula => (Some("str"), vec![Node::Element(text_node(prefix, "v", encode_escapes(s)))]),
        CellValue::Text(s) => match sst {
            Some(i) => (Some("s"), vec![Node::Element(text_node(prefix, "v", i.to_string()))]),
            None => {
                let mut is = Element::new(qualify(prefix, "is"));
                is.children.push(Node::Element(text_node(prefix, "t", encode_escapes(s))));
                (Some("inlineStr"), vec![Node::Element(is)])
            }
        },
    }
}

/// Makes `c` hold `spec`, keeping its other attributes and children (patch semantics).
fn fill_cell(c: &mut Element, prefix: &Option<String>, spec: &CellSpec, sst: &[String]) {
    // Text that is already this shared string stays as it is (a rich string keeps its runs).
    if let (CellValue::Text(text), None, Some("s")) = (&spec.value, &spec.formula, c.attr("t")) {
        let old = c.child("v").and_then(|v| v.text().trim().parse::<usize>().ok());
        if old.and_then(|i| sst.get(i)) == Some(text) {
            if spec.style == 0 {
                c.attrs.retain(|(k, _)| k != "s");
            } else {
                c.set_attr("s", spec.style.to_string());
            }
            c.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "f"));
            return;
        }
    }
    if spec.style == 0 {
        c.attrs.retain(|(k, _)| k != "s");
    } else {
        c.set_attr("s", spec.style.to_string());
    }
    let (t, nodes) = value_nodes(prefix, &spec.value, spec.formula.is_some(), spec.sst);
    match t {
        Some(t) => c.set_attr("t", t),
        None => c.attrs.retain(|(k, _)| k != "t"),
    }
    // A formula element that already says this keeps its attributes (an array's ref, say).
    let old_f = c.children.iter().find_map(|n| match n {
        Node::Element(e) if e.local() == "f" => Some(e.clone()),
        _ => None,
    });
    let old_value: Vec<Node> = c.children.iter().filter(|n| matches!(n, Node::Element(e) if matches!(e.local(), "v" | "is"))).cloned().collect();
    c.children.retain(|n| !matches!(n, Node::Element(e) if matches!(e.local(), "f" | "v" | "is")));
    let mut head = Vec::new();
    if let Some(f) = &spec.formula {
        let keep = old_f.filter(|e| e.attr("t") != Some("shared") && e.text() == *f);
        head.push(Node::Element(keep.unwrap_or_else(|| text_node(prefix, "f", f.clone()))));
    }
    // The same value written the same way keeps its exact element.
    let same = old_value.len() == nodes.len() && old_value.iter().zip(&nodes).all(|(a, b)| match (a, b) {
        (Node::Element(x), Node::Element(y)) => x.local() == y.local() && x.text() == y.text(),
        _ => false,
    });
    head.extend(if same { old_value } else { nodes });
    let rest = std::mem::take(&mut c.children);
    c.children = head;
    c.children.extend(rest);
}

fn new_cell(prefix: &Option<String>, row: u32, col: u32, spec: &CellSpec) -> Element {
    let mut c = Element::new(qualify(prefix, "c"));
    c.attrs.push(("r".into(), cell_name(row, col)));
    fill_cell(&mut c, prefix, spec, &[]);
    c
}

/// The shared formula groups (`si`) of a sheet whose cells are in `cells` (all groups for `None`).
pub fn shared_groups(sd: &Element, cells: Option<&BTreeSet<(u32, u32)>>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (ri, r) in row_positions(sd) {
        let row = elem(sd, ri);
        for (ci, c) in cell_positions(row) {
            if cells.is_some_and(|set| !set.contains(&(r, c))) {
                continue;
            }
            if let Some(f) = elem(row, ci).child("f") {
                if f.attr("t") == Some("shared") {
                    if let Some(si) = f.attr("si") {
                        if !out.iter().any(|s| s == si) {
                            out.push(si.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}

/// Every cell of the shared groups `sis` (row, col, si) and the master of each group.
fn group_cells(sd: &Element, sis: &[String]) -> (Vec<(u32, u32, String)>, HashMap<String, (u32, u32, String)>) {
    let mut cells = Vec::new();
    let mut masters = HashMap::new();
    for (ri, r) in row_positions(sd) {
        let row = elem(sd, ri);
        for (ci, c) in cell_positions(row) {
            let Some(f) = elem(row, ci).child("f") else { continue };
            if f.attr("t") != Some("shared") {
                continue;
            }
            let Some(si) = f.attr("si") else { continue };
            if !sis.iter().any(|s| s == si) {
                continue;
            }
            if f.attr("ref").is_some() {
                masters.insert(si.to_string(), (r, c, f.text()));
            }
            cells.push((r, c, si.to_string()));
        }
    }
    (cells, masters)
}

/// The rows holding cells of the shared groups `sis`.
pub fn group_rows(sd: &Element, sis: &[String]) -> Vec<u32> {
    let (cells, _) = group_cells(sd, sis);
    let mut rows: Vec<u32> = cells.iter().map(|c| c.0).collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Turns the shared formula groups `sis` into plain formulas, one per cell (its text worked out
/// from the master's).
pub fn explode_shared(sd: &mut Element, sis: &[String]) {
    if sis.is_empty() {
        return;
    }
    let (cells, masters) = group_cells(sd, sis);
    for (r, c, si) in cells {
        let Some((mr, mc, text)) = masters.get(&si) else { continue };
        let expanded = shift_formula(text, r as i64 - *mr as i64, c as i64 - *mc as i64);
        let Some((ri, _)) = row_positions(sd).into_iter().find(|p| p.1 == r) else { continue };
        let row = elem_mut(sd, ri);
        let Some((ci, _)) = cell_positions(row).into_iter().find(|p| p.1 == c) else { continue };
        if let Some(f) = elem_mut(row, ci).child_mut("f") {
            f.attrs.retain(|(k, _)| k != "t" && k != "si" && k != "ref");
            f.children = vec![Node::Text(expanded)];
        }
    }
}

fn attr_only_row(r: &Element) -> bool {
    r.attrs.iter().all(|(k, _)| k == "r" || k == "spans" || k.ends_with("dyDescent")) && r.children.iter().all(|n| !matches!(n, Node::Element(_)))
}

/// The child index of row `row`, made when missing (rows after it that rely on implicit numbering
/// get theirs written down first).
fn ensure_row(sd: &mut Element, prefix: &Option<String>, row: u32) -> usize {
    let positions = row_positions(sd);
    let ri = match positions.iter().find(|p| p.1 == row) {
        Some(&(ri, _)) => ri,
        None => {
            for &(i, rn) in &positions {
                if rn > row {
                    let e = elem_mut(sd, i);
                    if e.attr("r").is_none() {
                        e.attrs.insert(0, ("r".into(), (rn + 1).to_string()));
                    }
                }
            }
            let mut nrow = Element::new(qualify(prefix, "row"));
            nrow.attrs.push(("r".into(), (row + 1).to_string()));
            let at = positions.iter().find(|p| p.1 > row).map(|p| p.0).unwrap_or(sd.children.len());
            sd.children.insert(at, Node::Element(nrow));
            at
        }
    };
    let r = elem_mut(sd, ri);
    if r.attr("r").is_none() {
        r.attrs.insert(0, ("r".into(), (row + 1).to_string()));
    }
    ri
}

/// Writes cells into a worksheet tree: each one set (keeping what the cell element carries that
/// is not its value, formula or style) or removed. Shared formula groups a write touches are
/// turned into plain formulas first. Every row touched is snapshotted before and after.
pub fn write_cells(root: &mut Element, writes: &[CellWrite], sst: &[String]) -> Result<Outcome, String> {
    let sdi = sheet_data_index(root).ok_or("the worksheet has no sheetData")?;
    let mut out = Outcome::default();
    let targets: BTreeSet<(u32, u32)> = writes.iter().map(|w| (w.row, w.col)).collect();
    let prefix = elem(root, sdi).prefix().map(str::to_string);
    let sis = shared_groups(elem(root, sdi), Some(&targets));
    let mut touched: BTreeSet<u32> = writes.iter().map(|w| w.row).collect();
    touched.extend(group_rows(elem(root, sdi), &sis));
    {
        let sd = elem(root, sdi);
        for &r in &touched {
            out.old_rows.push(RowSnap { row: r, xml: row_xml(sd, r) });
        }
    }
    let sd = elem_mut(root, sdi);
    explode_shared(sd, &sis);

    let mut by_row: BTreeMap<u32, Vec<&CellWrite>> = BTreeMap::new();
    for w in writes {
        by_row.entry(w.row).or_default().push(w);
    }
    for (row, list) in by_row {
        let exists = row_positions(sd).iter().any(|p| p.1 == row);
        if !exists && list.iter().all(|w| w.spec.as_ref().map_or(true, CellSpec::is_blank)) {
            continue;
        }
        let ri = ensure_row(sd, &prefix, row);
        let r = elem_mut(sd, ri);
        // Explicit references on every cell before cells move around.
        for (ci, c) in cell_positions(r) {
            let e = elem_mut(r, ci);
            if e.attr("r").is_none() {
                e.attrs.insert(0, ("r".into(), cell_name(row, c)));
            }
        }
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        for w in &list {
            let cells = cell_positions(r);
            let old = cells.iter().find(|p| p.1 == w.col).copied();
            match (&w.spec, old) {
                (Some(spec), Some((ci, _))) if !spec.is_blank() => fill_cell(elem_mut(r, ci), &prefix, spec, sst),
                (Some(spec), None) if !spec.is_blank() => {
                    let at = cells.iter().find(|p| p.1 > w.col).map(|p| p.0).unwrap_or(r.children.len());
                    r.children.insert(at, Node::Element(new_cell(&prefix, row, w.col, spec)));
                }
                (_, Some((ci, _))) => {
                    r.children.remove(ci);
                }
                _ => {}
            }
            if w.spec.as_ref().is_some_and(|s| !s.is_blank()) {
                lo = lo.min(w.col + 1);
                hi = hi.max(w.col + 1);
            }
        }
        if lo <= hi {
            if let Some(sp) = r.attr("spans").map(str::to_string) {
                if let Some((a, b)) = sp.split_once(':') {
                    if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                        let (na, nb) = (a.min(lo), b.max(hi));
                        if (na, nb) != (a, b) {
                            r.set_attr("spans", format!("{na}:{nb}"));
                        }
                    }
                }
            }
        }
        // A row this write emptied and that carries nothing of its own goes.
        if !exists && attr_only_row(r) {
            sd.children.remove(ri);
        }
    }

    for snap in out.old_rows.clone() {
        out.new_rows.push(RowSnap { row: snap.row, xml: row_xml(elem(root, sdi), snap.row) });
    }
    // The dimension grows to hold the new values.
    let mut grow: Option<CellRange> = None;
    for w in writes {
        if w.spec.as_ref().is_some_and(|s| !s.is_blank()) {
            let here = CellRange::cell(w.row, w.col);
            grow = Some(grow.map_or(here, |g| g.union(&here)));
        }
    }
    if let Some(g) = grow {
        if let Some(d) = root.child_mut("dimension") {
            if let Some(old) = d.attr("ref").map(str::to_string) {
                if let Some(range) = CellRange::parse(&old) {
                    let grown = range.union(&g).to_string();
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

/// Writes calculated values into formula cells (their formulas stay). Answers the rows changed.
pub fn write_values(root: &mut Element, values: &[(u32, u32, CellValue)]) -> Vec<u32> {
    let Some(sdi) = sheet_data_index(root) else { return Vec::new() };
    let sd = elem_mut(root, sdi);
    let prefix = sd.prefix().map(str::to_string);
    let positions = row_positions(sd);
    let mut rows = Vec::new();
    for (r, c, v) in values {
        let Some(&(ri, _)) = positions.iter().find(|p| p.1 == *r) else { continue };
        let row = elem_mut(sd, ri);
        let Some((ci, _)) = cell_positions(row).into_iter().find(|p| p.1 == *c) else { continue };
        let cell = elem_mut(row, ci);
        if cell.child("f").is_none() {
            continue;
        }
        let (t, nodes) = value_nodes(&prefix, v, true, None);
        match t {
            Some(t) => cell.set_attr("t", t),
            None => cell.attrs.retain(|(k, _)| k != "t"),
        }
        cell.children.retain(|n| !matches!(n, Node::Element(e) if matches!(e.local(), "v" | "is")));
        let at = cell.children.iter().position(|n| matches!(n, Node::Element(e) if e.local() == "f")).map(|i| i + 1).unwrap_or(0);
        for (k, n) in nodes.into_iter().enumerate() {
            cell.children.insert(at + k, n);
        }
        if !rows.contains(r) {
            rows.push(*r);
        }
    }
    rows
}

/// Changes the `<row>` elements of `rows` with `f` (height, hidden, style), making rows as needed
/// and dropping rows left with nothing. Snapshots before and after.
pub fn edit_rows(root: &mut Element, rows: &[u32], f: &dyn Fn(&mut Element)) -> Result<Outcome, String> {
    let sdi = sheet_data_index(root).ok_or("the worksheet has no sheetData")?;
    let mut out = Outcome::default();
    let prefix = elem(root, sdi).prefix().map(str::to_string);
    for &row in rows {
        let sd = elem_mut(root, sdi);
        out.old_rows.push(RowSnap { row, xml: row_xml(sd, row) });
        let existed = row_positions(sd).iter().any(|p| p.1 == row);
        let ri = ensure_row(sd, &prefix, row);
        let r = elem_mut(sd, ri);
        f(r);
        if attr_only_row(r) && (!existed || cell_positions(r).is_empty()) {
            sd.children.remove(ri);
        }
        out.new_rows.push(RowSnap { row, xml: row_xml(elem(root, sdi), row) });
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
