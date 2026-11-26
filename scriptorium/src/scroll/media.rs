//! Pictures and links: a picture from a file's bytes (PNG, JPEG, GIF, BMP; shown when we can decode
//! it) placed inline at the caret as DrawingML (`wp:inline`, `pic:pic`, `r:embed` into
//! `word/media`), resized with its proportions kept; hyperlinks inserted, edited and removed
//! (`w:hyperlink` with an external relationship, runs in the Hyperlink character style).

use super::dom::{self, el_at, el_at_mut, prefix_of, wel, RPR_ORDER};
use super::edit::Tx;
use super::model::{RunItem, RunKind};
use super::pos::{atoms, AKind, SPos};
use super::ScrollDoc;
use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command};
use crate::ground::opc;
use crate::ground::xml::{Element, Node};

/// Standard base64 (padding and whitespace tolerated; a `data:...;base64,` prefix is skipped).
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = match s.find("base64,") {
        Some(i) if s.starts_with("data:") => &s[i + 7..],
        _ => s,
    };
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A picture's format (file extension, content type) and pixel size, from its bytes.
pub fn image_info(b: &[u8]) -> Option<(&'static str, &'static str, u32, u32)> {
    if b.len() >= 24 && b.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
        let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
        return Some(("png", opc::CT_PNG, w, h));
    }
    if b.len() >= 10 && (b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) {
        let w = u16::from_le_bytes([b[6], b[7]]) as u32;
        let h = u16::from_le_bytes([b[8], b[9]]) as u32;
        return Some(("gif", "image/gif", w, h));
    }
    if b.len() >= 26 && b.starts_with(b"BM") {
        let w = i32::from_le_bytes([b[18], b[19], b[20], b[21]]).unsigned_abs();
        let h = i32::from_le_bytes([b[22], b[23], b[24], b[25]]).unsigned_abs();
        return Some(("bmp", "image/bmp", w, h));
    }
    if b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) || m == 0xFF {
                i += if m == 0xFF { 1 } else { 2 };
                continue;
            }
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return Some(("jpeg", opc::CT_JPEG, w, h));
            }
            i += 2 + len;
        }
        return Some(("jpeg", opc::CT_JPEG, 0, 0));
    }
    None
}

const EMU_PER_PT: f64 = 12700.0;

fn emu(pt: f64) -> String {
    format!("{}", (pt * EMU_PER_PT).round() as i64)
}

/// The largest `wp:docPr` id in a part.
fn max_docpr(el: &Element) -> u64 {
    let mut m = 0;
    if el.local() == "docPr" {
        m = dom::attr_local(el, "id").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    }
    for c in el.elements() {
        m = m.max(max_docpr(c));
    }
    m
}

/// An inline picture run.
fn picture_run(w: &str, rel: &str, id: u64, name: &str, wpt: f64, hpt: f64, rpr: Option<Element>) -> String {
    let (cx, cy) = (emu(wpt), emu(hpt));
    let rpr_xml = rpr.map(|r| {
        let mut wr = crate::ground::xml::Writer::new();
        wr.element(&r);
        String::from_utf8_lossy(&wr.into_bytes()).into_owned()
    });
    let esc = crate::ground::xml::escape_attr(name);
    format!(
        "<{w}r>{rpr}<{w}drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\">\
<wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\" descr=\"{esc}\"/>\
<wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"{esc}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"{rel}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
</a:graphicData></a:graphic></wp:inline></{w}drawing></{w}r>",
        rpr = rpr_xml.unwrap_or_default()
    )
}

/// Find every descendant named `local`, mutable.
fn each_named<'e>(el: &'e mut Element, local: &str, f: &mut dyn FnMut(&mut Element)) {
    if el.local() == local {
        f(el);
    }
    for c in el.children.iter_mut() {
        if let Node::Element(e) = c {
            each_named(e, local, f);
        }
    }
}

impl ScrollDoc {
    /// `scroll.insert_picture`: `{ sel, data: base64, name, width?, height? }` (points; one of them
    /// keeps the proportions, neither uses the picture's own size at 96 dpi, shrunk to the column).
    pub(crate) fn cmd_insert_picture(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let data = cmd.args.get("data").and_then(Json::as_str).ok_or_else(|| BusError::bad_args(&cmd.name, "takes { \"sel\", \"data\": base64 bytes, \"name\": string, \"width\"?, \"height\"? }"))?;
        let bytes = base64_decode(data).filter(|b| !b.is_empty()).ok_or_else(|| BusError::bad_args(&cmd.name, "\"data\" is not base64"))?;
        let (ext, ct, pw, ph) = image_info(&bytes).ok_or_else(|| BusError::failed("the picture is not PNG, JPEG, GIF or BMP"))?;
        let name = cmd.args.get("name").and_then(Json::as_str).unwrap_or("picture").to_string();
        let ratio = if pw > 0 && ph > 0 { ph as f64 / pw as f64 } else { 0.75 };
        let col = self.column_width(&s.start.flow, s.start.p);
        let (wpt, hpt) = match (cmd.args.get("width").and_then(Json::as_f64), cmd.args.get("height").and_then(Json::as_f64)) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * ratio),
            (None, Some(h)) => (h / ratio, h),
            (None, None) => {
                let w = if pw > 0 { pw as f64 * 0.75 } else { 144.0 };
                let w = w.min(col);
                (w, w * ratio)
            }
        };
        if !(wpt > 0.0 && hpt > 0.0) {
            return Err(BusError::bad_args(&cmd.name, "\"width\" and \"height\" are points above zero"));
        }
        let mut tx = Tx::default();
        let at = self.delete_span(&mut tx, &s)?;
        let r = self.para_ref(&at.flow, at.p).ok_or_else(|| BusError::failed("no paragraph there"))?;
        let (part, path, fi) = (r.part.clone(), r.flat.path.clone(), r.para.fields_in.clone());
        // The same bytes already in the package are used again.
        let existing = self.pkg.parts().find(|p| p.name().starts_with("/word/media/") && self.pkg.part_bytes(p.name()).ok().as_deref() == Some(&bytes[..])).map(|p| p.name().to_string());
        let media = match existing {
            Some(n) => n,
            None => {
                let mut k = 1;
                let mut n = format!("/word/media/image{k}.{ext}");
                while self.pkg.part(&n).is_some() {
                    k += 1;
                    n = format!("/word/media/image{k}.{ext}");
                }
                self.pkg.add_part(&n, ct, bytes).map_err(|e| BusError::failed(e.to_string()))?;
                n
            }
        };
        let rel = match self.pkg.rels(&part).iter().find(|r| !r.target_mode_external && opc::rel_type_matches(&r.rel_type, opc::REL_IMAGE) && opc::same_part(&self.pkg.resolve(&part, r), &media)) {
            Some(r) => r.id.clone(),
            None => self.pkg.add_relationship(&part, opc::REL_IMAGE, &media, false),
        };
        tx.dirty = true;
        let o = at.o;
        self.tx_part(&mut tx, &part, |root, cx| {
            let id = max_docpr(root) + 1;
            let Some(p) = el_at_mut(root, &path) else { return };
            let w = prefix_of(p);
            let runs = cx.runs(p, &fi);
            let rpr = dom::point_at(p, &runs, o).rpr_from.and_then(|ri| dom::run_rpr(p, &runs, ri));
            let xml_text = picture_run(&w, &rel, id, &name, wpt, hpt, rpr);
            let (c, i) = dom::level_at(p, &runs, o);
            if let Some(cont) = el_at_mut(p, &c) {
                dom::insert_at(cont, i, vec![Node::Raw(xml_text.into_bytes())]);
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &at, &at.at(at.o + 1));
        Ok(ch)
    }

    /// The picture a selection is on: the selected object, or the one just before (or after) a caret.
    fn picture_at(&self, s: &super::pos::Span) -> Option<(SPos, usize)> {
        let flow = s.flow().clone();
        let ats = self.para_atoms(&flow, s.start.p);
        let cand = [Some(s.start.o), s.start.o.checked_sub(1)];
        for o in cand.into_iter().flatten() {
            if ats.get(o).is_some_and(|a| a.kind == AKind::Pic) {
                return Some((s.start.clone(), o));
            }
        }
        None
    }

    /// `scroll.picture_size`: `{ sel, width?, height? }` in points; one alone keeps the proportions.
    pub(crate) fn cmd_picture_size(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let (at, o) = self.picture_at(&s).ok_or_else(|| BusError::failed("select a picture first"))?;
        let r = self.para_ref(&at.flow, at.p).ok_or_else(|| BusError::failed("no paragraph there"))?;
        let ats = atoms(&r.para.runs);
        let a = ats[o];
        let run = &r.para.runs[a.run];
        let (cw, chh) = match &run.items[a.item] {
            RunItem::Pic(p) => (p.w, p.h),
            _ => return Err(BusError::failed("select a picture first")),
        };
        let ratio = if cw > 0.0 { chh / cw } else { 1.0 };
        let (w, h) = match (cmd.args.get("width").and_then(Json::as_f64), cmd.args.get("height").and_then(Json::as_f64)) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * ratio),
            (None, Some(h)) => (if ratio > 0.0 { h / ratio } else { h }, h),
            _ => return Err(BusError::bad_args(&cmd.name, "takes \"width\" and/or \"height\" in points")),
        };
        if !(w > 0.0 && h > 0.0) || run.kind != RunKind::R {
            return Err(BusError::bad_args(&cmd.name, "the size is points above zero"));
        }
        let (part, mut path) = (r.part.clone(), r.flat.path.clone());
        path.extend(run.src.iter().copied());
        path.push(run.item_src[a.item]);
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &part, |root, _| {
            let Some(d) = el_at_mut(root, &path) else { return };
            let (cx, cy) = (emu(w), emu(h));
            each_named(d, "extent", &mut |e| {
                dom::set_attr_local(e, "", "cx", &cx);
                dom::set_attr_local(e, "", "cy", &cy);
            });
            each_named(d, "xfrm", &mut |x| {
                if let Some(e) = x.child_mut("ext") {
                    e.set_attr("cx", cx.clone());
                    e.set_attr("cy", cy.clone());
                }
            });
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &at.at(o), &at.at(o + 1));
        Ok(ch)
    }

    /// `scroll.link`: `{ sel, url, text? }`. The selection becomes a link (in the Hyperlink style);
    /// a caret inside a link changes where it goes; a caret elsewhere inserts `text` (or the
    /// address) as a new link. A `#name` address links to a bookmark.
    pub(crate) fn cmd_link(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let url = cmd.args.get("url").and_then(Json::as_str).map(str::trim).filter(|u| !u.is_empty()).ok_or_else(|| BusError::bad_args(&cmd.name, "takes { \"sel\", \"url\": string, \"text\"?: string }"))?.to_string();
        let text = cmd.args.get("text").and_then(Json::as_str).map(str::to_string);
        if s.start.p != s.end.p {
            return Err(BusError::failed("a link's text stays inside one paragraph"));
        }
        let mut tx = Tx::default();
        self.ensure_style(&mut tx, "Hyperlink")?;
        let style = self.style_id("Hyperlink").unwrap_or_else(|| "Hyperlink".into());
        let r = self.para_ref(&s.start.flow, s.start.p).ok_or_else(|| BusError::failed("no paragraph there"))?;
        let (part, path, fi) = (r.part.clone(), r.flat.path.clone(), r.para.fields_in.clone());
        // A caret in an existing link edits it.
        let ats = atoms(&r.para.runs);
        let probe = if s.is_caret() { s.start.o.checked_sub(1).or(Some(0)) } else { Some(s.start.o) };
        let in_link = probe.and_then(|o| ats.get(o)).and_then(|a| {
            let run = &r.para.runs[a.run];
            run.link.as_ref()?;
            let root = self.part_dom(&part)?;
            let p = el_at(&root, &path)?;
            let mut cur = p;
            let mut hp = Vec::new();
            for (k, &i) in run.src.iter().enumerate() {
                cur = cur.elements().nth(i)?;
                if cur.local() == "hyperlink" {
                    hp = run.src[..=k].to_vec();
                }
            }
            (!hp.is_empty()).then_some(hp)
        });
        let (anchor, external) = match url.strip_prefix('#') {
            Some(a) => (Some(a.to_string()), false),
            None => (None, true),
        };
        let rel = if external {
            let found = self.pkg.rels(&part).iter().find(|x| x.target_mode_external && x.target == url && opc::rel_type_matches(&x.rel_type, opc::REL_HYPERLINK)).map(|x| x.id.clone());
            let id = found.unwrap_or_else(|| self.pkg.add_relationship(&part, opc::REL_HYPERLINK, &url, true));
            tx.dirty = true;
            Some(id)
        } else {
            None
        };
        let set_target = |h: &mut Element, w: &str| {
            dom::remove_attr_local(h, "id");
            dom::remove_attr_local(h, "anchor");
            if let Some(id) = &rel {
                h.attrs.push(("r:id".into(), id.clone()));
            }
            if let Some(a) = &anchor {
                h.attrs.push((format!("{w}anchor"), a.clone()));
            }
            if dom::attr_local(h, "history").is_none() {
                h.attrs.push((format!("{w}history"), "1".into()));
            }
        };
        if let Some(hp) = in_link.filter(|_| s.is_caret() && text.is_none()) {
            self.tx_part(&mut tx, &part, |root, _| {
                super::sections::ensure_r_ns(root);
                let Some(p) = el_at_mut(root, &path) else { return };
                let w = prefix_of(p);
                if let Some(h) = el_at_mut(p, &hp) {
                    set_target(h, &w);
                }
            })?;
            let ch = self.tx_commit(tx)?;
            self.land_caret(cmd, &s.start);
            return Ok(ch);
        }
        let (a, b) = (s.start.o, s.end.o);
        let land_end = self.tx_part(&mut tx, &part, |root, cx| {
            super::sections::ensure_r_ns(root);
            let Some(p) = el_at_mut(root, &path) else { return b };
            let w = prefix_of(p);
            let mut b = b;
            if a == b {
                let t = text.clone().unwrap_or_else(|| url.clone());
                let runs = cx.runs(p, &fi);
                b = a + dom::insert_text(p, &runs, a, &t, None);
            }
            // Run-level places at both ends (the end first, so the start split can only shift it).
            let runs = cx.runs(p, &fi);
            let (c2, i2) = dom::level_at(p, &runs, b);
            let before = el_at(p, &c2).map(dom::element_count).unwrap_or(0);
            let runs = cx.runs(p, &fi);
            let (c1, i1) = dom::level_at(p, &runs, a);
            if c1 != c2 {
                return b;
            }
            let Some(cont) = el_at_mut(p, &c1) else { return b };
            let end = i2 + dom::element_count(cont).saturating_sub(before);
            let mut h = wel(&w, "hyperlink", &[]);
            set_target(&mut h, &w);
            let mut moved = Vec::new();
            for _ in i1..end {
                if let Some(mut e) = dom::remove_at(cont, i1) {
                    if e.local() == "r" {
                        let rpr = dom::rpr_mut(&mut e, &w);
                        dom::set_child(rpr, "rStyle", Some(wel(&w, "rStyle", &[("val", &style)])), RPR_ORDER);
                    }
                    moved.push(Node::Element(e));
                }
            }
            h.children = moved;
            dom::insert_at(cont, i1, vec![Node::Element(h)]);
            b
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &s.start.at(a), &s.start.at(land_end));
        Ok(ch)
    }

    /// `scroll.unlink`: the links the selection touches become plain text again.
    pub(crate) fn cmd_unlink(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let s = self.sel_span(cmd)?;
        let flow = s.flow().clone();
        let mut work: Vec<(Vec<usize>, Vec<Vec<usize>>)> = Vec::new();
        for p in s.start.p..=s.end.p {
            let Some(r) = self.para_ref(&flow, p) else { continue };
            let ats = atoms(&r.para.runs);
            let (a, b) = (if p == s.start.p { s.start.o } else { 0 }, if p == s.end.p { s.end.o } else { ats.len() });
            let (a, b) = if a == b { (a.saturating_sub(1), (a + 1).min(ats.len())) } else { (a, b) };
            let mut links: Vec<Vec<usize>> = Vec::new();
            let root = self.part_dom(&r.part);
            for at in ats.iter().take(b).skip(a) {
                let run = &r.para.runs[at.run];
                if run.link.is_none() {
                    continue;
                }
                let Some(root) = root.as_ref() else { continue };
                let Some(pe) = el_at(root, &r.flat.path) else { continue };
                let mut cur = pe;
                let mut hp = None;
                for (k, &i) in run.src.iter().enumerate() {
                    let Some(n) = cur.elements().nth(i) else { break };
                    cur = n;
                    if cur.local() == "hyperlink" {
                        hp = Some(run.src[..=k].to_vec());
                    }
                }
                if let Some(h) = hp {
                    if !links.contains(&h) {
                        links.push(h);
                    }
                }
            }
            if !links.is_empty() {
                work.push((r.flat.path.clone(), links));
            }
        }
        if work.is_empty() {
            self.land_span(cmd, &s.start, &s.end);
            return Ok(Change::default());
        }
        let part = self.part_name(&flow);
        let link_style = self.style_id("Hyperlink");
        let mut tx = Tx::default();
        self.tx_part(&mut tx, &part, |root, _| {
            for (path, mut links) in work {
                let Some(p) = el_at_mut(root, &path) else { continue };
                links.sort();
                for hp in links.into_iter().rev() {
                    let (cpath, idx) = (hp[..hp.len() - 1].to_vec(), hp[hp.len() - 1]);
                    let Some(c) = el_at_mut(p, &cpath) else { continue };
                    let Some(h) = dom::remove_at(c, idx) else { continue };
                    let mut kids = Vec::new();
                    for mut n in h.children {
                        if let Node::Element(e) = &mut n {
                            if e.local() == "r" {
                                if let Some(rpr) = e.child_mut("rPr") {
                                    rpr.children.retain(|x| !matches!(x, Node::Element(st) if st.local() == "rStyle" && link_style.as_deref().is_some_and(|ls| dom::attr_local(st, "val") == Some(ls))));
                                }
                                e.children.retain(|x| !matches!(x, Node::Element(rp) if rp.local() == "rPr" && rp.children.is_empty() && rp.attrs.is_empty()));
                            }
                        }
                        kids.push(n);
                    }
                    let k = dom::node_index(c, idx);
                    for (j, n) in kids.into_iter().enumerate() {
                        c.children.insert(k + j, n);
                    }
                }
            }
        })?;
        let ch = self.tx_commit(tx)?;
        self.land_span(cmd, &s.start, &s.end);
        Ok(ch)
    }
}

