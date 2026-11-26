//! How a change reaches the file and comes back out of it. Every command ends in one of two
//! commits:
//!
//! - [`LedgerDoc::commit_cells`]: cells written into one worksheet (values, formulas, styles); its
//!   undo puts the touched rows back from their XML snapshots (cheap, exact).
//! - [`LedgerDoc::commit_parts`]: anything wider (structure, sheets, merges, panes, columns); its
//!   undo puts whole parts back (and removes the ones the change made), then the model is read
//!   again from the package.
//!
//! After either, the calculation runs ([`super::calc::recalc`]) and every formula whose value
//! changed gets its cached `<v>` written, so the saved file carries the new results.

use super::calc;
use super::edit::{self, CellWrite, RowSnap};
use super::model::CellValue;
use super::LedgerDoc;
use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command};
use crate::ground::opc;
use crate::ground::xml::{Element, Node};

/// A part as it was: its name and its XML text (`None`: there was no such part).
pub type PartSnap = (String, Option<String>);

impl LedgerDoc {
    /// A part's text now (`None` when the package has no such part).
    pub(super) fn part_text(&self, name: &str) -> Option<String> {
        let bytes = self.pkg.part_bytes(name).ok()?;
        match String::from_utf8(bytes) {
            Ok(s) => Some(s),
            Err(e) => crate::ground::xml::to_utf8(e.as_bytes()).ok().map(|c| String::from_utf8_lossy(&c).into_owned()),
        }
    }

    pub(super) fn snapshot(&self, names: &[String]) -> Vec<PartSnap> {
        let mut out: Vec<PartSnap> = Vec::new();
        for n in names {
            let n = opc::normalize_name(n);
            if !out.iter().any(|(m, _)| opc::same_part(m, &n)) {
                out.push((n.clone(), self.part_text(&n)));
            }
        }
        out
    }

    /// Puts parts back as snapshotted: removals first, then the parts, then relationships, then
    /// the content types (so each later write sees the earlier ones).
    pub(super) fn put_parts(&mut self, parts: &[PartSnap]) -> Result<(), String> {
        let is_rels = |n: &str| n.ends_with(".rels");
        let is_ct = |n: &str| opc::same_part(n, opc::CONTENT_TYPES_NAME);
        for (name, xml) in parts {
            if xml.is_none() && self.pkg.part(name).is_some() {
                self.pkg.remove_part(name);
            }
        }
        for pass in 0..3 {
            for (name, xml) in parts {
                let Some(xml) = xml else { continue };
                let here = match pass {
                    0 => !is_rels(name) && !is_ct(name),
                    1 => is_rels(name),
                    _ => is_ct(name),
                };
                if !here {
                    continue;
                }
                let bytes = xml.clone().into_bytes();
                if self.pkg.part(name).is_some() {
                    if self.pkg.part_bytes(name).ok().as_deref() != Some(bytes.as_slice()) {
                        self.pkg.set_part_bytes(name, bytes).map_err(|e| e.0)?;
                    }
                } else {
                    let ct = self.pkg.content_type(name).unwrap_or(opc::CT_XML).to_string();
                    self.pkg.add_part(name, &ct, bytes).map_err(|e| e.0)?;
                }
            }
        }
        Ok(())
    }

    fn parts_json(parts: &[PartSnap]) -> Json {
        Json::Arr(parts.iter().map(|(n, x)| Json::obj().with("part", n.as_str()).with("xml", x.clone())).collect())
    }

    pub(super) fn rows_json(rows: &[RowSnap]) -> Json {
        Json::Arr(rows.iter().map(|s| Json::obj().with("row", s.row as u64 + 1).with("xml", s.xml.clone())).collect())
    }

    /// The inverse command: rows of one sheet and/or whole parts.
    pub(super) fn restore_cmd(si: Option<usize>, rows: &[RowSnap], dimension: Option<String>, parts: &[PartSnap]) -> Command {
        let mut args = Json::obj();
        if let Some(si) = si {
            args.set("sheet", si);
            args.set("rows", Self::rows_json(rows));
        }
        if let Some(d) = dimension {
            args.set("dimension", d);
        }
        if !parts.is_empty() {
            args.set("parts", Self::parts_json(parts));
        }
        Command::new("ledger.restore_rows", args)
    }

    /// Recalculates (only what depends on `changed`, or everything for `None`) and writes every
    /// changed formula result into its cell's `<v>`.
    pub(super) fn sync_values(&mut self, changed: Option<&[calc::CellKey]>) {
        let names = self.names.clone();
        let opts = calc::Options { names: &names, date1904: self.date1904 };
        let report = match changed {
            Some(keys) => calc::recalc_from_with(&mut self.sheets, keys, &opts),
            None => calc::recalc_all(&mut self.sheets, &opts),
        };
        let mut by_sheet: std::collections::BTreeMap<usize, Vec<(u32, u32, CellValue)>> = Default::default();
        for (si, r, c) in report.changed {
            let v = self.sheets.get(si).and_then(|s| s.cell(r, c)).map(|x| x.value.clone()).unwrap_or(CellValue::Empty);
            by_sheet.entry(si).or_default().push((r, c, v));
        }
        for (si, list) in by_sheet {
            let Some(part) = self.sheets[si].part.clone() else { continue };
            let _ = self.pkg.edit_xml(&part, |root| {
                edit::write_values(root, &list);
            });
        }
    }

    /// Drops `calcChain.xml` (optional; stale once formulas move or go).
    pub(super) fn drop_calc_chain(&mut self) {
        let chains: Vec<String> = self.pkg.related(&self.workbook_part, "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain");
        for c in chains {
            self.pkg.remove_part(&c);
        }
    }

    /// Asks the next program that opens the file to calculate everything again (our results are
    /// written, but a formula we cannot work out yet keeps no value).
    pub(super) fn mark_full_calc(&mut self) {
        if self.full_calc_marked {
            return;
        }
        self.full_calc_marked = true;
        let wb = self.workbook_part.clone();
        let _ = self.pkg.edit_xml(&wb, |root| {
            if let Some(c) = root.child_mut("calcPr") {
                if c.attr("fullCalcOnLoad") != Some("1") {
                    c.set_attr("fullCalcOnLoad", "1");
                }
                return;
            }
            let prefix = root.prefix().map(str::to_string);
            let mut c = Element::new(edit::qualify(&prefix, "calcPr"));
            c.attrs.push(("fullCalcOnLoad".into(), "1".into()));
            let after = ["definedNames", "externalReferences", "functionGroups", "sheets"];
            let at = after
                .iter()
                .find_map(|n| root.children.iter().rposition(|x| matches!(x, Node::Element(e) if e.local() == *n)))
                .map(|i| i + 1)
                .unwrap_or(root.children.len());
            root.children.insert(at, Node::Element(c));
        });
    }

    /// Writes cells into sheet `si` as one change. `extra` are parts snapshotted before the
    /// command changed them (the styles, say), put back by the same undo.
    pub(super) fn commit_cells(&mut self, si: usize, mut writes: Vec<CellWrite>, extra: Vec<PartSnap>) -> Result<Change, BusError> {
        let part = self.worksheet_part(si)?;
        self.intern_writes(si, &mut writes);
        let formulas = writes.iter().any(|w| {
            w.spec.as_ref().is_some_and(|s| s.formula.is_some()) || self.sheets[si].cell(w.row, w.col).is_some_and(|c| c.formula.is_some())
        }) || !self.sheets[si].shared.is_empty();
        if writes.iter().any(|w| w.spec.as_ref().is_some_and(|s| s.formula.is_some())) {
            self.mark_full_calc();
        }
        let mut outcome = None;
        let sst = &self.sst;
        self.pkg.edit_xml(&part, |root| outcome = Some(edit::write_cells(root, &writes, sst))).map_err(|e| BusError::failed(e.0))?;
        let outcome = outcome.ok_or_else(|| BusError::failed("the worksheet could not be read"))?.map_err(BusError::failed)?;
        self.reparse_rows(si, &outcome.new_rows).map_err(BusError::failed)?;
        if let Some(d) = &outcome.new_dimension {
            self.sheets[si].dimension = Some(d.clone());
        }
        if formulas {
            self.drop_calc_chain();
        }
        let extra: Vec<PartSnap> = extra.into_iter().filter(|(n, x)| self.part_text(n) != *x).collect();
        let nothing = outcome.old_rows.iter().zip(&outcome.new_rows).all(|(a, b)| a.xml == b.xml) && extra.is_empty();
        let keys: Vec<calc::CellKey> = writes.iter().map(|w| (si, w.row, w.col)).collect();
        self.sync_values(Some(&keys));
        self.refresh(si);
        if nothing {
            return Ok(Change::default());
        }
        let undo = Self::restore_cmd(Some(si), &outcome.old_rows, outcome.old_dimension, &extra);
        Ok(Change { undo: vec![undo], pages_touched: vec![si] })
    }

    /// Runs `f` (which edits the package) as one change over the parts named; the model is read
    /// again afterwards. Parts `f` creates must be named too (as absent before).
    pub(super) fn commit_parts(&mut self, names: Vec<String>, f: impl FnOnce(&mut LedgerDoc) -> Result<(), BusError>) -> Result<Change, BusError> {
        let before = self.snapshot(&names);
        if let Err(e) = f(self) {
            // Leave the package as it was.
            let _ = self.put_parts(&before);
            let _ = self.reload();
            return Err(e);
        }
        let changed: Vec<PartSnap> = before.into_iter().filter(|(n, x)| self.part_text(n) != *x).collect();
        self.reload().map_err(BusError::failed)?;
        self.sync_values(None);
        if changed.is_empty() {
            return Ok(Change::default());
        }
        let pages = (0..self.sheets.len()).collect();
        Ok(Change { undo: vec![Self::restore_cmd(None, &[], None, &changed)], pages_touched: pages })
    }

    /// Every worksheet part and the workbook (what a structural change may touch).
    pub(super) fn book_parts(&self) -> Vec<String> {
        let mut v: Vec<String> = self.sheets.iter().filter_map(|s| s.part.clone()).collect();
        v.push(self.workbook_part.clone());
        v
    }

    /// `ledger.restore_rows`: the inverse of every change (rows of a sheet and/or whole parts).
    pub(super) fn restore(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let mut parts: Vec<PartSnap> = Vec::new();
        if let Some(list) = cmd.args.get("parts").and_then(Json::as_arr) {
            for p in list {
                let n = p.get("part").and_then(Json::as_str).ok_or_else(|| BusError::bad_args(name, "each part has a \"part\" name"))?;
                parts.push((n.to_string(), p.get("xml").and_then(Json::as_str).map(str::to_string)));
            }
        }
        let names: Vec<String> = parts.iter().map(|p| p.0.clone()).collect();
        let parts_now = self.snapshot(&names);
        if !parts.is_empty() {
            self.put_parts(&parts).map_err(BusError::failed)?;
            self.reload().map_err(BusError::failed)?;
        }
        let mut undo_rows = Vec::new();
        let mut undo_dim = None;
        let mut si_out = None;
        let mut keys: Vec<calc::CellKey> = Vec::new();
        let mut pages: Vec<usize> = if parts.is_empty() { Vec::new() } else { (0..self.sheets.len()).collect() };
        if let Some(list) = cmd.args.get("rows").and_then(Json::as_arr) {
            let si = self.sheet_index(name, cmd.args.get("sheet"))?;
            let mut snaps = Vec::new();
            for item in list {
                let row = item
                    .get("row")
                    .and_then(Json::as_usize)
                    .filter(|&r| r >= 1 && r <= super::MAX_ROWS as usize)
                    .ok_or_else(|| BusError::bad_args(name, "each row has a \"row\" number"))?;
                snaps.push(RowSnap { row: row as u32 - 1, xml: item.get("xml").and_then(Json::as_str).map(str::to_string) });
            }
            let dim = cmd.args.get("dimension").and_then(Json::as_str).map(str::to_string);
            let part = self.worksheet_part(si)?;
            let ns = self.sheets[si].ns_decls.clone();
            let cells_in = |doc: &LedgerDoc, keys: &mut Vec<calc::CellKey>| {
                for s in &snaps {
                    if let Some(row) = doc.sheets[si].rows.get(&s.row) {
                        keys.extend(row.cells.keys().map(|&c| (si, s.row, c)));
                    }
                }
            };
            cells_in(self, &mut keys);
            let mut outcome = None;
            self.pkg
                .edit_xml(&part, |root| outcome = Some(edit::restore_rows(root, &snaps, dim.as_deref(), &ns)))
                .map_err(|e| BusError::failed(e.0))?;
            let outcome = outcome.ok_or_else(|| BusError::failed("the worksheet could not be read"))?.map_err(BusError::failed)?;
            self.reparse_rows(si, &outcome.new_rows).map_err(BusError::failed)?;
            if let Some(d) = &outcome.new_dimension {
                self.sheets[si].dimension = Some(d.clone());
            }
            cells_in(self, &mut keys);
            undo_rows = outcome.old_rows;
            undo_dim = outcome.old_dimension;
            si_out = Some(si);
            if !pages.contains(&si) {
                pages.push(si);
            }
        }
        self.sync_values(if parts.is_empty() { Some(&keys) } else { None });
        for &p in &pages {
            self.refresh(p);
        }
        Ok(Change { undo: vec![Self::restore_cmd(si_out, &undo_rows, undo_dim, &parts_now)], pages_touched: pages })
    }
}
