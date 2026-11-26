//! The text engine: from styled runs to placed glyphs.
//!
//! - [`tables`]: our own Unicode property tables (grapheme break, line break, East Asian Width,
//!   Extended_Pictographic, a coarse General Category).
//! - Segmentation: [`graphemes`] (UAX #29) and [`line_break_opportunities`] (UAX #14 with the
//!   Office kinsoku tailoring, configurable through [`BreakOptions`]).
//! - Shaping-lite: [`FontCache`] maps a run of text in a [`TextStyle`] to glyphs: cmap per grapheme,
//!   font fallback per character (split into one [`ShapedRun`] per font), advances in points, pair
//!   kerning, letter spacing.
//! - Paragraph layout: [`layout_paragraph`] fills lines greedily over break opportunities, with tabs,
//!   indents, alignment (incl. Justify and Distribute), line spacing and trailing spaces hanging. It
//!   is generic over a [`Measurer`], so it runs on real fonts ([`FontCache`]) or on any stub.
//! - Output: [`ParagraphLayout::to_items`] gives display-list items, [`FontCache::fill_resources`]
//!   the outlines they need; [`ParagraphLayout::hit_test`] and [`ParagraphLayout::caret_rect`] serve
//!   the editors' cursors.
//!
//! Units: points; layout coordinates are y down with the origin at the paragraph's top-left.
//!
//! Scope and extension points: Latin, Greek, Cyrillic and CJK are laid out fully. Not yet done, by
//! design for a later milestone:
//! - Arabic/Jawi shaping (contextual forms, lam-alef) needs GSUB, which the font module does not
//!   read yet; it belongs in `shape.rs` where a grapheme run is mapped to glyphs (see the
//!   `EXTENSION (complex shaping)` note there).
//! - Right-to-left: the bidi algorithm (UAX #9) would assign levels per run before shaping and
//!   reorder each line's clusters visually after line breaking; `layout.rs` marks that spot
//!   (`EXTENSION (bidi)`). Today every line is laid out left to right in logical order.
//! - No GPOS mark positioning (combining marks use their own advance, usually zero, placed after
//!   the base), no GSUB ligatures (emoji ZWJ sequences show their parts, ligatures like "fi" are not
//!   formed), no hyphenation dictionary (soft hyphens U+00AD are honoured), no hanging punctuation.

mod layout;
mod segment;
mod shape;
pub mod tables;
#[cfg(test)]
mod tests;

pub use layout::{
    caret_rect, hit_test, layout_paragraph, Align, GlyphSpan, Line, LineCluster, LineSpacing, ParaOptions,
    ParagraphLayout, TabKind, TabLeader, TabStop,
};
pub use segment::{
    graphemes, is_grapheme_boundary, line_break_opportunities, line_break_opportunities_with, BreakKind,
    BreakOptions, Graphemes, KINSOKU_NO_END, KINSOKU_NO_START,
};
pub use shape::FontCache;

use super::display::Color;
use super::font::FontRef;
use std::ops::Range;

/// How a run of text looks.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// Font family as a user picks it ("Arial", "Yu Gothic").
    pub family: String,
    /// The family for East Asian characters (Word's `w:eastAsia` font); empty for none. Tried
    /// before `family` when a character needs a fallback.
    pub east_asian_family: String,
    pub size_pt: f64,
    /// 100..=900 (400 regular, 700 bold).
    pub weight: u16,
    pub italic: bool,
    pub color: Color,
    /// Extra space after every grapheme, in points (Word's character spacing; may be negative).
    pub letter_spacing: f64,
    pub underline: bool,
    pub strike: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            family: String::new(),
            east_asian_family: String::new(),
            size_pt: 11.0,
            weight: 400,
            italic: false,
            color: Color::BLACK,
            letter_spacing: 0.0,
            underline: false,
            strike: false,
        }
    }
}

impl TextStyle {
    pub fn new(family: &str, size_pt: f64) -> TextStyle {
        TextStyle { family: family.to_string(), size_pt, ..TextStyle::default() }
    }

    pub fn bold(mut self) -> TextStyle {
        self.weight = 700;
        self
    }

    pub fn italic(mut self) -> TextStyle {
        self.italic = true;
        self
    }
}

/// A piece of a paragraph's text in one style.
#[derive(Debug, Clone, PartialEq)]
pub struct StyledRun {
    pub text: String,
    pub style: TextStyle,
}

impl StyledRun {
    pub fn new(text: &str, style: TextStyle) -> StyledRun {
        StyledRun { text: text.to_string(), style }
    }
}

/// A font's vertical metrics scaled to a size, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
    /// Above the baseline (positive).
    pub ascent: f64,
    /// Below the baseline (positive).
    pub descent: f64,
    pub line_gap: f64,
    /// Top edge of the underline stroke, from the baseline, y down (positive: below the baseline).
    pub underline_y: f64,
    pub underline_thickness: f64,
    /// Top edge of the strikethrough stroke, from the baseline, y down (negative: above).
    pub strike_y: f64,
    pub strike_thickness: f64,
}

impl FontMetrics {
    /// Made-up metrics for a size when no font is available (0.8 em up, 0.2 em down).
    pub fn synthetic(size: f64) -> FontMetrics {
        FontMetrics {
            ascent: 0.8 * size,
            descent: 0.2 * size,
            line_gap: 0.0,
            underline_y: 0.1 * size,
            underline_thickness: 0.05 * size,
            strike_y: -0.3 * size,
            strike_thickness: 0.05 * size,
        }
    }

    /// Ascent + descent + line gap: the font's own line spacing (Office "single").
    pub fn line_height(&self) -> f64 {
        self.ascent + self.descent + self.line_gap
    }
}

/// One glyph of a shaped run.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedGlyph {
    pub gid: u16,
    /// Pen advance in points after this glyph, kerning with the next glyph and letter spacing
    /// included.
    pub advance: f64,
    /// The grapheme cluster this glyph belongs to: byte range in the text that was shaped.
    pub cluster: Range<usize>,
}

/// Glyphs of one font at one size.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedRun {
    /// The font's id in its [`FontCache`] (the id the display list uses); 0 means no font was
    /// available (glyph 0 at a synthetic advance).
    pub font: u32,
    /// Where the font lives, when it came from the system index.
    pub font_ref: Option<FontRef>,
    pub size: f64,
    pub glyphs: Vec<ShapedGlyph>,
    /// Sum of the advances.
    pub width: f64,
    pub metrics: FontMetrics,
}

/// What paragraph layout needs from fonts: shaping and metrics. [`FontCache`] is the real one;
/// tests use stubs.
pub trait Measurer {
    /// Shape `text` (no tabs or line breaks: layout handles those) in `style`. Glyph clusters are
    /// byte ranges of `text`; consecutive runs cover the text in order.
    fn shape(&mut self, text: &str, style: &TextStyle) -> Vec<ShapedRun>;

    /// The metrics of the style's primary font (for empty lines, tabs and line ends).
    fn font_metrics(&mut self, style: &TextStyle) -> FontMetrics;

    /// The advance width of `text` in `style`, in points.
    fn measure(&mut self, text: &str, style: &TextStyle) -> f64 {
        self.shape(text, style).iter().map(|r| r.width).sum()
    }
}
