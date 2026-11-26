//! The grid: one sheet's viewport as a display list. Virtualised: only the rows and columns that
//! fall inside the view are visited (the cells come from the sparse rows by range), so a sheet of
//! 1,048,576 x 16,384 draws in the time of its visible part.
//!
//! Order of painting: white page, gridlines, then per pane quadrant (frozen rows/columns and the
//! scrolling part, each clipped to itself) cell fills, merged areas, borders and text; then the
//! column and row headers and the frozen-pane lines.
//!
//! Text: the cell's font, size, colour, bold, italic, underline and strike; the number format's
//! colour wins over the font's. General alignment puts numbers right, text left, booleans and
//! errors centred; horizontal, vertical, wrap and indent follow the cell format. Text that does not
//! fit overflows into empty neighbours (right for left-aligned, left for right-aligned, both ways
//! for centred); numbers never overflow: General loses digits, other formats show `####`.
//!
//! Not yet: rotated text, shrink-to-fit, text overflowing in from a cell left of the view, rich
//! text runs, conditional formats, pictures and charts, the selection (the host draws it).

use std::collections::HashMap;

use super::cellref::{col_name, CellRange, MAX_COLS, MAX_ROWS};
use super::model::{Cell, CellValue, Sheet};
use super::numfmt::{general_fit, NumberFormat};
use super::styles::{BorderSide, BorderStyle, Font, HAlign, Styles, VAlign};
use crate::ground::display::{Color, DisplayList, Item, Paint, PathOp, Rect, Stroke};
use crate::ground::text::{layout_paragraph, Align, FontCache, Measurer, ParaOptions, StyledRun, TextStyle};

/// Height of the column header band, in points.
pub const HEADER_H: f64 = 15.0;
/// Space between a cell's edge and its text.
const PAD: f64 = 2.0;
const GRID: Color = Color { r: 0xD4, g: 0xD4, b: 0xD4, a: 255 };
const HEADER_BG: Color = Color { r: 0xF3, g: 0xF3, b: 0xF3, a: 255 };
const HEADER_LINE: Color = Color { r: 0xC8, g: 0xC8, b: 0xC8, a: 255 };
const HEADER_TEXT: Color = Color { r: 0x44, g: 0x44, b: 0x44, a: 255 };
const FREEZE: Color = Color { r: 0x7F, g: 0x7F, b: 0x7F, a: 255 };

/// Width of the row header band for the last row number it shows.
pub fn row_header_width(last_row: u32) -> f64 {
    let digits = (last_row as u64 + 1).to_string().len().max(3);
    10.0 + 6.0 * digits as f64
}

fn rect_path(r: Rect) -> Vec<PathOp> {
    vec![PathOp::MoveTo(r.x, r.y), PathOp::LineTo(r.right(), r.y), PathOp::LineTo(r.right(), r.bottom()), PathOp::LineTo(r.x, r.bottom()), PathOp::Close]
}

fn fill_rect(dl: &mut DisplayList, r: Rect, c: Color) {
    if r.w > 0.0 && r.h > 0.0 {
        dl.push(Item::Path { ops: rect_path(r), fill: Some(Paint::Solid(c)), stroke: None });
    }
}

fn stroke_lines(dl: &mut DisplayList, lines: &[(f64, f64, f64, f64)], stroke: Stroke) {
    if lines.is_empty() {
        return;
    }
    let mut ops = Vec::with_capacity(lines.len() * 2);
    for &(x1, y1, x2, y2) in lines {
        ops.push(PathOp::MoveTo(x1, y1));
        ops.push(PathOp::LineTo(x2, y2));
    }
    dl.push(Item::Path { ops, fill: None, stroke: Some(stroke) });
}

/// The visible rows (or columns) of a view: frozen ones first, then the scrolling ones.
pub struct Axis {
    /// (index, start, size) in screen order; indices increase.
    pub items: Vec<(u32, f64, f64)>,
    pub frozen_n: u32,
    /// Where the scrolling region starts on screen.
    pub frozen_end: f64,
    pub top: u32,
    pub start: f64,
    pub end: f64,
}

impl Axis {
    pub fn plan(size: &dyn Fn(u32) -> f64, start: f64, end: f64, frozen_n: u32, top: u32, max: u32) -> Axis {
        let mut items = Vec::new();
        let mut p = start;
        let mut i = 0;
        while i < frozen_n.min(max) && p < end {
            let s = size(i);
            if s > 0.0 {
                items.push((i, p, s));
            }
            p += s;
            i += 1;
        }
        let frozen_end = p.min(end);
        let top = top.max(frozen_n).min(max.saturating_sub(1));
        let mut i = top;
        while i < max && p < end {
            let s = size(i);
            if s > 0.0 {
                items.push((i, p, s));
            }
            p += s;
            i += 1;
        }
        Axis { items, frozen_n, frozen_end, top, start, end }
    }

    /// The screen position of the leading edge of `i` (may lie outside the view).
    pub fn pos(&self, size: &dyn Fn(u32) -> f64, i: u32) -> f64 {
        if let Ok(k) = self.items.binary_search_by_key(&i, |t| t.0) {
            return self.items[k].1;
        }
        const CAP: u32 = 4096;
        if i < self.frozen_n {
            let mut p = self.start;
            for k in 0..i.min(CAP) {
                p += size(k);
            }
            p + i.saturating_sub(CAP) as f64 * 15.0
        } else if i >= self.top {
            let mut p = self.frozen_end;
            let mut k = self.top;
            while k < i {
                if p > self.end + 10_000.0 {
                    break;
                }
                p += size(k);
                k += 1;
            }
            p
        } else {
            let mut p = self.frozen_end;
            let mut k = self.top;
            while k > i {
                k -= 1;
                p -= size(k);
                if p < self.start - 10_000.0 {
                    break;
                }
            }
            p
        }
    }

    fn region(&self, frozen: bool) -> Vec<(u32, f64, f64)> {
        self.items.iter().copied().filter(|t| (t.0 < self.frozen_n) == frozen).collect()
    }

    /// The screen extent of the frozen or the scrolling region.
    fn extent(&self, frozen: bool) -> (f64, f64) {
        if frozen {
            (self.start, self.frozen_end)
        } else {
            (self.frozen_end, self.end)
        }
    }
}

/// What one cell shows.
struct CellText {
    text: String,
    style: TextStyle,
    h: HAlign,
    v: VAlign,
    wrap: bool,
    indent: f64,
    number: Option<f64>,
    general: bool,
    fill: Option<(usize, char)>,
    hashes: bool,
}

fn text_style(font: &Font, color: Option<Color>) -> TextStyle {
    TextStyle {
        family: font.name.clone(),
        size_pt: if font.size > 0.0 { font.size } else { 11.0 },
        weight: if font.bold { 700 } else { 400 },
        italic: font.italic,
        color: color.or(font.color).unwrap_or(Color::BLACK),
        underline: font.underline,
        strike: font.strike,
        ..TextStyle::default()
    }
}

fn cell_text(cell: &Cell, styles: &Styles, date1904: bool) -> Option<CellText> {
    if cell.value.is_empty() {
        return None;
    }
    let xf = styles.xf(cell.style);
    let code = styles.format_code(xf.num_fmt);
    let nf = NumberFormat::parse(code);
    let f = nf.format(&cell.value, date1904);
    if f.text.is_empty() && f.fill.is_none() {
        return None;
    }
    let h = match xf.h_align {
        HAlign::General => match cell.value {
            CellValue::Number(_) => HAlign::Right,
            CellValue::Bool(_) | CellValue::Error(_) => HAlign::Center,
            _ => HAlign::Left,
        },
        HAlign::CenterContinuous => HAlign::Center,
        other => other,
    };
    let number = match cell.value {
        CellValue::Number(n) => Some(n),
        _ => None,
    };
    Some(CellText {
        text: f.text,
        style: text_style(styles.font(xf.font), f.color),
        h,
        v: xf.v_align,
        wrap: xf.wrap || matches!(xf.h_align, HAlign::Justify | HAlign::Distributed),
        indent: xf.indent as f64 * 9.0 * 0.75,
        number,
        general: number.is_some() && code.trim().eq_ignore_ascii_case("general"),
        fill: f.fill,
        hashes: f.hashes,
    })
}

fn hashes_for(fonts: &mut FontCache, style: &TextStyle, avail: f64) -> String {
    let w = fonts.measure("#", style).max(0.1);
    let n = ((avail / w).floor() as usize).max(1).min(255);
    "#".repeat(n)
}

/// Lays out and pushes one cell's text. `cell` is the cell's (or merged area's) box, `span` the
/// horizontal extent it may overflow into.
fn draw_text(dl: &mut DisplayList, fonts: &mut FontCache, ct: &CellText, cell: Rect, span: (f64, f64)) {
    let avail = (cell.w - 2.0 * PAD).max(0.0);
    let style = &ct.style;
    let runs_of = |s: &str| vec![StyledRun::new(s, style.clone())];
    let (layout, x0, clip) = if ct.wrap {
        let mut opts = ParaOptions::new(avail.max(1.0));
        opts.align = match ct.h {
            HAlign::Center => Align::Center,
            HAlign::Right => Align::Right,
            HAlign::Justify | HAlign::Distributed => Align::Justify,
            _ => Align::Left,
        };
        match ct.h {
            HAlign::Right => opts.right_indent = ct.indent,
            HAlign::Left => opts.left_indent = ct.indent,
            _ => {}
        }
        let layout = layout_paragraph(fonts, &runs_of(&ct.text), &opts);
        (layout, cell.x + PAD, cell)
    } else {
        let mut text: String = ct.text.replace(['\r', '\n'], " ");
        let mut tw = fonts.measure(&text, style);
        if ct.hashes {
            text = hashes_for(fonts, style, avail);
            tw = fonts.measure(&text, style);
        } else if let Some(v) = ct.number {
            if tw > avail + 0.01 {
                let mut done = false;
                if ct.general {
                    let mut k = text.chars().count();
                    while k > 1 {
                        k -= 1;
                        match general_fit(v, k) {
                            Some(s) => {
                                let w = fonts.measure(&s, style);
                                if w <= avail + 0.01 {
                                    text = s;
                                    tw = w;
                                    done = true;
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
                if !done {
                    text = hashes_for(fonts, style, avail);
                    tw = fonts.measure(&text, style);
                }
            }
        }
        if let Some((at, c)) = ct.fill {
            let cw = fonts.measure(&c.to_string(), style).max(0.1);
            let n = (((avail - tw) / cw).floor().max(0.0) as usize).min(512);
            if at <= text.len() && text.is_char_boundary(at) {
                text.insert_str(at, &c.to_string().repeat(n));
                tw = fonts.measure(&text, style);
            }
        }
        if ct.h == HAlign::Fill && tw > 0.0 && tw < avail {
            let n = ((avail / tw).floor() as usize).clamp(1, 256);
            text = text.repeat(n);
            tw = fonts.measure(&text, style);
        }
        let opts = ParaOptions::new(1e7);
        let layout = layout_paragraph(fonts, &runs_of(&text), &opts);
        let x = match ct.h {
            HAlign::Right => cell.right() - PAD - tw - ct.indent,
            HAlign::Center => cell.x + (cell.w - tw) / 2.0,
            _ if ct.fill.is_some() => cell.x + PAD,
            _ => cell.x + PAD + ct.indent,
        };
        (layout, x, Rect::new(span.0, cell.y, span.1 - span.0, cell.h))
    };
    let th = layout.height;
    let y0 = match ct.v {
        VAlign::Top | VAlign::Justify | VAlign::Distributed => cell.y + 1.0,
        VAlign::Center => cell.y + (cell.h - th) / 2.0,
        VAlign::Bottom => cell.bottom() - th - 1.0,
    };
    let items = layout.to_items(x0, y0);
    if items.is_empty() {
        return;
    }
    dl.push(Item::PushClip(rect_path(clip)));
    dl.items.extend(items);
    dl.push(Item::PopClip);
}

fn border_lines(dl: &mut DisplayList, side: &BorderSide, x1: f64, y1: f64, x2: f64, y2: f64) {
    let mut s = Stroke::new(side.style.width(), side.color);
    s.dash = side.style.dash();
    if side.style == BorderStyle::Double {
        // Two thin lines either side of the edge.
        let thin = Stroke::new(0.75, side.color);
        let (dx, dy) = if x1 == x2 { (1.0, 0.0) } else { (0.0, 1.0) };
        stroke_lines(dl, &[(x1 - dx, y1 - dy, x2 - dx, y2 - dy), (x1 + dx, y1 + dy, x2 + dx, y2 + dy)], thin);
        return;
    }
    stroke_lines(dl, &[(x1, y1, x2, y2)], s);
}

/// Renders a sheet's current view.
pub fn render(sheet: &Sheet, styles: &Styles, date1904: bool, fonts: &mut FontCache) -> DisplayList {
    let view = sheet.view;
    let (w, h) = (view.width.max(1.0), view.height.max(1.0));
    let mut dl = DisplayList::new(w, h);
    fill_rect(&mut dl, Rect::new(0.0, 0.0, w, h), Color::WHITE);
    let (fr, fc) = sheet.frozen();
    let row_size = |r: u32| sheet.row_height(r);
    let col_size = |c: u32| sheet.col_width(c);
    let rows = Axis::plan(&row_size, HEADER_H, h, fr, view.row, MAX_ROWS);
    let last_row = rows.items.last().map(|t| t.0).unwrap_or(0);
    let header_w = row_header_width(last_row);
    let cols = Axis::plan(&col_size, header_w, w, fc, view.col, MAX_COLS);

    // Gridlines.
    if sheet.show_grid {
        let mut lines = Vec::with_capacity(rows.items.len() + cols.items.len());
        for &(_, x, cw) in &cols.items {
            lines.push((x + cw, HEADER_H, x + cw, h));
        }
        for &(_, y, rh) in &rows.items {
            lines.push((header_w, y + rh, w, y + rh));
        }
        stroke_lines(&mut dl, &lines, Stroke::new(0.75, GRID));
    }

    // The merged ranges that touch the view.
    let view_range = match (rows.items.first(), rows.items.last(), cols.items.first(), cols.items.last()) {
        (Some(r1), Some(r2), Some(c1), Some(c2)) => Some(CellRange::new(r1.0, c1.0, r2.0, c2.0)),
        _ => None,
    };
    let merges: Vec<CellRange> = match view_range {
        Some(v) => {
            // A merge reaching into a frozen pane can start before the scrolled top.
            let reach = CellRange::new(0, 0, v.r2, v.c2);
            sheet.merges.iter().copied().filter(|m| m.intersects(&reach)).collect()
        }
        None => Vec::new(),
    };
    let covered = |r: u32, c: u32| merges.iter().find(|m| m.contains(r, c));

    for &(rf, cf) in &[(true, true), (true, false), (false, true), (false, false)] {
        let qrows = rows.region(rf);
        let qcols = cols.region(cf);
        if qrows.is_empty() || qcols.is_empty() {
            continue;
        }
        let (y1, y2) = rows.extent(rf);
        let (x1, x2) = cols.extent(cf);
        let qrect = Rect::new(x1, y1, x2 - x1, y2 - y1);
        let (r_lo, r_hi) = (qrows[0].0, qrows[qrows.len() - 1].0);
        let (c_lo, c_hi) = (qcols[0].0, qcols[qcols.len() - 1].0);
        let qrange = CellRange::new(r_lo, c_lo, r_hi, c_hi);
        let col_at: HashMap<u32, (f64, f64)> = qcols.iter().map(|&(c, x, cw)| (c, (x, cw))).collect();
        dl.push(Item::PushClip(rect_path(qrect)));

        // Fills of ordinary cells.
        for &(r, y, rh) in &qrows {
            for &(c, x, cw) in &qcols {
                if covered(r, c).is_some() {
                    continue;
                }
                let s = sheet.effective_style(r, c);
                if s == 0 {
                    continue;
                }
                if let Some(color) = styles.fill(styles.xf(s).fill) {
                    fill_rect(&mut dl, Rect::new(x, y, cw, rh), color);
                }
            }
        }

        // Merged areas: one fill (white when none, hiding the gridlines inside).
        let qmerges: Vec<CellRange> = merges.iter().copied().filter(|m| m.intersects(&qrange)).collect();
        let merge_rect = |m: &CellRange| -> Rect {
            let mx1 = cols.pos(&col_size, m.c1);
            let mx2 = cols.pos(&col_size, m.c2 + 1);
            let my1 = rows.pos(&row_size, m.r1);
            let my2 = rows.pos(&row_size, m.r2 + 1);
            Rect::from_ltrb(mx1, my1, mx2, my2)
        };
        for m in &qmerges {
            let s = sheet.effective_style(m.r1, m.c1);
            let color = styles.fill(styles.xf(s).fill).unwrap_or(Color::WHITE);
            fill_rect(&mut dl, merge_rect(m), color);
        }

        // Borders.
        for &(r, y, rh) in &qrows {
            for &(c, x, cw) in &qcols {
                let s = sheet.effective_style(r, c);
                if s == 0 {
                    continue;
                }
                let Some(b) = styles.border(styles.xf(s).border) else { continue };
                let m = covered(r, c);
                let on_edge = |left: bool, right: bool, top: bool, bottom: bool| match m {
                    None => true,
                    Some(m) => (left && c == m.c1) || (right && c == m.c2) || (top && r == m.r1) || (bottom && r == m.r2),
                };
                if let Some(sd) = &b.left {
                    if on_edge(true, false, false, false) {
                        border_lines(&mut dl, sd, x, y, x, y + rh);
                    }
                }
                if let Some(sd) = &b.right {
                    if on_edge(false, true, false, false) {
                        border_lines(&mut dl, sd, x + cw, y, x + cw, y + rh);
                    }
                }
                if let Some(sd) = &b.top {
                    if on_edge(false, false, true, false) {
                        border_lines(&mut dl, sd, x, y, x + cw, y);
                    }
                }
                if let Some(sd) = &b.bottom {
                    if on_edge(false, false, false, true) {
                        border_lines(&mut dl, sd, x, y + rh, x + cw, y + rh);
                    }
                }
            }
        }

        // Text of ordinary cells.
        let is_blank = |r: u32, c: u32| -> bool {
            covered(r, c).is_none() && sheet.cell(r, c).map_or(true, |cell| cell.value.is_empty())
        };
        for &(r, y, rh) in &qrows {
            let Some(row) = sheet.rows.get(&r) else { continue };
            for (&c, cell) in row.cells.range(c_lo..=c_hi) {
                let Some(&(x, cw)) = col_at.get(&c) else { continue };
                if covered(r, c).is_some() {
                    continue;
                }
                let Some(ct) = cell_text(cell, styles, date1904) else { continue };
                let rect = Rect::new(x, y, cw, rh);
                let mut span = (x, x + cw);
                if !ct.wrap && ct.number.is_none() && ct.fill.is_none() {
                    let need = fonts.measure(&ct.text.replace(['\r', '\n'], " "), &ct.style) + 2.0 * PAD + ct.indent;
                    if need > cw {
                        let pos = qcols.iter().position(|t| t.0 == c).unwrap_or(0);
                        let (grow_right, grow_left) = match ct.h {
                            HAlign::Right => (0.0, need - cw),
                            HAlign::Center => ((need - cw) / 2.0, (need - cw) / 2.0),
                            _ => (need - cw, 0.0),
                        };
                        let mut k = pos + 1;
                        while span.1 < x + cw + grow_right && k < qcols.len() && is_blank(r, qcols[k].0) {
                            span.1 = qcols[k].1 + qcols[k].2;
                            k += 1;
                        }
                        let mut k = pos;
                        while span.0 > x - grow_left && k > 0 && is_blank(r, qcols[k - 1].0) {
                            k -= 1;
                            span.0 = qcols[k].1;
                        }
                    }
                }
                draw_text(&mut dl, fonts, &ct, rect, span);
            }
        }
        // Text of merged areas.
        for m in &qmerges {
            let Some(cell) = sheet.cell(m.r1, m.c1) else { continue };
            let Some(ct) = cell_text(cell, styles, date1904) else { continue };
            let rect = merge_rect(m);
            draw_text(&mut dl, fonts, &ct, rect, (rect.x, rect.right()));
        }
        dl.push(Item::PopClip);
    }

    // Headers.
    fill_rect(&mut dl, Rect::new(0.0, 0.0, w, HEADER_H), HEADER_BG);
    fill_rect(&mut dl, Rect::new(0.0, HEADER_H, header_w, h - HEADER_H), HEADER_BG);
    let mut lines = vec![(0.0, HEADER_H, w, HEADER_H), (header_w, 0.0, header_w, h)];
    for &(_, x, cw) in &cols.items {
        lines.push((x + cw, 0.0, x + cw, HEADER_H));
    }
    for &(_, y, rh) in &rows.items {
        lines.push((0.0, y + rh, header_w, y + rh));
    }
    stroke_lines(&mut dl, &lines, Stroke::new(0.75, HEADER_LINE));
    let base = styles.font(0);
    let hs = TextStyle { family: base.name.clone(), size_pt: 10.0, color: HEADER_TEXT, ..TextStyle::default() };
    let header_text = |dl: &mut DisplayList, fonts: &mut FontCache, text: &str, r: Rect| {
        let ct = CellText {
            text: text.to_string(),
            style: hs.clone(),
            h: HAlign::Center,
            v: VAlign::Center,
            wrap: false,
            indent: 0.0,
            number: None,
            general: false,
            fill: None,
            hashes: false,
        };
        draw_text(dl, fonts, &ct, r, (r.x, r.right()));
    };
    for &(c, x, cw) in &cols.items {
        header_text(&mut dl, fonts, &col_name(c), Rect::new(x, 0.0, cw, HEADER_H));
    }
    for &(r, y, rh) in &rows.items {
        header_text(&mut dl, fonts, &(r + 1).to_string(), Rect::new(0.0, y, header_w, rh));
    }

    // Frozen-pane lines.
    let mut freeze = Vec::new();
    if fr > 0 {
        freeze.push((0.0, rows.frozen_end, w, rows.frozen_end));
    }
    if fc > 0 {
        freeze.push((cols.frozen_end, 0.0, cols.frozen_end, h));
    }
    stroke_lines(&mut dl, &freeze, Stroke::new(1.0, FREEZE));
    dl
}
