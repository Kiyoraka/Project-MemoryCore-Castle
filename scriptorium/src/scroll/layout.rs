//! The page engine (first version): resolves every paragraph's effective formatting, lays its lines
//! with the Ground's text engine inside the column, and paginates: a line or a table row that does
//! not fit goes to the next page; explicit page breaks, `w:pageBreakBefore` and section breaks
//! start new pages. Each page gets its header and footer (first / even / default, inherited across
//! sections) with `PAGE` and `NUMPAGES` computed.
//!
//! Inline pictures ride inside the line as one placeholder character that the measurer gives the
//! picture's width and height, so lines grow around them like Word's do.
//!
//! Honest limits (later milestones): floating objects and text wrap, footnotes and endnotes,
//! multiple columns, widow/orphan control and keep-with-next, rows that split across pages,
//! table-style conditional formatting (first row, banding), paragraph borders, auto-fit column
//! widths (the grid is used as written), right-to-left text.

use super::model::{Block, Cell, FieldKind, Model, Para, Pic, RunItem, SectKind, SectPr, Table, VAlign, VMerge};
use super::numbering::{format_number, Counters, Suffix};
use super::props::{Border, Borders, ParaProps, RunProps, VertAlign};
use crate::ground::display::{Color, Item, Paint, PathOp, Rect, Stroke};
use crate::ground::text::{
    layout_paragraph, Align, FontCache, FontMetrics, LineSpacing, Measurer, ParaOptions, ParagraphLayout, ShapedGlyph, ShapedRun,
    StyledRun, TabStop, TextStyle,
};

/// One laid-out page.
#[derive(Debug, Clone)]
pub struct Page {
    pub width: f64,
    pub height: f64,
    /// Number shown by `PAGE` (sections may restart it).
    pub number: i64,
    pub header_items: Vec<Item>,
    pub body_items: Vec<Item>,
    pub footer_items: Vec<Item>,
    pub header_text: String,
    pub body_text: String,
    pub footer_text: String,
}

/// The whole document laid out.
#[derive(Debug, Clone, Default)]
pub struct Laid {
    pub pages: Vec<Page>,
    /// The page each body paragraph starts on (by flat index).
    pub para_page: Vec<usize>,
}

/// Lay out the model. A document with `NUMPAGES` is laid twice when the first count was wrong.
pub fn layout(model: &Model, fonts: &mut FontCache) -> Laid {
    let first = Engine::new(model, fonts, 1).run();
    if model.has_numpages && first.pages.len() != 1 {
        let n = first.pages.len();
        return Engine::new(model, fonts, n).run();
    }
    first
}

// ---------------------------------------------------------------------------------------------------
// Measuring: real fonts, plus pictures as placeholder glyphs
// ---------------------------------------------------------------------------------------------------

const PIC_PREFIX: &str = "\u{1}pic:";
const PIC_CHAR: char = '\u{E000}';

fn pic_size(style: &TextStyle) -> Option<(f64, f64)> {
    let rest = style.family.strip_prefix(PIC_PREFIX)?;
    let mut it = rest.split(':');
    let w = it.next()?.parse().ok()?;
    let h = it.next()?.parse().ok()?;
    Some((w, h))
}

fn pic_metrics(h: f64) -> FontMetrics {
    FontMetrics { ascent: h, descent: 0.0, line_gap: 0.0, underline_y: 1.0, underline_thickness: 0.5, strike_y: -h / 2.0, strike_thickness: 0.5 }
}

struct DocMeasurer<'a> {
    fonts: &'a mut FontCache,
}

impl Measurer for DocMeasurer<'_> {
    fn shape(&mut self, text: &str, style: &TextStyle) -> Vec<ShapedRun> {
        if let Some((w, h)) = pic_size(style) {
            return vec![ShapedRun {
                font: 0,
                font_ref: None,
                size: style.size_pt,
                glyphs: vec![ShapedGlyph { gid: 0, advance: w, cluster: 0..text.len() }],
                width: w,
                metrics: pic_metrics(h),
            }];
        }
        self.fonts.shape_text(text, style)
    }

    fn font_metrics(&mut self, style: &TextStyle) -> FontMetrics {
        match pic_size(style) {
            Some((_, h)) => pic_metrics(h),
            None => self.fonts.style_metrics(style),
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Items helpers
// ---------------------------------------------------------------------------------------------------

fn rect_ops(x: f64, y: f64, w: f64, h: f64) -> Vec<PathOp> {
    vec![PathOp::MoveTo(x, y), PathOp::LineTo(x + w, y), PathOp::LineTo(x + w, y + h), PathOp::LineTo(x, y + h), PathOp::Close]
}

fn fill_rect(x: f64, y: f64, w: f64, h: f64, c: Color) -> Item {
    Item::Path { ops: rect_ops(x, y, w, h), fill: Some(Paint::Solid(c)), stroke: None }
}

fn line_item(x1: f64, y1: f64, x2: f64, y2: f64, b: &Border) -> Item {
    Item::Path { ops: vec![PathOp::MoveTo(x1, y1), PathOp::LineTo(x2, y2)], fill: None, stroke: Some(Stroke::new(b.width, b.color)) }
}

fn shift_ops(ops: &mut [PathOp], dx: f64, dy: f64) {
    for op in ops {
        *op = match *op {
            PathOp::MoveTo(x, y) => PathOp::MoveTo(x + dx, y + dy),
            PathOp::LineTo(x, y) => PathOp::LineTo(x + dx, y + dy),
            PathOp::QuadTo(a, b, x, y) => PathOp::QuadTo(a + dx, b + dy, x + dx, y + dy),
            PathOp::CubicTo(a, b, c, d, x, y) => PathOp::CubicTo(a + dx, b + dy, c + dx, d + dy, x + dx, y + dy),
            PathOp::Close => PathOp::Close,
        };
    }
}

/// Move items by (dx, dy).
pub fn shift_items(items: &mut [Item], dx: f64, dy: f64) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    for it in items {
        match it {
            Item::Path { ops, fill, .. } => {
                shift_ops(ops, dx, dy);
                if let Some(Paint::Linear { x0, y0, x1, y1, .. }) = fill {
                    *x0 += dx;
                    *x1 += dx;
                    *y0 += dy;
                    *y1 += dy;
                }
            }
            Item::Glyphs { glyphs, .. } => {
                for g in glyphs {
                    g.x += dx;
                    g.y += dy;
                }
            }
            Item::Image { rect, .. } | Item::Link { rect, .. } => {
                rect.x += dx;
                rect.y += dy;
            }
            Item::PushClip(ops) => shift_ops(ops, dx, dy),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Paragraph building
// ---------------------------------------------------------------------------------------------------

/// What a styled run carries beyond its text style.
#[derive(Debug, Clone, Default)]
struct Meta {
    /// Baseline shift (negative raises), points.
    shift: f64,
    highlight: Option<Color>,
    link: Option<String>,
    pic: Option<Pic>,
}

#[derive(Default)]
struct Segment {
    runs: Vec<StyledRun>,
    meta: Vec<Meta>,
    break_after: bool,
}

impl Segment {
    fn push(&mut self, text: &str, style: &TextStyle, meta: &Meta) {
        if text.is_empty() {
            return;
        }
        if meta.pic.is_none() {
            if let (Some(last), Some(lm)) = (self.runs.last_mut(), self.meta.last()) {
                if last.style == *style && lm.pic.is_none() && lm.shift == meta.shift && lm.highlight == meta.highlight && lm.link == meta.link {
                    last.text.push_str(text);
                    return;
                }
            }
        }
        self.runs.push(StyledRun::new(text, style.clone()));
        self.meta.push(meta.clone());
    }
}

/// A piece of the flow the paginator places: a line (or a table row), or a forced page break.
enum Slice {
    Content { height: f64, items: Vec<Item>, text: String, flat: Option<usize> },
    PageBreak,
}

/// A laid-out table row (items relative to the row's top).
struct RowOut {
    height: f64,
    items: Vec<Item>,
    text: String,
    header: bool,
    flats: Vec<usize>,
}

/// A block flow laid out without pagination (cells, headers, footers).
#[derive(Default)]
struct FlowOut {
    items: Vec<Item>,
    height: f64,
    text: Vec<String>,
    flats: Vec<usize>,
}

struct Engine<'a> {
    m: &'a Model,
    fonts: &'a mut FontCache,
    numpages: usize,
    counters: Counters,
}

fn is_symbol_font(f: &str) -> bool {
    let f = f.to_ascii_lowercase();
    f.contains("symbol") || f.contains("wingdings") || f.contains("webdings")
}

impl<'a> Engine<'a> {
    fn new(m: &'a Model, fonts: &'a mut FontCache, numpages: usize) -> Engine<'a> {
        Engine { m, fonts, numpages, counters: Counters::default() }
    }

    fn text_style(&self, e: &RunProps) -> TextStyle {
        TextStyle {
            family: e.font.clone().unwrap_or_else(|| "Times New Roman".to_string()),
            east_asian_family: e.font_ea.clone().unwrap_or_default(),
            size_pt: e.size.unwrap_or(10.0).max(1.0),
            weight: if e.bold == Some(true) { 700 } else { 400 },
            italic: e.italic == Some(true),
            color: e.color.flatten().unwrap_or(Color::BLACK),
            letter_spacing: e.spacing.unwrap_or(0.0),
            underline: e.underline == Some(true),
            strike: e.strike == Some(true),
        }
    }

    /// A run's effective properties over the paragraph level `base`.
    fn run_props(&self, base: &RunProps, direct: &RunProps) -> RunProps {
        let mut e = base.clone();
        if let Some(cs) = &direct.style {
            e.over_toggling(&self.m.styles.char_rpr(cs));
        }
        e.over(direct);
        e
    }

    /// The paragraph's effective paragraph properties, its run base, and its list label.
    fn resolve(&self, p: &Para, tstyle: Option<&str>, counters: Option<&mut Counters>) -> (ParaProps, RunProps, Option<(String, RunProps, Suffix)>) {
        let st = &self.m.styles;
        let mut pp = st.doc_ppr.clone();
        let mut rp = st.doc_rpr.clone();
        if let Some(ts) = tstyle {
            let (a, b) = st.folded(ts);
            pp.over(&a);
            rp.over(&b);
        }
        let sid = p.ppr.style.clone().or_else(|| st.default_para.clone());
        if let Some(s) = &sid {
            let (a, b) = st.folded(s);
            pp.over(&a);
            rp.over(&b);
        }
        let num_id = p.ppr.num_id.or(pp.num_id);
        let ilvl = p.ppr.ilvl.or(pp.ilvl).unwrap_or(0);
        let mut label = None;
        if let (Some(n), Some(c)) = (num_id, counters) {
            if let Some((text, level)) = self.m.numbering.next_label(c, n, ilvl, &self.m.styles) {
                let mut lp = level.ppr.clone();
                lp.num_id = None;
                lp.ilvl = None;
                lp.style = None;
                pp.over(&lp);
                let mut lr = self.run_props(&rp, &p.mark);
                let mut lvl_r = level.rpr.clone();
                if level.fmt == "bullet" && lvl_r.font.as_deref().is_some_and(is_symbol_font) {
                    // The bullet was mapped to a Unicode character: draw it in the text's font.
                    lvl_r.font = None;
                }
                lr.over(&lvl_r);
                lr.underline = lvl_r.underline;
                lr.highlight = None;
                label = Some((text, lr, level.suffix));
            }
        }
        let mut direct = p.ppr.clone();
        direct.style = None;
        pp.over(&direct);
        (pp, rp, label)
    }

    fn page_number_text(n: i64, fmt: Option<&str>) -> String {
        match fmt {
            Some(f) if f != "decimal" => format_number(n, f),
            _ => n.to_string(),
        }
    }

    /// Cut a paragraph into segments (split at page breaks) of styled runs.
    fn segments(&mut self, p: &Para, base: &RunProps, label: Option<(String, RunProps, Suffix)>, page: Pg) -> Vec<Segment> {
        let mut segs = vec![Segment::default()];
        if let Some((text, lr, suffix)) = label {
            let style = self.text_style(&lr);
            let mut t = text;
            match suffix {
                Suffix::Tab => t.push('\t'),
                Suffix::Space => t.push(' '),
                Suffix::Nothing => {}
            }
            segs[0].push(&t, &style, &Meta::default());
        }
        for run in &p.runs {
            let e = self.run_props(base, &run.rpr);
            if e.hidden == Some(true) {
                continue;
            }
            let mut style = self.text_style(&e);
            let mut meta = Meta { highlight: e.highlight.flatten().or_else(|| e.shade.flatten()), link: run.link.clone(), ..Meta::default() };
            match e.vert {
                Some(VertAlign::Super) => {
                    meta.shift = -style.size_pt * 0.33;
                    style.size_pt *= 0.65;
                }
                Some(VertAlign::Sub) => {
                    meta.shift = style.size_pt * 0.14;
                    style.size_pt *= 0.65;
                }
                _ => {}
            }
            let caps = e.caps == Some(true);
            let small = e.small_caps == Some(true) && !caps;
            for item in &run.items {
                let seg = segs.last_mut().expect("one segment");
                match item {
                    RunItem::Text(t) => {
                        if caps {
                            seg.push(&t.to_uppercase(), &style, &meta);
                        } else if small {
                            let mut small_style = style.clone();
                            small_style.size_pt *= 0.8;
                            let mut buf = String::new();
                            let mut buf_lower = false;
                            for ch in t.chars() {
                                let lower = ch.is_lowercase();
                                if lower != buf_lower && !buf.is_empty() {
                                    seg.push(&buf, if buf_lower { &small_style } else { &style }, &meta);
                                    buf.clear();
                                }
                                buf_lower = lower;
                                if lower {
                                    buf.extend(ch.to_uppercase());
                                } else {
                                    buf.push(ch);
                                }
                            }
                            seg.push(&buf, if buf_lower { &small_style } else { &style }, &meta);
                        } else {
                            seg.push(t, &style, &meta);
                        }
                    }
                    RunItem::Tab => seg.push("\t", &style, &meta),
                    RunItem::Break => seg.push("\n", &style, &meta),
                    RunItem::Sym(c, font) => {
                        let mut s2 = style.clone();
                        if let Some(f) = font {
                            s2.family = f.clone();
                        }
                        seg.push(&c.to_string(), &s2, &meta);
                    }
                    RunItem::Pic(pic) => {
                        let mut s2 = style.clone();
                        s2.family = format!("{PIC_PREFIX}{}:{}", pic.w, pic.h);
                        let m2 = Meta { pic: Some(pic.clone()), highlight: None, ..meta.clone() };
                        seg.push(&PIC_CHAR.to_string(), &s2, &m2);
                    }
                    RunItem::Field(FieldKind::Page) => {
                        // Each page break inside the paragraph moves its later text one page on.
                        let n = page.n + segs.len() as i64 - 1;
                        let seg = segs.last_mut().expect("one segment");
                        seg.push(&Self::page_number_text(n, page.fmt), &style, &meta)
                    }
                    RunItem::Field(FieldKind::NumPages) => seg.push(&self.numpages.to_string(), &style, &meta),
                    RunItem::PageBreak => {
                        seg.break_after = true;
                        segs.push(Segment::default());
                    }
                }
            }
        }
        // A trailing empty segment after a page break at the paragraph's end holds only the mark:
        // Word puts the rest of that paragraph (nothing) on the next page as an empty line.
        let mark = self.run_props(base, &p.mark);
        let mark_style = self.text_style(&mark);
        for s in &mut segs {
            if s.runs.is_empty() {
                s.runs.push(StyledRun::new("", mark_style.clone()));
                s.meta.push(Meta::default());
            }
        }
        segs
    }

    /// Lay out a paragraph into line slices at column `x` of width `width`.
    fn para_slices(&mut self, p: &Para, x: f64, width: f64, tstyle: Option<&str>, page: Pg, body: bool) -> (Vec<Slice>, bool) {
        let mut local = Counters::default();
        let mut counters = std::mem::take(&mut self.counters);
        let (pp, base, label) = self.resolve(p, tstyle, Some(if body { &mut counters } else { &mut local }));
        self.counters = counters;
        let segs = self.segments(p, &base, label, page);
        let mut opts = ParaOptions::new(width.max(1.0));
        opts.align = pp.align.unwrap_or(Align::Left);
        opts.left_indent = pp.ind_left.unwrap_or(0.0);
        opts.right_indent = pp.ind_right.unwrap_or(0.0);
        opts.first_line_indent = pp.ind_first.unwrap_or(0.0);
        opts.line_spacing = pp.line.unwrap_or(LineSpacing::Single);
        opts.tab_stops = pp.tabs.iter().filter(|t| !t.clear).map(|t| TabStop { pos: t.pos, kind: t.kind, leader: t.leader }).collect();
        opts.default_tab_width = self.m.settings.default_tab;
        let before = pp.before.unwrap_or(0.0).max(0.0);
        let after = pp.after.unwrap_or(0.0).max(0.0);
        let shade = pp.shade.flatten();
        let mut slices = Vec::new();
        let n_segs = segs.len();
        let mut first_flat = if p.flat == usize::MAX { None } else { Some(p.flat) };
        for (si, seg) in segs.iter().enumerate() {
            opts.space_before = if si == 0 { before } else { 0.0 };
            opts.space_after = if si + 1 == n_segs { after } else { 0.0 };
            let lay = {
                let mut m = DocMeasurer { fonts: &mut *self.fonts };
                layout_paragraph(&mut m, &seg.runs, &opts)
            };
            let n = lay.lines.len();
            for li in 0..n {
                let top_rel = if li == 0 { 0.0 } else { lay.lines[li].top };
                let bot_rel = if li + 1 == n { lay.height } else { lay.lines[li + 1].top };
                let mut items = Vec::new();
                if let Some(c) = shade {
                    items.push(fill_rect(x + opts.left_indent, 0.0, width - opts.left_indent - opts.right_indent, bot_rel - top_rel, c));
                }
                items.extend(line_items(&lay, li, &seg.meta, x, -top_rel));
                let line = &lay.lines[li];
                let text: String = lay.text[line.range.clone()].chars().filter(|c| *c != PIC_CHAR && *c != '\n').collect();
                slices.push(Slice::Content { height: (bot_rel - top_rel).max(0.0), items, text, flat: first_flat.take() });
            }
            if seg.break_after {
                slices.push(Slice::PageBreak);
            }
        }
        (slices, pp.page_break_before == Some(true))
    }

    /// Lay out blocks without pagination (a cell, a header, a footer) at column `x`.
    fn flow(&mut self, blocks: &[Block], x: f64, width: f64, tstyle: Option<&str>, page: Pg, body: bool) -> FlowOut {
        let mut out = FlowOut::default();
        for b in blocks {
            match b {
                Block::Para(p) => {
                    let (slices, _) = self.para_slices(p, x, width, tstyle, page, body);
                    for s in slices {
                        if let Slice::Content { height, mut items, text, flat } = s {
                            shift_items(&mut items, 0.0, out.height);
                            out.items.extend(items);
                            out.height += height;
                            out.text.push(text);
                            if let Some(f) = flat {
                                out.flats.push(f);
                            }
                        }
                    }
                }
                Block::Table(t) => {
                    for row in self.table_rows(t, x, width, page, body) {
                        let mut items = row.items;
                        shift_items(&mut items, 0.0, out.height);
                        out.items.extend(items);
                        out.height += row.height;
                        out.text.push(row.text);
                        out.flats.extend(row.flats);
                    }
                }
            }
        }
        out
    }

    fn table_rows(&mut self, t: &Table, x: f64, avail: f64, page: Pg, body: bool) -> Vec<RowOut> {
        let ncols_needed = t.rows.iter().map(|r| r.cells.iter().map(|c| c.span).sum::<usize>()).max().unwrap_or(0).max(1);
        let mut grid = t.grid.clone();
        let sum: f64 = grid.iter().sum();
        if grid.is_empty() || sum <= 0.0 {
            grid = vec![avail / ncols_needed as f64; ncols_needed];
        } else if grid.len() < ncols_needed {
            let avg = sum / grid.len() as f64;
            grid.resize(ncols_needed, avg);
        }
        let ncols = grid.len();
        let total: f64 = grid.iter().sum();
        let tstyle = t.style.clone().or_else(|| self.m.styles.default_table.clone());
        let (mut tb, mut mar) = match &tstyle {
            Some(s) => self.m.styles.table_look(s),
            None => (Borders::default(), Default::default()),
        };
        tb.over(&t.borders);
        mar.over(&t.cell_mar);
        let x0 = match t.align {
            Align::Center => x + (avail - total) / 2.0,
            Align::Right => x + avail - total,
            _ => x + t.ind,
        };
        let col_x: Vec<f64> = std::iter::once(0.0)
            .chain(grid.iter().scan(0.0, |acc, w| {
                *acc += w;
                Some(*acc)
            }))
            .collect();

        struct CellOut<'c> {
            cell: &'c Cell,
            col: usize,
            span: usize,
            x: f64,
            w: f64,
            mt: f64,
            mb: f64,
            flow: FlowOut,
        }
        let mut rows: Vec<Vec<CellOut>> = Vec::new();
        for row in &t.rows {
            let mut col = 0usize;
            let mut cells = Vec::new();
            for cell in &row.cells {
                let span = cell.span.min(ncols.saturating_sub(col).max(1));
                let c0 = col.min(ncols - 1);
                let c1 = (col + span).min(ncols);
                let cx = x0 + col_x[c0];
                let cw = (col_x[c1.max(c0 + 1)] - col_x[c0]).max(1.0);
                let mut m = mar;
                m.over(&cell.mar);
                let ml = m.left.unwrap_or(5.4);
                let mr = m.right.unwrap_or(5.4);
                let mt = m.top.unwrap_or(0.0);
                let mb = m.bottom.unwrap_or(0.0);
                let flow = if cell.vmerge == VMerge::Continue {
                    FlowOut::default()
                } else {
                    self.flow(&cell.blocks, cx + ml, (cw - ml - mr).max(1.0), tstyle.as_deref(), page, body)
                };
                cells.push(CellOut { cell, col, span, x: cx, w: cw, mt, mb, flow });
                col += span;
            }
            rows.push(cells);
        }
        // Row heights.
        let mut heights: Vec<f64> = Vec::with_capacity(rows.len());
        for (ri, cells) in rows.iter().enumerate() {
            let mut h: f64 = cells
                .iter()
                .filter(|c| c.cell.vmerge == VMerge::None)
                .map(|c| c.flow.height + c.mt + c.mb)
                .fold(0.0, f64::max);
            match t.rows[ri].height {
                Some((v, true)) => h = v,
                Some((v, false)) => h = h.max(v),
                None => {}
            }
            heights.push(h.max(1.0));
        }
        // Vertically merged cells: the merge's rows grow to hold the first cell's content.
        let continues = |rows: &Vec<Vec<CellOut>>, ri: usize, col: usize| -> bool {
            rows.get(ri).is_some_and(|cells| cells.iter().any(|c| c.col == col && c.cell.vmerge == VMerge::Continue))
        };
        let mut merge_extent: Vec<Vec<usize>> = rows.iter().map(|cells| vec![0; cells.len()]).collect();
        for ri in 0..rows.len() {
            for ci in 0..rows[ri].len() {
                let c = &rows[ri][ci];
                if c.cell.vmerge != VMerge::Restart {
                    continue;
                }
                let mut k = 0;
                while continues(&rows, ri + k + 1, c.col) {
                    k += 1;
                }
                merge_extent[ri][ci] = k;
                let need = c.flow.height + c.mt + c.mb;
                let have: f64 = heights[ri..=ri + k].iter().sum();
                if need > have && !matches!(t.rows[ri + k].height, Some((_, true))) {
                    heights[ri + k] += need - have;
                }
            }
        }
        // Items.
        let last_row = rows.len().saturating_sub(1);
        let mut out = Vec::with_capacity(rows.len());
        for (ri, cells) in rows.into_iter().enumerate() {
            let rh = heights[ri];
            let mut fills = Vec::new();
            let mut content = Vec::new();
            let mut lines = Vec::new();
            let mut texts = Vec::new();
            let mut flats = Vec::new();
            for (ci, c) in cells.into_iter().enumerate() {
                let extent = merge_extent[ri][ci];
                let span_h: f64 = heights[ri..=(ri + extent).min(heights.len() - 1)].iter().sum();
                if let Some(fill) = c.cell.fill {
                    if c.cell.vmerge != VMerge::Continue {
                        fills.push(fill_rect(c.x, 0.0, c.w, span_h, fill));
                    }
                }
                if c.cell.vmerge != VMerge::Continue {
                    let room = span_h - c.mt - c.mb - c.flow.height;
                    let off = match c.cell.valign {
                        VAlign::Top => 0.0,
                        VAlign::Center => (room / 2.0).max(0.0),
                        VAlign::Bottom => room.max(0.0),
                    };
                    let mut items = c.flow.items;
                    shift_items(&mut items, 0.0, c.mt + off);
                    content.extend(items);
                    texts.push(c.flow.text.join("\n"));
                    flats.extend(c.flow.flats);
                }
                // Borders: the cell's own, else the table's outer or inside lines.
                let b = &c.cell.borders;
                let first_col = c.col == 0;
                let last_col = c.col + c.span >= ncols;
                let top = b.top.or(if ri == 0 { tb.top } else { tb.inside_h });
                let bottom = b.bottom.or(if ri == last_row { tb.bottom } else { tb.inside_h });
                let left = b.left.or(if first_col { tb.left } else { tb.inside_v });
                let right = b.right.or(if last_col { tb.right } else { tb.inside_v });
                let (x1, x2) = (c.x, c.x + c.w);
                if c.cell.vmerge != VMerge::Continue {
                    if let Some(bd) = Border::visible(top) {
                        lines.push(line_item(x1, 0.0, x2, 0.0, &bd));
                    }
                }
                let merged_below = c.cell.vmerge != VMerge::None && ri < last_row && extent_continues(&t.rows, ri, c.col);
                if !merged_below {
                    if let Some(bd) = Border::visible(bottom) {
                        lines.push(line_item(x1, rh, x2, rh, &bd));
                    }
                }
                if let Some(bd) = Border::visible(left) {
                    lines.push(line_item(x1, 0.0, x1, rh, &bd));
                }
                if let Some(bd) = Border::visible(right) {
                    lines.push(line_item(x2, 0.0, x2, rh, &bd));
                }
            }
            let mut items = fills;
            items.extend(content);
            items.extend(lines);
            out.push(RowOut { height: rh, items, text: texts.join("\t"), header: t.rows[ri].header, flats });
        }
        out
    }
}

/// Whether the row after `ri` has a continuing vertical merge at grid column `col`.
fn extent_continues(rows: &[super::model::Row], ri: usize, col: usize) -> bool {
    let Some(next) = rows.get(ri + 1) else { return false };
    let mut c = 0;
    for cell in &next.cells {
        if c == col {
            return cell.vmerge == VMerge::Continue;
        }
        c += cell.span;
        if c > col {
            return false;
        }
    }
    false
}

/// The display items of one line of a laid paragraph, placed with the paragraph's origin at
/// (`x`, `y_origin`): highlights, glyphs (with superscript / subscript shifts), decorations,
/// pictures and link areas.
fn line_items(lay: &ParagraphLayout, li: usize, metas: &[Meta], x: f64, y_origin: f64) -> Vec<Item> {
    let line = &lay.lines[li];
    let mut items = Vec::new();
    let base_y = y_origin + line.y_baseline;
    for s in &line.spans {
        if let Some(c) = metas.get(s.run).and_then(|m| m.highlight) {
            let w = s.x_end - s.x_start;
            if w > 0.0 {
                items.push(fill_rect(x + s.x_start, base_y - s.metrics.ascent, w, s.metrics.ascent + s.metrics.descent, c));
            }
        }
    }
    let mut shifted = line.clone();
    for s in &mut shifted.spans {
        let sh = metas.get(s.run).map(|m| m.shift).unwrap_or(0.0);
        if sh != 0.0 {
            for g in &mut s.glyphs {
                g.y += sh;
            }
        }
    }
    let one = ParagraphLayout { text: String::new(), lines: vec![shifted], height: 0.0, width: lay.width };
    items.extend(one.to_items(x, y_origin));
    for s in &line.spans {
        let Some(meta) = metas.get(s.run) else { continue };
        if let Some(pic) = &meta.pic {
            let r = Rect::new(x + s.x_start, base_y - pic.h, pic.w, pic.h);
            match pic.image {
                Some(id) => items.push(Item::Image { image: id, rect: r }),
                None => {
                    items.push(fill_rect(r.x, r.y, r.w, r.h, Color::rgb(0xEE, 0xEE, 0xEE)));
                    items.push(Item::Path { ops: rect_ops(r.x, r.y, r.w, r.h), fill: None, stroke: Some(Stroke::new(0.5, Color::rgb(0x99, 0x99, 0x99))) });
                }
            }
        }
        if let Some(target) = &meta.link {
            let w = s.x_end - s.x_start;
            if w > 0.0 {
                items.push(Item::Link { rect: Rect::new(x + s.x_start, y_origin + line.top, w, line.height), target: target.clone() });
            }
        }
    }
    items
}

// ---------------------------------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------------------------------

struct Pager {
    pages: Vec<Page>,
    body_text: Vec<Vec<String>>,
    y: f64,
    bottom: f64,
    has_content: bool,
    para_page: Vec<usize>,
}

impl Pager {
    fn place(&mut self, height: f64, mut items: Vec<Item>, text: String) {
        shift_items(&mut items, 0.0, self.y);
        if let Some(p) = self.pages.last_mut() {
            p.body_items.extend(items);
        }
        if let Some(t) = self.body_text.last_mut() {
            t.push(text);
        }
        self.y += height;
        self.has_content = true;
    }

    fn fits(&self, height: f64) -> bool {
        self.y + height <= self.bottom + 0.01 || !self.has_content
    }

    fn mark(&mut self, flat: usize) {
        if flat < self.para_page.len() {
            self.para_page[flat] = self.pages.len().saturating_sub(1);
        }
    }
}

impl<'a> Engine<'a> {
    fn run(mut self) -> Laid {
        let m = self.m;
        // Sections: (blocks, sectPr).
        let mut sections: Vec<(Vec<&Block>, &SectPr)> = Vec::new();
        let mut cur: Vec<&Block> = Vec::new();
        for b in &m.body {
            cur.push(b);
            if let Block::Para(p) = b {
                if let Some(s) = &p.sect {
                    sections.push((std::mem::take(&mut cur), s));
                }
            }
        }
        sections.push((cur, &m.final_sect));

        let mut pager = Pager { pages: Vec::new(), body_text: Vec::new(), y: 0.0, bottom: 0.0, has_content: false, para_page: vec![0; m.flat.len()] };
        let mut refs_h: [Option<String>; 3] = [None, None, None];
        let mut refs_f: [Option<String>; 3] = [None, None, None];
        let mut next_number: i64 = 1;

        for (si, (blocks, sect)) in sections.iter().enumerate() {
            for i in 0..3 {
                if sect.headers[i].is_some() {
                    refs_h[i] = sect.headers[i].clone();
                }
                if sect.footers[i].is_some() {
                    refs_f[i] = sect.footers[i].clone();
                }
            }
            let geom = Geom { sect: (*sect).clone(), refs_h: refs_h.clone(), refs_f: refs_f.clone() };
            if let Some(start) = sect.pg_start {
                next_number = start;
            }
            let continuous = si > 0
                && sect.kind == SectKind::Continuous
                && pager.pages.last().is_some_and(|p| (p.width - sect.page_w).abs() < 0.5 && (p.height - sect.page_h).abs() < 0.5);
            // A continuous section stays on the page; its own margins apply from here down.
            if !continuous {
                if si > 0 {
                    let parity_wrong = match sect.kind {
                        SectKind::EvenPage => next_number % 2 != 0,
                        SectKind::OddPage => next_number % 2 == 0,
                        _ => false,
                    };
                    if parity_wrong {
                        self.new_page(&mut pager, &geom, next_number, false);
                        next_number += 1;
                    }
                }
                self.new_page(&mut pager, &geom, next_number, true);
                next_number += 1;
            }
            let x = sect.left;
            let width = (sect.page_w - sect.left - sect.right).max(10.0);
            for b in blocks {
                match b {
                    Block::Para(p) => {
                        let pg = Pg { n: pager.pages.last().map(|p| p.number).unwrap_or(1), fmt: sect.pg_fmt.as_deref() };
                        let saved = self.counters.clone();
                        let (mut slices, break_before) = self.para_slices(p, x, width, None, pg, true);
                        if break_before && pager.has_content {
                            self.new_page(&mut pager, &geom, next_number, false);
                            next_number += 1;
                            if has_page_field(p) {
                                // Lay it again with the page it now starts on (the list counters too).
                                self.counters = saved;
                                let pg = Pg { n: next_number - 1, fmt: sect.pg_fmt.as_deref() };
                                slices = self.para_slices(p, x, width, None, pg, true).0;
                            }
                        }
                        for s in slices {
                            match s {
                                Slice::PageBreak => {
                                    self.new_page(&mut pager, &geom, next_number, false);
                                    next_number += 1;
                                }
                                Slice::Content { height, items, text, flat } => {
                                    if !pager.fits(height) {
                                        self.new_page(&mut pager, &geom, next_number, false);
                                        next_number += 1;
                                    }
                                    if let Some(f) = flat {
                                        pager.mark(f);
                                    }
                                    pager.place(height, items, text);
                                }
                            }
                        }
                    }
                    Block::Table(t) => {
                        let pg = Pg { n: pager.pages.last().map(|p| p.number).unwrap_or(1), fmt: sect.pg_fmt.as_deref() };
                        let rows = self.table_rows(t, x, width, pg, true);
                        let n_header = rows.iter().take_while(|r| r.header).count();
                        for i in 0..rows.len() {
                            if !pager.fits(rows[i].height) {
                                self.new_page(&mut pager, &geom, next_number, false);
                                next_number += 1;
                                if i >= n_header && n_header > 0 {
                                    for h in &rows[..n_header] {
                                        pager.place(h.height, h.items.clone(), h.text.clone());
                                    }
                                }
                            }
                            for f in &rows[i].flats {
                                pager.mark(*f);
                            }
                            pager.place(rows[i].height, rows[i].items.clone(), rows[i].text.clone());
                        }
                    }
                }
            }
        }
        if pager.pages.is_empty() {
            let geom = Geom { sect: m.final_sect.clone(), refs_h, refs_f };
            self.new_page(&mut pager, &geom, 1, true);
        }
        let mut pages = pager.pages;
        for (p, t) in pages.iter_mut().zip(pager.body_text) {
            p.body_text = t.join("\n");
        }
        Laid { pages, para_page: pager.para_page }
    }

    /// Start a page: lay out its header and footer and set the body's top and bottom.
    fn new_page(&mut self, pager: &mut Pager, g: &Geom, number: i64, first_of_section: bool) {
        let s = &g.sect;
        let which = if s.title_pg && first_of_section {
            1
        } else if self.m.settings.even_odd_headers && number % 2 == 0 {
            2
        } else {
            0
        };
        let pg = Pg { n: number, fmt: s.pg_fmt.as_deref() };
        let x = s.left;
        let width = (s.page_w - s.left - s.right).max(10.0);
        let mut header = match g.refs_h[which].as_ref().and_then(|n| self.m.headers.get(n)) {
            Some(blocks) => self.flow(blocks, x, width, None, pg, false),
            None => FlowOut::default(),
        };
        let mut footer = match g.refs_f[which].as_ref().and_then(|n| self.m.headers.get(n)) {
            Some(blocks) => self.flow(blocks, x, width, None, pg, false),
            None => FlowOut::default(),
        };
        shift_items(&mut header.items, 0.0, s.header);
        let footer_top = s.page_h - s.footer - footer.height;
        shift_items(&mut footer.items, 0.0, footer_top);
        // A negative top or bottom margin is fixed: the header or footer may not push the body.
        let top = if s.top < 0.0 { -s.top } else { s.top.max(if header.height > 0.0 { s.header + header.height } else { 0.0 }) };
        let bottom = if s.bottom < 0.0 {
            s.page_h + s.bottom
        } else {
            (s.page_h - s.bottom).min(if footer.height > 0.0 { footer_top } else { s.page_h })
        };
        pager.pages.push(Page {
            width: s.page_w,
            height: s.page_h,
            number,
            header_items: header.items,
            body_items: Vec::new(),
            footer_items: footer.items,
            header_text: header.text.join("\n"),
            body_text: String::new(),
            footer_text: footer.text.join("\n"),
        });
        pager.body_text.push(Vec::new());
        pager.y = top;
        pager.bottom = bottom.max(top + 10.0);
        pager.has_content = false;
    }
}

/// The page number a field shows, and the section's number format.
#[derive(Clone, Copy)]
struct Pg<'f> {
    n: i64,
    fmt: Option<&'f str>,
}

fn has_page_field(p: &Para) -> bool {
    p.runs.iter().any(|r| r.items.iter().any(|i| matches!(i, RunItem::Field(FieldKind::Page))))
}

/// A section's geometry with its header and footer references after inheritance.
struct Geom {
    sect: SectPr,
    refs_h: [Option<String>; 3],
    refs_f: [Option<String>; 3],
}
