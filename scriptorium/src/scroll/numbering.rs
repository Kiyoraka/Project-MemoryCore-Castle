//! `word/numbering.xml`: abstract numbering definitions (nine levels each), the `w:num` instances
//! that point at them (with `w:lvlOverride` / `w:startOverride`), and the counters that walk the
//! document in order to give each list paragraph its label ("1.", "a)", "iv.", a bullet).
//!
//! Counters are kept per abstract definition (Word continues a list across `w:num` instances of the
//! same abstract unless one of them overrides the start), and a level restarts when a higher level
//! is used (or as `w:lvlRestart` says).

use super::props::{num_attr, on_off, val, Ctx, ParaProps, RunProps};
use super::styles::Styles;
use crate::ground::xml::{parse_dom, Element};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suffix {
    Tab,
    Space,
    Nothing,
}

/// One `w:lvl`.
#[derive(Debug, Clone)]
pub struct Level {
    pub start: i64,
    /// `w:numFmt` value (`decimal`, `lowerLetter`, `bullet`...).
    pub fmt: String,
    /// `w:lvlText` (`%1.`, `%1.%2)`, a bullet character).
    pub text: String,
    pub suffix: Suffix,
    pub ppr: ParaProps,
    pub rpr: RunProps,
    /// `w:isLgl`: every number in the label shown as decimal.
    pub legal: bool,
    /// `w:lvlRestart` (1-based level; 0 = never restart).
    pub restart: Option<i64>,
}

#[derive(Debug, Clone, Default)]
struct Abstract {
    levels: Vec<Option<Level>>,
    /// `w:numStyleLink`: the levels live in the numbering style's own list.
    style_link: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct NumInst {
    abs: i64,
    /// ilvl -> (start override, replacement level).
    overrides: HashMap<u8, (Option<i64>, Option<Level>)>,
}

/// The numbering part.
#[derive(Debug, Clone, Default)]
pub struct Numbering {
    abstracts: HashMap<i64, Abstract>,
    nums: HashMap<i64, NumInst>,
}

/// The counters of one walk through the document (one per layout).
#[derive(Debug, Clone, Default)]
pub struct Counters {
    counts: HashMap<i64, [Option<i64>; 9]>,
    started: HashSet<i64>,
}

fn parse_level(el: &Element, ctx: &Ctx) -> Level {
    Level {
        start: el.child("start").and_then(|e| num_attr(e, "val")).map(|v| v as i64).unwrap_or(0),
        fmt: el.child("numFmt").and_then(val).unwrap_or("decimal").trim().to_string(),
        text: el.child("lvlText").and_then(val).unwrap_or("").to_string(),
        suffix: match el.child("suff").and_then(val).map(str::trim) {
            Some("space") => Suffix::Space,
            Some("nothing") => Suffix::Nothing,
            _ => Suffix::Tab,
        },
        ppr: el.child("pPr").map(|p| ParaProps::parse(p, ctx)).unwrap_or_default(),
        rpr: el.child("rPr").map(|r| RunProps::parse(r, ctx)).unwrap_or_default(),
        legal: el.child("isLgl").is_some_and(on_off),
        restart: el.child("lvlRestart").and_then(|e| num_attr(e, "val")).map(|v| v as i64),
    }
}

impl Numbering {
    pub fn parse(bytes: &[u8], ctx: &Ctx) -> Numbering {
        let mut n = Numbering::default();
        let Ok(root) = parse_dom(bytes) else { return n };
        for a in root.children_named("abstractNum") {
            let Some(id) = num_attr(a, "abstractNumId") else { continue };
            let mut abs = Abstract { levels: vec![None; 9], style_link: a.child("numStyleLink").and_then(val).map(str::to_string) };
            for l in a.children_named("lvl") {
                let i = num_attr(l, "ilvl").unwrap_or(0.0) as usize;
                if i < 9 {
                    abs.levels[i] = Some(parse_level(l, ctx));
                }
            }
            n.abstracts.insert(id as i64, abs);
        }
        for m in root.children_named("num") {
            let Some(id) = num_attr(m, "numId") else { continue };
            let abs = m.child("abstractNumId").and_then(|e| num_attr(e, "val")).unwrap_or(-1.0) as i64;
            let mut inst = NumInst { abs, overrides: HashMap::new() };
            for o in m.children_named("lvlOverride") {
                let i = num_attr(o, "ilvl").unwrap_or(0.0).clamp(0.0, 8.0) as u8;
                let start = o.child("startOverride").and_then(|e| num_attr(e, "val")).map(|v| v as i64);
                let lvl = o.child("lvl").map(|l| parse_level(l, ctx));
                inst.overrides.insert(i, (start, lvl));
            }
            n.nums.insert(id as i64, inst);
        }
        n
    }

    pub fn is_empty(&self) -> bool {
        self.nums.is_empty()
    }

    /// The abstract id a `w:num` uses, following one `w:numStyleLink` through the styles.
    fn abstract_of(&self, num_id: i64, styles: &Styles) -> Option<i64> {
        let inst = self.nums.get(&num_id)?;
        let mut abs_id = inst.abs;
        for _ in 0..4 {
            let abs = self.abstracts.get(&abs_id)?;
            let Some(link) = &abs.style_link else { return Some(abs_id) };
            let (p, _) = styles.folded(link);
            let linked = p.num_id.and_then(|n| self.nums.get(&n))?;
            if linked.abs == abs_id {
                return Some(abs_id);
            }
            abs_id = linked.abs;
        }
        Some(abs_id)
    }

    /// The level definition a paragraph with (`num_id`, `ilvl`) uses.
    pub fn level(&self, num_id: i64, ilvl: u8, styles: &Styles) -> Option<Level> {
        let inst = self.nums.get(&num_id)?;
        if let Some((_, Some(l))) = inst.overrides.get(&ilvl) {
            return Some(l.clone());
        }
        let abs = self.abstracts.get(&self.abstract_of(num_id, styles)?)?;
        let mut l = abs.levels.get(ilvl as usize)?.clone()?;
        if let Some((Some(s), _)) = inst.overrides.get(&ilvl) {
            l.start = *s;
        }
        Some(l)
    }

    /// Count one list paragraph and give its label text, with the level it used. `None` when the
    /// numbering does not exist (or `num_id` is 0, which turns numbering off).
    pub fn next_label(&self, counters: &mut Counters, num_id: i64, ilvl: u8, styles: &Styles) -> Option<(String, Level)> {
        if num_id == 0 {
            return None;
        }
        let abs_id = self.abstract_of(num_id, styles)?;
        let level = self.level(num_id, ilvl, styles)?;
        let inst = self.nums.get(&num_id)?;
        let counts = counters.counts.entry(abs_id).or_insert([None; 9]);
        if counters.started.insert(num_id) {
            // The first paragraph of an instance with start overrides restarts those levels.
            for (l, (start, _)) in &inst.overrides {
                if start.is_some() {
                    counts[*l as usize] = None;
                }
            }
        }
        let i = ilvl as usize;
        counts[i] = Some(match counts[i] {
            Some(c) => c + 1,
            None => level.start,
        });
        for (deeper, slot) in counts.iter_mut().enumerate().skip(i + 1) {
            let restart = self.level(num_id, deeper as u8, styles).and_then(|l| l.restart);
            let resets = match restart {
                None => true,
                Some(0) => false,
                Some(r) => (i as i64) < r,
            };
            if resets {
                *slot = None;
            }
        }
        let snapshot = *counts;
        if level.fmt == "bullet" {
            return Some((bullet_text(&level.text, level.rpr.font.as_deref()), level));
        }
        if level.fmt == "none" {
            return Some((String::new(), level));
        }
        let mut out = String::new();
        let mut chars = level.text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%' {
                if let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                    chars.next();
                    if (1..=9).contains(&d) {
                        let li = d as usize - 1;
                        let lvl_def = self.level(num_id, li as u8, styles);
                        let start = lvl_def.as_ref().map(|l| l.start).unwrap_or(1);
                        let value = snapshot[li].unwrap_or(start);
                        let fmt = if level.legal && li != i {
                            "decimal".to_string()
                        } else if li == i {
                            level.fmt.clone()
                        } else {
                            lvl_def.map(|l| l.fmt).unwrap_or_else(|| "decimal".into())
                        };
                        out.push_str(&format_number(value, &fmt));
                    }
                    continue;
                }
            }
            out.push(c);
        }
        Some((out, level))
    }
}

/// A bullet's text: symbol-font private-use characters mapped to their Unicode look-alikes.
pub fn bullet_text(text: &str, font: Option<&str>) -> String {
    let symbolic = font.is_some_and(|f| {
        let f = f.to_ascii_lowercase();
        f.contains("symbol") || f.contains("wingdings") || f.contains("webdings")
    });
    text.chars()
        .map(|c| {
            let cp = c as u32;
            let low = if (0xF000..=0xF0FF).contains(&cp) { cp - 0xF000 } else if symbolic { cp } else { return c };
            match low {
                0xB7 => '\u{2022}',
                0xA7 => '\u{25AA}',
                0xA8 => '\u{25CB}',
                0x6E => '\u{25A0}',
                0x71 => '\u{25A1}',
                0x76 => '\u{2756}',
                0xD8 => '\u{27A2}',
                0xFC => '\u{2713}',
                0x6F => 'o',
                0x2D => '\u{2013}',
                _ if symbolic || (0xF000..=0xF0FF).contains(&cp) => '\u{2022}',
                _ => c,
            }
        })
        .collect()
}

fn roman(mut n: i64, upper: bool) -> String {
    if n <= 0 || n >= 4000 {
        return n.to_string();
    }
    let table = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut s = String::new();
    for (v, t) in table {
        while n >= v {
            s.push_str(t);
            n -= v;
        }
    }
    if upper {
        s.to_ascii_uppercase()
    } else {
        s
    }
}

/// Word's letter numbering: a..z, then aa..zz, aaa.. (the letter repeated).
fn letters(n: i64, upper: bool) -> String {
    if n <= 0 {
        return n.to_string();
    }
    let idx = ((n - 1) % 26) as u8;
    let reps = ((n - 1) / 26 + 1) as usize;
    let c = if upper { b'A' + idx } else { b'a' + idx } as char;
    std::iter::repeat(c).take(reps.min(50)).collect()
}

fn ordinal_suffix(n: i64) -> &'static str {
    let t = n.rem_euclid(100);
    if (11..=13).contains(&t) {
        return "th";
    }
    match n.rem_euclid(10) {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    }
}

/// A counter value in a `w:numFmt` (unknown formats fall back to decimal).
pub fn format_number(n: i64, fmt: &str) -> String {
    match fmt {
        "lowerLetter" => letters(n, false),
        "upperLetter" => letters(n, true),
        "lowerRoman" => roman(n, false),
        "upperRoman" => roman(n, true),
        "decimalZero" => {
            if (0..10).contains(&n) {
                format!("0{n}")
            } else {
                n.to_string()
            }
        }
        "ordinal" => format!("{n}{}", ordinal_suffix(n)),
        "none" => String::new(),
        _ => n.to_string(),
    }
}
