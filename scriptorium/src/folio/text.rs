//! Text extraction in reading order: positioned glyphs become lines (by baseline), lines split into
//! words (by gap) and into column segments (by wide gaps), segments stack into blocks, and blocks are
//! ordered top to bottom within a column and column by column.

use super::render::TextGlyph;

/// A gap wider than this (in font sizes) separates words.
const WORD_GAP: f64 = 0.2;
/// A gap wider than this separates columns.
const COLUMN_GAP: f64 = 1.6;

#[derive(Debug, Clone)]
struct Seg {
    text: String,
    x0: f64,
    x1: f64,
    y: f64,
    size: f64,
}

#[derive(Debug, Clone)]
struct Block {
    lines: Vec<Seg>,
    x0: f64,
    x1: f64,
    top: f64,
    bottom: f64,
}

/// The page's text in reading order: words joined by spaces, lines by `\n`, blocks by a blank line.
pub fn reading_order(glyphs: &[TextGlyph]) -> String {
    let blocks = blocks(glyphs);
    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        for (j, l) in b.lines.iter().enumerate() {
            if j > 0 {
                out.push('\n');
            }
            out.push_str(&l.text);
        }
    }
    out
}

fn blocks(glyphs: &[TextGlyph]) -> Vec<Block> {
    // Mostly-horizontal glyphs go through the layout analysis; the rest follow in stream order.
    let mut horiz: Vec<&TextGlyph> = Vec::new();
    let mut other: Vec<&TextGlyph> = Vec::new();
    for g in glyphs {
        if !g.x.is_finite() || !g.y.is_finite() || g.text.is_empty() {
            continue;
        }
        if g.dx > 0.9 {
            horiz.push(g);
        } else {
            other.push(g);
        }
    }
    // Lines: cluster by baseline.
    horiz.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal).then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal)));
    let mut lines: Vec<Vec<&TextGlyph>> = Vec::new();
    for g in horiz {
        let tol = (g.size * 0.4).max(1.0);
        match lines.last_mut() {
            Some(l) if (l[0].y - g.y).abs() <= tol => l.push(g),
            _ => lines.push(vec![g]),
        }
    }
    // Segments: words by small gaps, columns by large ones.
    let mut segs: Vec<Seg> = Vec::new();
    for mut l in lines {
        l.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        let mut cur: Option<Seg> = None;
        let mut last_end = f64::NEG_INFINITY;
        for g in l {
            let size = g.size.max(1.0);
            let is_space = g.text.trim().is_empty();
            let gap = g.x - last_end;
            let start_new = cur.is_none() || gap > COLUMN_GAP * size;
            if start_new {
                if let Some(s) = cur.take() {
                    segs.push(s);
                }
                last_end = g.x + g.w;
                if !is_space {
                    cur = Some(Seg { text: g.text.clone(), x0: g.x, x1: g.x + g.w, y: g.y, size });
                }
                continue;
            }
            let Some(s) = cur.as_mut() else { continue };
            // A glyph drawn twice on itself (fake bold) is kept once.
            if gap < -0.5 * size && s.text.ends_with(&g.text) && (g.x + g.w - s.x1).abs() < 0.3 * size {
                continue;
            }
            if is_space {
                if !s.text.ends_with(' ') {
                    s.text.push(' ');
                }
            } else {
                if gap > WORD_GAP * size && !s.text.ends_with(' ') {
                    s.text.push(' ');
                }
                s.text.push_str(&g.text);
                s.size = s.size.max(size);
            }
            s.x1 = s.x1.max(g.x + g.w);
            last_end = g.x + g.w;
        }
        if let Some(s) = cur {
            segs.push(s);
        }
    }
    for s in segs.iter_mut() {
        let t = s.text.trim_end().to_string();
        s.text = t;
    }
    segs.retain(|s| !s.text.is_empty());
    // Blocks: a segment joins the block whose last line sits just above it and overlaps it.
    let mut blocks: Vec<Block> = Vec::new();
    for s in segs {
        let mut joined = false;
        for b in blocks.iter_mut().rev() {
            let last = b.lines.last().map(|l| (l.y, l.size, l.x0, l.x1));
            if let Some((ly, lsize, lx0, lx1)) = last {
                let dy = s.y - ly;
                let overlap = s.x0.min(s.x1).max(lx0) <= s.x1.min(lx1) || (s.x0 - lx0).abs() < 2.0 * s.size;
                if dy > 0.0 && dy <= 2.0 * lsize.max(s.size) && overlap {
                    b.x0 = b.x0.min(s.x0);
                    b.x1 = b.x1.max(s.x1);
                    b.bottom = s.y;
                    b.lines.push(s.clone());
                    joined = true;
                    break;
                }
            }
        }
        if !joined {
            blocks.push(Block { x0: s.x0, x1: s.x1, top: s.y - s.size, bottom: s.y, lines: vec![s] });
        }
        if blocks.len() > 20_000 {
            break;
        }
    }
    let mut ordered = order_blocks(blocks);
    if !other.is_empty() {
        let mut text = String::new();
        for g in other {
            text.push_str(&g.text);
        }
        ordered.push(Block { lines: vec![Seg { text, x0: 0.0, x1: 0.0, y: 0.0, size: 1.0 }], x0: 0.0, x1: 0.0, top: 0.0, bottom: 0.0 });
    }
    ordered
}

/// Order blocks: A comes before B when A is above B and they share columns, or when A is wholly to
/// the left of B and starts above B's end. A topological sort with "top, then left" as tie-break.
fn order_blocks(blocks: Vec<Block>) -> Vec<Block> {
    let n = blocks.len();
    if n <= 1 {
        return blocks;
    }
    let before = |a: &Block, b: &Block| -> bool {
        let overlap = a.x0 < b.x1 && b.x0 < a.x1;
        if overlap {
            a.top < b.top
        } else {
            a.x1 <= b.x0 && a.top < b.bottom
        }
    };
    if n > 2000 {
        let mut b = blocks;
        b.sort_by(|a, c| a.top.partial_cmp(&c.top).unwrap_or(std::cmp::Ordering::Equal));
        return b;
    }
    let mut indeg = vec![0usize; n];
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in 0..n {
            if i != j && before(&blocks[i], &blocks[j]) && !before(&blocks[j], &blocks[i]) {
                edges[i].push(j);
                indeg[j] += 1;
            }
        }
    }
    let mut done = vec![false; n];
    let mut order = Vec::with_capacity(n);
    for _ in 0..n {
        // The ready block that is highest, then leftmost; on a cycle, the highest remaining.
        let pick = (0..n)
            .filter(|&i| !done[i] && indeg[i] == 0)
            .min_by(|&a, &b| key(&blocks[a]).partial_cmp(&key(&blocks[b])).unwrap_or(std::cmp::Ordering::Equal))
            .or_else(|| (0..n).filter(|&i| !done[i]).min_by(|&a, &b| key(&blocks[a]).partial_cmp(&key(&blocks[b])).unwrap_or(std::cmp::Ordering::Equal)));
        let Some(p) = pick else { break };
        done[p] = true;
        order.push(p);
        for &j in &edges[p] {
            indeg[j] = indeg[j].saturating_sub(1);
        }
    }
    let mut slots: Vec<Option<Block>> = blocks.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

fn key(b: &Block) -> (f64, f64) {
    (b.top, b.x0)
}
