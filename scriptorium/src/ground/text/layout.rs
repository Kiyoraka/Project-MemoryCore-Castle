//! Paragraph layout: styled runs in, lines of placed glyphs out.
//!
//! The paragraph's text is cut into grapheme clusters; each run's text between tabs and line
//! breaks is shaped by the [`Measurer`]. Lines are filled greedily over the UAX #14 break
//! opportunities: a cluster that would cross the right edge ends the line at the last opportunity,
//! or - when the line has none (one long word, a URL) - between grapheme clusters. Spaces at the
//! end of a line hang past the edge and never cause a wrap. Tabs advance to the next stop (explicit
//! stops, the hanging-indent stop, then the default grid), right/center/decimal stops measuring the
//! text after the tab. Alignment then shifts or stretches each line.
//!
//! Coordinates: points, y down, origin at the paragraph's top-left (before `left_indent` and
//! `space_before`).
//!
//! Line height follows Word: "single" is the tallest font's ascent + descent + line gap, extra
//! leading goes above the text (the baseline sits `descent` above the line's bottom).

use super::segment::{graphemes, line_break_opportunities_with, BreakKind, BreakOptions};
use super::tables::{is_cjk, line_break, LineBreak};
use super::{FontMetrics, Measurer, ShapedRun, StyledRun, TextStyle};
use crate::ground::display::{Color, Item, Paint, PathOp, PlacedGlyph, Rect};
use std::ops::Range;

const EPS: f64 = 1e-6;

/// Horizontal alignment of a paragraph's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    /// Extra space spread over the spaces of each line but the last (between CJK characters when a
    /// line has no spaces).
    Justify,
    /// Extra space spread between every pair of grapheme clusters, last line included.
    Distribute,
}

/// Line spacing (Word's options).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineSpacing {
    /// The tallest font's ascent + descent + line gap.
    Single,
    /// Single times a factor (1.5, 2.0...).
    Multiple(f64),
    /// Exactly this many points, whatever the fonts.
    Exactly(f64),
    /// Single, but at least this many points.
    AtLeast(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    /// Text after the tab starts at the stop.
    Left,
    /// Text after the tab (up to the next tab or line end) is centred on the stop.
    Center,
    /// Text after the tab ends at the stop.
    Right,
    /// The decimal separator of the text after the tab sits at the stop (right-aligned when the text
    /// has none).
    Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabLeader {
    None,
    /// "......"
    Dots,
    /// "------"
    Hyphens,
    /// A line along the baseline (drawn as the tab's underline).
    Underline,
    /// "······"
    MiddleDots,
}

/// A tab stop, measured from the paragraph's left edge (x = 0, before the left indent).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStop {
    pub pos: f64,
    pub kind: TabKind,
    pub leader: TabLeader,
}

impl TabStop {
    pub fn new(pos: f64, kind: TabKind) -> TabStop {
        TabStop { pos, kind, leader: TabLeader::None }
    }
}

/// Paragraph formatting.
#[derive(Debug, Clone, PartialEq)]
pub struct ParaOptions {
    /// The width lines are filled to (the column or cell width).
    pub width_pt: f64,
    pub align: Align,
    /// Added to `left_indent` on the first line (negative for a hanging indent).
    pub first_line_indent: f64,
    pub left_indent: f64,
    pub right_indent: f64,
    pub line_spacing: LineSpacing,
    pub space_before: f64,
    pub space_after: f64,
    pub tab_stops: Vec<TabStop>,
    /// The default tab grid after the last explicit stop (Word: 36 pt).
    pub default_tab_width: f64,
    /// The character decimal tabs align on.
    pub decimal_char: char,
    /// Line breaking tailoring (East Asian context, kinsoku sets).
    pub breaks: BreakOptions,
}

impl ParaOptions {
    pub fn new(width_pt: f64) -> ParaOptions {
        ParaOptions {
            width_pt,
            align: Align::Left,
            first_line_indent: 0.0,
            left_indent: 0.0,
            right_indent: 0.0,
            line_spacing: LineSpacing::Single,
            space_before: 0.0,
            space_after: 0.0,
            tab_stops: Vec::new(),
            default_tab_width: 36.0,
            decimal_char: '.',
            breaks: BreakOptions::default(),
        }
    }
}

impl Default for ParaOptions {
    fn default() -> Self {
        ParaOptions::new(468.0)
    }
}

/// Consecutive glyphs of one font, size and run on one line.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphSpan {
    /// Font id ([`super::FontCache`]); 0 when the span has no font (a tab without a leader).
    pub font: u32,
    pub size: f64,
    pub color: Color,
    /// Glyph origins: x in paragraph coordinates, y relative to the line's baseline (0).
    pub glyphs: Vec<PlacedGlyph>,
    /// The span's horizontal extent (paragraph coordinates).
    pub x_start: f64,
    pub x_end: f64,
    pub underline: bool,
    pub strike: bool,
    /// Metrics of the span's font, for decorations.
    pub metrics: FontMetrics,
    /// Index of the [`StyledRun`] the span comes from.
    pub run: usize,
    /// Byte range of the paragraph text.
    pub range: Range<usize>,
}

/// One grapheme cluster as placed on a line (for hit testing and carets).
#[derive(Debug, Clone, PartialEq)]
pub struct LineCluster {
    pub range: Range<usize>,
    pub x: f64,
    pub width: f64,
    /// A line break character (zero width, not drawn).
    pub is_break: bool,
}

/// A laid-out line.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Byte range of the paragraph text, trailing spaces and the line break character included.
    pub range: Range<usize>,
    /// Top of the line box.
    pub top: f64,
    pub y_baseline: f64,
    pub height: f64,
    pub ascent: f64,
    pub descent: f64,
    /// Left edge of the line's content after alignment.
    pub x: f64,
    /// Content width, trailing spaces excluded (justification stretch included).
    pub width: f64,
    /// True when the line ends with a line break character.
    pub ends_with_break: bool,
    pub spans: Vec<GlyphSpan>,
    pub clusters: Vec<LineCluster>,
}

/// A laid-out paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphLayout {
    /// The paragraph's text (the runs concatenated); line and cluster ranges index it.
    pub text: String,
    pub lines: Vec<Line>,
    /// Total height, space before and after included.
    pub height: f64,
    /// The width it was laid out to.
    pub width: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Space,
    Tab,
    Break,
    SoftHyphen,
}

#[derive(Debug, Clone)]
struct Cluster {
    range: Range<usize>,
    run: usize,
    /// (piece, glyph index)
    glyphs: Vec<(usize, usize)>,
    width: f64,
    kind: Kind,
    cjk: bool,
}

struct Fill {
    end: usize,
    next: usize,
    xs: Vec<f64>,
    ws: Vec<f64>,
    leaders: Vec<TabLeader>,
    forced: bool,
}

struct Builder<'a> {
    runs: &'a [StyledRun],
    opts: &'a ParaOptions,
    text: String,
    cls: Vec<Cluster>,
    pieces: Vec<ShapedRun>,
    /// Break allowed before cluster i (len = clusters + 1).
    brk: Vec<Option<BreakKind>>,
    run_metrics: Vec<FontMetrics>,
    default_metrics: FontMetrics,
    stops: Vec<TabStop>,
}

fn classify(c: char) -> Kind {
    match c {
        '\t' => Kind::Tab,
        ' ' => Kind::Space,
        '\u{AD}' => Kind::SoftHyphen,
        _ => match line_break(c) {
            LineBreak::BK | LineBreak::CR | LineBreak::LF | LineBreak::NL => Kind::Break,
            _ => Kind::Text,
        },
    }
}

/// Lay out one paragraph.
pub fn layout_paragraph<M: Measurer + ?Sized>(m: &mut M, runs: &[StyledRun], opts: &ParaOptions) -> ParagraphLayout {
    let mut b = Builder::new(m, runs, opts);
    b.run(m)
}

impl<'a> Builder<'a> {
    fn new<M: Measurer + ?Sized>(m: &mut M, runs: &'a [StyledRun], opts: &'a ParaOptions) -> Builder<'a> {
        let mut text = String::new();
        let mut cls: Vec<Cluster> = Vec::new();
        let mut pieces: Vec<ShapedRun> = Vec::new();
        for (ri, r) in runs.iter().enumerate() {
            let base = text.len();
            text.push_str(&r.text);
            let gs: Vec<Range<usize>> = graphemes(&r.text).collect();
            let kind_of = |g: &Range<usize>| classify(r.text[g.clone()].chars().next().unwrap_or(' '));
            let mut k = 0;
            while k < gs.len() {
                let kind = kind_of(&gs[k]);
                if matches!(kind, Kind::Tab | Kind::Break) {
                    cls.push(Cluster {
                        range: base + gs[k].start..base + gs[k].end,
                        run: ri,
                        glyphs: Vec::new(),
                        width: 0.0,
                        kind,
                        cjk: false,
                    });
                    k += 1;
                    continue;
                }
                let start = k;
                while k < gs.len() && !matches!(kind_of(&gs[k]), Kind::Tab | Kind::Break) {
                    k += 1;
                }
                let seg = gs[start].start..gs[k - 1].end;
                let first = cls.len();
                for g in &gs[start..k] {
                    let c0 = r.text[g.clone()].chars().next().unwrap_or(' ');
                    cls.push(Cluster {
                        range: base + g.start..base + g.end,
                        run: ri,
                        glyphs: Vec::new(),
                        width: 0.0,
                        kind: kind_of(g),
                        cjk: is_cjk(c0),
                    });
                }
                let seg_base = base + seg.start;
                for sr in m.shape(&r.text[seg.clone()], &r.style) {
                    let pi = pieces.len();
                    for (gi, sg) in sr.glyphs.iter().enumerate() {
                        let off = seg_base + sg.cluster.start;
                        let rel = cls[first..].partition_point(|c| c.range.end <= off);
                        let ci = (first + rel).min(cls.len() - 1);
                        cls[ci].glyphs.push((pi, gi));
                        cls[ci].width += sg.advance;
                    }
                    pieces.push(sr);
                }
            }
        }
        let n = cls.len();
        let mut brk = vec![None; n + 1];
        for (off, kind) in line_break_opportunities_with(&text, &opts.breaks) {
            let i = cls.partition_point(|c| c.range.start < off);
            if i <= n && (i == n || cls[i].range.start == off) {
                brk[i] = Some(kind);
            }
        }
        let run_metrics: Vec<FontMetrics> = runs.iter().map(|r| m.font_metrics(&r.style)).collect();
        let default_metrics = run_metrics.last().copied().unwrap_or_else(|| m.font_metrics(&TextStyle::default()));
        let mut stops: Vec<TabStop> = opts.tab_stops.clone();
        stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap_or(std::cmp::Ordering::Equal));
        Builder { runs, opts, text, cls, pieces, brk, run_metrics, default_metrics, stops }
    }

    fn x0(&self, first: bool) -> f64 {
        self.opts.left_indent + if first { self.opts.first_line_indent } else { 0.0 }
    }

    fn right(&self) -> f64 {
        self.opts.width_pt - self.opts.right_indent
    }

    fn next_tab(&self, x: f64, first: bool) -> (f64, TabKind, TabLeader) {
        let o = self.opts;
        let mut best = self.stops.iter().find(|s| s.pos > x + 0.01).map(|s| (s.pos, s.kind, s.leader));
        // A hanging indent acts as a tab stop on the first line (Word).
        if first && o.first_line_indent < 0.0 && o.left_indent > x + 0.01 && best.map_or(true, |b| o.left_indent < b.0) {
            best = Some((o.left_indent, TabKind::Left, TabLeader::None));
        }
        if let Some(b) = best {
            return b;
        }
        let d = if o.default_tab_width > 0.01 { o.default_tab_width } else { 36.0 };
        ((((x + 0.01) / d).floor() + 1.0) * d, TabKind::Left, TabLeader::None)
    }

    /// Width of the text after tab cluster `i` up to the next tab or break (up to the decimal
    /// separator when `decimal`).
    fn segment_width(&self, i: usize, decimal: bool) -> f64 {
        let mut w = 0.0;
        for c in &self.cls[i + 1..] {
            if matches!(c.kind, Kind::Tab | Kind::Break) {
                break;
            }
            if decimal && self.text[c.range.clone()].starts_with(self.opts.decimal_char) {
                break;
            }
            w += c.width;
        }
        w
    }

    fn fill_line(&self, start: usize, first: bool) -> Fill {
        let n = self.cls.len();
        let x0 = self.x0(first);
        let right = self.right();
        let mut f = Fill { end: n, next: n, xs: Vec::new(), ws: Vec::new(), leaders: Vec::new(), forced: false };
        let mut x = x0;
        let mut last_break: Option<usize> = None;
        let mut content = false;
        let mut i = start;
        while i < n {
            let c = &self.cls[i];
            match c.kind {
                Kind::Break => {
                    f.xs.push(x);
                    f.ws.push(0.0);
                    f.leaders.push(TabLeader::None);
                    f.end = i + 1;
                    f.next = i + 1;
                    f.forced = true;
                    return f;
                }
                Kind::Tab => {
                    let (pos, kind, leader) = self.next_tab(x, first);
                    let target = pos.min(right.max(x));
                    let portion = match kind {
                        TabKind::Left => 0.0,
                        TabKind::Right => self.segment_width(i, false),
                        TabKind::Center => self.segment_width(i, false) / 2.0,
                        TabKind::Decimal => self.segment_width(i, true),
                    };
                    let w = (target - portion - x).max(0.0);
                    f.xs.push(x);
                    f.ws.push(w);
                    f.leaders.push(leader);
                    x += w;
                    content = true;
                    i += 1;
                    if self.brk[i].is_some() {
                        last_break = Some(i);
                    }
                }
                Kind::Space => {
                    f.xs.push(x);
                    f.ws.push(c.width);
                    f.leaders.push(TabLeader::None);
                    x += c.width;
                    i += 1;
                    if content && self.brk[i].is_some() {
                        last_break = Some(i);
                    }
                }
                Kind::Text | Kind::SoftHyphen => {
                    if content && x + c.width > right + EPS {
                        let b = last_break.unwrap_or(i);
                        f.xs.truncate(b - start);
                        f.ws.truncate(b - start);
                        f.leaders.truncate(b - start);
                        f.end = b;
                        f.next = b;
                        return f;
                    }
                    f.xs.push(x);
                    f.ws.push(c.width);
                    f.leaders.push(TabLeader::None);
                    x += c.width;
                    content = true;
                    i += 1;
                    if i < n && self.brk[i] == Some(BreakKind::Allowed) {
                        last_break = Some(i);
                    }
                }
            }
        }
        f
    }

    fn cluster_metrics(&self, j: usize) -> FontMetrics {
        let c = &self.cls[j];
        match c.glyphs.first() {
            Some(&(pi, _)) => self.pieces[pi].metrics,
            None => self.run_metrics.get(c.run).copied().unwrap_or(self.default_metrics),
        }
    }

    fn line_height(&self, m: &FontMetrics) -> f64 {
        let natural = m.ascent + m.descent + m.line_gap;
        match self.opts.line_spacing {
            LineSpacing::Single => natural,
            LineSpacing::Multiple(f) => natural * f.max(0.0),
            LineSpacing::Exactly(p) => p.max(0.0),
            LineSpacing::AtLeast(p) => natural.max(p),
        }
    }

    fn run<M: Measurer + ?Sized>(&mut self, m: &mut M) -> ParagraphLayout {
        let n = self.cls.len();
        let mut lines = Vec::new();
        let mut y = self.opts.space_before;
        let mut start = 0;
        let mut first = true;
        while start < n {
            let f = self.fill_line(start, first);
            let last = f.next >= n && !f.forced;
            let line = self.make_line(m, start, &f, first, last, &mut y);
            lines.push(line);
            start = f.next.max(start + 1);
            first = false;
        }
        if n == 0 || self.cls[n - 1].kind == Kind::Break {
            lines.push(self.empty_line(first, &mut y));
        }
        ParagraphLayout {
            text: std::mem::take(&mut self.text),
            lines,
            height: y + self.opts.space_after,
            width: self.opts.width_pt,
        }
    }

    fn empty_line(&self, first: bool, y: &mut f64) -> Line {
        let n = self.cls.len();
        let mt = if n > 0 { self.run_metrics.get(self.cls[n - 1].run).copied().unwrap_or(self.default_metrics) } else { self.default_metrics };
        let x0 = self.x0(first);
        let avail = self.right() - x0;
        let shift = match self.opts.align {
            Align::Center | Align::Distribute => (avail / 2.0).max(0.0),
            Align::Right => avail.max(0.0),
            _ => 0.0,
        };
        let height = self.line_height(&mt);
        let top = *y;
        *y += height;
        let at = self.text.len();
        Line {
            range: at..at,
            top,
            y_baseline: top + height - mt.descent,
            height,
            ascent: mt.ascent,
            descent: mt.descent,
            x: x0 + shift,
            width: 0.0,
            ends_with_break: false,
            spans: Vec::new(),
            clusters: Vec::new(),
        }
    }

    fn leader_glyphs<M: Measurer + ?Sized>(&self, m: &mut M, leader: TabLeader, style: &TextStyle, x: f64, w: f64) -> Option<(u32, FontMetrics, Vec<PlacedGlyph>)> {
        let ch = match leader {
            TabLeader::Dots => ".",
            TabLeader::Hyphens => "-",
            TabLeader::MiddleDots => "\u{B7}",
            TabLeader::None | TabLeader::Underline => return None,
        };
        let run = m.shape(ch, style).into_iter().next()?;
        let g = run.glyphs.first()?;
        if g.advance <= 0.01 {
            return None;
        }
        // Leave a little room on both sides, then align the leader to the stop.
        let room = w - g.advance * 0.5;
        let count = (room / g.advance).floor().max(0.0) as usize;
        let start = x + w - count as f64 * g.advance;
        let glyphs = (0..count).map(|k| PlacedGlyph { gid: g.gid, x: start + k as f64 * g.advance, y: 0.0 }).collect();
        Some((run.font, run.metrics, glyphs))
    }

    fn make_line<M: Measurer + ?Sized>(&mut self, m: &mut M, start: usize, f: &Fill, first: bool, last: bool, y: &mut f64) -> Line {
        let runs = self.runs;
        let end = f.end;
        let x0 = self.x0(first);
        let right = self.right();
        let mut xs = f.xs.clone();
        let mut ws = f.ws.clone();
        // Content end: trailing spaces and the break hang.
        let mut ce = end;
        while ce > start && matches!(self.cls[ce - 1].kind, Kind::Space | Kind::Break) {
            ce -= 1;
        }
        // A soft hyphen where the line wrapped shows as a hyphen.
        if ce > start && self.cls[ce - 1].kind == Kind::SoftHyphen && !last && !f.forced && ce == end {
            let style = &runs[self.cls[ce - 1].run].style;
            if let Some(mut r) = m.shape("-", style).into_iter().next() {
                if !r.glyphs.is_empty() {
                    r.glyphs.truncate(1);
                    let adv = r.glyphs[0].advance;
                    let pi = self.pieces.len();
                    self.pieces.push(r);
                    self.cls[ce - 1].glyphs = vec![(pi, 0)];
                    self.cls[ce - 1].width = adv;
                    ws[ce - 1 - start] = adv;
                }
            }
        }
        let content_w = if ce > start { xs[ce - 1 - start] + ws[ce - 1 - start] - x0 } else { 0.0 };
        let avail = right - x0;
        let extra = avail - content_w;
        let count = end - start;
        let mut shift = 0.0;
        let mut add = vec![0.0; count];
        // EXTENSION (bidi): visual reordering of the line's clusters (UAX #9 levels, rule L2) goes
        // here, before alignment assigns final positions.
        match self.opts.align {
            Align::Left => {}
            Align::Center => shift = (extra / 2.0).max(0.0),
            Align::Right => shift = extra.max(0.0),
            Align::Justify | Align::Distribute if extra > EPS => {
                let distribute = self.opts.align == Align::Distribute;
                if distribute || !last {
                    let after_tab = (start..ce).rev().find(|&j| self.cls[j].kind == Kind::Tab).map_or(start, |j| j + 1);
                    let spaces: Vec<usize> = (after_tab..ce).filter(|&j| self.cls[j].kind == Kind::Space).collect();
                    if !distribute && !spaces.is_empty() {
                        let per = extra / spaces.len() as f64;
                        for j in spaces {
                            add[j - start] = per;
                        }
                    } else if distribute || (after_tab..ce).any(|j| self.cls[j].cjk) {
                        if ce > after_tab + 1 {
                            let gaps = ce - 1 - after_tab;
                            let per = extra / gaps as f64;
                            for j in after_tab..ce - 1 {
                                add[j - start] = per;
                            }
                        } else if distribute {
                            shift = extra / 2.0;
                        }
                    }
                }
            }
            _ => {}
        }
        let mut acc = shift;
        for k in 0..count {
            xs[k] += acc;
            ws[k] += add[k];
            acc += add[k];
        }
        let stretch: f64 = add[..ce - start].iter().sum();
        // Vertical metrics.
        let mut mt: Option<FontMetrics> = None;
        for j in start..end {
            let c = self.cluster_metrics(j);
            mt = Some(match mt {
                None => c,
                Some(a) => FontMetrics {
                    ascent: a.ascent.max(c.ascent),
                    descent: a.descent.max(c.descent),
                    line_gap: a.line_gap.max(c.line_gap),
                    ..a
                },
            });
        }
        let mt = mt.unwrap_or(self.default_metrics);
        let height = self.line_height(&mt);
        let top = *y;
        *y += height;
        let baseline = top + height - mt.descent;
        // Spans and clusters.
        let mut spans: Vec<GlyphSpan> = Vec::new();
        let mut clusters = Vec::with_capacity(count);
        let mut last_was_tab = false;
        for j in start..end {
            let k = j - start;
            let (cx, cw) = (xs[k], ws[k]);
            let c = &self.cls[j];
            clusters.push(LineCluster { range: c.range.clone(), x: cx, width: cw, is_break: c.kind == Kind::Break });
            if c.kind == Kind::Break {
                continue;
            }
            let style = &runs[c.run].style;
            if c.kind == Kind::Tab {
                let leader = f.leaders.get(k).copied().unwrap_or(TabLeader::None);
                let (font, metrics, glyphs) = self
                    .leader_glyphs(m, leader, style, cx, cw)
                    .unwrap_or((0, self.run_metrics.get(c.run).copied().unwrap_or(self.default_metrics), Vec::new()));
                spans.push(GlyphSpan {
                    font,
                    size: style.size_pt,
                    color: style.color,
                    glyphs,
                    x_start: cx,
                    x_end: cx + cw,
                    underline: style.underline || leader == TabLeader::Underline,
                    strike: style.strike,
                    metrics,
                    run: c.run,
                    range: c.range.clone(),
                });
                last_was_tab = true;
                continue;
            }
            if c.glyphs.is_empty() {
                match spans.last_mut() {
                    Some(s) if !last_was_tab && s.run == c.run => {
                        s.x_end = cx + cw;
                        s.range.end = c.range.end;
                    }
                    _ => {
                        spans.push(GlyphSpan {
                            font: 0,
                            size: style.size_pt,
                            color: style.color,
                            glyphs: Vec::new(),
                            x_start: cx,
                            x_end: cx + cw,
                            underline: style.underline,
                            strike: style.strike,
                            metrics: self.run_metrics.get(c.run).copied().unwrap_or(self.default_metrics),
                            run: c.run,
                            range: c.range.clone(),
                        });
                        last_was_tab = false;
                    }
                }
                continue;
            }
            let mut gx = cx;
            for &(pi, gi) in &c.glyphs {
                let piece = &self.pieces[pi];
                let g = &piece.glyphs[gi];
                let new_span = match spans.last() {
                    Some(s) => last_was_tab || s.run != c.run || s.font != piece.font,
                    None => true,
                };
                if new_span {
                    spans.push(GlyphSpan {
                        font: piece.font,
                        size: piece.size,
                        color: style.color,
                        glyphs: Vec::new(),
                        x_start: gx,
                        x_end: gx,
                        underline: style.underline,
                        strike: style.strike,
                        metrics: piece.metrics,
                        run: c.run,
                        range: c.range.start..c.range.start,
                    });
                    last_was_tab = false;
                }
                let s = spans.last_mut().expect("span pushed");
                s.glyphs.push(PlacedGlyph { gid: g.gid, x: gx, y: 0.0 });
                gx += g.advance;
            }
            if let Some(s) = spans.last_mut() {
                s.x_end = cx + cw;
                s.range.end = c.range.end;
            }
        }
        let range_start = self.cls[start].range.start;
        let range_end = self.cls[end - 1].range.end;
        Line {
            range: range_start..range_end,
            top,
            y_baseline: baseline,
            height,
            ascent: mt.ascent,
            descent: mt.descent,
            x: x0 + shift,
            width: content_w + stretch,
            ends_with_break: f.forced,
            spans,
            clusters,
        }
    }
}

fn rect_path(x: f64, y: f64, w: f64, h: f64) -> Vec<PathOp> {
    vec![PathOp::MoveTo(x, y), PathOp::LineTo(x + w, y), PathOp::LineTo(x + w, y + h), PathOp::LineTo(x, y + h), PathOp::Close]
}

impl ParagraphLayout {
    /// Display items for the paragraph placed with its top-left at (`origin_x`, `origin_y`): one
    /// `Item::Glyphs` per span, then thin filled rectangles for underlines and strikethroughs.
    /// Trailing spaces are not underlined.
    pub fn to_items(&self, origin_x: f64, origin_y: f64) -> Vec<Item> {
        let mut items = Vec::new();
        for line in &self.lines {
            let base_y = origin_y + line.y_baseline;
            let content_end = line.x + line.width;
            for s in &line.spans {
                if s.font != 0 && !s.glyphs.is_empty() {
                    items.push(Item::Glyphs {
                        font: s.font,
                        size: s.size,
                        color: s.color,
                        glyphs: s.glyphs.iter().map(|g| PlacedGlyph { gid: g.gid, x: origin_x + g.x, y: base_y + g.y }).collect(),
                    });
                }
            }
            for s in &line.spans {
                let x1 = s.x_start;
                let x2 = s.x_end.min(content_end);
                if x2 <= x1 + EPS {
                    continue;
                }
                if s.underline {
                    let t = s.metrics.underline_thickness.max(0.25);
                    items.push(Item::Path {
                        ops: rect_path(origin_x + x1, base_y + s.metrics.underline_y, x2 - x1, t),
                        fill: Some(Paint::Solid(s.color)),
                        stroke: None,
                    });
                }
                if s.strike {
                    let t = s.metrics.strike_thickness.max(0.25);
                    items.push(Item::Path {
                        ops: rect_path(origin_x + x1, base_y + s.metrics.strike_y, x2 - x1, t),
                        fill: Some(Paint::Solid(s.color)),
                        stroke: None,
                    });
                }
            }
        }
        items
    }

    /// The text offset nearest to a point in paragraph coordinates (the caret position a click
    /// lands on).
    pub fn hit_test(&self, x: f64, y: f64) -> usize {
        let Some(line) = self.lines.iter().find(|l| y < l.top + l.height).or(self.lines.last()) else { return 0 };
        for c in &line.clusters {
            if c.is_break {
                return c.range.start;
            }
            if x < c.x + c.width / 2.0 {
                return c.range.start;
            }
        }
        line.range.end
    }

    /// The caret at a text offset: a zero-width rectangle at the caret's x, from the line's ascent
    /// to its descent. Offsets inside a grapheme cluster snap to its start.
    pub fn caret_rect(&self, offset: usize) -> Rect {
        let Some(last) = self.lines.last() else { return Rect::new(0.0, 0.0, 0.0, 0.0) };
        let line = self.lines.iter().find(|l| offset < l.range.end).unwrap_or(last);
        let mut x = line.x;
        for c in &line.clusters {
            if c.range.start >= offset || c.range.contains(&offset) {
                x = c.x;
                return Rect::new(x, line.y_baseline - line.ascent, 0.0, line.ascent + line.descent);
            }
            x = c.x + c.width;
        }
        Rect::new(x, line.y_baseline - line.ascent, 0.0, line.ascent + line.descent)
    }
}

/// See [`ParagraphLayout::hit_test`].
pub fn hit_test(layout: &ParagraphLayout, x: f64, y: f64) -> usize {
    layout.hit_test(x, y)
}

/// See [`ParagraphLayout::caret_rect`].
pub fn caret_rect(layout: &ParagraphLayout, offset: usize) -> Rect {
    layout.caret_rect(offset)
}
