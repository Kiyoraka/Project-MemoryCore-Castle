//! The hand commands: typing, formatting, inserting, arranging and the slide list. Every one takes
//! the selection it acts on as `"sel"` (or names its slide and shapes), keeps an exact undo (the
//! parts as they were, `stage.restore_parts`) and says where the selection lands.

use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command, CommandSpec};
use crate::ground::display::Rect;
use crate::ground::draw::{preset_names, shape_geometry, Xfrm};
use crate::ground::edit::{Editing, Selection};
use crate::ground::opc::{self, name_key, Package};
use crate::ground::xml::{self, Element, Node};

use super::edit::{el_at, find_shape, ShapeKey};
use super::editing::{place_of, shapes_sel, slide_sel, Place, TextPos};
use super::model::{shape_id, sp_tree, Deck, Ph, REL_NOTES_SLIDE};
use super::ops::{self, emu, failed, Ns, Outline, EMU};
use super::render::{layout_block, resolve_paras, resolve_shape, Owner, Scene};
use super::snap::{self, Snap};
use super::style::Autofit;
use super::text::{put_ordered, At, Body, BODYPR_ORDER, FILLS, PPR_ORDER, RPR_ORDER};
use super::StageDoc;

/// PowerPoint's font size steps for grow and shrink.
const SIZES: [f64; 24] = [8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 44.0, 48.0, 54.0, 60.0, 66.0, 72.0, 80.0, 88.0, 96.0];

fn num(args: &Json, k: &str) -> Option<f64> {
    args.get(k).and_then(Json::as_f64).filter(|v| v.is_finite())
}

fn s<'a>(args: &'a Json, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Json::as_str)
}

fn flag(args: &Json, k: &str) -> Option<bool> {
    args.get(k).and_then(Json::as_bool)
}

/// The underline values PresentationML knows, from friendly words too.
fn underline_val(v: &Json) -> &'static str {
    if let Some(b) = v.as_bool() {
        return if b { "sng" } else { "none" };
    }
    match v.as_str().unwrap_or("sng") {
        "none" | "off" => "none",
        "double" | "dbl" => "dbl",
        "heavy" => "heavy",
        "dotted" => "dotted",
        "dash" | "dashed" => "dash",
        "wavy" => "wavy",
        "dotDash" => "dotDash",
        "words" => "words",
        _ => "sng",
    }
}

/// Apply the run formatting of `args` to one `a:rPr`.
fn apply_rpr(r: &mut Element, args: &Json, a: &str, size: Option<f64>) {
    if let Some(b) = flag(args, "bold") {
        r.set_attr("b", if b { "1" } else { "0" });
    }
    if let Some(i) = flag(args, "italic") {
        r.set_attr("i", if i { "1" } else { "0" });
    }
    if let Some(u) = args.get("underline") {
        r.set_attr("u", underline_val(u));
    }
    if let Some(st) = flag(args, "strike") {
        r.set_attr("strike", if st { "sngStrike" } else { "noStrike" });
    }
    if let Some(sz) = size.or_else(|| num(args, "size")) {
        r.set_attr("sz", ((sz.clamp(1.0, 4000.0) * 100.0).round() as i64).to_string());
    }
    if let Some(f) = s(args, "font").filter(|f| !f.trim().is_empty()) {
        let mut l = Element::new(format!("{a}:latin"));
        l.attrs.push(("typeface".into(), f.trim().to_string()));
        put_ordered(r, Some(l), &["latin"], RPR_ORDER);
    }
    match args.get("color") {
        Some(Json::Null) => put_ordered(r, None, FILLS, RPR_ORDER),
        Some(c) => {
            if let Some(h) = c.as_str().and_then(super::text::hex_arg) {
                put_ordered(r, Some(super::text::solid_fill(a, &h, None)), FILLS, RPR_ORDER);
            }
        }
        None => {}
    }
}

fn bad(cmd: &str, m: &str) -> BusError {
    BusError::bad_args(cmd, m)
}

impl StageDoc {
    /// What a command acts on: its `"sel"`, else the `"slide"` and `"shape"`/`"shapes"` it names (the
    /// AI's way), else the slide alone.
    fn place(&self, cmd: &str, args: &Json) -> Result<Place, BusError> {
        if let Some(p) = args.get("sel").and_then(place_of) {
            return Ok(p);
        }
        if args.get("shape").is_some() || args.get("shapes").is_some() {
            let (sl, ids) = self.shapes_of(cmd, args)?;
            return Ok(Place::Shapes(sl, ids));
        }
        if let Some(sl) = args.get("slide").and_then(Json::as_usize).filter(|s| *s < self.deck.slides.len()) {
            return Ok(Place::Blank(sl));
        }
        Err(bad(cmd, "\"sel\" is the selection { anchor, focus } the command acts on (or name \"slide\" and \"shape\"/\"shapes\")"))
    }

    /// The slide a command means: `"slide"`, else the selection's.
    fn slide_of(&self, cmd: &str, args: &Json) -> Result<usize, BusError> {
        let i = match args.get("slide").and_then(Json::as_usize) {
            Some(i) => i,
            None => match args.get("sel").and_then(place_of) {
                Some(Place::Text(_, f)) => f.slide,
                Some(Place::Shapes(sl, _)) | Some(Place::Slide(sl)) | Some(Place::Blank(sl)) => sl,
                None => return Err(bad(cmd, "\"slide\" (from 0) or a \"sel\" says which slide")),
            },
        };
        if i >= self.deck.slides.len() {
            return Err(failed(format!("no slide {} (the deck has {} slides)", i, self.deck.slides.len())));
        }
        Ok(i)
    }

    /// Shapes a command means: `"shapes": [ids]` / `"shape"` on `"slide"`, else the selection's.
    fn shapes_of(&self, cmd: &str, args: &Json) -> Result<(usize, Vec<u32>), BusError> {
        if let Some(list) = args.get("shapes").and_then(Json::as_arr) {
            let slide = self.slide_of(cmd, args)?;
            let ids = list.iter().filter_map(|v| self.shape_key_id(slide, v)).collect::<Vec<_>>();
            if ids.is_empty() {
                return Err(failed("none of those shapes are on the slide"));
            }
            return Ok((slide, ids));
        }
        if let Some(v) = args.get("shape") {
            let slide = self.slide_of(cmd, args)?;
            let id = self.shape_key_id(slide, v).ok_or_else(|| failed(format!("slide {slide} has no shape {v}")))?;
            return Ok((slide, vec![id]));
        }
        match self.place(cmd, args)? {
            Place::Shapes(sl, ids) => Ok((sl, ids)),
            Place::Text(_, f) => Ok((f.slide, vec![f.shape])),
            Place::Slide(_) | Place::Blank(_) => Err(failed("select one or more shapes first")),
        }
    }

    fn shape_key_id(&self, slide: usize, v: &Json) -> Option<u32> {
        let key = ShapeKey::from_json(v)?;
        let root = self.deck.sheet(&self.deck.slides.get(slide)?.part)?;
        let path = find_shape(root, &key)?;
        el_at(root, &path).and_then(shape_id)
    }

    /// A change confined to one slide part.
    fn on_slide(&mut self, slide: usize, f: impl FnOnce(&mut Deck) -> Result<(), BusError>) -> Result<Change, BusError> {
        let part = ops::slide_part(&self.deck, slide)?;
        let snap = Snap::of(&self.deck.pkg, &[&part]);
        if let Err(e) = f(&mut self.deck) {
            if let Some(u) = snap.undo(&self.deck.pkg) {
                let _ = self.restore(&u.args);
            }
            return Err(e);
        }
        self.render_page(slide);
        Ok(match snap.undo(&self.deck.pkg) {
            Some(u) => Change { undo: vec![u], pages_touched: vec![slide] },
            None => Change::default(),
        })
    }

    /// A change that may add, remove or relink parts.
    fn on_package(&mut self, f: impl FnOnce(&mut Deck) -> Result<(), BusError>) -> Result<Change, BusError> {
        let snap = Snap::whole(&self.deck.pkg);
        let r = f(&mut self.deck);
        if let Err(e) = r {
            if let Some(u) = snap.undo(&self.deck.pkg) {
                let _ = self.restore(&u.args);
            }
            return Err(e);
        }
        self.reload()?;
        Ok(match snap.undo(&self.deck.pkg) {
            Some(u) => Change { undo: vec![u], pages_touched: (0..self.deck.slides.len()).collect() },
            None => Change::default(),
        })
    }

    /// Read the deck again from its package (after the slide list or relationships changed).
    pub(crate) fn reload(&mut self) -> Result<(), BusError> {
        let pkg = std::mem::replace(&mut self.deck.pkg, Package::new());
        let backup = pkg.clone();
        match Deck::load(pkg) {
            Ok(d) => self.deck = d,
            Err(e) => {
                self.deck.pkg = backup;
                return Err(failed(e.0));
            }
        }
        self.render_all();
        Ok(())
    }

    /// `stage.restore_parts`.
    pub(crate) fn restore(&mut self, args: &Json) -> Result<Change, BusError> {
        let (inv, done) = snap::restore(&mut self.deck.pkg, args)?;
        for n in &done.names {
            self.images.remove(&name_key(n));
        }
        if done.structural {
            self.reload()?;
            return Ok(Change { undo: vec![inv], pages_touched: (0..self.deck.slides.len()).collect() });
        }
        let mut pages = Vec::new();
        for n in &done.names {
            self.deck.reload_part(n);
            for (i, s) in self.deck.slides.iter().enumerate() {
                if name_key(&s.part) == name_key(n) || s.layout.as_deref().is_some_and(|l| name_key(l) == name_key(n)) || s.master.as_deref().is_some_and(|m| name_key(m) == name_key(n)) {
                    pages.push(i);
                }
            }
        }
        pages.sort_unstable();
        pages.dedup();
        for &p in &pages {
            self.render_page(p);
        }
        Ok(Change { undo: vec![inv], pages_touched: pages })
    }

    fn land(&mut self, cmd: &Command, sel: Selection) {
        self.landed = Some((cmd.clone(), sel));
    }

    /// The resolved transform of a shape (its own, or inherited from its layout placeholder).
    fn resolved_xfrm(&self, slide: usize, id: u32) -> Option<Xfrm> {
        let scene = Scene::new(&self.deck, slide)?;
        let el = find_shape(scene.slide, &ShapeKey::Id(id)).and_then(|p| el_at(scene.slide, &p))?;
        if el.local() == "grpSp" {
            return el.child("grpSpPr").and_then(|g| g.child("xfrm")).map(Xfrm::parse);
        }
        resolve_shape(&scene, el, Owner::Slide).xfrm()
    }

    /// Fit text after an edit: shrink on overflow (`normAutofit`) or resize the shape (`spAutoFit`).
    fn refit(&mut self, slide: usize, id: u32) -> Result<(), BusError> {
        let Some(scene) = Scene::new(&self.deck, slide) else { return Ok(()) };
        let Some(el) = find_shape(scene.slide, &ShapeKey::Id(id)).and_then(|p| el_at(scene.slide, &p)) else { return Ok(()) };
        if el.local() != "sp" {
            return Ok(());
        }
        let Some(tx) = el.child("txBody") else { return Ok(()) };
        let r = resolve_shape(&scene, el, Owner::Slide);
        let body = r.body();
        let Some(x) = r.xfrm() else { return Ok(()) };
        let mode = match body.autofit {
            Some(Autofit::Normal { .. }) => 1,
            Some(Autofit::Shape) => 2,
            _ => return Ok(()),
        };
        let sg = shape_geometry(&r.geometry(), x.ext.0 as f64, x.ext.1 as f64, &[]);
        let rect = sg.text_rect;
        let (l, t, rr, b) = body.insets();
        let inner_w = (rect.w - l - rr).max(1.0);
        let inner_h = (rect.h - t - b).max(0.0);
        let wrap = body.wrap.unwrap_or(true);
        let chain = r.text_chain(&scene);
        let fonts = &mut self.fonts;
        let mut measure = |fs: f64, lr: f64| {
            let paras = resolve_paras(&scene, tx, &chain, fs, lr);
            let block = layout_block(fonts, &paras, inner_w, wrap);
            let natural = block.aligns.iter().map(|a| a.1).fold(0.0, f64::max);
            (block.height, natural)
        };
        if mode == 2 {
            let (h, natural) = measure(1.0, 0.0);
            let extra_h = x.ext.1 as f64 / EMU - rect.h;
            let want_h = h + t + b + extra_h.max(0.0);
            let mut nx = x;
            nx.ext.1 = emu(want_h);
            if !wrap {
                nx.ext.0 = emu(natural + l + rr);
            }
            if (nx.ext.1 - x.ext.1).abs() > 6350 || (nx.ext.0 - x.ext.0).abs() > 6350 {
                ops::with_slide(&mut self.deck, slide, |root| {
                    let ns = Ns::of(root);
                    let sp = ops::shape_mut(root, id)?;
                    ops::set_xfrm(sp, &nx, &ns)
                })?;
            }
            return Ok(());
        }
        let mut steps = vec![(1.0, 0.0), (1.0, 0.1), (1.0, 0.2)];
        let mut f = 0.925;
        while f >= 0.25 {
            steps.push((f, 0.2));
            f -= 0.075;
        }
        let mut pick = *steps.last().unwrap_or(&(1.0, 0.0));
        for &(fs, lr) in &steps {
            if measure(fs, lr).0 <= inner_h + 0.5 {
                pick = (fs, lr);
                break;
            }
        }
        let (old_fs, old_lr) = body.shrink();
        if (old_fs - pick.0).abs() < 1e-6 && (old_lr - pick.1).abs() < 1e-6 && tx.child("bodyPr").is_some_and(|b| b.child("normAutofit").is_some()) {
            return Ok(());
        }
        ops::with_slide(&mut self.deck, slide, |root| {
            let ns = Ns::of(root);
            let sp = ops::shape_mut(root, id)?;
            let bp = ops::body_pr_mut(sp, &ns)?;
            let mut n = Element::new(format!("{}:normAutofit", ns.a));
            if pick.0 < 1.0 {
                n.attrs.push(("fontScale".into(), ((pick.0 * 100000.0).round() as i64).to_string()));
            }
            if pick.1 > 0.0 {
                n.attrs.push(("lnSpcReduction".into(), ((pick.1 * 100000.0).round() as i64).to_string()));
            }
            put_ordered(bp, Some(n), &["noAutofit", "normAutofit", "spAutoFit"], BODYPR_ORDER);
            Ok(())
        })
    }

    /// A text edit on one shape's text, then a refit.
    fn text_edit(&mut self, tp: &TextPos, f: impl FnOnce(&mut Body) -> At) -> Result<(Change, At), BusError> {
        let mut at = tp.at;
        let (slide, shape, cell) = (tp.slide, tp.shape, tp.cell);
        let change = self.on_slide(slide, |deck| {
            at = ops::edit_body(deck, slide, shape, cell, f)?;
            Ok(())
        })?;
        if cell.is_none() && !change.undo.is_empty() {
            // The refit belongs to the same step: patch, then take the snapshot's undo as is.
            let _ = self.refit(slide, shape);
            self.render_page(slide);
        }
        Ok((change, at))
    }

    /// Every text body of the shapes (all cells of a table), formatted as a whole.
    fn format_shapes(&mut self, slide: usize, ids: &[u32], f: &dyn Fn(&mut Body)) -> Result<Change, BusError> {
        let ids = ids.to_vec();
        let ch = self.on_slide(slide, |deck| {
            for id in &ids {
                let (rows, cols, local) = {
                    let root = deck.sheet(&deck.slides[slide].part).ok_or_else(|| failed("the slide cannot be read"))?;
                    let el = find_shape(root, &ShapeKey::Id(*id)).and_then(|p| el_at(root, &p)).ok_or_else(|| failed(format!("no shape {id}")))?;
                    let (r, c) = ops::table_dims(el);
                    (r, c, el.local().to_string())
                };
                if rows > 0 {
                    for r in 0..rows {
                        for c in 0..cols {
                            let _ = ops::edit_body(deck, slide, *id, Some((r, c)), |b| f(b));
                        }
                    }
                } else if local == "sp" {
                    ops::edit_body(deck, slide, *id, None, |b| f(b))?;
                }
            }
            Ok(())
        })?;
        for id in &ids {
            let _ = self.refit(slide, *id);
        }
        self.render_page(slide);
        Ok(ch)
    }

    /// Apply run formatting to whatever the selection is.
    fn format(&mut self, cmd: &Command, args: &Json) -> Result<Change, BusError> {
        let place = self.place(&cmd.name, args)?;
        let grow = flag(args, "grow") == Some(true);
        let shrink = flag(args, "shrink") == Some(true);
        let size = if grow || shrink {
            let st = self.state(&Selection::from_json(args.get("sel").unwrap_or(&Json::Null)).ok_or_else(|| bad(&cmd.name, "sel"))?);
            let cur = st.get("size").and_then(Json::as_f64).unwrap_or(18.0);
            Some(if grow { SIZES.iter().copied().find(|v| *v > cur + 0.01).unwrap_or(cur + 8.0) } else { SIZES.iter().rev().copied().find(|v| *v < cur - 0.01).unwrap_or((cur - 1.0).max(1.0)) })
        } else {
            None
        };
        let a = self.prefix_a();
        let fargs = args.clone();
        match place {
            Place::Text(ta, tf) => {
                let (s, e) = (ta.at, tf.at);
                let (ch, _) = self.text_edit(&tf, |b| {
                    b.format_runs(s, e, &|r| apply_rpr(r, &fargs, &a, size));
                    e
                })?;
                if let Some(sel) = args.get("sel").and_then(Selection::from_json) {
                    self.land(cmd, sel);
                }
                Ok(ch)
            }
            Place::Shapes(slide, ids) => {
                let ch = self.format_shapes(slide, &ids, &|b| {
                    let e = b.end();
                    b.format_runs(At::new(0, 0), e, &|r| apply_rpr(r, &fargs, &a, size));
                })?;
                self.land(cmd, shapes_sel(slide, &ids));
                Ok(ch)
            }
            Place::Slide(_) | Place::Blank(_) => Err(failed("select text or shapes to format")),
        }
    }

    fn prefix_a(&self) -> String {
        "a".to_string()
    }

    /// Apply paragraph formatting to the selected paragraphs (or every paragraph of selected shapes).
    fn format_paras(&mut self, cmd: &Command, args: &Json, f: &dyn Fn(&mut Element, usize)) -> Result<Change, BusError> {
        match self.place(&cmd.name, args)? {
            Place::Text(ta, tf) => {
                let (p0, p1) = (ta.at.para.min(tf.at.para), ta.at.para.max(tf.at.para));
                let (ch, _) = self.text_edit(&tf, |b| {
                    for p in p0..=p1.min(b.paras.len().saturating_sub(1)) {
                        let lvl = b.paras[p].level();
                        b.format_paras(p, p, &|e| f(e, lvl));
                    }
                    tf.at
                })?;
                if let Some(sel) = args.get("sel").and_then(Selection::from_json) {
                    self.land(cmd, sel);
                }
                Ok(ch)
            }
            Place::Shapes(slide, ids) => {
                let ch = self.format_shapes(slide, &ids, &|b| {
                    for p in 0..b.paras.len() {
                        let lvl = b.paras[p].level();
                        b.format_paras(p, p, &|e| f(e, lvl));
                    }
                })?;
                self.land(cmd, shapes_sel(slide, &ids));
                Ok(ch)
            }
            Place::Slide(_) | Place::Blank(_) => Err(failed("select text or shapes first")),
        }
    }

    /// Change the shapes one by one through their XML (with their resolved transform at hand).
    fn each_shape(&mut self, cmd: &Command, args: &Json, f: &dyn Fn(&mut Element, &Ns, Option<Xfrm>, Option<(u32, &super::render::ShapeGeom)>) -> Result<(), BusError>) -> Result<Change, BusError> {
        let (slide, ids) = self.shapes_of(&cmd.name, args)?;
        let xs: Vec<(u32, Option<Xfrm>, Option<super::render::ShapeGeom>)> = ids.iter().map(|i| (*i, self.resolved_xfrm(slide, *i), self.geom(slide, *i).cloned())).collect();
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                let ns = Ns::of(root);
                for (id, x, g) in &xs {
                    let sp = ops::shape_mut(root, *id)?;
                    f(sp, &ns, *x, g.as_ref().map(|g| (*id, g)))?;
                }
                Ok(())
            })
        })?;
        self.land(cmd, shapes_sel(slide, &ids));
        Ok(ch)
    }

    /// The slide as a package for a copy into any deck.
    pub(crate) fn slide_package(&self, slide: usize) -> Option<Json> {
        let e = self.deck.slides.get(slide)?;
        let xml_bytes = self.deck.pkg.part_bytes(&e.part).ok()?;
        let xml = String::from_utf8(xml_bytes).ok()?;
        let mut rels = Vec::new();
        for r in self.deck.pkg.rels(&e.part) {
            if opc::rel_type_matches(&r.rel_type, opc::REL_SLIDE_LAYOUT) || opc::rel_type_matches(&r.rel_type, REL_NOTES_SLIDE) {
                continue;
            }
            let mut j = Json::obj().with("id", r.id.as_str()).with("type", r.rel_type.as_str());
            if r.target_mode_external {
                j.set("external", true);
                j.set("target", r.target.as_str());
            } else {
                let name = self.deck.pkg.resolve(&e.part, r);
                let Ok(b) = self.deck.pkg.part_bytes(&name) else { continue };
                j.set("name", name.as_str());
                j.set("ct", self.deck.pkg.content_type(&name).unwrap_or("application/octet-stream"));
                j.set("b64", snap::b64_encode(&b));
            }
            rels.push(j);
        }
        Some(Json::obj().with("xml", xml).with("layout", self.deck.layout_name(slide)).with("rels", Json::Arr(rels)).with("notes", self.deck.notes_text(slide).map(Json::from).unwrap_or(Json::Null)))
    }

    /// Insert copied slides after `after`; returns the index of the first.
    fn paste_slides(&mut self, after: usize, slides: &[Json]) -> Result<(Change, usize), BusError> {
        let fallback = self.deck.slides.get(after).and_then(|s| s.layout.clone());
        let slides = slides.to_vec();
        let mut first = after + 1;
        let ch = self.on_package(|deck| {
            let mut at = after + 1;
            first = at;
            let mut notes = Vec::new();
            for sl in &slides {
                let xml = sl.get("xml").and_then(Json::as_str).ok_or_else(|| failed("a copied slide has no XML"))?;
                xml::parse_dom(xml.as_bytes()).map_err(|e| failed(format!("a copied slide is not readable: {e}")))?;
                let layout = ops::find_layout(deck, sl.get("layout").unwrap_or(&Json::Null), fallback.as_deref()).or_else(|_| ops::find_layout(deck, &Json::Null, fallback.as_deref()))?;
                let mut rels: Vec<(String, String, String, bool)> = Vec::new();
                let slide_name = (1..).map(|n| format!("/ppt/slides/slide{n}.xml")).find(|n| deck.pkg.part(n).is_none()).unwrap_or_default();
                for r in sl.get("rels").and_then(Json::as_arr).unwrap_or(&[]) {
                    let (Some(id), Some(ty)) = (r.get("id").and_then(Json::as_str), r.get("type").and_then(Json::as_str)) else { continue };
                    if r.get("external").and_then(Json::as_bool) == Some(true) {
                        rels.push((id.into(), ty.into(), r.get("target").and_then(Json::as_str).unwrap_or("").into(), true));
                        continue;
                    }
                    let Some(bytes) = r.get("b64").and_then(Json::as_str).and_then(snap::b64_decode) else { continue };
                    let src = r.get("name").and_then(Json::as_str).unwrap_or("/ppt/media/image.bin");
                    let ct = r.get("ct").and_then(Json::as_str).unwrap_or("application/octet-stream");
                    let existing = deck.pkg.part_names().into_iter().find(|n| name_key(n) == name_key(src) && deck.pkg.part_bytes(n).ok().as_deref() == Some(&bytes[..]));
                    let name = match existing {
                        Some(n) => n,
                        None => {
                            let ext = src.rsplit('.').next().unwrap_or("bin").to_ascii_lowercase();
                            let dir = src.rsplit_once('/').map(|d| d.0).unwrap_or("/ppt/media");
                            let stem = src.rsplit('/').next().unwrap_or("item").trim_end_matches(&format!(".{ext}")).trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
                            let n = (1..).map(|k| format!("{dir}/{stem}{k}.{ext}")).find(|k| deck.pkg.part(k).is_none()).unwrap_or_default();
                            if deck.pkg.content_types().default_for(&ext).is_none() && ct.starts_with("image/") {
                                deck.pkg.set_default_content_type(&ext, ct);
                            }
                            deck.pkg.add_part(&n, ct, bytes).map_err(|e| failed(e.to_string()))?;
                            n
                        }
                    };
                    rels.push((id.into(), ty.into(), opc::relative_target(&slide_name, &name), false));
                }
                let used: std::collections::HashSet<String> = rels.iter().map(|r| r.0.clone()).collect();
                let lid = (1..).map(|k| format!("rId{k}")).find(|k| !used.contains(k)).unwrap_or_default();
                rels.insert(0, (lid, opc::REL_SLIDE_LAYOUT.to_string(), opc::relative_target(&slide_name, &layout), false));
                let name = ops::insert_slide_part(deck, at, xml.as_bytes().to_vec(), rels)?;
                debug_assert_eq!(name, slide_name);
                if let Some(n) = sl.get("notes").and_then(Json::as_str).filter(|n| !n.is_empty()) {
                    notes.push((at, n.to_string()));
                }
                at += 1;
            }
            // Notes pages, once the slides are in the list.
            if !notes.is_empty() {
                let pkg = std::mem::replace(&mut deck.pkg, Package::new());
                *deck = Deck::load(pkg).map_err(|e| failed(e.0))?;
                for (i, n) in notes {
                    super::edit::set_notes(deck, i, &n)?;
                }
            }
            Ok(())
        })?;
        Ok((ch, first))
    }

    /// Copied shapes onto a slide (fresh ids, their pictures brought along, a nudge when the same
    /// shapes are already there).
    fn paste_shapes(&mut self, slide: usize, shapes: &[Json], same_deck_slide: Option<usize>) -> Result<(Change, Vec<u32>), BusError> {
        let shapes = shapes.to_vec();
        let mut ids = Vec::new();
        let part = ops::slide_part(&self.deck, slide)?;
        let ch = self.on_package(|deck| {
            for sh in &shapes {
                let raw = sh.get("xml").and_then(Json::as_str).ok_or_else(|| failed("a copied shape has no XML"))?;
                let rels = sh.get("rels").and_then(Json::as_arr).unwrap_or(&[]).to_vec();
                let map = ops::adopt_rels(deck, &part, &rels);
                let id = ops::with_slide(deck, slide, |root| {
                    let ns = Ns::of(root);
                    ns.declare(root);
                    let mut el = ops::fragment(raw).or_else(|_| {
                        // A shape written with other prefixes: parse it with its own declarations.
                        xml::parse_dom(raw.as_bytes()).map_err(|e| failed(e.to_string()))
                    })?;
                    el.attrs.retain(|(k, _)| !k.starts_with("xmlns"));
                    ops::remap_rids(&mut el, &map);
                    let mut next = ops::next_id(root);
                    let first = next;
                    ops::renumber(&mut el, &mut next);
                    if same_deck_slide == Some(slide) {
                        if let Some(mut x) = ops::own_xfrm(&el).map(Xfrm::parse) {
                            x.off.0 += emu(18.0);
                            x.off.1 += emu(18.0);
                            ops::set_xfrm(&mut el, &x, &ns)?;
                        }
                    }
                    ops::push_shape(root, el)?;
                    Ok(first)
                })?;
                ids.push(id);
            }
            Ok(())
        })?;
        Ok((ch, ids))
    }

    /// The picture placeholder (or empty content placeholder) a selection points at.
    fn picture_target(&self, args: &Json) -> Option<(usize, u32)> {
        let (slide, id) = match args.get("sel").and_then(place_of)? {
            Place::Shapes(sl, ids) if ids.len() == 1 => (sl, ids[0]),
            Place::Text(_, f) if f.cell.is_none() => (f.slide, f.shape),
            _ => return None,
        };
        let root = self.deck.sheet(&self.deck.slides.get(slide)?.part)?;
        let el = find_shape(root, &ShapeKey::Id(id)).and_then(|p| el_at(root, &p))?;
        let ph = Ph::of(el)?;
        let empty = super::model::shape_text(el, None).trim().is_empty();
        (el.local() == "sp" && empty && matches!(ph.ty.as_str(), "pic" | "obj" | "body" | "clipArt")).then_some((slide, id))
    }

    fn default_box(&self, args: &Json, w: f64, h: f64) -> Rect {
        let (sw, sh) = self.slide_size_pt();
        let w = num(args, "w").unwrap_or(w).max(1.0);
        let h = num(args, "h").unwrap_or(h).max(1.0);
        Rect::new(num(args, "x").unwrap_or((sw - w) / 2.0), num(args, "y").unwrap_or((sh - h) / 2.0), w, h)
    }

    /// Apply a hand command, or `None` when the name is not one.
    pub(crate) fn apply_hand(&mut self, cmd: &Command) -> Option<Result<Change, BusError>> {
        let name = cmd.name.as_str();
        let args = &cmd.args;
        let r = match name {
            "stage.restore_parts" => self.restore(args),
            "stage.find" => self.cmd_find(cmd),
            "stage.replace" => self.cmd_replace(cmd),
            "stage.insert_text" => self.cmd_insert_text(cmd),
            "stage.delete" => self.cmd_delete(cmd),
            "stage.enter" => self.cmd_enter(cmd),
            "stage.tab" => self.cmd_tab(cmd),
            "stage.select_all" => self.cmd_select_all(cmd),
            "stage.select_slide" => self.slide_of(name, args).map(|sl| {
                self.land(cmd, slide_sel(sl));
                Change::default()
            }),
            "stage.arrow" => self.cmd_arrow(cmd),
            "stage.paste" => self.cmd_paste(cmd),
            "stage.format" => self.format(cmd, args),
            "stage.bold" | "stage.italic" | "stage.underline" | "stage.strike" => self.cmd_toggle(cmd),
            "stage.grow_font" | "stage.shrink_font" => {
                let mut a = args.clone();
                a.set(if name == "stage.grow_font" { "grow" } else { "shrink" }, true);
                self.format(cmd, &a)
            }
            "stage.align" => {
                let al = match s(args, "align").unwrap_or("left") {
                    "center" | "ctr" | "centre" => "ctr",
                    "right" | "r" => "r",
                    "justify" | "just" => "just",
                    _ => "l",
                };
                self.format_paras(cmd, args, &|p, _| p.set_attr("algn", al))
            }
            "stage.line_spacing" => {
                let v = num(args, "value").unwrap_or(1.0).clamp(0.1, 10.0);
                self.format_paras(cmd, args, &|p, _| {
                    let a = p.prefix().unwrap_or("a").to_string();
                    let mut l = Element::new(format!("{a}:lnSpc"));
                    let mut pc = Element::new(format!("{a}:spcPct"));
                    pc.attrs.push(("val".into(), ((v * 100000.0).round() as i64).to_string()));
                    l.children.push(Node::Element(pc));
                    put_ordered(p, Some(l), &["lnSpc"], PPR_ORDER);
                })
            }
            "stage.bullets" | "stage.numbering" => self.cmd_list(cmd),
            "stage.list_level" => {
                let d = args.get("delta").and_then(Json::as_i64).unwrap_or(1);
                self.format_paras(cmd, args, &|p, lvl| set_level(p, lvl, d))
            }
            "stage.autofit" => self.cmd_autofit(cmd),
            "stage.wrap" => {
                let on = flag(args, "on").unwrap_or(true);
                self.cmd_body(cmd, &|bp, _| bp.set_attr("wrap", if on { "square" } else { "none" }))
            }
            "stage.insert_text_box" => self.cmd_insert_text_box(cmd),
            "stage.insert_shape" => self.cmd_insert_shape(cmd),
            "stage.insert_line" => self.cmd_insert_line(cmd),
            "stage.insert_picture" => self.cmd_insert_picture(cmd),
            "stage.insert_table" => self.cmd_insert_table(cmd),
            "stage.table_insert" | "stage.table_delete" => self.cmd_table_change(cmd),
            "stage.table_size" => self.cmd_table_size(cmd),
            "stage.cell_fill" => self.cmd_cell_fill(cmd),
            "stage.fill" => {
                let color = args.get("color").cloned().unwrap_or(Json::Null);
                let hex = color.as_str().and_then(super::text::hex_arg);
                if !color.is_null() && hex.is_none() && color.as_str() != Some("none") {
                    return Some(Err(bad(name, "\"color\" is RRGGBB, or null (or \"none\") for no fill")));
                }
                let alpha = num(args, "alpha");
                self.each_shape(cmd, args, &|sp, ns, _, _| ops::set_fill(sp, hex.as_deref(), alpha, ns))
            }
            "stage.outline" => {
                let color = match args.get("color") {
                    None => None,
                    Some(Json::Null) => Some(None),
                    Some(c) if c.as_str() == Some("none") => Some(None),
                    Some(c) => match c.as_str().and_then(super::text::hex_arg) {
                        Some(h) => Some(Some(h)),
                        None => return Some(Err(bad(name, "\"color\" is RRGGBB, or null for no line"))),
                    },
                };
                let width = num(args, "width");
                let dash = s(args, "dash").map(str::to_string);
                let head = s(args, "head").map(str::to_string);
                let tail = s(args, "tail").map(str::to_string);
                self.each_shape(cmd, args, &|sp, ns, _, _| {
                    let o = Outline { color: color.as_ref().map(|c| c.as_deref()), width, dash: dash.as_deref(), head: head.as_deref(), tail: tail.as_deref() };
                    ops::set_outline(sp, &o, ns)
                })
            }
            "stage.crop" => {
                let v = |k: &str| num(args, k).unwrap_or(0.0).clamp(-1.0, 0.99);
                let (l, t, r, b) = (v("l"), v("t"), v("r"), v("b"));
                if l + r >= 1.0 || t + b >= 1.0 {
                    return Some(Err(failed("the crop leaves nothing of the picture")));
                }
                self.each_shape(cmd, args, &|sp, ns, _, _| {
                    if sp.local() != "pic" {
                        return Err(failed("only a picture can be cropped"));
                    }
                    let bf = sp.child_mut("blipFill").ok_or_else(|| failed("the picture has no fill"))?;
                    let mut src = Element::new(format!("{}:srcRect", ns.a));
                    for (k, val) in [("l", l), ("t", t), ("r", r), ("b", b)] {
                        if val.abs() > 1e-9 {
                            src.attrs.push((k.into(), ((val * 100000.0).round() as i64).to_string()));
                        }
                    }
                    let at = bf.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "srcRect"));
                    bf.children.retain(|c| !matches!(c, Node::Element(e) if e.local() == "srcRect"));
                    let at = at.unwrap_or_else(|| bf.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "blip")).map(|i| i + 1).unwrap_or(0));
                    bf.children.insert(at, Node::Element(src));
                    Ok(())
                })
            }
            "stage.order" => {
                let to = s(args, "to").unwrap_or("front").to_string();
                match self.shapes_of(name, args) {
                    Err(e) => Err(e),
                    Ok((slide, ids)) => {
                        let mut order = ids.clone();
                        if matches!(to.as_str(), "back" | "forward") {
                            order.reverse();
                        }
                        let r = self.on_slide(slide, |deck| {
                            ops::with_slide(deck, slide, |root| {
                                for id in &order {
                                    ops::reorder(root, *id, &to)?;
                                }
                                Ok(())
                            })
                        });
                        self.land(cmd, shapes_sel(slide, &ids));
                        r
                    }
                }
            }
            "stage.group" => self.cmd_group(cmd),
            "stage.ungroup" => self.cmd_ungroup(cmd),
            "stage.regroup" => self.cmd_regroup(cmd),
            "stage.align_shapes" => self.cmd_align_shapes(cmd),
            "stage.rotate" => {
                let by = num(args, "by");
                let to = num(args, "to");
                if by.is_none() && to.is_none() {
                    return Some(Err(bad(name, "\"by\" or \"to\" (degrees)")));
                }
                self.each_shape(cmd, args, &|sp, ns, x, _| {
                    let mut x = x.ok_or_else(|| failed("the shape has no position to rotate"))?;
                    let deg = to.unwrap_or(x.rotation_deg() + by.unwrap_or(0.0));
                    x.rot = ((deg * 60000.0).round() as i64).rem_euclid(21_600_000);
                    ops::set_xfrm(sp, &x, ns)
                })
            }
            "stage.flip" => {
                let h = matches!(s(args, "axis").unwrap_or("h"), "h" | "horizontal" | "x");
                self.each_shape(cmd, args, &|sp, ns, x, _| {
                    let mut x = x.ok_or_else(|| failed("the shape has no position to flip"))?;
                    if h {
                        x.flip_h = !x.flip_h;
                    } else {
                        x.flip_v = !x.flip_v;
                    }
                    ops::set_xfrm(sp, &x, ns)
                })
            }
            "stage.size" => {
                let (w, h) = (num(args, "w"), num(args, "h"));
                let scale = num(args, "scale").filter(|k| *k > 0.0);
                let lock = flag(args, "lock").unwrap_or(false);
                if w.is_none() && h.is_none() && scale.is_none() {
                    return Some(Err(bad(name, "\"w\" and/or \"h\" in points, or \"scale\"")));
                }
                self.each_shape(cmd, args, &|sp, ns, x, _| {
                    let mut x = x.ok_or_else(|| failed("the shape has no size to change"))?;
                    let (ow, oh) = (x.ext.0 as f64 / EMU, x.ext.1 as f64 / EMU);
                    let (mut nw, mut nh) = (w.unwrap_or(ow), h.unwrap_or(oh));
                    if let Some(k) = scale {
                        // Scaled about the centre.
                        nw = ow * k;
                        nh = oh * k;
                        x.off.0 -= emu((nw - ow) / 2.0);
                        x.off.1 -= emu((nh - oh) / 2.0);
                    }
                    if lock && ow > 0.0 && oh > 0.0 {
                        if w.is_some() && h.is_none() {
                            nh = oh * nw / ow;
                        } else if h.is_some() && w.is_none() {
                            nw = ow * nh / oh;
                        }
                    }
                    x.ext = (emu(nw.max(0.0)), emu(nh.max(0.0)));
                    if sp.local() == "graphicFrame" {
                        return Err(failed("size a table by its columns and rows (stage.table_size)"));
                    }
                    ops::set_xfrm(sp, &x, ns)
                })
            }
            "stage.position" | "stage.nudge" => {
                let abs = name == "stage.position";
                let (px, py) = (num(args, "x"), num(args, "y"));
                let (dx, dy) = (num(args, "dx").unwrap_or(0.0), num(args, "dy").unwrap_or(0.0));
                self.each_shape(cmd, args, &|sp, ns, x, g| {
                    let mut x = x.ok_or_else(|| failed("the shape has no position"))?;
                    let parent = g.map(|(_, g)| ops::parent_matrix(&g.m, &x)).unwrap_or_default();
                    let (mx, my) = if abs {
                        let b = g.map(|(_, g)| g.m.apply_rect(&Rect::new(0.0, 0.0, g.w, g.h))).unwrap_or_else(|| x.rect());
                        (px.map(|v| v - b.x).unwrap_or(0.0), py.map(|v| v - b.y).unwrap_or(0.0))
                    } else {
                        (dx, dy)
                    };
                    ops::move_by(&mut x, mx, my, &parent);
                    ops::set_xfrm(sp, &x, ns)
                })
            }
            "stage.duplicate" => self.cmd_duplicate(cmd),
            "stage.delete_shapes" => self.cmd_delete_shapes(cmd),
            "stage.new_slide" => self.cmd_new_slide(cmd),
            "stage.duplicate_slide" => {
                let slide = match self.slide_of(name, args) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let r = self.on_package(|deck| ops::duplicate_slide(deck, slide).map(|_| ()));
                if r.is_ok() {
                    self.land(cmd, slide_sel(slide + 1));
                }
                r
            }
            "stage.delete_slide" => {
                let slide = match self.slide_of(name, args) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                if self.deck.slides.len() <= 1 {
                    return Some(Err(failed("a deck keeps at least one slide")));
                }
                let r = self.on_package(|deck| ops::delete_slide(deck, slide));
                if r.is_ok() {
                    self.land(cmd, slide_sel(slide.min(self.deck.slides.len().saturating_sub(1))));
                }
                r
            }
            "stage.move_slide" => {
                let n = self.deck.slides.len();
                let from = match args.get("from").and_then(Json::as_usize).or_else(|| self.slide_of(name, args).ok()) {
                    Some(f) if f < n => f,
                    _ => return Some(Err(bad(name, "\"from\" is a slide index from 0"))),
                };
                let by = args.get("by").and_then(Json::as_i64).map(|b| (from as i64 + b).clamp(0, n as i64 - 1) as usize);
                let to = match args.get("to").and_then(Json::as_usize).or(by) {
                    Some(t) if t < n => t,
                    _ => return Some(Err(bad(name, &format!("\"to\" is the new index, 0 to {} (or \"by\": +1 / -1)", n - 1)))),
                };
                if to == from {
                    self.land(cmd, slide_sel(to));
                    return Some(Ok(Change::default()));
                }
                let r = self.on_package(|deck| ops::move_slide(deck, from, to));
                if r.is_ok() {
                    self.land(cmd, slide_sel(to));
                }
                r
            }
            "stage.set_layout" => {
                let slide = match self.slide_of(name, args) {
                    Ok(s) => s,
                    Err(e) => return Some(Err(e)),
                };
                let layout = match ops::find_layout(&self.deck, args.get("layout").unwrap_or(&Json::Null), None) {
                    Ok(l) => l,
                    Err(e) => return Some(Err(e)),
                };
                let boxes: Vec<(u32, Xfrm)> = self.geo.get(slide).map(|g| g.iter().filter_map(|s| self.resolved_xfrm(slide, s.id).map(|x| (s.id, x))).collect()).unwrap_or_default();
                let r = self.on_package(|deck| ops::set_layout(deck, slide, &layout, &boxes));
                if r.is_ok() {
                    self.land(cmd, slide_sel(slide));
                }
                r
            }
            _ => return None,
        };
        Some(r)
    }

    fn cmd_insert_text(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let args = &cmd.args;
        let text = s(args, "text").ok_or_else(|| bad(&cmd.name, "\"text\" is a string"))?.to_string();
        match self.place(&cmd.name, args)? {
            Place::Text(a, f) => {
                let (s0, e0) = (a.at, f.at);
                let t = text.clone();
                let (ch, at) = self.text_edit(&f, |b| {
                    let st = b.delete(s0, e0);
                    b.insert(st, &t, None)
                })?;
                self.land(cmd, Selection::caret(TextPos { at, ..f }.pos()));
                Ok(ch)
            }
            Place::Shapes(slide, ids) if ids.len() == 1 && self.geom(slide, ids[0]).is_some_and(|g| g.text.is_some()) => {
                let tp = TextPos { slide, shape: ids[0], cell: None, at: At::new(0, 0) };
                let t = text.clone();
                let (ch, at) = self.text_edit(&tp, |b| {
                    let e = b.end();
                    let st = b.delete(At::new(0, 0), e);
                    b.insert(st, &t, None)
                })?;
                self.land(cmd, Selection::caret(TextPos { at, ..tp }.pos()));
                Ok(ch)
            }
            Place::Shapes(..) => Err(failed("put the caret in a text, or select one shape that can hold text")),
            Place::Slide(_) | Place::Blank(_) => Err(failed("put the caret in a text first")),
        }
    }

    fn cmd_delete(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let args = &cmd.args;
        let back = flag(args, "back").unwrap_or(true);
        let word = flag(args, "word").unwrap_or(false);
        match self.place(&cmd.name, args)? {
            Place::Text(a, f) => {
                let body = self.body_of(&f).ok_or_else(|| failed("that text is gone"))?;
                let (s0, e0) = if a.at != f.at {
                    (a.at.min(f.at), a.at.max(f.at))
                } else {
                    let at = body.clamp(f.at);
                    let other = if back {
                        if at.ch > 0 {
                            At::new(at.para, if word { super::text::word_left(&body.chars(at.para), at.ch) } else { at.ch - 1 })
                        } else if at.para > 0 {
                            At::new(at.para - 1, body.paras[at.para - 1].len())
                        } else {
                            at
                        }
                    } else if at.ch < body.paras[at.para].len() {
                        At::new(at.para, if word { super::text::word_right(&body.chars(at.para), at.ch) } else { at.ch + 1 })
                    } else if at.para + 1 < body.paras.len() {
                        At::new(at.para + 1, 0)
                    } else {
                        at
                    };
                    (at.min(other), at.max(other))
                };
                if s0 == e0 {
                    self.land(cmd, Selection::caret(f.pos()));
                    return Ok(Change::default());
                }
                let (ch, at) = self.text_edit(&f, |b| b.delete(s0, e0))?;
                self.land(cmd, Selection::caret(TextPos { at, ..f }.pos()));
                Ok(ch)
            }
            Place::Shapes(..) => self.cmd_delete_shapes(cmd),
            Place::Blank(_) => Ok(Change::default()),
            Place::Slide(slide) => {
                if self.deck.slides.len() <= 1 {
                    return Err(failed("a deck keeps at least one slide"));
                }
                let r = self.on_package(|deck| ops::delete_slide(deck, slide))?;
                self.land(cmd, slide_sel(slide.min(self.deck.slides.len() - 1)));
                Ok(r)
            }
        }
    }

    fn cmd_delete_shapes(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let (slide, ids) = self.shapes_of(&cmd.name, &cmd.args)?;
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                for id in &ids {
                    ops::remove_shape(root, *id)?;
                }
                Ok(())
            })
        })?;
        self.land(cmd, slide_sel(slide));
        Ok(ch)
    }

    fn cmd_enter(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let shift = flag(&cmd.args, "shift").unwrap_or(false);
        match self.place(&cmd.name, &cmd.args)? {
            Place::Text(a, f) => {
                let (s0, e0) = (a.at, f.at);
                let (ch, at) = self.text_edit(&f, |b| {
                    let st = b.delete(s0, e0);
                    if shift {
                        b.line_break(st)
                    } else {
                        b.split(st)
                    }
                })?;
                self.land(cmd, Selection::caret(TextPos { at, ..f }.pos()));
                Ok(ch)
            }
            Place::Shapes(slide, ids) if ids.len() == 1 && self.geom(slide, ids[0]).is_some_and(|g| g.text.is_some()) => {
                // Enter on a selected shape puts the caret at the end of its text.
                let tp = TextPos { slide, shape: ids[0], cell: None, at: At::new(0, 0) };
                let end = self.body_of(&tp).map(|b| b.end()).unwrap_or(tp.at);
                self.land(cmd, Selection::caret(TextPos { at: end, ..tp }.pos()));
                Ok(Change::default())
            }
            _ => Ok(Change::default()),
        }
    }

    fn cmd_tab(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let back = flag(&cmd.args, "back").unwrap_or(false);
        let Place::Text(a, f) = self.place(&cmd.name, &cmd.args)? else { return Ok(Change::default()) };
        if let Some((r, c)) = f.cell {
            let (rows, cols) = {
                let root = self.deck.sheet(&self.deck.slides[f.slide].part).ok_or_else(|| failed("the slide cannot be read"))?;
                let el = find_shape(root, &ShapeKey::Id(f.shape)).and_then(|p| el_at(root, &p)).ok_or_else(|| failed("the table is gone"))?;
                ops::table_dims(el)
            };
            let k = r * cols + c;
            let mut change = Change::default();
            let next = if back {
                k.saturating_sub(1)
            } else if k + 1 >= rows * cols {
                // Tab in the last cell adds a row.
                let id = f.shape;
                change = self.on_slide(f.slide, |deck| ops::with_slide(deck, f.slide, |root| ops::table_change(ops::shape_mut(root, id)?, "row", rows - 1, true, true)))?;
                k + 1
            } else {
                k + 1
            };
            let cell = (next / cols.max(1), next % cols.max(1));
            let tp = TextPos { cell: Some(cell), at: At::new(0, 0), ..f };
            let end = self.body_of(&tp).map(|b| b.end()).unwrap_or(tp.at);
            self.land(cmd, Selection::new(TextPos { at: At::new(0, 0), ..tp }.pos(), TextPos { at: end, ..tp }.pos()));
            return Ok(change);
        }
        let multi = a.at.para != f.at.para;
        let body = self.body_of(&f).ok_or_else(|| failed("that text is gone"))?;
        let st = self.state(&Selection::caret(f.pos()));
        let listy = st.get("bullets").and_then(Json::as_bool) == Some(true) || st.get("numbered").and_then(Json::as_bool) == Some(true);
        let at_start = a.at == f.at && body.clamp(f.at).ch == 0;
        if multi || listy && (at_start || a.at != f.at) || at_start && body.paras.len() > 1 {
            let d = if back { -1 } else { 1 };
            let mut c2 = cmd.clone();
            c2.args.set("delta", d);
            let r = self.format_paras(&c2, &c2.args.clone(), &|p, lvl| set_level(p, lvl, d))?;
            if let Some(sel) = cmd.args.get("sel").and_then(Selection::from_json) {
                self.land(cmd, sel);
            }
            return Ok(r);
        }
        if back {
            return Ok(Change::default());
        }
        let (s0, e0) = (a.at, f.at);
        let (ch, at) = self.text_edit(&f, |b| {
            let st = b.delete(s0, e0);
            b.insert(st, "\t", None)
        })?;
        self.land(cmd, Selection::caret(TextPos { at, ..f }.pos()));
        Ok(ch)
    }

    fn cmd_select_all(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let sel = match self.place(&cmd.name, &cmd.args)? {
            Place::Text(_, f) => {
                let body = self.body_of(&f).ok_or_else(|| failed("that text is gone"))?;
                let all = Selection::new(TextPos { at: At::new(0, 0), ..f }.pos(), TextPos { at: body.end(), ..f }.pos());
                if body.is_empty() {
                    shapes_sel(f.slide, &self.top_ids(f.slide))
                } else {
                    all
                }
            }
            Place::Shapes(slide, ids) => {
                let all = self.top_ids(slide);
                if !all.is_empty() && all.iter().all(|i| ids.contains(i)) {
                    slide_sel(slide)
                } else {
                    shapes_sel(slide, &all)
                }
            }
            Place::Slide(slide) => slide_sel(slide),
            Place::Blank(slide) => shapes_sel(slide, &self.top_ids(slide)),
        };
        self.land(cmd, sel);
        Ok(Change::default())
    }

    fn top_ids(&self, slide: usize) -> Vec<u32> {
        self.geo.get(slide).map(|g| g.iter().filter(|s| s.group.is_none()).map(|s| s.id).collect()).unwrap_or_default()
    }

    fn cmd_arrow(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let args = &cmd.args;
        let dir = s(args, "dir").unwrap_or("right").to_string();
        let extend = flag(args, "extend").unwrap_or(false);
        match self.place(&cmd.name, args)? {
            Place::Text(a, f) => {
                let sel = self.arrow_text(a, f, &dir, extend);
                self.land(cmd, sel);
                Ok(Change::default())
            }
            Place::Shapes(..) => {
                let step = if flag(args, "fine") == Some(true) { 1.0 } else { 6.0 };
                let (dx, dy) = match dir.as_str() {
                    "left" => (-step, 0.0),
                    "right" => (step, 0.0),
                    "up" => (0.0, -step),
                    _ => (0.0, step),
                };
                let mut c = cmd.clone();
                c.name = "stage.nudge".into();
                c.args.set("dx", dx);
                c.args.set("dy", dy);
                let r = self.apply_hand(&c).unwrap_or_else(|| Ok(Change::default()));
                if let Some((_, sel)) = self.landed.clone() {
                    self.land(cmd, sel);
                }
                r
            }
            Place::Blank(_) => Ok(Change::default()),
            Place::Slide(slide) => {
                let n = self.deck.slides.len();
                let to = match dir.as_str() {
                    "left" | "up" => slide.saturating_sub(1),
                    _ => (slide + 1).min(n - 1),
                };
                self.land(cmd, slide_sel(to));
                Ok(Change::default())
            }
        }
    }

    fn cmd_toggle(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let key = cmd.name.trim_start_matches("stage.");
        let sel = cmd.args.get("sel").and_then(Selection::from_json).ok_or_else(|| bad(&cmd.name, "\"sel\" is the selection"))?;
        let on = match cmd.args.get("on").and_then(Json::as_bool) {
            Some(v) => v,
            None => self.state(&sel).get(key).and_then(Json::as_bool) != Some(true),
        };
        let mut a = cmd.args.clone();
        a.set(key, on);
        self.format(cmd, &a)
    }

    fn cmd_list(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let numbered = cmd.name == "stage.numbering";
        let sel = cmd.args.get("sel").and_then(Selection::from_json);
        let now = sel.map(|s| self.state(&s)).unwrap_or_else(Json::obj);
        let key = if numbered { "numbered" } else { "bullets" };
        let on = flag(&cmd.args, "on").unwrap_or(now.get(key).and_then(Json::as_bool) != Some(true));
        let scheme = s(&cmd.args, "scheme").unwrap_or("arabicPeriod").to_string();
        let ch = s(&cmd.args, "char").unwrap_or("\u{2022}").to_string();
        self.format_paras(cmd, &cmd.args.clone(), &|p, lvl| {
            let a = p.prefix().unwrap_or("a").to_string();
            for n in ["buNone", "buAutoNum", "buChar", "buBlip"] {
                put_ordered(p, None, &[n], PPR_ORDER);
            }
            if !on {
                put_ordered(p, Some(Element::new(format!("{a}:buNone"))), &[], PPR_ORDER);
                return;
            }
            if p.attr("marL").is_none() {
                p.set_attr("marL", (342900 + 457200 * lvl as i64).to_string());
                p.set_attr("indent", "-342900");
            }
            if numbered {
                put_ordered(p, None, &["buFont"], PPR_ORDER);
                let mut e = Element::new(format!("{a}:buFont"));
                e.attrs.push(("typeface".into(), "+mj-lt".into()));
                put_ordered(p, Some(e), &[], PPR_ORDER);
                let mut n = Element::new(format!("{a}:buAutoNum"));
                n.attrs.push(("type".into(), scheme.clone()));
                put_ordered(p, Some(n), &[], PPR_ORDER);
            } else {
                let mut f = Element::new(format!("{a}:buFont"));
                f.attrs.push(("typeface".into(), "Arial".into()));
                put_ordered(p, Some(f), &["buFont", "buFontTx"], PPR_ORDER);
                let mut c = Element::new(format!("{a}:buChar"));
                c.attrs.push(("char".into(), ch.clone()));
                put_ordered(p, Some(c), &[], PPR_ORDER);
            }
        })
    }

    /// Change the body properties of the selected shapes' text.
    fn cmd_body(&mut self, cmd: &Command, f: &dyn Fn(&mut Element, &Ns)) -> Result<Change, BusError> {
        let (slide, ids) = self.shapes_of(&cmd.name, &cmd.args)?;
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                let ns = Ns::of(root);
                for id in &ids {
                    let sp = ops::shape_mut(root, *id)?;
                    if sp.local() != "sp" {
                        continue;
                    }
                    let bp = ops::body_pr_mut(sp, &ns)?;
                    f(bp, &ns);
                }
                Ok(())
            })
        })?;
        for id in &ids {
            let _ = self.refit(slide, *id);
        }
        self.render_page(slide);
        if let Some(sel) = cmd.args.get("sel").and_then(Selection::from_json) {
            self.land(cmd, sel);
        }
        Ok(ch)
    }

    fn cmd_autofit(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let mode = s(&cmd.args, "mode").unwrap_or("shrink").to_string();
        let local = match mode.as_str() {
            "shrink" | "normal" => "normAutofit",
            "resize" | "shape" => "spAutoFit",
            "none" | "off" => "noAutofit",
            _ => return Err(bad(&cmd.name, "\"mode\" is shrink, resize or none")),
        };
        self.cmd_body(cmd, &|bp, ns| {
            put_ordered(bp, Some(Element::new(format!("{}:{local}", ns.a))), &["noAutofit", "normAutofit", "spAutoFit"], BODYPR_ORDER);
        })
    }

    fn insert_xml(&mut self, slide: usize, make: impl FnOnce(u32, &Element) -> String) -> Result<(Change, u32), BusError> {
        let mut id = 0;
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                let ns = Ns::of(root);
                id = ops::next_id(root);
                let xml = make(id, root);
                let el = ops::fragment(&xml)?;
                let el = if ns.p == "p" && ns.a == "a" { el } else { ops::fragment(&ns.fix(&xml)).unwrap_or(el) };
                ops::push_shape(root, el)
            })
        })?;
        Ok((ch, id))
    }

    fn cmd_insert_text_box(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let slide = self.slide_of(&cmd.name, &cmd.args)?;
        let text = s(&cmd.args, "text").unwrap_or("").to_string();
        let r = self.default_box(&cmd.args, 240.0, 40.0);
        let (ch, id) = self.insert_xml(slide, |id, root| ops::text_box_xml(id, &ops::fresh_name(root, "TextBox", id), r, &text))?;
        let _ = self.refit(slide, id);
        self.render_page(slide);
        let tp = TextPos { slide, shape: id, cell: None, at: At::new(0, 0) };
        let end = self.body_of(&tp).map(|b| b.end()).unwrap_or(tp.at);
        self.land(cmd, Selection::caret(TextPos { at: end, ..tp }.pos()));
        Ok(ch)
    }

    fn cmd_insert_shape(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let slide = self.slide_of(&cmd.name, &cmd.args)?;
        let preset = s(&cmd.args, "preset").unwrap_or("rect").to_string();
        if !preset_names().contains(&preset.as_str()) {
            let some: Vec<&str> = preset_names().into_iter().take(40).collect();
            return Err(failed(format!("there is no preset shape '{preset}'; some are: {}…", some.join(", "))));
        }
        let text = s(&cmd.args, "text").unwrap_or("").to_string();
        let r = self.default_box(&cmd.args, 144.0, 96.0);
        let base = {
            let mut c = preset.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        };
        let (ch, id) = self.insert_xml(slide, |id, root| ops::preset_xml(id, &ops::fresh_name(root, &base, id), &preset, r, &text))?;
        self.land(cmd, shapes_sel(slide, &[id]));
        Ok(ch)
    }

    fn cmd_insert_line(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let slide = self.slide_of(&cmd.name, &cmd.args)?;
        let (sw, sh) = self.slide_size_pt();
        let a = &cmd.args;
        let (x1, y1) = (num(a, "x1").unwrap_or(sw * 0.35), num(a, "y1").unwrap_or(sh / 2.0));
        let (x2, y2) = (num(a, "x2").unwrap_or(sw * 0.65), num(a, "y2").unwrap_or(sh / 2.0));
        let head = s(a, "head").map(str::to_string);
        let tail = s(a, "tail").map(str::to_string);
        let base = if head.is_some() || tail.is_some() { "Straight Arrow Connector" } else { "Straight Connector" };
        let (ch, id) = self.insert_xml(slide, |id, root| ops::line_xml(id, &ops::fresh_name(root, base, id), x1, y1, x2, y2, head.as_deref(), tail.as_deref()))?;
        self.land(cmd, shapes_sel(slide, &[id]));
        Ok(ch)
    }

    fn cmd_insert_picture(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let a = &cmd.args;
        let data = s(a, "data").ok_or_else(|| bad(&cmd.name, "\"data\" is the picture's bytes in base64"))?;
        let bytes = snap::b64_decode(data).filter(|b| !b.is_empty()).ok_or_else(|| failed("the picture's bytes are not readable base64"))?;
        let fname = s(a, "name").unwrap_or("picture").to_string();
        let (ext, ct) = ops::sniff_image(&bytes, &fname).ok_or_else(|| failed("that is not a picture this editor knows (PNG, JPEG, GIF, BMP or SVG)"))?;
        let natural = ops::image_size_pt(&bytes);
        let target = self.picture_target(a);
        let slide = match target {
            Some((sl, _)) => sl,
            None => self.slide_of(&cmd.name, a)?,
        };
        let part = ops::slide_part(&self.deck, slide)?;
        let (sw, sh) = self.slide_size_pt();
        let (nw, nh) = natural.unwrap_or((sw * 0.4, sh * 0.4));
        let target_box = target.and_then(|(_, id)| self.geom(slide, id).map(|g| (id, g.m.apply_rect(&Rect::new(0.0, 0.0, g.w, g.h)))));
        let mut new_id = 0;
        let descr = fname.clone();
        let user_box = num(a, "x").is_some() || num(a, "w").is_some();
        let ch = self.on_package(|deck| {
            let rid = ops::add_media(deck, &part, bytes, ext, ct, opc::REL_IMAGE)?;
            ops::with_slide(deck, slide, |root| {
                let ns = Ns::of(root);
                ns.declare(root);
                match target_box {
                    Some((pid, b)) => {
                        // Into the placeholder: the picture takes its place and box, cropped to fill.
                        let (iw, ih) = (nw.max(1.0), nh.max(1.0));
                        let (ar_box, ar_img) = (b.w / b.h.max(1.0), iw / ih);
                        let crop = if ar_img > ar_box {
                            let keep = ar_box / ar_img;
                            ((1.0 - keep) / 2.0, 0.0, (1.0 - keep) / 2.0, 0.0)
                        } else {
                            let keep = ar_img / ar_box;
                            (0.0, (1.0 - keep) / 2.0, 0.0, (1.0 - keep) / 2.0)
                        };
                        let sp = ops::shape_mut(root, pid)?;
                        let nv = sp.child("nvSpPr").cloned().ok_or_else(|| failed("the placeholder has no properties"))?;
                        let sp_pr = sp.child("spPr").cloned();
                        let xml = ops::picture_xml(pid, &super::model::shape_name(sp), &descr, &rid, b, Some(crop));
                        let mut pic = ops::fragment(&xml)?;
                        // Keep the placeholder's identity (its p:ph) and its own position, if any.
                        if let Some(nvpic) = pic.child_mut("nvPicPr") {
                            if let (Some(ph_nv), Some(slot)) = (nv.child("nvPr").cloned(), nvpic.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "nvPr"))) {
                                nvpic.children[slot] = Node::Element(ph_nv);
                            }
                        }
                        if let Some(own) = sp_pr {
                            if own.child("xfrm").is_none() {
                                if let Some(slot) = pic.children.iter().position(|c| matches!(c, Node::Element(e) if e.local() == "spPr")) {
                                    let mut e = own.clone();
                                    e.name = format!("{}:spPr", ns.p);
                                    pic.children[slot] = Node::Element(e);
                                }
                            }
                        }
                        new_id = pid;
                        let (parent, slot) = {
                            let path = ops::shape_path(root, pid)?;
                            (path[..path.len() - 1].to_vec(), *path.last().unwrap_or(&0))
                        };
                        let par = el_at_mut_path(root, &parent)?;
                        let idx = par.children.iter().enumerate().filter(|(_, c)| matches!(c, Node::Element(_))).nth(slot).map(|(i, _)| i).ok_or_else(|| failed("placeholder not found"))?;
                        par.children[idx] = Node::Element(pic);
                        Ok(())
                    }
                    None => {
                        let (mut w, mut h) = (nw, nh);
                        let k = (sw * 0.8 / w).min(sh * 0.8 / h).min(1.0);
                        w *= k;
                        h *= k;
                        if let (Some(uw), None) = (num(a, "w"), num(a, "h")) {
                            h = h * uw / w.max(1e-9);
                            w = uw;
                        } else if let (Some(uw), Some(uh)) = (num(a, "w"), num(a, "h")) {
                            w = uw;
                            h = uh;
                        }
                        let r = if user_box { Rect::new(num(a, "x").unwrap_or((sw - w) / 2.0), num(a, "y").unwrap_or((sh - h) / 2.0), w, h) } else { Rect::new((sw - w) / 2.0, (sh - h) / 2.0, w, h) };
                        new_id = ops::next_id(root);
                        let xml = ops::picture_xml(new_id, &ops::fresh_name(root, "Picture", new_id), &descr, &rid, r, None);
                        ops::push_shape(root, ops::fragment(&xml)?)
                    }
                }
            })
        })?;
        self.land(cmd, shapes_sel(slide, &[new_id]));
        Ok(ch)
    }

    fn cmd_insert_table(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let slide = self.slide_of(&cmd.name, &cmd.args)?;
        let rows = cmd.args.get("rows").and_then(Json::as_usize).unwrap_or(3).clamp(1, 75);
        let cols = cmd.args.get("cols").and_then(Json::as_usize).unwrap_or(3).clamp(1, 75);
        let (sw, _) = self.slide_size_pt();
        let r = self.default_box(&cmd.args, sw * 0.6, rows as f64 * 29.2);
        let (ch, id) = self.insert_xml(slide, |id, root| ops::table_xml(id, &ops::fresh_name(root, "Table", id), rows, cols, r))?;
        self.land(cmd, Selection::caret(TextPos { slide, shape: id, cell: Some((0, 0)), at: At::new(0, 0) }.pos()));
        Ok(ch)
    }

    /// The table and cell a table command means.
    fn table_at(&self, cmd: &Command) -> Result<(usize, u32, Option<(usize, usize)>), BusError> {
        let a = &cmd.args;
        if let (Some(_), Some(_)) = (a.get("shape"), a.get("slide")) {
            let (slide, ids) = self.shapes_of(&cmd.name, a)?;
            let cell = a.get("cell").and_then(Json::as_arr).and_then(|c| Some((c.first()?.as_usize()?, c.get(1)?.as_usize()?)));
            return Ok((slide, ids[0], cell));
        }
        match self.place(&cmd.name, a)? {
            Place::Text(_, f) if f.cell.is_some() => Ok((f.slide, f.shape, f.cell)),
            Place::Shapes(sl, ids) if ids.len() == 1 => Ok((sl, ids[0], None)),
            _ => Err(failed("put the caret in a table cell, or select a table")),
        }
    }

    fn cmd_table_change(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let (slide, id, cell) = self.table_at(cmd)?;
        let insert = cmd.name == "stage.table_insert";
        let what = s(&cmd.args, "what").unwrap_or("row").to_string();
        let after = flag(&cmd.args, "after").unwrap_or(true);
        let dims = {
            let root = self.deck.sheet(&self.deck.slides[slide].part).ok_or_else(|| failed("the slide cannot be read"))?;
            ops::table_dims(find_shape(root, &ShapeKey::Id(id)).and_then(|p| el_at(root, &p)).ok_or_else(|| failed("the table is gone"))?)
        };
        let row_mode = what == "row";
        let index = match (cell, row_mode) {
            (Some((r, _)), true) => r,
            (Some((_, c)), false) => c,
            (None, true) => if after { dims.0.saturating_sub(1) } else { 0 },
            (None, false) => if after { dims.1.saturating_sub(1) } else { 0 },
        };
        let index = cmd.args.get("index").and_then(Json::as_usize).unwrap_or(index);
        let ch = self.on_slide(slide, |deck| ops::with_slide(deck, slide, |root| ops::table_change(ops::shape_mut(root, id)?, &what, index, insert, after)))?;
        let land = match cell {
            Some((r, c)) => {
                let (r, c) = match (insert, row_mode, after) {
                    (true, true, true) => (r + 1, c),
                    (true, false, true) => (r, c + 1),
                    (false, true, _) => (r.min(dims.0.saturating_sub(2)), c),
                    (false, false, _) => (r, c.min(dims.1.saturating_sub(2))),
                    _ => (r, c),
                };
                Selection::caret(TextPos { slide, shape: id, cell: Some((r, c)), at: At::new(0, 0) }.pos())
            }
            None => shapes_sel(slide, &[id]),
        };
        self.land(cmd, land);
        Ok(ch)
    }

    fn cmd_table_size(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let (slide, id, cell) = self.table_at(cmd)?;
        let a = &cmd.args;
        let col = num(a, "width").map(|w| (a.get("col").and_then(Json::as_usize).or(cell.map(|c| c.1)).unwrap_or(0), w));
        let row = num(a, "height").map(|h| (a.get("row").and_then(Json::as_usize).or(cell.map(|c| c.0)).unwrap_or(0), h));
        if col.is_none() && row.is_none() {
            return Err(bad(&cmd.name, "\"width\" (with \"col\") and/or \"height\" (with \"row\") in points"));
        }
        let ch = self.on_slide(slide, |deck| ops::with_slide(deck, slide, |root| ops::table_size(ops::shape_mut(root, id)?, col, row)))?;
        if let Some(sel) = a.get("sel").and_then(Selection::from_json) {
            self.land(cmd, sel);
        }
        Ok(ch)
    }

    fn cmd_cell_fill(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let (slide, id, cell) = self.table_at(cmd)?;
        let color = cmd.args.get("color").cloned().unwrap_or(Json::Null);
        let hex = color.as_str().and_then(super::text::hex_arg);
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                let ns = Ns::of(root);
                let frame = ops::shape_mut(root, id)?;
                let (rows, cols) = ops::table_dims(frame);
                let cells: Vec<(usize, usize)> = match cell {
                    Some(c) => vec![c],
                    None => (0..rows).flat_map(|r| (0..cols).map(move |c| (r, c))).collect(),
                };
                for (r, c) in cells {
                    if let Some(tc) = ops::cell_mut(frame, r, c) {
                        ops::set_cell_fill(tc, hex.as_deref(), &ns);
                    }
                }
                Ok(())
            })
        })?;
        if let Some(sel) = cmd.args.get("sel").and_then(Selection::from_json) {
            self.land(cmd, sel);
        }
        Ok(ch)
    }

    fn cmd_group(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let (slide, ids) = self.shapes_of(&cmd.name, &cmd.args)?;
        let boxes: Vec<(u32, Xfrm)> = ids.iter().filter_map(|i| self.resolved_xfrm(slide, *i).map(|x| (*i, x))).collect();
        let mut gid = 0;
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                gid = ops::group(root, &ids, &boxes, None, None)?;
                Ok(())
            })
        })?;
        self.land(cmd, shapes_sel(slide, &[gid]));
        Ok(ch)
    }

    fn cmd_ungroup(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let (slide, ids) = self.shapes_of(&cmd.name, &cmd.args)?;
        let mut members = Vec::new();
        let mut info = Vec::new();
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                for id in &ids {
                    let (name, kids) = ops::ungroup(root, *id)?;
                    members.extend(kids.iter().copied());
                    info.push((*id, name, kids));
                }
                Ok(())
            })
        })?;
        for (id, name, kids) in info {
            self.ungrouped.retain(|u| !(u.0 == slide && u.1 == id));
            self.ungrouped.push((slide, id, name, kids));
        }
        self.land(cmd, shapes_sel(slide, &members));
        Ok(ch)
    }

    fn cmd_regroup(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let (slide, ids) = self.shapes_of(&cmd.name, &cmd.args)?;
        let found = self.ungrouped.iter().rev().find(|u| u.0 == slide && u.3.iter().any(|m| ids.contains(m))).cloned().ok_or_else(|| failed("those shapes were not ungrouped here; select shapes and group them instead"))?;
        let present: Vec<u32> = found.3.iter().copied().filter(|m| self.geom(slide, *m).is_some()).collect();
        let boxes: Vec<(u32, Xfrm)> = present.iter().filter_map(|i| self.resolved_xfrm(slide, *i).map(|x| (*i, x))).collect();
        let (gid, gname) = (found.1, found.2.clone());
        let free = self.geom(slide, gid).is_none();
        let mut made = 0;
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                made = ops::group(root, &present, &boxes, Some(&gname), if free { Some(gid) } else { None })?;
                Ok(())
            })
        })?;
        self.ungrouped.retain(|u| !(u.0 == slide && u.1 == gid));
        self.land(cmd, shapes_sel(slide, &[made]));
        Ok(ch)
    }

    fn cmd_align_shapes(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let (slide, ids) = self.shapes_of(&cmd.name, &cmd.args)?;
        let how = s(&cmd.args, "how").unwrap_or("left").to_string();
        let boxes: Vec<(u32, Rect)> = ids.iter().filter_map(|i| self.shape_box(slide, *i).map(|b| (*i, b))).collect();
        if boxes.is_empty() {
            return Err(failed("nothing to align"));
        }
        let (sw, sh) = self.slide_size_pt();
        let to_slide = flag(&cmd.args, "to_slide").unwrap_or(boxes.len() == 1);
        let bb = if to_slide { Rect::new(0.0, 0.0, sw, sh) } else { boxes.iter().skip(1).fold(boxes[0].1, |a, b| a.union(&b.1)) };
        let mut moves: Vec<(u32, f64, f64)> = Vec::new();
        match how.as_str() {
            "distribute_h" | "distribute_v" => {
                let h = how == "distribute_h";
                let mut v = boxes.clone();
                v.sort_by(|a, b| if h { a.1.x.total_cmp(&b.1.x) } else { a.1.y.total_cmp(&b.1.y) });
                if v.len() >= 2 {
                    let total: f64 = v.iter().map(|b| if h { b.1.w } else { b.1.h }).sum();
                    let span = if h { bb.w } else { bb.h };
                    let gap = (span - total) / (v.len() - 1) as f64;
                    let mut at = if h { bb.x } else { bb.y };
                    for (id, b) in &v {
                        let d = at - if h { b.x } else { b.y };
                        moves.push(if h { (*id, d, 0.0) } else { (*id, 0.0, d) });
                        at += (if h { b.w } else { b.h }) + gap;
                    }
                }
            }
            _ => {
                for (id, b) in &boxes {
                    let (dx, dy) = match how.as_str() {
                        "left" => (bb.x - b.x, 0.0),
                        "center" => (bb.x + bb.w / 2.0 - (b.x + b.w / 2.0), 0.0),
                        "right" => (bb.right() - b.right(), 0.0),
                        "top" => (0.0, bb.y - b.y),
                        "middle" => (0.0, bb.y + bb.h / 2.0 - (b.y + b.h / 2.0)),
                        "bottom" => (0.0, bb.bottom() - b.bottom()),
                        other => return Err(bad(&cmd.name, &format!("\"how\" is left, center, right, top, middle, bottom, distribute_h or distribute_v, not {other}"))),
                    };
                    moves.push((*id, dx, dy));
                }
            }
        }
        let geo: Vec<(u32, f64, f64, Option<Xfrm>, Option<super::render::ShapeGeom>)> =
            moves.iter().map(|(i, dx, dy)| (*i, *dx, *dy, self.resolved_xfrm(slide, *i), self.geom(slide, *i).cloned())).collect();
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                let ns = Ns::of(root);
                for (id, dx, dy, x, g) in &geo {
                    if dx.abs() < 1e-6 && dy.abs() < 1e-6 {
                        continue;
                    }
                    let Some(mut x) = *x else { continue };
                    let parent = g.as_ref().map(|g| ops::parent_matrix(&g.m, &x)).unwrap_or_default();
                    ops::move_by(&mut x, *dx, *dy, &parent);
                    ops::set_xfrm(ops::shape_mut(root, *id)?, &x, &ns)?;
                }
                Ok(())
            })
        })?;
        self.land(cmd, shapes_sel(slide, &ids));
        Ok(ch)
    }

    fn cmd_duplicate(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let (slide, ids) = self.shapes_of(&cmd.name, &cmd.args)?;
        let xs: Vec<(u32, Option<Xfrm>)> = ids.iter().map(|i| (*i, self.resolved_xfrm(slide, *i))).collect();
        let mut new_ids = Vec::new();
        let ch = self.on_slide(slide, |deck| {
            ops::with_slide(deck, slide, |root| {
                let ns = Ns::of(root);
                for (id, x) in &xs {
                    let path = ops::shape_path(root, *id)?;
                    let mut copy = el_at(root, &path).cloned().ok_or_else(|| failed("shape not found"))?;
                    let mut next = ops::next_id(root);
                    new_ids.push(next);
                    ops::renumber(&mut copy, &mut next);
                    if let Some(mut x) = *x {
                        x.off.0 += emu(18.0);
                        x.off.1 += emu(18.0);
                        ops::set_xfrm(&mut copy, &x, &ns)?;
                    }
                    ops::push_shape(root, copy)?;
                }
                Ok(())
            })
        })?;
        self.land(cmd, shapes_sel(slide, &new_ids));
        Ok(ch)
    }

    fn cmd_new_slide(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let a = &cmd.args;
        let after = match a.get("after").and_then(Json::as_usize) {
            Some(i) => i.min(self.deck.slides.len().saturating_sub(1)),
            None => self.slide_of(&cmd.name, a).unwrap_or(self.deck.slides.len().saturating_sub(1)),
        };
        let current = self.deck.slides.get(after).and_then(|s| s.layout.clone());
        let layout = match a.get("layout") {
            Some(l) if !l.is_null() => ops::find_layout(&self.deck, l, None)?,
            // After a title slide PowerPoint offers its "Title and Content"; else the same layout.
            _ => {
                let all = ops::layouts(&self.deck);
                let is_title = current.as_ref().and_then(|c| all.iter().find(|l| name_key(&l.0) == name_key(c))).is_some_and(|l| l.1.eq_ignore_ascii_case("Title Slide"));
                let content = all.iter().find(|l| l.1.eq_ignore_ascii_case("Title and Content")).map(|l| l.0.clone());
                match (is_title, content) {
                    (true, Some(c)) => c,
                    _ => ops::find_layout(&self.deck, &Json::Null, current.as_deref())?,
                }
            }
        };
        let tree = ops::placeholders_of(&self.deck, &layout);
        let title = s(a, "title").map(str::to_string);
        let at = after + 1;
        let ch = self.on_package(|deck| {
            let name = (1..).map(|n| format!("/ppt/slides/slide{n}.xml")).find(|n| deck.pkg.part(n).is_none()).unwrap_or_default();
            let rels = vec![("rId1".to_string(), opc::REL_SLIDE_LAYOUT.to_string(), opc::relative_target(&name, &layout), false)];
            ops::insert_slide_part(deck, at, ops::slide_xml(&tree).into_bytes(), rels)?;
            Ok(())
        })?;
        if let Some(t) = title {
            if let Some(tid) = self.deck.sheet(&self.deck.slides[at].part).and_then(sp_tree).and_then(|tr| super::model::all_shapes(tr).into_iter().find(|s| Ph::of(s).is_some_and(|p| matches!(p.ty.as_str(), "title" | "ctrTitle")))).and_then(shape_id) {
                let _ = ops::edit_body(&mut self.deck, at, tid, None, |b| {
                    let e = b.end();
                    let st = b.delete(At::new(0, 0), e);
                    b.insert(st, &t, None);
                });
                self.render_page(at);
            }
        }
        let undo = Snap::default();
        let _ = undo;
        self.land(cmd, slide_sel(at));
        Ok(ch)
    }

    fn cmd_paste(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let a = &cmd.args;
        if let Some(img) = a.get("image") {
            let mut c = cmd.clone();
            c.name = "stage.insert_picture".into();
            c.args.set("data", img.get("data").cloned().unwrap_or_else(|| img.clone()));
            c.args.set("name", img.get("name").cloned().unwrap_or(Json::from("pasted")));
            let r = self.cmd_insert_picture(&c);
            if let Some((_, s)) = self.landed.clone() {
                self.land(cmd, s);
            }
            return r;
        }
        let place = self.place(&cmd.name, a)?;
        let rich = a.get("rich").filter(|r| !r.is_null()).cloned();
        let kind = rich.as_ref().and_then(|r| r.get("kind")).and_then(Json::as_str).unwrap_or("").to_string();
        match (kind.as_str(), &place) {
            ("stage.text", Place::Text(ta, tf)) => {
                let xml = rich.as_ref().and_then(|r| r.get("xml")).and_then(Json::as_str).unwrap_or("");
                let tx = xml::parse_dom(xml.as_bytes()).map_err(|e| failed(format!("the copied text is not readable: {e}")))?;
                let other = Body::read(&tx);
                let (s0, e0) = (ta.at, tf.at);
                let (ch, at) = self.text_edit(tf, |b| {
                    let st = b.delete(s0, e0);
                    b.insert_body(st, &other)
                })?;
                self.land(cmd, Selection::caret(TextPos { at, ..*tf }.pos()));
                Ok(ch)
            }
            ("stage.shapes", _) => {
                let slide = match place {
                    Place::Text(_, f) => f.slide,
                    Place::Shapes(s, _) | Place::Slide(s) | Place::Blank(s) => s,
                };
                let from = rich.as_ref().and_then(|r| r.get("slide")).and_then(Json::as_usize);
                let shapes = rich.as_ref().and_then(|r| r.get("shapes")).and_then(Json::as_arr).unwrap_or(&[]).to_vec();
                let (ch, ids) = self.paste_shapes(slide, &shapes, from)?;
                self.land(cmd, shapes_sel(slide, &ids));
                Ok(ch)
            }
            ("stage.slides", _) => {
                let after = match place {
                    Place::Text(_, f) => f.slide,
                    Place::Shapes(s, _) | Place::Slide(s) | Place::Blank(s) => s,
                };
                let slides = rich.as_ref().and_then(|r| r.get("slides")).and_then(Json::as_arr).unwrap_or(&[]).to_vec();
                let (ch, first) = self.paste_slides(after, &slides)?;
                self.land(cmd, slide_sel(first));
                Ok(ch)
            }
            (_, Place::Text(..)) | (_, Place::Shapes(..)) if matches!(place, Place::Text(..)) || self.single_text_shape(&place) => {
                let mut c = cmd.clone();
                c.name = "stage.insert_text".into();
                let r = self.cmd_insert_text(&c);
                if let Some((_, s)) = self.landed.clone() {
                    self.land(cmd, s);
                }
                r
            }
            _ => {
                // Plain text onto a slide: a new text box with it.
                let slide = match place {
                    Place::Text(_, f) => f.slide,
                    Place::Shapes(s, _) | Place::Slide(s) | Place::Blank(s) => s,
                };
                let mut c = cmd.clone();
                c.name = "stage.insert_text_box".into();
                c.args.set("slide", slide);
                let r = self.cmd_insert_text_box(&c);
                if let Some((_, s)) = self.landed.clone() {
                    self.land(cmd, s);
                }
                r
            }
        }
    }

    /// Every text on the slides in reading order: (slide, shape, cell).
    fn texts(&self) -> Vec<(usize, u32, Option<(usize, usize)>)> {
        let mut out = Vec::new();
        for (i, geo) in self.geo.iter().enumerate() {
            for g in geo {
                if g.text.is_some() {
                    out.push((i, g.id, None));
                }
                for c in &g.cells {
                    out.push((i, g.id, Some((c.0, c.1))));
                }
            }
        }
        out
    }

    /// Every match of `query` (case-insensitive) in reading order.
    fn matches(&self, query: &str) -> Vec<(TextPos, TextPos)> {
        let q: Vec<char> = query.to_lowercase().chars().collect();
        let mut out = Vec::new();
        if q.is_empty() {
            return out;
        }
        for (slide, shape, cell) in self.texts() {
            let Some(body) = ops::read_body(&self.deck, slide, shape, cell) else { continue };
            for p in 0..body.paras.len() {
                let chars: Vec<char> = body.chars(p).iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
                let mut i = 0;
                while i + q.len() <= chars.len() {
                    if chars[i..i + q.len()] == q[..] {
                        let a = TextPos { slide, shape, cell, at: At::new(p, i) };
                        out.push((a, TextPos { at: At::new(p, i + q.len()), ..a }));
                        i += q.len();
                    } else {
                        i += 1;
                    }
                }
            }
        }
        out
    }

    /// Where a selection sits in the reading order (for the next match after it).
    fn order_key(&self, t: &TextPos) -> (usize, usize, usize, usize) {
        let k = self.texts().iter().position(|x| x.0 == t.slide && x.1 == t.shape && x.2 == t.cell).unwrap_or(0);
        (t.slide, k, t.at.para, t.at.ch)
    }

    fn found(&self, query: &str, next: bool, sel: Option<&Json>) -> Option<(TextPos, TextPos)> {
        let all = self.matches(query);
        if all.is_empty() {
            return None;
        }
        let here = match sel.and_then(place_of) {
            Some(Place::Text(a, f)) => Some(if next { if a.at > f.at { a } else { f } } else if a.at < f.at { a } else { f }),
            Some(Place::Shapes(sl, _)) | Some(Place::Slide(sl)) | Some(Place::Blank(sl)) => Some(TextPos { slide: sl, shape: 0, cell: None, at: At::new(0, 0) }),
            None => None,
        };
        let Some(h) = here else { return all.first().copied() };
        let hk = if h.shape == 0 { (h.slide, 0, 0, 0) } else { self.order_key(&h) };
        if next {
            all.iter().find(|m| self.order_key(&m.0) >= hk).copied().or_else(|| all.first().copied())
        } else {
            all.iter().rev().find(|m| self.order_key(&m.1) <= hk).copied().or_else(|| all.last().copied())
        }
    }

    fn cmd_find(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let q = s(&cmd.args, "query").unwrap_or("").to_string();
        let next = flag(&cmd.args, "next").unwrap_or(true);
        if let Some((a, f)) = self.found(&q, next, cmd.args.get("sel")) {
            self.land(cmd, Selection::new(a.pos(), f.pos()));
        }
        Ok(Change::default())
    }

    fn cmd_replace(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let q = s(&cmd.args, "query").unwrap_or("").to_string();
        let with = s(&cmd.args, "with").unwrap_or("").to_string();
        if q.is_empty() {
            return Err(bad(&cmd.name, "\"query\" is the text to replace"));
        }
        if flag(&cmd.args, "all") == Some(true) {
            let mut all = self.matches(&q);
            if all.is_empty() {
                return Ok(Change::default());
            }
            all.reverse();
            let snap = Snap::whole(&self.deck.pkg);
            let mut slides: Vec<usize> = Vec::new();
            for (a, f) in all {
                let w = with.clone();
                ops::edit_body(&mut self.deck, a.slide, a.shape, a.cell, |b| {
                    let st = b.delete(a.at, f.at);
                    b.insert(st, &w, None);
                })?;
                if !slides.contains(&a.slide) {
                    slides.push(a.slide);
                }
            }
            for &sl in &slides {
                self.render_page(sl);
            }
            let undo = snap.undo(&self.deck.pkg).map(|u| vec![u]).unwrap_or_default();
            return Ok(Change { undo, pages_touched: slides });
        }
        // One: the selection is a match -> replace it, then go to the next.
        let cur = match cmd.args.get("sel").and_then(place_of) {
            Some(Place::Text(a, f)) if a.at != f.at => self.matches(&q).into_iter().find(|m| (m.0 == a && m.1 == f) || (m.0 == f && m.1 == a)),
            _ => None,
        };
        let Some((a, f)) = cur else {
            if let Some((a, f)) = self.found(&q, true, cmd.args.get("sel")) {
                self.land(cmd, Selection::new(a.pos(), f.pos()));
            }
            return Ok(Change::default());
        };
        let w = with.clone();
        let (ch, at) = self.text_edit(&f, |b| {
            let st = b.delete(a.at, f.at);
            b.insert(st, &w, None)
        })?;
        let after = Selection::caret(TextPos { at, ..f }.pos()).to_json();
        let land = match self.found(&q, true, Some(&after)) {
            Some((x, y)) => Selection::new(x.pos(), y.pos()),
            None => Selection::caret(TextPos { at, ..f }.pos()),
        };
        self.land(cmd, land);
        Ok(ch)
    }

    fn single_text_shape(&self, p: &Place) -> bool {
        matches!(p, Place::Shapes(sl, ids) if ids.len() == 1 && self.geom(*sl, ids[0]).is_some_and(|g| g.text.is_some() && shape_kind_is_texty(g)))
    }
}

fn shape_kind_is_texty(g: &super::render::ShapeGeom) -> bool {
    g.placeholder || g.text_box || g.text.as_ref().is_some_and(|t| t.paras.iter().any(|p| p.model_len > 0))
}

fn el_at_mut_path<'a>(root: &'a mut Element, path: &[usize]) -> Result<&'a mut Element, BusError> {
    super::edit::el_at_mut(root, path).ok_or_else(|| failed("shape not found"))
}

/// The list level of a paragraph moved by `d` (0..=8).
fn set_level(p: &mut Element, lvl: usize, d: i64) {
    let n = (lvl as i64 + d).clamp(0, 8);
    if n == 0 {
        p.attrs.retain(|(k, _)| k != "lvl");
    } else {
        p.set_attr("lvl", n.to_string());
    }
}

/// The hand commands' specs (for the bus, the AI and the ribbon's ready buttons).
pub fn specs() -> Vec<CommandSpec> {
    let sel = r#""sel": { "anchor": pos, "focus": pos }"#;
    let c = |n: &str, s: &str, a: &str| CommandSpec::new(n, s, a.replace("SEL", sel));
    vec![
        c("stage.find", "Find text on the slides (case-insensitive), the next (or previous) match after the selection; changes nothing.", "{ \"query\": string, \"next\"?: bool, SEL? }"),
        c("stage.replace", "Replace the selected match and go to the next, or every match in the deck.", "{ \"query\": string, \"with\": string, \"all\"?: bool, SEL? }"),
        c("stage.insert_text", "Type text at the selection (replacing a selected range; \\n starts a paragraph, \\u000b breaks a line). A selected shape gets the text as its whole text.", "{ SEL, \"text\": string }"),
        c("stage.delete", "Delete the selected text (or one character / word before or after the caret), or the selected shapes, or the selected slide.", "{ SEL, \"back\": bool, \"word\"?: bool }"),
        c("stage.enter", "A new paragraph at the caret (shift: a line break in the paragraph).", "{ SEL, \"shift\"?: bool }"),
        c("stage.tab", "Tab: the next table cell, a list level deeper (back: shallower), or a tab character.", "{ SEL, \"back\"?: bool }"),
        c("stage.select_all", "Select all the text of the shape, else every shape on the slide, else the slide.", "{ SEL }"),
        c("stage.select_slide", "Select the slide as a whole (for a copy into another deck).", "{ SEL } | { \"slide\": number }"),
        c("stage.arrow", "An arrow key: the caret moves (extend grows the selection), selected shapes nudge 6 pt (fine: 1 pt).", "{ SEL, \"dir\": \"left\"|\"right\"|\"up\"|\"down\", \"extend\"?: bool, \"fine\"?: bool }"),
        c("stage.paste", "Paste: text at the caret, copied text with its formatting, copied shapes or slides (from any open deck), or a picture.", "{ SEL, \"text\"?: string, \"rich\"?: {..}, \"image\"?: { \"data\": base64, \"name\" } }"),
        c("stage.format", "Run formatting of the selected text (or of whole shapes): bold, italic, underline (true or single/double/heavy/dotted/dash/wavy/none), strike, font, size (pt), color (RRGGBB or null).", "{ SEL, \"bold\"?, \"italic\"?, \"underline\"?, \"strike\"?, \"font\"?, \"size\"?, \"color\"? }"),
        c("stage.bold", "Bold on or off (toggles without \"on\").", "{ SEL, \"on\"?: bool }"),
        c("stage.italic", "Italic on or off (toggles without \"on\").", "{ SEL, \"on\"?: bool }"),
        c("stage.underline", "Underline on or off (toggles without \"on\").", "{ SEL, \"on\"?: bool }"),
        c("stage.strike", "Strikethrough on or off (toggles without \"on\").", "{ SEL, \"on\"?: bool }"),
        c("stage.grow_font", "The next bigger font size.", "{ SEL }"),
        c("stage.shrink_font", "The next smaller font size.", "{ SEL }"),
        c("stage.align", "Align the selected paragraphs.", "{ SEL, \"align\": \"left\"|\"center\"|\"right\"|\"justify\" }"),
        c("stage.line_spacing", "Line spacing of the selected paragraphs, as a multiple (1, 1.15, 1.5, 2).", "{ SEL, \"value\": number }"),
        c("stage.bullets", "Bullets on the selected paragraphs (toggles without \"on\").", "{ SEL, \"on\"?: bool, \"char\"?: string }"),
        c("stage.numbering", "Numbering on the selected paragraphs (toggles without \"on\").", "{ SEL, \"on\"?: bool, \"scheme\"?: \"arabicPeriod\"|\"romanUcPeriod\"|\"alphaLcParenR\"|.. }"),
        c("stage.list_level", "Move the selected paragraphs a list level deeper (+1) or shallower (-1).", "{ SEL, \"delta\": 1|-1 }"),
        c("stage.autofit", "How text fits its shape: shrink text on overflow, resize the shape to fit, or none.", "{ SEL | \"slide\"+\"shape\", \"mode\": \"shrink\"|\"resize\"|\"none\" }"),
        c("stage.wrap", "Wrap text in the shape on or off.", "{ SEL | \"slide\"+\"shape\", \"on\": bool }"),
        c("stage.insert_text_box", "A new text box (centred unless placed), the caret in it.", "{ \"slide\"? | SEL, \"x\"?, \"y\"?, \"w\"?, \"h\"? (pt), \"text\"?: string }"),
        c("stage.insert_shape", "A new preset shape (rect, roundRect, ellipse, triangle, rightArrow, star5, ... any of the preset table).", "{ \"slide\"? | SEL, \"preset\": string, \"x\"?, \"y\"?, \"w\"?, \"h\"?, \"text\"? }"),
        c("stage.insert_line", "A straight line or arrow from (x1, y1) to (x2, y2) with optional arrowheads (triangle, arrow, stealth, oval, diamond).", "{ \"slide\"? | SEL, \"x1\", \"y1\", \"x2\", \"y2\", \"head\"?, \"tail\"? }"),
        c("stage.insert_picture", "A picture from its bytes (PNG, JPEG, GIF, BMP, SVG); into the selected empty picture or content placeholder, cropped to fill.", "{ \"slide\"? | SEL, \"data\": base64, \"name\"?, \"x\"?, \"y\"?, \"w\"?, \"h\"? }"),
        c("stage.insert_table", "A new table of rows x cols, the caret in its first cell.", "{ \"slide\"? | SEL, \"rows\", \"cols\", \"x\"?, \"y\"?, \"w\"?, \"h\"? }"),
        c("stage.table_insert", "Insert a row (below/above) or column (right/left) at the caret's cell.", "{ SEL | \"slide\"+\"shape\"+\"cell\"?, \"what\": \"row\"|\"col\", \"after\"?: bool }"),
        c("stage.table_delete", "Delete the caret's row or column.", "{ SEL | \"slide\"+\"shape\"+\"cell\"?, \"what\": \"row\"|\"col\" }"),
        c("stage.table_size", "A column's width and/or a row's height (pt).", "{ SEL | \"slide\"+\"shape\", \"col\"?, \"width\"?, \"row\"?, \"height\"? }"),
        c("stage.cell_fill", "Shade the caret's cell (or every cell of a selected table); null for no fill.", "{ SEL | \"slide\"+\"shape\"+\"cell\"?, \"color\": \"RRGGBB\"|null }"),
        c("stage.fill", "Solid fill of the selected shapes (null or \"none\": no fill), optional transparency as alpha 0..1.", "{ SEL | \"slide\"+\"shapes\", \"color\": \"RRGGBB\"|null, \"alpha\"? }"),
        c("stage.outline", "Outline of the selected shapes: colour (null: no line), width (pt), dash, arrowheads.", "{ SEL | \"slide\"+\"shapes\", \"color\"?, \"width\"?, \"dash\"?, \"head\"?, \"tail\"? }"),
        c("stage.crop", "Crop the selected picture: fractions trimmed from each edge.", "{ SEL | \"slide\"+\"shape\", \"l\"?, \"t\"?, \"r\"?, \"b\"? }"),
        c("stage.order", "Bring to front, send to back, bring forward or send backward.", "{ SEL | \"slide\"+\"shapes\", \"to\": \"front\"|\"back\"|\"forward\"|\"backward\" }"),
        c("stage.group", "Group the selected shapes.", "{ SEL | \"slide\"+\"shapes\" }"),
        c("stage.ungroup", "Ungroup the selected group.", "{ SEL | \"slide\"+\"shape\" }"),
        c("stage.regroup", "Group again the shapes last ungrouped together.", "{ SEL | \"slide\"+\"shapes\" }"),
        c("stage.align_shapes", "Align the selected shapes to each other (or to the slide for one shape, or with to_slide), or distribute them evenly.", "{ SEL | \"slide\"+\"shapes\", \"how\": \"left\"|\"center\"|\"right\"|\"top\"|\"middle\"|\"bottom\"|\"distribute_h\"|\"distribute_v\", \"to_slide\"?: bool }"),
        c("stage.rotate", "Rotate the selected shapes by or to an angle (degrees, clockwise).", "{ SEL | \"slide\"+\"shapes\", \"by\"? | \"to\"? }"),
        c("stage.flip", "Flip the selected shapes horizontally or vertically.", "{ SEL | \"slide\"+\"shapes\", \"axis\": \"h\"|\"v\" }"),
        c("stage.size", "Exact size of the selected shapes (pt), optionally keeping the aspect ratio; or scaled about the centre.", "{ SEL | \"slide\"+\"shapes\", \"w\"?, \"h\"?, \"lock\"?: bool, \"scale\"?: number }"),
        c("stage.position", "Exact position of the selected shapes' top-left corner on the slide (pt).", "{ SEL | \"slide\"+\"shapes\", \"x\"?, \"y\"? }"),
        c("stage.nudge", "Move the selected shapes by (dx, dy) points.", "{ SEL | \"slide\"+\"shapes\", \"dx\", \"dy\" }"),
        c("stage.duplicate", "Duplicate the selected shapes (a little down and right).", "{ SEL | \"slide\"+\"shapes\" }"),
        c("stage.delete_shapes", "Delete the selected shapes.", "{ SEL | \"slide\"+\"shapes\" }"),
        c("stage.new_slide", "A new slide after the given (or selected) one, on a layout by name or index (default: the same, or Title and Content after a title slide).", "{ \"after\"?: number, \"layout\"?: string|number, \"title\"?: string }"),
        c("stage.duplicate_slide", "Duplicate a slide (and its notes) right after itself.", "{ \"slide\"? | SEL }"),
        c("stage.delete_slide", "Delete a slide (a deck keeps at least one).", "{ \"slide\"? | SEL }"),
        c("stage.move_slide", "Move a slide to a new place in the deck (or by one place earlier or later).", "{ \"from\"? | SEL, \"to\": number | \"by\": -1|1 }"),
        c("stage.set_layout", "Put a slide on another layout (placeholders follow; the ones the layout lacks keep their place).", "{ \"slide\"? | SEL, \"layout\": string|number }"),
    ]
}

/// Keeps `Rect` used when building without tests.
#[allow(dead_code)]
fn _unused(_: Rect, _: Box<dyn Editing>) {}
