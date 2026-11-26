//! Text and body properties as PresentationML writes them, every field optional so that a level of the
//! inheritance chain (slide, layout placeholder, master placeholder, master text styles, the
//! presentation's default text style) only fills what the levels above it left open.

use crate::ground::display::Color;
use crate::ground::draw::{BaseColor, ColorSpec};
use crate::ground::xml::Element;

/// A number attribute.
pub(crate) fn num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// An ST_Percentage: `"100000"` (thousandths of a percent) or `"100%"`, as a fraction.
pub(crate) fn pct(s: &str) -> Option<f64> {
    let t = s.trim();
    match t.strip_suffix('%') {
        Some(p) => num(p).map(|v| v / 100.0),
        None => num(t).map(|v| v / 100000.0),
    }
}

/// An on/off attribute.
pub(crate) fn on(s: &str) -> bool {
    matches!(s.trim(), "1" | "true" | "on")
}

/// EMU attribute as points.
pub(crate) fn emu_attr_pt(el: &Element, name: &str) -> Option<f64> {
    el.attr(name).and_then(num).map(|v| v / 12700.0)
}

/// `a:rPr`, `a:defRPr`, `a:endParaRPr`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunProps {
    /// Points.
    pub size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// Typeface as written (may be a theme reference such as `+mj-lt`).
    pub latin: Option<String>,
    pub ea: Option<String>,
    pub color: Option<ColorSpec>,
    /// Character spacing, points.
    pub spacing: Option<f64>,
    /// Superscript / subscript offset as a fraction of the size.
    pub baseline: Option<f64>,
    /// `cap`: none / small / all.
    pub caps: Option<String>,
}

impl RunProps {
    pub fn parse(el: &Element) -> RunProps {
        let mut p = RunProps {
            size: el.attr("sz").and_then(num).map(|v| v / 100.0),
            bold: el.attr("b").map(on),
            italic: el.attr("i").map(on),
            underline: el.attr("u").map(|u| u.trim() != "none"),
            strike: el.attr("strike").map(|s| s.trim() != "noStrike"),
            spacing: el.attr("spc").and_then(num).map(|v| v / 100.0),
            baseline: el.attr("baseline").and_then(pct),
            caps: el.attr("cap").map(String::from),
            ..RunProps::default()
        };
        for c in el.elements() {
            match c.local() {
                "latin" => p.latin = c.attr("typeface").map(String::from),
                "ea" => p.ea = c.attr("typeface").map(String::from),
                "solidFill" => p.color = ColorSpec::parse(c),
                "gradFill" => {
                    p.color = c.child("gsLst").and_then(|l| l.children_named("gs").next()).and_then(ColorSpec::parse);
                }
                "noFill" => {
                    p.color = Some(ColorSpec { base: BaseColor::Rgb([0.0; 3]), mods: Vec::new() }.with(crate::ground::draw::ColorMod::Alpha(0.0)));
                }
                _ => {}
            }
        }
        p
    }

    /// Fill every unset field from `lower`.
    pub fn fill_from(&mut self, lower: &RunProps) {
        macro_rules! take {
            ($($f:ident),*) => { $( if self.$f.is_none() { self.$f = lower.$f.clone(); } )* };
        }
        take!(size, bold, italic, underline, strike, latin, ea, color, spacing, baseline, caps);
    }
}

/// Line or paragraph spacing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    /// A fraction of the line (1.0 = single).
    Pct(f64),
    /// Points.
    Pts(f64),
}

impl Spacing {
    fn parse(el: &Element) -> Option<Spacing> {
        if let Some(p) = el.child("spcPct") {
            return p.attr("val").and_then(pct).map(Spacing::Pct);
        }
        if let Some(p) = el.child("spcPts") {
            return p.attr("val").and_then(num).map(|v| Spacing::Pts(v / 100.0));
        }
        None
    }
}

/// What a paragraph's bullet is.
#[derive(Debug, Clone, PartialEq)]
pub enum BulletKind {
    None,
    Char(String),
    AutoNum { scheme: String, start: u32 },
}

/// The bullet's size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BulletSize {
    FollowText,
    Pct(f64),
    Pts(f64),
}

/// `a:pPr` and the `a:lvlNpPr` of list styles.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParaProps {
    /// `algn`: l, ctr, r, just, dist ...
    pub align: Option<String>,
    /// Points.
    pub mar_l: Option<f64>,
    pub mar_r: Option<f64>,
    /// Points (negative: hanging).
    pub indent: Option<f64>,
    pub line_spacing: Option<Spacing>,
    pub space_before: Option<Spacing>,
    pub space_after: Option<Spacing>,
    pub bullet: Option<BulletKind>,
    /// `Some(None)`: the bullet follows the text's font (`a:buFontTx`).
    pub bullet_font: Option<Option<String>>,
    /// `Some(None)`: the bullet follows the text's colour (`a:buClrTx`).
    pub bullet_color: Option<Option<ColorSpec>>,
    pub bullet_size: Option<BulletSize>,
    /// Default tab size, points.
    pub def_tab: Option<f64>,
    /// Tab stops (points, kind).
    pub tabs: Option<Vec<(f64, String)>>,
    /// The level's default run properties (`a:defRPr`).
    pub run: RunProps,
}

impl ParaProps {
    pub fn parse(el: &Element) -> ParaProps {
        let mut p = ParaProps {
            align: el.attr("algn").map(String::from),
            mar_l: emu_attr_pt(el, "marL"),
            mar_r: emu_attr_pt(el, "marR"),
            indent: emu_attr_pt(el, "indent"),
            def_tab: emu_attr_pt(el, "defTabSz"),
            ..ParaProps::default()
        };
        for c in el.elements() {
            match c.local() {
                "lnSpc" => p.line_spacing = Spacing::parse(c),
                "spcBef" => p.space_before = Spacing::parse(c),
                "spcAft" => p.space_after = Spacing::parse(c),
                "buNone" => p.bullet = Some(BulletKind::None),
                "buChar" => p.bullet = Some(BulletKind::Char(c.attr("char").unwrap_or("\u{2022}").to_string())),
                "buAutoNum" => {
                    p.bullet = Some(BulletKind::AutoNum {
                        scheme: c.attr("type").unwrap_or("arabicPeriod").to_string(),
                        start: c.attr("startAt").and_then(|s| s.trim().parse().ok()).unwrap_or(1),
                    })
                }
                "buBlip" => p.bullet = Some(BulletKind::Char("\u{2022}".into())),
                "buFontTx" => p.bullet_font = Some(None),
                "buFont" => p.bullet_font = Some(c.attr("typeface").map(String::from)),
                "buClrTx" => p.bullet_color = Some(None),
                "buClr" => p.bullet_color = Some(ColorSpec::parse(c)),
                "buSzTx" => p.bullet_size = Some(BulletSize::FollowText),
                "buSzPct" => p.bullet_size = c.attr("val").and_then(pct).map(BulletSize::Pct),
                "buSzPts" => p.bullet_size = c.attr("val").and_then(num).map(|v| BulletSize::Pts(v / 100.0)),
                "tabLst" => {
                    p.tabs = Some(
                        c.children_named("tab")
                            .map(|t| (emu_attr_pt(t, "pos").unwrap_or(0.0), t.attr("algn").unwrap_or("l").to_string()))
                            .collect(),
                    )
                }
                "defRPr" => p.run = RunProps::parse(c),
                _ => {}
            }
        }
        p
    }

    /// Fill every unset field (run properties included) from `lower`.
    pub fn fill_from(&mut self, lower: &ParaProps) {
        macro_rules! take {
            ($($f:ident),*) => { $( if self.$f.is_none() { self.$f = lower.$f.clone(); } )* };
        }
        take!(align, mar_l, mar_r, indent, line_spacing, space_before, space_after, bullet, bullet_font, bullet_color, bullet_size, def_tab, tabs);
        self.run.fill_from(&lower.run);
    }
}

/// `a:lstStyle`, `p:titleStyle` / `p:bodyStyle` / `p:otherStyle`, `p:defaultTextStyle`: nine levels.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListStyle {
    pub levels: Vec<ParaProps>,
}

impl ListStyle {
    pub fn parse(el: &Element) -> ListStyle {
        let def = el.child("defPPr").map(ParaProps::parse);
        let levels = (1..=9)
            .map(|i| {
                let mut p = el.child(&format!("lvl{}pPr", i)).map(ParaProps::parse).unwrap_or_default();
                if let Some(d) = &def {
                    p.fill_from(d);
                }
                p
            })
            .collect();
        ListStyle { levels }
    }

    /// Level 0..=8 (clamped).
    pub fn level(&self, lvl: usize) -> Option<&ParaProps> {
        self.levels.get(lvl.min(8))
    }
}

/// `a:normAutofit` / `a:spAutoFit` / `a:noAutofit`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Autofit {
    None,
    /// Shrink text on overflow: font scale and line spacing reduction, as fractions.
    Normal { font_scale: f64, line_reduction: f64 },
    /// Resize the shape to fit the text.
    Shape,
}

/// `a:bodyPr`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BodyProps {
    /// Insets, points.
    pub l: Option<f64>,
    pub t: Option<f64>,
    pub r: Option<f64>,
    pub b: Option<f64>,
    /// t, ctr, b (just, dist treated as t).
    pub anchor: Option<String>,
    pub anchor_ctr: Option<bool>,
    /// `wrap="square"` (true) or `"none"`.
    pub wrap: Option<bool>,
    pub autofit: Option<Autofit>,
    pub vert: Option<String>,
    pub num_col: Option<u32>,
}

impl BodyProps {
    pub fn parse(el: &Element) -> BodyProps {
        let mut b = BodyProps {
            l: emu_attr_pt(el, "lIns"),
            t: emu_attr_pt(el, "tIns"),
            r: emu_attr_pt(el, "rIns"),
            b: emu_attr_pt(el, "bIns"),
            anchor: el.attr("anchor").map(String::from),
            anchor_ctr: el.attr("anchorCtr").map(on),
            wrap: el.attr("wrap").map(|w| w.trim() != "none"),
            vert: el.attr("vert").map(String::from),
            num_col: el.attr("numCol").and_then(|s| s.trim().parse().ok()),
            ..BodyProps::default()
        };
        for c in el.elements() {
            match c.local() {
                "noAutofit" => b.autofit = Some(Autofit::None),
                "spAutoFit" => b.autofit = Some(Autofit::Shape),
                "normAutofit" => {
                    b.autofit = Some(Autofit::Normal {
                        font_scale: c.attr("fontScale").and_then(pct).unwrap_or(1.0).clamp(0.01, 1.0),
                        line_reduction: c.attr("lnSpcReduction").and_then(pct).unwrap_or(0.0).clamp(0.0, 0.9),
                    })
                }
                _ => {}
            }
        }
        b
    }

    pub fn fill_from(&mut self, lower: &BodyProps) {
        macro_rules! take {
            ($($f:ident),*) => { $( if self.$f.is_none() { self.$f = lower.$f.clone(); } )* };
        }
        take!(l, t, r, b, anchor, anchor_ctr, wrap, autofit, vert, num_col);
    }

    /// The insets with PowerPoint's defaults (0.1 in left/right, 0.05 in top/bottom), points.
    pub fn insets(&self) -> (f64, f64, f64, f64) {
        (self.l.unwrap_or(7.2), self.t.unwrap_or(3.6), self.r.unwrap_or(7.2), self.b.unwrap_or(3.6))
    }

    /// (font scale, line spacing reduction) of a normal autofit, (1, 0) otherwise.
    pub fn shrink(&self) -> (f64, f64) {
        match self.autofit {
            Some(Autofit::Normal { font_scale, line_reduction }) => (font_scale, line_reduction),
            _ => (1.0, 0.0),
        }
    }
}

/// A colour that is fully transparent (from `a:noFill` on a run).
pub(crate) fn is_invisible(c: Color) -> bool {
    c.a == 0
}

/// An autonumber in a scheme (`arabicPeriod`, `romanUcParenR`, `alphaLcParenBoth`, ...).
pub fn autonum_text(scheme: &str, n: u32) -> String {
    let (base, suffix) = if let Some(b) = scheme.strip_suffix("ParenBoth") {
        (b, "both")
    } else if let Some(b) = scheme.strip_suffix("ParenR") {
        (b, ")")
    } else if let Some(b) = scheme.strip_suffix("Period") {
        (b, ".")
    } else if let Some(b) = scheme.strip_suffix("Plain") {
        (b, "")
    } else if let Some(b) = scheme.strip_suffix("Minus") {
        (b, "-")
    } else {
        (scheme, ".")
    };
    let body = match base {
        "romanUc" => roman(n),
        "romanLc" => roman(n).to_lowercase(),
        "alphaUc" => alpha(n),
        "alphaLc" => alpha(n).to_lowercase(),
        "circleNumDbPlain" | "circleNumWdBlackPlain" | "circleNumWdWhitePlain" => n.to_string(),
        _ => n.to_string(),
    };
    match suffix {
        "both" => format!("({})", body),
        s => format!("{}{}", body, s),
    }
}

fn roman(mut n: u32) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    const T: [(u32, &str); 13] =
        [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut s = String::new();
    for (v, r) in T {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// A, B, ..., Z, AA, BB, ... (PowerPoint repeats the letter).
fn alpha(n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let letter = (b'A' + ((n - 1) % 26) as u8) as char;
    std::iter::repeat(letter).take(((n - 1) / 26 + 1) as usize).collect()
}
