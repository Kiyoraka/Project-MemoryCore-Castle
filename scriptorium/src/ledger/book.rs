//! The workbook's sheets: added, renamed, deleted and moved, with `workbook.xml`, its
//! relationships and the content types kept in step, sheet-scoped names (`localSheetId`) and the
//! active tab following, and every formula that names a sheet rewritten. Also CSV: a `.csv` opened
//! as a one-sheet workbook, CSV text put into a sheet, a sheet written out as CSV.

use super::cellref::{CellRange, MAX_COLS, MAX_ROWS};
use super::edit::qualify;
use super::entry::parse_input;
use super::model::CellValue;
use super::refs;
use super::LedgerDoc;
use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command};
use crate::ground::opc;
use crate::ground::text::FontCache;
use crate::ground::xml::{Element, Node};

fn bad(cmd: &str, msg: impl Into<String>) -> BusError {
    BusError::bad_args(cmd, msg)
}

/// Why a sheet name cannot be used (`None` when it can).
pub fn name_problem(name: &str) -> Option<&'static str> {
    if name.trim().is_empty() {
        return Some("a sheet name cannot be empty");
    }
    if name.chars().count() > 31 {
        return Some("a sheet name has at most 31 characters");
    }
    if name.contains(['[', ']', ':', '*', '?', '/', '\\']) {
        return Some("a sheet name cannot hold [ ] : * ? / or \\");
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return Some("a sheet name cannot begin or end with an apostrophe");
    }
    if name.eq_ignore_ascii_case("history") {
        return Some("History is a reserved sheet name");
    }
    None
}

/// The blank worksheet a new sheet starts as.
pub fn blank_sheet_xml() -> String {
    format!(
        "{}<worksheet xmlns=\"{}\" xmlns:r=\"{}\"><dimension ref=\"A1\"/><sheetViews><sheetView workbookViewId=\"0\"/></sheetViews>\
<sheetFormatPr defaultRowHeight=\"15\"/><sheetData/><pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>",
        super::DECL,
        super::NS_MAIN,
        super::NS_R
    )
}

/// The prefix the workbook binds to the relationships namespace (adding `r` when it has none).
fn rel_prefix(root: &mut Element) -> String {
    for (k, v) in &root.attrs {
        if v == super::NS_R {
            if let Some(p) = k.strip_prefix("xmlns:") {
                return p.to_string();
            }
        }
    }
    root.attrs.push(("xmlns:r".into(), super::NS_R.into()));
    "r".into()
}

fn sheets_el(root: &mut Element) -> Option<&mut Element> {
    root.child_mut("sheets")
}

/// `localSheetId` of every defined name remapped by `f` (`None` drops the name).
fn remap_local_ids(root: &mut Element, f: &dyn Fn(usize) -> Option<usize>) {
    if let Some(dn) = root.child_mut("definedNames") {
        dn.children.retain_mut(|n| {
            let Node::Element(d) = n else { return true };
            let Some(id) = d.attr("localSheetId").and_then(|v| v.parse::<usize>().ok()) else { return true };
            match f(id) {
                Some(nid) => {
                    if nid != id {
                        d.set_attr("localSheetId", nid.to_string());
                    }
                    true
                }
                None => false,
            }
        });
    }
    if root.child("definedNames").is_some_and(|d| d.elements().next().is_none()) {
        root.children.retain(|n| !matches!(n, Node::Element(e) if e.local() == "definedNames"));
    }
}

/// `activeTab` and `firstSheet` of the book views remapped by `f`.
fn remap_tabs(root: &mut Element, f: &dyn Fn(usize) -> usize) {
    if let Some(bv) = root.child_mut("bookViews") {
        for n in bv.children.iter_mut() {
            let Node::Element(v) = n else { continue };
            for a in ["activeTab", "firstSheet"] {
                if let Some(i) = v.attr(a).and_then(|x| x.parse::<usize>().ok()) {
                    let ni = f(i);
                    if ni != i {
                        v.set_attr(a, ni.to_string());
                    }
                }
            }
        }
    }
}

impl LedgerDoc {
    fn unique_sheet_name(&self) -> String {
        (1..).map(|i| format!("Sheet{i}")).find(|n| self.sheet_by_name(n).is_none()).unwrap_or_else(|| "Sheet".into())
    }

    fn wb_parts(&self) -> Vec<String> {
        vec![self.workbook_part.clone(), opc::rels_part_name(&self.workbook_part), opc::CONTENT_TYPES_NAME.to_string()]
    }

    pub(super) fn cmd_add_sheet(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let title = match cmd.args.get("name").and_then(Json::as_str) {
            Some(n) => n.trim().to_string(),
            None => self.unique_sheet_name(),
        };
        if let Some(p) = name_problem(&title) {
            return Err(bad(name, p));
        }
        if self.sheet_by_name(&title).is_some() {
            return Err(BusError::failed(format!("there is already a sheet named '{title}'")));
        }
        let at = cmd.args.get("at").and_then(Json::as_usize).unwrap_or(self.sheets.len()).min(self.sheets.len());
        let dir = {
            let wb = &self.workbook_part;
            match wb.rfind('/') {
                Some(i) => wb[..i].to_string(),
                None => String::new(),
            }
        };
        let part = (1..).map(|k| format!("{dir}/worksheets/sheet{k}.xml")).find(|p| self.pkg.part(p).is_none()).unwrap_or_default();
        let mut names = self.wb_parts();
        names.push(part.clone());
        let wb = self.workbook_part.clone();
        self.commit_parts(names, move |doc| {
            doc.pkg.add_part(&part, opc::CT_WORKSHEET, blank_sheet_xml().into_bytes()).map_err(|e| BusError::failed(e.0))?;
            let rid = doc.pkg.add_relationship(&wb, opc::REL_WORKSHEET, &part, false);
            let mut res = Ok(());
            doc.pkg
                .edit_xml(&wb, |root| {
                    let rp = rel_prefix(root);
                    let prefix = root.prefix().map(str::to_string);
                    let Some(list) = sheets_el(root) else {
                        res = Err(BusError::failed("the workbook has no sheet list"));
                        return;
                    };
                    let max_id = list.children_named("sheet").filter_map(|s| s.attr("sheetId").and_then(|v| v.parse::<u32>().ok())).max().unwrap_or(0);
                    let mut s = Element::new(qualify(&prefix, "sheet"));
                    s.attrs.push(("name".into(), title.clone()));
                    s.attrs.push(("sheetId".into(), (max_id + 1).to_string()));
                    s.attrs.push((format!("{rp}:id"), rid.clone()));
                    let pos: Vec<usize> = list.children.iter().enumerate().filter(|(_, n)| matches!(n, Node::Element(e) if e.local() == "sheet")).map(|(i, _)| i).collect();
                    let idx = pos.get(at).copied().unwrap_or_else(|| pos.last().map(|i| i + 1).unwrap_or(list.children.len()));
                    list.children.insert(idx, Node::Element(s));
                    remap_local_ids(root, &|i| Some(if i >= at { i + 1 } else { i }));
                    remap_tabs(root, &|i| if i >= at { i + 1 } else { i });
                })
                .map_err(|e| BusError::failed(e.0))?;
            res
        })
    }

    pub(super) fn cmd_rename_sheet(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let new = cmd.args.get("name").and_then(Json::as_str).map(str::trim).ok_or_else(|| bad(name, "\"name\" is the sheet's new name"))?.to_string();
        if let Some(p) = name_problem(&new) {
            return Err(bad(name, p));
        }
        let old = self.sheets[si].name.clone();
        if old == new {
            return Ok(Change::default());
        }
        if self.sheet_by_name(&new).is_some_and(|i| i != si) {
            return Err(BusError::failed(format!("there is already a sheet named '{new}'")));
        }
        let names = self.book_parts();
        let wb = self.workbook_part.clone();
        self.commit_parts(names, move |doc| {
            doc.rewrite_all(&|f, _home| refs::rename_sheet(f, &old, &new), &|f, _| refs::refs_of(f).iter().any(|r| r.sheet.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&old))))?;
            doc.pkg
                .edit_xml(&wb, |root| {
                    if let Some(list) = sheets_el(root) {
                        if let Some(Node::Element(s)) = list.children.iter_mut().filter(|n| matches!(n, Node::Element(e) if e.local() == "sheet")).nth(si) {
                            s.set_attr("name", new.clone());
                        }
                    }
                    super::structure::rewrite_names(root, &|f| refs::rename_sheet(f, &old, &new));
                })
                .map_err(|e| BusError::failed(e.0))?;
            Ok(())
        })
    }

    /// Every formula of every worksheet rewritten with `f` (given the formula and its sheet's
    /// name) where `needs` says so.
    pub(super) fn rewrite_all(&mut self, f: &dyn Fn(&str, &str) -> String, needs: &dyn Fn(&str, &str) -> bool) -> Result<(), BusError> {
        for i in 0..self.sheets.len() {
            let Some(part) = self.sheets[i].part.clone().filter(|p| self.pkg.part(p).is_some()) else { continue };
            let home = self.sheets[i].name.clone();
            self.pkg
                .edit_xml(&part, |root| super::structure::rewrite_formulas(root, &|t| f(t, &home), &|t| needs(t, &home), None))
                .map_err(|e| BusError::failed(e.0))?;
        }
        Ok(())
    }

    pub(super) fn cmd_delete_sheet(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let visible = self.sheets.iter().enumerate().filter(|(i, s)| *i != si && s.state == super::SheetState::Visible).count();
        if visible == 0 {
            return Err(BusError::failed("a workbook keeps at least one visible sheet"));
        }
        let gone = self.sheets[si].name.clone();
        let rid = self.sheets[si].rel_id.clone();
        let wb = self.workbook_part.clone();
        let mut names = self.book_parts();
        names.extend(self.wb_parts());
        let target = self.pkg.rel_by_id(&wb, &rid).filter(|r| !r.target_mode_external).map(|r| self.pkg.resolve(&wb, r));
        if let Some(t) = &target {
            names.push(opc::rels_part_name(t));
        }
        self.commit_parts(names, move |doc| {
            if let Some(t) = &target {
                doc.pkg.remove_part(t);
            } else {
                doc.pkg.remove_relationship(&wb, &rid);
            }
            doc.rewrite_all(&|f, _| refs::drop_sheet(f, &gone), &|f, _| refs::refs_of(f).iter().any(|r| r.sheet.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&gone))))?;
            doc.pkg
                .edit_xml(&wb, |root| {
                    if let Some(list) = sheets_el(root) {
                        let pos: Vec<usize> = list.children.iter().enumerate().filter(|(_, n)| matches!(n, Node::Element(e) if e.local() == "sheet")).map(|(i, _)| i).collect();
                        if let Some(&i) = pos.get(si) {
                            list.children.remove(i);
                        }
                    }
                    remap_local_ids(root, &|i| if i == si { None } else if i > si { Some(i - 1) } else { Some(i) });
                    super::structure::rewrite_names(root, &|f| refs::drop_sheet(f, &gone));
                    remap_tabs(root, &|i| if i > si || (i == si && i > 0) { i - 1 } else { i });
                })
                .map_err(|e| BusError::failed(e.0))?;
            doc.drop_calc_chain();
            Ok(())
        })
    }

    pub(super) fn cmd_move_sheet(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let n = self.sheets.len();
        let to = cmd.args.get("to").and_then(Json::as_usize).ok_or_else(|| bad(name, format!("\"to\" is the new place, from 0 to {}", n - 1)))?.min(n - 1);
        if to == si {
            return Ok(Change::default());
        }
        // Old index -> new index.
        let map = move |i: usize| -> usize {
            if i == si {
                to
            } else if si < to && i > si && i <= to {
                i - 1
            } else if to < si && i >= to && i < si {
                i + 1
            } else {
                i
            }
        };
        let wb = self.workbook_part.clone();
        self.commit_parts(vec![wb.clone()], move |doc| {
            doc.pkg
                .edit_xml(&wb, |root| {
                    if let Some(list) = sheets_el(root) {
                        let pos: Vec<usize> = list.children.iter().enumerate().filter(|(_, n)| matches!(n, Node::Element(e) if e.local() == "sheet")).map(|(i, _)| i).collect();
                        let mut els: Vec<Node> = pos.iter().map(|&i| list.children[i].clone()).collect();
                        let moved = els.remove(si);
                        els.insert(to, moved);
                        for (slot, el) in pos.iter().zip(els) {
                            list.children[*slot] = el;
                        }
                    }
                    remap_local_ids(root, &|i| Some(map(i)));
                    remap_tabs(root, &map);
                })
                .map_err(|e| BusError::failed(e.0))?;
            Ok(())
        })
    }

    // --- CSV -----------------------------------------------------------------------------------

    /// CSV fields as entries: numbers, dates and formulas read as typed, but leading zeros and
    /// long digit strings (codes, IDs) kept as text.
    fn csv_entry(&self, field: &str) -> super::entry::Entry {
        let t = field.trim();
        let digits = !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
        if digits && ((t.len() > 1 && t.starts_with('0')) || t.len() > 15) {
            return super::entry::Entry { value: CellValue::Text(field.to_string()), formula: None, format: None };
        }
        parse_input(field, self.date1904, false)
    }

    pub(super) fn cmd_import_csv(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let text = cmd.args.get("text").and_then(Json::as_str).ok_or_else(|| bad(name, "\"text\" is the CSV text"))?;
        let si = self.sheet_index(name, cmd.args.get("sheet"))?;
        let at = match cmd.args.get("cell").and_then(Json::as_str) {
            Some(c) => super::parse_cell(c).ok_or_else(|| bad(name, format!("'{c}' is not a cell")))?,
            None => (0, 0),
        };
        let sep = match cmd.args.get("separator").and_then(Json::as_str) {
            Some(s) if s.chars().count() == 1 => s.chars().next().unwrap_or(','),
            Some("tab") => '\t',
            _ => super::csv::sniff(text),
        };
        let rows = super::csv::parse_delimited(text, sep);
        let mut list = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            for (j, f) in row.iter().enumerate() {
                let (r, c) = (at.0 as usize + i, at.1 as usize + j);
                if r >= MAX_ROWS as usize || c >= MAX_COLS as usize {
                    return Err(BusError::failed("the CSV does not fit on the sheet"));
                }
                if f.is_empty() {
                    continue;
                }
                list.push((r as u32, c as u32, self.csv_entry(f)));
            }
        }
        if list.len() as u64 > super::ops::WRITE_LIMIT {
            return Err(BusError::failed("the CSV holds too many cells for one change"));
        }
        self.worksheet_part(si)?;
        self.put_entries(si, list)
    }

    /// Opens CSV (or tab-separated text) as a workbook of one sheet named `sheet_name`.
    pub fn open_csv(bytes: &[u8], sheet_name: &str, fonts: FontCache) -> Result<LedgerDoc, String> {
        let mut doc = LedgerDoc::new_blank_with_fonts(fonts);
        let text = super::csv::decode(bytes);
        let name = sheet_name.trim();
        if !name.is_empty() && name != "Sheet1" && name_problem(name).is_none() {
            doc.apply_inner(&Command::new("ledger.rename_sheet", Json::obj().with("sheet", 0).with("name", name))).map_err(|e| e.to_string())?;
        }
        doc.apply_inner(&Command::new("ledger.import_csv", Json::obj().with("sheet", 0).with("text", text))).map_err(|e| e.to_string())?;
        Ok(doc)
    }

    /// A sheet's cells as CSV: the shown text of each cell over the used range, comma separated,
    /// UTF-8 (with a byte order mark when `bom`).
    pub fn save_csv(&self, sheet: usize, bom: bool) -> Vec<u8> {
        let rows = self.sheet_rows(sheet);
        super::csv::write(&rows, ',', bom)
    }

    /// The shown text of every cell of a sheet's used range.
    pub fn sheet_rows(&self, sheet: usize) -> Vec<Vec<String>> {
        let Some(s) = self.sheets.get(sheet) else { return Vec::new() };
        let Some(u) = s.used_range() else { return Vec::new() };
        let u = CellRange::new(0, 0, u.r2, u.c2);
        (u.r1..=u.r2)
            .map(|r| {
                let mut row: Vec<String> = (u.c1..=u.c2).map(|c| s.cell(r, c).map(|x| self.formatted(x)).unwrap_or_default()).collect();
                while row.last().is_some_and(String::is_empty) && row.len() > 1 {
                    row.pop();
                }
                row
            })
            .collect()
    }
}
