//! Shaping-lite and the font cache: a run of text in a style becomes glyphs of one or more fonts.
//!
//! Per grapheme cluster: pick the first font that has every visible character of the cluster (the
//! style's own font, then the fallback used last, then a fresh fallback from the font index),
//! look each character up in its cmap, take advances from `hmtx` scaled to points, add pair kerning
//! between clusters of the same font and the style's letter spacing after each cluster. Consecutive
//! clusters of one font form one [`ShapedRun`].
//!
//! EXTENSION (complex shaping): Arabic/Jawi joining forms, Indic reordering and ligatures need
//! GSUB/GPOS lookups applied to each same-font run where `shape` builds it; the run's clusters
//! already carry the byte ranges that shaping would keep.

use super::segment::graphemes;
use super::tables::{grapheme_break, is_default_ignorable, GraphemeBreak};
use super::{FontMetrics, Measurer, ShapedGlyph, ShapedRun, TextStyle};
use crate::ground::display::{self, Item, Resources};
use crate::ground::font::{Font, FontIndex, FontRef, PathOp};
use std::collections::HashMap;
use std::sync::Arc;

struct Cached {
    font: Arc<Font>,
    font_ref: Option<FontRef>,
}

/// Owns the fonts a document uses and gives each a small id (1, 2, 3...) for the display list.
/// Resolves a style to its font, finds fallbacks, shapes text and fills display resources.
pub struct FontCache {
    index: Arc<FontIndex>,
    fonts: Vec<Cached>,
    by_ref: HashMap<FontRef, u32>,
    /// Fonts registered by hand (embedded in a document, or built in tests): lowercased family.
    registered: Vec<(String, u32)>,
    primary: HashMap<(String, u16, bool), Option<u32>>,
    fallback: HashMap<(char, String, u16, bool), Option<u32>>,
}

impl std::fmt::Debug for FontCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontCache").field("fonts", &self.fonts.len()).field("index", &self.index).finish()
    }
}

/// Families preferred for Han characters when the text around them is Japanese or Korean.
const JAPANESE: &[&str] = &["Yu Gothic", "Meiryo", "MS Gothic", "Noto Sans CJK JP", "Noto Sans JP", "Hiragino Sans"];
const KOREAN: &[&str] = &["Malgun Gothic", "Gulim", "Noto Sans CJK KR", "Noto Sans KR", "Apple SD Gothic Neo"];

fn script_hint(text: &str) -> &'static [&'static str] {
    let mut korean = false;
    for c in text.chars() {
        let cp = c as u32;
        if (0x3040..=0x30FF).contains(&cp) || (0x31F0..=0x31FF).contains(&cp) || (0xFF65..=0xFF9F).contains(&cp) {
            return JAPANESE;
        }
        if (0xAC00..=0xD7A3).contains(&cp) || (0x1100..=0x11FF).contains(&cp) || (0x3130..=0x318F).contains(&cp) {
            korean = true;
        }
    }
    if korean {
        KOREAN
    } else {
        &[]
    }
}

impl FontCache {
    /// A cache over a font index (share one index across documents: scanning is the slow part).
    pub fn new(index: Arc<FontIndex>) -> FontCache {
        FontCache {
            index,
            fonts: Vec::new(),
            by_ref: HashMap::new(),
            registered: Vec::new(),
            primary: HashMap::new(),
            fallback: HashMap::new(),
        }
    }

    /// A cache over this machine's installed fonts.
    pub fn system() -> FontCache {
        FontCache::new(Arc::new(FontIndex::system()))
    }

    /// A cache with no fonts at all (only [`FontCache::register`]ed ones).
    pub fn empty() -> FontCache {
        FontCache::new(Arc::new(FontIndex::new()))
    }

    pub fn index(&self) -> &FontIndex {
        &self.index
    }

    /// Add a font by hand under a family name (a document's embedded font, a test font). It wins
    /// over installed fonts of the same family. Returns its id.
    pub fn register(&mut self, family: &str, font: Font) -> u32 {
        self.fonts.push(Cached { font: Arc::new(font), font_ref: None });
        let id = self.fonts.len() as u32;
        self.registered.push((family.trim().to_lowercase(), id));
        self.primary.clear();
        self.fallback.clear();
        id
    }

    /// The font with this id.
    pub fn font(&self, id: u32) -> Option<&Arc<Font>> {
        self.fonts.get((id as usize).checked_sub(1)?).map(|c| &c.font)
    }

    /// Where the font with this id lives (None for registered fonts).
    pub fn font_ref(&self, id: u32) -> Option<&FontRef> {
        self.fonts.get((id as usize).checked_sub(1)?).and_then(|c| c.font_ref.as_ref())
    }

    /// How many fonts are loaded.
    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }

    /// The id of an indexed face, loading it on first use.
    pub fn id_for(&mut self, r: &FontRef) -> Option<u32> {
        if let Some(&id) = self.by_ref.get(r) {
            return Some(id);
        }
        let font = self.index.load(r)?;
        self.fonts.push(Cached { font, font_ref: Some(r.clone()) });
        let id = self.fonts.len() as u32;
        self.by_ref.insert(r.clone(), id);
        Some(id)
    }

    /// The style's own font (family + nearest weight and slant), if it is installed or registered.
    pub fn resolve(&mut self, style: &TextStyle) -> Option<u32> {
        let key = (style.family.trim().to_lowercase(), style.weight, style.italic);
        if let Some(v) = self.primary.get(&key) {
            return *v;
        }
        let v = self.resolve_uncached(&key.0, style.weight, style.italic);
        self.primary.insert(key, v);
        v
    }

    fn resolve_uncached(&mut self, family: &str, weight: u16, italic: bool) -> Option<u32> {
        if family.is_empty() {
            return None;
        }
        let mut best: Option<(u32, i64)> = None;
        for (fam, id) in &self.registered {
            if fam != family {
                continue;
            }
            let st = self.fonts[*id as usize - 1].font.style();
            let score = if st.italic != italic { 10_000 } else { 0 } + (st.weight as i64 - weight as i64).abs();
            if best.map_or(true, |(_, s)| score < s) {
                best = Some((*id, score));
            }
        }
        if let Some((id, _)) = best {
            return Some(id);
        }
        let r = self.index.find(family, weight, italic)?;
        self.id_for(&r)
    }

    /// A font covering `c` for this style: registered fonts of the style's families first, then
    /// the index's fallback (preferred families, then script defaults, then any face).
    fn fallback_font(&mut self, c: char, style: &TextStyle, hint: &[&str]) -> Option<u32> {
        let fam_key = format!("{}|{}|{}", style.east_asian_family.to_lowercase(), style.family.to_lowercase(), hint.join(","));
        let key = (c, fam_key, style.weight, style.italic);
        if let Some(v) = self.fallback.get(&key) {
            return *v;
        }
        let mut prefs: Vec<&str> = Vec::new();
        if !style.east_asian_family.trim().is_empty() {
            prefs.push(style.east_asian_family.trim());
        }
        if !style.family.trim().is_empty() {
            prefs.push(style.family.trim());
        }
        prefs.extend_from_slice(hint);
        let mut found = None;
        for (fam, id) in &self.registered {
            if self.fonts[*id as usize - 1].font.has_char(c) && (prefs.iter().any(|p| p.to_lowercase() == *fam) || prefs.is_empty()) {
                found = Some(*id);
                break;
            }
        }
        if found.is_none() {
            if let Some(r) = self.index.fallback_for_style(c, &prefs, style.weight, style.italic) {
                found = self.id_for(&r);
            }
        }
        if found.is_none() {
            // Last resort: any registered font with the character.
            found = self.registered.iter().map(|(_, id)| *id).find(|id| self.fonts[*id as usize - 1].font.has_char(c));
        }
        self.fallback.insert(key, found);
        found
    }

    fn covers_all(&self, id: u32, cluster: &str) -> bool {
        let f = &self.fonts[id as usize - 1].font;
        cluster.chars().filter(|&c| !is_default_ignorable(c)).all(|c| f.has_char(c))
    }

    fn has(&self, id: u32, c: char) -> bool {
        self.fonts[id as usize - 1].font.has_char(c)
    }

    fn pick(&mut self, cluster: &str, base: char, primary: Option<u32>, sticky: &mut Option<u32>, style: &TextStyle, hint: &[&str]) -> Option<u32> {
        if let Some(p) = primary {
            if self.covers_all(p, cluster) {
                return Some(p);
            }
        }
        if let Some(s) = *sticky {
            if Some(s) != primary && self.covers_all(s, cluster) {
                return Some(s);
            }
        }
        let fb = self.fallback_font(base, style, hint);
        if let Some(f) = fb {
            if self.covers_all(f, cluster) {
                *sticky = Some(f);
                return Some(f);
            }
        }
        for id in [primary, *sticky, fb].into_iter().flatten() {
            if self.has(id, base) {
                return Some(id);
            }
        }
        primary.or(fb).or(*sticky)
    }

    /// Metrics of a font id at a size (synthetic for id 0 or an unknown id).
    pub fn metrics_for(&self, id: u32, size: f64) -> FontMetrics {
        let Some(f) = self.font(id) else { return FontMetrics::synthetic(size) };
        let m = f.metrics();
        let s = size / m.units_per_em.max(1) as f64;
        FontMetrics {
            ascent: m.ascender as f64 * s,
            descent: -(m.descender as f64) * s,
            line_gap: (m.line_gap.max(0)) as f64 * s,
            underline_y: -(m.underline_pos as f64) * s,
            underline_thickness: m.underline_thickness.max(1) as f64 * s,
            strike_y: -(m.strikeout_pos as f64) * s,
            strike_thickness: m.strikeout_thickness.max(1) as f64 * s,
        }
    }

    fn new_run(&self, id: u32, size: f64) -> ShapedRun {
        ShapedRun {
            font: id,
            font_ref: self.font_ref(id).cloned(),
            size,
            glyphs: Vec::new(),
            width: 0.0,
            metrics: self.metrics_for(id, size),
        }
    }

    /// Shape a run of text (see [`Measurer::shape`]).
    pub fn shape_text(&mut self, text: &str, style: &TextStyle) -> Vec<ShapedRun> {
        let size = style.size_pt;
        let primary = self.resolve(style);
        let hint = script_hint(text);
        let mut runs: Vec<ShapedRun> = Vec::new();
        let mut cur: Option<ShapedRun> = None;
        let mut sticky: Option<u32> = None;
        let mut prev_gid: Option<u16> = None;
        for g in graphemes(text) {
            let cluster = &text[g.clone()];
            // A cluster of only invisible characters (a lone soft hyphen, ZWJ) gets no glyph.
            let Some(base) = cluster.chars().find(|&c| !is_default_ignorable(c)) else { continue };
            let id = self.pick(cluster, base, primary, &mut sticky, style, hint).unwrap_or(0);
            if cur.as_ref().map_or(true, |r| r.font != id) {
                if let Some(r) = cur.take() {
                    runs.push(r);
                }
                cur = Some(self.new_run(id, size));
                prev_gid = None;
            }
            let font = self.font(id).cloned();
            let run = cur.as_mut().expect("run just set");
            let scale = font.as_ref().map_or(0.0, |f| size / f.units_per_em().max(1) as f64);
            let mut first = true;
            let mut first_glyph = true;
            let mut last: Option<usize> = None;
            for c in cluster.chars() {
                if is_default_ignorable(c) {
                    continue;
                }
                let (gid, adv) = match &font {
                    Some(f) => {
                        let gid = f.glyph_id(c).unwrap_or(0);
                        (gid, f.advance(gid) as f64 * scale)
                    }
                    None => (0, size * 0.5),
                };
                if first {
                    if let (Some(p), Some(f)) = (prev_gid, &font) {
                        let k = f.kerning(p, gid) as f64 * scale;
                        if k != 0.0 {
                            if let Some(lg) = run.glyphs.last_mut() {
                                lg.advance += k;
                                run.width += k;
                            }
                        }
                    }
                    first = false;
                }
                let mut adv = adv;
                if !first_glyph && adv > 0.0 && grapheme_break(c) == GraphemeBreak::Extend {
                    // A combining mark the font made spacing: without GPOS mark positioning, stack it
                    // on the base's origin (the base gives its advance to the mark) rather than
                    // after it.
                    if let Some(prev) = run.glyphs.last_mut() {
                        adv = prev.advance;
                        prev.advance = 0.0;
                        run.width -= adv;
                    }
                }
                first_glyph = false;
                run.glyphs.push(ShapedGlyph { gid, advance: adv, cluster: g.clone() });
                run.width += adv;
                last = Some(run.glyphs.len() - 1);
            }
            if let Some(li) = last {
                run.glyphs[li].advance += style.letter_spacing;
                run.width += style.letter_spacing;
                prev_gid = Some(run.glyphs[li].gid);
            }
        }
        if let Some(r) = cur.take() {
            runs.push(r);
        }
        runs
    }

    /// The style's primary font metrics (a fallback face's, or synthetic, when it is missing).
    pub fn style_metrics(&mut self, style: &TextStyle) -> FontMetrics {
        let id = match self.resolve(style) {
            Some(id) => Some(id),
            None => self.fallback_font('a', style, &[]),
        };
        match id {
            Some(id) => self.metrics_for(id, style.size_pt),
            None => FontMetrics::synthetic(style.size_pt),
        }
    }

    /// Put into `res` the outline of every glyph the items draw (only those not already there).
    /// Outlines stay in font units with y up, as [`display::FontRes`] expects.
    pub fn fill_resources(&self, res: &mut Resources, items: &[Item]) {
        for item in items {
            if let Item::Glyphs { font, glyphs, .. } = item {
                self.add_glyphs(res, *font, glyphs.iter().map(|g| g.gid));
            }
        }
    }

    /// Put the outlines of these glyphs of font `id` into `res`.
    pub fn add_glyphs(&self, res: &mut Resources, id: u32, gids: impl IntoIterator<Item = u16>) {
        let Some(f) = self.font(id) else { return };
        let fr = res.add_font(id, f.units_per_em() as f64);
        for gid in gids {
            if fr.glyphs.contains_key(&gid) {
                continue;
            }
            let ops = f.outline(gid).unwrap_or_default();
            fr.glyphs.insert(gid, ops.iter().map(convert).collect());
        }
    }
}

fn convert(op: &PathOp) -> display::PathOp {
    match *op {
        PathOp::MoveTo(x, y) => display::PathOp::MoveTo(x as f64, y as f64),
        PathOp::LineTo(x, y) => display::PathOp::LineTo(x as f64, y as f64),
        PathOp::QuadTo(a, b, x, y) => display::PathOp::QuadTo(a as f64, b as f64, x as f64, y as f64),
        PathOp::CubicTo(a, b, c, d, x, y) => {
            display::PathOp::CubicTo(a as f64, b as f64, c as f64, d as f64, x as f64, y as f64)
        }
        PathOp::Close => display::PathOp::Close,
    }
}

impl Measurer for FontCache {
    fn shape(&mut self, text: &str, style: &TextStyle) -> Vec<ShapedRun> {
        self.shape_text(text, style)
    }

    fn font_metrics(&mut self, style: &TextStyle) -> FontMetrics {
        self.style_metrics(style)
    }
}
