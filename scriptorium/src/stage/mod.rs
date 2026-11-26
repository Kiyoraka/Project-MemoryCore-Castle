//! Stage: the slides editor. M1 is **open and see**: a `.pptx` opens, every slide is resolved through
//! its layout, master and theme and drawn as a display list, saving changes nothing it did not touch,
//! and the AI's door can outline, read and set the text of shapes and notes.
//!
//! - [`model`]: the deck as read (presentation, slide order, parsed slide / layout / master / notes
//!   parts, themes, master text styles) and the placeholder matching rules.
//! - [`style`]: run, paragraph, list and body properties with their fill-from-below inheritance.
//! - [`render`]: a slide to a [`DisplayList`] (background, master and layout shapes, slide shapes in
//!   z-order; shapes, pictures, groups, connectors and tables).
//! - [`edit`]: the commands' XML patches and their exact inverses.
//! - [`blank`]: the parts of a new deck.

pub mod blank;
pub mod edit;
pub mod model;
pub mod render;
pub mod style;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Change, Command, CommandSpec, Document};
use crate::ground::display::{DisplayList, Item, Matrix, Rect, Resources};
use crate::ground::draw::{Fill, Geometry, Line};
use crate::ground::font::FontIndex;
use crate::ground::opc::Package;
use crate::ground::text::FontCache;

use edit::{ShapeKey, Target};
use model::{all_shapes, shape_id, shape_kind, shape_name, shape_text, sp_tree, Deck, Ph};
use render::{resolve_paras, resolve_shape, Owner, Painter, RPara, Scene};
use style::BodyProps;

/// Why a deck could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageError(pub String);

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StageError {}

/// This machine's fonts, indexed once per process and shared by every open deck.
pub fn system_fonts() -> FontCache {
    static INDEX: OnceLock<Arc<FontIndex>> = OnceLock::new();
    FontCache::new(INDEX.get_or_init(|| Arc::new(FontIndex::system())).clone())
}

/// What a shape resolves to through its placeholder chain (for the AI, the inspector and tests).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeInfo {
    pub name: String,
    pub id: Option<u32>,
    pub kind: &'static str,
    pub placeholder: Option<Ph>,
    /// The unrotated box on the slide (points; a group's children in the group's child space).
    pub rect: Option<Rect>,
    pub rotation: f64,
    pub geometry: Geometry,
    pub fill: Option<Fill>,
    pub line: Option<Line>,
    pub body: BodyProps,
    pub paragraphs: Vec<RPara>,
}

/// An open deck.
pub struct StageDoc {
    deck: Deck,
    fonts: FontCache,
    resources: Resources,
    images: HashMap<String, Option<u32>>,
    pages: Vec<DisplayList>,
}

impl fmt::Debug for StageDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StageDoc").field("slides", &self.deck.slides.len()).field("part", &self.deck.pres_part).finish()
    }
}

impl StageDoc {
    /// Open a `.pptx` (or `.pptm`) with this machine's fonts.
    pub fn open(bytes: &[u8]) -> Result<StageDoc, StageError> {
        StageDoc::open_with_fonts(bytes, system_fonts())
    }

    /// Open with a font cache of the caller's (tests use an empty one).
    pub fn open_with_fonts(bytes: &[u8], fonts: FontCache) -> Result<StageDoc, StageError> {
        let pkg = Package::open(bytes).map_err(|e| StageError(e.to_string()))?;
        let deck = Deck::load(pkg)?;
        let mut doc = StageDoc { deck, fonts, resources: Resources::new(), images: HashMap::new(), pages: Vec::new() };
        doc.render_all();
        Ok(doc)
    }

    /// A new deck: one title slide on a 16:9 page.
    pub fn new_blank() -> StageDoc {
        StageDoc::new_blank_with_fonts(system_fonts())
    }

    pub fn new_blank_with_fonts(fonts: FontCache) -> StageDoc {
        match StageDoc::open_with_fonts(&blank::blank_deck(), fonts) {
            Ok(d) => d,
            // Our own blank deck always opens; this arm only keeps the signature total.
            Err(e) => panic!("the blank deck must open: {}", e),
        }
    }

    /// The deck's bytes: the input unchanged when nothing was edited, otherwise only edited parts
    /// rewritten (every other part keeps its exact compressed bytes).
    pub fn save(&self) -> Vec<u8> {
        self.deck.pkg.save()
    }

    pub fn package(&self) -> &Package {
        &self.deck.pkg
    }

    pub fn deck(&self) -> &Deck {
        &self.deck
    }

    pub fn slide_count(&self) -> usize {
        self.deck.slides.len()
    }

    /// The slide size in points.
    pub fn slide_size_pt(&self) -> (f64, f64) {
        (self.deck.size.0 as f64 / 12700.0, self.deck.size.1 as f64 / 12700.0)
    }

    fn render_all(&mut self) {
        let StageDoc { deck, fonts, resources, images, pages } = self;
        let mut painter = Painter { fonts, res: resources, images };
        *pages = (0..deck.slides.len()).map(|i| painter.render_slide(deck, i)).collect();
    }

    fn render_page(&mut self, i: usize) {
        let StageDoc { deck, fonts, resources, images, pages } = self;
        if i < pages.len() {
            let mut painter = Painter { fonts, res: resources, images };
            pages[i] = painter.render_slide(deck, i);
        }
    }

    /// A slide scaled to `width_pt` wide, for the slide strip.
    pub fn thumbnail(&self, page: usize, width_pt: f64) -> DisplayList {
        let (w, h) = self.slide_size_pt();
        let k = if w > 0.0 { width_pt / w } else { 1.0 };
        let mut dl = DisplayList::new(width_pt, h * k);
        if let Some(p) = self.pages.get(page) {
            dl.items.reserve(p.items.len() + 2);
            dl.items.push(Item::PushTransform(Matrix::scale(k, k)));
            dl.items.extend(p.items.iter().cloned());
            dl.items.push(Item::PopTransform);
        }
        dl
    }

    /// A shape of a slide (by name, or id) as it resolves through its chain.
    pub fn shape_info(&self, slide: usize, shape: &str) -> Option<ShapeInfo> {
        let scene = Scene::new(&self.deck, slide)?;
        let path = edit::find_shape(scene.slide, &ShapeKey::Name(shape.to_string()))?;
        let el = edit::el_at(scene.slide, &path)?;
        let r = resolve_shape(&scene, el, Owner::Slide);
        let xfrm = r.xfrm();
        let body = r.body();
        let (fs, lr) = body.shrink();
        let paragraphs = el.child("txBody").map(|tx| resolve_paras(&scene, tx, &r.text_chain(&scene), fs, lr)).unwrap_or_default();
        Some(ShapeInfo {
            name: shape_name(el),
            id: shape_id(el),
            kind: shape_kind(el),
            placeholder: r.ph.clone(),
            rect: xfrm.map(|x| x.rect()),
            rotation: xfrm.map(|x| x.rotation_deg()).unwrap_or(0.0),
            geometry: r.geometry(),
            fill: r.fill(&scene),
            line: r.line(&scene),
            body,
            paragraphs,
        })
    }

    fn slide_json(&self, i: usize, full: bool) -> Json {
        let excerpt = |s: String| -> Json {
            if full || s.chars().count() <= 120 {
                Json::from(s)
            } else {
                Json::from(format!("{}…", s.chars().take(120).collect::<String>()))
            }
        };
        let num = Some(self.deck.slide_number(i));
        let mut title = String::new();
        let mut phs = Vec::new();
        let mut others = Vec::new();
        if let Some(root) = self.deck.sheet(&self.deck.slides[i].part) {
            if let Some(tree) = sp_tree(root) {
                for s in all_shapes(tree) {
                    let text = shape_text(s, num);
                    let mut j = Json::obj().with("name", shape_name(s)).with("kind", shape_kind(s));
                    if let Some(id) = shape_id(s) {
                        j.set("id", id);
                    }
                    match Ph::of(s) {
                        Some(ph) => {
                            if title.is_empty() && matches!(ph.ty.as_str(), "title" | "ctrTitle") {
                                title = text.clone();
                            }
                            j.set("type", ph.ty.as_str());
                            if let Some(idx) = ph.idx {
                                j.set("idx", idx);
                            }
                            j.set("text", excerpt(text));
                            phs.push(j);
                        }
                        None => {
                            if !text.is_empty() || !full {
                                j.set("text", excerpt(text));
                            }
                            others.push(j);
                        }
                    }
                }
            }
        }
        let mut j = Json::obj().with("index", i).with("layout", self.deck.layout_name(i)).with("title", title);
        if full {
            let mut all = phs;
            all.extend(others);
            j.set("shapes", Json::Arr(all));
        } else {
            j.set("placeholders", Json::Arr(phs));
            j.set("shapes", Json::Arr(others));
        }
        j.set("notes", self.deck.notes_text(i).map(excerpt).unwrap_or(Json::Null));
        j
    }

    fn slide_arg(&self, cmd: &str, args: &Json) -> Result<usize, BusError> {
        let i = args.get("slide").and_then(Json::as_usize).ok_or_else(|| BusError::bad_args(cmd, "\"slide\" is a slide index from 0"))?;
        if i >= self.deck.slides.len() {
            return Err(BusError::failed(format!("no slide {} (the deck has {} slide{})", i, self.deck.slides.len(), if self.deck.slides.len() == 1 { "" } else { "s" })));
        }
        Ok(i)
    }
}

impl Document for StageDoc {
    fn kind(&self) -> &'static str {
        "stage"
    }

    fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
        let name = cmd.name.as_str();
        let args = &cmd.args;
        match name {
            "stage.set_shape_text" => {
                let slide = self.slide_arg(name, args)?;
                let key = args.get("shape").and_then(ShapeKey::from_json).ok_or_else(|| BusError::bad_args(name, "\"shape\" is a shape's name or id"))?;
                let text = args.get("text").and_then(Json::as_str).ok_or_else(|| BusError::bad_args(name, "\"text\" is a string"))?;
                let undo = edit::set_shape_text(&mut self.deck, slide, &key, text)?;
                self.render_page(slide);
                Ok(Change { undo: vec![undo], pages_touched: vec![slide] })
            }
            "stage.set_notes" => {
                let slide = self.slide_arg(name, args)?;
                let text = args.get("text").and_then(Json::as_str).ok_or_else(|| BusError::bad_args(name, "\"text\" is a string"))?;
                let undo = edit::set_notes(&mut self.deck, slide, text)?;
                Ok(Change { undo: vec![undo], pages_touched: Vec::new() })
            }
            "stage.restore_text" => {
                let slide = self.slide_arg(name, args)?;
                let target = match args.get("target").and_then(Json::as_str) {
                    Some("notes") => Target::Notes,
                    _ => Target::Slide,
                };
                let id = args.get("shape_id").and_then(Json::as_usize).and_then(|n| u32::try_from(n).ok()).ok_or_else(|| BusError::bad_args(name, "\"shape_id\" is a shape id"))?;
                let xml = args.get("xml").and_then(Json::as_str);
                let undo = edit::restore_text(&mut self.deck, slide, target, id, xml)?;
                let pages = if target == Target::Slide {
                    self.render_page(slide);
                    vec![slide]
                } else {
                    Vec::new()
                };
                Ok(Change { undo: vec![undo], pages_touched: pages })
            }
            "stage.drop_notes" => {
                let slide = self.slide_arg(name, args)?;
                let master = args.get("master").and_then(Json::as_bool).unwrap_or(false);
                let text = self.deck.notes_text(slide).unwrap_or_default();
                edit::drop_notes(&mut self.deck, slide, master)?;
                Ok(Change { undo: vec![Command::new("stage.set_notes", Json::obj().with("slide", slide).with("text", text))], pages_touched: Vec::new() })
            }
            _ => Err(BusError::unknown(name)),
        }
    }

    fn describe(&self) -> Json {
        let (w, h) = self.slide_size_pt();
        Json::obj()
            .with("slide_width_pt", w)
            .with("slide_height_pt", h)
            .with("slide_count", self.deck.slides.len())
            .with("slides", Json::Arr((0..self.deck.slides.len()).map(|i| self.slide_json(i, false)).collect()))
    }

    fn read(&self, what: &Json) -> Result<Json, BusError> {
        match what.get("slide") {
            None | Some(Json::Null) => Ok(Json::obj().with("slides", Json::Arr((0..self.deck.slides.len()).map(|i| self.slide_json(i, true)).collect()))),
            Some(_) => {
                let i = self.slide_arg("read", what)?;
                Ok(self.slide_json(i, true).with("slide", i))
            }
        }
    }

    fn display(&self, page: usize) -> DisplayList {
        self.pages.get(page).cloned().unwrap_or_else(|| {
            let (w, h) = self.slide_size_pt();
            DisplayList::new(w, h)
        })
    }

    fn page_count(&self) -> usize {
        self.deck.slides.len()
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![
            CommandSpec::new(
                "stage.set_shape_text",
                "Replace the text of a shape on a slide (lines become paragraphs), keeping its first paragraph's and first run's formatting.",
                r#"{ "slide": number (from 0), "shape": string (name) | number (id), "text": string }"#,
            ),
            CommandSpec::new("stage.set_notes", "Replace the speaker notes of a slide (lines become paragraphs).", r#"{ "slide": number (from 0), "text": string }"#),
        ]
    }

    fn resources(&self) -> Option<&Resources> {
        Some(&self.resources)
    }
}
