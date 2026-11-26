//! Scroll's places: a paragraph and a character offset in it, in the body or in a header or footer
//! part, and the characters ("atoms") a paragraph is made of for editing.
//!
//! A [`SPos`] travels as the ground's opaque [`Pos`] JSON: `{"p": 3, "o": 12}` for the body (the
//! paragraph's flat index, table cells included, and the character offset), or
//! `{"h": "/word/header1.xml", "p": 0, "o": 2, "pg": 1}` in a header or footer (`pg`: the page it was
//! clicked on, since the same header shows on many pages).
//!
//! Every run item is one character except text, which is one per `char`: a tab `\t`, a line break
//! `\n`, a page break `\u{c}`, a symbol its character, a picture or a field `\u{fffc}`.

use super::layout::item_atoms;
use super::model::{Run, RunItem};
use crate::ground::bus::json::Json;
use crate::ground::edit::Pos;
use std::cmp::Ordering;

/// Which flow a place is in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Flow {
    Body,
    /// A header or footer part, by name.
    Part(String),
}

/// A place in a Scroll document.
#[derive(Debug, Clone, PartialEq)]
pub struct SPos {
    pub flow: Flow,
    pub p: usize,
    pub o: usize,
    /// The page a header or footer place was met on.
    pub pg: Option<usize>,
}

impl SPos {
    pub fn body(p: usize, o: usize) -> SPos {
        SPos { flow: Flow::Body, p, o, pg: None }
    }

    pub fn at(&self, o: usize) -> SPos {
        SPos { o, ..self.clone() }
    }

    pub fn para(&self, p: usize, o: usize) -> SPos {
        SPos { p, o, ..self.clone() }
    }

    pub fn from_json(v: &Json) -> Option<SPos> {
        let p = v.get("p")?.as_usize()?;
        let o = v.get("o").and_then(Json::as_usize).unwrap_or(0);
        let flow = match v.get("h").and_then(Json::as_str) {
            Some(h) => Flow::Part(h.to_string()),
            None => Flow::Body,
        };
        let pg = v.get("pg").and_then(Json::as_usize);
        Some(SPos { flow, p, o, pg })
    }

    pub fn from_pos(pos: &Pos) -> Option<SPos> {
        SPos::from_json(pos.json())
    }

    pub fn to_json(&self) -> Json {
        let mut j = Json::obj();
        if let Flow::Part(h) = &self.flow {
            j.set("h", h.as_str());
        }
        j.set("p", self.p);
        j.set("o", self.o);
        if let (Flow::Part(_), Some(pg)) = (&self.flow, self.pg) {
            j.set("pg", pg);
        }
        j
    }

    pub fn to_pos(&self) -> Pos {
        Pos(self.to_json())
    }

    /// Reading order within one flow (`None` across flows).
    pub fn cmp_in_flow(&self, other: &SPos) -> Option<Ordering> {
        if self.flow != other.flow {
            return None;
        }
        Some((self.p, self.o).cmp(&(other.p, other.o)))
    }
}

/// A selection in Scroll's terms, put in reading order.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: SPos,
    pub end: SPos,
    /// The anchor came after the focus.
    pub backward: bool,
}

impl Span {
    pub fn caret(p: SPos) -> Span {
        Span { start: p.clone(), end: p, backward: false }
    }

    /// Two places in order; across flows the selection collapses to the focus.
    pub fn new(anchor: SPos, focus: SPos) -> Span {
        match anchor.cmp_in_flow(&focus) {
            Some(Ordering::Greater) => Span { start: focus, end: anchor, backward: true },
            Some(_) => Span { start: anchor, end: focus, backward: false },
            None => Span::caret(focus),
        }
    }

    pub fn is_caret(&self) -> bool {
        self.start == self.end || (self.start.flow == self.end.flow && self.start.p == self.end.p && self.start.o == self.end.o)
    }

    pub fn flow(&self) -> &Flow {
        &self.start.flow
    }
}

/// What a character is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AKind {
    Text,
    Tab,
    Break,
    PageBreak,
    Sym,
    Pic,
    Field,
}

/// One editing character of a paragraph: which run and item it comes from, and its index inside a
/// text item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Atom {
    pub ch: char,
    pub run: usize,
    pub item: usize,
    pub sub: usize,
    pub kind: AKind,
}

pub const OBJECT: char = '\u{FFFC}';
pub const PAGE_BREAK: char = '\u{C}';

/// A paragraph's characters.
pub fn atoms(runs: &[Run]) -> Vec<Atom> {
    let mut out = Vec::new();
    for (ri, r) in runs.iter().enumerate() {
        for (ii, it) in r.items.iter().enumerate() {
            match it {
                RunItem::Text(t) => {
                    for (k, ch) in t.chars().enumerate() {
                        out.push(Atom { ch, run: ri, item: ii, sub: k, kind: AKind::Text });
                    }
                }
                RunItem::Tab => out.push(Atom { ch: '\t', run: ri, item: ii, sub: 0, kind: AKind::Tab }),
                RunItem::Break => out.push(Atom { ch: '\n', run: ri, item: ii, sub: 0, kind: AKind::Break }),
                RunItem::PageBreak => out.push(Atom { ch: PAGE_BREAK, run: ri, item: ii, sub: 0, kind: AKind::PageBreak }),
                RunItem::Sym(c, _) => out.push(Atom { ch: *c, run: ri, item: ii, sub: 0, kind: AKind::Sym }),
                RunItem::Pic(_) => out.push(Atom { ch: OBJECT, run: ri, item: ii, sub: 0, kind: AKind::Pic }),
                RunItem::Field(_) => out.push(Atom { ch: OBJECT, run: ri, item: ii, sub: 0, kind: AKind::Field }),
            }
        }
    }
    out
}

/// How many characters a paragraph's runs are.
pub fn atom_count(runs: &[Run]) -> usize {
    runs.iter().map(|r| r.items.iter().map(item_atoms).sum::<usize>()).sum()
}

/// The characters as a string (objects as U+FFFC).
pub fn atom_string(atoms: &[Atom]) -> String {
    atoms.iter().map(|a| a.ch).collect()
}

/// A character a word is made of.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\'' || c == '\u{2019}'
}

/// The start of the word before `o` (Ctrl+Left): skip spaces, then the word.
pub fn word_left(chars: &[char], o: usize) -> usize {
    let mut i = o.min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    if i > 0 && is_word_char(chars[i - 1]) {
        while i > 0 && is_word_char(chars[i - 1]) {
            i -= 1;
        }
    } else if i > 0 {
        i -= 1;
    }
    i
}

/// The start of the next word after `o` (Ctrl+Right): the word, then its spaces.
pub fn word_right(chars: &[char], o: usize) -> usize {
    let n = chars.len();
    let mut i = o.min(n);
    if i < n && is_word_char(chars[i]) {
        while i < n && is_word_char(chars[i]) {
            i += 1;
        }
    } else if i < n && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < n && chars[i].is_whitespace() && chars[i] != '\n' {
        i += 1;
    }
    i
}

/// The word around `o`: `(start, end)`, empty when `o` touches no word character.
pub fn word_at(chars: &[char], o: usize) -> (usize, usize) {
    let n = chars.len();
    let o = o.min(n);
    let mut a = o;
    while a > 0 && is_word_char(chars[a - 1]) {
        a -= 1;
    }
    let mut b = o;
    while b < n && is_word_char(chars[b]) {
        b += 1;
    }
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_round_trip_and_order() {
        let p = SPos::body(3, 7);
        assert_eq!(SPos::from_json(&p.to_json()), Some(p.clone()));
        assert_eq!(p.to_json().to_string(), r#"{"p":3,"o":7}"#);
        let h = SPos { flow: Flow::Part("/word/header1.xml".into()), p: 0, o: 2, pg: Some(4) };
        assert_eq!(SPos::from_json(&h.to_json()), Some(h.clone()));
        assert!(p.cmp_in_flow(&h).is_none());
        let s = Span::new(SPos::body(4, 0), SPos::body(3, 9));
        assert!(s.backward);
        assert_eq!(s.start, SPos::body(3, 9));
        assert!(Span::new(h.clone(), p.clone()).is_caret());
    }

    #[test]
    fn word_moves() {
        let c: Vec<char> = "one two,  three".chars().collect();
        assert_eq!(word_right(&c, 0), 4);
        assert_eq!(word_right(&c, 4), 7);
        assert_eq!(word_right(&c, 7), 10);
        assert_eq!(word_left(&c, 10), 7);
        assert_eq!(word_left(&c, 7), 4);
        assert_eq!(word_left(&c, 6), 4);
        assert_eq!(word_at(&c, 5), (4, 7));
        assert_eq!(word_at(&c, 8), (8, 8));
    }
}
