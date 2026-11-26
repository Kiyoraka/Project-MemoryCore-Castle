//! Segmentation: grapheme clusters (UAX #29, rules GB1-GB13 and GB999 of Unicode 15.0) and line
//! break opportunities (UAX #14, rules LB1-LB31 of Unicode 15.0) with the Office kinsoku tailoring
//! for Japanese and Chinese text.
//!
//! The line breaker evaluates the rules in order for each pair of characters (not a precomputed
//! pair table): that keeps LB9/LB10 (combining marks), the `SP*` look-backs of LB14-LB17, the
//! numeric run of LB25 and the regional-indicator pairs of LB30a exact.

use super::tables::{
    grapheme_break, is_east_asian_fwh, is_extended_pictographic, line_break, GraphemeBreak, LineBreak,
};
use std::collections::HashSet;
use std::ops::Range;

// ---------------------------------------------------------------------------------------------------
// Graphemes
// ---------------------------------------------------------------------------------------------------

/// Extended grapheme clusters of `s`, as byte ranges in order. Created by [`graphemes`].
#[derive(Debug, Clone)]
pub struct Graphemes<'a> {
    s: &'a str,
    pos: usize,
}

/// The extended grapheme clusters of a string (what a user sees as one character: "e" + U+0301,
/// a Hangul syllable written as jamo, a flag, a family emoji joined by ZWJ).
pub fn graphemes(s: &str) -> Graphemes<'_> {
    Graphemes { s, pos: 0 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pict {
    None,
    /// Extended_Pictographic Extend*
    Seen,
    /// Extended_Pictographic Extend* ZWJ
    Zwj,
}

impl<'a> Iterator for Graphemes<'a> {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Range<usize>> {
        let start = self.pos;
        let rest = self.s.get(start..)?;
        let mut it = rest.char_indices();
        let (_, c0) = it.next()?;
        let mut prev = grapheme_break(c0);
        let mut ri_run = u32::from(prev == GraphemeBreak::RegionalIndicator);
        let mut pict = if is_extended_pictographic(c0) { Pict::Seen } else { Pict::None };
        let mut end = c0.len_utf8();
        for (i, c) in it {
            let cur = grapheme_break(c);
            let cur_pict = is_extended_pictographic(c);
            if grapheme_boundary(prev, cur, pict, ri_run, cur_pict) {
                break;
            }
            pict = if cur_pict {
                Pict::Seen
            } else if cur == GraphemeBreak::Extend && pict == Pict::Seen {
                Pict::Seen
            } else if cur == GraphemeBreak::ZWJ && pict == Pict::Seen {
                Pict::Zwj
            } else {
                Pict::None
            };
            ri_run = if cur == GraphemeBreak::RegionalIndicator { ri_run + 1 } else { 0 };
            prev = cur;
            end = i + c.len_utf8();
        }
        self.pos = start + end;
        Some(start..start + end)
    }
}

fn grapheme_boundary(prev: GraphemeBreak, cur: GraphemeBreak, pict: Pict, ri_run: u32, cur_pict: bool) -> bool {
    use GraphemeBreak::*;
    match (prev, cur) {
        // GB3
        (CR, LF) => false,
        // GB4, GB5
        (Control | CR | LF, _) | (_, Control | CR | LF) => true,
        // GB6-GB8
        (L, L | V | LV | LVT) | (LV | V, V | T) | (LVT | T, T) => false,
        // GB9, GB9a
        (_, Extend | ZWJ | SpacingMark) => false,
        // GB9b
        (Prepend, _) => false,
        // GB11
        (ZWJ, _) if pict == Pict::Zwj && cur_pict => false,
        // GB12, GB13
        (RegionalIndicator, RegionalIndicator) => ri_run % 2 == 0,
        // GB999
        _ => true,
    }
}

/// True when `offset` is a grapheme cluster boundary of `s` (0 and `s.len()` always are).
pub fn is_grapheme_boundary(s: &str, offset: usize) -> bool {
    if offset == 0 || offset >= s.len() {
        return offset == 0 || offset == s.len();
    }
    if !s.is_char_boundary(offset) {
        return false;
    }
    graphemes(s).any(|g| g.start == offset)
}

// ---------------------------------------------------------------------------------------------------
// Line breaks
// ---------------------------------------------------------------------------------------------------

/// What kind of line break an opportunity is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakKind {
    /// The line must end here (after a hard line break character, and at the end of the text).
    Mandatory,
    /// The line may end here.
    Allowed,
}

/// The tailoring of UAX #14 for a document's languages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakOptions {
    /// East Asian context: ambiguous-width characters (AI: circled numbers, some symbols and
    /// Greek/Cyrillic-adjacent punctuation) break like ideographs. Otherwise they are alphabetic.
    pub cjk: bool,
    /// Kinsoku: characters that may never begin a line (closing brackets, 、。, small kana, the
    /// prolonged sound mark ー, iteration marks...). Applied on top of UAX #14, never to a
    /// mandatory break.
    pub kinsoku_no_start: Vec<char>,
    /// Kinsoku: characters that may never end a line (opening brackets and quotes, prefix
    /// currency signs).
    pub kinsoku_no_end: Vec<char>,
}

/// Office's Japanese "standard" kinsoku: characters that may not start a line. ASCII is left out
/// because UAX #14 already keeps it off the line start (CL/CP/EX/IS/NS/PO).
pub const KINSOKU_NO_START: &str = concat!(
    "、。，．・：；？！゛゜ヽヾゝゞ々〻ー‐゠–〜～‼⁇⁈⁉",
    "’”）〕］｝〉》」』】〙〗〟｠»›",
    "ぁぃぅぇぉっゃゅょゎゕゖァィゥェォッャュョヮヵヶㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ",
    "％°′″℃¢‰￠｡｣､･ｧｨｩｪｫｬｭｮｯｰﾞﾟ",
);

/// Office's Japanese "standard" kinsoku: characters that may not end a line.
pub const KINSOKU_NO_END: &str = "‘“（〔［｛〈《「『【〘〖〝｟«‹＄￡￥＃｢";

impl Default for BreakOptions {
    fn default() -> Self {
        BreakOptions {
            cjk: false,
            kinsoku_no_start: KINSOKU_NO_START.chars().collect(),
            kinsoku_no_end: KINSOKU_NO_END.chars().collect(),
        }
    }
}

impl BreakOptions {
    /// East Asian context (AI resolves to ID) with the standard kinsoku sets.
    pub fn east_asian() -> Self {
        BreakOptions { cjk: true, ..BreakOptions::default() }
    }

    /// Plain UAX #14: no kinsoku sets (small kana may then start a line).
    pub fn plain() -> Self {
        BreakOptions { cjk: false, kinsoku_no_start: Vec::new(), kinsoku_no_end: Vec::new() }
    }
}

/// Line break opportunities of `s` with the default options: byte offsets where a line may (or
/// must) end, in order. The offset is where the next line starts, so spaces stay at the end of the
/// line before ("Hello world" gives 6, not 5). The end of the text is always the last entry
/// (LB3, Mandatory); an empty string has none.
pub fn line_break_opportunities(s: &str) -> Vec<(usize, BreakKind)> {
    line_break_opportunities_with(s, &BreakOptions::default())
}

/// [`line_break_opportunities`] with explicit tailoring.
pub fn line_break_opportunities_with(s: &str, opts: &BreakOptions) -> Vec<(usize, BreakKind)> {
    use LineBreak::*;
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    // LB1: resolve AI, SA, CJ, XX.
    let cls: Vec<LineBreak> = chars.iter().map(|&(_, c)| resolve(c, opts.cjk)).collect();
    // LB9/LB10: combining marks take the class of their base; a mark without one is AL.
    let mut eff = cls.clone();
    let mut base: Vec<usize> = (0..n).collect();
    for i in 0..n {
        if matches!(cls[i], CM | ZWJ) {
            if i > 0 && !matches!(eff[i - 1], BK | CR | LF | NL | SP | ZW) {
                eff[i] = eff[i - 1];
                base[i] = base[i - 1];
            } else {
                eff[i] = AL;
            }
        }
    }
    let no_start: HashSet<char> = opts.kinsoku_no_start.iter().copied().collect();
    let no_end: HashSet<char> = opts.kinsoku_no_end.iter().copied().collect();
    let ctx = Ctx { chars: &chars, cls: &cls, eff: &eff, base: &base };
    for i in 1..n {
        let Some(kind) = ctx.pair(i) else { continue };
        if kind == BreakKind::Allowed && (!no_start.is_empty() || !no_end.is_empty()) {
            if no_start.contains(&chars[i].1) {
                continue;
            }
            let mut j = i - 1;
            while j > 0 && cls[j] == SP {
                j -= 1;
            }
            if no_end.contains(&chars[j].1) {
                continue;
            }
        }
        out.push((chars[i].0, kind));
    }
    out.push((s.len(), BreakKind::Mandatory));
    out
}

/// LB1 for one character.
fn resolve(c: char, cjk: bool) -> LineBreak {
    use LineBreak::*;
    match line_break(c) {
        AI => {
            if cjk {
                ID
            } else {
                AL
            }
        }
        XX => AL,
        SA => match grapheme_break(c) {
            GraphemeBreak::Extend | GraphemeBreak::SpacingMark => CM,
            _ => AL,
        },
        // Small kana break like ideographs; the kinsoku sets keep them off the line start (the
        // Office behaviour), and clearing the sets gives the "loose" CSS behaviour.
        CJ => ID,
        other => other,
    }
}

struct Ctx<'a> {
    chars: &'a [(usize, char)],
    cls: &'a [LineBreak],
    eff: &'a [LineBreak],
    base: &'a [usize],
}

impl Ctx<'_> {
    /// The index before `i` once spaces are skipped (`SP*`), if any.
    fn before_spaces(&self, i: usize) -> Option<usize> {
        let mut j = i.checked_sub(1)?;
        loop {
            if self.eff[j] != LineBreak::SP {
                return Some(j);
            }
            j = j.checked_sub(1)?;
        }
    }

    /// True when the characters ending at `j` (inclusive) are `NU (NU|SY|IS)*`.
    fn numeric_before(&self, j: usize) -> bool {
        use LineBreak::*;
        let mut k = j;
        loop {
            match self.eff[k] {
                NU => return true,
                SY | IS => {}
                _ => return false,
            }
            if k == 0 {
                return false;
            }
            k -= 1;
        }
    }

    /// The next base character after `i` (skipping attached marks).
    fn next_base(&self, i: usize) -> Option<usize> {
        let mut k = i + 1;
        while k < self.eff.len() && self.base[k] != k {
            k += 1;
        }
        (k < self.eff.len()).then_some(k)
    }

    /// The rules LB4-LB31 for the boundary before character `i` (`i >= 1`).
    fn pair(&self, i: usize) -> Option<BreakKind> {
        use BreakKind::*;
        use LineBreak::*;
        let (a0, b0) = (self.cls[i - 1], self.cls[i]);
        // LB4, LB5
        if a0 == BK {
            return Some(Mandatory);
        }
        if a0 == CR && b0 == LF {
            return None;
        }
        if matches!(a0, CR | LF | NL) {
            return Some(Mandatory);
        }
        // LB6, LB7
        if matches!(b0, BK | CR | LF | NL | SP | ZW) {
            return None;
        }
        // LB8
        if let Some(j) = self.before_spaces(i) {
            if self.cls[j] == ZW {
                return Some(Allowed);
            }
        }
        // LB8a
        if a0 == ZWJ {
            return None;
        }
        // LB9
        if self.base[i] != i {
            return None;
        }
        let a = self.eff[i - 1];
        let b = self.eff[i];
        let prev = self.before_spaces(i).map(|j| self.eff[j]);
        // LB11
        if a == WJ || b == WJ {
            return None;
        }
        // LB12, LB12a
        if a == GL {
            return None;
        }
        if b == GL && !matches!(a, SP | BA | HY) {
            return None;
        }
        // LB13
        if matches!(b, CL | CP | EX | IS | SY) {
            return None;
        }
        // LB14
        if prev == Some(OP) {
            return None;
        }
        // LB15
        if b == OP && prev == Some(QU) {
            return None;
        }
        // LB16
        if b == NS && matches!(prev, Some(CL | CP)) {
            return None;
        }
        // LB17
        if b == B2 && prev == Some(B2) {
            return None;
        }
        // LB18
        if a == SP {
            return Some(Allowed);
        }
        // LB19
        if a == QU || b == QU {
            return None;
        }
        // LB20
        if a == CB || b == CB {
            return Some(Allowed);
        }
        // LB21
        if matches!(b, BA | HY | NS) || a == BB {
            return None;
        }
        // LB21a
        if matches!(a, HY | BA) {
            let j = self.base[i - 1];
            if j > 0 && self.eff[j - 1] == HL {
                return None;
            }
        }
        // LB21b
        if a == SY && b == HL {
            return None;
        }
        // LB22
        if b == IN {
            return None;
        }
        // LB23
        if (matches!(a, AL | HL) && b == NU) || (a == NU && matches!(b, AL | HL)) {
            return None;
        }
        // LB23a
        if (a == PR && matches!(b, ID | EB | EM)) || (matches!(a, ID | EB | EM) && b == PO) {
            return None;
        }
        // LB24
        if (matches!(a, PR | PO) && matches!(b, AL | HL)) || (matches!(a, AL | HL) && matches!(b, PR | PO)) {
            return None;
        }
        // LB25 (the regular-expression form of the numeric rule)
        if matches!(a, PR | PO) && b == NU {
            return None;
        }
        if matches!(a, PR | PO) && matches!(b, OP | HY) && self.next_base(i).map(|k| self.eff[k]) == Some(NU) {
            return None;
        }
        if matches!(a, OP | HY) && b == NU {
            return None;
        }
        if matches!(b, NU | SY | IS | CL | CP) && self.numeric_before(i - 1) {
            return None;
        }
        if matches!(b, PO | PR) {
            let mut j = self.base[i - 1];
            if matches!(self.eff[j], CL | CP) && j > 0 {
                j -= 1;
            }
            if self.numeric_before(j) {
                return None;
            }
        }
        // LB26
        if a == JL && matches!(b, JL | JV | H2 | H3) {
            return None;
        }
        if matches!(a, JV | H2) && matches!(b, JV | JT) {
            return None;
        }
        if matches!(a, JT | H3) && b == JT {
            return None;
        }
        // LB27
        if (matches!(a, JL | JV | JT | H2 | H3) && b == PO) || (a == PR && matches!(b, JL | JV | JT | H2 | H3)) {
            return None;
        }
        // LB28
        if matches!(a, AL | HL) && matches!(b, AL | HL) {
            return None;
        }
        // LB29
        if a == IS && matches!(b, AL | HL) {
            return None;
        }
        // LB30
        if matches!(a, AL | HL | NU) && b == OP && !is_east_asian_fwh(self.chars[i].1) {
            return None;
        }
        if a == CP && matches!(b, AL | HL | NU) && !is_east_asian_fwh(self.chars[self.base[i - 1]].1) {
            return None;
        }
        // LB30a
        if a == RI && b == RI {
            let mut count = 0;
            let mut j = i;
            while j > 0 {
                j -= 1;
                if self.base[j] != j {
                    continue;
                }
                if self.cls[j] == RI {
                    count += 1;
                } else {
                    break;
                }
            }
            if count % 2 == 1 {
                return None;
            }
        }
        // LB30b
        if a == EB && b == EM {
            return None;
        }
        // LB31
        Some(Allowed)
    }
}
