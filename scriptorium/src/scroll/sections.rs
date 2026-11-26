//! Pages and sections: page breaks, section breaks (next page, continuous, even, odd), margins,
//! orientation and paper size per section, headers and footers (default, first page, even pages)
//! created and written, page number fields, different first page.
//!
//! A section's properties live in the `w:sectPr` that ends it: inside the last paragraph's `w:pPr`,
//! or the body's own last child for the final section.

use super::dom::{self, el_at, el_at_mut, prefix_of, set_child, wel, PPR_ORDER, SECT_ORDER};
use super::edit::{Tx, W_NS};
use super::layout::LineKey;
use super::pos::{atom_count, Flow, SPos, Span};
use super::ScrollDoc;
use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command};
use crate::ground::opc;
use crate::ground::xml::{Element, Node};

/// Paper sizes in points (portrait).
pub(crate) fn paper(name: &str) -> Option<(f64, f64)> {
    Some(match name.to_ascii_lowercase().replace([' ', '-'], "").as_str() {
        "letter" => (612.0, 792.0),
        "legal" => (612.0, 1008.0),
        "a3" => (841.9, 1190.55),
        "a4" => (595.3, 841.9),
        "a5" => (419.55, 595.3),
        "b5" => (498.9, 708.65),
        "executive" => (522.0, 756.0),
        "tabloid" => (792.0, 1224.0),
        _ => return None,
    })
}

fn tw(pt: f64) -> String {
    format!("{}", (pt * 20.0).round() as i64)
}

/// The XML path of the `w:sectPr` that ends the section holding body child `top` (the body's own
/// one for the last section; `None` when the document has none at all).
fn sect_path(root: &Element, top: usize) -> Option<Vec<usize>> {
    let (bi, body) = root.elements().enumerate().find(|(_, e)| e.local() == "body")?;
    for (k, el) in body.elements().enumerate().skip(top) {
        if el.local() == "p" {
            if let Some((pi, ppr)) = el.elements().enumerate().find(|(_, e)| e.local() == "pPr") {
                if let Some((si, _)) = ppr.elements().enumerate().find(|(_, e)| e.local() == "sectPr") {
                    return Some(vec![bi, k, pi, si]);
                }
            }
        }
        if el.local() == "sectPr" {
            return Some(vec![bi, k]);
        }
    }
    None
}

/// Every `w:sectPr` path of the body, in order.
fn all_sects(root: &Element) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    let mut top = 0;
    while let Some(p) = sect_path(root, top) {
        top = p[1] + 1;
        out.push(p);
    }
    out
}

impl ScrollDoc {
    /// The body child a place's section is found from (header places use their page's first body
    /// paragraph).
    fn section_top(&self, at: &SPos) -> usize {
        let flat = match &at.flow {
            Flow::Body => Some(at.p),
            Flow::Part(_) => {
                let pg = at.pg.unwrap_or(0);
                self.laid.lines.iter().find_map(|l| match l.key {
                    LineKey::Body(f) if l.page == pg => Some(f),
                    _ => None,
                })
            }
        };
        flat.and_then(|f| self.model.flat.get(f)).and_then(|f| f.path.get(1).copied()).unwrap_or(0)
    }

    /// The governing `w:sectPr` path for a place, creating the body's final one when missing.
    fn ensure_sect(&mut self, tx: &mut Tx, at: &SPos) -> Result<Vec<usize>, BusError> {
        let top = self.section_top(at);
        let main = self.main.clone();
        self.tx_part(tx, &main, |root, _| {
            if let Some(p) = sect_path(root, top) {
                return p;
            }
            let (bi, _) = root.elements().enumerate().find(|(_, e)| e.local() == "body").unwrap_or((0, root));
            let w = prefix_of(root);
            let Some(body) = dom::child_at_mut(root, bi) else { return Vec::new() };
            let mut s = wel(&w, "sectPr", &[]);
            s.children.push(Node::Element(wel(&w, "pgSz", &[("w", "12240"), ("h", "15840")])));
            s.children.push(Node::Element(wel(
                &w,
                "pgMar",
                &[("top", "1440"), ("right", "1440"), ("bottom", "1440"), ("left", "1440"), ("header", "720"), ("footer", "720"), ("gutter", "0")],
            )));
            body.children.push(Node::Element(s));
            vec![bi, dom::element_count(body) - 1]
        })
    }

    /// `scroll.page_setup`: margins, orientation and paper size for the selection's section (or
    /// every section with `"scope": "all"`).
    pub(crate) fn cmd_page_setup(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_or_start(cmd);
        let a = &cmd.args;
        let num = |k: &str| a.get(k).and_then(Json::as_f64);
        let size = match a.get("size") {
            Some(Json::Str(n)) => Some(paper(n).ok_or_else(|| BusError::bad_args(&cmd.name, "\"size\" is Letter, Legal, A3, A4, A5, B5, Executive or Tabloid (or give \"width\" and \"height\")"))?),
            _ => match (num("width"), num("height")) {
                (Some(w), Some(h)) if w > 36.0 && h > 36.0 => Some((w.min(h), w.max(h))),
                _ => None,
            },
        };
        let orient = match a.get("orientation").and_then(Json::as_str) {
            None => None,
            Some("portrait") => Some(false),
            Some("landscape") => Some(true),
            Some(_) => return Err(BusError::bad_args(&cmd.name, "\"orientation\" is portrait or landscape")),
        };
        let margins: Vec<(&str, f64)> = ["top", "bottom", "left", "right", "header", "footer", "gutter"].iter().filter_map(|k| num(k).map(|v| (*k, v))).collect();
        if size.is_none() && orient.is_none() && margins.is_empty() {
            return Err(BusError::bad_args(&cmd.name, "give margins (top, bottom, left, right, header, footer, gutter), \"orientation\" or \"size\""));
        }
        let all = a.get("scope").and_then(Json::as_str) == Some("all");
        let mut tx = Tx::default();
        let here = self.ensure_sect(&mut tx, &s.start)?;
        let main = self.main.clone();
        self.tx_part(&mut tx, &main, |root, _| {
            let paths = if all { all_sects(root) } else { vec![here.clone()] };
            for path in paths {
                let Some(sp) = el_at_mut(root, &path) else { continue };
                let w = prefix_of(sp);
                if size.is_some() || orient.is_some() {
                    let mut pg = sp.child("pgSz").cloned().unwrap_or_else(|| wel(&w, "pgSz", &[("w", "12240"), ("h", "15840")]));
                    let cur_w = dom::attr_local(&pg, "w").and_then(|v| v.parse::<f64>().ok()).unwrap_or(12240.0) / 20.0;
                    let cur_h = dom::attr_local(&pg, "h").and_then(|v| v.parse::<f64>().ok()).unwrap_or(15840.0) / 20.0;
                    let was_land = dom::attr_local(&pg, "orient") == Some("landscape") || cur_w > cur_h;
                    let land = orient.unwrap_or(was_land);
                    let (short, long) = match size {
                        Some(sz) => sz,
                        None => (cur_w.min(cur_h), cur_w.max(cur_h)),
                    };
                    let (pw, ph) = if land { (long, short) } else { (short, long) };
                    dom::set_attr_local(&mut pg, &w, "w", &tw(pw));
                    dom::set_attr_local(&mut pg, &w, "h", &tw(ph));
                    if land {
                        dom::set_attr_local(&mut pg, &w, "orient", "landscape");
                    } else {
                        dom::remove_attr_local(&mut pg, "orient");
                    }
                    // Margins turn with the page.
                    if land != was_land && margins.is_empty() {
                        if let Some(m) = sp.child_mut("pgMar") {
                            let get = |m: &Element, k: &str| dom::attr_local(m, k).map(str::to_string);
                            let (t, r, b, l) = (get(m, "top"), get(m, "right"), get(m, "bottom"), get(m, "left"));
                            let rot = if land { [(l, "top"), (t, "right"), (r, "bottom"), (b, "left")] } else { [(r, "top"), (b, "right"), (l, "bottom"), (t, "left")] };
                            for (v, k) in rot {
                                if let Some(v) = v {
                                    dom::set_attr_local(m, &w, k, &v);
                                }
                            }
                        }
                    }
                    set_child(sp, "pgSz", Some(pg), SECT_ORDER);
                }
                if !margins.is_empty() {
                    let mut m = sp.child("pgMar").cloned().unwrap_or_else(|| {
                        wel(&w, "pgMar", &[("top", "1440"), ("right", "1440"), ("bottom", "1440"), ("left", "1440"), ("header", "720"), ("footer", "720"), ("gutter", "0")])
                    });
                    for (k, v) in &margins {
                        dom::set_attr_local(&mut m, &w, k, &tw(*v));
                    }
                    set_child(sp, "pgMar", Some(m), SECT_ORDER);
                }
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &s.start, &s.end);
        Ok(ch)
    }

    pub(crate) fn cmd_page_break(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let mut tx = Tx::default();
        let at = self.delete_span(&mut tx, &s)?;
        let land = self.insert_at(&mut tx, &at, "\u{c}", None)?;
        let ch = self.tx_commit(tx)?;
        self.land_caret(cmd, &land);
        Ok(ch)
    }

    /// `scroll.section_break`: the section ends at the caret; the new section after it starts as
    /// `kind` says and keeps the page setup and headers.
    pub(crate) fn cmd_section_break(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let kind = match cmd.args.get("kind").and_then(Json::as_str).unwrap_or("nextPage") {
            "nextPage" | "next_page" | "next" => "nextPage",
            "continuous" => "continuous",
            "evenPage" | "even_page" | "even" => "evenPage",
            "oddPage" | "odd_page" | "odd" => "oddPage",
            _ => return Err(BusError::bad_args(&cmd.name, "\"kind\" is nextPage, continuous, evenPage or oddPage")),
        };
        if s.start.flow != Flow::Body {
            return Err(BusError::failed("a section break goes in the body"));
        }
        if self.tloc(&s.start).is_some() {
            return Err(BusError::failed("a section break cannot go inside a table"));
        }
        let mut tx = Tx::default();
        let at = self.delete_span(&mut tx, &s)?;
        let sect = self.ensure_sect(&mut tx, &at)?;
        let r = self.para_ref(&at.flow, at.p).ok_or_else(|| BusError::failed("no paragraph there"))?;
        let (path, fi) = (r.flat.path.clone(), r.para.fields_in.clone());
        let o = at.o;
        let main = self.main.clone();
        self.tx_part(&mut tx, &main, |root, cx| {
            let Some(orig) = el_at(root, &sect).cloned() else { return };
            let w = prefix_of(&orig);
            let mut first = orig.clone();
            // The section after the break starts as asked; its page-number restart belongs to the first.
            if let Some(sp) = el_at_mut(root, &sect) {
                set_child(sp, "type", if kind == "nextPage" { None } else { Some(wel(&w, "type", &[("val", kind)])) }, SECT_ORDER);
                if let Some(n) = sp.child_mut("pgNumType") {
                    dom::remove_attr_local(n, "start");
                }
            }
            set_child(&mut first, "sectPrChange", None, SECT_ORDER);
            let (cpath, idx) = (path[..path.len() - 1].to_vec(), path[path.len() - 1]);
            let Some(c) = el_at_mut(root, &cpath) else { return };
            let Some(p) = dom::child_at_mut(c, idx) else { return };
            let runs = cx.runs(p, &fi);
            let n = atom_count(&runs);
            if o == 0 && n > 0 {
                let mut np = wel(&w, "p", &[]);
                let mut ppr = p.child("pPr").cloned().unwrap_or_else(|| wel(&w, "pPr", &[]));
                set_child(&mut ppr, "numPr", None, PPR_ORDER);
                set_child(&mut ppr, "sectPr", Some(first), PPR_ORDER);
                np.children.push(Node::Element(ppr));
                dom::insert_at(c, idx, vec![Node::Element(np)]);
                return;
            }
            let right = dom::split_para(p, &runs, o);
            let ppr = dom::ppr_mut(p, &w);
            set_child(ppr, "sectPr", Some(first), PPR_ORDER);
            dom::insert_at(c, idx + 1, vec![Node::Element(right)]);
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_caret(cmd, &at.para(at.p + 1, 0));
        Ok(ch)
    }

    // --- headers and footers -----------------------------------------------------------------------

    /// The header or footer part shown for `kind` (0 default, 1 first, 2 even) in the section of
    /// `at`, following the sections before it.
    fn shown_part(&self, at: &SPos, header: bool, kind: usize) -> Option<String> {
        let top = self.section_top(at);
        let mut sects: Vec<(usize, &super::model::SectPr)> = Vec::new();
        for f in &self.model.flat {
            let _ = f;
        }
        // Sections in order with the body child that ends each.
        let mut k = 0usize;
        for b in &self.model.body {
            if let super::model::Block::Para(p) = b {
                if let Some(sp) = &p.sect {
                    let end = self.model.flat.get(p.flat).and_then(|f| f.path.get(1).copied()).unwrap_or(k);
                    sects.push((end, sp));
                }
            }
            k += 1;
        }
        sects.push((usize::MAX, &self.model.final_sect));
        let mut shown = None;
        for (end, sp) in sects {
            let r = if header { &sp.headers[kind] } else { &sp.footers[kind] };
            if r.is_some() {
                shown = r.clone();
            }
            if end >= top {
                break;
            }
        }
        shown
    }

    /// The header or footer part for a place, created (with one empty paragraph in the Header or
    /// Footer style) and referenced from its section when there is none.
    pub(crate) fn ensure_hf(&mut self, tx: &mut Tx, at: &SPos, header: bool, kind: usize) -> Result<String, BusError> {
        if kind == 2 {
            self.ensure_even_odd(tx)?;
        }
        if kind == 1 {
            let sect = self.ensure_sect(tx, at)?;
            let main = self.main.clone();
            self.tx_part(tx, &main, |root, _| {
                if let Some(sp) = el_at_mut(root, &sect) {
                    if sp.child("titlePg").is_none() {
                        let w = prefix_of(sp);
                        set_child(sp, "titlePg", Some(wel(&w, "titlePg", &[])), SECT_ORDER);
                    }
                }
            })?;
        }
        if let Some(p) = self.shown_part(at, header, kind) {
            if self.pkg.part(&p).is_some() {
                return Ok(p);
            }
        }
        let style = if header { "Header" } else { "Footer" };
        let rel_type = if header { opc::REL_HEADER } else { opc::REL_FOOTER };
        // A part made before and no longer referenced (an undone edit) is used again, so a redo
        // writes the very same XML.
        let referenced: Vec<String> = self
            .part_dom(&self.main)
            .map(|root| {
                let mut ids = Vec::new();
                for sp in all_sects(&root) {
                    if let Some(s) = el_at(&root, &sp) {
                        for r in s.elements().filter(|e| e.local() == "headerReference" || e.local() == "footerReference") {
                            if let Some(id) = dom::attr_local(r, "id") {
                                ids.push(id.to_string());
                            }
                        }
                    }
                }
                ids
            })
            .unwrap_or_default();
        let spare = self
            .pkg
            .rels(&self.main)
            .iter()
            .filter(|r| !r.target_mode_external && opc::rel_type_matches(&r.rel_type, rel_type) && !referenced.contains(&r.id))
            .map(|r| (r.id.clone(), self.pkg.resolve(&self.main, r)))
            .find(|(_, name)| self.pkg.part(name).is_some());
        self.ensure_style(tx, style)?;
        let base = if header { "header" } else { "footer" };
        let mut n = 1;
        let mut name = format!("/word/{base}{n}.xml");
        while self.pkg.part(&name).is_some() {
            n += 1;
            name = format!("/word/{base}{n}.xml");
        }
        let (name, rel) = match spare {
            Some((id, part)) => (part, id),
            None => {
                let root_name = if header { "hdr" } else { "ftr" };
                let xml_text = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:{root_name} xmlns:w=\"{W_NS}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr></w:p></w:{root_name}>"
                );
                let ct = if header { opc::CT_HEADER } else { opc::CT_FOOTER };
                self.pkg.add_part(&name, ct, xml_text.into_bytes()).map_err(|e| BusError::failed(e.to_string()))?;
                let rel = self.pkg.add_relationship(&self.main, rel_type, &format!("{base}{n}.xml"), false);
                (name, rel)
            }
        };
        tx.dirty = true;
        let sect = self.ensure_sect(tx, at)?;
        let main = self.main.clone();
        let local = if header { "headerReference" } else { "footerReference" };
        let ty = ["default", "first", "even"][kind];
        self.tx_part(tx, &main, |root, _| {
            ensure_r_ns(root);
            let Some(sp) = el_at_mut(root, &sect) else { return };
            let w = prefix_of(sp);
            sp.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == local && dom::attr_local(e, "type").unwrap_or("default") == ty));
            let r = wel(&w, local, &[("type", ty), ("r:id", &rel)]);
            // Header references first, then footer references.
            let at = if header {
                0
            } else {
                sp.elements().take_while(|e| e.local() == "headerReference" || e.local() == "footerReference").count()
            };
            dom::insert_at(sp, at, vec![Node::Element(r)]);
        })?;
        Ok(name)
    }

    /// Turn on different odd and even page headers in the settings part (created when missing).
    fn ensure_even_odd(&mut self, tx: &mut Tx) -> Result<(), BusError> {
        if self.model.settings.even_odd_headers {
            return Ok(());
        }
        let part = match self.pkg.related(&self.main, opc::REL_SETTINGS).first().cloned() {
            Some(p) => p,
            None => {
                let name = "/word/settings.xml".to_string();
                let body = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:settings xmlns:w=\"{W_NS}\"></w:settings>");
                if self.pkg.part(&name).is_none() {
                    self.pkg.add_part(&name, opc::CT_SETTINGS, body.into_bytes()).map_err(|e| BusError::failed(e.to_string()))?;
                }
                self.pkg.add_relationship(&self.main, opc::REL_SETTINGS, "settings.xml", false);
                tx.dirty = true;
                name
            }
        };
        self.tx_part(tx, &part, |root, _| {
            if root.child("evenAndOddHeaders").is_none() {
                let w = prefix_of(root);
                // After the zoom and view settings near the top is where it belongs; early is safe.
                const ORDER: &[&str] = &[
                    "writeProtection", "view", "zoom", "removePersonalInformation", "removeDateAndTime", "doNotDisplayPageBoundaries", "displayBackgroundShape",
                    "printPostScriptOverText", "printFractionalCharacterWidth", "printFormsData", "embedTrueTypeFonts", "embedSystemFonts", "saveSubsetFonts",
                    "saveFormsData", "mirrorMargins", "alignBordersAndEdges", "bordersDoNotSurroundHeader", "bordersDoNotSurroundFooter", "gutterAtTop",
                    "hideSpellingErrors", "hideGrammaticalErrors", "activeWritingStyle", "proofState", "formsDesign", "attachedTemplate", "linkStyles",
                    "stylePaneFormatFilter", "stylePaneSortMethod", "documentType", "mailMerge", "revisionView", "trackRevisions", "doNotTrackMoves",
                    "doNotTrackFormatting", "documentProtection", "autoFormatOverride", "styleLockTheme", "styleLockQFSet", "defaultTabStop",
                    "autoHyphenation", "consecutiveHyphenLimit", "hyphenationZone", "doNotHyphenateCaps", "showEnvelope", "summaryLength",
                    "clickAndTypeStyle", "defaultTableStyle", "evenAndOddHeaders",
                ];
                set_child(root, "evenAndOddHeaders", Some(wel(&w, "evenAndOddHeaders", &[])), ORDER);
            }
        })?;
        Ok(())
    }

    /// `scroll.header_footer`: open (create when needed) the selection's section header or footer,
    /// optionally writing its text; the caret lands at its end.
    pub(crate) fn cmd_header_footer(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_or_start(cmd);
        let header = match cmd.args.get("which").and_then(Json::as_str).unwrap_or("header") {
            "header" => true,
            "footer" => false,
            _ => return Err(BusError::bad_args(&cmd.name, "\"which\" is header or footer")),
        };
        let kind = match cmd.args.get("kind").and_then(Json::as_str).unwrap_or("default") {
            "default" | "odd" => 0,
            "first" => 1,
            "even" => 2,
            _ => return Err(BusError::bad_args(&cmd.name, "\"kind\" is default, first or even")),
        };
        let text = cmd.args.get("text").and_then(Json::as_str).map(str::to_string);
        let at = s.start.clone();
        let mut tx = Tx::default();
        let part = self.ensure_hf(&mut tx, &at, header, kind)?;
        if let Some(t) = &text {
            let lines: Vec<String> = t.replace("\r\n", "\n").split('\n').map(str::to_string).collect();
            self.tx_part(&mut tx, &part, |root, _| {
                let w = prefix_of(root);
                let ppr = root.child("p").and_then(|p| p.child("pPr")).cloned();
                let rpr = root.child("p").and_then(|p| p.elements().find(|e| e.local() == "r")).and_then(|r| r.child("rPr")).cloned();
                root.children.retain(|c| !matches!(c, Node::Element(e) if matches!(e.local(), "p" | "tbl" | "sdt")));
                for line in &lines {
                    let mut p = wel(&w, "p", &[]);
                    if let Some(pp) = &ppr {
                        p.children.push(Node::Element(pp.clone()));
                    }
                    if !line.is_empty() {
                        p.children.push(Node::Element(dom::new_run(&w, rpr.clone(), dom::content_nodes(&w, line))));
                    }
                    root.children.push(Node::Element(p));
                }
            })?;
        }
        let ch = self.tx_commit(tx)?;
        let flow = Flow::Part(part.clone());
        let last = self.flow_len(&flow).saturating_sub(1);
        let n = self.para_len(&flow, last);
        let pg = self.page_of(&at);
        self.land_caret(cmd, &SPos { flow, p: last, o: n, pg: Some(pg) });
        Ok(ch)
    }

    /// The page a place is shown on.
    pub(crate) fn page_of(&self, at: &SPos) -> usize {
        match &at.flow {
            Flow::Body => self.laid.lines.iter().find(|l| l.key == LineKey::Body(at.p) && l.atoms.start <= at.o).map(|l| l.page).unwrap_or(0),
            Flow::Part(_) => at.pg.unwrap_or(0),
        }
    }

    /// `scroll.page_number`: a PAGE field (optionally "Page X of Y") at the caret, or in the header
    /// or footer aligned left, center or right.
    pub(crate) fn cmd_page_number(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_or_start(cmd);
        let place = cmd.args.get("where").and_then(Json::as_str).unwrap_or("footer").to_string();
        let format = cmd.args.get("format").and_then(Json::as_str).unwrap_or("plain").to_string();
        if !matches!(format.as_str(), "plain" | "page_x" | "page_x_of_y" | "x_of_y") {
            return Err(BusError::bad_args(&cmd.name, "\"format\" is plain, page_x, page_x_of_y or x_of_y"));
        }
        let align = match cmd.args.get("align").and_then(Json::as_str).unwrap_or("center") {
            "left" => "left",
            "center" | "centre" => "center",
            "right" => "right",
            _ => return Err(BusError::bad_args(&cmd.name, "\"align\" is left, center or right")),
        };
        let mut tx = Tx::default();
        match place.as_str() {
            "here" | "caret" => {
                let at = self.delete_span(&mut tx, &s)?;
                let r = self.para_ref(&at.flow, at.p).ok_or_else(|| BusError::failed("no paragraph there"))?;
                let (part, path, fi) = (r.part.clone(), r.flat.path.clone(), r.para.fields_in.clone());
                let o = at.o;
                let added = self.tx_part(&mut tx, &part, |root, cx| {
                    let Some(p) = el_at_mut(root, &path) else { return 0 };
                    let w = prefix_of(p);
                    let runs = cx.runs(p, &fi);
                    let rpr = dom::point_at(p, &runs, o).rpr_from.and_then(|ri| dom::run_rpr(p, &runs, ri));
                    let nodes = number_nodes(&w, &format, rpr.as_ref());
                    let n = nodes.iter().map(node_atoms).sum();
                    let (c, i) = dom::level_at(p, &runs, o);
                    if let Some(cont) = el_at_mut(p, &c) {
                        dom::insert_at(cont, i, nodes);
                    }
                    n
                })?;
                let ch = self.tx_commit(tx)?;
                self.land_caret(cmd, &at.at(at.o + added));
                Ok(ch)
            }
            "header" | "footer" => {
                let header = place == "header";
                let part = self.ensure_hf(&mut tx, &s.start, header, 0)?;
                self.tx_part(&mut tx, &part, |root, _| {
                    let w = prefix_of(root);
                    // The part's last paragraph when it is empty, else a new one.
                    let last_empty = root.elements().filter(|e| e.local() == "p").last().is_some_and(|p| !p.elements().any(|e| e.local() != "pPr"));
                    if !last_empty {
                        let mut p = wel(&w, "p", &[]);
                        if let Some(pp) = root.elements().filter(|e| e.local() == "p").last().and_then(|p| p.child("pPr")).cloned() {
                            p.children.push(Node::Element(pp));
                        }
                        root.children.push(Node::Element(p));
                    }
                    let k = root.elements().enumerate().filter(|(_, e)| e.local() == "p").map(|(i, _)| i).last().unwrap_or(0);
                    let Some(p) = dom::child_at_mut(root, k) else { return };
                    let ppr = dom::ppr_mut(p, &w);
                    set_child(ppr, "jc", Some(wel(&w, "jc", &[("val", align)])), PPR_ORDER);
                    let nodes = number_nodes(&w, &format, None);
                    p.children.extend(nodes);
                })?;
                let ch = self.tx_commit(tx)?;
                let flow = Flow::Part(part);
                let last = self.flow_len(&flow).saturating_sub(1);
                let n = self.para_len(&flow, last);
                let pg = self.page_of(&s.start);
                self.land_caret(cmd, &SPos { flow, p: last, o: n, pg: Some(pg) });
                Ok(ch)
            }
            _ => Err(BusError::bad_args(&cmd.name, "\"where\" is header, footer or here")),
        }
    }

    /// `scroll.first_page`: a different first page header and footer for the section (on or off).
    pub(crate) fn cmd_first_page(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_or_start(cmd);
        let on = cmd.args.get("on").and_then(Json::as_bool).unwrap_or(true);
        let mut tx = Tx::default();
        let sect = self.ensure_sect(&mut tx, &s.start)?;
        let main = self.main.clone();
        self.tx_part(&mut tx, &main, |root, _| {
            if let Some(sp) = el_at_mut(root, &sect) {
                let w = prefix_of(sp);
                set_child(sp, "titlePg", if on { Some(wel(&w, "titlePg", &[])) } else { None }, SECT_ORDER);
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &s.start, &s.end);
        Ok(ch)
    }

    /// `scroll.page_numbering`: restart (or continue) page numbers in the section, with a format.
    pub(crate) fn cmd_page_numbering(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_or_start(cmd);
        let start = cmd.args.get("start").and_then(Json::as_i64);
        let fmt = cmd.args.get("format").and_then(Json::as_str).map(str::to_string);
        if let Some(f) = &fmt {
            if !matches!(f.as_str(), "decimal" | "lowerRoman" | "upperRoman" | "lowerLetter" | "upperLetter") {
                return Err(BusError::bad_args(&cmd.name, "\"format\" is decimal, lowerRoman, upperRoman, lowerLetter or upperLetter"));
            }
        }
        let mut tx = Tx::default();
        let sect = self.ensure_sect(&mut tx, &s.start)?;
        let main = self.main.clone();
        self.tx_part(&mut tx, &main, |root, _| {
            let Some(sp) = el_at_mut(root, &sect) else { return };
            let w = prefix_of(sp);
            let mut n = sp.child("pgNumType").cloned().unwrap_or_else(|| wel(&w, "pgNumType", &[]));
            match start {
                Some(v) => dom::set_attr_local(&mut n, &w, "start", &v.to_string()),
                None => dom::remove_attr_local(&mut n, "start"),
            }
            if let Some(f) = &fmt {
                if f == "decimal" {
                    dom::remove_attr_local(&mut n, "fmt");
                } else {
                    dom::set_attr_local(&mut n, &w, "fmt", f);
                }
            }
            let keep = !n.attrs.is_empty();
            set_child(sp, "pgNumType", if keep { Some(n) } else { None }, SECT_ORDER);
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &s.start, &s.end);
        Ok(ch)
    }
}

/// Declare the relationships prefix on the root when it is missing.
pub(crate) fn ensure_r_ns(root: &mut Element) {
    if root.attr("xmlns:r").is_none() {
        root.attrs.push(("xmlns:r".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships".into()));
    }
}

fn field(w: &str, instr: &str, rpr: Option<&Element>) -> Node {
    let mut f = wel(w, "fldSimple", &[("instr", instr)]);
    let mut rp = rpr.cloned().unwrap_or_else(|| wel(w, "rPr", &[]));
    set_child(&mut rp, "noProof", Some(wel(w, "noProof", &[])), dom::RPR_ORDER);
    f.children.push(Node::Element(dom::new_run(w, Some(rp), dom::content_nodes(w, "1"))));
    Node::Element(f)
}

/// The page number as fields: "1", "Page 1", "Page 1 of 3", "1 of 3".
fn number_nodes(w: &str, format: &str, rpr: Option<&Element>) -> Vec<Node> {
    let text = |t: &str| Node::Element(dom::new_run(w, rpr.cloned(), dom::content_nodes(w, t)));
    match format {
        "page_x" => vec![text("Page "), field(w, " PAGE ", rpr)],
        "page_x_of_y" => vec![text("Page "), field(w, " PAGE ", rpr), text(" of "), field(w, " NUMPAGES ", rpr)],
        "x_of_y" => vec![field(w, " PAGE ", rpr), text(" of "), field(w, " NUMPAGES ", rpr)],
        _ => vec![field(w, " PAGE ", rpr)],
    }
}

/// How many editing characters a node made by [`number_nodes`] is.
fn node_atoms(n: &Node) -> usize {
    match n {
        Node::Element(e) if e.local() == "fldSimple" => 1,
        Node::Element(e) => e.elements().filter(|c| c.local() == "t").map(|t| t.text().chars().count()).sum(),
        _ => 0,
    }
}

#[allow(dead_code)]
fn unused(_: &Span) {}
