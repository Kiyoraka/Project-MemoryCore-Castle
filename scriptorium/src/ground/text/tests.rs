//! Tests of the text engine: tables, graphemes, line breaks, layout on a stub measurer, and real
//! system fonts when this machine has them (skipped silently otherwise).

use super::tables::*;
use super::*;
use crate::ground::display::{Item, PlacedGlyph, Resources};
use crate::ground::font::FontIndex;
use std::sync::{Arc, OnceLock};

// ---------------------------------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------------------------------

#[test]
fn tables_are_sorted() {
    all_tables_sorted().unwrap();
}

#[test]
fn table_spot_checks() {
    use GraphemeBreak as G;
    use LineBreak as L;
    assert_eq!(grapheme_break('a'), G::Other);
    assert_eq!(grapheme_break('\r'), G::CR);
    assert_eq!(grapheme_break('\u{301}'), G::Extend);
    assert_eq!(grapheme_break('\u{200D}'), G::ZWJ);
    assert_eq!(grapheme_break('\u{1F1EF}'), G::RegionalIndicator);
    assert_eq!(grapheme_break('\u{1100}'), G::L);
    assert_eq!(grapheme_break('\u{1161}'), G::V);
    assert_eq!(grapheme_break('\u{11A8}'), G::T);
    assert_eq!(grapheme_break('가'), G::LV);
    assert_eq!(grapheme_break('각'), G::LVT);
    assert_eq!(grapheme_break('\u{93F}'), G::SpacingMark);
    assert_eq!(grapheme_break('\u{1F3FD}'), G::Extend);
    assert!(is_extended_pictographic('😀'));
    assert!(is_extended_pictographic('©'));
    assert!(!is_extended_pictographic('\u{1F3FD}'));
    assert!(!is_extended_pictographic('A'));

    assert_eq!(line_break(' '), L::SP);
    assert_eq!(line_break('a'), L::AL);
    assert_eq!(line_break('Ж'), L::AL);
    assert_eq!(line_break('5'), L::NU);
    assert_eq!(line_break('$'), L::PR);
    assert_eq!(line_break('%'), L::PO);
    assert_eq!(line_break('('), L::OP);
    assert_eq!(line_break(')'), L::CP);
    assert_eq!(line_break('-'), L::HY);
    assert_eq!(line_break('\u{2014}'), L::B2);
    assert_eq!(line_break('\u{A0}'), L::GL);
    assert_eq!(line_break('\u{200B}'), L::ZW);
    assert_eq!(line_break('\u{2060}'), L::WJ);
    assert_eq!(line_break('\u{301}'), L::CM);
    assert_eq!(line_break('\u{94D}'), L::CM);
    assert_eq!(line_break('漢'), L::ID);
    assert_eq!(line_break('\u{20000}'), L::ID);
    assert_eq!(line_break('あ'), L::ID);
    assert_eq!(line_break('ぁ'), L::CJ);
    assert_eq!(line_break('ー'), L::CJ);
    assert_eq!(line_break('。'), L::CL);
    assert_eq!(line_break('「'), L::OP);
    assert_eq!(line_break('」'), L::CL);
    assert_eq!(line_break('ゝ'), L::NS);
    assert_eq!(line_break('가'), L::H2);
    assert_eq!(line_break('각'), L::H3);
    assert_eq!(line_break('\u{1100}'), L::JL);
    assert_eq!(line_break('\u{1161}'), L::JV);
    assert_eq!(line_break('\u{11A8}'), L::JT);
    assert_eq!(line_break('\u{1F1EF}'), L::RI);
    assert_eq!(line_break('👍'), L::EB);
    assert_eq!(line_break('\u{1F3FD}'), L::EM);
    assert_eq!(line_break('😀'), L::ID);
    assert_eq!(line_break('①'), L::AI);
    assert_eq!(line_break('ก'), L::SA);
    assert_eq!(line_break('\u{FFFC}'), L::CB);
    assert_eq!(line_break('\u{2026}'), L::IN);
    assert_eq!(line_break('א'), L::HL);
    assert_eq!(line_break('（'), L::OP);
    assert_eq!(line_break('！'), L::EX);

    assert_eq!(east_asian_width('a'), EastAsianWidth::N);
    assert_eq!(east_asian_width('漢'), EastAsianWidth::W);
    assert_eq!(east_asian_width('Ａ'), EastAsianWidth::F);
    assert_eq!(east_asian_width('ｱ'), EastAsianWidth::H);
    assert_eq!(east_asian_width('①'), EastAsianWidth::A);
    assert_eq!(east_asian_width('😀'), EastAsianWidth::W);

    assert_eq!(coarse_category('a'), Category::Letter);
    assert_eq!(coarse_category('漢'), Category::Letter);
    assert_eq!(coarse_category('7'), Category::Number);
    assert_eq!(coarse_category('\u{301}'), Category::Mark);
    assert_eq!(coarse_category(','), Category::Punctuation);
    assert_eq!(coarse_category('「'), Category::Punctuation);
    assert_eq!(coarse_category('$'), Category::Symbol);
    assert_eq!(coarse_category('😀'), Category::Symbol);
    assert_eq!(coarse_category(' '), Category::Separator);
    assert_eq!(coarse_category('\u{3000}'), Category::Separator);
    assert_eq!(coarse_category('\n'), Category::Control);
    assert_eq!(coarse_category('\u{200D}'), Category::Other);

    assert!(is_default_ignorable('\u{AD}'));
    assert!(is_default_ignorable('\u{FE0F}'));
    assert!(!is_default_ignorable('a'));
    assert!(is_cjk('漢') && is_cjk('あ') && is_cjk('。') && is_cjk('Ａ') && is_cjk('한'));
    assert!(!is_cjk('a') && !is_cjk('😀'));
}

// ---------------------------------------------------------------------------------------------------
// Graphemes
// ---------------------------------------------------------------------------------------------------

fn clusters(s: &str) -> Vec<&str> {
    graphemes(s).map(|r| &s[r]).collect()
}

#[test]
fn graphemes_combining_and_hangul() {
    assert_eq!(clusters("e\u{301}x"), vec!["e\u{301}", "x"]);
    assert_eq!(clusters("a\u{300}\u{316}b"), vec!["a\u{300}\u{316}", "b"]);
    // Hangul: conjoining jamo L V T, LV + T, LVT + T, LV + V; LVT + V breaks.
    assert_eq!(clusters("\u{1100}\u{1161}\u{11A8}"), vec!["\u{1100}\u{1161}\u{11A8}"]);
    assert_eq!(clusters("\u{AC00}\u{11A8}"), vec!["\u{AC00}\u{11A8}"]);
    assert_eq!(clusters("\u{AC01}\u{11A8}"), vec!["\u{AC01}\u{11A8}"]);
    assert_eq!(clusters("\u{AC00}\u{1161}"), vec!["\u{AC00}\u{1161}"]);
    assert_eq!(clusters("\u{AC01}\u{1161}"), vec!["\u{AC01}", "\u{1161}"]);
    assert_eq!(clusters("한국어"), vec!["한", "국", "어"]);
    // CR LF is one cluster; controls stand alone.
    assert_eq!(clusters("a\r\nb"), vec!["a", "\r\n", "b"]);
    assert_eq!(clusters("\n\u{301}"), vec!["\n", "\u{301}"]);
    // Prepend and SpacingMark.
    assert_eq!(clusters("\u{600}1"), vec!["\u{600}1"]);
    assert_eq!(clusters("कि"), vec!["कि"]);
    assert_eq!(clusters(""), Vec::<&str>::new());
}

#[test]
fn graphemes_emoji() {
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
    assert_eq!(clusters(family), vec![family]);
    assert_eq!(clusters(&format!("a{family}b")), vec!["a", family, "b"]);
    // Skin tone modifier, VS16, keycap.
    assert_eq!(clusters("👍🏽👍"), vec!["👍🏽", "👍"]);
    assert_eq!(clusters("❤\u{FE0F}x"), vec!["❤\u{FE0F}", "x"]);
    assert_eq!(clusters("1\u{FE0F}\u{20E3}"), vec!["1\u{FE0F}\u{20E3}"]);
    // A ZWJ not after a pictograph does not join.
    assert_eq!(clusters("a\u{200D}b"), vec!["a\u{200D}", "b"]);
    // Flags pair up regional indicators.
    assert_eq!(clusters("🇯🇵🇲🇾"), vec!["🇯🇵", "🇲🇾"]);
    assert_eq!(clusters("🇯🇵🇲"), vec!["🇯🇵", "🇲"]);
    assert!(is_grapheme_boundary("e\u{301}x", 0));
    assert!(!is_grapheme_boundary("e\u{301}x", 1));
    assert!(is_grapheme_boundary("e\u{301}x", 3));
    assert!(is_grapheme_boundary("e\u{301}x", 4));
}

// ---------------------------------------------------------------------------------------------------
// Line breaks
// ---------------------------------------------------------------------------------------------------

fn allowed(s: &str) -> Vec<usize> {
    line_break_opportunities(s).into_iter().filter(|b| b.1 == BreakKind::Allowed).map(|b| b.0).collect()
}

fn allowed_with(s: &str, o: &BreakOptions) -> Vec<usize> {
    line_break_opportunities_with(s, o).into_iter().filter(|b| b.1 == BreakKind::Allowed).map(|b| b.0).collect()
}

/// Character indices (not bytes) of the allowed breaks.
fn allowed_chars(s: &str) -> Vec<usize> {
    let offs = allowed(s);
    s.char_indices().enumerate().filter(|(_, (b, _))| offs.contains(b)).map(|(i, _)| i).collect()
}

#[test]
fn breaks_spaces_and_ends() {
    assert_eq!(line_break_opportunities("Hello world"), vec![(6, BreakKind::Allowed), (11, BreakKind::Mandatory)]);
    assert_eq!(line_break_opportunities(""), vec![]);
    assert_eq!(allowed("a  b"), vec![3]);
    assert_eq!(line_break_opportunities("a\nb"), vec![(2, BreakKind::Mandatory), (3, BreakKind::Mandatory)]);
    assert_eq!(line_break_opportunities("a\r\nb"), vec![(3, BreakKind::Mandatory), (4, BreakKind::Mandatory)]);
    assert_eq!(line_break_opportunities("a \u{2028}b")[0], (5, BreakKind::Mandatory));
    // No break before a line break, even after a space.
    assert_eq!(allowed("a \nb"), vec![]);
    // NBSP and word joiner glue; ZWSP opens a break.
    assert_eq!(allowed("a\u{A0}b c"), vec![5]);
    assert_eq!(allowed("ab\u{2060}cd"), vec![]);
    assert_eq!(allowed("ab\u{200B}cd"), vec![5]);
    // A combining mark stays with its base.
    assert_eq!(allowed("e\u{301} x"), vec![4]);
}

#[test]
fn breaks_hyphens_and_punctuation() {
    assert_eq!(allowed("well-known"), vec![5]);
    assert_eq!(allowed("a - b"), vec![2, 4]);
    // An em dash breaks on either side, but not between two.
    assert_eq!(allowed_chars("a\u{2014}b"), vec![1, 2]);
    assert_eq!(allowed_chars("a\u{2014}\u{2014}b"), vec![1, 3]);
    // Closing punctuation and quotes do not start a line.
    assert_eq!(allowed("end. Next"), vec![5]);
    assert_eq!(allowed("Hi! (yes) ok"), vec![4, 10]);
    assert_eq!(allowed("say \"hi\" now"), vec![4, 9]);
    // Ellipsis (IN) never starts a line.
    assert_eq!(allowed("wait\u{2026}"), vec![]);
}

#[test]
fn breaks_urls_and_numbers() {
    // URL-ish: breaks only after the solidus, never inside "example.com" or after ':'.
    let url = "http://example.com/path";
    let b = allowed(url);
    assert!(!b.is_empty());
    for off in &b {
        assert_eq!(&url[off - 1..*off], "/", "break at {off} in {url}");
    }
    // Numbers with punctuation stay whole.
    assert_eq!(allowed("$1,234.50 x"), vec![10]);
    assert_eq!(allowed("(12.5) x"), vec![7]);
    assert_eq!(allowed("100% sure"), vec![5]);
    assert_eq!(allowed("-5 C"), vec![3]);
    assert_eq!(allowed("¥1,000"), vec![]);
    assert_eq!(allowed("1/2 cup"), vec![4]);
}

#[test]
fn breaks_cjk_and_kinsoku() {
    // Between ideographs.
    assert_eq!(line_break_opportunities("日本語"), vec![(3, BreakKind::Allowed), (6, BreakKind::Allowed), (9, BreakKind::Mandatory)]);
    // Never before 。 or 」, never after 「.
    assert_eq!(allowed_chars("日本。日本"), vec![1, 3, 4]);
    assert_eq!(allowed_chars("「日本」です"), vec![2, 4, 5]);
    // Small kana and the prolonged sound mark stay off the line start (kinsoku)...
    assert_eq!(allowed_chars("ちょっと"), vec![3]);
    assert_eq!(allowed_chars("コーヒー"), vec![2]);
    // ...unless the kinsoku sets are cleared.
    let plain = BreakOptions::plain();
    let offs = allowed_with("ちょっと", &plain);
    assert_eq!(offs, vec![3, 6, 9]);
    // Custom kinsoku: forbid breaking before 本.
    let mut custom = BreakOptions::default();
    custom.kinsoku_no_start.push('本');
    assert_eq!(allowed_with("日本語", &custom), vec![6]);
    custom.kinsoku_no_end.push('語');
    assert_eq!(allowed_with("日本語日", &custom), vec![6]);
    // Fullwidth parentheses and punctuation.
    assert_eq!(allowed_chars("（注）本文、終わり。"), vec![3, 4, 6, 7, 8]);
    // Latin next to CJK may break; a fullwidth comma keeps to the left.
    assert_eq!(allowed_chars("ABC日本"), vec![3, 4]);
    // Iteration mark 々 is a non-starter.
    assert_eq!(allowed_chars("人々"), vec![]);
    // Ambiguous characters: alphabetic outside an East Asian context, ideographic inside.
    assert_eq!(allowed("a①b"), vec![]);
    assert_eq!(allowed_with("a①b", &BreakOptions::east_asian()), vec![1, 4]);
}

#[test]
fn breaks_emoji_hangul_and_others() {
    // Flags: a break between two flags, never inside one.
    assert_eq!(allowed("🇯🇵🇲🇾"), vec![8]);
    // Emoji base + modifier stays together.
    assert_eq!(allowed("👍🏽👍"), vec![8]);
    // ZWJ sequences stay together.
    let family = "\u{1F468}\u{200D}\u{1F469}";
    assert_eq!(allowed(family), vec![]);
    // Hangul jamo sequences stay together; syllables break like ideographs.
    assert_eq!(allowed("\u{1100}\u{1161}\u{11A8}"), vec![]);
    assert_eq!(allowed_chars("한국 어"), vec![1, 3]);
    // Thai (SA) is resolved to alphabetic: no dictionary, so no breaks inside a word.
    assert_eq!(allowed("ภาษาไทย"), vec![]);
    // Hebrew letter + hyphen does not break (LB21a).
    assert_eq!(allowed("א-ב"), vec![]);
}

// ---------------------------------------------------------------------------------------------------
// Layout on a stub measurer
// ---------------------------------------------------------------------------------------------------

/// Latin graphemes 0.5 em, spaces 0.25 em, CJK 1 em; ascent 0.8 em, descent 0.2 em, no gap.
struct Stub;

impl Stub {
    fn adv(c: char, size: f64) -> f64 {
        if c == ' ' {
            0.25 * size
        } else if is_cjk(c) {
            size
        } else {
            0.5 * size
        }
    }
}

impl Measurer for Stub {
    fn shape(&mut self, text: &str, style: &TextStyle) -> Vec<ShapedRun> {
        let s = style.size_pt;
        let mut glyphs = Vec::new();
        let mut width = 0.0;
        for g in graphemes(text) {
            let c = text[g.clone()].chars().next().unwrap();
            if c == '\u{AD}' {
                continue;
            }
            let a = Stub::adv(c, s) + style.letter_spacing;
            glyphs.push(ShapedGlyph { gid: (c as u32 & 0xFFFF) as u16, advance: a, cluster: g });
            width += a;
        }
        vec![ShapedRun { font: 1, font_ref: None, size: s, glyphs, width, metrics: FontMetrics::synthetic(s) }]
    }

    fn font_metrics(&mut self, style: &TextStyle) -> FontMetrics {
        FontMetrics::synthetic(style.size_pt)
    }
}

fn st(size: f64) -> TextStyle {
    TextStyle::new("Stub", size)
}

fn lay(text: &str, opts: &ParaOptions) -> ParagraphLayout {
    layout_paragraph(&mut Stub, &[StyledRun::new(text, st(10.0))], opts)
}

fn line_texts(p: &ParagraphLayout) -> Vec<&str> {
    p.lines.iter().map(|l| &p.text[l.range.clone()]).collect()
}

fn cluster_x(p: &ParagraphLayout, line: usize, idx: usize) -> f64 {
    p.lines[line].clusters[idx].x
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn layout_wraps_at_spaces() {
    let p = lay("aaaa bbbb cccc", &ParaOptions::new(50.0));
    assert_eq!(line_texts(&p), vec!["aaaa bbbb ", "cccc"]);
    assert!(close(p.lines[0].width, 42.5), "{}", p.lines[0].width);
    assert!(close(p.lines[1].width, 20.0));
    assert!(close(p.lines[0].height, 10.0));
    assert!(close(p.lines[0].y_baseline, 8.0));
    assert!(close(p.lines[1].y_baseline, 18.0));
    assert!(close(p.height, 20.0));
    // Exactly fitting: the space after "bbbb" hangs, it does not force a wrap.
    let p = lay("aaaa bbbbb cc", &ParaOptions::new(47.5));
    assert_eq!(line_texts(&p), vec!["aaaa bbbbb ", "cc"]);
    // Many spaces still hang.
    let p = lay("aaaa          bb", &ParaOptions::new(25.0));
    assert_eq!(line_texts(&p), vec!["aaaa          ", "bb"]);
}

#[test]
fn layout_breaks_long_words_by_grapheme() {
    let p = lay(&"a".repeat(20), &ParaOptions::new(50.0));
    assert_eq!(line_texts(&p), vec!["aaaaaaaaaa", "aaaaaaaaaa"]);
    // A long word after a short one moves down first, then splits.
    let p = lay("ab cccccccccccc", &ParaOptions::new(50.0));
    assert_eq!(line_texts(&p), vec!["ab ", "cccccccccc", "cc"]);
    // Combining marks are never split from their base.
    let p = lay("e\u{301}e\u{301}e\u{301}", &ParaOptions::new(10.0));
    assert_eq!(line_texts(&p), vec!["e\u{301}e\u{301}", "e\u{301}"]);
    // A narrower-than-one-cluster width still makes progress.
    let p = lay("abc", &ParaOptions::new(1.0));
    assert_eq!(line_texts(&p), vec!["a", "b", "c"]);
}

#[test]
fn layout_mandatory_breaks_and_empty() {
    let p = lay("ab\ncd", &ParaOptions::new(100.0));
    assert_eq!(line_texts(&p), vec!["ab\n", "cd"]);
    assert!(p.lines[0].ends_with_break);
    let p = lay("ab\n", &ParaOptions::new(100.0));
    assert_eq!(p.lines.len(), 2);
    assert_eq!(p.lines[1].range, 3..3);
    let p = lay("", &ParaOptions::new(100.0));
    assert_eq!(p.lines.len(), 1);
    assert!(close(p.lines[0].height, 10.0));
    let p = layout_paragraph(&mut Stub, &[], &ParaOptions::new(100.0));
    assert_eq!(p.lines.len(), 1);
}

#[test]
fn layout_alignment() {
    let mut o = ParaOptions::new(50.0);
    o.align = Align::Center;
    let p = lay("aaaa bbbb cccc", &o);
    assert!(close(p.lines[0].x, 3.75));
    assert!(close(p.lines[1].x, 15.0));
    o.align = Align::Right;
    let p = lay("aaaa bbbb cccc", &o);
    assert!(close(p.lines[0].x, 7.5));
    assert!(close(p.lines[1].x, 30.0));
    assert!(close(cluster_x(&p, 1, 0), 30.0));
    // Justify: the one interior space of line 1 takes all 7.5 pt; the last line stays left.
    o.align = Align::Justify;
    let p = lay("aaaa bbbb cccc", &o);
    assert!(close(p.lines[0].width, 50.0));
    assert!(close(cluster_x(&p, 0, 5), 30.0), "{}", cluster_x(&p, 0, 5));
    assert!(close(p.lines[1].x, 0.0));
    assert!(close(p.lines[1].width, 20.0));
    // Distribute: every gap of every line, the last included.
    o.align = Align::Distribute;
    let p = lay("aaaa bbbb cccc", &o);
    let xs: Vec<f64> = (0..4).map(|i| cluster_x(&p, 1, i)).collect();
    assert!(close(xs[0], 0.0) && close(xs[1], 15.0) && close(xs[2], 30.0) && close(xs[3], 45.0), "{xs:?}");
    // Justify spreads between CJK characters when a line has no spaces.
    o.align = Align::Justify;
    let p = lay("日本語日本語", &ParaOptions { width_pt: 35.0, ..o.clone() });
    assert_eq!(line_texts(&p), vec!["日本語", "日本語"]);
    assert!(close(cluster_x(&p, 0, 1), 12.5) && close(cluster_x(&p, 0, 2), 25.0));
    assert!(close(cluster_x(&p, 1, 1), 10.0));
}

#[test]
fn layout_cjk_wrapping_and_kinsoku() {
    let p = lay("日本語日本語", &ParaOptions::new(35.0));
    assert_eq!(line_texts(&p), vec!["日本語", "日本語"]);
    // 。 cannot start a line: 語 goes down with it.
    let p = lay("日本語。日本", &ParaOptions::new(30.0));
    assert_eq!(line_texts(&p), vec!["日本", "語。日", "本"]);
    for l in &p.lines {
        assert!(!p.text[l.range.clone()].starts_with('。'));
    }
    // 「 cannot end a line.
    let p = lay("日本「語」", &ParaOptions::new(30.0));
    assert_eq!(line_texts(&p), vec!["日本", "「語」"]);
    // Small kana stay with the previous character.
    let p = lay("ちょっと", &ParaOptions::new(30.0));
    assert_eq!(line_texts(&p), vec!["ちょっ", "と"]);
    let p = lay("ちょっと", &ParaOptions::new(35.0));
    assert_eq!(line_texts(&p), vec!["ちょっ", "と"]);
}

#[test]
fn layout_indents() {
    let mut o = ParaOptions::new(60.0);
    o.left_indent = 5.0;
    o.first_line_indent = 10.0;
    o.right_indent = 5.0;
    let p = lay("aaaa bbbb cccc dd", &o);
    assert!(close(p.lines[0].x, 15.0));
    assert!(close(p.lines[1].x, 5.0));
    // First line has 40 pt: "aaaa bbbb" is 42.5, so it wraps after "aaaa".
    assert_eq!(line_texts(&p), vec!["aaaa ", "bbbb cccc ", "dd"]);
    // Hanging indent: a tab on the first line goes to the left indent.
    let mut o = ParaOptions::new(100.0);
    o.left_indent = 20.0;
    o.first_line_indent = -20.0;
    let p = lay("1.\tItem", &o);
    assert!(close(p.lines[0].x, 0.0));
    assert!(close(cluster_x(&p, 0, 3), 20.0));
}

#[test]
fn layout_tabs() {
    let mut o = ParaOptions::new(200.0);
    o.tab_stops = vec![TabStop::new(30.0, TabKind::Left)];
    let p = lay("a\tb", &o);
    assert!(close(cluster_x(&p, 0, 2), 30.0));
    // Right tab: "123" (15 pt) ends at 50.
    o.tab_stops = vec![TabStop::new(50.0, TabKind::Right)];
    let p = lay("a\t123", &o);
    assert!(close(cluster_x(&p, 0, 2), 35.0));
    // Decimal tab: the '.' sits at 40.
    o.tab_stops = vec![TabStop::new(40.0, TabKind::Decimal)];
    let p = lay("a\t12.50", &o);
    assert!(close(cluster_x(&p, 0, 2), 30.0));
    assert!(close(cluster_x(&p, 0, 4), 40.0));
    // Decimal with no separator: right-aligned.
    let p = lay("a\t12", &o);
    assert!(close(cluster_x(&p, 0, 3) + 5.0, 40.0));
    // Center tab: "abcd" (20 pt) centred on 40.
    o.tab_stops = vec![TabStop::new(40.0, TabKind::Center)];
    let p = lay("x\tabcd", &o);
    assert!(close(cluster_x(&p, 0, 2), 30.0));
    // Default grid of 36 pt after the explicit stops.
    o.tab_stops.clear();
    let p = lay("a\tb", &o);
    assert!(close(cluster_x(&p, 0, 2), 36.0));
    let p = lay("aaaaaaaa\tb", &o);
    assert!(close(cluster_x(&p, 0, 9), 72.0));
    o.tab_stops = vec![TabStop::new(10.0, TabKind::Left)];
    let p = lay("a\tb\tc", &o);
    assert!(close(cluster_x(&p, 0, 2), 10.0));
    assert!(close(cluster_x(&p, 0, 4), 36.0));
    // Leaders draw repeated glyphs inside the tab.
    o.tab_stops = vec![TabStop { pos: 60.0, kind: TabKind::Right, leader: TabLeader::Dots }];
    let p = lay("Intro\t7", &o);
    let tab_span = p.lines[0].spans.iter().find(|s| s.range == (5..6)).unwrap();
    assert!(!tab_span.glyphs.is_empty());
    assert!(tab_span.glyphs.iter().all(|g| g.gid == '.' as u16 && g.x >= 25.0 - 1e-9 && g.x + 5.0 <= 55.0 + 1e-9));
}

#[test]
fn layout_line_spacing() {
    let text = "aaaa bbbb cccc";
    let mut o = ParaOptions::new(50.0);
    o.space_before = 6.0;
    o.space_after = 4.0;
    let p = lay(text, &o);
    assert!(close(p.lines[0].top, 6.0) && close(p.lines[0].y_baseline, 14.0) && close(p.height, 30.0));
    o.line_spacing = LineSpacing::Multiple(1.5);
    let p = lay(text, &o);
    assert!(close(p.lines[0].height, 15.0) && close(p.lines[0].y_baseline, 6.0 + 15.0 - 2.0));
    assert!(close(p.height, 6.0 + 30.0 + 4.0));
    o.line_spacing = LineSpacing::Exactly(20.0);
    let p = lay(text, &o);
    assert!(close(p.lines[1].top, 26.0) && close(p.lines[1].y_baseline, 44.0));
    o.line_spacing = LineSpacing::AtLeast(5.0);
    assert!(close(lay(text, &o).lines[0].height, 10.0));
    o.line_spacing = LineSpacing::AtLeast(14.0);
    assert!(close(lay(text, &o).lines[0].height, 14.0));
    // Mixed sizes: the tallest run sets the line.
    let runs = [StyledRun::new("small ", st(10.0)), StyledRun::new("BIG", st(20.0))];
    let p = layout_paragraph(&mut Stub, &runs, &ParaOptions::new(200.0));
    assert!(close(p.lines[0].ascent, 16.0) && close(p.lines[0].descent, 4.0) && close(p.lines[0].height, 20.0));
}

#[test]
fn layout_letter_spacing_and_soft_hyphen() {
    let mut s = st(10.0);
    s.letter_spacing = 1.0;
    let p = layout_paragraph(&mut Stub, &[StyledRun::new("abc", s)], &ParaOptions::new(100.0));
    assert!(close(p.lines[0].width, 18.0));
    // A soft hyphen is invisible mid-line and shows as '-' where the line wraps.
    let p = lay("ab\u{AD}cd", &ParaOptions::new(100.0));
    assert!(close(p.lines[0].width, 20.0));
    let p = lay("aaa\u{AD}bbb", &ParaOptions::new(20.0));
    assert_eq!(line_texts(&p), vec!["aaa\u{AD}", "bbb"]);
    assert!(close(p.lines[0].width, 20.0));
    let last = p.lines[0].spans.last().unwrap();
    assert_eq!(last.glyphs.last().unwrap().gid, '-' as u16);
}

#[test]
fn layout_hit_test_and_caret() {
    let p = lay("aaaa bbbb cccc", &ParaOptions::new(50.0));
    // The second 'b' (offset 6) spans 27.5..32.5.
    let r = p.caret_rect(6);
    assert!(close(r.x, 27.5) && close(r.y, 0.0) && close(r.h, 10.0) && close(r.w, 0.0), "{r:?}");
    assert!(close(p.caret_rect(7).x, 32.5));
    assert_eq!(p.hit_test(27.6, 5.0), 6);
    assert_eq!(p.hit_test(29.9, 5.0), 6);
    assert_eq!(p.hit_test(30.1, 5.0), 7);
    // Second line, past its end: the end of the text.
    assert_eq!(p.hit_test(100.0, 15.0), 14);
    assert!(close(p.caret_rect(14).x, 20.0) && close(p.caret_rect(14).y, 10.0));
    // Left of a line: its start. Below the paragraph: the last line.
    assert_eq!(p.hit_test(-5.0, 15.0), 10);
    assert_eq!(p.hit_test(0.0, 500.0), 10);
    assert!(close(p.caret_rect(10).x, 0.0));
    // A line ending in a break: clicking past it lands before the break.
    let p = lay("ab\ncd", &ParaOptions::new(100.0));
    assert_eq!(p.hit_test(90.0, 5.0), 2);
    assert!(close(p.caret_rect(2).x, 10.0) && close(p.caret_rect(2).y, 0.0));
    assert!(close(p.caret_rect(3).y, 10.0));
    // Free functions give the same.
    assert_eq!(hit_test(&p, 90.0, 5.0), 2);
    assert_eq!(caret_rect(&p, 3), p.caret_rect(3));
    // Offsets inside a cluster snap to its start.
    let p = lay("e\u{301}x", &ParaOptions::new(100.0));
    assert!(close(p.caret_rect(2).x, 0.0));
    assert!(close(p.caret_rect(3).x, 5.0));
}

#[test]
fn layout_to_items() {
    let mut s1 = st(10.0);
    s1.underline = true;
    let mut s2 = st(10.0);
    s2.strike = true;
    s2.color = crate::ground::display::Color::rgb(255, 0, 0);
    let runs = [StyledRun::new("ab ", s1), StyledRun::new("cd", s2)];
    let p = layout_paragraph(&mut Stub, &runs, &ParaOptions::new(100.0));
    let items = p.to_items(100.0, 200.0);
    let glyph_items: Vec<&Item> = items.iter().filter(|i| matches!(i, Item::Glyphs { .. })).collect();
    assert_eq!(glyph_items.len(), 2);
    match glyph_items[0] {
        Item::Glyphs { font, size, glyphs, .. } => {
            assert_eq!(*font, 1);
            assert!(close(*size, 10.0));
            assert_eq!(glyphs[0], PlacedGlyph { gid: 'a' as u16, x: 100.0, y: 208.0 });
            assert_eq!(glyphs.len(), 3);
        }
        _ => unreachable!(),
    }
    match glyph_items[1] {
        Item::Glyphs { color, glyphs, .. } => {
            assert_eq!(color.r, 255);
            assert!(close(glyphs[0].x, 112.5));
        }
        _ => unreachable!(),
    }
    let paths: Vec<&Item> = items.iter().filter(|i| matches!(i, Item::Path { .. })).collect();
    assert_eq!(paths.len(), 2);
    // The underline covers "ab " (12.5 pt, the space is interior here) just below the baseline.
    let b = paths[0].bounds().unwrap();
    assert!(close(b.x, 100.0) && close(b.w, 12.5) && close(b.y, 209.0), "{b:?}");
    let b = paths[1].bounds().unwrap();
    assert!(close(b.x, 112.5) && close(b.w, 10.0) && b.y < 208.0);
    // Trailing spaces at a wrap are not underlined.
    let mut su = st(10.0);
    su.underline = true;
    let p = layout_paragraph(&mut Stub, &[StyledRun::new("aaaa bbbb", su)], &ParaOptions::new(25.0));
    let under: Vec<_> = p.to_items(0.0, 0.0).into_iter().filter(|i| matches!(i, Item::Path { .. })).collect();
    assert_eq!(under.len(), 2);
    assert!(close(under[0].bounds().unwrap().w, 20.0));
}

#[test]
fn layout_runs_keep_offsets() {
    let runs = [StyledRun::new("Hello ", st(10.0)), StyledRun::new("wor", st(10.0).bold()), StyledRun::new("ld", st(10.0))];
    let p = layout_paragraph(&mut Stub, &runs, &ParaOptions::new(1000.0));
    assert_eq!(p.text, "Hello world");
    let spans = &p.lines[0].spans;
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[1].range, 6..9);
    assert_eq!(spans[1].run, 1);
    assert!(close(spans[2].x_start, 42.5));
    // "wor" + "ld" is one word: no break between the runs.
    let p = layout_paragraph(&mut Stub, &runs, &ParaOptions::new(40.0));
    assert_eq!(line_texts(&p), vec!["Hello ", "world"]);
}

// ---------------------------------------------------------------------------------------------------
// Real fonts (when this machine has them)
// ---------------------------------------------------------------------------------------------------

fn win_font_exists(name: &str) -> bool {
    let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
    dir.join("Fonts").join(name).exists()
}

fn system_index() -> Arc<FontIndex> {
    static IDX: OnceLock<Arc<FontIndex>> = OnceLock::new();
    IDX.get_or_init(|| Arc::new(FontIndex::system())).clone()
}

#[test]
fn real_arial_measured() {
    if !win_font_exists("arial.ttf") {
        return;
    }
    let mut fc = FontCache::new(system_index());
    let style = TextStyle::new("Arial", 12.0);
    let id = fc.resolve(&style).expect("Arial resolves");
    let font = fc.font(id).unwrap().clone();
    assert_eq!(font.names().family, "Arial");
    let upem = font.units_per_em() as f64;
    // "Hello" by hand: advances plus kerning.
    let gids: Vec<u16> = "Hello".chars().map(|c| font.glyph_id(c).unwrap()).collect();
    let mut want = 0.0;
    for (i, g) in gids.iter().enumerate() {
        want += font.advance(*g) as f64;
        if i + 1 < gids.len() {
            want += font.kerning(*g, gids[i + 1]) as f64;
        }
    }
    want *= 12.0 / upem;
    let runs = fc.shape("Hello", &style);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].font, id);
    assert!(runs[0].font_ref.is_some());
    assert!((runs[0].width - want).abs() < 1e-9, "{} vs {want}", runs[0].width);
    assert!((fc.measure("Hello", &style) - want).abs() < 1e-9);
    assert!(want > 20.0 && want < 35.0, "{want}");
    // Kerning pulls "AV" together.
    let a = font.glyph_id('A').unwrap();
    let v = font.glyph_id('V').unwrap();
    let apart = (font.advance(a) + font.advance(v)) as f64 * 12.0 / upem;
    let kerned = fc.measure("AV", &style);
    assert!(kerned <= apart + 1e-9);
    // Metrics: Arial's line is ~1.15 em.
    let m = fc.font_metrics(&style);
    assert!((m.line_height() / 12.0 - 1.15).abs() < 0.03, "{}", m.line_height());
    // Bold resolves to a bold face and is wider.
    let bold = style.clone().bold();
    assert!(fc.measure("Hello", &bold) > want);
    // A paragraph to display items and resources with outlines.
    let p = layout_paragraph(&mut fc, &[StyledRun::new("Hello world, this wraps.", style.clone())], &ParaOptions::new(80.0));
    assert!(p.lines.len() >= 2);
    let items = p.to_items(10.0, 10.0);
    let mut res = Resources::new();
    fc.fill_resources(&mut res, &items);
    let fr = res.font(id).unwrap();
    assert_eq!(fr.units_per_em, upem);
    let h = font.glyph_id('H').unwrap();
    assert!(!fr.glyphs[&h].is_empty());
    let sp = font.glyph_id(' ').unwrap();
    assert!(fr.glyphs[&sp].is_empty());
}

#[test]
fn real_segoe_ui_measured() {
    if !win_font_exists("segoeui.ttf") {
        return;
    }
    let mut fc = FontCache::new(system_index());
    let style = TextStyle::new("Segoe UI", 11.0);
    let runs = fc.shape("Scriptorium", &style);
    assert_eq!(runs.len(), 1);
    assert!(fc.font(runs[0].font).unwrap().names().preferred_family().starts_with("Segoe UI"));
    assert!(runs[0].width > 40.0 && runs[0].width < 80.0, "{}", runs[0].width);
    // Combining acute stays in the same run and adds a glyph without widening much.
    let plain = fc.measure("e", &style);
    let comb = fc.shape("e\u{301}", &style);
    assert_eq!(comb.len(), 1);
    assert_eq!(comb[0].glyphs.len(), 2);
    assert!((comb[0].width - plain).abs() < 1e-9, "{} vs {plain}", comb[0].width);
}

#[test]
fn real_japanese_falls_back() {
    if !win_font_exists("arial.ttf") || !(win_font_exists("YuGothR.ttc") || win_font_exists("YuGothM.ttc") || win_font_exists("msgothic.ttc")) {
        return;
    }
    let mut fc = FontCache::new(system_index());
    let style = TextStyle::new("Arial", 12.0);
    let arial = fc.resolve(&style).unwrap();
    let runs = fc.shape("Hello 日本語のテキスト。", &style);
    assert!(runs.len() >= 2, "{runs:?}");
    assert_eq!(runs[0].font, arial);
    let jp = runs.last().unwrap();
    assert_ne!(jp.font, arial);
    let fam = fc.font(jp.font).unwrap().names().preferred_family().to_string();
    assert!(fam.starts_with("Yu Gothic") || fam.starts_with("MS Gothic") || fam.contains("ゴシック"), "{fam}");
    // Every Japanese character landed in that one font (no tofu, no mid-word switching).
    assert!(jp.glyphs.iter().all(|g| g.gid != 0));
    assert_eq!(jp.glyphs.len(), "日本語のテキスト。".chars().count());
    // Ideographs are about 1 em wide.
    let w = fc.measure("日本語", &style);
    assert!((w / 36.0 - 1.0).abs() < 0.1, "{w}");
    // In a paragraph the Japanese font's taller metrics lift the line, and it wraps by kinsoku.
    let p = layout_paragraph(&mut fc, &[StyledRun::new("これは日本語の文章です。改行のテストをします。", style.clone())], &ParaOptions::new(100.0));
    assert!(p.lines.len() >= 3);
    for l in &p.lines {
        let t = &p.text[l.range.clone()];
        assert!(!t.starts_with('。') && !t.starts_with('す') || !t.is_empty());
        assert!(!t.starts_with('。'), "{t}");
        assert!(l.width <= 100.0 + 1e-6, "{t} {}", l.width);
    }
    let items = p.to_items(0.0, 0.0);
    let mut res = Resources::new();
    fc.fill_resources(&mut res, &items);
    let fr = res.font(jp.font).unwrap();
    assert!(fr.glyphs.values().any(|o| !o.is_empty()));
}

#[test]
fn empty_cache_still_lays_out() {
    let mut fc = FontCache::empty();
    let style = TextStyle::new("Nothing", 10.0);
    let runs = fc.shape("ab", &style);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].font, 0);
    assert!(close(runs[0].width, 10.0));
    let p = layout_paragraph(&mut fc, &[StyledRun::new("ab cd", style)], &ParaOptions::new(12.0));
    assert_eq!(p.lines.len(), 2);
    // No font: no glyph items, nothing to fill.
    assert!(p.to_items(0.0, 0.0).iter().all(|i| !matches!(i, Item::Glyphs { .. })));
}
