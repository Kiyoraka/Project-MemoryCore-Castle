//! The editing ground: what every editor shares to put a caret and a selection on its pages. The page
//! (the host's canvas) knows only page points; the editor knows its own places. This module is the
//! seam between them, and nothing in it knows any one editor.
//!
//! - A [`Pos`] is an editor's own place, as JSON the ground never looks inside (Scroll may say
//!   `{"para":3,"offset":12}`, Ledger `{"sheet":0,"cell":"B7"}`, Stage `{"slide":2,"shape":5,..}`).
//! - A [`Selection`] is an anchor and a focus (the focus moves; equal means a caret).
//! - [`Editing`] is what a document offers once it can be edited by hand: a hit test from a page
//!   point to a place, the caret's box, the selection's boxes, the formatting state under the
//!   selection (the ribbon's pressed buttons) and a copy of what is selected.
//!
//! Every change to the document stays an ordinary bus command each editor defines - typing, paste
//! and delete included - so undo, the history and the AI's door see them like any other step. The
//! convention: the command takes the selection it acts on as `"sel"` (the JSON of [`Selection`]),
//! e.g. `scroll.insert_text { "sel": {..}, "text": "abc" }`, `scroll.delete { "sel": {..}, "back": true }`,
//! `ledger.paste { "sel": {..}, "text": "..", "rich"?: {..} }`. After a command, [`Editing::landed`]
//! says where the selection lands (the caret after the typed text, the match a find went to).
//!
//! A find is a command too (`<editor>.find { "query", "next": bool, "sel" }`): it changes nothing,
//! answers its match through `landed`, and the bus keeps no undo step for a command that changed
//! nothing. A replace (`<editor>.replace { "query", "with", "all": bool, "sel" }`) is a real step.
//!
//! The host reaches all of this through [`door`] (verbs `hit`, `caret`, `selection`, `state`, `copy`,
//! `move`), JSON in and out, the same way it reaches the bus's own door.

use super::bus::json::Json;
use super::bus::{BusError, Command, Document};
use super::display::Rect;

/// An editor's own place in its document. Opaque to the ground: only the editor that made it reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pos(pub Json);

impl Pos {
    pub fn new(value: Json) -> Pos {
        Pos(value)
    }

    pub fn json(&self) -> &Json {
        &self.0
    }

    pub fn to_json(&self) -> Json {
        self.0.clone()
    }

    /// Any JSON but `null` is a place (null means "nowhere").
    pub fn from_json(v: &Json) -> Option<Pos> {
        if v.is_null() {
            None
        } else {
            Some(Pos(v.clone()))
        }
    }
}

/// Where the selection starts (`anchor`, fixed while it grows) and where it ends (`focus`, the caret).
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub anchor: Pos,
    pub focus: Pos,
}

impl Selection {
    pub fn new(anchor: Pos, focus: Pos) -> Selection {
        Selection { anchor, focus }
    }

    /// A caret: anchor and focus at one place.
    pub fn caret(pos: Pos) -> Selection {
        Selection { anchor: pos.clone(), focus: pos }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }

    /// `{"anchor": pos, "focus": pos}`
    pub fn to_json(&self) -> Json {
        Json::obj().with("anchor", self.anchor.to_json()).with("focus", self.focus.to_json())
    }

    /// Reads `{"anchor": pos, "focus"?: pos}` (no focus, or a null one, means a caret at the anchor).
    pub fn from_json(v: &Json) -> Option<Selection> {
        let anchor = Pos::from_json(v.get("anchor")?)?;
        let focus = v.get("focus").and_then(Pos::from_json).unwrap_or_else(|| anchor.clone());
        Some(Selection { anchor, focus })
    }
}

/// The caret's box on a page, in page points (y down): a thin bar `height` tall from (`x`, `y`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretRect {
    pub page: usize,
    pub x: f64,
    pub y: f64,
    pub height: f64,
}

impl CaretRect {
    /// `{"page","x","y","height"}`
    pub fn to_json(&self) -> Json {
        Json::obj().with("page", self.page).with("x", self.x).with("y", self.y).with("height", self.height)
    }

    pub fn from_json(v: &Json) -> Option<CaretRect> {
        let num = |k: &str| v.get(k).and_then(Json::as_f64).filter(|n| n.is_finite());
        Some(CaretRect { page: v.get("page")?.as_usize()?, x: num("x")?, y: num("y")?, height: num("height")?.max(0.0) })
    }
}

/// What a copy takes: plain text always, and the editor's own rich form when it has one (pasted back
/// into the same editor it keeps formatting; anywhere else the text goes).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Clip {
    pub text: String,
    pub rich: Option<Json>,
}

impl Clip {
    pub fn text(text: impl Into<String>) -> Clip {
        Clip { text: text.into(), rich: None }
    }

    /// `{"text": "..", "rich": {..} | null}`
    pub fn to_json(&self) -> Json {
        Json::obj().with("text", self.text.as_str()).with("rich", self.rich.clone().unwrap_or(Json::Null))
    }

    pub fn from_json(v: &Json) -> Clip {
        Clip { text: v.get("text").and_then(Json::as_str).unwrap_or("").to_string(), rich: v.get("rich").filter(|r| !r.is_null()).cloned() }
    }
}

/// One selection box as JSON: `{"page","x","y","w","h"}`.
pub fn rect_json(page: usize, r: &Rect) -> Json {
    Json::obj().with("page", page).with("x", r.x).with("y", r.y).with("w", r.w).with("h", r.h)
}

/// Reads [`rect_json`]'s shape back.
pub fn rect_from_json(v: &Json) -> Option<(usize, Rect)> {
    let num = |k: &str| v.get(k).and_then(Json::as_f64).filter(|n| n.is_finite());
    Some((v.get("page")?.as_usize()?, Rect::new(num("x")?, num("y")?, num("w")?, num("h")?)))
}

/// The caret motions [`Editing::step`] is asked for (the page's arrow keys, Home/End, Ctrl+Home/End).
pub const MOTIONS: [&str; 10] = ["left", "right", "up", "down", "word_left", "word_right", "line_start", "line_end", "doc_start", "doc_end"];

/// What a document offers once it can be edited by hand. A document says so through
/// [`Document::editing`]; the five required methods are the seam, the rest have defaults.
pub trait Editing {
    /// The place under the page point (`page`, `x`, `y`) in points, or `None` when nothing there takes
    /// a caret.
    fn hit(&self, page: usize, x: f64, y: f64) -> Option<Pos>;
    /// The caret's box at `pos`, or `None` for a place that is not shown (any more).
    fn caret(&self, pos: &Pos) -> Option<CaretRect>;
    /// The boxes that paint `sel` (each with its page), in reading order; none for a caret.
    fn selection_rects(&self, sel: &Selection) -> Vec<(usize, Rect)>;
    /// The formatting under the selection for the ribbon, the editor's own keys (`{"bold":true,..}`);
    /// a ribbon button is pressed when its `stateKey` is true here.
    fn state(&self, sel: &Selection) -> Json;
    /// What a copy of `sel` takes.
    fn copy(&self, sel: &Selection) -> Clip;

    /// The place one `motion` (one of [`MOTIONS`]) away from `pos`. `None` lets the ground try (up and
    /// down by the caret's box and a hit test) or keep the caret where it is.
    fn step(&self, _pos: &Pos, _motion: &str) -> Option<Pos> {
        None
    }

    /// Where the selection lands after `cmd` was applied (the caret after typed text, a find's match).
    /// `None` keeps the page's selection as it was.
    fn landed(&self, _cmd: &Command) -> Option<Selection> {
        None
    }
}

/// `step` with the ground's own help: when the editor has no answer, up and down hit the page a line
/// above or below the caret's box.
pub fn step(ed: &dyn Editing, pos: &Pos, motion: &str) -> Option<Pos> {
    if let Some(p) = ed.step(pos, motion) {
        return Some(p);
    }
    let c = ed.caret(pos)?;
    let line = if c.height > 0.0 { c.height } else { 12.0 };
    match motion {
        "up" if c.y - line * 0.5 >= 0.0 => ed.hit(c.page, c.x, c.y - line * 0.5),
        "down" => ed.hit(c.page, c.x, c.y + line * 1.5),
        _ => None,
    }
}

fn sel_arg(args: &Json, verb: &str) -> Result<Selection, BusError> {
    args.get("sel")
        .and_then(Selection::from_json)
        .ok_or_else(|| BusError::bad_args("", format!("{verb} takes {{ \"sel\": {{ \"anchor\": pos, \"focus\": pos }} }}")))
}

fn pos_arg(args: &Json, verb: &str) -> Result<Pos, BusError> {
    args.get("pos").and_then(Pos::from_json).ok_or_else(|| BusError::bad_args("", format!("{verb} takes {{ \"pos\": pos }}")))
}

/// The host's way in, JSON in and out (a document that cannot be edited yet answers nothing, never an
/// error, so the page simply shows no caret):
/// - `hit` `{ "page", "x", "y" }` -> a pos or `null`
/// - `caret` `{ "pos" }` -> `{ "page", "x", "y", "height" }` or `null`
/// - `selection` `{ "sel" }` -> `[{ "page", "x", "y", "w", "h" }, ..]`
/// - `state` `{ "sel" }` -> the editor's state object (`{}` when it cannot be edited)
/// - `copy` `{ "sel" }` -> `{ "text", "rich" }`
/// - `move` `{ "pos", "motion" }` -> the new pos, or `null` when it stays
pub fn door(doc: &dyn Document, verb: &str, args: &Json) -> Result<Json, BusError> {
    let ed = doc.editing();
    match verb {
        "hit" => {
            let page = args.get("page").and_then(Json::as_usize).ok_or_else(|| BusError::bad_args("", "hit's \"page\" is a whole number from 0"))?;
            let num = |k: &str| args.get(k).and_then(Json::as_f64).filter(|n| n.is_finite());
            let (Some(x), Some(y)) = (num("x"), num("y")) else {
                return Err(BusError::bad_args("", "hit takes { \"page\", \"x\", \"y\" } in points"));
            };
            if page >= doc.page_count() {
                return Ok(Json::Null);
            }
            Ok(ed.and_then(|e| e.hit(page, x, y)).map(|p| p.to_json()).unwrap_or(Json::Null))
        }
        "caret" => {
            let pos = pos_arg(args, verb)?;
            Ok(ed.and_then(|e| e.caret(&pos)).map(|c| c.to_json()).unwrap_or(Json::Null))
        }
        "selection" => {
            let sel = sel_arg(args, verb)?;
            let rects = ed.map(|e| e.selection_rects(&sel)).unwrap_or_default();
            Ok(Json::Arr(rects.iter().map(|(p, r)| rect_json(*p, r)).collect()))
        }
        "state" => {
            let sel = sel_arg(args, verb)?;
            Ok(ed.map(|e| e.state(&sel)).unwrap_or_else(Json::obj))
        }
        "copy" => {
            let sel = sel_arg(args, verb)?;
            Ok(ed.map(|e| e.copy(&sel)).unwrap_or_default().to_json())
        }
        "move" => {
            let pos = pos_arg(args, verb)?;
            let motion = args.get("motion").and_then(Json::as_str).unwrap_or("");
            if !MOTIONS.contains(&motion) {
                return Err(BusError::bad_args("", format!("move's \"motion\" is one of: {}", MOTIONS.join(", "))));
            }
            Ok(ed.and_then(|e| step(e, &pos, motion)).map(|p| p.to_json()).unwrap_or(Json::Null))
        }
        other => Err(BusError::bad_args("", format!("the editing door has no verb '{other}'; its verbs are: {}", EDIT_VERBS.join(", ")))),
    }
}

/// The editing door's verbs.
pub const EDIT_VERBS: [&str; 6] = ["hit", "caret", "selection", "state", "copy", "move"];

/// [`door`] for a host edge that speaks text; errors come back as `{"ok":false,"error":".."}`.
pub fn door_json(doc: &dyn Document, verb: &str, args_text: &str) -> String {
    let args = if args_text.trim().is_empty() { Ok(Json::obj()) } else { super::bus::json::parse(args_text).map_err(|e| BusError::bad_args("", e)) };
    match args.and_then(|a| door(doc, verb, &a)) {
        Ok(v) => v.to_string(),
        Err(e) => Json::obj().with("ok", false).with("error", e.to_string()).to_string(),
    }
}

/// The selection a command landed on, as JSON (`null` when the document cannot say).
pub fn landed_json(doc: &dyn Document, cmd: &Command) -> Json {
    doc.editing().and_then(|e| e.landed(cmd)).map(|s| s.to_json()).unwrap_or(Json::Null)
}

// ---------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground::bus::json::parse;
    use crate::ground::bus::{Change, CommandSpec};
    use crate::ground::display::DisplayList;

    /// One line of text on one page, 10 points per character, the line 20 points tall at y 100.
    struct OneLine {
        text: String,
    }

    fn at(p: &Pos) -> usize {
        p.json().get("at").and_then(Json::as_usize).unwrap_or(0)
    }

    fn pos(i: usize) -> Pos {
        Pos(Json::obj().with("at", i))
    }

    impl Editing for OneLine {
        fn hit(&self, _page: usize, x: f64, y: f64) -> Option<Pos> {
            if !(100.0..120.0).contains(&y) {
                return None;
            }
            Some(pos(((x / 10.0).round().max(0.0) as usize).min(self.text.len())))
        }
        fn caret(&self, p: &Pos) -> Option<CaretRect> {
            let i = at(p);
            (i <= self.text.len()).then(|| CaretRect { page: 0, x: i as f64 * 10.0, y: 100.0, height: 20.0 })
        }
        fn selection_rects(&self, sel: &Selection) -> Vec<(usize, Rect)> {
            let (a, b) = (at(&sel.anchor).min(at(&sel.focus)), at(&sel.anchor).max(at(&sel.focus)));
            if a == b {
                return Vec::new();
            }
            vec![(0, Rect::new(a as f64 * 10.0, 100.0, (b - a) as f64 * 10.0, 20.0))]
        }
        fn state(&self, sel: &Selection) -> Json {
            Json::obj().with("collapsed", sel.is_collapsed())
        }
        fn copy(&self, sel: &Selection) -> Clip {
            let (a, b) = (at(&sel.anchor).min(at(&sel.focus)), at(&sel.anchor).max(at(&sel.focus)));
            Clip::text(&self.text[a.min(self.text.len())..b.min(self.text.len())])
        }
        fn step(&self, p: &Pos, motion: &str) -> Option<Pos> {
            let i = at(p);
            match motion {
                "left" => Some(pos(i.saturating_sub(1))),
                "right" => Some(pos((i + 1).min(self.text.len()))),
                "line_start" | "doc_start" => Some(pos(0)),
                "line_end" | "doc_end" => Some(pos(self.text.len())),
                _ => None,
            }
        }
        fn landed(&self, cmd: &Command) -> Option<Selection> {
            let sel = Selection::from_json(cmd.args.get("sel")?)?;
            let text = cmd.args.get("text").and_then(Json::as_str)?;
            Some(Selection::caret(pos(at(&sel.focus) + text.len())))
        }
    }

    impl Document for OneLine {
        fn kind(&self) -> &'static str {
            "line"
        }
        fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
            Err(BusError::unknown(&cmd.name))
        }
        fn describe(&self) -> Json {
            Json::obj()
        }
        fn read(&self, _what: &Json) -> Result<Json, BusError> {
            Ok(Json::Null)
        }
        fn display(&self, _page: usize) -> DisplayList {
            DisplayList::new(300.0, 400.0)
        }
        fn page_count(&self) -> usize {
            1
        }
        fn commands(&self) -> Vec<CommandSpec> {
            Vec::new()
        }
        fn editing(&self) -> Option<&dyn Editing> {
            Some(self)
        }
    }

    /// A document that cannot be edited yet.
    struct Plain;

    impl Document for Plain {
        fn kind(&self) -> &'static str {
            "plain"
        }
        fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
            Err(BusError::unknown(&cmd.name))
        }
        fn describe(&self) -> Json {
            Json::obj()
        }
        fn read(&self, _what: &Json) -> Result<Json, BusError> {
            Ok(Json::Null)
        }
        fn display(&self, _page: usize) -> DisplayList {
            DisplayList::new(300.0, 400.0)
        }
        fn page_count(&self) -> usize {
            1
        }
        fn commands(&self) -> Vec<CommandSpec> {
            Vec::new()
        }
    }

    fn doc() -> OneLine {
        OneLine { text: "hello world".into() }
    }

    #[test]
    fn json_round_trips() {
        let sel = Selection::new(pos(1), pos(4));
        let back = Selection::from_json(&parse(&sel.to_json().to_string()).unwrap()).unwrap();
        assert_eq!(back, sel);
        assert!(!sel.is_collapsed());
        let caret = Selection::from_json(&parse(r#"{"anchor":{"at":3}}"#).unwrap()).unwrap();
        assert!(caret.is_collapsed());
        assert_eq!(caret.focus, pos(3));
        assert_eq!(Selection::from_json(&parse(r#"{"focus":{"at":3}}"#).unwrap()), None);
        assert_eq!(Selection::from_json(&parse(r#"{"anchor":null}"#).unwrap()), None);
        assert_eq!(Pos::from_json(&Json::Null), None);

        let c = CaretRect { page: 2, x: 10.5, y: 20.0, height: 12.0 };
        assert_eq!(CaretRect::from_json(&parse(&c.to_json().to_string()).unwrap()), Some(c));
        assert_eq!(CaretRect::from_json(&parse(r#"{"page":-1,"x":0,"y":0,"height":1}"#).unwrap()), None);

        let clip = Clip { text: "a\tb".into(), rich: Some(Json::obj().with("runs", 1)) };
        assert_eq!(Clip::from_json(&parse(&clip.to_json().to_string()).unwrap()), clip);
        assert_eq!(Clip::from_json(&parse(r#"{"text":"x","rich":null}"#).unwrap()), Clip::text("x"));

        let r = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(rect_from_json(&rect_json(5, &r)), Some((5, r)));
    }

    #[test]
    fn the_door_speaks_for_an_editing_document() {
        let d = doc();
        let hit = door(&d, "hit", &Json::obj().with("page", 0).with("x", 31.0).with("y", 105.0)).unwrap();
        assert_eq!(hit.to_string(), r#"{"at":3}"#);
        assert!(door(&d, "hit", &Json::obj().with("page", 0).with("x", 31.0).with("y", 5.0)).unwrap().is_null());
        assert!(door(&d, "hit", &Json::obj().with("page", 4).with("x", 31.0).with("y", 105.0)).unwrap().is_null(), "no such page");
        assert!(door(&d, "hit", &Json::obj().with("page", 0)).is_err());

        let caret = door(&d, "caret", &Json::obj().with("pos", pos(3).to_json())).unwrap();
        assert_eq!(caret.to_string(), r#"{"page":0,"x":30,"y":100,"height":20}"#);

        let sel = Selection::new(pos(6), pos(1)).to_json();
        let rects = door(&d, "selection", &Json::obj().with("sel", sel.clone())).unwrap();
        assert_eq!(rects.to_string(), r#"[{"page":0,"x":10,"y":100,"w":50,"h":20}]"#);
        assert_eq!(door(&d, "copy", &Json::obj().with("sel", sel.clone())).unwrap().to_string(), r#"{"text":"ello ","rich":null}"#);
        assert_eq!(door(&d, "state", &Json::obj().with("sel", sel)).unwrap().to_string(), r#"{"collapsed":false}"#);
        assert!(door(&d, "state", &Json::obj()).is_err(), "state needs a selection");

        let mv = |p: usize, m: &str| door(&d, "move", &Json::obj().with("pos", pos(p).to_json()).with("motion", m)).unwrap();
        assert_eq!(mv(3, "left").to_string(), r#"{"at":2}"#);
        assert_eq!(mv(3, "doc_end").to_string(), r#"{"at":11}"#);
        assert!(mv(3, "up").is_null(), "the ground's try: nothing a line above");
        assert!(door(&d, "move", &Json::obj().with("pos", pos(1).to_json()).with("motion", "sideways")).is_err());
        assert!(door(&d, "fly", &Json::obj()).unwrap_err().to_string().contains("hit, caret"));

        let typed = Command::new("line.insert_text", Json::obj().with("sel", Selection::caret(pos(2)).to_json()).with("text", "abc"));
        assert_eq!(landed_json(&d, &typed).to_string(), r#"{"anchor":{"at":5},"focus":{"at":5}}"#);
    }

    #[test]
    fn a_document_without_editing_answers_nothing() {
        let d = Plain;
        assert!(d.editing().is_none());
        assert!(door(&d, "hit", &Json::obj().with("page", 0).with("x", 1.0).with("y", 1.0)).unwrap().is_null());
        assert!(door(&d, "caret", &Json::obj().with("pos", pos(0).to_json())).unwrap().is_null());
        let sel = Json::obj().with("sel", Selection::caret(pos(0)).to_json());
        assert_eq!(door(&d, "selection", &sel).unwrap().to_string(), "[]");
        assert_eq!(door(&d, "state", &sel).unwrap().to_string(), "{}");
        assert_eq!(door(&d, "copy", &sel).unwrap().to_string(), r#"{"text":"","rich":null}"#);
        assert!(landed_json(&d, &Command::new("x", Json::obj())).is_null());
        assert!(door_json(&d, "caret", "{").contains("\"ok\":false"));
        assert_eq!(door_json(&d, "hit", r#"{"page":0,"x":1,"y":2}"#), "null");
    }

    #[test]
    fn the_ground_steps_up_and_down_by_the_caret() {
        /// Two lines: y 100..120 and 120..140; no motions of its own.
        struct TwoLines;
        impl Editing for TwoLines {
            fn hit(&self, _page: usize, x: f64, y: f64) -> Option<Pos> {
                let line = if (100.0..120.0).contains(&y) { 0 } else if (120.0..140.0).contains(&y) { 1 } else { return None };
                Some(Pos(Json::obj().with("line", line).with("x", (x / 10.0).round())))
            }
            fn caret(&self, p: &Pos) -> Option<CaretRect> {
                let line = p.json().get("line")?.as_f64()?;
                let x = p.json().get("x")?.as_f64()? * 10.0;
                Some(CaretRect { page: 0, x, y: 100.0 + line * 20.0, height: 20.0 })
            }
            fn selection_rects(&self, _sel: &Selection) -> Vec<(usize, Rect)> {
                Vec::new()
            }
            fn state(&self, _sel: &Selection) -> Json {
                Json::obj()
            }
            fn copy(&self, _sel: &Selection) -> Clip {
                Clip::default()
            }
        }
        let top = Pos(Json::obj().with("line", 0).with("x", 4));
        let down = step(&TwoLines, &top, "down").unwrap();
        assert_eq!(down.to_json().to_string(), r#"{"line":1,"x":4}"#);
        assert_eq!(step(&TwoLines, &down, "up"), Some(top.clone()));
        assert_eq!(step(&TwoLines, &down, "down"), None);
        assert_eq!(step(&TwoLines, &top, "left"), None, "left and right are the editor's to say");
    }
}
