//! `xl/styles.xml`: number formats, fonts, fills, borders and the cell formats (`cellXfs`) a
//! cell's `s` points at, with colours resolved through the theme (theme index + tint), the legacy
//! indexed palette (or the file's own `indexedColors`) and ARGB.
//!
//! Not yet: `cellStyleXfs` inheritance beyond what `cellXfs` already carries, `dxfs` (conditional
//! formats and tables), gradient fills (their first stop is used), diagonal borders.

use std::collections::HashMap;

use super::numfmt::{builtin_format, hex_color, INDEXED};
use crate::ground::display::Color;
use crate::ground::draw::Theme;
use crate::ground::xml::{parse_dom, Element};

#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    pub name: String,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<Color>,
}

impl Default for Font {
    fn default() -> Font {
        Font { name: "Calibri".into(), size: 11.0, bold: false, italic: false, underline: false, strike: false, color: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    Thin,
    Medium,
    Thick,
    Dashed,
    Dotted,
    Hair,
    Double,
    MediumDashed,
    DashDot,
    MediumDashDot,
    DashDotDot,
    MediumDashDotDot,
    SlantDashDot,
}

impl BorderStyle {
    pub fn parse(s: &str) -> Option<BorderStyle> {
        Some(match s {
            "thin" => BorderStyle::Thin,
            "medium" => BorderStyle::Medium,
            "thick" => BorderStyle::Thick,
            "dashed" => BorderStyle::Dashed,
            "dotted" => BorderStyle::Dotted,
            "hair" => BorderStyle::Hair,
            "double" => BorderStyle::Double,
            "mediumDashed" => BorderStyle::MediumDashed,
            "dashDot" => BorderStyle::DashDot,
            "mediumDashDot" => BorderStyle::MediumDashDot,
            "dashDotDot" => BorderStyle::DashDotDot,
            "mediumDashDotDot" => BorderStyle::MediumDashDotDot,
            "slantDashDot" => BorderStyle::SlantDashDot,
            _ => return None,
        })
    }

    /// Line width in points (1 px = 0.75 pt).
    pub fn width(self) -> f64 {
        match self {
            BorderStyle::Hair | BorderStyle::Thin | BorderStyle::Dashed | BorderStyle::Dotted | BorderStyle::DashDot | BorderStyle::DashDotDot => 0.75,
            BorderStyle::Thick | BorderStyle::Double => 2.25,
            _ => 1.5,
        }
    }

    /// The dash pattern in points (empty: solid).
    pub fn dash(self) -> Vec<f64> {
        match self {
            BorderStyle::Dashed | BorderStyle::MediumDashed => vec![3.0, 1.5],
            BorderStyle::Dotted => vec![0.75, 0.75],
            BorderStyle::Hair => vec![0.75, 0.75],
            BorderStyle::DashDot | BorderStyle::MediumDashDot | BorderStyle::SlantDashDot => vec![3.0, 1.5, 0.75, 1.5],
            BorderStyle::DashDotDot | BorderStyle::MediumDashDotDot => vec![3.0, 1.5, 0.75, 1.5, 0.75, 1.5],
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderSide {
    pub style: BorderStyle,
    pub color: Color,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Border {
    pub left: Option<BorderSide>,
    pub right: Option<BorderSide>,
    pub top: Option<BorderSide>,
    pub bottom: Option<BorderSide>,
}

impl Border {
    pub fn is_empty(&self) -> bool {
        self.left.is_none() && self.right.is_none() && self.top.is_none() && self.bottom.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    General,
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterContinuous,
    Distributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
    Justify,
    Distributed,
}

/// One `cellXfs` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Xf {
    pub num_fmt: u32,
    pub font: usize,
    pub fill: usize,
    pub border: usize,
    pub h_align: HAlign,
    pub v_align: VAlign,
    pub wrap: bool,
    pub indent: u32,
    pub rotation: i32,
    pub shrink: bool,
}

impl Default for Xf {
    fn default() -> Xf {
        Xf { num_fmt: 0, font: 0, fill: 0, border: 0, h_align: HAlign::General, v_align: VAlign::Bottom, wrap: false, indent: 0, rotation: 0, shrink: false }
    }
}

/// The parsed styles part.
#[derive(Debug, Clone)]
pub struct Styles {
    pub num_fmts: HashMap<u32, String>,
    pub fonts: Vec<Font>,
    /// Each fill's paint colour (`None`: no fill).
    pub fills: Vec<Option<Color>>,
    pub borders: Vec<Border>,
    pub xfs: Vec<Xf>,
}

impl Default for Styles {
    fn default() -> Styles {
        Styles { num_fmts: HashMap::new(), fonts: vec![Font::default()], fills: vec![None, None], borders: vec![Border::default()], xfs: vec![Xf::default()] }
    }
}

/// The colours a style colour can name: theme, palette.
pub struct ColorContext<'a> {
    pub theme: &'a Theme,
    pub palette: Vec<Color>,
}

/// SpreadsheetML's theme colour order: lt1, dk1, lt2, dk2, accents, hyperlinks.
fn theme_slot(theme: &Theme, idx: u32) -> Color {
    let c = &theme.colors;
    match idx {
        0 => c.lt1,
        1 => c.dk1,
        2 => c.lt2,
        3 => c.dk2,
        4 => c.accent1,
        5 => c.accent2,
        6 => c.accent3,
        7 => c.accent4,
        8 => c.accent5,
        9 => c.accent6,
        10 => c.hlink,
        11 => c.fol_hlink,
        _ => Color::BLACK,
    }
}

fn rgb_to_hls(c: Color) -> (f64, f64, f64) {
    let (r, g, b) = (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, l, 0.0);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, l, s)
}

fn hls_to_rgb(h: f64, l: f64, s: f64, a: u8) -> Color {
    let to = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if s == 0.0 {
        return Color::rgba(to(l), to(l), to(l), a);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    Color::rgba(to(hue(h + 1.0 / 3.0)), to(hue(h)), to(hue(h - 1.0 / 3.0)), a)
}

/// SpreadsheetML tint: luminance darkened (tint < 0) or lightened (tint > 0) in HLS.
pub fn apply_tint(c: Color, tint: f64) -> Color {
    if tint == 0.0 {
        return c;
    }
    let (h, l, s) = rgb_to_hls(c);
    let l = if tint < 0.0 { l * (1.0 + tint) } else { l * (1.0 - tint) + tint };
    hls_to_rgb(h, l, s, c.a)
}

fn parse_argb(s: &str) -> Option<Color> {
    let v = u32::from_str_radix(s.trim().trim_start_matches('#'), 16).ok()?;
    if s.trim().trim_start_matches('#').len() <= 6 {
        return Some(hex_color(v));
    }
    Some(Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

impl ColorContext<'_> {
    /// A `CT_Color` element (`rgb`, `theme` + `tint`, `indexed`, `auto`); `None` for automatic.
    pub fn resolve(&self, el: &Element) -> Option<Color> {
        let base = if let Some(rgb) = el.attr("rgb") {
            parse_argb(rgb)?
        } else if let Some(t) = el.attr("theme") {
            theme_slot(self.theme, t.parse().ok()?)
        } else if let Some(i) = el.attr("indexed") {
            let i: usize = i.parse().ok()?;
            match i {
                64 => Color::BLACK,
                65 => Color::WHITE,
                _ => *self.palette.get(i)?,
            }
        } else {
            return None;
        };
        let tint = el.attr("tint").and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0);
        Some(apply_tint(base, tint))
    }
}

fn truthy(el: &Element, child: &str) -> bool {
    match el.child(child) {
        Some(c) => !matches!(c.attr("val"), Some("0") | Some("false")),
        None => false,
    }
}

fn pattern_alpha(p: &str) -> Option<f64> {
    Some(match p {
        "solid" => 1.0,
        "darkGray" => 0.75,
        "mediumGray" => 0.5,
        "lightGray" => 0.25,
        "gray125" => 0.125,
        "gray0625" => 0.0625,
        "none" => return None,
        _ => 0.5,
    })
}

impl Styles {
    /// Parses `styles.xml`; anything missing keeps Excel's defaults.
    pub fn parse(bytes: &[u8], theme: &Theme) -> Result<Styles, String> {
        let root = parse_dom(bytes).map_err(|e| format!("styles.xml: {e}"))?;
        let mut palette: Vec<Color> = INDEXED.iter().map(|&v| hex_color(v)).collect();
        if let Some(ic) = root.child("colors").and_then(|c| c.child("indexedColors")) {
            let custom: Vec<Color> = ic.children_named("rgbColor").filter_map(|c| c.attr("rgb").and_then(parse_argb)).collect();
            if !custom.is_empty() {
                palette = custom;
            }
        }
        let ctx = ColorContext { theme, palette };
        let mut st = Styles::default();
        if let Some(nf) = root.child("numFmts") {
            for f in nf.children_named("numFmt") {
                if let (Some(id), Some(code)) = (f.attr("numFmtId").and_then(|v| v.parse().ok()), f.attr("formatCode")) {
                    st.num_fmts.insert(id, code.to_string());
                }
            }
        }
        if let Some(fonts) = root.child("fonts") {
            let list: Vec<Font> = fonts
                .children_named("font")
                .map(|f| Font {
                    name: f.child("name").and_then(|n| n.attr("val")).unwrap_or("Calibri").to_string(),
                    size: f.child("sz").and_then(|n| n.attr("val")).and_then(|v| v.parse().ok()).unwrap_or(11.0),
                    bold: truthy(f, "b"),
                    italic: truthy(f, "i"),
                    underline: f.child("u").is_some_and(|u| u.attr("val") != Some("none")),
                    strike: truthy(f, "strike"),
                    color: f.child("color").and_then(|c| ctx.resolve(c)),
                })
                .collect();
            if !list.is_empty() {
                st.fonts = list;
            }
        }
        if let Some(fills) = root.child("fills") {
            let list: Vec<Option<Color>> = fills
                .children_named("fill")
                .map(|f| {
                    if let Some(p) = f.child("patternFill") {
                        let alpha = pattern_alpha(p.attr("patternType").unwrap_or("none"))?;
                        let fg = p.child("fgColor").and_then(|c| ctx.resolve(c));
                        let bg = p.child("bgColor").and_then(|c| ctx.resolve(c));
                        if alpha >= 1.0 {
                            return Some(fg.unwrap_or(Color::BLACK));
                        }
                        // A pattern: the foreground over the background, by the pattern's density.
                        let fg = fg.unwrap_or(Color::BLACK);
                        let bg = bg.unwrap_or(Color::WHITE);
                        let mix = |a: u8, b: u8| (a as f64 * alpha + b as f64 * (1.0 - alpha)).round() as u8;
                        return Some(Color::rgb(mix(fg.r, bg.r), mix(fg.g, bg.g), mix(fg.b, bg.b)));
                    }
                    if let Some(g) = f.child("gradientFill") {
                        return g.children_named("stop").next().and_then(|s| s.child("color")).and_then(|c| ctx.resolve(c));
                    }
                    None
                })
                .collect();
            if !list.is_empty() {
                st.fills = list;
            }
        }
        if let Some(borders) = root.child("borders") {
            let side = |b: &Element, name: &str| -> Option<BorderSide> {
                let s = b.child(name)?;
                let style = BorderStyle::parse(s.attr("style")?)?;
                let color = s.child("color").and_then(|c| ctx.resolve(c)).unwrap_or(Color::BLACK);
                Some(BorderSide { style, color })
            };
            let list: Vec<Border> = borders
                .children_named("border")
                .map(|b| Border {
                    left: side(b, "left").or_else(|| side(b, "start")),
                    right: side(b, "right").or_else(|| side(b, "end")),
                    top: side(b, "top"),
                    bottom: side(b, "bottom"),
                })
                .collect();
            if !list.is_empty() {
                st.borders = list;
            }
        }
        if let Some(xfs) = root.child("cellXfs") {
            let num = |x: &Element, a: &str| -> u32 { x.attr(a).and_then(|v| v.parse().ok()).unwrap_or(0) };
            let list: Vec<Xf> = xfs
                .children_named("xf")
                .map(|x| {
                    let mut xf = Xf {
                        num_fmt: num(x, "numFmtId"),
                        font: num(x, "fontId") as usize,
                        fill: num(x, "fillId") as usize,
                        border: num(x, "borderId") as usize,
                        ..Xf::default()
                    };
                    if let Some(a) = x.child("alignment") {
                        xf.h_align = match a.attr("horizontal") {
                            Some("left") => HAlign::Left,
                            Some("center") => HAlign::Center,
                            Some("right") => HAlign::Right,
                            Some("fill") => HAlign::Fill,
                            Some("justify") => HAlign::Justify,
                            Some("centerContinuous") => HAlign::CenterContinuous,
                            Some("distributed") => HAlign::Distributed,
                            _ => HAlign::General,
                        };
                        xf.v_align = match a.attr("vertical") {
                            Some("top") => VAlign::Top,
                            Some("center") => VAlign::Center,
                            Some("justify") => VAlign::Justify,
                            Some("distributed") => VAlign::Distributed,
                            _ => VAlign::Bottom,
                        };
                        xf.wrap = matches!(a.attr("wrapText"), Some("1") | Some("true"));
                        xf.shrink = matches!(a.attr("shrinkToFit"), Some("1") | Some("true"));
                        xf.indent = num(a, "indent");
                        xf.rotation = a.attr("textRotation").and_then(|v| v.parse().ok()).unwrap_or(0);
                    }
                    xf
                })
                .collect();
            if !list.is_empty() {
                st.xfs = list;
            }
        }
        Ok(st)
    }

    pub fn xf(&self, i: u32) -> &Xf {
        static DEFAULT: std::sync::OnceLock<Xf> = std::sync::OnceLock::new();
        self.xfs.get(i as usize).unwrap_or_else(|| DEFAULT.get_or_init(Xf::default))
    }

    pub fn font(&self, i: usize) -> &Font {
        self.fonts.get(i).or_else(|| self.fonts.first()).expect("fonts never empty")
    }

    pub fn fill(&self, i: usize) -> Option<Color> {
        self.fills.get(i).copied().flatten()
    }

    pub fn border(&self, i: usize) -> Option<&Border> {
        self.borders.get(i).filter(|b| !b.is_empty())
    }

    /// The format code of a `numFmtId`: the file's own, else the built-in one, else General.
    pub fn format_code(&self, id: u32) -> &str {
        match self.num_fmts.get(&id) {
            Some(c) => c,
            None => builtin_format(id).unwrap_or("General"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_lightens_and_darkens() {
        let blue = Color::rgb(0x44, 0x72, 0xC4);
        let light = apply_tint(blue, 0.7999816888943144);
        assert!(light.r > 0xD0 && light.b > 0xEE, "{light:?}");
        let dark = apply_tint(blue, -0.499984740745262);
        assert!(dark.b < 0x70 && dark.r < 0x30, "{dark:?}");
        assert_eq!(apply_tint(Color::WHITE, -0.5), Color::rgb(128, 128, 128));
    }

    #[test]
    fn parses_the_parts() {
        let xml = br#"<?xml version="1.0"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="1"><numFmt numFmtId="164" formatCode="0.000"/></numFmts>
<fonts><font><sz val="11"/><name val="Calibri"/></font><font><b/><i val="1"/><sz val="14"/><color rgb="FFFF0000"/><name val="Arial"/></font><font><color theme="4"/><name val="Calibri"/></font></fonts>
<fills><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor indexed="13"/></patternFill></fill></fills>
<borders><border/><border><left style="thin"/><bottom style="thick"><color rgb="FF0000FF"/></bottom></border></borders>
<cellXfs><xf/><xf numFmtId="164" fontId="1" fillId="2" borderId="1"><alignment horizontal="center" vertical="top" wrapText="1" indent="2"/></xf></cellXfs>
</styleSheet>"#;
        let st = Styles::parse(xml, &Theme::office_default()).unwrap();
        assert_eq!(st.format_code(164), "0.000");
        assert_eq!(st.format_code(9), "0%");
        assert_eq!(st.format_code(999), "General");
        let f = st.font(1);
        assert!(f.bold && f.italic && f.size == 14.0 && f.name == "Arial" && f.color == Some(Color::rgb(255, 0, 0)));
        assert_eq!(st.font(2).color, Some(Color::rgb(0x44, 0x72, 0xC4)));
        assert_eq!(st.fill(0), None);
        assert_eq!(st.fill(2), Some(Color::rgb(255, 255, 0)));
        let b = st.border(1).unwrap();
        assert_eq!(b.left.unwrap().style, BorderStyle::Thin);
        assert_eq!(b.bottom.unwrap().color, Color::rgb(0, 0, 255));
        assert!(st.border(0).is_none());
        let xf = st.xf(1);
        assert_eq!((xf.h_align, xf.v_align, xf.wrap, xf.indent), (HAlign::Center, VAlign::Top, true, 2));
        assert_eq!(st.xf(77).num_fmt, 0);
    }
}
