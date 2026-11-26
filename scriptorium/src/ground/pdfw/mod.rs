//! Our own PDF writer (PDF 1.7, ISO 32000-1). Export and print for every editor go through it, and
//! the PDF editor saves with it.
//!
//! A [`PdfWriter`] takes one [`DisplayList`] per page (points, y down, origin top-left) and the
//! [`PageResources`] its ids refer to, and writes:
//!
//! - paths (`m l c h re`, quadratics raised to cubics), fills (nonzero; the display list has no
//!   fill-rule field), strokes (`w J j M d`), fill+stroke, DeviceRGB colours;
//! - alpha through ExtGState `ca`/`CA` (colour alpha times every `PushOpacity` around the item);
//! - linear and radial gradients as axial/radial shadings (Type 2/3) with stitched Type 2 functions
//!   for more than two stops; stops with differing alpha get a luminosity soft mask;
//! - clips (`W n`) and transforms (`cm`), nested with `q`/`Q`;
//! - images as XObjects: RGB + an SMask for alpha (FlateDecode), or the original JPEG bytes
//!   (DCTDecode) when the picture came from a JPEG;
//! - links as `/Link` annotations (`/URI`, or `/Dest` for `#page=N`);
//! - text as real text: Type0 fonts with Identity-H and the glyph ids themselves; TrueType faces as
//!   CIDFontType2 + a subset FontFile2 (`subset_truetype` keeps ids, so `/CIDToGIDMap /Identity`),
//!   CFF faces as CIDFontType0 + the whole face as FontFile3/OpenType (CFF subsetting is still a TODO
//!   in `font/`); `/W` widths, a FontDescriptor, and a ToUnicode CMap so text copies and searches.
//!   Glyphs are placed with `Tm` per run and `TJ` adjustments so they land exactly where the display
//!   list put them.
//!
//! The file has a classic cross-reference table, a trailer `/ID` (MD5, ours), `/Info`, the catalog
//! with its page tree and outlines. Every stream is FlateDecode (JPEG passthrough aside). The output is
//! deterministic for the same input once a creation date is given ([`PdfWriter::set_date`]); without
//! one, the clock's time is used.

mod content;
mod embed;
mod image;
mod md5;
mod obj;
#[cfg(test)]
mod reader;
#[cfg(test)]
mod tests;

pub use image::PdfImage;
pub use md5::{md5, Md5};

use std::collections::HashMap;
use std::sync::Arc;

use super::display::{DisplayList, ImageRes};
use super::flate::zlib_compress;
use super::font::Font;
use super::picture::jpeg::Image;
use embed::FontEntry;
use obj::{nums, text_string};

/// What a page's display list refers to by id: fonts (display font ids) and images (display image
/// ids). Cheap to clone: fonts and images are shared. Give the same `Arc` to every page that uses a
/// font or a picture and the PDF holds it once.
#[derive(Clone, Default)]
pub struct PageResources {
    fonts: HashMap<u32, Arc<Font>>,
    images: HashMap<u32, Arc<PdfImage>>,
    /// Text for glyphs the font's cmap cannot name (ligatures, alternates), by (font id, glyph id).
    glyph_text: HashMap<(u32, u16), String>,
}

impl PageResources {
    pub fn new() -> PageResources {
        PageResources::default()
    }

    /// The face behind display font id `id`.
    pub fn add_font(&mut self, id: u32, font: Arc<Font>) {
        self.fonts.insert(id, font);
    }

    /// What a glyph of display font `font` stands for in copied or searched text, when the font's
    /// cmap does not say (a ligature "fi", a stylistic alternate). Wins over the cmap.
    pub fn set_glyph_text(&mut self, font: u32, gid: u16, text: &str) {
        self.glyph_text.insert((font, gid), text.to_string());
    }

    /// An image for display image id `id` (RGBA pixels or JPEG bytes).
    pub fn add_image(&mut self, id: u32, image: Arc<PdfImage>) {
        self.images.insert(id, image);
    }

    /// Decoded RGBA pixels for image id `id`.
    pub fn add_rgba(&mut self, id: u32, image: Image) {
        self.images.insert(id, Arc::new(PdfImage::Rgba(image)));
    }

    /// The display resources' own image (its id is the display id).
    pub fn add_image_res(&mut self, image: &ImageRes) {
        let img = Image { width: image.width, height: image.height, pixels: image.rgba.clone() };
        self.images.insert(image.id, Arc::new(PdfImage::Rgba(img)));
    }

    /// The original bytes of a JPEG, embedded as they are (DCTDecode). `false` when the bytes are not
    /// a JPEG we can embed (nothing is added then).
    pub fn add_jpeg(&mut self, id: u32, bytes: Vec<u8>) -> bool {
        match PdfImage::from_jpeg(bytes) {
            Some(img) => {
                self.images.insert(id, Arc::new(img));
                true
            }
            None => false,
        }
    }

    pub fn font(&self, id: u32) -> Option<&Arc<Font>> {
        self.fonts.get(&id)
    }

    pub fn image(&self, id: u32) -> Option<&Arc<PdfImage>> {
        self.images.get(&id)
    }
}

/// The document information dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocInfo {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    pub creator: String,
    pub producer: String,
}

impl Default for DocInfo {
    fn default() -> Self {
        DocInfo {
            title: String::new(),
            author: String::new(),
            subject: String::new(),
            keywords: String::new(),
            creator: "Scriptorium".to_string(),
            producer: "Scriptorium PDF writer".to_string(),
        }
    }
}

/// Where a link goes, once read from its display target.
#[derive(Debug, Clone, PartialEq)]
enum LinkTarget {
    Uri(String),
    /// Page index (0-based) and an optional y (display points from the top).
    Page(usize, Option<f64>),
}

#[derive(Debug, Clone)]
struct LinkRec {
    /// In PDF page space: left, bottom, right, top.
    rect: [f64; 4],
    target: LinkTarget,
}

struct PageRec {
    id: u32,
    width: f64,
    height: f64,
    contents: u32,
    resources: String,
    links: Vec<LinkRec>,
}

struct OutlineEntry {
    title: String,
    page: usize,
    y: f64,
    level: usize,
}

/// Builds one PDF file, page by page. See the module docs for what it writes.
pub struct PdfWriter {
    /// Object bodies by number - 1 (`None` = reserved, not yet written).
    objs: Vec<Option<Vec<u8>>>,
    /// Bodies written once and shared (graphics states, shadings), by their bytes.
    interned: HashMap<Vec<u8>, u32>,
    pages: Vec<PageRec>,
    fonts: Vec<FontEntry>,
    images: Vec<(Arc<PdfImage>, Option<u32>)>,
    outlines: Vec<OutlineEntry>,
    info: DocInfo,
    date: Option<i64>,
    lang: Option<String>,
}

const CATALOG: u32 = 1;
const PAGES: u32 = 2;

impl Default for PdfWriter {
    fn default() -> Self {
        PdfWriter::new()
    }
}

impl PdfWriter {
    pub fn new() -> PdfWriter {
        PdfWriter {
            objs: vec![None, None],
            interned: HashMap::new(),
            pages: Vec::new(),
            fonts: Vec::new(),
            images: Vec::new(),
            outlines: Vec::new(),
            info: DocInfo::default(),
            date: None,
            lang: None,
        }
    }

    /// The document's title, author, subject and keywords, and the creating application and PDF
    /// producer (an empty `creator` means "Scriptorium", an empty `producer` our writer's name).
    pub fn set_info(&mut self, title: &str, author: &str, subject: &str, keywords: &str, creator: &str, producer: &str) {
        let d = DocInfo::default();
        self.info = DocInfo {
            title: title.to_string(),
            author: author.to_string(),
            subject: subject.to_string(),
            keywords: keywords.to_string(),
            creator: if creator.is_empty() { d.creator } else { creator.to_string() },
            producer: if producer.is_empty() { d.producer } else { producer.to_string() },
        };
    }

    /// The whole information dictionary at once.
    pub fn set_doc_info(&mut self, info: DocInfo) {
        self.info = info;
    }

    /// Creation (and modification) date as seconds since the Unix epoch, UTC. With it the output is
    /// byte-for-byte the same for the same input; without it the clock is read at `finish`.
    pub fn set_date(&mut self, unix_seconds: i64) {
        self.date = Some(unix_seconds);
    }

    /// The document's natural language (`/Lang`, e.g. "en-US").
    pub fn set_lang(&mut self, lang: &str) {
        self.lang = if lang.is_empty() { None } else { Some(lang.to_string()) };
    }

    /// How many pages have been added.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Add a page drawn by `list` (its width and height are the page size in points; a size that is
    /// not positive becomes US Letter). Fonts and images are taken from `res` by id; an item whose id
    /// is missing there is left out. Returns the page's index (0-based).
    pub fn add_page(&mut self, list: &DisplayList, res: &PageResources) -> usize {
        let width = if list.width.is_finite() && list.width > 0.0 { list.width } else { 612.0 };
        let height = if list.height.is_finite() && list.height > 0.0 { list.height } else { 792.0 };
        let id = self.alloc();
        let (content, resources, links) = content::page_content(self, list, res, height);
        let contents = self.alloc();
        self.put_stream(contents, "", content.as_bytes(), true);
        self.pages.push(PageRec { id, width, height, contents, resources, links });
        self.pages.len() - 1
    }

    /// A top-level bookmark to page `page` (0-based) at `y` display points from the page's top.
    pub fn add_outline(&mut self, title: &str, page: usize, y: f64) {
        self.add_outline_level(title, page, y, 0);
    }

    /// A bookmark at nesting `level` (0 top level; an entry nests under the nearest earlier entry of
    /// a lower level).
    pub fn add_outline_level(&mut self, title: &str, page: usize, y: f64, level: usize) {
        self.outlines.push(OutlineEntry { title: title.to_string(), page, y, level });
    }

    /// The finished file.
    pub fn finish(mut self) -> Vec<u8> {
        if self.pages.is_empty() {
            self.add_page(&DisplayList::new(612.0, 792.0), &PageResources::new());
        }
        let fonts = std::mem::take(&mut self.fonts);
        for f in &fonts {
            embed::write_font(&mut self, f);
        }
        let page_ids: Vec<u32> = self.pages.iter().map(|p| p.id).collect();
        let page_heights: Vec<f64> = self.pages.iter().map(|p| p.height).collect();

        // Pages, with their link annotations.
        let pages = std::mem::take(&mut self.pages);
        for p in &pages {
            let mut annots = Vec::new();
            for l in &p.links {
                let action = match &l.target {
                    LinkTarget::Uri(u) => format!("/A << /S /URI /URI {} >>", obj::byte_string(u.as_bytes())),
                    LinkTarget::Page(i, y) => {
                        let i = (*i).min(page_ids.len() - 1);
                        let top = match y {
                            Some(y) => obj::num(page_heights[i] - y),
                            None => "null".to_string(),
                        };
                        format!("/Dest [{} 0 R /XYZ null {} null]", page_ids[i], top)
                    }
                };
                let a = self.alloc();
                let body = format!("<< /Type /Annot /Subtype /Link /Rect [{}] /Border [0 0 0] {} >>", nums(&l.rect), action);
                self.put(a, body.into_bytes());
                annots.push(a);
            }
            let mut d = format!(
                "<< /Type /Page /Parent {PAGES} 0 R /MediaBox [0 0 {}] /Resources {} /Contents {} 0 R",
                nums(&[p.width, p.height]),
                p.resources,
                p.contents
            );
            if !annots.is_empty() {
                d.push_str(" /Annots [");
                d.push_str(&refs(&annots));
                d.push(']');
            }
            d.push_str(" >>");
            self.put(p.id, d.into_bytes());
        }
        self.put(PAGES, format!("<< /Type /Pages /Kids [{}] /Count {} >>", refs(&page_ids), page_ids.len()).into_bytes());

        let outlines = self.write_outlines(&page_ids, &page_heights);

        let mut cat = format!("<< /Type /Catalog /Pages {PAGES} 0 R");
        if let Some(o) = outlines {
            cat.push_str(&format!(" /Outlines {o} 0 R /PageMode /UseOutlines"));
        }
        if let Some(l) = &self.lang {
            cat.push_str(&format!(" /Lang {}", text_string(l)));
        }
        cat.push_str(" >>");
        self.put(CATALOG, cat.into_bytes());

        let date = obj::pdf_date(self.date.unwrap_or_else(|| {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
        }));
        let mut info = String::from("<<");
        for (k, v) in [
            ("Title", &self.info.title),
            ("Author", &self.info.author),
            ("Subject", &self.info.subject),
            ("Keywords", &self.info.keywords),
            ("Creator", &self.info.creator),
            ("Producer", &self.info.producer),
        ] {
            if !v.is_empty() {
                info.push_str(&format!(" /{k} {}", text_string(v)));
            }
        }
        info.push_str(&format!(" /CreationDate ({date}) /ModDate ({date}) >>"));
        let info_id = self.alloc();
        self.put(info_id, info.into_bytes());

        self.serialize(info_id)
    }

    // -----------------------------------------------------------------------------------------------
    // Objects
    // -----------------------------------------------------------------------------------------------

    fn alloc(&mut self) -> u32 {
        self.objs.push(None);
        self.objs.len() as u32
    }

    fn put(&mut self, id: u32, body: Vec<u8>) {
        if let Some(slot) = self.objs.get_mut(id as usize - 1) {
            *slot = Some(body);
        }
    }

    /// A stream object: `dict` holds the entries besides `/Length` (and `/Filter` when `flate`).
    fn put_stream(&mut self, id: u32, dict: &str, data: &[u8], flate: bool) {
        let body = stream_body(dict, data, flate);
        self.put(id, body);
    }

    /// A new stream object.
    fn new_stream(&mut self, dict: &str, data: &[u8], flate: bool) -> u32 {
        let id = self.alloc();
        self.put_stream(id, dict, data, flate);
        id
    }

    /// An object shared by everything that writes the same bytes.
    fn intern(&mut self, body: Vec<u8>) -> u32 {
        if let Some(&id) = self.interned.get(&body) {
            return id;
        }
        let id = self.alloc();
        self.put(id, body.clone());
        self.interned.insert(body, id);
        id
    }

    /// The font's entry (created on first use), by the identity of its `Arc`.
    fn font_entry(&mut self, font: &Arc<Font>) -> usize {
        if let Some(i) = self.fonts.iter().position(|f| Arc::ptr_eq(&f.font, font)) {
            return i;
        }
        let id = self.alloc();
        self.fonts.push(FontEntry::new(font.clone(), id));
        self.fonts.len() - 1
    }

    /// The image's XObject (written on first use), or `None` for an empty picture.
    fn image_obj(&mut self, img: &Arc<PdfImage>) -> Option<u32> {
        if let Some((_, id)) = self.images.iter().find(|(i, _)| Arc::ptr_eq(i, img)) {
            return *id;
        }
        let id = image::write_image(self, img);
        self.images.push((img.clone(), id));
        id
    }

    fn write_outlines(&mut self, page_ids: &[u32], heights: &[f64]) -> Option<u32> {
        if self.outlines.is_empty() {
            return None;
        }
        let root = self.alloc();
        let n = self.outlines.len();
        let ids: Vec<u32> = (0..n).map(|_| self.alloc()).collect();
        // Parent of each entry (None = the root), from the levels.
        let mut parent: Vec<Option<usize>> = vec![None; n];
        let mut stack: Vec<usize> = Vec::new();
        for i in 0..n {
            while let Some(&top) = stack.last() {
                if self.outlines[top].level < self.outlines[i].level {
                    break;
                }
                stack.pop();
            }
            parent[i] = stack.last().copied();
            stack.push(i);
        }
        let children = |p: Option<usize>| -> Vec<usize> { (0..n).filter(|&i| parent[i] == p).collect() };
        // Every entry is open, so a count is all its descendants.
        let mut desc = vec![0usize; n];
        for i in (0..n).rev() {
            if let Some(p) = parent[i] {
                desc[p] += 1 + desc[i];
            }
        }
        for i in 0..n {
            let e = &self.outlines[i];
            let siblings = children(parent[i]);
            let pos = siblings.iter().position(|&s| s == i).unwrap_or(0);
            let page = e.page.min(page_ids.len() - 1);
            let mut d = format!(
                "<< /Title {} /Parent {} 0 R /Dest [{} 0 R /XYZ 0 {} null]",
                text_string(&e.title),
                parent[i].map_or(root, |p| ids[p]),
                page_ids[page],
                obj::num(heights[page] - e.y)
            );
            if pos > 0 {
                d.push_str(&format!(" /Prev {} 0 R", ids[siblings[pos - 1]]));
            }
            if pos + 1 < siblings.len() {
                d.push_str(&format!(" /Next {} 0 R", ids[siblings[pos + 1]]));
            }
            let kids = children(Some(i));
            if let (Some(f), Some(l)) = (kids.first(), kids.last()) {
                d.push_str(&format!(" /First {} 0 R /Last {} 0 R /Count {}", ids[*f], ids[*l], desc[i]));
            }
            d.push_str(" >>");
            self.put(ids[i], d.into_bytes());
        }
        let top = children(None);
        let body = format!("<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {} >>", ids[top[0]], ids[*top.last().unwrap_or(&top[0])], n);
        self.put(root, body.into_bytes());
        Some(root)
    }

    fn serialize(&self, info_id: u32) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::with_capacity(self.objs.len());
        for (i, body) in self.objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            match body {
                Some(b) => out.extend_from_slice(b),
                None => out.extend_from_slice(b"null"),
            }
            out.extend_from_slice(b"\nendobj\n");
        }
        let id = obj::hex(&md5(&out));
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", self.objs.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f\r\n");
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root {CATALOG} 0 R /Info {info_id} 0 R /ID [<{id}> <{id}>] >>\nstartxref\n{xref}\n%%EOF\n",
                self.objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}

fn refs(ids: &[u32]) -> String {
    ids.iter().map(|i| format!("{i} 0 R")).collect::<Vec<_>>().join(" ")
}

fn stream_body(dict: &str, data: &[u8], flate: bool) -> Vec<u8> {
    let packed;
    let (bytes, filter) = if flate {
        packed = zlib_compress(data, 6);
        (&packed[..], " /Filter /FlateDecode")
    } else {
        (data, "")
    };
    let sep = if dict.is_empty() { "" } else { " " };
    let mut body = format!("<<{sep}{dict}{filter} /Length {} >>\nstream\n", bytes.len()).into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\nendstream");
    body
}
