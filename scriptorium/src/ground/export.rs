//! Export and print for every editor: any [`Document`] - every page's display list and the resources
//! it refers to - written as one PDF by our own writer ([`super::pdfw`]). Print is the same file
//! handed to the system's printing by the host.
//!
//! Text is real PDF text when the document gives its faces ([`Document::pdf_resources`]); a glyph run
//! whose face is not given is drawn from its outline in [`Document::resources`] instead, so the page
//! looks the same either way. Images come from the display resources unless the faces' resources
//! already hold them (an original JPEG, say).

use super::bus::Document;
use super::display::{DisplayList, Item, Paint, PathOp, Resources};
use super::pdfw::{PageResources, PdfWriter};

/// How an export is written.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportOptions {
    /// The PDF's title (its information dictionary); empty for none.
    pub title: String,
    /// Creation date, seconds since the Unix epoch (UTC); `None` reads the clock. With it the output is
    /// byte-for-byte the same for the same document.
    pub date: Option<i64>,
}

/// Every page of `doc` as one PDF file.
pub fn export_pdf(doc: &dyn Document) -> Vec<u8> {
    export_pdf_with(doc, &ExportOptions::default())
}

/// [`export_pdf`] with a title and a fixed date.
pub fn export_pdf_with(doc: &dyn Document, opts: &ExportOptions) -> Vec<u8> {
    let mut res = doc.pdf_resources().unwrap_or_default();
    let display = doc.resources();
    if let Some(d) = display {
        for img in &d.images {
            if res.image(img.id).is_none() {
                res.add_image_res(img);
            }
        }
    }
    let mut w = PdfWriter::new();
    if !opts.title.is_empty() {
        w.set_info(&opts.title, "", "", "", "", "");
    }
    if let Some(date) = opts.date {
        w.set_date(date);
    }
    for page in 0..doc.page_count() {
        let list = outline_missing_faces(&doc.display(page), display, &res);
        w.add_page(&list, &res);
    }
    w.finish()
}

/// `list` with every glyph run whose face `res` lacks turned into a filled path of its outlines (from
/// `display`); a run with no outlines there either is left out.
pub fn outline_missing_faces(list: &DisplayList, display: Option<&Resources>, res: &PageResources) -> DisplayList {
    let mut out = DisplayList::new(list.width, list.height);
    for item in &list.items {
        match item {
            Item::Glyphs { font, size, color, glyphs } if res.font(*font).is_none() => {
                let Some(face) = display.and_then(|d| d.font(*font)) else { continue };
                let upem = if face.units_per_em > 0.0 { face.units_per_em } else { 1000.0 };
                let s = size / upem;
                let mut ops = Vec::new();
                for g in glyphs {
                    let Some(outline) = face.glyphs.get(&g.gid) else { continue };
                    // font units, y up -> page points, y down, at the glyph's origin
                    let p = |x: f64, y: f64| (g.x + x * s, g.y - y * s);
                    for op in outline {
                        ops.push(match *op {
                            PathOp::MoveTo(x, y) => {
                                let (x, y) = p(x, y);
                                PathOp::MoveTo(x, y)
                            }
                            PathOp::LineTo(x, y) => {
                                let (x, y) = p(x, y);
                                PathOp::LineTo(x, y)
                            }
                            PathOp::QuadTo(cx, cy, x, y) => {
                                let ((cx, cy), (x, y)) = (p(cx, cy), p(x, y));
                                PathOp::QuadTo(cx, cy, x, y)
                            }
                            PathOp::CubicTo(ax, ay, bx, by, x, y) => {
                                let ((ax, ay), (bx, by), (x, y)) = (p(ax, ay), p(bx, by), p(x, y));
                                PathOp::CubicTo(ax, ay, bx, by, x, y)
                            }
                            PathOp::Close => PathOp::Close,
                        });
                    }
                }
                if !ops.is_empty() {
                    out.items.push(Item::Path { ops, fill: Some(Paint::Solid(*color)), stroke: None });
                }
            }
            other => out.items.push(other.clone()),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground::bus::json::Json;
    use crate::ground::bus::{BusError, Change, Command, CommandSpec};
    use crate::ground::display::{Color, ImageRes, PlacedGlyph, Rect};
    use crate::ground::pdfw::reader::Doc;

    /// Pages of a given size, each white; optionally one glyph run and one picture on the first.
    struct Pages {
        count: usize,
        res: Option<Resources>,
    }

    impl Document for Pages {
        fn kind(&self) -> &'static str {
            "pages"
        }
        fn apply(&mut self, cmd: &Command) -> Result<Change, BusError> {
            Err(BusError::unknown(&cmd.name))
        }
        fn describe(&self) -> Json {
            Json::obj()
        }
        fn read(&self, _what: &Json) -> Result<Json, BusError> {
            Ok(Json::Null)
        }
        fn display(&self, page: usize) -> DisplayList {
            let mut dl = DisplayList::new(595.0, 842.0);
            dl.push(Item::Path {
                ops: vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(595.0, 0.0), PathOp::LineTo(595.0, 842.0), PathOp::LineTo(0.0, 842.0), PathOp::Close],
                fill: Some(Paint::Solid(Color::WHITE)),
                stroke: None,
            });
            if page == 0 && self.res.is_some() {
                dl.push(Item::Glyphs { font: 1, size: 10.0, color: Color::BLACK, glyphs: vec![PlacedGlyph { gid: 5, x: 72.0, y: 100.0 }, PlacedGlyph { gid: 9, x: 80.0, y: 100.0 }] });
                dl.push(Item::Image { image: 3, rect: Rect::new(72.0, 200.0, 20.0, 10.0) });
            }
            dl
        }
        fn page_count(&self) -> usize {
            self.count
        }
        fn commands(&self) -> Vec<CommandSpec> {
            Vec::new()
        }
        fn resources(&self) -> Option<&Resources> {
            self.res.as_ref()
        }
    }

    fn content(doc: &Doc, page: usize) -> String {
        let p = &doc.pages()[page];
        String::from_utf8(doc.stream_data(p.get("Contents").unwrap())).unwrap()
    }

    #[test]
    fn a_blank_page_exports_and_reads_back() {
        let bytes = export_pdf_with(&Pages { count: 1, res: None }, &ExportOptions { title: "Blank".into(), date: Some(1_700_000_000) });
        assert!(bytes.starts_with(b"%PDF-"));
        let doc = Doc::parse(bytes.clone()).expect("our export parses");
        for n in 1..doc.count() as u32 {
            doc.obj(n).unwrap_or_else(|e| panic!("object {n}: {e}"));
        }
        let pages = doc.pages();
        assert_eq!(pages.len(), 1);
        let mb: Vec<f64> = pages[0].get("MediaBox").unwrap().arr().unwrap().iter().map(|v| v.num().unwrap()).collect();
        assert_eq!(mb, vec![0.0, 0.0, 595.0, 842.0]);
        assert!(content(&doc, 0).contains("0 0 595 842 re"), "the white page is drawn");
        // the same document and date write the same bytes
        assert_eq!(bytes, export_pdf_with(&Pages { count: 1, res: None }, &ExportOptions { title: "Blank".into(), date: Some(1_700_000_000) }));
    }

    #[test]
    fn every_page_is_exported() {
        let doc = Doc::parse(export_pdf(&Pages { count: 3, res: None })).unwrap();
        assert_eq!(doc.pages().len(), 3);
        // a document with no pages still makes a valid one-page file
        assert_eq!(Doc::parse(export_pdf(&Pages { count: 0, res: None })).unwrap().pages().len(), 1);
    }

    #[test]
    fn glyphs_without_faces_are_drawn_from_their_outlines() {
        let mut res = Resources::new();
        res.add_font(1, 1000.0);
        res.set_glyph(1, 5, vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(500.0, 0.0), PathOp::LineTo(500.0, 700.0), PathOp::Close]);
        // glyph 9 has no outline cached: it is skipped, never a failure
        res.add_image(ImageRes { id: 3, width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 255] });
        let d = Pages { count: 1, res: Some(res) };

        let list = outline_missing_faces(&d.display(0), d.resources(), &PageResources::new());
        let Item::Path { ops, fill, .. } = &list.items[1] else { panic!("the run became a path: {:?}", list.items[1]) };
        assert_eq!(fill, &Some(Paint::Solid(Color::BLACK)));
        assert_eq!(ops[0], PathOp::MoveTo(72.0, 100.0));
        assert_eq!(ops[2], PathOp::LineTo(77.0, 93.0), "font units y up -> points y down at size 10");
        assert!(matches!(list.items[2], Item::Image { image: 3, .. }));

        let doc = Doc::parse(export_pdf(&d)).unwrap();
        let text = content(&doc, 0);
        assert!(text.contains("72 100 m") && text.contains("77 93 l"), "the glyph's outline is on the page (in display points under the page's flip): {text}");
        assert!(!text.contains("Tf"), "no font was given, so no PDF text");
        assert_eq!(doc.find_all("Image").len(), 1, "the picture came from the display resources");
    }
}
