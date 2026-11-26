//! DrawingML (ECMA-376 Part 1, §20.1): the drawing language shared by the document, spreadsheet and
//! slide editors. This module turns its XML into the Ground's display vocabulary.
//!
//! - [`Theme`] (`a:theme`, `themeN.xml`): the colour scheme, the major/minor font schemes and the
//!   format scheme's fill, line and background-fill styles; [`Theme::office_default`] is the Office
//!   2013+ theme. [`ColorMap`] (`p:clrMap`, `w:clrSchemeMapping`) maps `bg1`/`tx1`/... onto it.
//! - Colours ([`ColorSpec`], [`resolve_color`]): `a:srgbClr`, `a:scrgbClr`, `a:hslClr`, `a:sysClr`,
//!   `a:schemeClr`, `a:prstClr` and the whole transform list applied in document order (HSL for
//!   hue/sat/lum and comp, linear RGB for tint/shade and the red/green/blue family, as Office does).
//! - [`Fill`] and [`Line`] parsed from `a:spPr` children, generic over the colour so a theme style can
//!   keep `phClr` unresolved ([`Fill<ColorSpec>`]) until a shape's style reference supplies it; a
//!   resolved [`Fill`] becomes a [`Paint`], a resolved [`Line`] a [`Stroke`].
//! - Geometry ([`Geometry`], [`geometry`], [`shape_geometry`]): the guide-formula engine, the path
//!   commands (arcs included) and the preset shapes encoded as compact data in this file, plus
//!   `a:custGeom`.
//! - [`Xfrm`] (`a:xfrm`, group `chOff`/`chExt`) to a [`Matrix`], and [`shape_items`] to emit a shape.
//!
//! Units: DrawingML lengths are EMU (914400 per inch, 12700 per point); everything handed to the
//! display list is in points. Angles are 60000ths of a degree in XML and degrees in this API.
//! Malformed input never panics: missing or unreadable values fall back to the spec's defaults.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::fmt;
use std::sync::OnceLock;

use super::display::{path_bounds, Color, GradientStop, Item, LineCap, LineJoin, Matrix, Paint, PathOp, Rect, Stroke};
use super::xml::{parse_dom, Element, XmlError};

/// EMU per point.
pub const EMU_PER_PT: f64 = 12700.0;

/// EMU to points.
pub fn emu_to_pt(emu: f64) -> f64 {
    emu / EMU_PER_PT
}

// ---------------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum DrawError {
    /// The part is not well-formed XML.
    Xml(XmlError),
    /// The root element is not `a:theme` (its name is kept).
    NotTheme(String),
}

impl fmt::Display for DrawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DrawError::Xml(e) => write!(f, "theme XML: {}", e),
            DrawError::NotTheme(n) => write!(f, "not a DrawingML theme (root element <{}>)", n),
        }
    }
}

impl std::error::Error for DrawError {}

impl From<XmlError> for DrawError {
    fn from(e: XmlError) -> DrawError {
        DrawError::Xml(e)
    }
}

// ---------------------------------------------------------------------------------------------------
// Small readers
// ---------------------------------------------------------------------------------------------------

fn num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn attr_num(el: &Element, name: &str) -> Option<f64> {
    el.attr(name).and_then(num)
}

/// ST_Percentage and friends: `"50000"` (1000ths of a percent) or `"50%"` (strict), as a fraction.
fn pct(s: &str) -> Option<f64> {
    let t = s.trim();
    match t.strip_suffix('%') {
        Some(p) => num(p).map(|v| v / 100.0),
        None => num(t).map(|v| v / 100000.0),
    }
}

fn attr_pct(el: &Element, name: &str) -> Option<f64> {
    el.attr(name).and_then(pct)
}

/// An ST_Angle attribute (60000ths of a degree) in degrees.
fn attr_ang(el: &Element, name: &str) -> Option<f64> {
    attr_num(el, name).map(|v| v / 60000.0)
}

fn attr_emu(el: &Element, name: &str) -> Option<i64> {
    el.attr(name).and_then(|s| s.trim().parse::<i64>().ok())
}

fn flag(s: Option<&str>) -> Option<bool> {
    s.map(|v| matches!(v.trim(), "1" | "true" | "on"))
}

/// An attribute by local name, whatever its prefix (`r:embed`).
fn attr_local<'a>(el: &'a Element, local: &str) -> Option<&'a str> {
    el.attrs.iter().find(|(k, _)| k.rsplit(':').next() == Some(local)).map(|(_, v)| v.as_str())
}

fn hex_rgb(s: &str) -> Option<[f64; 3]> {
    let t = s.trim();
    if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(t, 16).ok()?;
    Some([((v >> 16) & 255) as f64 / 255.0, ((v >> 8) & 255) as f64 / 255.0, (v & 255) as f64 / 255.0])
}

/// Clamp to 0..=1 (NaN becomes 0).
fn unit(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

fn sane(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------------------------------
// Colour maths
// ---------------------------------------------------------------------------------------------------

/// sRGB-encoded channel (0..=1) to linear light.
fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light to sRGB-encoded (0..=1).
fn to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB (0..=1) to hue (degrees), saturation and luminance (0..=1).
fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d <= 0.0 {
        return (0.0, 0.0, l);
    }
    let s = if l <= 0.5 { d / (max + min) } else { d / (2.0 - max - min) };
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = sane(h).rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (r + m, g + m, b + m)
}

fn color_to_f(c: Color) -> [f64; 4] {
    [c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0, c.a as f64 / 255.0]
}

fn f_to_color(c: [f64; 4]) -> Color {
    let q = |v: f64| (unit(v) * 255.0).round() as u8;
    Color::rgba(q(c[0]), q(c[1]), q(c[2]), q(c[3]))
}

/// Channel-wise blend from `a` (t = 0) to `b` (t = 1), alpha included.
fn mix(a: Color, b: Color, t: f64) -> Color {
    let t = unit(t);
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round().clamp(0.0, 255.0) as u8;
    Color::rgba(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a))
}

/// The CSS colour names (lower case) behind `a:prstClr`.
const CSS_COLORS: &str = "aliceblue f0f8ff antiquewhite faebd7 aqua 00ffff aquamarine 7fffd4 azure f0ffff \
beige f5f5dc bisque ffe4c4 black 000000 blanchedalmond ffebcd blue 0000ff blueviolet 8a2be2 brown a52a2a \
burlywood deb887 cadetblue 5f9ea0 chartreuse 7fff00 chocolate d2691e coral ff7f50 cornflowerblue 6495ed \
cornsilk fff8dc crimson dc143c cyan 00ffff darkblue 00008b darkcyan 008b8b darkgoldenrod b8860b \
darkgray a9a9a9 darkgrey a9a9a9 darkgreen 006400 darkkhaki bdb76b darkmagenta 8b008b darkolivegreen 556b2f \
darkorange ff8c00 darkorchid 9932cc darkred 8b0000 darksalmon e9967a darkseagreen 8fbc8f \
darkslateblue 483d8b darkslategray 2f4f4f darkslategrey 2f4f4f darkturquoise 00ced1 darkviolet 9400d3 \
deeppink ff1493 deepskyblue 00bfff dimgray 696969 dimgrey 696969 dodgerblue 1e90ff firebrick b22222 \
floralwhite fffaf0 forestgreen 228b22 fuchsia ff00ff gainsboro dcdcdc ghostwhite f8f8ff gold ffd700 \
goldenrod daa520 gray 808080 grey 808080 green 008000 greenyellow adff2f honeydew f0fff0 hotpink ff69b4 \
indianred cd5c5c indigo 4b0082 ivory fffff0 khaki f0e68c lavender e6e6fa lavenderblush fff0f5 \
lawngreen 7cfc00 lemonchiffon fffacd lightblue add8e6 lightcoral f08080 lightcyan e0ffff \
lightgoldenrodyellow fafad2 lightgray d3d3d3 lightgrey d3d3d3 lightgreen 90ee90 lightpink ffb6c1 \
lightsalmon ffa07a lightseagreen 20b2aa lightskyblue 87cefa lightslategray 778899 lightslategrey 778899 \
lightsteelblue b0c4de lightyellow ffffe0 lime 00ff00 limegreen 32cd32 linen faf0e6 magenta ff00ff \
maroon 800000 mediumaquamarine 66cdaa mediumblue 0000cd mediumorchid ba55d3 mediumpurple 9370db \
mediumseagreen 3cb371 mediumslateblue 7b68ee mediumspringgreen 00fa9a mediumturquoise 48d1cc \
mediumvioletred c71585 midnightblue 191970 mintcream f5fffa mistyrose ffe4e1 moccasin ffe4b5 \
navajowhite ffdead navy 000080 oldlace fdf5e6 olive 808000 olivedrab 6b8e23 orange ffa500 \
orangered ff4500 orchid da70d6 palegoldenrod eee8aa palegreen 98fb98 paleturquoise afeeee \
palevioletred db7093 papayawhip ffefd5 peachpuff ffdab9 peru cd853f pink ffc0cb plum dda0dd \
powderblue b0e0e6 purple 800080 red ff0000 rosybrown bc8f8f royalblue 4169e1 saddlebrown 8b4513 \
salmon fa8072 sandybrown f4a460 seagreen 2e8b57 seashell fff5ee sienna a0522d silver c0c0c0 \
skyblue 87ceeb slateblue 6a5acd slategray 708090 slategrey 708090 snow fffafa springgreen 00ff7f \
steelblue 4682b4 tan d2b48c teal 008080 thistle d8bfd8 tomato ff6347 turquoise 40e0d0 violet ee82ee \
wheat f5deb3 white ffffff whitesmoke f5f5f5 yellow ffff00 yellowgreen 9acd32";

/// The `a:prstClr` value table (ST_PresetColorVal): the CSS names, plus the `dk`/`lt`/`med` short
/// forms (`dkBlue`, `ltGray`, `medSeaGreen`).
pub fn preset_color(name: &str) -> Option<Color> {
    preset_rgb(name).map(|c| f_to_color([c[0], c[1], c[2], 1.0]))
}

fn preset_rgb(name: &str) -> Option<[f64; 3]> {
    let lower = name.trim().to_ascii_lowercase();
    let n = if lower.starts_with("medium") {
        lower
    } else if let Some(r) = lower.strip_prefix("med") {
        format!("medium{}", r)
    } else if let Some(r) = lower.strip_prefix("dk") {
        format!("dark{}", r)
    } else if let Some(r) = lower.strip_prefix("lt") {
        format!("light{}", r)
    } else {
        lower
    };
    let mut it = CSS_COLORS.split_whitespace();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        if k == n {
            return hex_rgb(v);
        }
    }
    None
}

/// Windows system colours for an `a:sysClr` without `lastClr` (the usual Windows 10 values).
const SYSTEM_COLORS: &str = "windowText 000000 window ffffff btnFace f0f0f0 btnText 000000 highlight 0078d7 \
highlightText ffffff grayText 6d6d6d menu f0f0f0 menuText 000000 captionText 000000 activeCaption 99b4d1 \
inactiveCaption bfcddb inactiveCaptionText 000000 infoBk ffffe1 infoText 000000 3dDkShadow 696969 \
3dLight e3e3e3 btnHighlight ffffff btnShadow a0a0a0 scrollBar c8c8c8 background 000000 activeBorder b4b4b4 \
inactiveBorder f4f7fc appWorkspace ababab hotLight 0066cc gradientActiveCaption b9d1ea \
gradientInactiveCaption d7e4f2 menuHighlight 3399ff menuBar f0f0f0 windowFrame 646464";

fn system_rgb(name: &str) -> Option<[f64; 3]> {
    let mut it = SYSTEM_COLORS.split_whitespace();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        if k == name.trim() {
            return hex_rgb(v);
        }
    }
    None
}

/// Scheme colour names in every spelling the formats use (`text1`, `dark1`, `hyperlink`, ...) to the
/// DrawingML ones (`tx1`, `dk1`, `hlink`, ...).
pub fn canonical_scheme(name: &str) -> &str {
    match name {
        "text1" | "t1" => "tx1",
        "text2" | "t2" => "tx2",
        "background1" => "bg1",
        "background2" => "bg2",
        "dark1" => "dk1",
        "light1" => "lt1",
        "dark2" => "dk2",
        "light2" => "lt2",
        "hyperlink" => "hlink",
        "followedHyperlink" => "folHlink",
        other => other,
    }
}

/// The default role mapping (`bg1` -> `lt1`, `tx1` -> `dk1`, `bg2` -> `lt2`, `tx2` -> `dk2`).
fn role_default(name: &str) -> Option<&'static str> {
    match name {
        "bg1" => Some("lt1"),
        "tx1" => Some("dk1"),
        "bg2" => Some("lt2"),
        "tx2" => Some("dk2"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------
// Colours as written
// ---------------------------------------------------------------------------------------------------

/// One colour transform (§20.1.2.3). Percentages are fractions (`75000` -> 0.75), angles degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorMod {
    Tint(f64),
    Shade(f64),
    Comp,
    Inv,
    Gray,
    Alpha(f64),
    AlphaOff(f64),
    AlphaMod(f64),
    Hue(f64),
    HueOff(f64),
    HueMod(f64),
    Sat(f64),
    SatOff(f64),
    SatMod(f64),
    Lum(f64),
    LumOff(f64),
    LumMod(f64),
    Red(f64),
    RedOff(f64),
    RedMod(f64),
    Green(f64),
    GreenOff(f64),
    GreenMod(f64),
    Blue(f64),
    BlueOff(f64),
    BlueMod(f64),
    Gamma,
    InvGamma,
}

impl ColorMod {
    /// A transform element, or `None` for anything else (or a transform without its value).
    pub fn parse(el: &Element) -> Option<ColorMod> {
        use ColorMod::*;
        let v = el.attr("val");
        let p = || v.and_then(pct);
        let a = || v.and_then(num).map(|x| x / 60000.0);
        Some(match el.local() {
            "tint" => Tint(p()?),
            "shade" => Shade(p()?),
            "comp" => Comp,
            "inv" => Inv,
            "gray" => Gray,
            "alpha" => Alpha(p()?),
            "alphaOff" => AlphaOff(p()?),
            "alphaMod" => AlphaMod(p()?),
            "hue" => Hue(a()?),
            "hueOff" => HueOff(a()?),
            "hueMod" => HueMod(p()?),
            "sat" => Sat(p()?),
            "satOff" => SatOff(p()?),
            "satMod" => SatMod(p()?),
            "lum" => Lum(p()?),
            "lumOff" => LumOff(p()?),
            "lumMod" => LumMod(p()?),
            "red" => Red(p()?),
            "redOff" => RedOff(p()?),
            "redMod" => RedMod(p()?),
            "green" => Green(p()?),
            "greenOff" => GreenOff(p()?),
            "greenMod" => GreenMod(p()?),
            "blue" => Blue(p()?),
            "blueOff" => BlueOff(p()?),
            "blueMod" => BlueMod(p()?),
            "gamma" => Gamma,
            "invGamma" => InvGamma,
            _ => return None,
        })
    }
}

fn with_hsl(c: &mut [f64; 4], f: impl FnOnce(&mut f64, &mut f64, &mut f64)) {
    let (mut h, mut s, mut l) = rgb_to_hsl(c[0], c[1], c[2]);
    f(&mut h, &mut s, &mut l);
    let (r, g, b) = hsl_to_rgb(sane(h).rem_euclid(360.0), unit(s), unit(l));
    c[0] = r;
    c[1] = g;
    c[2] = b;
}

fn with_linear(c: &mut [f64; 4], i: usize, f: impl FnOnce(f64) -> f64) {
    c[i] = to_srgb(unit(f(to_linear(c[i]))));
}

/// Apply the transforms in order to an sRGB + alpha colour (all 0..=1).
fn apply_mods(mut c: [f64; 4], mods: &[ColorMod]) -> [f64; 4] {
    use ColorMod::*;
    for m in mods {
        match *m {
            // Office mixes towards white / black in linear RGB.
            Tint(v) => {
                let v = unit(v);
                for i in 0..3 {
                    with_linear(&mut c, i, |x| 1.0 - (1.0 - x) * v);
                }
            }
            Shade(v) => {
                let v = unit(v);
                for i in 0..3 {
                    with_linear(&mut c, i, |x| x * v);
                }
            }
            Comp => with_hsl(&mut c, |h, _, _| *h += 180.0),
            Inv => {
                for ch in c.iter_mut().take(3) {
                    *ch = 1.0 - *ch;
                }
            }
            Gray => {
                let y = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                c[0] = y;
                c[1] = y;
                c[2] = y;
            }
            Alpha(v) => c[3] = v,
            AlphaOff(v) => c[3] += v,
            AlphaMod(v) => c[3] *= v,
            Hue(v) => with_hsl(&mut c, |h, _, _| *h = v),
            HueOff(v) => with_hsl(&mut c, |h, _, _| *h += v),
            HueMod(v) => with_hsl(&mut c, |h, _, _| *h *= v),
            Sat(v) => with_hsl(&mut c, |_, s, _| *s = v),
            SatOff(v) => with_hsl(&mut c, |_, s, _| *s += v),
            SatMod(v) => with_hsl(&mut c, |_, s, _| *s *= v),
            Lum(v) => with_hsl(&mut c, |_, _, l| *l = v),
            LumOff(v) => with_hsl(&mut c, |_, _, l| *l += v),
            LumMod(v) => with_hsl(&mut c, |_, _, l| *l *= v),
            Red(v) => with_linear(&mut c, 0, |_| v),
            RedOff(v) => with_linear(&mut c, 0, |x| x + v),
            RedMod(v) => with_linear(&mut c, 0, |x| x * v),
            Green(v) => with_linear(&mut c, 1, |_| v),
            GreenOff(v) => with_linear(&mut c, 1, |x| x + v),
            GreenMod(v) => with_linear(&mut c, 1, |x| x * v),
            Blue(v) => with_linear(&mut c, 2, |_| v),
            BlueOff(v) => with_linear(&mut c, 2, |x| x + v),
            BlueMod(v) => with_linear(&mut c, 2, |x| x * v),
            // The input taken as linear and encoded with the sRGB curve (lighter), and back (darker).
            Gamma => {
                for ch in c.iter_mut().take(3) {
                    *ch = to_srgb(unit(*ch));
                }
            }
            InvGamma => {
                for ch in c.iter_mut().take(3) {
                    *ch = to_linear(unit(*ch));
                }
            }
        }
        for ch in c.iter_mut() {
            *ch = unit(*ch);
        }
    }
    c
}

/// The base of a colour: a concrete sRGB value (0..=1), or a scheme name resolved later
/// (`accent1`, `tx1`, `phClr`, ...).
#[derive(Debug, Clone, PartialEq)]
pub enum BaseColor {
    Rgb([f64; 3]),
    Scheme(String),
}

/// A colour as written: its base and its transforms, kept until a theme, a colour map and (for
/// `phClr`) a placeholder colour are known.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorSpec {
    pub base: BaseColor,
    pub mods: Vec<ColorMod>,
}

const COLOR_ELEMENTS: [&str; 6] = ["srgbClr", "scrgbClr", "hslClr", "sysClr", "schemeClr", "prstClr"];

fn is_color_element(el: &Element) -> bool {
    COLOR_ELEMENTS.contains(&el.local())
}

impl ColorSpec {
    pub fn rgb(c: Color) -> ColorSpec {
        let f = color_to_f(c);
        let mut spec = ColorSpec { base: BaseColor::Rgb([f[0], f[1], f[2]]), mods: Vec::new() };
        if c.a != 255 {
            spec.mods.push(ColorMod::Alpha(f[3]));
        }
        spec
    }

    pub fn scheme(name: &str) -> ColorSpec {
        ColorSpec { base: BaseColor::Scheme(name.to_string()), mods: Vec::new() }
    }

    /// With one more transform.
    pub fn with(mut self, m: ColorMod) -> ColorSpec {
        self.mods.push(m);
        self
    }

    /// A colour element, or the first colour element inside a container (`a:solidFill`, `a:gs`,
    /// `a:fgClr`, `a:dk1`, ...). Unreadable values become black rather than failing.
    pub fn parse(el: &Element) -> Option<ColorSpec> {
        let el = if is_color_element(el) { el } else { el.elements().find(|e| is_color_element(e))? };
        let black = [0.0; 3];
        let base = match el.local() {
            "srgbClr" => BaseColor::Rgb(el.attr("val").and_then(hex_rgb).unwrap_or(black)),
            "scrgbClr" => {
                let ch = |n: &str| to_srgb(unit(attr_pct(el, n).unwrap_or(0.0)));
                BaseColor::Rgb([ch("r"), ch("g"), ch("b")])
            }
            "hslClr" => {
                let h = attr_ang(el, "hue").unwrap_or(0.0);
                let s = unit(attr_pct(el, "sat").unwrap_or(0.0));
                let l = unit(attr_pct(el, "lum").unwrap_or(0.0));
                let (r, g, b) = hsl_to_rgb(h, s, l);
                BaseColor::Rgb([r, g, b])
            }
            "sysClr" => BaseColor::Rgb(
                el.attr("lastClr").and_then(hex_rgb).or_else(|| el.attr("val").and_then(system_rgb)).unwrap_or(black),
            ),
            "schemeClr" => BaseColor::Scheme(el.attr("val").unwrap_or("tx1").trim().to_string()),
            _ => BaseColor::Rgb(el.attr("val").and_then(preset_rgb).unwrap_or(black)),
        };
        let mods = el.elements().filter_map(ColorMod::parse).collect();
        Some(ColorSpec { base, mods })
    }

    /// The colour, with scheme names looked up through `cmap` in `theme` and `phClr` taken from
    /// `placeholder` (black when there is none).
    pub fn resolve(&self, theme: &Theme, cmap: &ColorMap, placeholder: Option<Color>) -> Color {
        self.resolve_in(&theme.colors, cmap, placeholder)
    }

    /// [`ColorSpec::resolve`] against a bare colour scheme.
    pub fn resolve_in(&self, scheme: &ColorScheme, cmap: &ColorMap, placeholder: Option<Color>) -> Color {
        let base = match &self.base {
            BaseColor::Rgb(c) => [c[0], c[1], c[2], 1.0],
            BaseColor::Scheme(n) if n == "phClr" => color_to_f(placeholder.unwrap_or(Color::BLACK)),
            BaseColor::Scheme(n) => color_to_f(scheme.lookup(n, cmap)),
        };
        f_to_color(apply_mods(base, &self.mods))
    }
}

/// Resolve a colour element (or the first colour inside a container element) to a display colour.
/// `phClr` uses `placeholder`. Anything unreadable is black.
pub fn resolve_color(el: &Element, theme: &Theme, cmap: &ColorMap, placeholder: Option<Color>) -> Color {
    ColorSpec::parse(el).map(|c| c.resolve(theme, cmap, placeholder)).unwrap_or(Color::BLACK)
}

// ---------------------------------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------------------------------

/// `a:clrScheme`: the twelve theme colours, as sRGB (`a:sysClr` uses its `lastClr`).
#[derive(Debug, Clone, PartialEq)]
pub struct ColorScheme {
    pub name: String,
    pub dk1: Color,
    pub lt1: Color,
    pub dk2: Color,
    pub lt2: Color,
    pub accent1: Color,
    pub accent2: Color,
    pub accent3: Color,
    pub accent4: Color,
    pub accent5: Color,
    pub accent6: Color,
    pub hlink: Color,
    pub fol_hlink: Color,
}

impl ColorScheme {
    /// The Office 2013+ palette.
    pub fn office() -> ColorScheme {
        let h = |v: u32| Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        ColorScheme {
            name: "Office".into(),
            dk1: h(0x000000),
            lt1: h(0xFFFFFF),
            dk2: h(0x44546A),
            lt2: h(0xE7E6E6),
            accent1: h(0x4472C4),
            accent2: h(0xED7D31),
            accent3: h(0xA5A5A5),
            accent4: h(0xFFC000),
            accent5: h(0x5B9BD5),
            accent6: h(0x70AD47),
            hlink: h(0x0563C1),
            fol_hlink: h(0x954F72),
        }
    }

    /// A slot by its own name (`dk1` .. `folHlink`, any spelling of [`canonical_scheme`]); the role
    /// names (`bg1`, `tx1`, ...) are not slots - see [`ColorScheme::lookup`].
    pub fn get(&self, name: &str) -> Option<Color> {
        Some(match canonical_scheme(name) {
            "dk1" => self.dk1,
            "lt1" => self.lt1,
            "dk2" => self.dk2,
            "lt2" => self.lt2,
            "accent1" => self.accent1,
            "accent2" => self.accent2,
            "accent3" => self.accent3,
            "accent4" => self.accent4,
            "accent5" => self.accent5,
            "accent6" => self.accent6,
            "hlink" => self.hlink,
            "folHlink" => self.fol_hlink,
            _ => return None,
        })
    }

    fn slot(&mut self, name: &str) -> Option<&mut Color> {
        Some(match canonical_scheme(name) {
            "dk1" => &mut self.dk1,
            "lt1" => &mut self.lt1,
            "dk2" => &mut self.dk2,
            "lt2" => &mut self.lt2,
            "accent1" => &mut self.accent1,
            "accent2" => &mut self.accent2,
            "accent3" => &mut self.accent3,
            "accent4" => &mut self.accent4,
            "accent5" => &mut self.accent5,
            "accent6" => &mut self.accent6,
            "hlink" => &mut self.hlink,
            "folHlink" => &mut self.fol_hlink,
            _ => return None,
        })
    }

    /// A scheme colour by any name a document uses, roles mapped through `cmap` (unknown: black).
    pub fn lookup(&self, name: &str, cmap: &ColorMap) -> Color {
        let n = canonical_scheme(name.trim());
        let mapped = canonical_scheme(cmap.map(n));
        let mapped = role_default(mapped).unwrap_or(mapped);
        self.get(mapped).unwrap_or(Color::BLACK)
    }
}

/// `p:clrMap` / `p:clrMapOvr` / `w:clrSchemeMapping`: which scheme slot each role name uses.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorMap {
    /// (role, slot) pairs, e.g. ("bg1", "lt1").
    pub pairs: Vec<(String, String)>,
}

const MAP_KEYS: [&str; 12] =
    ["bg1", "tx1", "bg2", "tx2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"];

impl Default for ColorMap {
    fn default() -> ColorMap {
        ColorMap { pairs: MAP_KEYS.iter().map(|k| (k.to_string(), role_default(k).unwrap_or(k).to_string())).collect() }
    }
}

impl ColorMap {
    /// From the mapping element's attributes (PresentationML `bg1="lt1"` or WordprocessingML
    /// `w:bg1="light1"`, `w:t1="dark1"`, `w:hyperlink="hyperlink"`); a `p:clrMapOvr` holding an
    /// `a:overrideClrMapping` uses that. Missing roles keep the default mapping.
    pub fn parse(el: &Element) -> ColorMap {
        if let Some(o) = el.child("overrideClrMapping") {
            return ColorMap::parse(o);
        }
        let mut m = ColorMap::default();
        for (k, v) in &el.attrs {
            let local = k.rsplit(':').next().unwrap_or(k);
            let key = match local {
                "t1" => "tx1",
                "t2" => "tx2",
                "hyperlink" => "hlink",
                "followedHyperlink" => "folHlink",
                o => o,
            };
            if !MAP_KEYS.contains(&key) {
                continue;
            }
            let val = canonical_scheme(v.trim());
            if let Some(slot) = m.pairs.iter_mut().find(|(a, _)| a == key) {
                slot.1 = val.to_string();
            }
        }
        m
    }

    /// The slot a role maps to (any other name is returned unchanged).
    pub fn map<'a>(&'a self, name: &'a str) -> &'a str {
        self.pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).unwrap_or(name)
    }
}

/// `a:majorFont` / `a:minorFont`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FontScheme {
    pub latin: String,
    pub ea: String,
    pub cs: String,
    /// (script tag, typeface), e.g. ("Jpan", "Yu Gothic").
    pub scripts: Vec<(String, String)>,
}

impl FontScheme {
    fn parse(el: &Element) -> FontScheme {
        let face = |n: &str| el.child(n).and_then(|c| c.attr("typeface")).unwrap_or("").to_string();
        FontScheme {
            latin: face("latin"),
            ea: face("ea"),
            cs: face("cs"),
            scripts: el
                .children_named("font")
                .filter_map(|f| Some((f.attr("script")?.to_string(), f.attr("typeface").unwrap_or("").to_string())))
                .collect(),
        }
    }

    /// The typeface for a script tag (`Jpan`, `Arab`, ...).
    pub fn script(&self, script: &str) -> Option<&str> {
        self.scripts.iter().find(|(s, _)| s == script).map(|(_, f)| f.as_str())
    }
}

/// A DrawingML theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub colors: ColorScheme,
    pub major_font: FontScheme,
    pub minor_font: FontScheme,
    /// `a:fillStyleLst` (style matrix indices 1..=999), colours unresolved (`phClr`).
    pub fill_styles: Vec<Fill<ColorSpec>>,
    /// `a:lnStyleLst`.
    pub line_styles: Vec<Line<ColorSpec>>,
    /// `a:bgFillStyleLst` (indices 1001..).
    pub bg_fill_styles: Vec<Fill<ColorSpec>>,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::office_default()
    }
}

impl Theme {
    fn bare() -> Theme {
        Theme {
            name: "Office Theme".into(),
            colors: ColorScheme::office(),
            major_font: FontScheme { latin: "Calibri Light".into(), ..FontScheme::default() },
            minor_font: FontScheme { latin: "Calibri".into(), ..FontScheme::default() },
            fill_styles: Vec::new(),
            line_styles: Vec::new(),
            bg_fill_styles: Vec::new(),
        }
    }

    /// Parse a theme part. Parts it lacks keep the Office defaults (colours and fonts) or stay empty
    /// (styles).
    pub fn parse(xml: &[u8]) -> Result<Theme, DrawError> {
        let root = parse_dom(xml)?;
        if root.local() != "theme" {
            return Err(DrawError::NotTheme(root.name.clone()));
        }
        Ok(Theme::from_element(&root))
    }

    /// From an already parsed `a:theme` element.
    pub fn from_element(root: &Element) -> Theme {
        let mut t = Theme::bare();
        if let Some(n) = root.attr("name") {
            t.name = n.to_string();
        }
        let elems = match root.child("themeElements") {
            Some(e) => e,
            None => return t,
        };
        if let Some(cs) = elems.child("clrScheme") {
            if let Some(n) = cs.attr("name") {
                t.colors.name = n.to_string();
            }
            let base = ColorScheme::office();
            let cmap = ColorMap::default();
            for c in cs.elements() {
                if let Some(spec) = ColorSpec::parse(c) {
                    let col = spec.resolve_in(&base, &cmap, None);
                    if let Some(slot) = t.colors.slot(c.local()) {
                        *slot = col;
                    }
                }
            }
        }
        if let Some(fs) = elems.child("fontScheme") {
            if let Some(m) = fs.child("majorFont") {
                t.major_font = FontScheme::parse(m);
            }
            if let Some(m) = fs.child("minorFont") {
                t.minor_font = FontScheme::parse(m);
            }
        }
        if let Some(fmt) = elems.child("fmtScheme") {
            if let Some(l) = fmt.child("fillStyleLst") {
                t.fill_styles = l.elements().filter_map(parse_fill).collect();
            }
            if let Some(l) = fmt.child("lnStyleLst") {
                t.line_styles = l.children_named("ln").map(parse_line).collect();
            }
            if let Some(l) = fmt.child("bgFillStyleLst") {
                t.bg_fill_styles = l.elements().filter_map(parse_fill).collect();
            }
        }
        t
    }

    /// The Office 2013+ theme: its palette, Calibri Light / Calibri, and its format scheme.
    pub fn office_default() -> Theme {
        Theme::parse(DEFAULT_THEME_XML.as_bytes()).unwrap_or_else(|_| Theme::bare())
    }

    /// A scheme colour through a colour map.
    pub fn scheme_color(&self, name: &str, cmap: &ColorMap) -> Color {
        self.colors.lookup(name, cmap)
    }

    /// A style-matrix fill reference (`a:fillRef idx`, `a:bgFillRef`): 0 is no fill, 1..=999 the
    /// fill styles, 1001.. the background fill styles.
    pub fn fill_ref(&self, idx: u32) -> Option<&Fill<ColorSpec>> {
        match idx {
            0 => None,
            1..=999 => self.fill_styles.get(idx as usize - 1),
            1000 => None,
            _ => self.bg_fill_styles.get(idx as usize - 1001),
        }
    }

    /// A style-matrix line reference (`a:lnRef idx`, 1-based; 0 is no line).
    pub fn line_ref(&self, idx: u32) -> Option<&Line<ColorSpec>> {
        if idx == 0 {
            None
        } else {
            self.line_styles.get(idx as usize - 1)
        }
    }

    /// Theme font references (`+mj-lt`, `+mn-ea`, `+mj-cs`, ...) to the typeface; anything else is
    /// returned as given.
    pub fn resolve_typeface<'a>(&'a self, typeface: &'a str) -> &'a str {
        let (scheme, slot) = match typeface {
            "+mj-lt" => (&self.major_font, 0),
            "+mj-ea" => (&self.major_font, 1),
            "+mj-cs" => (&self.major_font, 2),
            "+mn-lt" => (&self.minor_font, 0),
            "+mn-ea" => (&self.minor_font, 1),
            "+mn-cs" => (&self.minor_font, 2),
            _ => return typeface,
        };
        match slot {
            0 => &scheme.latin,
            1 => &scheme.ea,
            _ => &scheme.cs,
        }
    }
}

const DEFAULT_THEME_XML: &str = concat!(
    r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office Theme"><a:themeElements>"#,
    r#"<a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>"#,
    r#"<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2>"#,
    r#"<a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1>"#,
    r#"<a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3>"#,
    r#"<a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5>"#,
    r#"<a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink>"#,
    r#"<a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>"#,
    r#"<a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/>"#,
    r#"<a:font script="Jpan" typeface="游ゴシック Light"/><a:font script="Hang" typeface="맑은 고딕"/>"#,
    r#"<a:font script="Hans" typeface="等线 Light"/><a:font script="Hant" typeface="新細明體"/>"#,
    r#"<a:font script="Arab" typeface="Times New Roman"/><a:font script="Hebr" typeface="Times New Roman"/>"#,
    r#"<a:font script="Thai" typeface="Angsana New"/></a:majorFont>"#,
    r#"<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/>"#,
    r#"<a:font script="Jpan" typeface="游明朝"/><a:font script="Hang" typeface="맑은 고딕"/>"#,
    r#"<a:font script="Hans" typeface="等线"/><a:font script="Hant" typeface="新細明體"/>"#,
    r#"<a:font script="Arab" typeface="Arial"/><a:font script="Hebr" typeface="Arial"/>"#,
    r#"<a:font script="Thai" typeface="Cordia New"/></a:minorFont></a:fontScheme>"#,
    r#"<a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#,
    r#"<a:gradFill rotWithShape="1"><a:gsLst>"#,
    r#"<a:gs pos="0"><a:schemeClr val="phClr"><a:lumMod val="110000"/><a:satMod val="105000"/><a:tint val="67000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="50000"><a:schemeClr val="phClr"><a:lumMod val="105000"/><a:satMod val="103000"/><a:tint val="73000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="100000"><a:schemeClr val="phClr"><a:lumMod val="105000"/><a:satMod val="109000"/><a:tint val="81000"/></a:schemeClr></a:gs>"#,
    r#"</a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>"#,
    r#"<a:gradFill rotWithShape="1"><a:gsLst>"#,
    r#"<a:gs pos="0"><a:schemeClr val="phClr"><a:satMod val="103000"/><a:lumMod val="102000"/><a:tint val="94000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="50000"><a:schemeClr val="phClr"><a:satMod val="110000"/><a:lumMod val="100000"/><a:shade val="100000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="100000"><a:schemeClr val="phClr"><a:lumMod val="99000"/><a:satMod val="120000"/><a:shade val="78000"/></a:schemeClr></a:gs>"#,
    r#"</a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill></a:fillStyleLst>"#,
    r#"<a:lnStyleLst>"#,
    r#"<a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>"#,
    r#"<a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>"#,
    r#"<a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>"#,
    r#"</a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle>"#,
    r#"<a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>"#,
    r#"<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#,
    r#"<a:solidFill><a:schemeClr val="phClr"><a:tint val="95000"/><a:satMod val="170000"/></a:schemeClr></a:solidFill>"#,
    r#"<a:gradFill rotWithShape="1"><a:gsLst>"#,
    r#"<a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="93000"/><a:satMod val="150000"/><a:shade val="98000"/><a:lumMod val="102000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="50000"><a:schemeClr val="phClr"><a:tint val="98000"/><a:satMod val="130000"/><a:shade val="90000"/><a:lumMod val="103000"/></a:schemeClr></a:gs>"#,
    r#"<a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="63000"/><a:satMod val="120000"/></a:schemeClr></a:gs>"#,
    r#"</a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
);

// ---------------------------------------------------------------------------------------------------
// Fills
// ---------------------------------------------------------------------------------------------------

/// Fractions of a box's width/height inset from each side (`a:fillToRect`, `a:srcRect`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

fn insets(el: &Element) -> Insets {
    Insets {
        l: attr_pct(el, "l").unwrap_or(0.0),
        t: attr_pct(el, "t").unwrap_or(0.0),
        r: attr_pct(el, "r").unwrap_or(0.0),
        b: attr_pct(el, "b").unwrap_or(0.0),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathShade {
    Circle,
    Rect,
    Shape,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientKind {
    /// `a:lin`: angle in degrees clockwise from the x axis; `scaled` stretches it with the box.
    Linear { angle: f64, scaled: bool },
    /// `a:path`: shades out from the `focus` rect (`a:fillToRect`) to the edge.
    Path { shape: PathShade, focus: Insets },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stop<C = Color> {
    /// 0..=1.
    pub pos: f64,
    pub color: C,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gradient<C = Color> {
    /// Sorted by position.
    pub stops: Vec<Stop<C>>,
    pub kind: GradientKind,
    pub rot_with_shape: bool,
    /// `flip` (none / x / y / xy), kept as written.
    pub flip: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PictureMode {
    /// `a:stretch` with its `a:fillRect`.
    Stretch(Insets),
    /// `a:tile`: offsets in EMU, scales as fractions, flip and alignment as written.
    Tile { tx: i64, ty: i64, sx: f64, sy: f64, flip: String, align: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PictureFill {
    /// The relationship id of the image part (`r:embed`).
    pub embed: Option<String>,
    /// An external image (`r:link`).
    pub link: Option<String>,
    pub src_rect: Insets,
    pub mode: PictureMode,
    pub rot_with_shape: bool,
    /// From `a:alphaModFix` (1 when absent).
    pub alpha: f64,
}

/// A DrawingML fill, generic over its colours: [`Fill<ColorSpec>`] as written (theme styles keep
/// `phClr`), [`Fill`] (= `Fill<Color>`) once resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum Fill<C = Color> {
    None,
    Solid(C),
    Gradient(Gradient<C>),
    Pattern { prst: String, fg: C, bg: C },
    Picture(PictureFill),
    /// `a:grpFill`: the enclosing group's fill.
    Group,
}

impl<C> Fill<C> {
    /// The same fill with every colour mapped.
    pub fn map<D, F: FnMut(&C) -> D>(&self, f: &mut F) -> Fill<D> {
        match self {
            Fill::None => Fill::None,
            Fill::Solid(c) => Fill::Solid(f(c)),
            Fill::Gradient(g) => Fill::Gradient(Gradient {
                stops: g.stops.iter().map(|s| Stop { pos: s.pos, color: f(&s.color) }).collect(),
                kind: g.kind,
                rot_with_shape: g.rot_with_shape,
                flip: g.flip.clone(),
            }),
            Fill::Pattern { prst, fg, bg } => {
                let fg = f(fg);
                let bg = f(bg);
                Fill::Pattern { prst: prst.clone(), fg, bg }
            }
            Fill::Picture(p) => Fill::Picture(p.clone()),
            Fill::Group => Fill::Group,
        }
    }
}

impl Fill<ColorSpec> {
    pub fn resolve(&self, theme: &Theme, cmap: &ColorMap, placeholder: Option<Color>) -> Fill {
        self.map(&mut |c: &ColorSpec| c.resolve(theme, cmap, placeholder))
    }
}

/// A fill element (`a:noFill`, `a:solidFill`, `a:gradFill`, `a:pattFill`, `a:blipFill`, `a:grpFill`),
/// or `None` for any other element.
pub fn parse_fill(el: &Element) -> Option<Fill<ColorSpec>> {
    let black = || ColorSpec { base: BaseColor::Rgb([0.0; 3]), mods: Vec::new() };
    Some(match el.local() {
        "noFill" => Fill::None,
        "solidFill" => Fill::Solid(ColorSpec::parse(el).unwrap_or_else(black)),
        "gradFill" => {
            let mut stops: Vec<Stop<ColorSpec>> = el
                .child("gsLst")
                .map(|l| {
                    l.children_named("gs")
                        .filter_map(|gs| Some(Stop { pos: unit(attr_pct(gs, "pos").unwrap_or(0.0)), color: ColorSpec::parse(gs)? }))
                        .collect()
                })
                .unwrap_or_default();
            stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap_or(Ordering::Equal));
            let kind = if let Some(lin) = el.child("lin") {
                GradientKind::Linear { angle: attr_ang(lin, "ang").unwrap_or(0.0), scaled: flag(lin.attr("scaled")).unwrap_or(false) }
            } else if let Some(p) = el.child("path") {
                let shape = match p.attr("path") {
                    Some("rect") => PathShade::Rect,
                    Some("shape") => PathShade::Shape,
                    _ => PathShade::Circle,
                };
                GradientKind::Path { shape, focus: p.child("fillToRect").map(insets).unwrap_or_default() }
            } else {
                GradientKind::Linear { angle: 0.0, scaled: false }
            };
            Fill::Gradient(Gradient {
                stops,
                kind,
                rot_with_shape: flag(el.attr("rotWithShape")).unwrap_or(false),
                flip: el.attr("flip").map(String::from),
            })
        }
        "pattFill" => Fill::Pattern {
            prst: el.attr("prst").unwrap_or("pct5").to_string(),
            fg: el.child("fgClr").and_then(ColorSpec::parse).unwrap_or_else(black),
            bg: el.child("bgClr").and_then(ColorSpec::parse).unwrap_or_else(|| ColorSpec::rgb(Color::WHITE)),
        },
        "blipFill" => {
            let blip = el.child("blip");
            let mode = if let Some(t) = el.child("tile") {
                PictureMode::Tile {
                    tx: attr_emu(t, "tx").unwrap_or(0),
                    ty: attr_emu(t, "ty").unwrap_or(0),
                    sx: attr_pct(t, "sx").unwrap_or(1.0),
                    sy: attr_pct(t, "sy").unwrap_or(1.0),
                    flip: t.attr("flip").unwrap_or("none").to_string(),
                    align: t.attr("algn").unwrap_or("tl").to_string(),
                }
            } else {
                PictureMode::Stretch(el.child("stretch").and_then(|s| s.child("fillRect")).map(insets).unwrap_or_default())
            };
            Fill::Picture(PictureFill {
                embed: blip.and_then(|b| attr_local(b, "embed")).map(String::from),
                link: blip.and_then(|b| attr_local(b, "link")).map(String::from),
                src_rect: el.child("srcRect").map(insets).unwrap_or_default(),
                mode,
                rot_with_shape: flag(el.attr("rotWithShape")).unwrap_or(true),
                alpha: blip.and_then(|b| b.child("alphaModFix")).and_then(|a| attr_pct(a, "amt")).map(unit).unwrap_or(1.0),
            })
        }
        "grpFill" => Fill::Group,
        _ => return None,
    })
}

/// The first fill among an element's children (`a:spPr`, `p:bgPr`, `a:tcPr`, ...).
pub fn find_fill(parent: &Element) -> Option<Fill<ColorSpec>> {
    parent.elements().find_map(parse_fill)
}

/// How much of a pattern's cell its foreground covers (patterns are painted as that blend).
fn pattern_coverage(prst: &str) -> f64 {
    if let Some(p) = prst.strip_prefix("pct") {
        if let Some(v) = num(p) {
            return unit(v / 100.0);
        }
    }
    match prst {
        "ltHorz" | "ltVert" | "ltDnDiag" | "ltUpDiag" | "narHorz" | "narVert" | "dashHorz" | "dashVert" | "dashDnDiag"
        | "dashUpDiag" | "smGrid" | "dotGrid" | "dotDmnd" | "smConfetti" => 0.25,
        "dkHorz" | "dkVert" | "dkDnDiag" | "dkUpDiag" | "wdDnDiag" | "wdUpDiag" | "trellis" | "solidDmnd" | "lgCheck"
        | "smCheck" | "plaid" | "sphere" | "weave" | "shingle" | "divot" | "lgConfetti" => 0.5,
        _ => 0.375,
    }
}

/// The gradient line of a linear gradient over `bx`: through the centre, long enough that the
/// corners reach the first and last stops.
fn linear_ends(bx: Rect, angle_deg: f64, scaled: bool) -> (f64, f64, f64, f64) {
    let a = sane(angle_deg).to_radians();
    let (mut dx, mut dy) = (a.cos(), a.sin());
    if scaled && bx.w > 0.0 && bx.h > 0.0 {
        // The angle is drawn in a square stretched to the box: its isolines stretch, and the
        // gradient direction stays perpendicular to them.
        let (x, y) = (dx * bx.h, dy * bx.w);
        let n = (x * x + y * y).sqrt();
        if n > 0.0 {
            dx = x / n;
            dy = y / n;
        }
    }
    let half = (dx.abs() * bx.w + dy.abs() * bx.h) / 2.0;
    let (cx, cy) = (bx.x + bx.w / 2.0, bx.y + bx.h / 2.0);
    (cx - dx * half, cy - dy * half, cx + dx * half, cy + dy * half)
}

impl Fill<Color> {
    /// The fill as a paint over the box `bx` (the shape's own coordinates, points). `None` for no
    /// fill, a group fill (the caller passes the group's) and a picture fill (the caller draws the
    /// image clipped to the path).
    pub fn to_paint(&self, bx: Rect) -> Option<Paint> {
        self.to_paint_rotated(bx, 0.0)
    }

    /// [`Fill::to_paint`] for a shape rotated by `shape_rot` degrees: a gradient that does not
    /// rotate with its shape is turned back by that much.
    pub fn to_paint_rotated(&self, bx: Rect, shape_rot: f64) -> Option<Paint> {
        match self {
            Fill::Solid(c) => Some(Paint::Solid(*c)),
            Fill::Gradient(g) => {
                match g.stops.len() {
                    0 => return None,
                    1 => return Some(Paint::Solid(g.stops[0].color)),
                    _ => {}
                }
                let stops: Vec<GradientStop> = g.stops.iter().map(|s| GradientStop { offset: unit(s.pos), color: s.color }).collect();
                match g.kind {
                    GradientKind::Linear { angle, scaled } => {
                        let a = if g.rot_with_shape { angle } else { angle - shape_rot };
                        let (x0, y0, x1, y1) = linear_ends(bx, a, scaled);
                        Some(Paint::Linear { x0, y0, x1, y1, stops })
                    }
                    GradientKind::Path { focus, .. } => {
                        let l = bx.x + focus.l * bx.w;
                        let t = bx.y + focus.t * bx.h;
                        let r = bx.right() - focus.r * bx.w;
                        let b = bx.bottom() - focus.b * bx.h;
                        let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
                        let corners = [(bx.x, bx.y), (bx.right(), bx.y), (bx.x, bx.bottom()), (bx.right(), bx.bottom())];
                        let radius = corners.iter().map(|(x, y)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()).fold(0.0, f64::max);
                        Some(Paint::Radial { fx: cx, fy: cy, fr: 0.0, cx, cy, r: radius, stops })
                    }
                }
            }
            Fill::Pattern { prst, fg, bg } => Some(Paint::Solid(mix(*bg, *fg, pattern_coverage(prst)))),
            Fill::None | Fill::Picture(_) | Fill::Group => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Lines
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Dash {
    /// `a:prstDash` (solid, dot, dash, lgDash, dashDot, lgDashDot, lgDashDotDot, sysDash, sysDot,
    /// sysDashDot, sysDashDotDot).
    Preset(String),
    /// `a:custDash`: (dash, space) pairs as multiples of the line width.
    Custom(Vec<(f64, f64)>),
}

impl Dash {
    /// On/off lengths as multiples of the line width (empty: solid).
    pub fn pattern(&self) -> Vec<f64> {
        match self {
            Dash::Preset(p) => match p.as_str() {
                "dot" => vec![1.0, 3.0],
                "dash" => vec![4.0, 3.0],
                "lgDash" => vec![8.0, 3.0],
                "dashDot" => vec![4.0, 3.0, 1.0, 3.0],
                "lgDashDot" => vec![8.0, 3.0, 1.0, 3.0],
                "lgDashDotDot" => vec![8.0, 3.0, 1.0, 3.0, 1.0, 3.0],
                "sysDash" => vec![3.0, 1.0],
                "sysDot" => vec![1.0, 1.0],
                "sysDashDot" => vec![3.0, 1.0, 1.0, 1.0],
                "sysDashDotDot" => vec![3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                _ => Vec::new(),
            },
            Dash::Custom(v) => {
                let out: Vec<f64> = v.iter().flat_map(|&(d, s)| [d, s]).collect();
                if out.iter().all(|x| *x <= 0.0) {
                    Vec::new()
                } else {
                    out
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndKind {
    None,
    Triangle,
    Stealth,
    Diamond,
    Oval,
    Arrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndSize {
    Small,
    Medium,
    Large,
}

impl EndSize {
    fn parse(s: Option<&str>) -> EndSize {
        match s {
            Some("sm") => EndSize::Small,
            Some("lg") => EndSize::Large,
            _ => EndSize::Medium,
        }
    }

    /// Multiples of the line width.
    pub fn factor(self) -> f64 {
        match self {
            EndSize::Small => 2.0,
            EndSize::Medium => 3.0,
            EndSize::Large => 5.0,
        }
    }
}

/// `a:headEnd` / `a:tailEnd`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineEnd {
    pub kind: LineEndKind,
    pub w: EndSize,
    pub len: EndSize,
}

impl LineEnd {
    pub fn parse(el: &Element) -> LineEnd {
        let kind = match el.attr("type") {
            Some("triangle") => LineEndKind::Triangle,
            Some("stealth") => LineEndKind::Stealth,
            Some("diamond") => LineEndKind::Diamond,
            Some("oval") => LineEndKind::Oval,
            Some("arrow") => LineEndKind::Arrow,
            _ => LineEndKind::None,
        };
        LineEnd { kind, w: EndSize::parse(el.attr("w")), len: EndSize::parse(el.attr("len")) }
    }
}

/// `a:ln`. Every field is optional so a shape's own line can sit over its style's
/// ([`Line::over`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Line<C = Color> {
    pub width_emu: Option<i64>,
    pub fill: Option<Fill<C>>,
    pub dash: Option<Dash>,
    pub cap: Option<LineCap>,
    pub join: Option<LineJoin>,
    /// From `a:miter lim` (a multiple of the width).
    pub miter_limit: Option<f64>,
    /// `cmpd` (sng, dbl, thickThin, thinThick, tri), kept as written.
    pub compound: Option<String>,
    pub head: Option<LineEnd>,
    pub tail: Option<LineEnd>,
}

impl<C> Default for Line<C> {
    fn default() -> Line<C> {
        Line { width_emu: None, fill: None, dash: None, cap: None, join: None, miter_limit: None, compound: None, head: None, tail: None }
    }
}

impl<C: Clone> Line<C> {
    /// This line's set fields over `base`'s.
    pub fn over(&self, base: &Line<C>) -> Line<C> {
        Line {
            width_emu: self.width_emu.or(base.width_emu),
            fill: self.fill.clone().or_else(|| base.fill.clone()),
            dash: self.dash.clone().or_else(|| base.dash.clone()),
            cap: self.cap.or(base.cap),
            join: self.join.or(base.join),
            miter_limit: self.miter_limit.or(base.miter_limit),
            compound: self.compound.clone().or_else(|| base.compound.clone()),
            head: self.head.or(base.head),
            tail: self.tail.or(base.tail),
        }
    }
}

impl<C> Line<C> {
    pub fn map<D, F: FnMut(&C) -> D>(&self, f: &mut F) -> Line<D> {
        Line {
            width_emu: self.width_emu,
            fill: self.fill.as_ref().map(|x| x.map(f)),
            dash: self.dash.clone(),
            cap: self.cap,
            join: self.join,
            miter_limit: self.miter_limit,
            compound: self.compound.clone(),
            head: self.head,
            tail: self.tail,
        }
    }

    /// The width in points: 0.75 pt (9525 EMU) when unset, and a zero width (the thinnest line the
    /// device can draw) as 0.75 pt too.
    pub fn width_pt(&self) -> f64 {
        let w = self.width_emu.map_or(9525.0, |w| w as f64) / EMU_PER_PT;
        if w > 0.0 {
            w
        } else {
            0.75
        }
    }
}

impl Line<ColorSpec> {
    pub fn resolve(&self, theme: &Theme, cmap: &ColorMap, placeholder: Option<Color>) -> Line {
        self.map(&mut |c: &ColorSpec| c.resolve(theme, cmap, placeholder))
    }
}

impl Line<Color> {
    /// The line as a stroke; `None` when it has no fill (or `a:noFill`). Gradient lines use their
    /// first stop's colour and pattern lines their foreground.
    pub fn to_stroke(&self) -> Option<Stroke> {
        let color = match self.fill.as_ref()? {
            Fill::Solid(c) => *c,
            Fill::Gradient(g) => g.stops.first()?.color,
            Fill::Pattern { fg, .. } => *fg,
            _ => return None,
        };
        let w = self.width_pt();
        let mut s = Stroke::new(w, color);
        s.cap = self.cap.unwrap_or(LineCap::Butt);
        s.join = self.join.unwrap_or(LineJoin::Round);
        if let Some(m) = self.miter_limit.filter(|m| *m > 0.0) {
            s.miter_limit = m;
        }
        s.dash = self.dash.as_ref().map(|d| d.pattern().into_iter().map(|x| x * w).collect()).unwrap_or_default();
        Some(s)
    }
}

/// An `a:ln` element.
pub fn parse_line(ln: &Element) -> Line<ColorSpec> {
    let mut line: Line<ColorSpec> = Line::default();
    line.width_emu = attr_emu(ln, "w").map(|w| w.max(0));
    line.cap = match ln.attr("cap") {
        Some("rnd") => Some(LineCap::Round),
        Some("sq") => Some(LineCap::Square),
        Some("flat") => Some(LineCap::Butt),
        _ => None,
    };
    line.compound = ln.attr("cmpd").map(String::from);
    for c in ln.elements() {
        match c.local() {
            "noFill" | "solidFill" | "gradFill" | "pattFill" => line.fill = parse_fill(c),
            "prstDash" => line.dash = Some(Dash::Preset(c.attr("val").unwrap_or("solid").to_string())),
            "custDash" => {
                line.dash = Some(Dash::Custom(
                    c.children_named("ds")
                        .map(|d| (attr_pct(d, "d").unwrap_or(0.0).max(0.0), attr_pct(d, "sp").unwrap_or(0.0).max(0.0)))
                        .collect(),
                ))
            }
            "round" => line.join = Some(LineJoin::Round),
            "bevel" => line.join = Some(LineJoin::Bevel),
            "miter" => {
                line.join = Some(LineJoin::Miter);
                line.miter_limit = attr_pct(c, "lim");
            }
            "headEnd" => line.head = Some(LineEnd::parse(c)),
            "tailEnd" => line.tail = Some(LineEnd::parse(c)),
            _ => {}
        }
    }
    line
}

// ---------------------------------------------------------------------------------------------------
// Geometry: definitions
// ---------------------------------------------------------------------------------------------------

/// A path's fill mode (`fill` on `a:path`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathFill {
    None,
    Norm,
    Lighten,
    LightenLess,
    Darken,
    DarkenLess,
}

impl PathFill {
    pub fn parse(s: &str) -> PathFill {
        match s {
            "none" => PathFill::None,
            "lighten" => PathFill::Lighten,
            "lightenLess" => PathFill::LightenLess,
            "darken" => PathFill::Darken,
            "darkenLess" => PathFill::DarkenLess,
            _ => PathFill::Norm,
        }
    }
}

/// One path command; arguments are guide names or numbers, evaluated when the shape is sized.
#[derive(Debug, Clone, PartialEq)]
pub enum PathCmd {
    MoveTo(String, String),
    LineTo(String, String),
    ArcTo { wr: String, hr: String, st: String, sw: String },
    QuadTo([String; 4]),
    CubicTo([String; 6]),
    Close,
}

/// `a:path`.
#[derive(Debug, Clone, PartialEq)]
pub struct PathDef {
    /// The path's own coordinate space (0: the shape's).
    pub w: f64,
    pub h: f64,
    pub fill: PathFill,
    pub stroke: bool,
    pub extrusion_ok: bool,
    pub cmds: Vec<PathCmd>,
}

impl Default for PathDef {
    fn default() -> PathDef {
        PathDef { w: 0.0, h: 0.0, fill: PathFill::Norm, stroke: true, extrusion_ok: true, cmds: Vec::new() }
    }
}

/// A geometry definition: adjust values, guides, text rectangle and paths (a preset's, or an
/// `a:custGeom`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GeomDef {
    /// (name, formula) adjust values with their defaults.
    pub av: Vec<(String, String)>,
    /// (name, formula) guides, evaluated in order.
    pub gd: Vec<(String, String)>,
    /// l, t, r, b.
    pub text_rect: Option<[String; 4]>,
    pub paths: Vec<PathDef>,
}

fn guide_list(list: Option<&Element>) -> Vec<(String, String)> {
    list.map(|l| {
        l.children_named("gd")
            .map(|g| (g.attr("name").unwrap_or("").to_string(), g.attr("fmla").unwrap_or("val 0").to_string()))
            .collect()
    })
    .unwrap_or_default()
}

fn parse_path_el(p: &Element) -> PathDef {
    let mut d = PathDef {
        w: attr_num(p, "w").unwrap_or(0.0).max(0.0),
        h: attr_num(p, "h").unwrap_or(0.0).max(0.0),
        fill: p.attr("fill").map(PathFill::parse).unwrap_or(PathFill::Norm),
        stroke: flag(p.attr("stroke")).unwrap_or(true),
        extrusion_ok: flag(p.attr("extrusionOk")).unwrap_or(true),
        cmds: Vec::new(),
    };
    for c in p.elements() {
        let pts: Vec<(String, String)> = c
            .children_named("pt")
            .map(|pt| (pt.attr("x").unwrap_or("0").to_string(), pt.attr("y").unwrap_or("0").to_string()))
            .collect();
        let pt = |i: usize| pts.get(i).cloned().unwrap_or_else(|| ("0".to_string(), "0".to_string()));
        let a = |n: &str| c.attr(n).unwrap_or("0").to_string();
        let cmd = match c.local() {
            "moveTo" => {
                let (x, y) = pt(0);
                PathCmd::MoveTo(x, y)
            }
            "lnTo" => {
                let (x, y) = pt(0);
                PathCmd::LineTo(x, y)
            }
            "arcTo" => PathCmd::ArcTo { wr: a("wR"), hr: a("hR"), st: a("stAng"), sw: a("swAng") },
            "quadBezTo" => {
                let (x1, y1) = pt(0);
                let (x, y) = pt(1);
                PathCmd::QuadTo([x1, y1, x, y])
            }
            "cubicBezTo" => {
                let (x1, y1) = pt(0);
                let (x2, y2) = pt(1);
                let (x, y) = pt(2);
                PathCmd::CubicTo([x1, y1, x2, y2, x, y])
            }
            "close" => PathCmd::Close,
            _ => continue,
        };
        d.cmds.push(cmd);
    }
    d
}

impl GeomDef {
    /// An `a:custGeom` element.
    pub fn parse_cust_geom(el: &Element) -> GeomDef {
        GeomDef {
            av: guide_list(el.child("avLst")),
            gd: guide_list(el.child("gdLst")),
            text_rect: el.child("rect").map(|r| ["l", "t", "r", "b"].map(|k| r.attr(k).unwrap_or("0").to_string())),
            paths: el.child("pathLst").map(|pl| pl.children_named("path").map(parse_path_el).collect()).unwrap_or_default(),
        }
    }
}

/// A shape's geometry: a preset by name (with the `a:avLst` written on it), or a custom one.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    Preset { name: String, av: Vec<(String, String)> },
    Custom(GeomDef),
}

impl Geometry {
    pub fn preset(name: &str) -> Geometry {
        Geometry::Preset { name: name.to_string(), av: Vec::new() }
    }

    /// From `a:prstGeom` / `a:custGeom`, or the first of them inside an element (`a:spPr`).
    pub fn parse(el: &Element) -> Option<Geometry> {
        let is_geom = |e: &Element| matches!(e.local(), "prstGeom" | "custGeom");
        let g = if is_geom(el) { el } else { el.elements().find(|e| is_geom(e))? };
        if g.local() == "custGeom" {
            return Some(Geometry::Custom(GeomDef::parse_cust_geom(g)));
        }
        Some(Geometry::Preset { name: g.attr("prst").unwrap_or("rect").to_string(), av: guide_list(g.child("avLst")) })
    }
}

// ---------------------------------------------------------------------------------------------------
// Geometry: the guide engine
// ---------------------------------------------------------------------------------------------------

/// The built-in guides (§20.1.9.11) for a w x h shape.
fn builtin_guide(name: &str, w: f64, h: f64) -> Option<f64> {
    let ss = w.min(h);
    Some(match name {
        "w" => w,
        "h" => h,
        "l" | "t" => 0.0,
        "r" => w,
        "b" => h,
        "hc" => w / 2.0,
        "vc" => h / 2.0,
        "ss" => ss,
        "ls" => w.max(h),
        "cd2" => 10800000.0,
        "cd4" => 5400000.0,
        "cd8" => 2700000.0,
        "3cd4" => 16200000.0,
        "3cd8" => 8100000.0,
        "5cd8" => 13500000.0,
        "7cd8" => 18900000.0,
        _ => {
            let (base, d) = if let Some(d) = name.strip_prefix("ssd") {
                (ss, d)
            } else if let Some(d) = name.strip_prefix("wd") {
                (w, d)
            } else if let Some(d) = name.strip_prefix("hd") {
                (h, d)
            } else {
                return None;
            };
            let n: u32 = d.parse().ok().filter(|n| *n > 0)?;
            base / n as f64
        }
    })
}

/// Guide values for one sizing of a shape.
struct Guides {
    w: f64,
    h: f64,
    vars: HashMap<String, f64>,
}

impl Guides {
    /// A guide, a built-in or a number (anything else is 0).
    fn get(&self, tok: &str) -> f64 {
        if let Some(v) = self.vars.get(tok) {
            return *v;
        }
        if let Some(v) = builtin_guide(tok, self.w, self.h) {
            return v;
        }
        num(tok).unwrap_or(0.0)
    }

    /// A guide formula (§20.1.9.11): `*/ +- +/ ?: abs at2 cat2 cos max min mod pin sat2 sin sqrt tan
    /// val`. Angles are 60000ths of a degree; a division by zero gives 0.
    fn eval(&self, fmla: &str) -> f64 {
        let mut it = fmla.split_whitespace();
        let op = match it.next() {
            Some(o) => o,
            None => return 0.0,
        };
        let mut args = [0.0f64; 3];
        for (slot, tok) in args.iter_mut().zip(it) {
            *slot = self.get(tok);
        }
        let [x, y, z] = args;
        let rad = |v: f64| v / 60000.0 * PI / 180.0;
        let v = match op {
            "*/" => {
                if z == 0.0 {
                    0.0
                } else {
                    x * y / z
                }
            }
            "+-" => x + y - z,
            "+/" => {
                if z == 0.0 {
                    0.0
                } else {
                    (x + y) / z
                }
            }
            "?:" => {
                if x > 0.0 {
                    y
                } else {
                    z
                }
            }
            "abs" => x.abs(),
            "at2" => y.atan2(x).to_degrees() * 60000.0,
            "cat2" => x * z.atan2(y).cos(),
            "cos" => x * rad(y).cos(),
            "max" => x.max(y),
            "min" => x.min(y),
            "mod" => (x * x + y * y + z * z).sqrt(),
            "pin" => {
                if y < x {
                    x
                } else if y > z {
                    z
                } else {
                    y
                }
            }
            "sat2" => x * z.atan2(y).sin(),
            "sin" => x * rad(y).sin(),
            "sqrt" => x.max(0.0).sqrt(),
            "tan" => x * rad(y).tan(),
            "val" => x,
            bare => self.get(bare),
        };
        sane(v)
    }
}

/// Evaluate one guide formula for a w x h shape (EMU), with extra named values. For tests and for
/// callers that place adjust handles.
pub fn eval_formula(fmla: &str, w: f64, h: f64, vars: &[(String, f64)]) -> f64 {
    let g = Guides { w: sane(w), h: sane(h), vars: vars.iter().cloned().collect() };
    g.eval(fmla)
}

// ---------------------------------------------------------------------------------------------------
// Geometry: paths
// ---------------------------------------------------------------------------------------------------

/// One output path of a shape, in points in the shape's own box (0,0)-(w,h).
#[derive(Debug, Clone, PartialEq)]
pub struct GeomPath {
    pub ops: Vec<PathOp>,
    pub fill: PathFill,
    pub stroke: bool,
    pub extrusion_ok: bool,
}

/// A sized shape: its paths and its text rectangle (points).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeGeometry {
    pub paths: Vec<GeomPath>,
    pub text_rect: Rect,
}

/// `arcTo` from the current point `cur`: the point lies on the ellipse (radii `wr`, `hr`) at the
/// visual angle `st` (degrees, clockwise, y down), and the arc sweeps `sw` degrees. Visual angles
/// are turned into the ellipse's parametric angles; the arc is drawn as cubic Béziers of at most
/// 90 degrees each.
fn arc_to(ops: &mut Vec<PathOp>, cur: &mut (f64, f64), wr: f64, hr: f64, st: f64, sw: f64) {
    let (wr, hr, st, sw) = (sane(wr), sane(hr), sane(st), sane(sw));
    if sw == 0.0 {
        return;
    }
    let param = |a: f64| (wr * a.sin()).atan2(hr * a.cos());
    let a1 = st.to_radians();
    let swr = sw.to_radians();
    let t1 = param(a1);
    let d = if swr.abs() >= 2.0 * PI - 1e-9 {
        2.0 * PI * swr.signum()
    } else {
        let t2 = param(a1 + swr);
        let mut d = t2 - t1;
        if swr > 0.0 && d < 0.0 {
            d += 2.0 * PI;
        } else if swr < 0.0 && d > 0.0 {
            d -= 2.0 * PI;
        }
        d
    };
    let (cx, cy) = (cur.0 - wr * t1.cos(), cur.1 - hr * t1.sin());
    let n = ((d.abs() / (PI / 2.0)).ceil() as usize).clamp(1, 4);
    let step = d / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let mut ta = t1;
    for _ in 0..n {
        let tb = ta + step;
        let (p0x, p0y) = (cx + wr * ta.cos(), cy + hr * ta.sin());
        let (p3x, p3y) = (cx + wr * tb.cos(), cy + hr * tb.sin());
        let c1 = (p0x - k * wr * ta.sin(), p0y + k * hr * ta.cos());
        let c2 = (p3x + k * wr * tb.sin(), p3y - k * hr * tb.cos());
        ops.push(PathOp::CubicTo(c1.0, c1.1, c2.0, c2.1, p3x, p3y));
        ta = tb;
    }
    *cur = (cx + wr * ta.cos(), cy + hr * ta.sin());
}

fn scale_op(op: PathOp, kx: f64, ky: f64) -> PathOp {
    let f = |v: f64| sane(v);
    match op {
        PathOp::MoveTo(x, y) => PathOp::MoveTo(f(x * kx), f(y * ky)),
        PathOp::LineTo(x, y) => PathOp::LineTo(f(x * kx), f(y * ky)),
        PathOp::QuadTo(a, b, x, y) => PathOp::QuadTo(f(a * kx), f(b * ky), f(x * kx), f(y * ky)),
        PathOp::CubicTo(a, b, c, d, x, y) => PathOp::CubicTo(f(a * kx), f(b * ky), f(c * kx), f(d * ky), f(x * kx), f(y * ky)),
        PathOp::Close => PathOp::Close,
    }
}

fn build_path(p: &PathDef, g: &Guides) -> GeomPath {
    let sx = if p.w > 0.0 { g.w / p.w } else { 1.0 };
    let sy = if p.h > 0.0 { g.h / p.h } else { 1.0 };
    // Everything is drawn in the path's own space (arcs included, so a path stretches as a
    // whole), then scaled to the shape and to points.
    let pt = |x: &str, y: &str| (g.get(x), g.get(y));
    let mut ops = Vec::new();
    let mut cur = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    let mut open = false;
    let begin = |ops: &mut Vec<PathOp>, open: &mut bool, start: &mut (f64, f64), cur: (f64, f64)| {
        if !*open {
            ops.push(PathOp::MoveTo(cur.0, cur.1));
            *start = cur;
            *open = true;
        }
    };
    for c in &p.cmds {
        match c {
            PathCmd::MoveTo(x, y) => {
                let q = pt(x, y);
                ops.push(PathOp::MoveTo(q.0, q.1));
                cur = q;
                start = q;
                open = true;
            }
            PathCmd::LineTo(x, y) => {
                begin(&mut ops, &mut open, &mut start, cur);
                let q = pt(x, y);
                ops.push(PathOp::LineTo(q.0, q.1));
                cur = q;
            }
            PathCmd::ArcTo { wr, hr, st, sw } => {
                begin(&mut ops, &mut open, &mut start, cur);
                arc_to(&mut ops, &mut cur, g.get(wr), g.get(hr), g.get(st) / 60000.0, g.get(sw) / 60000.0);
            }
            PathCmd::QuadTo(a) => {
                begin(&mut ops, &mut open, &mut start, cur);
                let p1 = pt(&a[0], &a[1]);
                let p2 = pt(&a[2], &a[3]);
                ops.push(PathOp::QuadTo(p1.0, p1.1, p2.0, p2.1));
                cur = p2;
            }
            PathCmd::CubicTo(a) => {
                begin(&mut ops, &mut open, &mut start, cur);
                let p1 = pt(&a[0], &a[1]);
                let p2 = pt(&a[2], &a[3]);
                let p3 = pt(&a[4], &a[5]);
                ops.push(PathOp::CubicTo(p1.0, p1.1, p2.0, p2.1, p3.0, p3.1));
                cur = p3;
            }
            PathCmd::Close => {
                if open {
                    ops.push(PathOp::Close);
                    cur = start;
                    open = false;
                }
            }
        }
    }
    let k = 1.0 / EMU_PER_PT;
    GeomPath { ops: ops.into_iter().map(|o| scale_op(o, sx * k, sy * k)).collect(), fill: p.fill, stroke: p.stroke, extrusion_ok: p.extrusion_ok }
}

fn evaluate(def: &GeomDef, w: f64, h: f64, av_over: &[(String, String)], adjust: &[(String, f64)]) -> ShapeGeometry {
    let w = sane(w).max(0.0);
    let h = sane(h).max(0.0);
    let mut g = Guides { w, h, vars: HashMap::new() };
    for (name, fmla) in &def.av {
        let v = if let Some((_, v)) = adjust.iter().find(|(n, _)| n == name) {
            sane(*v)
        } else if let Some((_, f)) = av_over.iter().find(|(n, _)| n == name) {
            g.eval(f)
        } else {
            g.eval(fmla)
        };
        g.vars.insert(name.clone(), v);
    }
    for (name, fmla) in &def.gd {
        let v = g.eval(fmla);
        g.vars.insert(name.clone(), v);
    }
    let paths = def.paths.iter().map(|p| build_path(p, &g)).collect();
    let k = 1.0 / EMU_PER_PT;
    let text_rect = match &def.text_rect {
        Some([l, t, r, b]) => Rect::from_ltrb(g.get(l) * k, g.get(t) * k, g.get(r) * k, g.get(b) * k),
        None => Rect::new(0.0, 0.0, w * k, h * k),
    };
    ShapeGeometry { paths, text_rect }
}

/// The shape's paths for a `w_emu` x `h_emu` box, in points. Adjust values come from the preset's
/// defaults, then the geometry's own `avLst`, then `adjust`. An unknown preset draws as `rect`.
pub fn geometry(geom: &Geometry, w_emu: f64, h_emu: f64, adjust: &[(String, f64)]) -> Vec<GeomPath> {
    shape_geometry(geom, w_emu, h_emu, adjust).paths
}

/// [`geometry`] with the text rectangle.
pub fn shape_geometry(geom: &Geometry, w_emu: f64, h_emu: f64, adjust: &[(String, f64)]) -> ShapeGeometry {
    match geom {
        Geometry::Preset { name, av } => {
            let p = presets();
            match p.get(name.as_str()).or_else(|| p.get("rect")) {
                Some(def) => evaluate(def, w_emu, h_emu, av, adjust),
                None => ShapeGeometry { paths: Vec::new(), text_rect: Rect::new(0.0, 0.0, 0.0, 0.0) },
            }
        }
        Geometry::Custom(def) => evaluate(def, w_emu, h_emu, &[], adjust),
    }
}

/// A preset's paths by name, or `None` for a name this module does not know.
pub fn preset_geometry(name: &str, w_emu: f64, h_emu: f64, adjust: &[(String, f64)]) -> Option<Vec<GeomPath>> {
    presets().get(name).map(|def| evaluate(def, w_emu, h_emu, &[], adjust).paths)
}

/// The preset names this module draws exactly (sorted).
pub fn preset_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = presets().keys().map(|k| k.as_str()).collect();
    v.sort_unstable();
    v
}

/// A preset's definition (adjust defaults, guides, paths).
pub fn preset_def(name: &str) -> Option<&'static GeomDef> {
    presets().get(name)
}

fn presets() -> &'static HashMap<String, GeomDef> {
    static P: OnceLock<HashMap<String, GeomDef>> = OnceLock::new();
    P.get_or_init(|| parse_presets(&preset_source()))
}

/// Reads the compact preset notation below: `@name [alias..]` starts a shape; `av NAME FMLA`,
/// `gd NAME FMLA`, `tx L T R B`; `path [w=..] [h=..] [fill=..] [stroke=0] [ext=0]` starts a path;
/// `M x y`, `L x y`, `A wR hR stAng swAng`, `Q x1 y1 x y`, `C x1 y1 x2 y2 x y`, `Z`. Statements end
/// at a newline or `;`.
fn parse_presets(src: &str) -> HashMap<String, GeomDef> {
    fn flush(names: &mut Vec<String>, def: &mut GeomDef, map: &mut HashMap<String, GeomDef>) {
        for n in names.drain(..) {
            map.insert(n, def.clone());
        }
        *def = GeomDef::default();
    }
    let mut map = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    let mut def = GeomDef::default();
    for stmt in src.split(|c| c == '\n' || c == ';') {
        let mut toks = stmt.split_whitespace();
        let head = match toks.next() {
            Some(h) => h,
            None => continue,
        };
        if let Some(name) = head.strip_prefix('@') {
            flush(&mut names, &mut def, &mut map);
            names.push(name.to_string());
            names.extend(toks.map(String::from));
            continue;
        }
        let rest: Vec<&str> = toks.collect();
        let s = |i: usize| rest.get(i).copied().unwrap_or("0").to_string();
        match head {
            "av" | "gd" => {
                if rest.is_empty() {
                    continue;
                }
                let entry = (rest[0].to_string(), rest[1..].join(" "));
                if head == "av" {
                    def.av.push(entry);
                } else {
                    def.gd.push(entry);
                }
            }
            "tx" => def.text_rect = Some([s(0), s(1), s(2), s(3)]),
            "path" => {
                let mut p = PathDef::default();
                for opt in &rest {
                    if let Some((k, v)) = opt.split_once('=') {
                        match k {
                            "w" => p.w = num(v).unwrap_or(0.0),
                            "h" => p.h = num(v).unwrap_or(0.0),
                            "fill" => p.fill = PathFill::parse(v),
                            "stroke" => p.stroke = flag(Some(v)).unwrap_or(true),
                            "ext" => p.extrusion_ok = flag(Some(v)).unwrap_or(true),
                            _ => {}
                        }
                    }
                }
                def.paths.push(p);
            }
            cmd => {
                let c = match cmd {
                    "M" => PathCmd::MoveTo(s(0), s(1)),
                    "L" => PathCmd::LineTo(s(0), s(1)),
                    "A" => PathCmd::ArcTo { wr: s(0), hr: s(1), st: s(2), sw: s(3) },
                    "Q" => PathCmd::QuadTo([s(0), s(1), s(2), s(3)]),
                    "C" => PathCmd::CubicTo([s(0), s(1), s(2), s(3), s(4), s(5)]),
                    "Z" => PathCmd::Close,
                    _ => continue,
                };
                if def.paths.is_empty() {
                    def.paths.push(PathDef::default());
                }
                if let Some(p) = def.paths.last_mut() {
                    p.cmds.push(c);
                }
            }
        }
    }
    flush(&mut names, &mut def, &mut map);
    map
}

// The inscribed-rectangle guides of an ellipse (text rect of ellipse, donut, noSmoking, ...).
macro_rules! ellipse_text_guides {
    () => {
        "gd idx cos wd2 2700000\ngd idy sin hd2 2700000\ngd il +- hc 0 idx\ngd ir +- hc idx 0\ngd it +- vc 0 idy\ngd ib +- vc idy 0\n"
    };
}

// The full ellipse as four arcs (a closed subpath).
macro_rules! ellipse_path {
    () => {
        "M l vc; A wd2 hd2 cd2 cd4; A wd2 hd2 3cd4 cd4; A wd2 hd2 0 cd4; A wd2 hd2 cd4 cd4; Z\n"
    };
}

// The pointer guides shared by wedgeRectCallout and wedgeRoundRectCallout.
macro_rules! wedge_guides {
    () => {
        concat!(
            "av adj1 -20833\nav adj2 62500\n",
            "gd dxPos */ w adj1 100000\ngd dyPos */ h adj2 100000\ngd xPos +- hc dxPos 0\ngd yPos +- vc dyPos 0\n",
            "gd dx */ dxPos h 1\ngd dy */ dyPos w 1\ngd adx abs dx\ngd ady abs dy\ngd dq +- adx 0 ady\n",
            "gd xg1 ?: dxPos 7 2\ngd xg2 ?: dxPos 10 5\ngd x1 */ w xg1 12\ngd x2 */ w xg2 12\n",
            "gd yg1 ?: dyPos 7 2\ngd yg2 ?: dyPos 10 5\ngd y1 */ h yg1 12\ngd y2 */ h yg2 12\n",
            "gd t1 ?: dxPos l xPos\ngd xl ?: dq t1 l\ngd t2 ?: dyPos x1 xPos\ngd xt ?: dq x1 t2\n",
            "gd t3 ?: dxPos xPos r\ngd xr ?: dq t3 r\ngd t4 ?: dyPos xPos x1\ngd xb ?: dq x1 t4\n",
            "gd t5 ?: dxPos y1 yPos\ngd yl ?: dq t5 y1\ngd t6 ?: dyPos t yPos\ngd yt ?: dq t t6\n",
            "gd t7 ?: dxPos yPos y1\ngd yr ?: dq t7 y1\ngd t8 ?: dyPos yPos b\ngd yb ?: dq b t8\n"
        )
    };
}

// The cloud outline in its 43200 x 43200 path space (approximate, see the module notes).
macro_rules! cloud_path {
    () => {
        concat!(
            "path w=43200 h=43200\n",
            "M 3900 14370; A 6753 9190 -11429249 7426832; A 5333 7267 -8646143 5396714; ",
            "A 4365 5945 -8748475 5983381; A 4857 6595 -7859164 7034504; A 5333 7273 -4722533 6541615; ",
            "A 6775 9220 -2776035 7816140; A 5785 7867 37501 6842000; A 6752 9215 1347096 6910353; ",
            "A 7720 10543 3974558 4542661; A 4360 5918 -16496525 8804134; A 4345 5945 -14809710 9151131; Z\n"
        )
    };
}

// Start point and sweep of an arc between two adjust angles (arc, pie, chord, blockArc).
macro_rules! arc_start_guides {
    () => {
        concat!(
            "gd sw11 +- enAng 0 stAng\ngd sw12 +- sw11 21600000 0\ngd swAng ?: sw11 sw11 sw12\n",
            "gd wt1 sin wd2 stAng\ngd ht1 cos hd2 stAng\ngd dx1 cat2 wd2 ht1 wt1\ngd dy1 sat2 hd2 ht1 wt1\n",
            "gd x1 +- hc dx1 0\ngd y1 +- vc dy1 0\n"
        )
    };
}

/// The preset shapes, transcribed from the spec's presetShapeDefinitions.xml.
fn preset_source() -> String {
    let parts: &[&str] = &[
        "@rect\ntx l t r b\npath\nM l t; L r t; L r b; L l b; Z\n",
        concat!(
            "@roundRect\nav adj 16667\ngd a pin 0 adj 50000\ngd x1 */ ss a 100000\ngd x2 +- r 0 x1\ngd y2 +- b 0 x1\n",
            "gd il */ x1 29289 100000\ngd ir +- r 0 il\ngd ib +- b 0 il\ntx il il ir ib\npath\n",
            "M l x1; A x1 x1 cd2 cd4; L x2 t; A x1 x1 3cd4 cd4; L r y2; A x1 x1 0 cd4; L x1 b; A x1 x1 cd4 cd4; Z\n"
        ),
        concat!("@ellipse\n", ellipse_text_guides!(), "tx il it ir ib\npath\n", ellipse_path!()),
        concat!(
            "@triangle\nav adj 50000\ngd a pin 0 adj 100000\ngd x1 */ w a 200000\ngd x2 */ w a 100000\n",
            "gd x3 +- x1 wd2 0\ntx x1 vc x3 b\npath\nM l b; L x2 t; L r b; Z\n"
        ),
        "@rtTriangle\ngd it */ h 7 12\ngd ir */ w 7 12\ngd ib */ h 11 12\ntx wd12 it ir ib\npath\nM l b; L l t; L r b; Z\n",
        concat!(
            "@parallelogram\nav adj 25000\ngd maxAdj */ 100000 w ss\ngd a pin 0 adj maxAdj\ngd x1 */ ss a 200000\n",
            "gd x2 */ ss a 100000\ngd x6 +- r 0 x2\ngd x5 +- r 0 x1\ngd q1 */ 5 a maxAdj\ngd q2 +/ 1 q1 12\n",
            "gd il */ q2 w 1\ngd it */ q2 h 1\ngd ir +- r 0 il\ngd ib +- b 0 it\ntx il it ir ib\n",
            "path\nM l b; L x2 t; L r t; L x6 b; Z\n"
        ),
        concat!(
            "@trapezoid\nav adj 25000\ngd maxAdj */ 50000 w ss\ngd a pin 0 adj maxAdj\ngd x1 */ ss a 200000\n",
            "gd x2 */ ss a 100000\ngd x3 +- r 0 x2\ngd x4 +- r 0 x1\ngd il */ wd3 a maxAdj\ngd it */ hd3 a maxAdj\n",
            "gd ir +- r 0 il\ntx il it ir b\npath\nM l b; L x2 t; L x3 t; L r b; Z\n"
        ),
        "@diamond\ngd ir */ w 3 4\ngd ib */ h 3 4\ntx wd4 hd4 ir ib\npath\nM l vc; L hc t; L r vc; L hc b; Z\n",
        concat!(
            "@pentagon\nav hf 105146\nav vf 110557\ngd swd2 */ wd2 hf 100000\ngd shd2 */ hd2 vf 100000\n",
            "gd svc */ vc vf 100000\ngd dx1 cos swd2 1080000\ngd dx2 cos swd2 18360000\ngd dy1 sin shd2 1080000\n",
            "gd dy2 sin shd2 18360000\ngd x1 +- hc 0 dx1\ngd x2 +- hc 0 dx2\ngd x3 +- hc dx2 0\ngd x4 +- hc dx1 0\n",
            "gd y1 +- svc 0 dy1\ngd y2 +- svc 0 dy2\ngd it */ y1 dx2 dx1\ntx x2 it x3 y2\n",
            "path\nM x1 y1; L hc t; L x4 y1; L x3 y2; L x2 y2; Z\n"
        ),
        concat!(
            "@hexagon\nav adj 25000\nav vf 115470\ngd maxAdj */ 50000 w ss\ngd a pin 0 adj maxAdj\n",
            "gd shd2 */ hd2 vf 100000\ngd x1 */ ss a 100000\ngd x2 +- r 0 x1\ngd dy1 sin shd2 3600000\n",
            "gd y1 +- vc 0 dy1\ngd y2 +- vc dy1 0\ntx x1 y1 x2 y2\n",
            "path\nM l vc; L x1 y1; L x2 y1; L r vc; L x2 y2; L x1 y2; Z\n"
        ),
        concat!(
            "@octagon\nav adj 29289\ngd a pin 0 adj 50000\ngd x1 */ ss a 100000\ngd x2 +- r 0 x1\ngd y2 +- b 0 x1\n",
            "gd il */ x1 1 2\ngd ir +- r 0 il\ngd ib +- b 0 il\ntx il il ir ib\n",
            "path\nM l x1; L x1 t; L x2 t; L r x1; L r y2; L x2 b; L x1 b; L l y2; Z\n"
        ),
        concat!(
            "@plus\nav adj 25000\ngd a pin 0 adj 50000\ngd x1 */ ss a 100000\ngd x2 +- r 0 x1\ngd y2 +- b 0 x1\n",
            "gd d +- w 0 h\ngd il ?: d l x1\ngd ir ?: d r x2\ngd it ?: d x1 t\ngd ib ?: d y2 b\ntx il it ir ib\n",
            "path\nM l x1; L x1 x1; L x1 t; L x2 t; L x2 x1; L r x1; L r y2; L x2 y2; L x2 b; L x1 b; L x1 y2; L l y2; Z\n"
        ),
        concat!(
            "@star4\nav adj 12500\ngd a pin 0 adj 50000\ngd iwd2 */ wd2 a 50000\ngd ihd2 */ hd2 a 50000\n",
            "gd sdx cos iwd2 2700000\ngd sdy sin ihd2 2700000\ngd sx1 +- hc 0 sdx\ngd sx2 +- hc sdx 0\n",
            "gd sy1 +- vc 0 sdy\ngd sy2 +- vc sdy 0\ngd yAdj +- vc 0 ihd2\ntx sx1 sy1 sx2 sy2\n",
            "path\nM l vc; L sx1 sy1; L hc t; L sx2 sy1; L r vc; L sx2 sy2; L hc b; L sx1 sy2; Z\n"
        ),
        concat!(
            "@star5\nav adj 19098\nav hf 105146\nav vf 110557\ngd a pin 0 adj 50000\ngd swd2 */ wd2 hf 100000\n",
            "gd shd2 */ hd2 vf 100000\ngd svc */ vc vf 100000\ngd dx1 cos swd2 1080000\ngd dx2 cos swd2 18360000\n",
            "gd dy1 sin shd2 1080000\ngd dy2 sin shd2 18360000\ngd x1 +- hc 0 dx1\ngd x2 +- hc 0 dx2\n",
            "gd x3 +- hc dx2 0\ngd x4 +- hc dx1 0\ngd y1 +- svc 0 dy1\ngd y2 +- svc 0 dy2\n",
            "gd iwd2 */ swd2 a 50000\ngd ihd2 */ shd2 a 50000\ngd sdx1 cos iwd2 20520000\ngd sdx2 cos iwd2 3240000\n",
            "gd sdy1 sin ihd2 3240000\ngd sdy2 sin ihd2 20520000\ngd sx1 +- hc 0 sdx1\ngd sx2 +- hc 0 sdx2\n",
            "gd sx3 +- hc sdx2 0\ngd sx4 +- hc sdx1 0\ngd sy1 +- svc 0 sdy1\ngd sy2 +- svc 0 sdy2\n",
            "gd sy3 +- svc ihd2 0\ngd yAdj +- svc 0 ihd2\ntx sx1 sy1 sx4 sy3\n",
            "path\nM x1 y1; L sx2 sy1; L hc t; L sx3 sy1; L x4 y1; L sx4 sy2; L x3 y2; L hc sy3; L x2 y2; L sx1 sy2; Z\n"
        ),
        concat!(
            "@star6\nav adj 28868\nav hf 115470\ngd a pin 0 adj 50000\ngd swd2 */ wd2 hf 100000\n",
            "gd dx1 cos swd2 1800000\ngd x1 +- hc 0 dx1\ngd x2 +- hc dx1 0\ngd y2 +- vc hd4 0\n",
            "gd iwd2 */ swd2 a 50000\ngd ihd2 */ hd2 a 50000\ngd sdx2 */ iwd2 1 2\ngd sx1 +- hc 0 iwd2\n",
            "gd sx2 +- hc 0 sdx2\ngd sx3 +- hc sdx2 0\ngd sx4 +- hc iwd2 0\ngd sdy1 sin ihd2 3600000\n",
            "gd sy1 +- vc 0 sdy1\ngd sy2 +- vc sdy1 0\ngd yAdj +- vc 0 ihd2\ntx sx1 sy1 sx4 sy2\n",
            "path\nM x1 hd4; L sx2 sy1; L hc t; L sx3 sy1; L x2 hd4; L sx4 vc; L x2 y2; L sx3 sy2; L hc b; L sx2 sy2; L x1 y2; L sx1 vc; Z\n"
        ),
        concat!(
            "@rightArrow\nav adj1 50000\nav adj2 50000\ngd maxAdj2 */ 100000 w ss\ngd a1 pin 0 adj1 100000\n",
            "gd a2 pin 0 adj2 maxAdj2\ngd dx1 */ ss a2 100000\ngd x1 +- r 0 dx1\ngd dy1 */ h a1 200000\n",
            "gd y1 +- vc 0 dy1\ngd y2 +- vc dy1 0\ngd dx2 */ y1 dx1 hd2\ngd x2 +- x1 dx2 0\ntx l y1 x2 y2\n",
            "path\nM l y1; L x1 y1; L x1 t; L r vc; L x1 b; L x1 y2; L l y2; Z\n"
        ),
        concat!(
            "@leftArrow\nav adj1 50000\nav adj2 50000\ngd maxAdj2 */ 100000 w ss\ngd a1 pin 0 adj1 100000\n",
            "gd a2 pin 0 adj2 maxAdj2\ngd dx2 */ ss a2 100000\ngd x2 +- l dx2 0\ngd dy1 */ h a1 200000\n",
            "gd y1 +- vc 0 dy1\ngd y2 +- vc dy1 0\ngd dx1 */ y1 dx2 hd2\ngd x1 +- x2 0 dx1\ntx x1 y1 r y2\n",
            "path\nM l vc; L x2 t; L x2 y1; L r y1; L r y2; L x2 y2; L x2 b; Z\n"
        ),
        concat!(
            "@upArrow\nav adj1 50000\nav adj2 50000\ngd maxAdj2 */ 100000 h ss\ngd a1 pin 0 adj1 100000\n",
            "gd a2 pin 0 adj2 maxAdj2\ngd dy2 */ ss a2 100000\ngd y2 +- t dy2 0\ngd dx1 */ w a1 200000\n",
            "gd x1 +- hc 0 dx1\ngd x2 +- hc dx1 0\ngd dy1 */ x1 dy2 wd2\ngd y1 +- y2 0 dy1\ntx x1 y1 x2 b\n",
            "path\nM l y2; L hc t; L r y2; L x2 y2; L x2 b; L x1 b; L x1 y2; Z\n"
        ),
        concat!(
            "@downArrow\nav adj1 50000\nav adj2 50000\ngd maxAdj2 */ 100000 h ss\ngd a1 pin 0 adj1 100000\n",
            "gd a2 pin 0 adj2 maxAdj2\ngd dy1 */ ss a2 100000\ngd y1 +- b 0 dy1\ngd dx1 */ w a1 200000\n",
            "gd x1 +- hc 0 dx1\ngd x2 +- hc dx1 0\ngd dy2 */ x1 dy1 wd2\ngd y2 +- y1 dy2 0\ntx x1 t x2 y2\n",
            "path\nM l y1; L x1 y1; L x1 t; L x2 t; L x2 y1; L r y1; L hc b; Z\n"
        ),
        concat!(
            "@leftRightArrow\nav adj1 50000\nav adj2 50000\ngd maxAdj2 */ 50000 w ss\ngd a1 pin 0 adj1 100000\n",
            "gd a2 pin 0 adj2 maxAdj2\ngd x2 */ ss a2 100000\ngd x3 +- r 0 x2\ngd dy */ h a1 200000\n",
            "gd y1 +- vc 0 dy\ngd y2 +- vc dy 0\ngd dx1 */ y1 x2 hd2\ngd x1 +- x2 0 dx1\ngd x4 +- x3 dx1 0\n",
            "tx x1 y1 x4 y2\npath\nM l vc; L x2 t; L x2 y1; L x3 y1; L x3 t; L r vc; L x3 b; L x3 y2; L x2 y2; L x2 b; Z\n"
        ),
        concat!(
            "@chevron\nav adj 50000\ngd maxAdj */ 100000 w ss\ngd a pin 0 adj maxAdj\ngd x1 */ ss a 100000\n",
            "gd x2 +- r 0 x1\ngd x3 */ x2 1 2\ngd dx +- x2 0 x1\ngd il ?: dx x1 l\ngd ir ?: dx x2 r\ntx il t ir b\n",
            "path\nM l t; L x2 t; L r vc; L x2 b; L l b; L x1 vc; Z\n"
        ),
        concat!(
            "@homePlate\nav adj 50000\ngd maxAdj */ 100000 w ss\ngd a pin 0 adj maxAdj\ngd dx1 */ ss a 100000\n",
            "gd x1 +- r 0 dx1\ngd ir +/ x1 r 2\ngd x2 */ x1 1 2\ntx l t ir b\n",
            "path\nM l t; L x1 t; L r vc; L x1 b; L l b; Z\n"
        ),
        "@line straightConnector1\npath fill=none\nM l t; L r b\n",
        "@bentConnector2\npath fill=none\nM l t; L r t; L r b\n",
        "@bentConnector3\nav adj1 50000\ngd x1 */ w adj1 100000\npath fill=none\nM l t; L x1 t; L x1 b; L r b\n",
        concat!(
            "@curvedConnector3\nav adj1 50000\ngd x2 */ w adj1 100000\ngd x1 +/ l x2 2\ngd x3 +/ r x2 2\ngd y3 */ h 3 4\n",
            "path fill=none\nM l t; C x1 t x2 hd4 x2 vc; C x2 y3 x3 b r b\n"
        ),
        concat!(
            "@wedgeRectCallout\n",
            wedge_guides!(),
            "path\nM l t; L x1 t; L xt yt; L x2 t; L r t; L r y1; L xr yr; L r y2; L r b; L x2 b; L xb yb; L x1 b; L l b; L l y2; L xl yl; L l y1; Z\n"
        ),
        concat!(
            "@wedgeRoundRectCallout\n",
            wedge_guides!(),
            "av adj3 16667\ngd u1 */ ss adj3 100000\ngd u2 +- r 0 u1\ngd v2 +- b 0 u1\n",
            "gd il */ u1 29289 100000\ngd ir +- r 0 il\ngd ib +- b 0 il\ntx il il ir ib\n",
            "path\nM l u1; A u1 u1 cd2 cd4; L x1 t; L xt yt; L x2 t; L u2 t; A u1 u1 3cd4 cd4; L r y1; L xr yr; L r y2; ",
            "L r v2; A u1 u1 0 cd4; L x2 b; L xb yb; L x1 b; L u1 b; A u1 u1 cd4 cd4; L l y2; L xl yl; L l y1; Z\n"
        ),
        concat!(
            "@wedgeEllipseCallout\nav adj1 -20833\nav adj2 62500\ngd dxPos */ w adj1 100000\ngd dyPos */ h adj2 100000\n",
            "gd xPos +- hc dxPos 0\ngd yPos +- vc dyPos 0\ngd sdx */ dxPos h 1\ngd sdy */ dyPos w 1\ngd pang at2 sdx sdy\n",
            "gd stAng +- pang 660000 0\ngd enAng +- pang 0 660000\ngd dx1 cos wd2 stAng\ngd dy1 sin hd2 stAng\n",
            "gd x1 +- hc dx1 0\ngd y1 +- vc dy1 0\ngd dx2 cos wd2 enAng\ngd dy2 sin hd2 enAng\ngd x2 +- hc dx2 0\n",
            "gd y2 +- vc dy2 0\ngd stAng1 at2 dx1 dy1\ngd enAng1 at2 dx2 dy2\ngd swAng1 +- enAng1 0 stAng1\n",
            "gd swAng2 +- swAng1 21600000 0\ngd swAng ?: swAng1 swAng1 swAng2\n",
            ellipse_text_guides!(),
            "tx il it ir ib\npath\nM xPos yPos; L x1 y1; A wd2 hd2 stAng1 swAng; Z\n"
        ),
        concat!("@cloud\ngd il */ w 2977 21600\ngd it */ h 3262 21600\ngd ir */ w 17087 21600\ngd ib */ h 17337 21600\ntx il it ir ib\n", cloud_path!()),
        concat!(
            "@cloudCallout\nav adj1 -20833\nav adj2 62500\ngd dxPos */ w adj1 100000\ngd dyPos */ h adj2 100000\n",
            "gd xPos +- hc dxPos 0\ngd yPos +- vc dyPos 0\ngd ht cat2 hd2 dxPos dyPos\ngd wt sat2 wd2 dxPos dyPos\n",
            "gd g2 cat2 wd2 ht wt\ngd g3 sat2 hd2 ht wt\ngd g4 +- hc g2 0\ngd g5 +- vc g3 0\ngd g6 +- g4 0 xPos\n",
            "gd g7 +- g5 0 yPos\ngd g8 mod g6 g7 0\ngd g9 */ ss 6600 21600\ngd g10 +- g8 0 g9\ngd g11 */ g10 1 3\n",
            "gd g12 */ ss 1800 21600\ngd g13 +- g11 g12 0\ngd g14 */ g13 g6 g8\ngd g15 */ g13 g7 g8\n",
            "gd g16 +- g14 xPos 0\ngd g17 +- g15 yPos 0\ngd g18 */ ss 4800 21600\ngd g19 */ g11 2 1\n",
            "gd g20 +- g18 g19 0\ngd g21 */ g20 g6 g8\ngd g22 */ g20 g7 g8\ngd g23 +- g21 xPos 0\ngd g24 +- g22 yPos 0\n",
            "gd g25 */ ss 1200 21600\ngd g26 */ ss 600 21600\ngd x23 +- xPos g26 0\ngd x24 +- g16 g25 0\n",
            "gd x25 +- g23 g12 0\ngd il */ w 2977 21600\ngd it */ h 3262 21600\ngd ir */ w 17087 21600\n",
            "gd ib */ h 17337 21600\ntx il it ir ib\n",
            cloud_path!(),
            "path\nM x23 yPos; A g26 g26 0 21600000; Z\n",
            "path\nM x24 g17; A g25 g25 0 21600000; Z\n",
            "path\nM x25 g24; A g12 g12 0 21600000; Z\n"
        ),
        concat!(
            "@heart\ngd dx1 */ w 49 48\ngd dx2 */ w 10 48\ngd x1 +- hc 0 dx1\ngd x2 +- hc 0 dx2\ngd x3 +- hc dx2 0\n",
            "gd x4 +- hc dx1 0\ngd y1 +- t 0 hd3\ngd il */ w 1 6\ngd ir */ w 5 6\ngd ib */ h 2 3\ntx il hd4 ir ib\n",
            "path\nM hc hd4; C x3 y1 x4 hd4 hc b; C x1 hd4 x2 y1 hc hd4; Z\n"
        ),
        concat!(
            "@can\nav adj 25000\ngd maxAdj */ 50000 h ss\ngd a pin 0 adj maxAdj\ngd y1 */ ss a 200000\ngd y2 +- y1 y1 0\n",
            "gd y3 +- b 0 y1\ntx l y2 r y3\n",
            "path stroke=0 ext=0\nM l y1; A wd2 y1 cd2 -10800000; L r y3; A wd2 y1 0 cd2; Z\n",
            "path fill=lighten stroke=0 ext=0\nM l y1; A wd2 y1 cd2 cd2; A wd2 y1 0 cd2; Z\n",
            "path fill=none\nM r y1; A wd2 y1 0 cd2; A wd2 y1 cd2 cd2; L r y3; A wd2 y1 0 cd2; L l y1\n"
        ),
        concat!(
            "@cube\nav adj 25000\ngd a pin 0 adj 100000\ngd y1 */ ss a 100000\ngd y4 +- b 0 y1\ngd y2 */ y4 1 2\n",
            "gd y3 +/ y1 b 2\ngd x4 +- r 0 y1\ngd x2 */ x4 1 2\ngd x3 +/ y1 r 2\ntx l y1 x4 b\n",
            "path stroke=0 ext=0\nM l y1; L x4 y1; L x4 b; L l b; Z\n",
            "path fill=darkenLess stroke=0 ext=0\nM x4 y1; L r t; L r y4; L x4 b; Z\n",
            "path fill=lightenLess stroke=0 ext=0\nM l y1; L y1 t; L r t; L x4 y1; Z\n",
            "path fill=none ext=0\nM l y1; L y1 t; L r t; L r y4; L x4 b; L l b; Z; M l y1; L x4 y1; L r t; M x4 y1; L x4 b\n"
        ),
        concat!(
            "@donut\nav adj 25000\ngd a pin 0 adj 50000\ngd dr */ ss a 100000\ngd iwd2 +- wd2 0 dr\ngd ihd2 +- hd2 0 dr\n",
            ellipse_text_guides!(),
            "tx il it ir ib\npath\n",
            ellipse_path!(),
            "M dr vc; A iwd2 ihd2 cd2 -5400000; A iwd2 ihd2 cd4 -5400000; A iwd2 ihd2 0 -5400000; A iwd2 ihd2 3cd4 -5400000; Z\n"
        ),
        concat!(
            "@noSmoking\nav adj 18750\ngd a pin 0 adj 50000\ngd dr */ ss a 100000\ngd iwd2 +- wd2 0 dr\ngd ihd2 +- hd2 0 dr\n",
            "gd ang at2 w h\ngd ct cos ihd2 ang\ngd st sin iwd2 ang\ngd m mod ct st 0\ngd n */ iwd2 ihd2 m\n",
            "gd drd2 */ dr 1 2\ngd dang at2 n drd2\ngd dang2 */ dang 2 1\ngd swAng +- -10800000 dang2 0\n",
            "gd t3 at2 w h\ngd stAng1 +- t3 0 dang\ngd stAng2 +- stAng1 0 cd2\ngd ct1 cos ihd2 stAng1\n",
            "gd st1 sin iwd2 stAng1\ngd m1 mod ct1 st1 0\ngd n1 */ iwd2 ihd2 m1\ngd dx1 cos n1 stAng1\n",
            "gd dy1 sin n1 stAng1\ngd x1 +- hc dx1 0\ngd y1 +- vc dy1 0\ngd x2 +- hc 0 dx1\ngd y2 +- vc 0 dy1\n",
            ellipse_text_guides!(),
            "tx il it ir ib\npath\n",
            ellipse_path!(),
            "M x1 y1; A iwd2 ihd2 stAng1 swAng; Z; M x2 y2; A iwd2 ihd2 stAng2 swAng; Z\n"
        ),
        concat!(
            "@blockArc\nav adj1 10800000\nav adj2 0\nav adj3 25000\ngd stAng pin 0 adj1 21599999\n",
            "gd istAng pin 0 adj2 21599999\ngd a3 pin 0 adj3 50000\ngd sw11 +- istAng 0 stAng\n",
            "gd sw12 +- sw11 21600000 0\ngd swAng ?: sw11 sw11 sw12\ngd iswAng +- 0 0 swAng\n",
            "gd wt1 sin wd2 stAng\ngd ht1 cos hd2 stAng\ngd dx1 cat2 wd2 ht1 wt1\ngd dy1 sat2 hd2 ht1 wt1\n",
            "gd x1 +- hc dx1 0\ngd y1 +- vc dy1 0\ngd dr */ ss a3 100000\ngd iwd2 +- wd2 0 dr\ngd ihd2 +- hd2 0 dr\n",
            "gd wt2 sin iwd2 istAng\ngd ht2 cos ihd2 istAng\ngd dx2 cat2 iwd2 ht2 wt2\ngd dy2 sat2 ihd2 ht2 wt2\n",
            "gd x2 +- hc dx2 0\ngd y2 +- vc dy2 0\n",
            "path\nM x1 y1; A wd2 hd2 stAng swAng; L x2 y2; A iwd2 ihd2 istAng iswAng; Z\n"
        ),
        concat!(
            "@arc\nav adj1 16200000\nav adj2 0\ngd stAng pin 0 adj1 21599999\ngd enAng pin 0 adj2 21599999\n",
            arc_start_guides!(),
            "path stroke=0 ext=0\nM x1 y1; A wd2 hd2 stAng swAng; L hc vc; Z\n",
            "path fill=none\nM x1 y1; A wd2 hd2 stAng swAng\n"
        ),
        concat!(
            "@chord\nav adj1 2700000\nav adj2 16200000\ngd stAng pin 0 adj1 21599999\ngd enAng pin 0 adj2 21599999\n",
            arc_start_guides!(),
            ellipse_text_guides!(),
            "tx il it ir ib\npath\nM x1 y1; A wd2 hd2 stAng swAng; Z\n"
        ),
        concat!(
            "@pie\nav adj1 0\nav adj2 16200000\ngd stAng pin 0 adj1 21599999\ngd enAng pin 0 adj2 21599999\n",
            arc_start_guides!(),
            ellipse_text_guides!(),
            "tx il it ir ib\npath\nM x1 y1; A wd2 hd2 stAng swAng; L hc vc; Z\n"
        ),
        concat!(
            "@frame\nav adj1 12500\ngd a1 pin 0 adj1 50000\ngd x1 */ ss a1 100000\ngd x4 +- r 0 x1\ngd y4 +- b 0 x1\n",
            "tx x1 x1 x4 y4\npath\nM l t; L r t; L r b; L l b; Z; M x1 x1; L x1 y4; L x4 y4; L x4 x1; Z\n"
        ),
        "@flowChartProcess\npath w=1 h=1\nM 0 0; L 1 0; L 1 1; L 0 1; Z\n",
        "@flowChartDecision\ngd ir */ w 3 4\ngd ib */ h 3 4\ntx wd4 hd4 ir ib\npath w=2 h=2\nM 0 1; L 1 0; L 2 1; L 1 2; Z\n",
        concat!(
            "@flowChartTerminator\ngd il */ w 1018 21600\ngd ir */ w 20582 21600\ngd it */ h 3163 21600\n",
            "gd ib */ h 18437 21600\ntx il it ir ib\n",
            "path w=21600 h=21600\nM 3475 0; L 18125 0; A 3475 10800 3cd4 cd2; L 3475 21600; A 3475 10800 cd4 cd2; Z\n"
        ),
        concat!(
            "@flowChartDocument\ngd y1 */ h 17322 21600\ngd y2 */ h 20172 21600\ntx l t r y1\n",
            "path w=21600 h=21600\nM 0 0; L 21600 0; L 21600 17322; C 10800 17322 10800 23922 0 20172; Z\n"
        ),
    ];
    parts.concat()
}

// ---------------------------------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------------------------------

/// `a:xfrm` (or `p:xfrm`): offset and extent in EMU, rotation in 60000ths of a degree (clockwise),
/// flips, and for a group its child offset/extent.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Xfrm {
    pub off: (i64, i64),
    pub ext: (i64, i64),
    pub rot: i64,
    pub flip_h: bool,
    pub flip_v: bool,
    pub ch_off: Option<(i64, i64)>,
    pub ch_ext: Option<(i64, i64)>,
}

impl Xfrm {
    /// From an `xfrm` element, or the `xfrm` inside an element (`a:spPr`, `p:grpSpPr`); all zero
    /// when there is none.
    pub fn parse(el: &Element) -> Xfrm {
        let x = if el.local() == "xfrm" {
            el
        } else {
            match el.child("xfrm") {
                Some(x) => x,
                None => return Xfrm::default(),
            }
        };
        let pair = |name: &str, a: &str, b: &str| x.child(name).map(|c| (attr_emu(c, a).unwrap_or(0), attr_emu(c, b).unwrap_or(0)));
        Xfrm {
            off: pair("off", "x", "y").unwrap_or((0, 0)),
            ext: pair("ext", "cx", "cy").unwrap_or((0, 0)),
            rot: attr_emu(x, "rot").unwrap_or(0),
            flip_h: flag(x.attr("flipH")).unwrap_or(false),
            flip_v: flag(x.attr("flipV")).unwrap_or(false),
            ch_off: pair("chOff", "x", "y"),
            ch_ext: pair("chExt", "cx", "cy"),
        }
    }

    /// The unrotated box in the parent's coordinates (points).
    pub fn rect(&self) -> Rect {
        Rect::new(
            emu_to_pt(self.off.0 as f64),
            emu_to_pt(self.off.1 as f64),
            emu_to_pt(self.ext.0 as f64),
            emu_to_pt(self.ext.1 as f64),
        )
    }

    pub fn rotation_deg(&self) -> f64 {
        self.rot as f64 / 60000.0
    }

    /// Shape-local points (0,0)-(w,h) to the parent's points: flipped and rotated about the box's
    /// centre, then placed at the offset.
    pub fn matrix(&self) -> Matrix {
        let r = self.rect();
        let (hw, hh) = (r.w / 2.0, r.h / 2.0);
        Matrix::translate(-hw, -hh)
            .then(&Matrix::scale(if self.flip_h { -1.0 } else { 1.0 }, if self.flip_v { -1.0 } else { 1.0 }))
            .then(&Matrix::rotate(self.rotation_deg().to_radians()))
            .then(&Matrix::translate(r.x + hw, r.y + hh))
    }

    /// For a group: its children's coordinate space (`chOff`/`chExt`, in points) to the parent's.
    /// A child's own [`Xfrm::matrix`] goes after it: `group.child_matrix().multiply(&child.matrix())`.
    pub fn child_matrix(&self) -> Matrix {
        let (cx, cy) = self.ch_off.unwrap_or(self.off);
        let (cw, ch) = self.ch_ext.unwrap_or(self.ext);
        let sx = if cw != 0 { self.ext.0 as f64 / cw as f64 } else { 1.0 };
        let sy = if ch != 0 { self.ext.1 as f64 / ch as f64 } else { 1.0 };
        Matrix::translate(-emu_to_pt(cx as f64), -emu_to_pt(cy as f64)).then(&Matrix::scale(sx, sy)).then(&self.matrix())
    }
}

// ---------------------------------------------------------------------------------------------------
// Shapes to display items
// ---------------------------------------------------------------------------------------------------

/// Lighter or darker paint for a path's fill mode (lighten: 40% towards white, lightenLess: 20%,
/// darken: 60% of the colour, darkenLess: 80%). `None` for `fill="none"`.
fn modulate(p: &Paint, mode: PathFill) -> Option<Paint> {
    let f = |c: Color| -> Color {
        let white = Color::rgba(255, 255, 255, c.a);
        let black = Color::rgba(0, 0, 0, c.a);
        match mode {
            PathFill::Lighten => mix(c, white, 0.4),
            PathFill::LightenLess => mix(c, white, 0.2),
            PathFill::Darken => mix(c, black, 0.4),
            PathFill::DarkenLess => mix(c, black, 0.2),
            _ => c,
        }
    };
    let stops = |s: &[GradientStop]| s.iter().map(|g| GradientStop { offset: g.offset, color: f(g.color) }).collect::<Vec<_>>();
    match mode {
        PathFill::None => None,
        PathFill::Norm => Some(p.clone()),
        _ => Some(match p {
            Paint::Solid(c) => Paint::Solid(f(*c)),
            Paint::Linear { x0, y0, x1, y1, stops: s } => Paint::Linear { x0: *x0, y0: *y0, x1: *x1, y1: *y1, stops: stops(s) },
            Paint::Radial { fx, fy, fr, cx, cy, r, stops: s } => {
                Paint::Radial { fx: *fx, fy: *fy, fr: *fr, cx: *cx, cy: *cy, r: *r, stops: stops(s) }
            }
        }),
    }
}

fn op_end(op: &PathOp) -> Option<(f64, f64)> {
    match *op {
        PathOp::MoveTo(x, y) | PathOp::LineTo(x, y) | PathOp::QuadTo(_, _, x, y) | PathOp::CubicTo(_, _, _, _, x, y) => Some((x, y)),
        PathOp::Close => None,
    }
}

/// The start of an open first subpath: (tip, the point the line arrives from).
fn path_start(ops: &[PathOp]) -> Option<((f64, f64), (f64, f64))> {
    let tip = match ops.first()? {
        PathOp::MoveTo(x, y) => (*x, *y),
        _ => return None,
    };
    let end = ops[1..].iter().position(|o| matches!(o, PathOp::MoveTo(..))).map_or(ops.len(), |i| i + 1);
    if ops[..end].iter().any(|o| matches!(o, PathOp::Close)) {
        return None;
    }
    let cands: Vec<(f64, f64)> = match *ops.get(1)? {
        PathOp::LineTo(x, y) => vec![(x, y)],
        PathOp::QuadTo(a, b, x, y) => vec![(a, b), (x, y)],
        PathOp::CubicTo(a, b, c, d, x, y) => vec![(a, b), (c, d), (x, y)],
        _ => return None,
    };
    cands.into_iter().find(|p| *p != tip).map(|from| (tip, from))
}

/// The end of an open last subpath: (tip, the point the line arrives from).
fn path_end(ops: &[PathOp]) -> Option<((f64, f64), (f64, f64))> {
    let s = ops.iter().rposition(|o| matches!(o, PathOp::MoveTo(..)))?;
    let sub = &ops[s..];
    if sub.len() < 2 || sub.iter().any(|o| matches!(o, PathOp::Close)) {
        return None;
    }
    let before = op_end(&sub[sub.len() - 2])?;
    let (tip, cands): ((f64, f64), Vec<(f64, f64)>) = match sub[sub.len() - 1] {
        PathOp::LineTo(x, y) => ((x, y), vec![before]),
        PathOp::QuadTo(a, b, x, y) => ((x, y), vec![(a, b), before]),
        PathOp::CubicTo(a, b, c, d, x, y) => ((x, y), vec![(c, d), (a, b), before]),
        _ => return None,
    };
    cands.into_iter().find(|p| *p != tip).map(|from| (tip, from))
}

/// The outline of a line end drawn at `tip`, pointing away from `from`, for a line `line_w`
/// points wide, and whether it is filled (an `arrow` end is two stroked strokes).
pub fn line_end_path(end: &LineEnd, tip: (f64, f64), from: (f64, f64), line_w: f64) -> Option<(Vec<PathOp>, bool)> {
    let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
    let n = (dx * dx + dy * dy).sqrt();
    if !(n > 0.0) || end.kind == LineEndKind::None {
        return None;
    }
    let (ux, uy) = (dx / n, dy / n);
    let (nx, ny) = (-uy, ux);
    let lw = sane(line_w).max(0.75);
    let len = end.len.factor() * lw;
    let hw = end.w.factor() * lw / 2.0;
    let at = |along: f64, across: f64| (tip.0 + ux * along + nx * across, tip.1 + uy * along + ny * across);
    let poly = |pts: &[(f64, f64)]| {
        let mut ops = vec![PathOp::MoveTo(pts[0].0, pts[0].1)];
        ops.extend(pts[1..].iter().map(|p| PathOp::LineTo(p.0, p.1)));
        ops.push(PathOp::Close);
        ops
    };
    Some(match end.kind {
        LineEndKind::Triangle => (poly(&[tip, at(-len, hw), at(-len, -hw)]), true),
        LineEndKind::Stealth => (poly(&[tip, at(-len, hw), at(-len * 0.5, 0.0), at(-len, -hw)]), true),
        LineEndKind::Diamond => (poly(&[at(len / 2.0, 0.0), at(0.0, hw), at(-len / 2.0, 0.0), at(0.0, -hw)]), true),
        LineEndKind::Oval => {
            // An ellipse centred on the tip: radius len/2 along the line, hw across it, as four
            // quarter Béziers in the (along, across) frame.
            let (a, b) = (len / 2.0, hw);
            let k = 4.0 / 3.0 * (PI / 8.0).tan();
            let p = |t: f64| (a * t.cos(), b * t.sin());
            let d = |t: f64| (-a * t.sin(), b * t.cos());
            let s0 = at(a, 0.0);
            let mut ops = vec![PathOp::MoveTo(s0.0, s0.1)];
            for i in 0..4 {
                let (t0, t1) = (i as f64 * PI / 2.0, (i + 1) as f64 * PI / 2.0);
                let (p0, d0, p1, d1) = (p(t0), d(t0), p(t1), d(t1));
                let c1 = at(p0.0 + k * d0.0, p0.1 + k * d0.1);
                let c2 = at(p1.0 - k * d1.0, p1.1 - k * d1.1);
                let e = at(p1.0, p1.1);
                ops.push(PathOp::CubicTo(c1.0, c1.1, c2.0, c2.1, e.0, e.1));
            }
            ops.push(PathOp::Close);
            (ops, true)
        }
        LineEndKind::Arrow => {
            let (a, b) = (at(-len, hw), at(-len, -hw));
            (vec![PathOp::MoveTo(a.0, a.1), PathOp::LineTo(tip.0, tip.1), PathOp::LineTo(b.0, b.1)], false)
        }
        LineEndKind::None => return None,
    })
}

/// A shape as display items: its geometry `paths` (from [`geometry`], in the shape's own points),
/// filled with `fill` and stroked with `line` (path fill modes and `stroke="0"` honoured, line ends
/// drawn on open paths), wrapped in the `xfrm`'s transform when given. Gradients span the xfrm's box
/// (or the paths' bounds without one).
pub fn shape_items(paths: &[GeomPath], fill: Option<&Fill>, line: Option<&Line>, xfrm: Option<&Xfrm>) -> Vec<Item> {
    let mut items = Vec::new();
    let bx = match xfrm {
        Some(x) => {
            let r = x.rect();
            Rect::new(0.0, 0.0, r.w, r.h)
        }
        None => paths
            .iter()
            .filter_map(|p| path_bounds(&p.ops))
            .reduce(|a, b| a.union(&b))
            .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
    };
    let rot = xfrm.map_or(0.0, |x| x.rotation_deg());
    let paint = fill.and_then(|f| f.to_paint_rotated(bx, rot));
    let stroke = line.and_then(|l| l.to_stroke());
    if let Some(x) = xfrm {
        items.push(Item::PushTransform(x.matrix()));
    }
    for p in paths {
        if p.ops.is_empty() {
            continue;
        }
        let f = paint.as_ref().and_then(|pt| modulate(pt, p.fill));
        let s = if p.stroke { stroke.clone() } else { None };
        if f.is_none() && s.is_none() {
            continue;
        }
        items.push(Item::Path { ops: p.ops.clone(), fill: f, stroke: s });
    }
    if let (Some(l), Some(s)) = (line, stroke.as_ref()) {
        let stroked: Vec<&GeomPath> = paths.iter().filter(|p| p.stroke && !p.ops.is_empty()).collect();
        let mut solid = s.clone();
        solid.dash.clear();
        let ends = [(l.head, stroked.first().and_then(|p| path_start(&p.ops))), (l.tail, stroked.last().and_then(|p| path_end(&p.ops)))];
        for (end, place) in ends {
            if let (Some(end), Some((tip, from))) = (end, place) {
                if let Some((ops, filled)) = line_end_path(&end, tip, from, s.width) {
                    if filled {
                        items.push(Item::Path { ops, fill: Some(Paint::Solid(s.color)), stroke: None });
                    } else {
                        items.push(Item::Path { ops, fill: None, stroke: Some(solid.clone()) });
                    }
                }
            }
        }
    }
    if xfrm.is_some() {
        items.push(Item::PopTransform);
    }
    items
}

/// Everything a shape's `a:spPr` says about drawing it, colours unresolved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapeProps {
    pub xfrm: Option<Xfrm>,
    pub geometry: Option<Geometry>,
    pub fill: Option<Fill<ColorSpec>>,
    pub line: Option<Line<ColorSpec>>,
}

impl ShapeProps {
    pub fn parse(sp_pr: &Element) -> ShapeProps {
        ShapeProps {
            xfrm: sp_pr.child("xfrm").map(Xfrm::parse),
            geometry: Geometry::parse(sp_pr),
            fill: find_fill(sp_pr),
            line: sp_pr.child("ln").map(parse_line),
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The first element of `body`, with the usual prefixes bound.
    fn el(body: &str) -> Element {
        let doc = format!(
            r#"<root xmlns:a="urn:a" xmlns:r="urn:r" xmlns:p="urn:p" xmlns:w="urn:w">{}</root>"#,
            body
        );
        parse_dom(doc.as_bytes()).unwrap().elements().next().unwrap().clone()
    }

    fn hex(c: Color) -> String {
        format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    fn color(xml: &str) -> Color {
        let t = Theme::office_default();
        resolve_color(&el(xml), &t, &ColorMap::default(), None)
    }

    fn pt(emu_pt: f64) -> f64 {
        emu_pt * EMU_PER_PT
    }

    fn ends(ops: &[PathOp]) -> Vec<(f64, f64)> {
        ops.iter().filter_map(op_end).collect()
    }

    const INVENTED_THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Kiyo Dusk">
 <a:themeElements>
  <a:clrScheme name="Dusk">
   <a:dk1><a:sysClr val="windowText" lastClr="101010"/></a:dk1>
   <a:lt1><a:sysClr val="window"/></a:lt1>
   <a:dk2><a:srgbClr val="1F2A44"/></a:dk2>
   <a:lt2><a:srgbClr val="EEEAE0"/></a:lt2>
   <a:accent1><a:srgbClr val="7B4FA0"/></a:accent1>
   <a:accent2><a:srgbClr val="C0504D"/></a:accent2>
   <a:accent3><a:prstClr val="medSeaGreen"/></a:accent3>
   <a:accent4><a:hslClr hue="0" sat="100000" lum="50000"/></a:accent4>
   <a:accent5><a:scrgbClr r="0" g="0" b="100000"/></a:accent5>
   <a:accent6><a:srgbClr val="FFC000"/></a:accent6>
   <a:hlink><a:srgbClr val="0000EE"/></a:hlink>
   <a:folHlink><a:srgbClr val="551A8B"/></a:folHlink>
  </a:clrScheme>
  <a:fontScheme name="Dusk">
   <a:majorFont><a:latin typeface="Georgia"/><a:ea typeface="MS Mincho"/><a:cs typeface="Arial"/>
    <a:font script="Jpan" typeface="Yu Mincho"/><a:font script="Arab" typeface="Tahoma"/></a:majorFont>
   <a:minorFont><a:latin typeface="Verdana"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>
  </a:fontScheme>
  <a:fmtScheme name="Dusk">
   <a:fillStyleLst>
    <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
    <a:gradFill rotWithShape="1"><a:gsLst>
      <a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="50000"/></a:schemeClr></a:gs>
      <a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/></a:schemeClr></a:gs>
     </a:gsLst><a:lin ang="5400000" scaled="1"/></a:gradFill>
    <a:pattFill prst="pct50"><a:fgClr><a:schemeClr val="phClr"/></a:fgClr><a:bgClr><a:srgbClr val="FFFFFF"/></a:bgClr></a:pattFill>
   </a:fillStyleLst>
   <a:lnStyleLst>
    <a:ln w="9525" cap="rnd"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="dash"/><a:round/></a:ln>
    <a:ln w="25400"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:custDash><a:ds d="200000" sp="100000"/></a:custDash><a:miter lim="800000"/></a:ln>
   </a:lnStyleLst>
   <a:effectStyleLst/>
   <a:bgFillStyleLst><a:noFill/><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></a:bgFillStyleLst>
  </a:fmtScheme>
 </a:themeElements>
</a:theme>"#;

    // ----- theme -----

    #[test]
    fn invented_theme_parses() {
        let t = Theme::parse(INVENTED_THEME.as_bytes()).unwrap();
        assert_eq!(t.name, "Kiyo Dusk");
        assert_eq!(t.colors.name, "Dusk");
        assert_eq!(hex(t.colors.dk1), "101010", "sysClr uses lastClr");
        assert_eq!(hex(t.colors.lt1), "FFFFFF", "sysClr without lastClr uses the system table");
        assert_eq!(hex(t.colors.dk2), "1F2A44");
        assert_eq!(hex(t.colors.accent1), "7B4FA0");
        assert_eq!(hex(t.colors.accent3), "3CB371", "prstClr medSeaGreen");
        assert_eq!(hex(t.colors.accent4), "FF0000", "hslClr");
        assert_eq!(hex(t.colors.accent5), "0000FF", "scrgbClr");
        assert_eq!(hex(t.colors.fol_hlink), "551A8B");
        assert_eq!(t.major_font.latin, "Georgia");
        assert_eq!(t.major_font.ea, "MS Mincho");
        assert_eq!(t.major_font.cs, "Arial");
        assert_eq!(t.major_font.script("Jpan"), Some("Yu Mincho"));
        assert_eq!(t.major_font.script("Arab"), Some("Tahoma"));
        assert_eq!(t.major_font.script("Thai"), None);
        assert_eq!(t.minor_font.latin, "Verdana");
        assert_eq!(t.resolve_typeface("+mj-lt"), "Georgia");
        assert_eq!(t.resolve_typeface("+mn-lt"), "Verdana");
        assert_eq!(t.resolve_typeface("+mj-ea"), "MS Mincho");
        assert_eq!(t.resolve_typeface("Consolas"), "Consolas");

        assert_eq!(t.fill_styles.len(), 3);
        assert_eq!(t.line_styles.len(), 2);
        assert_eq!(t.bg_fill_styles.len(), 2);
        let cmap = ColorMap::default();
        let ph = Some(t.colors.accent1);
        // fillRef idx 1 with accent1 as placeholder: solid accent1.
        assert_eq!(t.fill_ref(1).unwrap().resolve(&t, &cmap, ph), Fill::Solid(t.colors.accent1));
        // The gradient's stops are sorted and keep their transforms.
        match t.fill_ref(2).unwrap().resolve(&t, &cmap, ph) {
            Fill::Gradient(g) => {
                assert_eq!(g.stops.len(), 2);
                assert_eq!(g.stops[0].pos, 0.0);
                assert!(g.rot_with_shape);
                assert_eq!(g.kind, GradientKind::Linear { angle: 90.0, scaled: true });
                assert!(g.stops[0].color.r > t.colors.accent1.r, "tinted stop is lighter");
                assert!(g.stops[1].color.r < t.colors.accent1.r, "shaded stop is darker");
            }
            other => panic!("expected gradient, got {:?}", other),
        }
        match t.fill_ref(3).unwrap() {
            Fill::Pattern { prst, .. } => assert_eq!(prst, "pct50"),
            other => panic!("expected pattern, got {:?}", other),
        }
        assert_eq!(t.fill_ref(0), None);
        assert_eq!(t.fill_ref(4), None);
        assert_eq!(t.fill_ref(1001), Some(&Fill::None));
        assert_eq!(t.fill_ref(1002).unwrap().resolve(&t, &cmap, None), Fill::Solid(t.colors.accent2));

        let l1 = t.line_ref(1).unwrap().resolve(&t, &cmap, ph);
        assert_eq!(l1.width_emu, Some(9525));
        assert_eq!(l1.cap, Some(LineCap::Round));
        assert_eq!(l1.join, Some(LineJoin::Round));
        let s1 = l1.to_stroke().unwrap();
        assert!(close(s1.width, 0.75));
        assert_eq!(s1.color, t.colors.accent1);
        assert_eq!(s1.dash, vec![3.0, 2.25]);
        let s2 = t.line_ref(2).unwrap().resolve(&t, &cmap, ph).to_stroke().unwrap();
        assert!(close(s2.width, 2.0));
        assert_eq!(s2.dash, vec![4.0, 2.0], "custDash in multiples of the width");
        assert_eq!(s2.join, LineJoin::Miter);
        assert!(close(s2.miter_limit, 8.0));
        assert_eq!(t.line_ref(0), None);
        assert_eq!(t.line_ref(3), None);
    }

    #[test]
    fn office_default_theme() {
        let t = Theme::office_default();
        assert_eq!(t, Theme::default());
        assert_eq!(hex(t.colors.accent1), "4472C4");
        assert_eq!(hex(t.colors.accent2), "ED7D31");
        assert_eq!(hex(t.colors.accent6), "70AD47");
        assert_eq!(hex(t.colors.dk2), "44546A");
        assert_eq!(hex(t.colors.hlink), "0563C1");
        assert_eq!(t.major_font.latin, "Calibri Light");
        assert_eq!(t.minor_font.latin, "Calibri");
        assert_eq!(t.minor_font.script("Arab"), Some("Arial"));
        assert_eq!(t.fill_styles.len(), 3);
        assert_eq!(t.bg_fill_styles.len(), 3);
        let widths: Vec<Option<i64>> = t.line_styles.iter().map(|l| l.width_emu).collect();
        assert_eq!(widths, vec![Some(6350), Some(12700), Some(19050)]);
        let s = t.line_ref(2).unwrap().resolve(&t, &ColorMap::default(), Some(t.colors.accent1)).to_stroke().unwrap();
        assert!(close(s.width, 1.0));
        assert_eq!(s.cap, LineCap::Butt);
        assert!(s.dash.is_empty());
    }

    #[test]
    fn theme_errors_and_partial_themes() {
        assert!(matches!(Theme::parse(b"not xml at all <<<"), Err(DrawError::Xml(_))));
        assert!(matches!(Theme::parse(b""), Err(DrawError::Xml(_))));
        match Theme::parse(br#"<a:foo xmlns:a="urn:a"/>"#) {
            Err(DrawError::NotTheme(n)) => assert_eq!(n, "a:foo"),
            other => panic!("{:?}", other),
        }
        let e = Theme::parse(br#"<a:foo xmlns:a="urn:a"/>"#).unwrap_err();
        assert!(e.to_string().contains("a:foo"));
        // A theme with only some colours keeps the Office values for the rest.
        let t = Theme::parse(br#"<a:theme xmlns:a="urn:a"><a:themeElements><a:clrScheme name="x"><a:accent1><a:srgbClr val="123456"/></a:accent1><a:accent2><a:srgbClr val="zz"/></a:accent2><a:bogus><a:srgbClr val="FFFFFF"/></a:bogus></a:clrScheme></a:themeElements></a:theme>"#).unwrap();
        assert_eq!(hex(t.colors.accent1), "123456");
        assert_eq!(hex(t.colors.accent2), "000000", "an unreadable value is black");
        assert_eq!(hex(t.colors.accent3), "A5A5A5");
        assert_eq!(t.major_font.latin, "Calibri Light");
        assert!(t.fill_styles.is_empty());
        let empty = Theme::parse(br#"<theme/>"#).unwrap();
        assert_eq!(empty.colors, ColorScheme::office());
    }

    #[test]
    fn color_maps() {
        let t = Theme::office_default();
        let d = ColorMap::default();
        assert_eq!(t.scheme_color("bg1", &d), Color::WHITE);
        assert_eq!(t.scheme_color("tx1", &d), Color::BLACK);
        assert_eq!(hex(t.scheme_color("tx2", &d)), "44546A");
        assert_eq!(hex(t.scheme_color("bg2", &d)), "E7E6E6");
        assert_eq!(hex(t.scheme_color("accent1", &d)), "4472C4");
        assert_eq!(hex(t.scheme_color("text2", &d)), "44546A");
        assert_eq!(hex(t.scheme_color("hyperlink", &d)), "0563C1");
        assert_eq!(t.scheme_color("nonsense", &d), Color::BLACK);

        // A dark slide master swaps the roles.
        let dark = ColorMap::parse(&el(r#"<p:clrMap bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2" accent1="accent2" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#));
        assert_eq!(t.scheme_color("bg1", &dark), Color::BLACK);
        assert_eq!(t.scheme_color("tx1", &dark), Color::WHITE);
        assert_eq!(hex(t.scheme_color("bg2", &dark)), "44546A");
        assert_eq!(hex(t.scheme_color("accent1", &dark)), "ED7D31");
        // WordprocessingML spelling.
        let w = ColorMap::parse(&el(r#"<w:clrSchemeMapping w:bg1="dark1" w:t1="light1" w:bg2="light2" w:t2="dark2" w:accent1="accent1" w:hyperlink="followedHyperlink" w:followedHyperlink="followedHyperlink"/>"#));
        assert_eq!(t.scheme_color("bg1", &w), Color::BLACK);
        assert_eq!(t.scheme_color("tx1", &w), Color::WHITE);
        assert_eq!(hex(t.scheme_color("hlink", &w)), "954F72");
        // An override holding a mapping, and one that keeps the master's.
        let o = ColorMap::parse(&el(r#"<p:clrMapOvr><a:overrideClrMapping bg1="dk1" tx1="lt1"/></p:clrMapOvr>"#));
        assert_eq!(t.scheme_color("bg1", &o), Color::BLACK);
        let keep = ColorMap::parse(&el(r#"<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>"#));
        assert_eq!(keep, ColorMap::default());
        // Through resolve_color.
        assert_eq!(resolve_color(&el(r#"<a:schemeClr val="bg1"/>"#), &t, &dark, None), Color::BLACK);
    }

    // ----- colours -----

    #[test]
    fn luminance_transforms_match_office() {
        assert_eq!(hex(color(r#"<a:schemeClr val="accent1"><a:lumMod val="75000"/></a:schemeClr>"#)), "2F5597");
        assert_eq!(hex(color(r#"<a:schemeClr val="accent1"><a:lumMod val="60000"/><a:lumOff val="40000"/></a:schemeClr>"#)), "8FAADC");
        assert_eq!(hex(color(r#"<a:schemeClr val="accent1"><a:lumMod val="20000"/><a:lumOff val="80000"/></a:schemeClr>"#)), "DAE3F3");
        assert_eq!(hex(color(r#"<a:srgbClr val="4472C4"><a:lumMod val="40000"/><a:lumOff val="60000"/></a:srgbClr>"#)), "B4C7E7");
        // Strict-format percentages.
        assert_eq!(hex(color(r#"<a:schemeClr val="accent1"><a:lumMod val="75%"/></a:schemeClr>"#)), "2F5597");
        // Order matters: lumOff then lumMod differs.
        let a = color(r#"<a:srgbClr val="4472C4"><a:lumOff val="40000"/><a:lumMod val="60000"/></a:srgbClr>"#);
        assert_ne!(hex(a), "8FAADC");
    }

    #[test]
    fn tint_and_shade_in_linear_rgb() {
        // Expected values from the spec's definition (mix with white / black) done in linear RGB.
        assert_eq!(hex(color(r#"<a:srgbClr val="4472C4"><a:tint val="50000"/></a:srgbClr>"#)), "C0C9E4");
        assert_eq!(hex(color(r#"<a:srgbClr val="4472C4"><a:shade val="50000"/></a:srgbClr>"#)), "2F528F");
        assert_eq!(hex(color(r#"<a:srgbClr val="4472C4"><a:tint val="20000"/></a:srgbClr>"#)), "E9EBF5");
        assert_eq!(hex(color(r#"<a:srgbClr val="4472C4"><a:shade val="75000"/></a:srgbClr>"#)), "3B64AC");
        assert_eq!(hex(color(r#"<a:srgbClr val="4472C4"><a:tint val="100000"/></a:srgbClr>"#)), "4472C4");
        assert_eq!(hex(color(r#"<a:srgbClr val="4472C4"><a:tint val="0"/></a:srgbClr>"#)), "FFFFFF");
        assert_eq!(hex(color(r#"<a:srgbClr val="4472C4"><a:shade val="0"/></a:srgbClr>"#)), "000000");
        assert_eq!(hex(color(r#"<a:srgbClr val="FFFFFF"><a:shade val="50000"/></a:srgbClr>"#)), "BCBCBC");
    }

    #[test]
    fn other_transforms() {
        assert_eq!(hex(color(r#"<a:srgbClr val="FF0000"><a:comp/></a:srgbClr>"#)), "00FFFF");
        assert_eq!(hex(color(r#"<a:srgbClr val="FF8000"><a:inv/></a:srgbClr>"#)), "007FFF");
        assert_eq!(hex(color(r#"<a:srgbClr val="FFFFFF"><a:gray/></a:srgbClr>"#)), "FFFFFF");
        assert_eq!(hex(color(r#"<a:srgbClr val="FF0000"><a:gray/></a:srgbClr>"#)), "4C4C4C");
        let c = color(r#"<a:srgbClr val="FF0000"><a:alpha val="50000"/></a:srgbClr>"#);
        assert_eq!((c.r, c.a), (255, 128));
        let c = color(r#"<a:srgbClr val="FF0000"><a:alpha val="50000"/><a:alphaMod val="50000"/><a:alphaOff val="10000"/></a:srgbClr>"#);
        assert_eq!(c.a, 89); // (0.5 * 0.5 + 0.1) * 255
        assert_eq!(hex(color(r#"<a:srgbClr val="FF0000"><a:hueOff val="7200000"/></a:srgbClr>"#)), "00FF00");
        assert_eq!(hex(color(r#"<a:srgbClr val="FF0000"><a:hue val="14400000"/></a:srgbClr>"#)), "0000FF");
        assert_eq!(hex(color(r#"<a:srgbClr val="00FF00"><a:hueMod val="200000"/></a:srgbClr>"#)), "0000FF");
        assert_eq!(hex(color(r#"<a:srgbClr val="FF0000"><a:satMod val="0"/></a:srgbClr>"#)), "808080");
        assert_eq!(hex(color(r#"<a:srgbClr val="FF0000"><a:sat val="0"/></a:srgbClr>"#)), "808080");
        assert_eq!(hex(color(r#"<a:srgbClr val="808080"><a:satOff val="100000"/><a:hue val="0"/></a:srgbClr>"#)), "FF0101");
        assert_eq!(hex(color(r#"<a:srgbClr val="FF0000"><a:lum val="100000"/></a:srgbClr>"#)), "FFFFFF");
        assert_eq!(hex(color(r#"<a:srgbClr val="FFFFFF"><a:red val="0"/></a:srgbClr>"#)), "00FFFF");
        assert_eq!(hex(color(r#"<a:srgbClr val="FFFFFF"><a:greenMod val="0"/><a:blueOff val="-100000"/></a:srgbClr>"#)), "FF0000");
        assert_eq!(hex(color(r#"<a:srgbClr val="000000"><a:redOff val="100000"/><a:green val="100000"/><a:blue val="100000"/></a:srgbClr>"#)), "FFFFFF");
        assert_eq!(hex(color(r#"<a:srgbClr val="FFFFFF"><a:redMod val="50000"/></a:srgbClr>"#)), "BCFFFF");
        // gamma then invGamma is the identity; gamma lightens.
        assert_eq!(hex(color(r#"<a:srgbClr val="4472C4"><a:gamma/><a:invGamma/></a:srgbClr>"#)), "4472C4");
        let g = color(r#"<a:srgbClr val="404040"><a:gamma/></a:srgbClr>"#);
        assert!(g.r > 0x40);
        let ig = color(r#"<a:srgbClr val="808080"><a:invGamma/></a:srgbClr>"#);
        assert_eq!(hex(ig), "373737");
        // Unknown transform elements are skipped.
        assert_eq!(hex(color(r#"<a:srgbClr val="123456"><a:sparkle val="1"/><a:extLst/></a:srgbClr>"#)), "123456");
    }

    #[test]
    fn colour_bases() {
        assert_eq!(hex(color(r#"<a:hslClr hue="0" sat="100000" lum="50000"/>"#)), "FF0000");
        assert_eq!(hex(color(r#"<a:hslClr hue="7200000" sat="100%" lum="25%"/>"#)), "008000");
        assert_eq!(hex(color(r#"<a:scrgbClr r="100000" g="50000" b="0"/>"#)), "FFBC00");
        assert_eq!(hex(color(r#"<a:prstClr val="dkBlue"/>"#)), "00008B");
        assert_eq!(hex(color(r#"<a:prstClr val="ltGray"/>"#)), "D3D3D3");
        assert_eq!(hex(color(r#"<a:prstClr val="medSeaGreen"/>"#)), "3CB371");
        assert_eq!(hex(color(r#"<a:prstClr val="mediumSeaGreen"/>"#)), "3CB371");
        assert_eq!(hex(color(r#"<a:prstClr val="cornflowerBlue"/>"#)), "6495ED");
        assert_eq!(hex(color(r#"<a:prstClr val="nope"/>"#)), "000000");
        assert_eq!(preset_color("yellowGreen"), Some(Color::rgb(0x9A, 0xCD, 0x32)));
        assert_eq!(preset_color("dkSlateGrey"), Some(Color::rgb(0x2F, 0x4F, 0x4F)));
        assert_eq!(preset_color("bogus"), None);
        assert_eq!(hex(color(r#"<a:sysClr val="window"/>"#)), "FFFFFF");
        assert_eq!(hex(color(r#"<a:sysClr val="windowText" lastClr="222222"/>"#)), "222222");
        assert_eq!(hex(color(r#"<a:sysClr val="highlight"/>"#)), "0078D7");
        // A container element finds its colour.
        assert_eq!(hex(color(r#"<a:solidFill><a:srgbClr val="ABCDEF"/></a:solidFill>"#)), "ABCDEF");
        // phClr with and without a placeholder.
        let t = Theme::office_default();
        let ph = resolve_color(&el(r#"<a:schemeClr val="phClr"><a:lumMod val="75000"/></a:schemeClr>"#), &t, &ColorMap::default(), Some(t.colors.accent1));
        assert_eq!(hex(ph), "2F5597");
        assert_eq!(resolve_color(&el(r#"<a:schemeClr val="phClr"/>"#), &t, &ColorMap::default(), None), Color::BLACK);
        // Not a colour at all.
        assert_eq!(resolve_color(&el(r#"<a:ln/>"#), &t, &ColorMap::default(), None), Color::BLACK);
        assert!(ColorSpec::parse(&el(r#"<a:ln/>"#)).is_none());
        // Built by hand.
        let s = ColorSpec::scheme("accent1").with(ColorMod::LumMod(0.75));
        assert_eq!(hex(s.resolve(&t, &ColorMap::default(), None)), "2F5597");
        let s = ColorSpec::rgb(Color::rgba(10, 20, 30, 40));
        assert_eq!(s.resolve(&t, &ColorMap::default(), None), Color::rgba(10, 20, 30, 40));
    }

    // ----- fills and lines -----

    #[test]
    fn fills_parse_and_paint() {
        let t = Theme::office_default();
        let cm = ColorMap::default();
        let bx = Rect::new(0.0, 0.0, 100.0, 50.0);
        let solid = find_fill(&el(r#"<a:spPr><a:xfrm/><a:prstGeom prst="rect"/><a:solidFill><a:schemeClr val="accent2"/></a:solidFill><a:ln/></a:spPr>"#)).unwrap();
        assert_eq!(solid.resolve(&t, &cm, None).to_paint(bx), Some(Paint::Solid(t.colors.accent2)));
        assert_eq!(parse_fill(&el(r#"<a:noFill/>"#)), Some(Fill::None));
        assert_eq!(parse_fill(&el(r#"<a:grpFill/>"#)), Some(Fill::Group));
        assert_eq!(parse_fill(&el(r#"<a:ln/>"#)), None);
        assert_eq!(Fill::None.to_paint(bx), None);
        assert_eq!(Fill::<Color>::Group.to_paint(bx), None);

        // A vertical linear gradient (90 degrees) runs top to bottom through the centre.
        let g = parse_fill(&el(r#"<a:gradFill><a:gsLst><a:gs pos="0"><a:srgbClr val="FF0000"/></a:gs><a:gs pos="100000"><a:srgbClr val="0000FF"/></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>"#)).unwrap().resolve(&t, &cm, None);
        match g.to_paint(bx).unwrap() {
            Paint::Linear { x0, y0, x1, y1, stops } => {
                assert!(close(x0, 50.0) && close(y0, 0.0) && close(x1, 50.0) && close(y1, 50.0), "{} {} {} {}", x0, y0, x1, y1);
                assert_eq!(stops.len(), 2);
                assert_eq!(stops[1].color, Color::rgb(0, 0, 255));
            }
            p => panic!("{:?}", p),
        }
        // 0 degrees: left to right; 45 unscaled: the corners reach the ends.
        let lin = |angle: f64, scaled: bool| {
            let f: Fill = Fill::Gradient(Gradient {
                stops: vec![Stop { pos: 0.0, color: Color::BLACK }, Stop { pos: 1.0, color: Color::WHITE }],
                kind: GradientKind::Linear { angle, scaled },
                rot_with_shape: true,
                flip: None,
            });
            match f.to_paint(bx).unwrap() {
                Paint::Linear { x0, y0, x1, y1, .. } => (x0, y0, x1, y1),
                p => panic!("{:?}", p),
            }
        };
        let (x0, y0, x1, y1) = lin(0.0, false);
        assert!(close(x0, 0.0) && close(y0, 25.0) && close(x1, 100.0) && close(y1, 25.0));
        let (x0, y0, x1, y1) = lin(45.0, false);
        let half = (100.0 + 50.0) / 2.0 * (0.5f64).sqrt();
        assert!(close(x1 - x0, y1 - y0), "45 degrees");
        assert!(close(((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt(), 2.0 * half));
        // Scaled 45 degrees: the isolines run corner to corner, so the direction is (h, w).
        let (x0, y0, x1, y1) = lin(45.0, true);
        assert!(close((y1 - y0) / (x1 - x0), 100.0 / 50.0));
        // A gradient that does not rotate with its shape is turned back.
        let f: Fill = Fill::Gradient(Gradient {
            stops: vec![Stop { pos: 0.0, color: Color::BLACK }, Stop { pos: 1.0, color: Color::WHITE }],
            kind: GradientKind::Linear { angle: 90.0, scaled: false },
            rot_with_shape: false,
            flip: None,
        });
        match f.to_paint_rotated(Rect::new(0.0, 0.0, 50.0, 50.0), 90.0).unwrap() {
            Paint::Linear { x0, y0, x1, y1, .. } => assert!(close(x0, 0.0) && close(y0, 25.0) && close(x1, 50.0) && close(y1, 25.0)),
            p => panic!("{:?}", p),
        }

        // A path gradient from the centre is radial.
        let r = parse_fill(&el(r#"<a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:srgbClr val="FFFFFF"/></a:gs><a:gs pos="100000"><a:srgbClr val="000000"/></a:gs></a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="50000" r="50000" b="50000"/></a:path></a:gradFill>"#)).unwrap().resolve(&t, &cm, None);
        match r.to_paint(bx).unwrap() {
            Paint::Radial { fx, fy, fr, cx, cy, r, stops } => {
                assert!(close(fx, 50.0) && close(fy, 25.0) && close(fr, 0.0) && close(cx, 50.0) && close(cy, 25.0));
                assert!(close(r, (50f64 * 50.0 + 25.0 * 25.0).sqrt()));
                assert_eq!(stops[0].color, Color::WHITE);
            }
            p => panic!("{:?}", p),
        }
        // From the top-left corner.
        let r = parse_fill(&el(r#"<a:gradFill><a:gsLst><a:gs pos="0"><a:srgbClr val="FFFFFF"/></a:gs><a:gs pos="100000"><a:srgbClr val="000000"/></a:gs></a:gsLst><a:path path="rect"><a:fillToRect r="100000" b="100000"/></a:path></a:gradFill>"#)).unwrap().resolve(&t, &cm, None);
        match r.to_paint(bx).unwrap() {
            Paint::Radial { cx, cy, r, .. } => assert!(close(cx, 0.0) && close(cy, 0.0) && close(r, (100f64 * 100.0 + 50.0 * 50.0).sqrt())),
            p => panic!("{:?}", p),
        }
        // One stop is solid; none is nothing.
        let one = parse_fill(&el(r#"<a:gradFill><a:gsLst><a:gs pos="30000"><a:srgbClr val="00FF00"/></a:gs></a:gsLst></a:gradFill>"#)).unwrap().resolve(&t, &cm, None);
        assert_eq!(one.to_paint(bx), Some(Paint::Solid(Color::rgb(0, 255, 0))));
        let none = parse_fill(&el(r#"<a:gradFill/>"#)).unwrap().resolve(&t, &cm, None);
        assert_eq!(none.to_paint(bx), None);

        // Patterns paint as the blend of their colours.
        let p = parse_fill(&el(r#"<a:pattFill prst="pct25"><a:fgClr><a:srgbClr val="000000"/></a:fgClr><a:bgClr><a:srgbClr val="FFFFFF"/></a:bgClr></a:pattFill>"#)).unwrap().resolve(&t, &cm, None);
        assert_eq!(p.to_paint(bx), Some(Paint::Solid(Color::rgb(191, 191, 191))));

        // Picture fills keep their relationship and mode.
        let pic = parse_fill(&el(r#"<a:blipFill rotWithShape="1"><a:blip r:embed="rId7"><a:alphaModFix amt="50000"/></a:blip><a:srcRect l="10000" r="10000"/><a:stretch><a:fillRect/></a:stretch></a:blipFill>"#)).unwrap();
        match &pic {
            Fill::Picture(p) => {
                assert_eq!(p.embed.as_deref(), Some("rId7"));
                assert!(close(p.alpha, 0.5));
                assert!(close(p.src_rect.l, 0.1) && close(p.src_rect.r, 0.1));
                assert_eq!(p.mode, PictureMode::Stretch(Insets::default()));
            }
            f => panic!("{:?}", f),
        }
        assert_eq!(pic.resolve(&t, &cm, None).to_paint(bx), None);
        let tile = parse_fill(&el(r#"<a:blipFill><a:blip r:link="rId2"/><a:tile tx="12700" ty="0" sx="50000" sy="50000" flip="xy" algn="ctr"/></a:blipFill>"#)).unwrap();
        match tile {
            Fill::Picture(p) => {
                assert_eq!(p.link.as_deref(), Some("rId2"));
                assert_eq!(p.mode, PictureMode::Tile { tx: 12700, ty: 0, sx: 0.5, sy: 0.5, flip: "xy".into(), align: "ctr".into() });
            }
            f => panic!("{:?}", f),
        }
    }

    #[test]
    fn lines_parse_and_stroke() {
        let t = Theme::office_default();
        let cm = ColorMap::default();
        let l = parse_line(&el(r#"<a:ln w="38100" cap="sq" cmpd="dbl"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:prstDash val="lgDashDot"/><a:bevel/><a:headEnd type="triangle" w="lg" len="sm"/><a:tailEnd type="arrow"/></a:ln>"#));
        assert_eq!(l.width_emu, Some(38100));
        assert_eq!(l.compound.as_deref(), Some("dbl"));
        assert_eq!(l.head, Some(LineEnd { kind: LineEndKind::Triangle, w: EndSize::Large, len: EndSize::Small }));
        assert_eq!(l.tail, Some(LineEnd { kind: LineEndKind::Arrow, w: EndSize::Medium, len: EndSize::Medium }));
        let s = l.resolve(&t, &cm, None).to_stroke().unwrap();
        assert!(close(s.width, 3.0));
        assert_eq!(s.color, Color::rgb(255, 0, 0));
        assert_eq!(s.cap, LineCap::Square);
        assert_eq!(s.join, LineJoin::Bevel);
        assert_eq!(s.dash, vec![24.0, 9.0, 3.0, 9.0]);
        // No fill: no stroke; noFill: no stroke.
        assert!(parse_line(&el(r#"<a:ln w="12700"/>"#)).resolve(&t, &cm, None).to_stroke().is_none());
        assert!(parse_line(&el(r#"<a:ln w="12700"><a:noFill/></a:ln>"#)).resolve(&t, &cm, None).to_stroke().is_none());
        // Width defaults and the zero-width hairline.
        let z = parse_line(&el(r#"<a:ln w="0"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln>"#)).resolve(&t, &cm, None);
        assert!(close(z.to_stroke().unwrap().width, 0.75));
        // A shape's line over its style line.
        let style = t.line_ref(3).unwrap();
        let own = parse_line(&el(r#"<a:ln><a:prstDash val="sysDot"/></a:ln>"#));
        let merged = own.over(style).resolve(&t, &cm, Some(t.colors.accent1)).to_stroke().unwrap();
        assert!(close(merged.width, 1.5));
        assert_eq!(merged.color, t.colors.accent1);
        assert_eq!(merged.dash, vec![1.5, 1.5]);
        // Every dash preset has an even pattern; solid and unknown are empty.
        for p in ["dot", "dash", "lgDash", "dashDot", "lgDashDot", "lgDashDotDot", "sysDash", "sysDot", "sysDashDot", "sysDashDotDot"] {
            let v = Dash::Preset(p.into()).pattern();
            assert!(!v.is_empty() && v.len() % 2 == 0, "{}", p);
        }
        assert!(Dash::Preset("solid".into()).pattern().is_empty());
        assert!(Dash::Custom(vec![(0.0, 0.0)]).pattern().is_empty());
    }

    // ----- geometry -----

    fn sized(name: &str, w_pt: f64, h_pt: f64, adj: &[(&str, f64)]) -> Vec<GeomPath> {
        let adj: Vec<(String, f64)> = adj.iter().map(|(n, v)| (n.to_string(), *v)).collect();
        preset_geometry(name, pt(w_pt), pt(h_pt), &adj).unwrap()
    }

    fn assert_pts(got: &[(f64, f64)], want: &[(f64, f64)]) {
        assert_eq!(got.len(), want.len(), "{:?} vs {:?}", got, want);
        for (g, w) in got.iter().zip(want) {
            assert!(near(g.0, w.0, 1e-6) && near(g.1, w.1, 1e-6), "{:?} vs {:?}", got, want);
        }
    }

    #[test]
    fn rect_geometry() {
        let p = sized("rect", 100.0, 50.0, &[]);
        assert_eq!(p.len(), 1);
        assert_eq!(
            p[0].ops,
            vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(100.0, 0.0), PathOp::LineTo(100.0, 50.0), PathOp::LineTo(0.0, 50.0), PathOp::Close]
        );
        assert_eq!(p[0].fill, PathFill::Norm);
        assert!(p[0].stroke);
        // Unknown presets draw as rect through `geometry`, but are None by name.
        assert!(preset_geometry("noSuchShape", 1.0, 1.0, &[]).is_none());
        assert_eq!(geometry(&Geometry::preset("noSuchShape"), pt(100.0), pt(50.0), &[]), p);
        let sg = shape_geometry(&Geometry::preset("rect"), pt(100.0), pt(50.0), &[]);
        assert_eq!(sg.text_rect, Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn ellipse_geometry() {
        let p = sized("ellipse", 100.0, 50.0, &[]);
        let ops = &p[0].ops;
        assert_eq!(ops.len(), 6);
        assert_pts(&ends(ops), &[(0.0, 25.0), (50.0, 0.0), (100.0, 25.0), (50.0, 50.0), (0.0, 25.0)]);
        let k = 4.0 / 3.0 * (PI / 8.0).tan();
        match ops[1] {
            PathOp::CubicTo(x1, y1, x2, y2, _, _) => {
                assert!(close(x1, 0.0) && close(y1, 25.0 - 25.0 * k));
                assert!(close(x2, 50.0 - 50.0 * k) && close(y2, 0.0));
            }
            o => panic!("{:?}", o),
        }
        assert_eq!(ops[5], PathOp::Close);
        // Text rect: the inscribed rectangle at 45 degrees.
        let sg = shape_geometry(&Geometry::preset("ellipse"), pt(100.0), pt(50.0), &[]);
        let c = (PI / 4.0).cos();
        assert!(close(sg.text_rect.x, 50.0 - 50.0 * c) && close(sg.text_rect.h, 50.0 * c));
    }

    #[test]
    fn round_rect_geometry() {
        // Default adj 16667 on 100 x 50: radius ss * 0.16667.
        let r = 50.0 * 0.16667;
        let p = sized("roundRect", 100.0, 50.0, &[]);
        let e = ends(&p[0].ops);
        assert_pts(
            &e,
            &[(0.0, r), (r, 0.0), (100.0 - r, 0.0), (100.0, r), (100.0, 50.0 - r), (100.0 - r, 50.0), (r, 50.0), (0.0, 50.0 - r)],
        );
        // adj 50000: the corners meet (a capsule).
        let p = sized("roundRect", 100.0, 50.0, &[("adj", 50000.0)]);
        assert_pts(&ends(&p[0].ops)[..3], &[(0.0, 25.0), (25.0, 0.0), (75.0, 0.0)]);
        // Over the pin: clamped to 50000.
        assert_eq!(sized("roundRect", 100.0, 50.0, &[("adj", 90000.0)]), p);
        // avLst written on the geometry.
        let g = Geometry::parse(&el(r#"<a:spPr><a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 50000"/></a:avLst></a:prstGeom></a:spPr>"#)).unwrap();
        assert_eq!(geometry(&g, pt(100.0), pt(50.0), &[]), p);
        let sg = shape_geometry(&Geometry::preset("roundRect"), pt(100.0), pt(50.0), &[]);
        let il = r * 0.29289;
        assert!(close(sg.text_rect.x, il) && close(sg.text_rect.right(), 100.0 - il));
    }

    #[test]
    fn right_arrow_geometry() {
        let p = sized("rightArrow", 100.0, 50.0, &[]);
        assert_pts(
            &ends(&p[0].ops),
            &[(0.0, 12.5), (75.0, 12.5), (75.0, 0.0), (100.0, 25.0), (75.0, 50.0), (75.0, 37.5), (0.0, 37.5)],
        );
        // A thinner shaft and a longer head.
        let p = sized("rightArrow", 100.0, 50.0, &[("adj1", 20000.0), ("adj2", 100000.0)]);
        assert_pts(&ends(&p[0].ops), &[(0.0, 20.0), (50.0, 20.0), (50.0, 0.0), (100.0, 25.0), (50.0, 50.0), (50.0, 30.0), (0.0, 30.0)]);
        // adj2 pinned to maxAdj2 = 100000 * w / ss.
        let p = sized("rightArrow", 100.0, 50.0, &[("adj2", 900000.0)]);
        assert!(close(ends(&p[0].ops)[1].0, 0.0));
        let la = sized("leftArrow", 100.0, 50.0, &[]);
        assert_pts(&ends(&la[0].ops)[..3], &[(0.0, 25.0), (25.0, 0.0), (25.0, 12.5)]);
    }

    #[test]
    fn arc_maths() {
        // A quarter circle of radius 10 from 0 degrees: from (10, 0) around (0, 0) to (0, 10).
        let mut ops = vec![PathOp::MoveTo(10.0, 0.0)];
        let mut cur = (10.0, 0.0);
        arc_to(&mut ops, &mut cur, 10.0, 10.0, 0.0, 90.0);
        assert_eq!(ops.len(), 2);
        assert!(close(cur.0, 0.0) && close(cur.1, 10.0));
        let k = 4.0 / 3.0 * (PI / 8.0).tan() * 10.0;
        match ops[1] {
            PathOp::CubicTo(x1, y1, x2, y2, x, y) => {
                assert!(close(x1, 10.0) && close(y1, k) && close(x2, k) && close(y2, 10.0) && close(x, 0.0) && close(y, 10.0));
            }
            o => panic!("{:?}", o),
        }
        // A full negative turn: four segments back to the start.
        let mut ops = Vec::new();
        let mut cur = (5.0, 5.0);
        arc_to(&mut ops, &mut cur, 3.0, 2.0, 45.0, -360.0);
        assert_eq!(ops.len(), 4);
        assert!(close(cur.0, 5.0) && close(cur.1, 5.0));
        // Visual angles on an ellipse: starting at 45 degrees on a 100 x 50 ellipse, the centre
        // is where the 45-degree ray through the point meets.
        let mut ops = Vec::new();
        let t = (2.0f64).atan(); // tan t = (wR / hR) tan 45
        let start = (100.0 * t.cos(), 50.0 * t.sin());
        let mut cur = start;
        arc_to(&mut ops, &mut cur, 100.0, 50.0, 45.0, 180.0);
        // Half-way round: the opposite point (the centre is the origin).
        assert!(close(cur.0, -start.0) && close(cur.1, -start.1));
        // Zero sweep draws nothing; an enormous sweep is capped at one turn.
        let mut ops = Vec::new();
        let mut cur = (1.0, 1.0);
        arc_to(&mut ops, &mut cur, 5.0, 5.0, 10.0, 0.0);
        assert!(ops.is_empty());
        arc_to(&mut ops, &mut cur, 5.0, 5.0, 10.0, 1e12);
        assert_eq!(ops.len(), 4);
        arc_to(&mut ops, &mut cur, f64::NAN, f64::INFINITY, f64::NAN, 30.0);
        assert!(ops.iter().all(|o| ends(&[*o]).iter().all(|p| p.0.is_finite() && p.1.is_finite())));
    }

    #[test]
    fn pie_and_arc_presets() {
        // pie adj1 = 45 deg, adj2 = 270 deg on 200 x 100: starts at the 45-degree point.
        let p = sized("pie", 200.0, 100.0, &[("adj1", 2700000.0)]);
        let e = ends(&p[0].ops);
        let t = (2.0f64).atan();
        assert!(near(e[0].0, 100.0 + 100.0 * t.cos(), 1e-6) && near(e[0].1, 50.0 + 50.0 * t.sin(), 1e-6));
        // ... sweeps to 270 degrees (the top centre), then the centre.
        let n = e.len();
        assert!(near(e[n - 2].0, 100.0, 1e-6) && near(e[n - 2].1, 0.0, 1e-6), "{:?}", e);
        assert!(near(e[n - 1].0, 100.0, 1e-6) && near(e[n - 1].1, 50.0, 1e-6));
        // The default arc: a quarter from 270 to 0 degrees; two paths, the outline unfilled.
        let a = sized("arc", 100.0, 100.0, &[]);
        assert_eq!(a.len(), 2);
        assert!(!a[0].stroke && a[0].fill == PathFill::Norm);
        assert_eq!(a[1].fill, PathFill::None);
        let e = ends(&a[1].ops);
        assert_pts(&[e[0], *e.last().unwrap()], &[(50.0, 0.0), (100.0, 50.0)]);
        // chord closes straight.
        let c = sized("chord", 100.0, 100.0, &[]);
        assert_eq!(*c[0].ops.last().unwrap(), PathOp::Close);
        // blockArc's default is the upper half ring.
        let b = sized("blockArc", 100.0, 100.0, &[]);
        let e = ends(&b[0].ops);
        assert_pts(&[e[0]], &[(0.0, 50.0)]);
    }

    #[test]
    fn callouts_point_where_asked() {
        // wedgeRectCallout default: the pointer is at (hc + w * -0.20833, vc + h * 0.625).
        let p = sized("wedgeRectCallout", 120.0, 60.0, &[]);
        let tip = (60.0 - 120.0 * 0.20833, 30.0 + 60.0 * 0.625);
        assert!(ends(&p[0].ops).iter().any(|q| near(q.0, tip.0, 1e-6) && near(q.1, tip.1, 1e-6)));
        // Pointing right instead: the tip is on the right side's run.
        let p = sized("wedgeRectCallout", 120.0, 60.0, &[("adj1", 80000.0), ("adj2", 0.0)]);
        let e = ends(&p[0].ops);
        assert!(e.iter().any(|q| near(q.0, 60.0 + 96.0, 1e-6) && near(q.1, 30.0, 1e-6)), "{:?}", e);
        let r = sized("wedgeRoundRectCallout", 120.0, 60.0, &[]);
        assert!(ends(&r[0].ops).iter().any(|q| near(q.0, tip.0, 1e-6) && near(q.1, tip.1, 1e-6)));
        let el_ = sized("wedgeEllipseCallout", 120.0, 60.0, &[]);
        assert_pts(&ends(&el_[0].ops)[..1], &[tip]);
        let cloud = sized("cloudCallout", 120.0, 60.0, &[]);
        assert_eq!(cloud.len(), 4);
    }

    #[test]
    fn every_preset_is_sane() {
        let names = preset_names();
        assert!(names.len() >= 40, "{} presets", names.len());
        for want in ["rect", "roundRect", "ellipse", "star5", "heart", "can", "cube", "donut", "noSmoking", "frame", "flowChartDocument", "straightConnector1"] {
            assert!(names.contains(&want), "{}", want);
        }
        let sizes = [(100.0, 50.0), (50.0, 100.0), (0.0, 0.0), (1.0, 1000.0), (1e-3, 1e-3)];
        for name in &names {
            let def = preset_def(name).unwrap();
            assert!(!def.paths.is_empty(), "{}", name);
            for &(w, h) in &sizes {
                let mut adjs: Vec<Vec<(String, f64)>> = vec![vec![]];
                for v in [-1e7, 0.0, 1e7, 21599999.0] {
                    adjs.push(def.av.iter().map(|(n, _)| (n.clone(), v)).collect());
                }
                for adj in adjs {
                    let sg = shape_geometry(&Geometry::preset(name), pt(w), pt(h), &adj);
                    assert_eq!(sg.paths.len(), def.paths.len());
                    for p in &sg.paths {
                        assert!(matches!(p.ops.first(), Some(PathOp::MoveTo(..))), "{} starts with a move", name);
                        for q in ends(&p.ops) {
                            assert!(q.0.is_finite() && q.1.is_finite(), "{} {:?}", name, q);
                        }
                    }
                    let tr = sg.text_rect;
                    assert!(tr.x.is_finite() && tr.y.is_finite() && tr.w.is_finite() && tr.h.is_finite());
                }
            }
            // With default adjusts on a real box the shape stays (nearly) inside it, except the
            // callouts' pointers.
            if !name.contains("Callout") {
                let sg = shape_geometry(&Geometry::preset(name), pt(100.0), pt(80.0), &[]);
                for p in &sg.paths {
                    for q in ends(&p.ops) {
                        assert!(q.0 > -1.0 && q.0 < 101.0 && q.1 > -1.0 && q.1 < 81.0, "{} {:?}", name, q);
                    }
                }
            }
        }
    }

    #[test]
    fn preset_details() {
        // star5 top point and its inner points' symmetry.
        let s = sized("star5", 100.0, 100.0, &[]);
        let e = ends(&s[0].ops);
        assert_pts(&[e[2]], &[(50.0, 0.0)]);
        assert!(close(e[1].0 + e[3].0, 100.0) && close(e[1].1, e[3].1));
        // can: three paths with their modes.
        let c = sized("can", 50.0, 100.0, &[]);
        assert_eq!(c.iter().map(|p| (p.fill, p.stroke)).collect::<Vec<_>>(), vec![(PathFill::Norm, false), (PathFill::Lighten, false), (PathFill::None, true)]);
        // cube's faces.
        let c = sized("cube", 100.0, 100.0, &[]);
        assert_eq!(c.iter().map(|p| p.fill).collect::<Vec<_>>(), vec![PathFill::Norm, PathFill::DarkenLess, PathFill::LightenLess, PathFill::None]);
        // donut: two closed subpaths.
        let d = sized("donut", 100.0, 100.0, &[]);
        assert_eq!(d[0].ops.iter().filter(|o| **o == PathOp::Close).count(), 2);
        // noSmoking: three.
        let n = sized("noSmoking", 100.0, 100.0, &[]);
        assert_eq!(n[0].ops.iter().filter(|o| **o == PathOp::Close).count(), 3);
        // line: unfilled, corner to corner.
        let l = sized("line", 30.0, 40.0, &[]);
        assert_eq!(l[0].fill, PathFill::None);
        assert_eq!(l[0].ops, vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(30.0, 40.0)]);
        // bentConnector3 at adj1 = 25%.
        let b = sized("bentConnector3", 100.0, 40.0, &[("adj1", 25000.0)]);
        assert_pts(&ends(&b[0].ops), &[(0.0, 0.0), (25.0, 0.0), (25.0, 40.0), (100.0, 40.0)]);
        // Path-space scaling: flowChartDecision is drawn in a 2 x 2 space.
        let f = sized("flowChartDecision", 80.0, 40.0, &[]);
        assert_pts(&ends(&f[0].ops), &[(0.0, 20.0), (40.0, 0.0), (80.0, 20.0), (40.0, 40.0)]);
        let f = sized("flowChartTerminator", 216.0, 21.6, &[]);
        assert_pts(&ends(&f[0].ops)[..2], &[(34.75, 0.0), (181.25, 0.0)]);
        // frame: inner hole.
        let f = sized("frame", 100.0, 100.0, &[("adj1", 10000.0)]);
        assert!(f[0].ops.contains(&PathOp::MoveTo(10.0, 10.0)));
        // hexagon touches top and bottom with the default vf.
        let h = sized("hexagon", 100.0, 80.0, &[]);
        let e = ends(&h[0].ops);
        assert!(near(e[1].1, 0.0, 1e-3) && near(e[4].1, 80.0, 1e-3), "{:?}", e);
        // pentagon touches the bottom.
        let p = sized("pentagon", 100.0, 100.0, &[]);
        assert!(near(ends(&p[0].ops)[3].1, 100.0, 1e-2));
    }

    #[test]
    fn formulas() {
        let f = |s: &str| eval_formula(s, 1000.0, 500.0, &[("a".to_string(), 30.0), ("b".to_string(), -4.0)]);
        assert_eq!(f("*/ a 3 2"), 45.0);
        assert_eq!(f("*/ a 3 0"), 0.0);
        assert_eq!(f("+- a 5 b"), 39.0);
        assert_eq!(f("+/ a 10 4"), 10.0);
        assert_eq!(f("+/ a 10 0"), 0.0);
        assert_eq!(f("?: a 1 2"), 1.0);
        assert_eq!(f("?: b 1 2"), 2.0);
        assert_eq!(f("?: 0 1 2"), 2.0);
        assert_eq!(f("abs b"), 4.0);
        assert!(close(f("at2 1 1"), 2700000.0));
        assert!(close(f("at2 0 1"), 5400000.0));
        assert!(close(f("cat2 10 1 1"), 10.0 * (PI / 4.0).cos()));
        assert!(close(f("sat2 10 0 1"), 10.0));
        assert!(close(f("cos 10 5400000"), 0.0));
        assert!(close(f("cos 10 cd2"), -10.0));
        assert!(close(f("sin 10 5400000"), 10.0));
        assert!(close(f("tan 10 2700000"), 10.0));
        assert_eq!(f("max a b"), 30.0);
        assert_eq!(f("min a b"), -4.0);
        assert_eq!(f("mod 3 4 0"), 5.0);
        assert_eq!(f("mod 2 3 6"), 7.0);
        assert_eq!(f("pin 0 a 20"), 20.0);
        assert_eq!(f("pin 0 b 20"), 0.0);
        assert_eq!(f("pin 0 10 20"), 10.0);
        assert_eq!(f("sqrt 16"), 4.0);
        assert_eq!(f("sqrt b"), 0.0);
        assert_eq!(f("val a"), 30.0);
        assert_eq!(f("val 7"), 7.0);
        assert_eq!(f("42"), 42.0);
        assert_eq!(f(""), 0.0);
        assert_eq!(f("frob 1 2 3"), 0.0);
        assert_eq!(f("+- unknown 1"), 1.0);
        // Built-ins.
        assert_eq!(f("val w"), 1000.0);
        assert_eq!(f("val h"), 500.0);
        assert_eq!(f("val r"), 1000.0);
        assert_eq!(f("val b"), -4.0, "a guide shadows the built-in");
        assert_eq!(f("val hc"), 500.0);
        assert_eq!(f("val vc"), 250.0);
        assert_eq!(f("val ss"), 500.0);
        assert_eq!(f("val ls"), 1000.0);
        assert_eq!(f("val wd2"), 500.0);
        assert_eq!(f("val wd3"), 1000.0 / 3.0);
        assert_eq!(f("val wd12"), 1000.0 / 12.0);
        assert_eq!(f("val hd10"), 50.0);
        assert_eq!(f("val ssd32"), 500.0 / 32.0);
        assert_eq!(f("val ssd6"), 500.0 / 6.0);
        assert_eq!(f("val cd2"), 10800000.0);
        assert_eq!(f("val cd4"), 5400000.0);
        assert_eq!(f("val cd8"), 2700000.0);
        assert_eq!(f("val 3cd4"), 16200000.0);
        assert_eq!(f("val 3cd8"), 8100000.0);
        assert_eq!(f("val 5cd8"), 13500000.0);
        assert_eq!(f("val 7cd8"), 18900000.0);
        assert_eq!(f("val wd0"), 0.0);
        assert_eq!(f("val l"), 0.0);
        assert_eq!(f("val t"), 0.0);
        // Overflow becomes 0.
        assert_eq!(eval_formula("*/ 1e300 1e300 1", 1.0, 1.0, &[]), 0.0);
    }

    #[test]
    fn custom_geometry() {
        let sp = el(r#"<a:spPr><a:custGeom><a:avLst><a:gd name="k" fmla="val 25000"/></a:avLst>
            <a:gdLst><a:gd name="x1" fmla="*/ w k 100000"/><a:gd name="y1" fmla="+- b 0 x1"/></a:gdLst>
            <a:ahLst/><a:cxnSpLst/><a:rect l="x1" t="t" r="r" b="y1"/>
            <a:pathLst>
              <a:path w="100" h="100"><a:moveTo><a:pt x="0" y="0"/></a:moveTo><a:lnTo><a:pt x="100" y="0"/></a:lnTo>
                <a:quadBezTo><a:pt x="100" y="100"/><a:pt x="50" y="100"/></a:quadBezTo>
                <a:cubicBezTo><a:pt x="25" y="100"/><a:pt x="0" y="75"/><a:pt x="0" y="50"/></a:cubicBezTo><a:close/></a:path>
              <a:path fill="none" stroke="0"><a:moveTo><a:pt x="x1" y="t"/></a:moveTo><a:arcTo wR="x1" hR="x1" stAng="cd2" swAng="cd4"/></a:path>
              <a:path><a:lnTo><a:pt x="r" y="b"/></a:lnTo><a:bogus/></a:path>
            </a:pathLst></a:custGeom></a:spPr>"#);
        let g = Geometry::parse(&sp).unwrap();
        let sg = shape_geometry(&g, pt(200.0), pt(100.0), &[]);
        assert_eq!(sg.paths.len(), 3);
        // Path 1 in a 100 x 100 space scaled to 200 x 100.
        assert_eq!(
            sg.paths[0].ops,
            vec![
                PathOp::MoveTo(0.0, 0.0),
                PathOp::LineTo(200.0, 0.0),
                PathOp::QuadTo(200.0, 100.0, 100.0, 100.0),
                PathOp::CubicTo(50.0, 100.0, 0.0, 75.0, 0.0, 50.0),
                PathOp::Close
            ]
        );
        // Path 2 uses the guides: x1 = 50 pt; the arc goes from (50, 0) a quarter round.
        assert_eq!(sg.paths[1].fill, PathFill::None);
        assert!(!sg.paths[1].stroke);
        let e = ends(&sg.paths[1].ops);
        assert_pts(&[e[0], e[1]], &[(50.0, 0.0), (100.0, -50.0)]);
        // Path 3: a line with no move starts at the origin; unknown commands are skipped.
        assert_eq!(sg.paths[2].ops, vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(200.0, 100.0)]);
        assert_eq!(sg.text_rect, Rect::from_ltrb(50.0, 0.0, 200.0, 50.0));
        // An adjust override of a custom avLst.
        let sg = shape_geometry(&g, pt(200.0), pt(100.0), &[("k".to_string(), 50000.0)]);
        assert_eq!(sg.text_rect.x, 100.0);
        // ShapeProps reads it all.
        let props = ShapeProps::parse(&sp);
        assert!(matches!(props.geometry, Some(Geometry::Custom(_))));
        assert!(props.xfrm.is_none() && props.fill.is_none() && props.line.is_none());
    }

    // ----- transforms -----

    fn xfrm(xml: &str) -> Xfrm {
        Xfrm::parse(&el(xml))
    }

    fn close_pt(a: (f64, f64), b: (f64, f64)) -> bool {
        near(a.0, b.0, 1e-9) && near(a.1, b.1, 1e-9)
    }

    #[test]
    fn transforms() {
        // 100 x 50 pt at (100, 0).
        let plain = xfrm(r#"<a:xfrm><a:off x="1270000" y="0"/><a:ext cx="1270000" cy="635000"/></a:xfrm>"#);
        assert_eq!(plain.rect(), Rect::new(100.0, 0.0, 100.0, 50.0));
        assert!(close_pt(plain.matrix().apply(0.0, 0.0), (100.0, 0.0)));
        assert!(close_pt(plain.matrix().apply(100.0, 50.0), (200.0, 50.0)));
        // Rotated 90 degrees clockwise about the centre (150, 25).
        let rot = xfrm(r#"<a:xfrm rot="5400000"><a:off x="1270000" y="0"/><a:ext cx="1270000" cy="635000"/></a:xfrm>"#);
        assert_eq!(rot.rotation_deg(), 90.0);
        let m = rot.matrix();
        assert!(close_pt(m.apply(0.0, 0.0), (175.0, -25.0)));
        assert!(close_pt(m.apply(50.0, 25.0), (150.0, 25.0)));
        assert!(close_pt(m.apply(100.0, 0.0), (175.0, 75.0)));
        // Flips mirror within the box.
        let fh = xfrm(r#"<a:xfrm flipH="1"><a:off x="1270000" y="0"/><a:ext cx="1270000" cy="635000"/></a:xfrm>"#);
        assert!(close_pt(fh.matrix().apply(0.0, 0.0), (200.0, 0.0)));
        assert!(close_pt(fh.matrix().apply(100.0, 50.0), (100.0, 50.0)));
        let fv = xfrm(r#"<a:xfrm flipV="true"><a:off x="0" y="0"/><a:ext cx="1270000" cy="635000"/></a:xfrm>"#);
        assert!(close_pt(fv.matrix().apply(0.0, 0.0), (0.0, 50.0)));
        // Flip happens before rotation: flipH + 90 degrees.
        let both = xfrm(r#"<a:xfrm rot="5400000" flipH="1"><a:off x="0" y="0"/><a:ext cx="1270000" cy="1270000"/></a:xfrm>"#);
        assert!(close_pt(both.matrix().apply(0.0, 0.0), (100.0, 100.0)));
        // Parsed through spPr, and nothing at all.
        let sp = Xfrm::parse(&el(r#"<a:spPr><a:xfrm rot="-60000"><a:off x="-12700" y="25400"/><a:ext cx="0" cy="0"/></a:xfrm></a:spPr>"#));
        assert_eq!((sp.off, sp.rot), ((-12700, 25400), -60000));
        assert_eq!(Xfrm::parse(&el(r#"<a:spPr/>"#)), Xfrm::default());
        assert!(Xfrm::default().matrix().is_identity());
    }

    #[test]
    fn group_transforms() {
        // A group at (100, 100) pt, 200 x 100 pt, whose children live in a 100 x 50 space at (10, 10).
        let g = xfrm(r#"<a:xfrm><a:off x="1270000" y="1270000"/><a:ext cx="2540000" cy="1270000"/><a:chOff x="127000" y="127000"/><a:chExt cx="1270000" cy="635000"/></a:xfrm>"#);
        let cm = g.child_matrix();
        assert!(close_pt(cm.apply(10.0, 10.0), (100.0, 100.0)));
        assert!(close_pt(cm.apply(110.0, 60.0), (300.0, 200.0)));
        // A child filling the child space lands on the group's box.
        let child = xfrm(r#"<a:xfrm><a:off x="127000" y="127000"/><a:ext cx="1270000" cy="635000"/></a:xfrm>"#);
        let full = cm.multiply(&child.matrix());
        assert!(close_pt(full.apply(0.0, 0.0), (100.0, 100.0)));
        assert!(close_pt(full.apply(100.0, 50.0), (300.0, 200.0)));
        // A rotated group turns its children about its own centre.
        let rg = xfrm(r#"<a:xfrm rot="10800000"><a:off x="0" y="0"/><a:ext cx="1270000" cy="1270000"/><a:chOff x="0" y="0"/><a:chExt cx="1270000" cy="1270000"/></a:xfrm>"#);
        assert!(close_pt(rg.child_matrix().apply(0.0, 0.0), (100.0, 100.0)));
        // A zero child extent does not divide by zero.
        let z = xfrm(r#"<a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="100"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm>"#);
        let (x, y) = z.child_matrix().apply(5.0, 5.0);
        assert!(x.is_finite() && y.is_finite());
    }

    // ----- shape items -----

    #[test]
    fn shape_items_emit_paths() {
        let t = Theme::office_default();
        let cm = ColorMap::default();
        let x = xfrm(r#"<a:xfrm rot="2700000"><a:off x="0" y="0"/><a:ext cx="1270000" cy="635000"/></a:xfrm>"#);
        let paths = geometry(&Geometry::preset("rect"), 1270000.0, 635000.0, &[]);
        let fill = Fill::Solid(t.colors.accent1);
        let line = t.line_ref(1).unwrap().resolve(&t, &cm, Some(Color::BLACK));
        let items = shape_items(&paths, Some(&fill), Some(&line), Some(&x));
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], Item::PushTransform(x.matrix()));
        match &items[1] {
            Item::Path { ops, fill, stroke } => {
                assert_eq!(ops, &paths[0].ops);
                assert_eq!(fill, &Some(Paint::Solid(t.colors.accent1)));
                assert!(close(stroke.as_ref().unwrap().width, 0.5));
            }
            i => panic!("{:?}", i),
        }
        assert_eq!(items[2], Item::PopTransform);
        // No fill and no line: nothing but the transform pair; no xfrm: no transform.
        assert_eq!(shape_items(&paths, None, None, Some(&x)).len(), 2);
        let bare = shape_items(&paths, Some(&fill), None, None);
        assert_eq!(bare.len(), 1);

        // Fill modes and stroke flags: the can's lighten face is lighter, its outline unfilled.
        let can = geometry(&Geometry::preset("can"), 635000.0, 1270000.0, &[]);
        let red = Fill::Solid(Color::rgb(200, 0, 0));
        let items = shape_items(&can, Some(&red), Some(&line), None);
        assert_eq!(items.len(), 3);
        match (&items[0], &items[1], &items[2]) {
            (Item::Path { fill: f0, stroke: s0, .. }, Item::Path { fill: f1, stroke: s1, .. }, Item::Path { fill: f2, stroke: s2, .. }) => {
                assert_eq!(f0, &Some(Paint::Solid(Color::rgb(200, 0, 0))));
                assert!(s0.is_none() && s1.is_none() && s2.is_some() && f2.is_none());
                assert_eq!(f1, &Some(Paint::Solid(Color::rgb(222, 102, 102))));
            }
            i => panic!("{:?}", i),
        }
        let p = Paint::Solid(Color::rgb(100, 100, 100));
        assert_eq!(modulate(&p, PathFill::Darken), Some(Paint::Solid(Color::rgb(60, 60, 60))));
        assert_eq!(modulate(&p, PathFill::DarkenLess), Some(Paint::Solid(Color::rgb(80, 80, 80))));
        assert_eq!(modulate(&p, PathFill::LightenLess), Some(Paint::Solid(Color::rgb(131, 131, 131))));
        assert_eq!(modulate(&p, PathFill::None), None);

        // A gradient fill spans the xfrm box.
        let g: Fill = Fill::Gradient(Gradient {
            stops: vec![Stop { pos: 0.0, color: Color::BLACK }, Stop { pos: 1.0, color: Color::WHITE }],
            kind: GradientKind::Linear { angle: 0.0, scaled: false },
            rot_with_shape: true,
            flip: None,
        });
        let items = shape_items(&paths, Some(&g), None, Some(&x));
        match &items[1] {
            Item::Path { fill: Some(Paint::Linear { x0, x1, .. }), .. } => assert!(close(*x0, 0.0) && close(*x1, 100.0)),
            i => panic!("{:?}", i),
        }
    }

    #[test]
    fn arrowheads_on_open_paths() {
        let t = Theme::office_default();
        let cm = ColorMap::default();
        let line = parse_line(&el(r#"<a:ln w="12700"><a:solidFill><a:srgbClr val="000000"/></a:solidFill><a:prstDash val="dash"/><a:headEnd type="arrow"/><a:tailEnd type="triangle" w="sm" len="lg"/></a:ln>"#)).resolve(&t, &cm, None);
        let paths = geometry(&Geometry::preset("straightConnector1"), pt(100.0), 0.0, &[]);
        let items = shape_items(&paths, None, Some(&line), None);
        assert_eq!(items.len(), 3, "{:?}", items);
        // The head (an open arrow) at the start, pointing left.
        match &items[1] {
            Item::Path { ops, fill: None, stroke: Some(s) } => {
                assert!(s.dash.is_empty(), "line ends are solid");
                assert_eq!(ops[1], PathOp::LineTo(0.0, 0.0));
                assert!(ends(ops).iter().all(|p| p.0 >= 0.0));
            }
            i => panic!("{:?}", i),
        }
        // The tail (a filled triangle, 5 x 2 widths) at the end.
        match &items[2] {
            Item::Path { ops, fill: Some(Paint::Solid(c)), stroke: None } => {
                assert_eq!(*c, Color::BLACK);
                assert_pts(&ends(ops), &[(100.0, 0.0), (95.0, 1.0), (95.0, -1.0)]);
            }
            i => panic!("{:?}", i),
        }
        // Closed paths get no ends.
        let rect = geometry(&Geometry::preset("rect"), pt(10.0), pt(10.0), &[]);
        assert_eq!(shape_items(&rect, None, Some(&line), None).len(), 1);
        // Every kind draws something finite.
        for kind in [LineEndKind::Triangle, LineEndKind::Stealth, LineEndKind::Diamond, LineEndKind::Oval, LineEndKind::Arrow] {
            let end = LineEnd { kind, w: EndSize::Medium, len: EndSize::Medium };
            let (ops, _) = line_end_path(&end, (10.0, 10.0), (0.0, 10.0), 1.0).unwrap();
            let b = path_bounds(&ops).unwrap();
            assert!(b.w <= 3.0 + 1e-9 && b.h <= 3.0 + 1e-9 && b.right() <= 11.5 + 1e-9, "{:?} {:?}", kind, b);
        }
        assert!(line_end_path(&LineEnd { kind: LineEndKind::None, w: EndSize::Small, len: EndSize::Small }, (0.0, 0.0), (1.0, 0.0), 1.0).is_none());
        assert!(line_end_path(&LineEnd { kind: LineEndKind::Triangle, w: EndSize::Small, len: EndSize::Small }, (0.0, 0.0), (0.0, 0.0), 1.0).is_none());
        // A curved connector's tail points along its last tangent.
        let curve = geometry(&Geometry::preset("curvedConnector3"), pt(100.0), pt(50.0), &[]);
        let (tip, from) = path_end(&curve[0].ops).unwrap();
        assert!(close_pt(tip, (100.0, 50.0)) && from.0 < 100.0 && close(from.1, 50.0));
    }

    #[test]
    fn malformed_input_never_panics() {
        let t = Theme::office_default();
        let cm = ColorMap::default();
        // Garbage values everywhere.
        let junk = [
            r##"<a:srgbClr val="#12"><a:lumMod val="abc"/><a:tint/><a:hueOff val="NaN"/></a:srgbClr>"##,
            r#"<a:srgbClr val="+12345"/>"#,
            r#"<a:srgbClr val="ÿÿÿÿÿÿ"/>"#,
            r#"<a:hslClr hue="99999999999999999999" sat="-5" lum="1e400"/>"#,
            r#"<a:scrgbClr r="inf" g="-inf" b=""/>"#,
            r#"<a:schemeClr/>"#,
            r#"<a:schemeClr val="accent1"><a:lumMod val="1e308"/><a:lumOff val="-1e308"/><a:alphaMod val="1e308"/><a:hueMod val="1e308"/></a:schemeClr>"#,
            r#"<a:sysClr/>"#,
            r#"<a:prstClr/>"#,
        ];
        for j in junk {
            let _ = resolve_color(&el(j), &t, &cm, None);
        }
        let _ = parse_fill(&el(r#"<a:gradFill><a:gsLst><a:gs pos="x"/><a:gs><a:srgbClr val="FF0000"/></a:gs></a:gsLst><a:lin ang="1e999"/></a:gradFill>"#))
            .unwrap()
            .resolve(&t, &cm, None)
            .to_paint(Rect::new(0.0, 0.0, 0.0, 0.0));
        let _ = parse_fill(&el(r#"<a:pattFill/>"#)).unwrap().resolve(&t, &cm, None).to_paint(Rect::new(0.0, 0.0, 1.0, 1.0));
        let _ = parse_fill(&el(r#"<a:blipFill><a:tile sx="q"/></a:blipFill>"#));
        let _ = parse_line(&el(r#"<a:ln w="-5" cap="zz"><a:custDash><a:ds/></a:custDash><a:miter lim="-1"/><a:headEnd type="?" w="huge"/></a:ln>"#)).resolve(&t, &cm, None).to_stroke();
        let _ = ColorMap::parse(&el(r#"<p:clrMap bg1="" tx1="zzz" nonsense="1"/>"#));
        // Theme bytes that are cut, binary, or UTF-16 noise.
        let full = DEFAULT_THEME_XML.as_bytes();
        for cut in [1, 10, 100, 1000, full.len() / 2, full.len() - 1] {
            let _ = Theme::parse(&full[..cut]);
        }
        let _ = Theme::parse(&[0xff, 0xfe, 0x00, 0x00, 0x3c]);
        let _ = Theme::parse(&(0u8..=255).collect::<Vec<u8>>());
        // Geometry with nonsense.
        let sp = el(r#"<a:custGeom><a:gdLst><a:gd name="a" fmla="*/ a a"/><a:gd fmla="tan 1 cd4"/><a:gd name="b" fmla="+/ 1"/></a:gdLst>
            <a:rect l="nope"/><a:pathLst><a:path w="-10" h="abc"><a:arcTo wR="a" hR="b" stAng="?" swAng="99999999999999999"/>
            <a:cubicBezTo><a:pt x="1e309"/></a:cubicBezTo><a:quadBezTo/><a:close/><a:close/></a:path><a:path/></a:pathLst></a:custGeom>"#);
        let g = Geometry::parse(&sp).unwrap();
        for (w, h) in [(0.0, 0.0), (-5.0, 7.0), (f64::NAN, f64::INFINITY), (1e300, 1e300)] {
            let sg = shape_geometry(&g, w, h, &[("a".into(), f64::NAN)]);
            for p in &sg.paths {
                for q in ends(&p.ops) {
                    assert!(q.0.is_finite() && q.1.is_finite());
                }
            }
            let _ = shape_items(&sg.paths, Some(&Fill::Solid(Color::BLACK)), None, None);
        }
        assert!(Geometry::parse(&el(r#"<a:spPr/>"#)).is_none());
        assert_eq!(Geometry::parse(&el(r#"<a:prstGeom/>"#)), Some(Geometry::preset("rect")));
        let _ = Xfrm::parse(&el(r#"<a:xfrm rot="x" flipH="maybe"><a:off x="1e5"/><a:ext/></a:xfrm>"#)).matrix();
        // The preset notation reader tolerates junk.
        let m = parse_presets("@a\nav\ngd\nZ\nQ 1\n@b c\npath w=x fill=? stroke=no\nM\nfrob 1 2\n;;;\n");
        assert!(m.contains_key("a") && m.contains_key("b") && m.contains_key("c"));
        let _ = evaluate(&m["a"], 10.0, 10.0, &[], &[]);
    }
}
