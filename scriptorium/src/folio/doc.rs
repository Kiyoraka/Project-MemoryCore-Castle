//! The document structure: the catalog, the page tree with inherited attributes (MediaBox, CropBox,
//! Rotate, Resources), the info dictionary, outlines with their destination pages, annotations and
//! link targets.

use std::collections::{HashMap, HashSet};

use super::obj::{Dict, Obj};
use super::parse::PdfFile;
use crate::ground::display::Matrix;

/// The most pages a document may have (a guard against cyclic or absurd trees).
const MAX_PAGES: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct PageInfo {
    /// The page object's number and generation (0 when the page dictionary is direct).
    pub num: u32,
    pub gen: u16,
    pub dict: Dict,
    pub media: [f64; 4],
    pub crop: [f64; 4],
    /// 0, 90, 180 or 270 (clockwise).
    pub rotate: i32,
    pub resources: Dict,
}

impl PageInfo {
    /// The displayed size in points (rotation applied).
    pub fn size(&self) -> (f64, f64) {
        let w = self.crop[2] - self.crop[0];
        let h = self.crop[3] - self.crop[1];
        if self.rotate == 90 || self.rotate == 270 {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// PDF user space (y up, crop box) to display space (y down, origin top-left, rotated).
    pub fn base_matrix(&self) -> Matrix {
        let [x0, _y0, x1, y1] = self.crop;
        let w = x1 - x0;
        let h = self.crop[3] - self.crop[1];
        // Unrotated: u = x - x0, v = y1 - y.
        let flip = Matrix::new(1.0, 0.0, 0.0, -1.0, -x0, y1);
        let rot = match self.rotate {
            90 => Matrix::new(0.0, 1.0, -1.0, 0.0, h, 0.0),
            180 => Matrix::new(-1.0, 0.0, 0.0, -1.0, w, h),
            270 => Matrix::new(0.0, -1.0, 1.0, 0.0, 0.0, w),
            _ => Matrix::IDENTITY,
        };
        rot.multiply(&flip)
    }
}

fn rect_of(o: &Obj) -> Option<[f64; 4]> {
    let v = o.nums();
    if v.len() < 4 || v.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let r = [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])];
    if r[2] - r[0] < 1e-3 || r[3] - r[1] < 1e-3 {
        return None;
    }
    Some(r)
}

#[derive(Clone, Default)]
struct Inherit {
    resources: Option<Obj>,
    media: Option<Obj>,
    crop: Option<Obj>,
    rotate: Option<Obj>,
}

/// Every page, in order. When the tree is missing or broken, page objects found anywhere are used.
pub fn collect_pages(file: &PdfFile) -> Vec<PageInfo> {
    let mut out = Vec::new();
    let trailer = file.trailer();
    if let Some(root) = trailer.get("Root").map(|r| file.resolve(r)).and_then(|r| r.as_dict().cloned()) {
        if let Some(pages) = root.get("Pages") {
            let mut visited = HashSet::new();
            walk(file, pages, &Inherit::default(), &mut visited, 0, &mut out);
        }
    }
    if out.is_empty() {
        for n in file.object_numbers() {
            let o = file.get(n);
            if let Some(d) = o.as_dict() {
                if d.is_type("Page") {
                    let mut inh = Inherit::default();
                    // Climb the parents for inherited attributes.
                    let mut parent = d.get("Parent").cloned();
                    let mut chain = Vec::new();
                    for _ in 0..32 {
                        let Some(p) = parent.take() else { break };
                        let pd = file.resolve(&p);
                        let Some(pd) = pd.as_dict() else { break };
                        chain.push(pd.clone());
                        parent = pd.get("Parent").cloned();
                    }
                    for pd in chain.iter().rev() {
                        inh = inherit(pd, &inh);
                    }
                    out.push(make_page(file, n, file.generation(n), d, &inh));
                    if out.len() >= MAX_PAGES {
                        break;
                    }
                }
            }
        }
    }
    out
}

fn inherit(d: &Dict, inh: &Inherit) -> Inherit {
    let mut i = inh.clone();
    if let Some(v) = d.get("Resources") {
        i.resources = Some(v.clone());
    }
    if let Some(v) = d.get("MediaBox") {
        i.media = Some(v.clone());
    }
    if let Some(v) = d.get("CropBox") {
        i.crop = Some(v.clone());
    }
    if let Some(v) = d.get("Rotate") {
        i.rotate = Some(v.clone());
    }
    i
}

fn walk(file: &PdfFile, node: &Obj, inh: &Inherit, visited: &mut HashSet<u32>, depth: u32, out: &mut Vec<PageInfo>) {
    if depth > 64 || out.len() >= MAX_PAGES {
        return;
    }
    let (num, gen) = node.as_ref().unwrap_or((0, 0));
    if num != 0 && !visited.insert(num) {
        return;
    }
    let o = file.resolve(node);
    let Some(d) = o.as_dict() else { return };
    let inh = inherit(d, inh);
    let kids = file.dget(d, "Kids");
    let is_pages = d.is_type("Pages") || (!d.is_type("Page") && kids.as_arr().is_some());
    if is_pages {
        if let Some(kids) = kids.as_arr() {
            for k in kids {
                walk(file, k, &inh, visited, depth + 1, out);
            }
        }
    } else {
        out.push(make_page(file, num, gen, d, &inh));
    }
}

fn make_page(file: &PdfFile, num: u32, gen: u16, d: &Dict, inh: &Inherit) -> PageInfo {
    let media = inh.media.as_ref().map(|m| file.resolve(m)).and_then(|m| rect_of(&m)).unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let mut crop = inh.crop.as_ref().map(|m| file.resolve(m)).and_then(|m| rect_of(&m)).unwrap_or(media);
    // The crop box is clipped to the media box.
    crop = [crop[0].max(media[0]), crop[1].max(media[1]), crop[2].min(media[2]), crop[3].min(media[3])];
    if crop[2] - crop[0] < 1.0 || crop[3] - crop[1] < 1.0 {
        crop = media;
    }
    let rot = inh.rotate.as_ref().map(|r| file.resolve(r)).and_then(|r| r.as_i64()).unwrap_or(0);
    let rotate = (((rot % 360) + 360) % 360 / 90 * 90) as i32;
    let resources = inh.resources.as_ref().map(|r| file.resolve(r)).and_then(|r| r.as_dict().cloned()).unwrap_or_default();
    PageInfo { num, gen, dict: d.clone(), media, crop, rotate, resources }
}

/// One bookmark.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineEntry {
    pub title: String,
    /// The destination page (0-based), when it resolves.
    pub page: Option<usize>,
    pub level: usize,
}

/// The page index of a destination (explicit array, name, string, or a GoTo action).
pub fn dest_page(file: &PdfFile, dest: &Obj, pages: &HashMap<u32, usize>, depth: u32) -> Option<usize> {
    if depth > 8 {
        return None;
    }
    match file.resolve(dest) {
        Obj::Arr(a) => match a.first()? {
            Obj::Ref(n, _) => pages.get(n).copied(),
            Obj::Int(i) if *i >= 0 => Some(*i as usize),
            _ => None,
        },
        Obj::Name(n) => {
            let root = file.trailer().get("Root").map(|r| file.resolve(r))?;
            let dests = file.dict_of(root.as_dict()?, "Dests")?;
            let v = dests.get(&n)?.clone();
            dest_from_value(file, &v, pages, depth)
        }
        Obj::Str(s) => {
            let root = file.trailer().get("Root").map(|r| file.resolve(r))?;
            let names = file.dict_of(root.as_dict()?, "Names")?;
            let tree = file.dget(&names, "Dests");
            let v = name_tree_lookup(file, &tree, &s, 0)?;
            dest_from_value(file, &v, pages, depth)
        }
        Obj::Dict(d) => {
            if let Some(dd) = d.get("D") {
                return dest_page(file, dd, pages, depth + 1);
            }
            None
        }
        _ => None,
    }
}

fn dest_from_value(file: &PdfFile, v: &Obj, pages: &HashMap<u32, usize>, depth: u32) -> Option<usize> {
    match file.resolve(v) {
        Obj::Dict(d) => dest_page(file, d.get("D")?, pages, depth + 1),
        other => dest_page(file, &other, pages, depth + 1),
    }
}

fn name_tree_lookup(file: &PdfFile, node: &Obj, key: &[u8], depth: u32) -> Option<Obj> {
    if depth > 32 {
        return None;
    }
    let n = file.resolve(node);
    let d = n.as_dict()?;
    if let Obj::Arr(names) = file.dget(d, "Names") {
        for pair in names.chunks(2) {
            if pair.len() == 2 && file.resolve(&pair[0]).as_bytes() == Some(key) {
                return Some(pair[1].clone());
            }
        }
    }
    if let Obj::Arr(kids) = file.dget(d, "Kids") {
        for k in kids.iter().take(10_000) {
            let kd = file.resolve(k);
            if let Some(kd) = kd.as_dict() {
                if let Obj::Arr(lim) = file.dget(kd, "Limits") {
                    if lim.len() == 2 {
                        let lo = file.resolve(&lim[0]);
                        let hi = file.resolve(&lim[1]);
                        if let (Some(lo), Some(hi)) = (lo.as_bytes(), hi.as_bytes()) {
                            if key < lo || key > hi {
                                continue;
                            }
                        }
                    }
                }
            }
            if let Some(v) = name_tree_lookup(file, k, key, depth + 1) {
                return Some(v);
            }
        }
    }
    None
}

/// The outline (bookmarks), flattened depth-first.
pub fn collect_outlines(file: &PdfFile, pages: &HashMap<u32, usize>) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    let trailer = file.trailer();
    let Some(root) = trailer.get("Root").map(|r| file.resolve(r)) else { return out };
    let Some(root) = root.as_dict() else { return out };
    let Some(outlines) = file.dict_of(root, "Outlines") else { return out };
    let mut visited = HashSet::new();
    outline_level(file, outlines.get("First"), 0, pages, &mut visited, &mut out);
    out
}

fn outline_level(file: &PdfFile, first: Option<&Obj>, level: usize, pages: &HashMap<u32, usize>, visited: &mut HashSet<u32>, out: &mut Vec<OutlineEntry>) {
    if level > 32 {
        return;
    }
    let mut cur = first.cloned();
    while let Some(item) = cur.take() {
        if out.len() > 100_000 {
            return;
        }
        if let Obj::Ref(n, _) = item {
            if !visited.insert(n) {
                return;
            }
        }
        let o = file.resolve(&item);
        let Some(d) = o.as_dict() else { return };
        let title = file.dget(d, "Title").as_text().unwrap_or_default();
        let page = match d.get("Dest") {
            Some(dest) => dest_page(file, dest, pages, 0),
            None => file.dict_of(d, "A").and_then(|a| {
                if a.name("S") == Some("GoTo") {
                    a.get("D").and_then(|dd| dest_page(file, dd, pages, 0))
                } else {
                    None
                }
            }),
        };
        out.push(OutlineEntry { title, page, level });
        if d.contains("First") {
            outline_level(file, d.get("First"), level + 1, pages, visited, out);
        }
        cur = d.get("Next").cloned();
    }
}

/// One annotation, as `describe` and `read` report it.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotInfo {
    /// The annotation object's number (0 when direct).
    pub num: u32,
    pub subtype: String,
    /// PDF user space: x0, y0, x1, y1.
    pub rect: [f64; 4],
    pub contents: String,
    pub has_appearance: bool,
}

/// A page's annotation dictionaries with their object numbers.
pub fn page_annots(file: &PdfFile, page: &PageInfo) -> Vec<(u32, Dict)> {
    let mut out = Vec::new();
    if let Obj::Arr(a) = file.dget(&page.dict, "Annots") {
        for item in a.iter().take(10_000) {
            let num = item.as_ref().map_or(0, |r| r.0);
            if let Some(d) = file.resolve(item).as_dict() {
                out.push((num, d.clone()));
            }
        }
    }
    out
}

pub fn annot_info(file: &PdfFile, num: u32, d: &Dict) -> AnnotInfo {
    let r = file.dget(d, "Rect").nums();
    let rect = if r.len() >= 4 { [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])] } else { [0.0; 4] };
    AnnotInfo {
        num,
        subtype: d.name("Subtype").unwrap_or("").to_string(),
        rect,
        contents: file.dget(d, "Contents").as_text().unwrap_or_default(),
        has_appearance: file.dict_of(d, "AP").map_or(false, |ap| !file.dget(&ap, "N").is_null()),
    }
}

/// A link annotation's target: `#page=N` (1-based) for internal ones, else the URI.
pub fn link_target(file: &PdfFile, d: &Dict, pages: &HashMap<u32, usize>) -> Option<String> {
    if let Some(dest) = d.get("Dest") {
        return dest_page(file, dest, pages, 0).map(|p| format!("#page={}", p + 1));
    }
    let a = file.dict_of(d, "A")?;
    match a.name("S") {
        Some("URI") => file.dget(&a, "URI").as_bytes().map(|b| String::from_utf8_lossy(b).into_owned()),
        Some("GoTo") => a.get("D").and_then(|dd| dest_page(file, dd, pages, 0)).map(|p| format!("#page={}", p + 1)),
        _ => None,
    }
}

/// The info dictionary's text entry (`Title`, `Author`, ...).
pub fn info_text(file: &PdfFile, key: &str) -> Option<String> {
    let t = file.trailer();
    let info = t.get("Info").map(|i| file.resolve(i))?;
    let s = file.dget(info.as_dict()?, key).as_text()?;
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
