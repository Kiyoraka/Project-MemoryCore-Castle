//! `word/styles.xml`: docDefaults and the paragraph, character and table styles, with their
//! `basedOn` chains resolved. Latent styles are ignored; numbering styles are kept for
//! `w:numStyleLink`.

use super::props::{attr, val, Borders, Ctx, Margins, ParaProps, RunProps};
use crate::ground::xml::{parse_dom, Element};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleKind {
    Paragraph,
    Character,
    Table,
    Numbering,
}

/// One `w:style`.
#[derive(Debug, Clone)]
pub struct Style {
    pub id: String,
    pub name: String,
    pub kind: StyleKind,
    pub based_on: Option<String>,
    /// `w:next`: the style a new paragraph after this one takes.
    pub next: Option<String>,
    /// `w:link`: the paired character (or paragraph) style.
    pub link: Option<String>,
    pub ppr: ParaProps,
    pub rpr: RunProps,
    /// Table styles: `w:tblPr/w:tblBorders`.
    pub borders: Borders,
    /// Table styles: `w:tblPr/w:tblCellMar`.
    pub cell_mar: Margins,
    pub is_default: bool,
}

/// All the styles of a document.
#[derive(Debug, Clone, Default)]
pub struct Styles {
    pub map: HashMap<String, Style>,
    pub doc_rpr: RunProps,
    pub doc_ppr: ParaProps,
    pub default_para: Option<String>,
    pub default_table: Option<String>,
}

/// How deep a `basedOn` chain may go (cycles and silly files stop here).
const MAX_CHAIN: usize = 32;

impl Styles {
    /// Read `styles.xml`. A part that does not parse gives no styles (Word's built-in defaults).
    pub fn parse(bytes: &[u8], ctx: &Ctx) -> Styles {
        let mut s = Styles::default();
        let Ok(root) = parse_dom(bytes) else { return s };
        if let Some(dd) = root.child("docDefaults") {
            if let Some(r) = dd.child("rPrDefault").and_then(|e| e.child("rPr")) {
                s.doc_rpr = RunProps::parse(r, ctx);
            }
            if let Some(p) = dd.child("pPrDefault").and_then(|e| e.child("pPr")) {
                s.doc_ppr = ParaProps::parse(p, ctx);
            }
        }
        for el in root.children_named("style") {
            let kind = match attr(el, "type").map(str::trim) {
                Some("character") => StyleKind::Character,
                Some("table") => StyleKind::Table,
                Some("numbering") => StyleKind::Numbering,
                _ => StyleKind::Paragraph,
            };
            let Some(id) = attr(el, "styleId") else { continue };
            let is_default = attr(el, "default").is_some_and(|v| v == "1" || v == "true" || v == "on");
            let style = parse_style(el, id, kind, is_default, ctx);
            if is_default {
                match kind {
                    StyleKind::Paragraph if s.default_para.is_none() => s.default_para = Some(id.to_string()),
                    StyleKind::Table if s.default_table.is_none() => s.default_table = Some(id.to_string()),
                    _ => {}
                }
            }
            s.map.insert(id.to_string(), style);
        }
        s
    }

    pub fn get(&self, id: &str) -> Option<&Style> {
        self.map.get(id)
    }

    /// The style and its ancestors, root first.
    pub fn chain(&self, id: &str) -> Vec<&Style> {
        let mut out: Vec<&Style> = Vec::new();
        let mut cur = self.map.get(id);
        while let Some(s) = cur {
            if out.len() >= MAX_CHAIN || out.iter().any(|o| o.id == s.id) {
                break;
            }
            out.push(s);
            cur = s.based_on.as_deref().and_then(|b| self.map.get(b));
        }
        out.reverse();
        out
    }

    /// A paragraph (or table) style's paragraph and run properties with its chain folded in.
    pub fn folded(&self, id: &str) -> (ParaProps, RunProps) {
        let mut p = ParaProps::default();
        let mut r = RunProps::default();
        for s in self.chain(id) {
            p.over(&s.ppr);
            r.over(&s.rpr);
        }
        p.style = None;
        r.style = None;
        (p, r)
    }

    /// A character style's run properties with its chain folded in.
    pub fn char_rpr(&self, id: &str) -> RunProps {
        let mut r = RunProps::default();
        for s in self.chain(id) {
            r.over(&s.rpr);
        }
        r.style = None;
        r
    }

    /// A table style's borders and cell margins with its chain folded in.
    pub fn table_look(&self, id: &str) -> (Borders, Margins) {
        let mut b = Borders::default();
        let mut m = Margins::default();
        for s in self.chain(id) {
            b.over(&s.borders);
            m.over(&s.cell_mar);
        }
        (b, m)
    }

    /// The style's display name (its id when it has none).
    pub fn name_of<'a>(&'a self, id: &'a str) -> &'a str {
        self.map.get(id).map(|s| s.name.as_str()).filter(|n| !n.is_empty()).unwrap_or(id)
    }

    /// The heading level a paragraph style gives: 0 for "Title", 1..=9 for "heading N" (by name or by
    /// id), else the chain's `w:outlineLvl` + 1.
    pub fn heading_level(&self, id: &str) -> Option<u8> {
        let name = self.name_of(id).to_ascii_lowercase();
        if name == "title" || id.eq_ignore_ascii_case("title") {
            return Some(0);
        }
        for cand in [name.as_str(), &id.to_ascii_lowercase()] {
            let c = cand.replace(' ', "");
            if let Some(n) = c.strip_prefix("heading") {
                if let Ok(n) = n.parse::<u8>() {
                    if (1..=9).contains(&n) {
                        return Some(n);
                    }
                }
            }
        }
        let (p, _) = self.folded(id);
        p.outline.filter(|o| *o < 9).map(|o| o + 1)
    }
}

fn parse_style(el: &Element, id: &str, kind: StyleKind, is_default: bool, ctx: &Ctx) -> Style {
    let name = el.child("name").and_then(val).unwrap_or("").to_string();
    let based_on = el.child("basedOn").and_then(val).map(str::to_string);
    let next = el.child("next").and_then(val).map(str::to_string);
    let link = el.child("link").and_then(val).map(str::to_string);
    let ppr = el.child("pPr").map(|p| ParaProps::parse(p, ctx)).unwrap_or_default();
    let rpr = el.child("rPr").map(|r| RunProps::parse(r, ctx)).unwrap_or_default();
    let (borders, cell_mar) = match el.child("tblPr") {
        Some(t) => (
            t.child("tblBorders").map(|b| Borders::parse(b, ctx)).unwrap_or_default(),
            t.child("tblCellMar").map(Margins::parse).unwrap_or_default(),
        ),
        None => (Borders::default(), Margins::default()),
    };
    Style { id: id.to_string(), name, kind, based_on, next, link, ppr, rpr, borders, cell_mar, is_default }
}
