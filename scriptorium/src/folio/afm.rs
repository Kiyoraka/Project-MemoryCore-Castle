//! The standard 14 fonts: their advance widths (from Adobe's AFM files, in 1/1000 em), typed in for
//! ASCII and the common punctuation, and which installed family stands in for each one.

use super::encoding::ascii_index;

/// Widths for ASCII_NAMES order (space .. asciitilde; 39 = quoteright, 96 = quoteleft).
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 222, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278,
    278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 222, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556,
    556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 278, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333,
    333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 278, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611,
    611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

const TIMES_ROMAN: [u16; 95] = [
    250, 333, 408, 500, 500, 833, 778, 333, 333, 333, 500, 564, 250, 333, 250, 278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 278,
    278, 564, 564, 564, 444, 921, 722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611, 889, 722, 722, 556, 722, 667, 556, 611, 722,
    722, 944, 722, 722, 611, 333, 278, 333, 469, 500, 333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, 500, 500, 500,
    500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541,
];

const TIMES_BOLD: [u16; 95] = [
    250, 333, 555, 500, 500, 1000, 833, 333, 333, 333, 500, 570, 250, 333, 250, 278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333,
    333, 570, 570, 570, 500, 930, 722, 667, 722, 722, 667, 611, 778, 778, 389, 500, 778, 667, 944, 722, 778, 611, 778, 722, 556, 667, 722,
    722, 1000, 722, 722, 667, 333, 278, 333, 581, 500, 333, 500, 556, 444, 556, 444, 333, 500, 556, 278, 333, 556, 278, 833, 556, 500, 556,
    556, 444, 389, 333, 556, 500, 722, 500, 500, 444, 394, 220, 394, 520,
];

const TIMES_ITALIC: [u16; 95] = [
    250, 333, 420, 500, 500, 833, 778, 333, 333, 333, 500, 675, 250, 333, 250, 278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333,
    333, 675, 675, 675, 500, 920, 611, 611, 667, 722, 611, 611, 722, 722, 333, 444, 667, 556, 833, 667, 722, 611, 722, 611, 500, 556, 722,
    611, 833, 611, 556, 556, 389, 278, 389, 422, 500, 333, 500, 500, 444, 500, 444, 278, 500, 500, 278, 278, 444, 278, 722, 500, 500, 500,
    500, 389, 389, 278, 500, 444, 667, 444, 444, 389, 400, 275, 400, 541,
];

const TIMES_BOLD_ITALIC: [u16; 95] = [
    250, 389, 555, 500, 500, 833, 778, 333, 333, 333, 500, 570, 250, 333, 250, 278, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333,
    333, 570, 570, 570, 500, 832, 667, 667, 667, 722, 667, 667, 722, 778, 389, 500, 667, 611, 889, 722, 722, 611, 722, 667, 556, 611, 722,
    667, 889, 667, 611, 611, 333, 278, 333, 570, 500, 333, 500, 500, 444, 500, 444, 333, 500, 556, 278, 278, 500, 278, 778, 556, 500, 500,
    500, 389, 389, 278, 556, 444, 667, 500, 444, 389, 348, 220, 348, 570,
];

/// Common non-ASCII glyphs: quotesingle, grave, bullet, endash, emdash, quotedblleft, quotedblright,
/// quotesinglbase, quotedblbase, ellipsis, copyright, registered, degree, trademark.
const EXTRA_NAMES: [&str; 14] = [
    "quotesingle", "grave", "bullet", "endash", "emdash", "quotedblleft", "quotedblright", "quotesinglbase", "quotedblbase", "ellipsis",
    "copyright", "registered", "degree", "trademark",
];
const EXTRA_HELVETICA: [u16; 14] = [191, 333, 350, 556, 1000, 333, 333, 222, 333, 1000, 737, 737, 400, 1000];
const EXTRA_HELVETICA_BOLD: [u16; 14] = [238, 333, 350, 556, 1000, 500, 500, 278, 500, 1000, 737, 737, 400, 1000];
const EXTRA_TIMES: [u16; 14] = [180, 333, 350, 500, 1000, 444, 444, 333, 444, 1000, 760, 760, 400, 980];
const EXTRA_TIMES_BOLD: [u16; 14] = [278, 333, 350, 500, 1000, 500, 500, 333, 500, 1000, 747, 747, 400, 1000];
const EXTRA_TIMES_ITALIC: [u16; 14] = [214, 333, 350, 500, 889, 556, 556, 333, 556, 889, 760, 760, 400, 980];
const EXTRA_TIMES_BOLD_ITALIC: [u16; 14] = [278, 333, 350, 500, 1000, 500, 500, 333, 500, 1000, 747, 747, 400, 1000];

/// One of the standard 14 faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Std14 {
    Helvetica { bold: bool, italic: bool },
    Times { bold: bool, italic: bool },
    Courier { bold: bool, italic: bool },
    Symbol,
    ZapfDingbats,
}

impl Std14 {
    /// Recognise a standard-14 name and its common aliases (Arial, TimesNewRoman, CourierNew...).
    pub fn from_name(base: &str) -> Option<Std14> {
        let n = base.to_ascii_lowercase().replace([' ', '-', ',', '_'], "");
        let bold = n.contains("bold") || n.contains("black") || n.contains("heavy");
        let italic = n.contains("italic") || n.contains("oblique");
        if n.starts_with("helvetica") || n.starts_with("arial") {
            return Some(Std14::Helvetica { bold, italic });
        }
        if n.starts_with("times") {
            return Some(Std14::Times { bold, italic });
        }
        if n.starts_with("courier") {
            return Some(Std14::Courier { bold, italic });
        }
        if n.starts_with("symbol") {
            return Some(Std14::Symbol);
        }
        if n.starts_with("zapfdingbats") || n.starts_with("dingbats") {
            return Some(Std14::ZapfDingbats);
        }
        None
    }

    pub fn is_symbolic(&self) -> bool {
        matches!(self, Std14::Symbol | Std14::ZapfDingbats)
    }

    /// The installed families to try, best first.
    pub fn families(&self) -> &'static [&'static str] {
        match self {
            Std14::Helvetica { .. } => &["Arial", "Liberation Sans", "Helvetica", "Nimbus Sans", "DejaVu Sans"],
            Std14::Times { .. } => &["Times New Roman", "Liberation Serif", "Times", "Nimbus Roman", "DejaVu Serif"],
            Std14::Courier { .. } => &["Courier New", "Liberation Mono", "Courier", "Nimbus Mono PS", "DejaVu Sans Mono"],
            Std14::Symbol => &["Symbol", "Segoe UI Symbol", "DejaVu Sans"],
            Std14::ZapfDingbats => &["Segoe UI Symbol", "DejaVu Sans"],
        }
    }

    /// The advance of a glyph (by name) in 1/1000 em.
    pub fn width(&self, glyph: &str) -> f64 {
        let (table, extra, default): (&[u16; 95], &[u16; 14], u16) = match *self {
            Std14::Courier { .. } => return 600.0,
            Std14::Symbol => return if glyph == "space" { 250.0 } else { 600.0 },
            Std14::ZapfDingbats => return if glyph == "space" { 278.0 } else { 788.0 },
            Std14::Helvetica { bold: false, .. } => (&HELVETICA, &EXTRA_HELVETICA, 556),
            Std14::Helvetica { bold: true, .. } => (&HELVETICA_BOLD, &EXTRA_HELVETICA_BOLD, 556),
            Std14::Times { bold: false, italic: false } => (&TIMES_ROMAN, &EXTRA_TIMES, 500),
            Std14::Times { bold: true, italic: false } => (&TIMES_BOLD, &EXTRA_TIMES_BOLD, 500),
            Std14::Times { bold: false, italic: true } => (&TIMES_ITALIC, &EXTRA_TIMES_ITALIC, 500),
            Std14::Times { bold: true, italic: true } => (&TIMES_BOLD_ITALIC, &EXTRA_TIMES_BOLD_ITALIC, 500),
        };
        if let Some(i) = ascii_index(glyph) {
            return table[i] as f64;
        }
        if let Some(i) = EXTRA_NAMES.iter().position(|n| *n == glyph) {
            return extra[i] as f64;
        }
        // Accented letters take their base letter's width.
        for suffix in ["acute", "grave", "dieresis", "circumflex", "tilde", "ring", "cedilla", "caron", "slash"] {
            if let Some(base) = glyph.strip_suffix(suffix) {
                if base.len() == 1 {
                    if let Some(i) = ascii_index(base) {
                        return table[i] as f64;
                    }
                }
            }
        }
        default as f64
    }
}
