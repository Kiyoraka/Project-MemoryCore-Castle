//! The PDF writer's tests: each writes a file and reads it back with the test reader.

use std::sync::Arc;

use super::reader::{Doc, Obj};
use super::*;
use crate::ground::display::{Color, GradientStop, Item, LineCap, Matrix, Paint, PathOp, PlacedGlyph, Rect, Stroke};
use crate::ground::font::Font;
use crate::ground::picture::jpeg;

const DATE: i64 = 1_700_000_000;

fn finish(mut w: PdfWriter) -> (Vec<u8>, Doc) {
    w.set_date(DATE);
    let bytes = w.finish();
    dump(&bytes);
    let doc = Doc::parse(bytes.clone()).expect("our output parses");
    // Every object's offset points at "n 0 obj" and every object parses.
    for n in 1..doc.count() as u32 {
        doc.obj(n).unwrap_or_else(|e| panic!("object {n}: {e}"));
    }
    (bytes, doc)
}

/// With SCRIPTORIUM_PDFW_DUMP set to a folder, each test's file is written there for viewers and
/// other readers to open (never part of the repository).
fn dump(bytes: &[u8]) {
    if let Ok(dir) = std::env::var("SCRIPTORIUM_PDFW_DUMP") {
        let name = std::thread::current().name().unwrap_or("pdf").replace("::", "_");
        let _ = std::fs::write(std::path::Path::new(&dir).join(format!("{name}.pdf")), bytes);
    }
}

fn content(doc: &Doc, page: usize) -> String {
    let p = &doc.pages()[page];
    String::from_utf8(doc.stream_data(p.get("Contents").unwrap())).unwrap()
}

fn square(x: f64, y: f64, s: f64) -> Vec<PathOp> {
    vec![PathOp::MoveTo(x, y), PathOp::LineTo(x + s, y), PathOp::LineTo(x + s, y + s), PathOp::LineTo(x, y + s), PathOp::Close]
}

fn nums_of(o: &Obj) -> Vec<f64> {
    o.arr().unwrap().iter().map(|v| v.num().unwrap()).collect()
}

fn page_resource(doc: &Doc, page: usize, cat: &str) -> Vec<(String, Obj)> {
    let p = &doc.pages()[page];
    let res = doc.get(p, "Resources").unwrap();
    match doc.get(&res, cat) {
        Some(d) => d.dict().unwrap().iter().map(|(k, v)| (k.clone(), doc.resolve(v))).collect(),
        None => Vec::new(),
    }
}

fn system_font() -> Option<Arc<Font>> {
    for p in ["C:/Windows/Fonts/arial.ttf", "C:/Windows/Fonts/segoeui.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/Library/Fonts/Arial.ttf"] {
        if let Ok(data) = std::fs::read(p) {
            if let Ok(f) = Font::parse(data, 0) {
                if f.has_truetype_outlines() {
                    return Some(Arc::new(f));
                }
            }
        }
    }
    None
}

#[test]
fn one_blank_page() {
    let mut w = PdfWriter::new();
    assert_eq!(w.add_page(&DisplayList::new(595.0, 842.0), &PageResources::new()), 0);
    let (bytes, doc) = finish(w);
    assert!(bytes.starts_with(b"%PDF-1.7\n"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    let cat = doc.catalog();
    assert_eq!(cat.get("Type").unwrap().name(), Some("Catalog"));
    let pages = doc.get(&cat, "Pages").unwrap();
    assert_eq!(pages.get("Count").unwrap().num(), Some(1.0));
    let pg = &doc.pages()[0];
    assert_eq!(pg.get("Type").unwrap().name(), Some("Page"));
    assert_eq!(nums_of(pg.get("MediaBox").unwrap()), vec![0.0, 0.0, 595.0, 842.0]);
    assert_eq!(content(&doc, 0), "1 0 0 -1 0 842 cm\n");
    // A writer with no pages still makes a valid one-page (Letter) file.
    let (_, doc) = finish(PdfWriter::new());
    assert_eq!(nums_of(doc.pages()[0].get("MediaBox").unwrap()), vec![0.0, 0.0, 612.0, 792.0]);
}

#[test]
fn paths_clip_transform_opacity_gradients() {
    let mut dl = DisplayList::new(300.0, 400.0);
    dl.push(Item::Path { ops: square(10.0, 10.0, 50.0), fill: Some(Paint::Solid(Color::rgb(255, 0, 0))), stroke: None });
    let mut st = Stroke::new(2.0, Color::rgb(0, 0, 255));
    st.dash = vec![3.0, 1.0];
    st.cap = LineCap::Round;
    dl.push(Item::Path { ops: vec![PathOp::MoveTo(0.0, 0.0), PathOp::QuadTo(30.0, 60.0, 60.0, 0.0)], fill: None, stroke: Some(st.clone()) });
    dl.push(Item::Path { ops: square(100.0, 100.0, 20.0), fill: Some(Paint::Solid(Color::rgba(0, 255, 0, 128))), stroke: Some(st) });
    dl.push(Item::PushTransform(Matrix::translate(10.0, 20.0)));
    dl.push(Item::PushClip(vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(100.0, 0.0), PathOp::LineTo(50.0, 80.0), PathOp::Close]));
    dl.push(Item::PushOpacity(0.5));
    dl.push(Item::Path { ops: square(0.0, 0.0, 30.0), fill: Some(Paint::Solid(Color::BLACK)), stroke: None });
    dl.push(Item::PopOpacity);
    let stops3 = vec![
        GradientStop { offset: 0.0, color: Color::rgb(255, 0, 0) },
        GradientStop { offset: 0.5, color: Color::rgb(0, 255, 0) },
        GradientStop { offset: 1.0, color: Color::rgb(0, 0, 255) },
    ];
    dl.push(Item::Path { ops: square(0.0, 0.0, 100.0), fill: Some(Paint::Linear { x0: 0.0, y0: 0.0, x1: 100.0, y1: 0.0, stops: stops3 }), stroke: None });
    dl.push(Item::PopClip);
    dl.push(Item::PopTransform);
    let stops2 = vec![GradientStop { offset: 0.0, color: Color::WHITE }, GradientStop { offset: 1.0, color: Color::BLACK }];
    dl.push(Item::Path {
        ops: square(150.0, 150.0, 100.0),
        fill: Some(Paint::Radial { fx: 200.0, fy: 200.0, fr: 0.0, cx: 200.0, cy: 200.0, r: 50.0, stops: stops2 }),
        stroke: None,
    });
    let fading = vec![GradientStop { offset: 0.0, color: Color::rgba(255, 0, 0, 255) }, GradientStop { offset: 1.0, color: Color::rgba(255, 0, 0, 0) }];
    dl.push(Item::Path { ops: square(0.0, 300.0, 100.0), fill: Some(Paint::Linear { x0: 0.0, y0: 300.0, x1: 100.0, y1: 300.0, stops: fading }), stroke: None });
    // Unbalanced pops are ignored; an unclosed push is closed at the end.
    dl.push(Item::PopClip);
    dl.push(Item::PushTransform(Matrix::scale(2.0, 2.0)));

    let mut w = PdfWriter::new();
    w.add_page(&dl, &PageResources::new());
    let (_, doc) = finish(w);
    let c = content(&doc, 0);
    assert!(c.starts_with("1 0 0 -1 0 400 cm\n"), "{c}");
    assert!(c.contains("1 0 0 rg\n10 10 50 50 re\nf\n"), "rectangle as re + f:\n{c}");
    assert!(c.contains("0 0 m\n20 40 40 40 60 0 c\n"), "quadratic raised to a cubic:\n{c}");
    assert!(c.contains("2 w\n1 J\n0 j\n10 M\n[3 1] 0 d\n"), "stroke state:\n{c}");
    assert!(c.contains("\nS\n") && c.contains("\nB\n"));
    assert!(c.contains("1 0 0 1 10 20 cm\n"));
    assert!(c.contains("0 0 m\n100 0 l\n50 80 l\nh\nW n\n"), "clip:\n{c}");
    assert_eq!(c.matches("q\n").count(), c.matches("Q\n").count(), "q/Q balanced");
    assert!(c.contains(" sh\n"));

    // ExtGStates: the half-green fill (ca 0.502, CA 1), the 50% opacity, a soft mask.
    let gs = page_resource(&doc, 0, "ExtGState");
    let cas: Vec<f64> = gs.iter().filter_map(|(_, g)| g.get("ca").and_then(|v| v.num())).collect();
    assert!(cas.contains(&0.502) && cas.contains(&0.5), "{cas:?}");
    let masked: Vec<&Obj> = gs.iter().map(|(_, g)| g).filter(|g| g.get("SMask").is_some()).collect();
    assert_eq!(masked.len(), 1);
    let sm = masked[0].get("SMask").unwrap();
    assert_eq!(sm.get("S").unwrap().name(), Some("Luminosity"));
    let form = doc.resolve(sm.get("G").unwrap());
    assert_eq!(form.get("Subtype").unwrap().name(), Some("Form"));
    assert!(String::from_utf8(doc.stream_data(&form)).unwrap().contains(" sh"));

    // Shadings: axial with a stitched function, radial with one Type 2.
    let sh = page_resource(&doc, 0, "Shading");
    let axial: Vec<&Obj> = sh.iter().map(|(_, s)| s).filter(|s| s.get("ShadingType").unwrap().num() == Some(2.0)).collect();
    let radial: Vec<&Obj> = sh.iter().map(|(_, s)| s).filter(|s| s.get("ShadingType").unwrap().num() == Some(3.0)).collect();
    assert_eq!(radial.len(), 1);
    assert_eq!(nums_of(radial[0].get("Coords").unwrap()), vec![200.0, 200.0, 0.0, 200.0, 200.0, 50.0]);
    assert_eq!(radial[0].get("Function").unwrap().get("FunctionType").unwrap().num(), Some(2.0));
    let stitched = axial.iter().find(|s| s.get("Function").unwrap().get("FunctionType").unwrap().num() == Some(3.0)).expect("stitched axial");
    let f = stitched.get("Function").unwrap();
    assert_eq!(nums_of(f.get("Bounds").unwrap()), vec![0.5]);
    assert_eq!(f.get("Functions").unwrap().arr().unwrap().len(), 2);
    assert_eq!(nums_of(f.get("Encode").unwrap()), vec![0.0, 1.0, 0.0, 1.0]);
}

#[test]
fn hard_stops_and_padding() {
    let stops = vec![
        GradientStop { offset: 0.25, color: Color::rgb(255, 0, 0) },
        GradientStop { offset: 0.5, color: Color::rgb(255, 0, 0) },
        GradientStop { offset: 0.5, color: Color::rgb(0, 0, 255) },
    ];
    let n = content::normalize_stops(&stops);
    assert_eq!(n.len(), 5);
    assert_eq!((n[0].offset, n[4].offset), (0.0, 1.0));
    let f = content::function(&n, |c| vec![c.r as f64 / 255.0]);
    // Three intervals of positive width: [0,.25] [.25,.5] [.5,1]; the hard edge is the bound at .5.
    assert!(f.contains("/Bounds [0.25 0.5]"), "{f}");
    assert!(f.contains("/C0 [0] /C1 [0]"), "{f}");
}

#[test]
fn image_with_alpha_opaque_image_and_jpeg() {
    let mut rgba = Image::filled(2, 2, [10, 20, 30, 255]);
    rgba.pixels[3] = 0;
    rgba.pixels[7] = 128;
    let opaque = Image::filled(3, 1, [200, 100, 50, 255]);
    let jpg_src = Image::filled(16, 8, [90, 160, 220, 255]);
    let jpg = jpeg::encode(&jpg_src, 85);

    let mut res = PageResources::new();
    res.add_rgba(1, rgba);
    res.add_rgba(2, opaque);
    assert!(res.add_jpeg(3, jpg.clone()));
    assert!(!res.add_jpeg(4, b"not a jpeg".to_vec()));
    let mut dl = DisplayList::new(200.0, 200.0);
    dl.push(Item::Image { image: 1, rect: Rect::new(10.0, 20.0, 40.0, 30.0) });
    dl.push(Item::Image { image: 2, rect: Rect::new(0.0, 0.0, 30.0, 10.0) });
    dl.push(Item::PushOpacity(0.25));
    dl.push(Item::Image { image: 3, rect: Rect::new(50.0, 50.0, 16.0, 8.0) });
    dl.push(Item::PopOpacity);
    dl.push(Item::Image { image: 99, rect: Rect::new(0.0, 0.0, 1.0, 1.0) }); // missing: left out
    let mut w = PdfWriter::new();
    w.add_page(&dl, &res);
    w.add_page(&dl, &res); // same images again: written once
    let (_, doc) = finish(w);

    let c = content(&doc, 0);
    assert!(c.contains("40 0 0 -30 10 50 cm /Im"), "image placed upright over its rect:\n{c}");
    assert_eq!(c.matches(" Do").count(), 3);
    let xo = page_resource(&doc, 0, "XObject");
    assert_eq!(xo.len(), 3);
    assert_eq!(doc.find_all("XObject").len(), 4, "3 images + 1 SMask, shared by both pages");

    let with_alpha = xo.iter().map(|(_, o)| o).find(|o| o.get("Width").unwrap().num() == Some(2.0)).unwrap();
    assert_eq!(with_alpha.get("ColorSpace").unwrap().name(), Some("DeviceRGB"));
    assert_eq!(doc.stream_data(with_alpha), vec![10, 20, 30, 10, 20, 30, 10, 20, 30, 10, 20, 30]);
    let smask = doc.resolve(with_alpha.get("SMask").expect("SMask for alpha"));
    assert_eq!(smask.get("ColorSpace").unwrap().name(), Some("DeviceGray"));
    assert_eq!(doc.stream_data(&smask), vec![0, 128, 255, 255]);

    let op = xo.iter().map(|(_, o)| o).find(|o| o.get("Width").unwrap().num() == Some(3.0)).unwrap();
    assert!(op.get("SMask").is_none());

    let j = xo.iter().map(|(_, o)| o).find(|o| o.get("Filter").and_then(|f| f.name()) == Some("DCTDecode")).expect("jpeg passthrough");
    assert_eq!((j.get("Width").unwrap().num(), j.get("Height").unwrap().num()), (Some(16.0), Some(8.0)));
    assert_eq!(doc.stream_data(j), jpg, "the JPEG's bytes unchanged");
    assert!(c.contains("/GS"), "opacity on the image:\n{c}");
}

#[test]
fn jpeg_with_exif_rotation_is_decoded_upright() {
    // A JPEG with an EXIF orientation 6 (turn 90 degrees clockwise) segment spliced in after SOI.
    let src = Image::filled(8, 4, [255, 0, 0, 255]);
    let jpg = jpeg::encode(&src, 90);
    let tiff: Vec<u8> = [b"MM\0\x2a\0\0\0\x08".as_slice(), &[0, 1], &[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0], &[0, 0, 0, 0]].concat();
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    let mut with = jpg[..2].to_vec();
    with.extend_from_slice(&[0xFF, 0xE1]);
    with.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
    with.extend_from_slice(&app1);
    with.extend_from_slice(&jpg[2..]);
    match PdfImage::from_jpeg(with).unwrap() {
        PdfImage::Rgba(i) => assert_eq!((i.width, i.height), (4, 8)),
        other => panic!("expected decoded pixels, got {:?}", other.width()),
    }
    assert!(matches!(PdfImage::from_jpeg(jpg).unwrap(), PdfImage::Jpeg { components: 3, .. }));
}

/// Re-runs a `[ ... ] TJ` array to find where each glyph lands (x in user space).
fn tj_positions(tj: &str, x0: f64, size: f64, width: impl Fn(u16) -> f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut pen = x0;
    let mut i = 0;
    let b = tj.as_bytes();
    while i < b.len() {
        match b[i] {
            b'<' => {
                let end = tj[i..].find('>').unwrap() + i;
                let hex = &tj[i + 1..end];
                for k in (0..hex.len()).step_by(4) {
                    let g = u16::from_str_radix(&hex[k..k + 4], 16).unwrap();
                    out.push(pen);
                    pen += size * width(g) / 1000.0;
                }
                i = end + 1;
            }
            b'-' | b'0'..=b'9' | b'.' => {
                let end = tj[i..].find(|c: char| !(c == '-' || c == '.' || c.is_ascii_digit())).unwrap() + i;
                let adj: f64 = tj[i..end].parse().unwrap();
                pen -= adj * size / 1000.0;
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}

#[test]
fn text_with_an_installed_font() {
    let Some(font) = system_font() else {
        eprintln!("no Arial / Segoe UI / DejaVu Sans here: text test skipped");
        return;
    };
    let size = 12.0;
    let upem = font.units_per_em() as f64;
    let gids: Vec<u16> = "Hello".chars().map(|c| font.glyph_id(c).unwrap()).collect();
    // Run 1: exactly at the advances. Run 2: letter-spaced by 1.7pt, on another baseline.
    let mut run1 = Vec::new();
    let mut run2 = Vec::new();
    let (mut x1, mut x2) = (72.0, 72.0);
    for &g in &gids {
        run1.push(PlacedGlyph { gid: g, x: x1, y: 100.0 });
        run2.push(PlacedGlyph { gid: g, x: x2, y: 130.0 });
        let adv = font.advance(g) as f64 * size / upem;
        x1 += adv;
        x2 += adv + 1.7;
    }
    let mut dl = DisplayList::new(612.0, 792.0);
    dl.push(Item::Glyphs { font: 7, size, color: Color::BLACK, glyphs: run1.clone() });
    dl.push(Item::Glyphs { font: 7, size, color: Color::rgb(0, 0, 128), glyphs: run2.clone() });
    let fi = font.glyph_id('f').unwrap();
    dl.push(Item::Glyphs { font: 7, size, color: Color::BLACK, glyphs: vec![PlacedGlyph { gid: fi, x: 72.0, y: 160.0 }] });
    let mut res = PageResources::new();
    res.add_font(7, font.clone());
    res.set_glyph_text(7, fi, "f\u{2063}"); // caller-given text wins over the cmap
    let mut w = PdfWriter::new();
    w.add_page(&dl, &res);
    w.add_page(&dl, &res); // the same face on a second page: one font in the file
    let (_, doc) = finish(w);

    let fonts = page_resource(&doc, 0, "Font");
    assert_eq!(fonts.len(), 1);
    assert_eq!(doc.find_all("Type0").len(), 1);
    let t0 = &fonts[0].1;
    assert_eq!(t0.get("Subtype").unwrap().name(), Some("Type0"));
    assert_eq!(t0.get("Encoding").unwrap().name(), Some("Identity-H"));
    let base = t0.get("BaseFont").unwrap().name().unwrap().to_string();
    let (tag, ps) = base.split_once('+').expect("subset tag");
    assert!(tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()), "{base}");
    assert_eq!(ps, font.names().postscript_name.replace(' ', ""));

    let cid = doc.resolve(&t0.get("DescendantFonts").unwrap().arr().unwrap()[0]);
    assert_eq!(cid.get("Subtype").unwrap().name(), Some("CIDFontType2"));
    assert_eq!(cid.get("CIDToGIDMap").unwrap().name(), Some("Identity"));
    let warr = cid.get("W").unwrap().arr().unwrap();
    assert!(!warr.is_empty());
    let fd = doc.get(&cid, "FontDescriptor").unwrap();
    assert_eq!(fd.get("FontName").unwrap().name(), Some(base.as_str()));
    assert!(fd.get("Ascent").unwrap().num().unwrap() > 0.0 && fd.get("Descent").unwrap().num().unwrap() < 0.0);
    assert!(fd.get("CapHeight").is_some() && fd.get("StemV").is_some() && fd.get("ItalicAngle").is_some());
    let ff = doc.get(&fd, "FontFile2").expect("FontFile2");
    let program = doc.stream_data(&ff);
    assert_eq!(ff.get("Length1").unwrap().num(), Some(program.len() as f64));
    assert!(program.len() < font.data().len() / 4, "subset is small: {} of {}", program.len(), font.data().len());
    let sub = Font::parse(program, 0).expect("the subset re-parses with our Font::parse");
    for &g in &gids {
        assert_eq!(sub.advance(g), font.advance(g), "ids kept");
        assert!(!sub.outline(g).unwrap().is_empty(), "outline kept for {g}");
    }

    let tu = String::from_utf8(doc.stream_data(t0.get("ToUnicode").unwrap())).unwrap();
    let h = font.glyph_id('H').unwrap();
    assert!(tu.contains(&format!("<{h:04X}> <0048>")), "{tu}");
    assert!(tu.contains(&format!("<{fi:04X}> <00662063>")), "{tu}");
    assert!(tu.contains("begincmap") && tu.contains("endcmap"));

    // Placement: the TJ arrays put each glyph exactly where the display list did.
    let c = content(&doc, 0);
    assert!(c.contains("BT\n/F") && c.contains(" 1 Tf\n") && c.contains("ET\n"));
    let width = |g: u16| (font.advance(g) as f64 * 1000.0 / upem * 1000.0).round() / 1000.0;
    let lines: Vec<&str> = c.lines().collect();
    let tms: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.ends_with(" Tm")).map(|(i, _)| i).collect();
    assert_eq!(tms.len(), 3);
    assert_eq!(lines[tms[0]], "12 0 0 -12 72 100 Tm");
    let tj1 = lines[tms[0] + 1];
    assert!(!tj1.contains('>') || tj1.matches('<').count() == 1, "no adjustments at the advances: {tj1}");
    for (tm, run) in [(tms[0], &run1), (tms[1], &run2)] {
        let got = tj_positions(lines[tm + 1], 72.0, size, width);
        assert_eq!(got.len(), run.len());
        for (g, want) in got.iter().zip(run.iter()) {
            assert!((g - want.x).abs() < 0.001, "glyph at {g}, display list says {}", want.x);
        }
    }
    assert!(lines[tms[1] + 1].matches('<').count() == 5, "letter-spaced run has adjustments: {}", lines[tms[1] + 1]);
}

#[test]
fn multiple_pages_and_outlines() {
    let mut w = PdfWriter::new();
    for (wd, ht) in [(612.0, 792.0), (842.0, 595.0), (300.0, 300.0)] {
        let mut dl = DisplayList::new(wd, ht);
        dl.push(Item::Path { ops: square(1.0, 1.0, 10.0), fill: Some(Paint::Solid(Color::BLACK)), stroke: None });
        w.add_page(&dl, &PageResources::new());
    }
    assert_eq!(w.page_count(), 3);
    w.add_outline("Chapter 1", 0, 72.0);
    w.add_outline_level("Section 1.1", 1, 100.0, 1);
    w.add_outline_level("Section 1.2 \u{2014} caf\u{e9}", 1, 200.0, 1);
    w.add_outline("Chapter 2", 2, 0.0);
    w.add_outline("Beyond the end", 99, 0.0);
    let (_, doc) = finish(w);
    let pages = doc.pages();
    assert_eq!(pages.len(), 3);
    assert_eq!(nums_of(pages[1].get("MediaBox").unwrap()), vec![0.0, 0.0, 842.0, 595.0]);
    assert_eq!(content(&doc, 2), "1 0 0 -1 0 300 cm\n0 0 0 rg\n1 1 10 10 re\nf\n");

    let cat = doc.catalog();
    assert_eq!(cat.get("PageMode").unwrap().name(), Some("UseOutlines"));
    let root = doc.get(&cat, "Outlines").unwrap();
    assert_eq!(root.get("Count").unwrap().num(), Some(5.0));
    let ch1 = doc.get(&root, "First").unwrap();
    assert_eq!(ch1.get("Title").unwrap().bytes(), Some(b"Chapter 1".as_slice()));
    let dest = ch1.get("Dest").unwrap().arr().unwrap();
    assert_eq!(dest[0].reference(), Some(doc.page_ref(0)));
    assert_eq!(dest[1].name(), Some("XYZ"));
    assert_eq!(dest[3].num(), Some(720.0), "y flipped to PDF space");
    assert_eq!(ch1.get("Count").unwrap().num(), Some(2.0));
    let s11 = doc.get(&ch1, "First").unwrap();
    let s12 = doc.get(&s11, "Next").unwrap();
    assert_eq!(s12.get("Title").unwrap().bytes().unwrap()[..2], [0xFE, 0xFF], "non-ASCII titles in UTF-16BE");
    assert_eq!(s12.get("Dest").unwrap().arr().unwrap()[3].num(), Some(395.0));
    assert!(s12.get("Next").is_none());
    let ch2 = doc.get(&ch1, "Next").unwrap();
    assert_eq!(ch2.get("Dest").unwrap().arr().unwrap()[0].reference(), Some(doc.page_ref(2)));
    let last = doc.get(&root, "Last").unwrap();
    assert_eq!(last.get("Dest").unwrap().arr().unwrap()[0].reference(), Some(doc.page_ref(2)), "page clamped");
    assert!(last.get("Prev").is_some());
}

#[test]
fn links_uri_and_internal() {
    let mut dl = DisplayList::new(600.0, 800.0);
    dl.push(Item::Link { rect: Rect::new(10.0, 20.0, 100.0, 15.0), target: "https://example.com/a(b)".into() });
    dl.push(Item::PushTransform(Matrix::translate(50.0, 50.0)));
    dl.push(Item::Link { rect: Rect::new(0.0, 0.0, 10.0, 10.0), target: "#page=2&y=100".into() });
    dl.push(Item::PopTransform);
    dl.push(Item::Link { rect: Rect::new(0.0, 0.0, 10.0, 10.0), target: "#somewhere".into() });
    let mut w = PdfWriter::new();
    w.add_page(&dl, &PageResources::new());
    w.add_page(&DisplayList::new(600.0, 500.0), &PageResources::new());
    let (_, doc) = finish(w);
    let annots = doc.pages()[0].get("Annots").unwrap().arr().unwrap().to_vec();
    assert_eq!(annots.len(), 2);
    let uri = doc.resolve(&annots[0]);
    assert_eq!(uri.get("Subtype").unwrap().name(), Some("Link"));
    assert_eq!(nums_of(uri.get("Rect").unwrap()), vec![10.0, 765.0, 110.0, 780.0]);
    let a = uri.get("A").unwrap();
    assert_eq!(a.get("S").unwrap().name(), Some("URI"));
    assert_eq!(a.get("URI").unwrap().bytes(), Some(b"https://example.com/a(b)".as_slice()));
    let internal = doc.resolve(&annots[1]);
    assert_eq!(nums_of(internal.get("Rect").unwrap()), vec![50.0, 740.0, 60.0, 750.0]);
    let d = internal.get("Dest").unwrap().arr().unwrap();
    assert_eq!(d[0].reference(), Some(doc.page_ref(1)));
    assert_eq!(d[3].num(), Some(400.0));
    assert!(doc.pages()[1].get("Annots").is_none());
}

#[test]
fn info_trailer_and_determinism() {
    let build = |date: i64| {
        let mut w = PdfWriter::new();
        w.set_info("A (title)", "Kiyo", "Tests", "pdf, writer", "", "");
        w.set_lang("en-US");
        w.set_date(date);
        let mut dl = DisplayList::new(100.0, 100.0);
        dl.push(Item::Path { ops: square(0.0, 0.0, 5.0), fill: Some(Paint::Solid(Color::rgba(1, 2, 3, 100))), stroke: None });
        w.add_page(&dl, &PageResources::new());
        w.add_outline("Top", 0, 0.0);
        w.finish()
    };
    let a = build(DATE);
    assert_eq!(a, build(DATE), "same input, same bytes");
    assert_ne!(a, build(DATE + 1));
    let doc = Doc::parse(a).unwrap();
    for n in 1..doc.count() as u32 {
        doc.obj(n).unwrap();
    }
    let info = doc.get(&doc.trailer, "Info").unwrap();
    assert_eq!(info.get("Title").unwrap().bytes(), Some(b"A (title)".as_slice()));
    assert_eq!(info.get("Creator").unwrap().bytes(), Some(b"Scriptorium".as_slice()));
    assert_eq!(info.get("Producer").unwrap().bytes(), Some(b"Scriptorium PDF writer".as_slice()));
    assert_eq!(info.get("CreationDate").unwrap().bytes(), Some(b"D:20231114221320Z".as_slice()));
    assert_eq!(doc.catalog().get("Lang").unwrap().bytes(), Some(b"en-US".as_slice()));
    let id = doc.trailer.get("ID").unwrap().arr().unwrap();
    assert_eq!(id.len(), 2);
    assert_eq!(id[0].bytes().unwrap().len(), 16);
    assert_eq!(id[0], id[1]);
    assert_eq!(doc.trailer.get("Size").unwrap().num(), Some(doc.count() as f64));
}

#[test]
fn xref_offsets_point_at_their_objects() {
    let mut dl = DisplayList::new(200.0, 200.0);
    for i in 0..20 {
        dl.push(Item::Path { ops: square(i as f64, i as f64, 3.0), fill: Some(Paint::Solid(Color::rgba(i * 10, 0, 0, 200 - i * 5))), stroke: None });
    }
    let mut w = PdfWriter::new();
    w.add_page(&dl, &PageResources::new());
    w.add_page(&dl, &PageResources::new());
    let (bytes, doc) = finish(w);
    assert!(doc.count() > 10);
    for n in 1..doc.count() {
        let off = doc.offsets[n];
        assert!(bytes[off..].starts_with(format!("{n} 0 obj\n").as_bytes()), "object {n} at {off}");
    }
    // The startxref offset points at the xref keyword (checked by the parse) and the free head is first.
    let x = bytes.windows(6).rposition(|w| w == b"
xref
").unwrap() + 1;
    assert!(bytes[x..].starts_with(b"xref\n0 "));
    assert!(String::from_utf8_lossy(&bytes[x..]).contains("0000000000 65535 f\r\n"));
}

#[test]
fn cff_face_if_present() {
    // A CFF (PostScript-outline) OpenType face is embedded whole as FontFile3/OpenType.
    let dirs = ["C:/Windows/Fonts", "/usr/share/fonts/opentype", "/Library/Fonts"];
    let mut found = None;
    'outer: for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut paths: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("otf")) != Some(true) {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&p) else { continue };
            if meta.len() > 3_000_000 {
                continue;
            }
            if let Some(f) = std::fs::read(&p).ok().and_then(|d| Font::parse(d, 0).ok()) {
                if f.is_cff() && f.glyph_id('A').is_some() {
                    found = Some(Arc::new(f));
                    break 'outer;
                }
            }
        }
    }
    let Some(font) = found else {
        eprintln!("no CFF .otf face here: CFF test skipped");
        return;
    };
    let g = font.glyph_id('A').unwrap();
    let mut dl = DisplayList::new(200.0, 200.0);
    dl.push(Item::Glyphs { font: 1, size: 20.0, color: Color::BLACK, glyphs: vec![PlacedGlyph { gid: g, x: 10.0, y: 50.0 }] });
    let mut res = PageResources::new();
    res.add_font(1, font.clone());
    let mut w = PdfWriter::new();
    w.add_page(&dl, &res);
    let (_, doc) = finish(w);
    let t0 = &page_resource(&doc, 0, "Font")[0].1;
    let cid = doc.resolve(&t0.get("DescendantFonts").unwrap().arr().unwrap()[0]);
    assert_eq!(cid.get("Subtype").unwrap().name(), Some("CIDFontType0"));
    let fd = doc.get(&cid, "FontDescriptor").unwrap();
    let ff3 = doc.get(&fd, "FontFile3").unwrap();
    assert_eq!(ff3.get("Subtype").unwrap().name(), Some("OpenType"));
    assert!(Font::parse(doc.stream_data(&ff3), 0).unwrap().is_cff());
}

#[test]
fn rect_detection_and_paths() {
    assert_eq!(content::path_ops(&square(1.0, 2.0, 3.0)), "1 2 3 3 re\n");
    // Closing back to the start explicitly is still a rectangle; a rotated quad is not.
    let mut sq = square(0.0, 0.0, 2.0);
    sq.insert(4, PathOp::LineTo(0.0, 0.0));
    assert_eq!(content::path_ops(&sq), "0 0 2 2 re\n");
    let diamond = vec![PathOp::MoveTo(1.0, 0.0), PathOp::LineTo(2.0, 1.0), PathOp::LineTo(1.0, 2.0), PathOp::LineTo(0.0, 1.0), PathOp::Close];
    assert_eq!(content::path_ops(&diamond), "1 0 m\n2 1 l\n1 2 l\n0 1 l\nh\n");
    // Two subpaths, a cubic, and a curve after a close starting from the subpath's start.
    let ops = vec![
        PathOp::MoveTo(0.0, 0.0),
        PathOp::CubicTo(1.0, 1.0, 2.0, 1.0, 3.0, 0.0),
        PathOp::Close,
        PathOp::QuadTo(3.0, 3.0, 6.0, 0.0),
    ];
    assert_eq!(content::path_ops(&ops), "0 0 m\n1 1 2 1 3 0 c\nh\n0 0 m\n2 2 4 2 6 0 c\n");
    assert_eq!(content::path_ops(&[]), "");
}
