//! Run and paragraph properties as WordprocessingML writes them (`w:rPr`, `w:pPr`), read into
//! "maybe set" values so the style chain can lay them over one another: docDefaults, table style,
//! paragraph style chain, numbering level, character style chain, direct formatting.
//!
//! Units on the way in: twips (1/20 pt) for indents, spacing and tabs; half-points for font sizes;
//! eighths of a point for borders. Everything is stored in points.

use crate::ground::display::Color;
use crate::ground::draw::{canonical_scheme, ColorMap, Theme};
use crate::ground::text::{Align, LineSpacing, TabKind, TabLeader};
use crate::ground::xml::Element;

/// What colour resolution needs: the document's theme and its `w:clrSchemeMapping`.
pub struct Ctx<'a> {
    pub theme: &'a Theme,
    pub cmap: &'a ColorMap,
}

// ---------------------------------------------------------------------------------------------------
// Attribute helpers
// ---------------------------------------------------------------------------------------------------

/// The attribute whose local name is `local` (`w:val`, `val`, `r:id` all answer to their local part).
pub fn attr<'e>(el: &'e Element, local: &str) -> Option<&'e str> {
    el.attrs
        .iter()
        .find(|(k, _)| {
            if k.starts_with("xmlns") {
                return false;
            }
            match k.rfind(':') {
                Some(i) => &k[i + 1..] == local,
                None => k == local,
            }
        })
        .map(|(_, v)| v.as_str())
}

/// `w:val` of an element.
pub fn val(el: &Element) -> Option<&str> {
    attr(el, "val")
}

/// A number attribute (twips, half-points, EMU...). Accepts decimals and a trailing unit-less sign.
pub fn num_attr(el: &Element, local: &str) -> Option<f64> {
    let v = attr(el, local)?.trim();
    parse_measure(v)
}

/// A measure as written: a plain number (in the attribute's own unit) or a universal measure with a
/// unit (`12pt`, `1in`, `2.5cm`, `10mm`, `3pc`, `4pi`), which is converted to twips.
pub fn parse_measure(v: &str) -> Option<f64> {
    let v = v.trim();
    if let Ok(n) = v.parse::<f64>() {
        return Some(n);
    }
    let units: [(&str, f64); 6] = [("pt", 20.0), ("in", 1440.0), ("cm", 566.929), ("mm", 56.6929), ("pc", 240.0), ("pi", 240.0)];
    for (u, f) in units {
        if let Some(n) = v.strip_suffix(u) {
            return n.trim().parse::<f64>().ok().map(|x| x * f);
        }
    }
    None
}

/// Twips attribute in points.
pub fn twips_attr(el: &Element, local: &str) -> Option<f64> {
    num_attr(el, local).map(|t| t / 20.0)
}

/// An on/off property: absent `w:val` means on; `0`, `false`, `off`, `none` mean off.
pub fn on_off(el: &Element) -> bool {
    match val(el) {
        None => true,
        Some(v) => !matches!(v.trim(), "0" | "false" | "off" | "none"),
    }
}

fn hex_color(v: &str) -> Option<Color> {
    let v = v.trim().trim_start_matches('#');
    if v.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(v, 16).ok()?;
    Some(Color::rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

/// A WordprocessingML colour: `w:val` hex or `auto`, overridden by `w:themeColor` with
/// `w:themeTint` / `w:themeShade` (hex bytes). `fill_attr` picks the attribute (`val`, `fill`,
/// `color`) and the theme attribute family (`themeColor` / `themeFill`).
pub fn word_color(el: &Element, ctx: &Ctx, value_attr: &str, theme_attr: &str, tint_attr: &str, shade_attr: &str) -> Option<Color> {
    if let Some(t) = attr(el, theme_attr) {
        let name = canonical_scheme(t.trim());
        let mut c = ctx.theme.scheme_color(name, ctx.cmap);
        if let Some(s) = attr(el, shade_attr).and_then(|s| u8::from_str_radix(s.trim(), 16).ok()) {
            let f = s as f64 / 255.0;
            let ch = |x: u8| ((x as f64) * f).round().clamp(0.0, 255.0) as u8;
            c = Color::rgb(ch(c.r), ch(c.g), ch(c.b));
        }
        if let Some(s) = attr(el, tint_attr).and_then(|s| u8::from_str_radix(s.trim(), 16).ok()) {
            let f = s as f64 / 255.0;
            let ch = |x: u8| ((x as f64) * f + 255.0 * (1.0 - f)).round().clamp(0.0, 255.0) as u8;
            c = Color::rgb(ch(c.r), ch(c.g), ch(c.b));
        }
        return Some(c);
    }
    let v = attr(el, value_attr)?;
    if v.trim() == "auto" {
        return None;
    }
    hex_color(v)
}

/// The sixteen highlight colours (`w:highlight`).
pub fn highlight_color(name: &str) -> Option<Color> {
    let hex = match name.trim() {
        "black" => 0x000000,
        "blue" => 0x0000FF,
        "cyan" => 0x00FFFF,
        "green" => 0x00FF00,
        "magenta" => 0xFF00FF,
        "red" => 0xFF0000,
        "yellow" => 0xFFFF00,
        "white" => 0xFFFFFF,
        "darkBlue" => 0x000080,
        "darkCyan" => 0x008080,
        "darkGreen" => 0x008000,
        "darkMagenta" => 0x800080,
        "darkRed" => 0x800000,
        "darkYellow" => 0x808000,
        "darkGray" => 0x808080,
        "lightGray" => 0xC0C0C0,
        _ => return None,
    };
    Some(Color::rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8))
}

// ---------------------------------------------------------------------------------------------------
// Run properties
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertAlign {
    Baseline,
    Super,
    Sub,
}

/// `w:rPr`, every field "unset" until a level of the hierarchy sets it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunProps {
    /// `w:rStyle` (character style id).
    pub style: Option<String>,
    /// Latin family (`w:ascii` / `w:hAnsi`, theme fonts resolved).
    pub font: Option<String>,
    /// East Asian family (`w:eastAsia`).
    pub font_ea: Option<String>,
    /// Points.
    pub size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// `Some(None)` is `auto` (black on a white page).
    pub color: Option<Option<Color>>,
    /// `Some(None)` is explicitly no highlight.
    pub highlight: Option<Option<Color>>,
    /// Run shading (`w:shd w:fill`), drawn like a highlight when there is none.
    pub shade: Option<Option<Color>>,
    pub caps: Option<bool>,
    pub small_caps: Option<bool>,
    pub vert: Option<VertAlign>,
    /// Character spacing in points.
    pub spacing: Option<f64>,
    pub hidden: Option<bool>,
}

fn theme_font(theme: &Theme, name: &str) -> Option<String> {
    let f = match name.trim() {
        "majorHAnsi" | "majorAscii" => &theme.major_font.latin,
        "minorHAnsi" | "minorAscii" => &theme.minor_font.latin,
        "majorEastAsia" => &theme.major_font.ea,
        "minorEastAsia" => &theme.minor_font.ea,
        "majorBidi" => &theme.major_font.cs,
        "minorBidi" => &theme.minor_font.cs,
        _ => return None,
    };
    if f.is_empty() {
        None
    } else {
        Some(f.clone())
    }
}

impl RunProps {
    /// Read a `w:rPr` element.
    pub fn parse(el: &Element, ctx: &Ctx) -> RunProps {
        let mut p = RunProps::default();
        for c in el.elements() {
            match c.local() {
                "rStyle" => p.style = val(c).map(str::to_string),
                "rFonts" => {
                    let latin = attr(c, "asciiTheme")
                        .and_then(|t| theme_font(ctx.theme, t))
                        .or_else(|| attr(c, "hAnsiTheme").and_then(|t| theme_font(ctx.theme, t)))
                        .or_else(|| attr(c, "ascii").map(str::to_string))
                        .or_else(|| attr(c, "hAnsi").map(str::to_string));
                    if latin.is_some() {
                        p.font = latin;
                    }
                    let ea = attr(c, "eastAsiaTheme")
                        .and_then(|t| theme_font(ctx.theme, t))
                        .or_else(|| attr(c, "eastAsia").map(str::to_string));
                    if ea.is_some() {
                        p.font_ea = ea;
                    }
                }
                "sz" => p.size = num_attr(c, "val").map(|h| h / 2.0),
                "b" => p.bold = Some(on_off(c)),
                "i" => p.italic = Some(on_off(c)),
                "u" => p.underline = Some(!matches!(val(c).map(str::trim), Some("none") | Some("0") | Some("false"))),
                "strike" | "dstrike" => {
                    let on = on_off(c);
                    if on || p.strike.is_none() {
                        p.strike = Some(on);
                    }
                }
                "color" => p.color = Some(word_color(c, ctx, "val", "themeColor", "themeTint", "themeShade")),
                "highlight" => p.highlight = Some(val(c).and_then(highlight_color)),
                "shd" => p.shade = Some(word_color(c, ctx, "fill", "themeFill", "themeFillTint", "themeFillShade")),
                "caps" => p.caps = Some(on_off(c)),
                "smallCaps" => p.small_caps = Some(on_off(c)),
                "vertAlign" => {
                    p.vert = Some(match val(c).map(str::trim) {
                        Some("superscript") => VertAlign::Super,
                        Some("subscript") => VertAlign::Sub,
                        _ => VertAlign::Baseline,
                    })
                }
                "spacing" => p.spacing = twips_attr(c, "val"),
                "vanish" => p.hidden = Some(on_off(c)),
                _ => {}
            }
        }
        p
    }

    /// Lay `top` over `self`: every field `top` sets wins.
    pub fn over(&mut self, top: &RunProps) {
        macro_rules! take {
            ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f.clone(); } )* };
        }
        take!(style, font, font_ea, size, bold, italic, underline, strike, color, highlight, shade, caps, small_caps, vert, spacing, hidden);
    }

    /// Lay a character style's properties over the paragraph level: the toggle properties (bold,
    /// italic, caps, small caps, strike) flip when both levels turn them on (ECMA-376 17.7.3),
    /// everything else simply wins.
    pub fn over_toggling(&mut self, top: &RunProps) {
        let flips = |base: Option<bool>, t: Option<bool>| -> Option<bool> {
            match (base, t) {
                (Some(true), Some(true)) => Some(false),
                (b, None) => b,
                (_, t) => t,
            }
        };
        let bold = flips(self.bold, top.bold);
        let italic = flips(self.italic, top.italic);
        let caps = flips(self.caps, top.caps);
        let small_caps = flips(self.small_caps, top.small_caps);
        let strike = flips(self.strike, top.strike);
        self.over(top);
        self.bold = bold;
        self.italic = italic;
        self.caps = caps;
        self.small_caps = small_caps;
        self.strike = strike;
    }
}

// ---------------------------------------------------------------------------------------------------
// Paragraph properties
// ---------------------------------------------------------------------------------------------------

/// One `w:tab` of `w:tabs` (position in points from the text column's left edge).
#[derive(Debug, Clone, PartialEq)]
pub struct TabDef {
    pub pos: f64,
    pub kind: TabKind,
    pub leader: TabLeader,
    /// `w:val="clear"`: removes an inherited stop at this position.
    pub clear: bool,
}

/// `w:pPr`, every field unset until a level sets it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParaProps {
    /// `w:pStyle`.
    pub style: Option<String>,
    pub align: Option<Align>,
    pub ind_left: Option<f64>,
    pub ind_right: Option<f64>,
    /// First-line indent (negative for a hanging indent).
    pub ind_first: Option<f64>,
    pub before: Option<f64>,
    pub after: Option<f64>,
    pub line: Option<LineSpacing>,
    pub tabs: Vec<TabDef>,
    pub num_id: Option<i64>,
    pub ilvl: Option<u8>,
    pub page_break_before: Option<bool>,
    pub outline: Option<u8>,
    /// Paragraph shading fill.
    pub shade: Option<Option<Color>>,
}

impl ParaProps {
    /// Read a `w:pPr` element (its `w:rPr` and `w:sectPr` are read by the caller).
    pub fn parse(el: &Element, ctx: &Ctx) -> ParaProps {
        let mut p = ParaProps::default();
        for c in el.elements() {
            match c.local() {
                "pStyle" => p.style = val(c).map(str::to_string),
                "jc" => {
                    p.align = match val(c).map(str::trim) {
                        Some("center") => Some(Align::Center),
                        Some("right") | Some("end") => Some(Align::Right),
                        Some("both") | Some("lowKashida") | Some("mediumKashida") | Some("highKashida") => Some(Align::Justify),
                        Some("distribute") | Some("thaiDistribute") => Some(Align::Distribute),
                        Some(_) => Some(Align::Left),
                        None => None,
                    }
                }
                "ind" => {
                    if let Some(v) = twips_attr(c, "left").or_else(|| twips_attr(c, "start")) {
                        p.ind_left = Some(v);
                    }
                    if let Some(v) = twips_attr(c, "right").or_else(|| twips_attr(c, "end")) {
                        p.ind_right = Some(v);
                    }
                    if let Some(v) = twips_attr(c, "hanging") {
                        p.ind_first = Some(-v);
                    } else if let Some(v) = twips_attr(c, "firstLine") {
                        p.ind_first = Some(v);
                    }
                }
                "spacing" => {
                    if attr(c, "beforeAutospacing").is_some_and(|v| v != "0" && v != "false") {
                        p.before = Some(14.0);
                    } else if let Some(v) = twips_attr(c, "before") {
                        p.before = Some(v);
                    }
                    if attr(c, "afterAutospacing").is_some_and(|v| v != "0" && v != "false") {
                        p.after = Some(14.0);
                    } else if let Some(v) = twips_attr(c, "after") {
                        p.after = Some(v);
                    }
                    if let Some(line) = num_attr(c, "line") {
                        p.line = Some(match attr(c, "lineRule").map(str::trim) {
                            Some("exact") => LineSpacing::Exactly((line / 20.0).abs()),
                            Some("atLeast") => LineSpacing::AtLeast(line / 20.0),
                            _ => {
                                let f = line / 240.0;
                                if (f - 1.0).abs() < 1e-9 {
                                    LineSpacing::Single
                                } else {
                                    LineSpacing::Multiple(f)
                                }
                            }
                        });
                    }
                }
                "tabs" => {
                    for t in c.children_named("tab") {
                        let Some(pos) = twips_attr(t, "pos") else { continue };
                        let v = val(t).unwrap_or("left").trim();
                        let kind = match v {
                            "center" => TabKind::Center,
                            "right" | "end" => TabKind::Right,
                            "decimal" => TabKind::Decimal,
                            _ => TabKind::Left,
                        };
                        let leader = match attr(t, "leader").map(str::trim) {
                            Some("dot") => TabLeader::Dots,
                            Some("hyphen") => TabLeader::Hyphens,
                            Some("underscore") | Some("heavy") => TabLeader::Underline,
                            Some("middleDot") => TabLeader::MiddleDots,
                            _ => TabLeader::None,
                        };
                        // A bar tab draws a line, not a stop; "num" is a list tab and acts as a stop.
                        if v == "bar" {
                            continue;
                        }
                        p.tabs.push(TabDef { pos, kind, leader, clear: v == "clear" });
                    }
                }
                "numPr" => {
                    if let Some(n) = c.child("numId").and_then(|e| num_attr(e, "val")) {
                        p.num_id = Some(n as i64);
                    }
                    if let Some(n) = c.child("ilvl").and_then(|e| num_attr(e, "val")) {
                        p.ilvl = Some(n.clamp(0.0, 8.0) as u8);
                    }
                }
                "pageBreakBefore" => p.page_break_before = Some(on_off(c)),
                "outlineLvl" => p.outline = num_attr(c, "val").map(|v| v.clamp(0.0, 9.0) as u8),
                "shd" => p.shade = Some(word_color(c, ctx, "fill", "themeFill", "themeFillTint", "themeFillShade")),
                _ => {}
            }
        }
        p
    }

    /// Lay `top` over `self`. Tab stops accumulate: `top`'s stops replace inherited ones at the same
    /// position and a `clear` removes one.
    pub fn over(&mut self, top: &ParaProps) {
        macro_rules! take {
            ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f.clone(); } )* };
        }
        take!(style, align, ind_left, ind_right, ind_first, before, after, line, num_id, ilvl, page_break_before, outline, shade);
        for t in &top.tabs {
            self.tabs.retain(|s| (s.pos - t.pos).abs() > 0.05);
            if !t.clear {
                self.tabs.push(t.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Borders
// ---------------------------------------------------------------------------------------------------

/// One border line. `none` is an explicit "no border" (`w:val="nil"` / `"none"`), which still
/// overrides an inherited one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    pub width: f64,
    pub color: Color,
    pub none: bool,
}

impl Border {
    pub fn parse(el: &Element, ctx: &Ctx) -> Border {
        let v = val(el).unwrap_or("single").trim();
        let none = matches!(v, "nil" | "none");
        let width = num_attr(el, "sz").map(|s| s / 8.0).unwrap_or(0.5).max(0.25);
        let width = if v == "double" { width * 3.0 } else { width };
        let color = word_color(el, ctx, "color", "themeColor", "themeTint", "themeShade").unwrap_or(Color::BLACK);
        Border { width, color, none }
    }

    /// A border that draws something.
    pub fn visible(b: Option<Border>) -> Option<Border> {
        b.filter(|b| !b.none)
    }
}

/// The six sides a table (or a cell) can set.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Borders {
    pub top: Option<Border>,
    pub left: Option<Border>,
    pub bottom: Option<Border>,
    pub right: Option<Border>,
    pub inside_h: Option<Border>,
    pub inside_v: Option<Border>,
}

impl Borders {
    pub fn parse(el: &Element, ctx: &Ctx) -> Borders {
        let mut b = Borders::default();
        for c in el.elements() {
            let border = Some(Border::parse(c, ctx));
            match c.local() {
                "top" => b.top = border,
                "left" | "start" => b.left = border,
                "bottom" => b.bottom = border,
                "right" | "end" => b.right = border,
                "insideH" => b.inside_h = border,
                "insideV" => b.inside_v = border,
                _ => {}
            }
        }
        b
    }

    pub fn over(&mut self, top: &Borders) {
        macro_rules! take {
            ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f; } )* };
        }
        take!(top, left, bottom, right, inside_h, inside_v);
    }
}

/// Cell margins (`w:tblCellMar` / `w:tcMar`) in points: top, left, bottom, right.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Margins {
    pub top: Option<f64>,
    pub left: Option<f64>,
    pub bottom: Option<f64>,
    pub right: Option<f64>,
}

impl Margins {
    pub fn parse(el: &Element) -> Margins {
        let mut m = Margins::default();
        for c in el.elements() {
            // Only fixed widths (dxa or no type) are understood.
            if attr(c, "type").is_some_and(|t| t != "dxa") {
                continue;
            }
            let w = twips_attr(c, "w");
            match c.local() {
                "top" => m.top = w,
                "left" | "start" => m.left = w,
                "bottom" => m.bottom = w,
                "right" | "end" => m.right = w,
                _ => {}
            }
        }
        m
    }

    pub fn over(&mut self, top: &Margins) {
        if top.top.is_some() {
            self.top = top.top;
        }
        if top.left.is_some() {
            self.left = top.left;
        }
        if top.bottom.is_some() {
            self.bottom = top.bottom;
        }
        if top.right.is_some() {
            self.right = top.right;
        }
    }
}
