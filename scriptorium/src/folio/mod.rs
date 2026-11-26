//! Folio, the PDF editor. M1: open and see.
//!
//! [`FolioDoc::open`] parses a PDF (classic and stream cross-references, object streams, incremental
//! updates, repair of broken files; encrypted files are refused clearly), walks its page tree, and
//! draws every page into a `display::DisplayList` with glyph outlines and image pixels in
//! `display::Resources`. Text comes back in reading order for `read` and search. Edits (the info
//! title, sticky notes) live in an overlay and [`FolioDoc::save`] appends them as an incremental
//! update, so the original bytes stay an exact prefix of the saved file.
//!
//! Modules: `lexer` and `obj` (tokens and objects), `parse` (the file), `filters` (stream decoding),
//! `doc` (pages, outlines, annotations), `fonts` / `cff` / `cmap` / `encoding` / `afm` (fonts),
//! `colour` and `image` (paint), `render` (content streams), `text` (reading order), `edit` (saving).

mod afm;
mod cff;
mod cmap;
mod colour;
mod doc;
mod edit;
mod encoding;
mod filters;
mod fonts;
mod image;
mod lexer;
mod obj;
mod parse;
mod render;
mod text;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

pub use doc::{AnnotInfo, OutlineEntry, PageInfo};
pub use obj::{decode_text, Dict, FolioError, Obj, Stream};
pub use parse::PdfFile;
pub use render::TextGlyph;

use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command, CommandSpec, Document};
use crate::ground::display::{DisplayList, Item, Matrix, Rect, Resources};
use render::{PageSource, RenderCache};

/// The size of a sticky note's icon, in points.
const NOTE_SIZE: f64 = 20.0;

pub struct FolioDoc {
    file: PdfFile,
    pages: Vec<PageInfo>,
    outlines: Vec<OutlineEntry>,
    lists: Vec<DisplayList>,
    texts: Vec<Vec<TextGlyph>>,
    image_counts: Vec<usize>,
    resources: Resources,
    cache: RenderCache,
    /// Objects the edits created (removed again when an edit is undone).
    created: BTreeSet<u32>,
}

impl std::fmt::Debug for FolioDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FolioDoc").field("pages", &self.pages.len()).field("file", &self.file).finish()
    }
}

impl FolioDoc {
    /// Open a PDF from its bytes.
    pub fn open(bytes: impl Into<Vec<u8>>) -> Result<FolioDoc, FolioError> {
        let file = PdfFile::open(bytes.into())?;
        let mut d = FolioDoc {
            file,
            pages: Vec::new(),
            outlines: Vec::new(),
            lists: Vec::new(),
            texts: Vec::new(),
            image_counts: Vec::new(),
            resources: Resources::new(),
            cache: RenderCache::new(),
            created: BTreeSet::new(),
        };
        d.reload_structure();
        d.render_all();
        Ok(d)
    }

    /// The bytes to write: the original file exactly when nothing changed, else the original with an
    /// incremental update appended.
    pub fn save(&self) -> Vec<u8> {
        edit::incremental_update(&self.file)
    }

    /// True when there are edits to save.
    pub fn is_modified(&self) -> bool {
        !self.file.overlay.is_empty() || !self.file.trailer_edits.0.is_empty()
    }

    /// The bytes as opened.
    pub fn original(&self) -> &[u8] {
        &self.file.data
    }

    /// True when the cross-reference was broken and rebuilt by scanning.
    pub fn was_repaired(&self) -> bool {
        self.file.repaired
    }

    pub fn version(&self) -> &str {
        &self.file.version
    }

    pub fn page_info(&self, page: usize) -> Option<&PageInfo> {
        self.pages.get(page)
    }

    /// The displayed page size in points (rotation applied).
    pub fn page_size(&self, page: usize) -> Option<(f64, f64)> {
        self.pages.get(page).map(PageInfo::size)
    }

    pub fn title(&self) -> Option<String> {
        doc::info_text(&self.file, "Title")
    }

    pub fn author(&self) -> Option<String> {
        doc::info_text(&self.file, "Author")
    }

    pub fn outlines(&self) -> &[OutlineEntry] {
        &self.outlines
    }

    pub fn annotations(&self, page: usize) -> Vec<AnnotInfo> {
        match self.pages.get(page) {
            Some(p) => doc::page_annots(&self.file, p).iter().map(|(n, d)| doc::annot_info(&self.file, *n, d)).collect(),
            None => Vec::new(),
        }
    }

    /// The positioned characters of a page (display coordinates).
    pub fn page_glyphs(&self, page: usize) -> &[TextGlyph] {
        self.texts.get(page).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// A page's text in reading order.
    pub fn page_text(&self, page: usize) -> String {
        text::reading_order(self.page_glyphs(page))
    }

    /// Every page containing `query` (case-insensitive), with a little context around each match.
    pub fn search(&self, query: &str) -> Vec<(usize, String)> {
        let q = query.trim().to_lowercase();
        let mut out = Vec::new();
        if q.is_empty() {
            return out;
        }
        for p in 0..self.pages.len() {
            let t = self.page_text(p).replace('\n', " ");
            let lower = t.to_lowercase();
            let mut from = 0;
            while let Some(i) = lower[from..].find(&q) {
                let at = from + i;
                let chars: Vec<(usize, char)> = t.char_indices().collect();
                let ci = chars.iter().position(|(b, _)| *b >= at).unwrap_or(0);
                let s = chars.get(ci.saturating_sub(30)).map_or(0, |c| c.0);
                let e = chars.get(ci + q.chars().count() + 30).map_or(t.len(), |c| c.0);
                out.push((p, t.get(s..e).unwrap_or("").trim().to_string()));
                from = at + q.len().max(1);
                if out.len() > 1000 || from >= lower.len() {
                    break;
                }
            }
            if out.len() > 1000 {
                break;
            }
        }
        out
    }

    /// A page drawn `width` points wide (the page's display list scaled to fit).
    pub fn thumbnail(&self, page: usize, width: f64) -> DisplayList {
        let src = self.display(page);
        let k = if src.width > 0.0 && width > 0.0 { width / src.width } else { 1.0 };
        let mut out = DisplayList::new(src.width * k, src.height * k);
        out.items.reserve(src.items.len() + 2);
        out.push(Item::PushTransform(Matrix::scale(k, k)));
        out.items.extend(src.items);
        out.push(Item::PopTransform);
        out
    }

    // -----------------------------------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------------------------------

    fn page_map(&self) -> HashMap<u32, usize> {
        self.pages.iter().enumerate().filter(|(_, p)| p.num != 0).map(|(i, p)| (p.num, i)).collect()
    }

    fn reload_structure(&mut self) {
        self.pages = doc::collect_pages(&self.file);
        let map = self.page_map();
        self.outlines = doc::collect_outlines(&self.file, &map);
    }

    fn render_all(&mut self) {
        self.lists.clear();
        self.texts.clear();
        self.image_counts.clear();
        for i in 0..self.pages.len() {
            let (l, t, n) = self.render_one(i);
            self.lists.push(l);
            self.texts.push(t);
            self.image_counts.push(n);
        }
    }

    fn rerender(&mut self, page: usize) {
        if self.lists.len() != self.pages.len() {
            self.render_all();
            return;
        }
        if page < self.pages.len() {
            let (l, t, n) = self.render_one(page);
            self.lists[page] = l;
            self.texts[page] = t;
            self.image_counts[page] = n;
        }
    }

    fn render_one(&mut self, i: usize) -> (DisplayList, Vec<TextGlyph>, usize) {
        let p = self.pages[i].clone();
        let map = self.page_map();
        let mut annots = Vec::new();
        let mut links = Vec::new();
        for (_, a) in doc::page_annots(&self.file, &p) {
            if a.name("Subtype") == Some("Link") {
                if let Some(t) = doc::link_target(&self.file, &a, &map) {
                    let r = self.file.dget(&a, "Rect").nums();
                    if r.len() >= 4 {
                        links.push(([r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])], t));
                    }
                }
            }
            annots.push(a);
        }
        let (w, h) = p.size();
        let src = PageSource {
            width: w,
            height: h,
            base: p.base_matrix(),
            resources: &p.resources,
            contents: p.dict.get("Contents").cloned().unwrap_or(Obj::Null),
            annots,
            links,
        };
        let r = render::render_page(&self.file, &src, &mut self.cache, &mut self.resources);
        (r.list, r.text, r.image_count)
    }

    // -----------------------------------------------------------------------------------------------
    // Edits
    // -----------------------------------------------------------------------------------------------

    /// Put an object in the overlay, or drop it from there when it equals the file's own again.
    fn set_object(&mut self, num: u32, obj: Obj) {
        if !self.created.contains(&num) && self.file.base_object(num) == obj {
            self.file.overlay.remove(&num);
        } else {
            self.file.overlay.insert(num, obj);
        }
    }

    fn drop_created(&mut self, num: u32) {
        if self.created.remove(&num) {
            self.file.overlay.remove(&num);
        }
    }

    fn new_object(&mut self, obj: Obj) -> u32 {
        let n = self.file.size();
        self.created.insert(n);
        self.file.overlay.insert(n, obj);
        n
    }

    /// Set (or with `None`, remove) the info dictionary's /Title. Returns the old title.
    pub fn set_title(&mut self, title: Option<&str>) -> Option<String> {
        let old = self.title();
        let t = self.file.trailer();
        let value = title.map(|s| Obj::Str(obj::encode_text(s)));
        match t.get("Info").cloned() {
            Some(Obj::Ref(n, _)) => {
                let mut d = self.file.get(n).as_dict().cloned().unwrap_or_default();
                match value {
                    Some(v) => d.set("Title", v),
                    None => {
                        d.remove("Title");
                    }
                }
                if self.created.contains(&n) && d.0.is_empty() {
                    self.drop_created(n);
                    self.file.trailer_edits.remove("Info");
                } else {
                    self.set_object(n, Obj::Dict(d));
                }
            }
            Some(Obj::Dict(mut d)) => {
                match value {
                    Some(v) => d.set("Title", v),
                    None => {
                        d.remove("Title");
                    }
                }
                if self.file.base_trailer().get("Info") == Some(&Obj::Dict(d.clone())) {
                    self.file.trailer_edits.remove("Info");
                } else {
                    self.file.trailer_edits.set("Info", Obj::Dict(d));
                }
            }
            _ => {
                if let Some(v) = value {
                    let n = self.new_object(Obj::Dict(Dict::new().with("Title", v)));
                    self.file.trailer_edits.set("Info", Obj::Ref(n, 0));
                }
            }
        }
        old
    }

    /// The page's /Annots as references, and how to write them back (into the page or its array object).
    fn annots_of(&self, page: usize) -> Result<(Vec<Obj>, Option<u32>), String> {
        let p = self.pages.get(page).ok_or_else(|| format!("no page {} (the document has {} pages)", page, self.pages.len()))?;
        if p.num == 0 {
            return Err("this page cannot be edited (its dictionary is not an object of its own)".into());
        }
        let page_obj = self.file.get(p.num);
        let d = page_obj.as_dict().ok_or("the page object is missing")?;
        match d.get("Annots") {
            Some(Obj::Ref(n, _)) => Ok((self.file.get(*n).as_arr().map(|a| a.to_vec()).unwrap_or_default(), Some(*n))),
            Some(Obj::Arr(a)) => Ok((a.clone(), None)),
            _ => Ok((Vec::new(), None)),
        }
    }

    fn write_annots(&mut self, page: usize, list: Vec<Obj>, array_obj: Option<u32>) {
        match array_obj {
            Some(n) => self.set_object(n, Obj::Arr(list)),
            None => {
                let num = self.pages[page].num;
                let mut d = self.file.get(num).as_dict().cloned().unwrap_or_default();
                if list.is_empty() && !self.file.base_object(num).as_dict().map_or(false, |b| b.contains("Annots")) {
                    d.remove("Annots");
                } else {
                    d.set("Annots", Obj::Arr(list));
                }
                self.set_object(num, Obj::Dict(d));
            }
        }
        self.reload_structure();
        self.rerender(page);
    }

    /// Add a sticky note (a Text annotation with its own appearance) whose icon's top-left corner
    /// is at (x, y) in display points. Returns the annotation's object number.
    pub fn add_note(&mut self, page: usize, x: f64, y: f64, text: &str) -> Result<u32, String> {
        let (mut list, array_obj) = self.annots_of(page)?;
        let p = &self.pages[page];
        let inv = p.base_matrix().invert().ok_or("the page has no area")?;
        let r = inv.apply_rect(&Rect::new(x, y, NOTE_SIZE, NOTE_SIZE));
        let page_ref = Obj::Ref(p.num, p.gen);
        let ap = "q 1 0.85 0.2 rg 0.55 0.42 0.05 RG 0.8 w 0.5 0.5 19 19 re B 0.4 0.3 0 RG 0.9 w 4 14.5 m 16 14.5 l 4 10.5 m 16 10.5 l 4 6.5 m 12 6.5 l S Q";
        let ap_dict = Dict::new()
            .with("Type", Obj::name("XObject"))
            .with("Subtype", Obj::name("Form"))
            .with("BBox", Obj::Arr(vec![Obj::Int(0), Obj::Int(0), Obj::Int(20), Obj::Int(20)]));
        let ap_num = self.new_object(Obj::Stream(Arc::new(Stream { dict: ap_dict, data: ap.as_bytes().to_vec() })));
        let n2 = |v: f64| Obj::Real((v * 100.0).round() / 100.0);
        let annot = Dict::new()
            .with("Type", Obj::name("Annot"))
            .with("Subtype", Obj::name("Text"))
            .with("Rect", Obj::Arr(vec![n2(r.x), n2(r.y), n2(r.right()), n2(r.bottom())]))
            .with("Contents", Obj::Str(obj::encode_text(text)))
            .with("Name", Obj::name("Note"))
            .with("F", Obj::Int(4))
            .with("C", Obj::Arr(vec![Obj::Real(1.0), Obj::Real(0.85), Obj::Real(0.2)]))
            .with("P", page_ref)
            .with("AP", Obj::Dict(Dict::new().with("N", Obj::Ref(ap_num, 0))));
        let n = self.new_object(Obj::Dict(annot));
        list.push(Obj::Ref(n, 0));
        self.write_annots(page, list, array_obj);
        Ok(n)
    }

    /// Take annotation `note` off a page. Returns the command that puts it back.
    pub fn remove_note(&mut self, page: usize, note: u32) -> Result<Command, String> {
        let (mut list, array_obj) = self.annots_of(page)?;
        let idx = list.iter().position(|o| o.as_ref().map(|r| r.0) == Some(note)).ok_or_else(|| format!("page {} has no annotation {}", page, note))?;
        list.remove(idx);
        let inverse = if self.created.contains(&note) {
            let d = self.file.get(note).as_dict().cloned().unwrap_or_default();
            let r = self.file.dget(&d, "Rect").nums();
            let base = self.pages[page].base_matrix();
            let dr = if r.len() >= 4 { base.apply_rect(&Rect::from_ltrb(r[0], r[1], r[2], r[3])) } else { Rect::new(0.0, 0.0, 0.0, 0.0) };
            let text = self.file.dget(&d, "Contents").as_text().unwrap_or_default();
            if let Some(Obj::Ref(ap, _)) = self.file.dict_of(&d, "AP").and_then(|a| a.get("N").cloned()) {
                self.drop_created(ap);
            }
            self.drop_created(note);
            Command::new("folio.add_note", Json::obj().with("page", page).with("x", dr.x).with("y", dr.y).with("text", text))
        } else {
            Command::new("folio.attach_note", Json::obj().with("page", page).with("note", note).with("index", idx))
        };
        self.write_annots(page, list, array_obj);
        Ok(inverse)
    }

    /// Put an existing annotation back on a page at `index` (the inverse of removing a file's own note).
    pub fn attach_note(&mut self, page: usize, note: u32, index: usize) -> Result<(), String> {
        let (mut list, array_obj) = self.annots_of(page)?;
        let gen = self.file.generation(note);
        list.insert(index.min(list.len()), Obj::Ref(note, gen));
        self.write_annots(page, list, array_obj);
        Ok(())
    }

    fn has_text(&self) -> bool {
        self.texts.iter().any(|t| t.iter().any(|g| !g.text.trim().is_empty()))
    }
}

fn arg_usize(cmd: &Command, key: &str) -> Result<usize, BusError> {
    cmd.args.get(key).and_then(Json::as_usize).ok_or_else(|| BusError::bad_args(&cmd.name, format!("\"{}\" must be a whole number from 0", key)))
}

fn arg_f64(cmd: &Command, key: &str) -> Result<f64, BusError> {
    cmd.args.get(key).and_then(Json::as_f64).ok_or_else(|| BusError::bad_args(&cmd.name, format!("\"{}\" must be a number", key)))
}

impl Document for FolioDoc {
    fn kind(&self) -> &'static str {
        "folio"
    }

    fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
        match cmd.name.as_str() {
            "folio.set_title" => {
                let title = match cmd.args.get("title") {
                    Some(Json::Str(s)) => Some(s.clone()),
                    None | Some(Json::Null) => None,
                    _ => return Err(BusError::bad_args(&cmd.name, "\"title\" must be a string (or null to remove it)")),
                };
                let old = self.set_title(title.as_deref());
                let undo = Command::new("folio.set_title", Json::obj().with("title", old.map(Json::from).unwrap_or(Json::Null)));
                Ok(Change { undo: vec![undo], pages_touched: Vec::new() })
            }
            "folio.add_note" => {
                let page = arg_usize(cmd, "page")?;
                let x = arg_f64(cmd, "x")?;
                let y = arg_f64(cmd, "y")?;
                let text = cmd.args.get("text").and_then(Json::as_str).unwrap_or("").to_string();
                let n = self.add_note(page, x, y, &text).map_err(BusError::failed)?;
                Ok(Change { undo: vec![Command::new("folio.remove_note", Json::obj().with("page", page).with("note", n))], pages_touched: vec![page] })
            }
            "folio.remove_note" => {
                let page = arg_usize(cmd, "page")?;
                let note = arg_usize(cmd, "note")?;
                let inv = self.remove_note(page, note.min(u32::MAX as usize) as u32).map_err(BusError::failed)?;
                Ok(Change { undo: vec![inv], pages_touched: vec![page] })
            }
            "folio.attach_note" => {
                let page = arg_usize(cmd, "page")?;
                let note = arg_usize(cmd, "note")?.min(u32::MAX as usize) as u32;
                let index = cmd.args.get("index").and_then(Json::as_usize).unwrap_or(usize::MAX);
                self.attach_note(page, note, index).map_err(BusError::failed)?;
                Ok(Change { undo: vec![Command::new("folio.remove_note", Json::obj().with("page", page).with("note", note))], pages_touched: vec![page] })
            }
            other => Err(BusError::unknown(other)),
        }
    }

    fn describe(&self) -> Json {
        let mut pages = Json::arr();
        for (i, p) in self.pages.iter().enumerate().take(500) {
            let (w, h) = p.size();
            pages.push(
                Json::obj()
                    .with("page", i)
                    .with("width", (w * 100.0).round() / 100.0)
                    .with("height", (h * 100.0).round() / 100.0)
                    .with("rotate", p.rotate as i64)
                    .with("annotations", self.annotations(i).len()),
            );
        }
        let mut outline = Json::arr();
        for o in self.outlines.iter().take(300) {
            outline.push(
                Json::obj()
                    .with("title", o.title.as_str())
                    .with("page", o.page.map(Json::from).unwrap_or(Json::Null))
                    .with("level", o.level),
            );
        }
        let annots: usize = (0..self.pages.len()).map(|i| self.annotations(i).len()).sum();
        let has_text = self.has_text();
        let images: usize = self.image_counts.iter().sum();
        Json::obj()
            .with("page_count", self.pages.len())
            .with("pages", pages)
            .with("title", self.title().map(Json::from).unwrap_or(Json::Null))
            .with("author", self.author().map(Json::from).unwrap_or(Json::Null))
            .with("outline", outline)
            .with("annotations", annots)
            .with("has_text", has_text)
            .with("seems_scanned", !has_text && images > 0)
            .with("version", self.file.version.as_str())
            .with("repaired", self.file.repaired)
            .with("modified", self.is_modified())
    }

    fn read(&self, what: &Json) -> Result<Json, BusError> {
        if let Some(p) = what.get("page") {
            let page = p.as_usize().ok_or_else(|| BusError::bad_args("read", "\"page\" is a whole number from 0"))?;
            if page >= self.pages.len() {
                return Err(BusError::failed(format!("no page {} (the document has {} pages)", page, self.pages.len())));
            }
            return Ok(Json::obj().with("page", page).with("text", self.page_text(page)));
        }
        if let Some(q) = what.get("search") {
            let q = q.as_str().ok_or_else(|| BusError::bad_args("read", "\"search\" is the text to find"))?;
            let hits = self.search(q);
            let mut matches = Json::arr();
            for (p, ctx) in &hits {
                matches.push(Json::obj().with("page", *p).with("text", ctx.as_str()));
            }
            return Ok(Json::obj().with("query", q).with("count", hits.len()).with("matches", matches));
        }
        if let Some(p) = what.get("annotations") {
            let page = p.as_usize().ok_or_else(|| BusError::bad_args("read", "\"annotations\" is a page number from 0"))?;
            let mut list = Json::arr();
            for a in self.annotations(page) {
                list.push(
                    Json::obj()
                        .with("note", a.num)
                        .with("subtype", a.subtype.as_str())
                        .with("rect", Json::Arr(a.rect.iter().map(|v| Json::from((v * 100.0).round() / 100.0)).collect()))
                        .with("contents", a.contents.as_str()),
                );
            }
            return Ok(Json::obj().with("page", page).with("annotations", list));
        }
        Err(BusError::bad_args("read", "read takes { \"page\": n }, { \"search\": \"words\" } or { \"annotations\": n }"))
    }

    fn display(&self, page: usize) -> DisplayList {
        match self.lists.get(page) {
            Some(l) => l.clone(),
            None => DisplayList::new(612.0, 792.0),
        }
    }

    fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![
            CommandSpec::new("folio.set_title", "Set the document's title (its info dictionary), saved as an incremental update", "{ \"title\": string | null }"),
            CommandSpec::new(
                "folio.add_note",
                "Add a sticky note on a page; x and y are display points from the page's top-left",
                "{ \"page\": n, \"x\": number, \"y\": number, \"text\": string }",
            ),
            CommandSpec::new("folio.remove_note", "Remove an annotation from a page (its number comes from read { \"annotations\": page })", "{ \"page\": n, \"note\": n }"),
        ]
    }

    fn resources(&self) -> Option<&Resources> {
        Some(&self.resources)
    }
}
