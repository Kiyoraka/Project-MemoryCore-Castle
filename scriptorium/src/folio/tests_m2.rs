//! Folio M2's tests (select, copy, find, the ways around, organising pages, forms, locked files), on
//! invented PDFs built here.

use super::crypt::{self, Method};
use super::tests::{b, open, pdf, stream};
use super::*;
use crate::ground::bus::json::Json;
use crate::ground::bus::{Bus, Command};
use crate::ground::edit::{door, Editing, Pos, Selection};

// ---------------------------------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------------------------------

/// Pages showing `contents` with Helvetica as /F1 (object 3). Page i is object 4 + 2i, its content
/// 5 + 2i; `extra` objects follow from 4 + 2n.
fn pages_pdf(contents: &[&str], page_extra: &[&str], catalog_extra: &str, extra: Vec<Vec<u8>>, trailer_extra: &str) -> Vec<u8> {
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut bodies = vec![
        b(&format!("<< /Type /Catalog /Pages 2 0 R {} >>", catalog_extra)),
        b(&format!("<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 612 792] >>", kids.join(" "), n)),
        b("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"),
    ];
    for (i, c) in contents.iter().enumerate() {
        bodies.push(b(&format!(
            "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R {} >>",
            5 + 2 * i,
            page_extra.get(i).copied().unwrap_or("")
        )));
        bodies.push(stream("", c.as_bytes()));
    }
    bodies.extend(extra);
    pdf(&bodies, trailer_extra)
}

fn tp(page: usize, index: usize) -> Pos {
    select::text_pos(page, index)
}

fn sel(a: Pos, f: Pos) -> Selection {
    Selection::new(a, f)
}

fn at(p: &Pos) -> (usize, usize) {
    (p.json().get("page").and_then(Json::as_usize).unwrap(), p.json().get("index").and_then(Json::as_usize).unwrap())
}

fn cmd(name: &str, args: Json) -> Command {
    Command::new(name, args)
}

fn landed(out: &Json) -> Option<(usize, usize, usize, usize)> {
    let s = out.get("selection")?;
    if s.is_null() {
        return None;
    }
    let a = s.get("anchor")?;
    let f = s.get("focus")?;
    Some((a.get("page")?.as_usize()?, a.get("index")?.as_usize()?, f.get("page")?.as_usize()?, f.get("index")?.as_usize()?))
}

fn text_doc() -> Vec<u8> {
    pages_pdf(
        &["BT /F1 24 Tf 72 700 Td (Hello World) Tj 0 -30 Td (Second line here) Tj ET", "BT /F1 24 Tf 72 700 Td (Page two text) Tj ET"],
        &[],
        "",
        Vec::new(),
        "",
    )
}

// ---------------------------------------------------------------------------------------------------
// The editing ground
// ---------------------------------------------------------------------------------------------------

#[test]
fn the_page_text_is_laid_out_for_the_caret() {
    let d = open(&text_doc());
    assert!(d.editing().is_some());
    assert_eq!(d.page_text(0), "Hello World\nSecond line here");
    assert_eq!(d.laid[0].chars.len(), d.page_text(0).chars().count());

    // Carets sit on the glyph boxes, left to right, the line's height tall.
    let c0 = d.caret(&tp(0, 0)).unwrap();
    assert_eq!(c0.page, 0);
    assert!((c0.x - 72.0).abs() < 0.01 && (c0.height - 24.0).abs() < 0.01, "{:?}", c0);
    assert!((c0.y - (92.0 - 0.8 * 24.0)).abs() < 0.01, "{:?}", c0);
    let c1 = d.caret(&tp(0, 1)).unwrap();
    assert!(c1.x > c0.x + 10.0 && (c1.y - c0.y).abs() < 1e-9);
    let end = d.caret(&tp(0, 11)).unwrap();
    let second = d.caret(&tp(0, 12)).unwrap();
    assert!(end.x > c1.x && (second.x - 72.0).abs() < 0.01 && second.y > c0.y + 20.0, "{:?} {:?}", end, second);
    assert!(d.caret(&tp(0, 999)).is_some(), "past the end clamps to the end");
    assert!(d.caret(&tp(7, 0)).is_none());

    // Hits land before or after a glyph by its middle; far from any text, nothing.
    let mid_h = (c0.x + c1.x) / 2.0;
    assert_eq!(at(&d.hit(0, mid_h - 1.0, 85.0).unwrap()), (0, 0));
    assert_eq!(at(&d.hit(0, mid_h + 1.0, 85.0).unwrap()), (0, 1));
    assert_eq!(at(&d.hit(0, 80.0, second.y + 5.0).unwrap()).1, 12);
    assert!(d.hit(0, 500.0, 700.0).is_none());
    // Through the ground's door, as the page asks.
    let hit = door(&d, "hit", &Json::obj().with("page", 0).with("x", mid_h - 1.0).with("y", 85.0)).unwrap();
    assert_eq!(hit.to_string(), r#"{"page":0,"index":0}"#);
}

#[test]
fn caret_motions_cross_words_lines_and_pages() {
    let d = open(&text_doc());
    let mv = |p: (usize, usize), m: &str| d.step(&tp(p.0, p.1), m).map(|q| at(&q));
    assert_eq!(mv((0, 11), "right"), Some((0, 12)));
    assert_eq!(mv((0, 0), "left"), None);
    assert_eq!(mv((0, 0), "word_right"), Some((0, 6)));
    assert_eq!(mv((0, 5), "word_right"), Some((0, 6)));
    assert_eq!(mv((0, 8), "word_left"), Some((0, 6)));
    assert_eq!(mv((0, 15), "line_start"), Some((0, 12)));
    assert_eq!(mv((0, 3), "line_end"), Some((0, 11)));
    let len0 = d.laid[0].chars.len();
    let len1 = d.laid[1].chars.len();
    assert_eq!(mv((0, len0), "right"), Some((1, 0)));
    assert_eq!(mv((1, 0), "left"), Some((0, len0)));
    assert_eq!(mv((0, 4), "doc_end"), Some((1, len1)));
    assert_eq!(mv((1, 4), "doc_start"), Some((0, 0)));
    let down = mv((0, 2), "down").unwrap();
    assert!(down.0 == 0 && (13..=15).contains(&down.1), "{:?}", down);
    assert_eq!(mv((0, 14), "down").map(|p| p.0), Some(1));
    assert_eq!(mv((1, 2), "up").map(|p| (p.0, p.1 >= 12)), Some((0, true)));
    assert_eq!(mv((0, 2), "up"), None, "nothing above the first line");
}

#[test]
fn selections_paint_glyph_boxes_and_copy_in_reading_order() {
    let d = open(&text_doc());
    let r = d.selection_rects(&sel(tp(0, 0), tp(0, 5)));
    assert_eq!(r.len(), 1);
    let c5 = d.caret(&tp(0, 5)).unwrap();
    assert!((r[0].1.x - 72.0).abs() < 0.01 && (r[0].1.right() - c5.x).abs() < 0.01 && (r[0].1.h - 24.0).abs() < 0.01, "{:?}", r);
    assert_eq!(d.selection_rects(&sel(tp(0, 6), tp(0, 18))).len(), 2, "two lines, two boxes");
    let across = d.selection_rects(&sel(tp(1, 4), tp(0, 12)));
    assert_eq!(across.iter().map(|(p, _)| *p).collect::<Vec<_>>(), vec![0, 1]);
    assert!(d.selection_rects(&Selection::caret(tp(0, 3))).is_empty());

    assert_eq!(d.copy(&sel(tp(0, 6), tp(0, 18))).text, "World\nSecond");
    assert_eq!(d.copy(&sel(tp(0, 18), tp(0, 6))).text, "World\nSecond", "a backwards selection copies the same");
    assert_eq!(d.copy(&sel(tp(0, 12), tp(1, 4))).text, "Second line here\n\nPage");
    let all = sel(d.step(&tp(0, 3), "doc_start").unwrap(), d.step(&tp(0, 3), "doc_end").unwrap());
    assert_eq!(d.copy(&all).text, "Hello World\nSecond line here\n\nPage two text");

    let st = d.state(&sel(tp(0, 0), tp(0, 5)));
    assert_eq!(st.get("page").and_then(Json::as_usize), Some(0));
    assert_eq!(st.get("selected").and_then(Json::as_usize), Some(5));
    assert_eq!(st.get("width").and_then(Json::as_f64), Some(612.0));
    assert_eq!(st.get("height").and_then(Json::as_f64), Some(792.0));
    assert_eq!(st.get("page_count").and_then(Json::as_usize), Some(2));
    assert!(st.get("link").unwrap().is_null());
    assert_eq!(st.get("can_copy").and_then(Json::as_bool), Some(true));
}

#[test]
fn find_goes_forward_and_back_wrapping_round() {
    let mut bus = Bus::new(open(&text_doc()));
    let find = |bus: &mut Bus<FolioDoc>, q: &str, next: bool, s: Option<Selection>| {
        let mut args = Json::obj().with("query", q).with("next", next);
        if let Some(s) = s {
            args.set("sel", s.to_json());
        }
        let out = bus.do_user(cmd("folio.find", args)).unwrap();
        assert_eq!(out.get("step").and_then(Json::as_usize), Some(0), "a find leaves no undo step");
        landed(&out)
    };
    assert_eq!(find(&mut bus, "e", true, None), Some((0, 1, 0, 2)));
    assert_eq!(find(&mut bus, "e", true, Some(sel(tp(0, 1), tp(0, 2)))), Some((0, 13, 0, 14)));
    assert_eq!(find(&mut bus, "e", false, Some(sel(tp(0, 13), tp(0, 14)))), Some((0, 1, 0, 2)));
    assert_eq!(find(&mut bus, "e", false, Some(sel(tp(0, 1), tp(0, 2)))), Some((1, 10, 1, 11)), "back past the start wraps to the end");
    assert_eq!(find(&mut bus, "e", true, Some(sel(tp(1, 10), tp(1, 11)))), Some((0, 1, 0, 2)), "on past the end wraps to the start");
    assert_eq!(find(&mut bus, "LINE", true, None), Some((0, 19, 0, 23)), "any case");
    assert_eq!(find(&mut bus, "world second", true, None), Some((0, 6, 0, 18)), "a space finds a line break");
    assert_eq!(find(&mut bus, "nowhere", true, None), None);
    assert!(bus.do_user(cmd("folio.find", Json::obj().with("query", " "))).is_err());
    assert!(bus.history().is_empty());
}

// ---------------------------------------------------------------------------------------------------
// The ways around
// ---------------------------------------------------------------------------------------------------

fn nav_doc() -> Vec<u8> {
    pages_pdf(
        &[
            "BT /F1 12 Tf 72 605 Td (Go to the end) Tj 0 -100 Td (Visit the site) Tj ET",
            "BT /F1 12 Tf 72 700 Td (Middle) Tj ET",
            "BT /F1 12 Tf 72 700 Td (The end) Tj ET",
        ],
        &["/Annots [10 0 R 11 0 R]"],
        "/Outlines 12 0 R /Names << /Dests << /Names [(sec) [6 0 R /Fit]] >> >>",
        vec![
            b("<< /Type /Annot /Subtype /Link /Rect [72 600 200 620] /Dest [8 0 R /Fit] >>"),
            b("<< /Type /Annot /Subtype /Link /Rect [72 500 200 520] /A << /S /URI /URI (https://example.org/) >> >>"),
            b("<< /Type /Outlines /First 13 0 R /Last 15 0 R /Count 3 >>"),
            b("<< /Title (One) /Parent 12 0 R /Next 15 0 R /Dest [4 0 R /Fit] /First 14 0 R /Last 14 0 R /Count 1 >>"),
            b("<< /Title (One a) /Parent 13 0 R /Dest (sec) >>"),
            b("<< /Title (Three) /Parent 12 0 R /Prev 13 0 R /A << /S /GoTo /D [8 0 R /Fit] >> >>"),
        ],
        "",
    )
}

#[test]
fn pages_links_and_bookmarks_turn_the_page_without_changing_it() {
    let mut bus = Bus::new(open(&nav_doc()));
    let caret0 = Selection::caret(tp(0, 0)).to_json();
    let go = |bus: &mut Bus<FolioDoc>, name: &str, args: Json| landed(&bus.do_user(cmd(name, args)).unwrap()).map(|l| (l.0, l.1));
    assert_eq!(go(&mut bus, "folio.go_to_page", Json::obj().with("page", 2)), Some((2, 0)));
    assert_eq!(go(&mut bus, "folio.go_to_page", Json::obj().with("to", "next").with("sel", caret0.clone())), Some((1, 0)));
    assert_eq!(go(&mut bus, "folio.go_to_page", Json::obj().with("to", "previous").with("sel", caret0.clone())), Some((0, 0)));
    assert_eq!(go(&mut bus, "folio.go_to_page", Json::obj().with("to", "last")), Some((2, 0)));
    assert_eq!(go(&mut bus, "folio.go_to_page", Json::obj().with("to", "first").with("from", 2)), Some((0, 0)));
    assert!(bus.do_user(cmd("folio.go_to_page", Json::obj().with("page", 9))).is_err());

    // Links: the internal one turns to its page; the web one only says where it goes.
    assert_eq!(go(&mut bus, "folio.follow_link", Json::obj().with("page", 0).with("x", 100.0).with("y", 180.0)), Some((2, 0)));
    assert_eq!(go(&mut bus, "folio.follow_link", Json::obj().with("sel", Selection::caret(tp(0, 2)).to_json())), Some((2, 0)));
    let web = bus.do_user(cmd("folio.follow_link", Json::obj().with("page", 0).with("x", 100.0).with("y", 280.0))).unwrap();
    assert!(web.get("selection").unwrap().is_null());
    let r = bus.door("read", &Json::obj().with("link", Json::obj().with("page", 0).with("x", 100.0).with("y", 280.0))).unwrap();
    assert_eq!(r.to_string(), r#"{"link":{"uri":"https://example.org/"}}"#);
    assert!(bus.do_user(cmd("folio.follow_link", Json::obj().with("page", 0).with("x", 500.0).with("y", 500.0))).is_err());
    let d = bus.doc();
    let visit = d.page_text(0).find("Visit").unwrap();
    assert_eq!(d.state(&Selection::caret(tp(0, visit + 2))).get("link").unwrap().to_string(), r#"{"uri":"https://example.org/"}"#);
    assert_eq!(d.state(&Selection::caret(tp(0, 2))).get("link").unwrap().to_string(), r#"{"page":2}"#);
    let links = bus.door("read", &Json::obj().with("links", 0)).unwrap();
    assert_eq!(links.get("links").and_then(Json::as_arr).map(|a| a.len()), Some(2));

    // Bookmarks: as a tree for the panel, and by index or next / previous.
    let tree = bus.door("read", &Json::obj().with("outline", true)).unwrap();
    assert_eq!(
        tree.to_string(),
        r#"{"outline":[{"index":0,"title":"One","page":0,"children":[{"index":1,"title":"One a","page":1,"children":[]}]},{"index":2,"title":"Three","page":2,"children":[]}]}"#
    );
    assert_eq!(go(&mut bus, "folio.bookmark", Json::obj().with("index", 1)), Some((1, 0)));
    assert_eq!(go(&mut bus, "folio.bookmark", Json::obj().with("to", "next").with("sel", caret0.clone())), Some((1, 0)));
    assert_eq!(go(&mut bus, "folio.bookmark", Json::obj().with("to", "previous").with("from", 2)), Some((1, 0)));
    assert!(bus.do_user(cmd("folio.bookmark", Json::obj().with("index", 7))).is_err());
    assert!(bus.history().is_empty(), "the ways around leave no undo steps");
    assert!(!bus.doc().is_modified());
}

// ---------------------------------------------------------------------------------------------------
// Organising pages
// ---------------------------------------------------------------------------------------------------

/// Three pages in a nested tree: A and B under an inner node that carries their resources; C with
/// its own /Rotate 90.
fn tree_doc() -> Vec<u8> {
    pdf(
        &[
            b("<< /Type /Catalog /Pages 2 0 R >>"),
            b("<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 3 /MediaBox [0 0 612 792] >>"),
            b("<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 5 0 R] /Count 2 /Resources << /Font << /F1 7 0 R >> >> >>"),
            b("<< /Type /Page /Parent 3 0 R /Contents 8 0 R >>"),
            b("<< /Type /Page /Parent 3 0 R /Contents 9 0 R >>"),
            b("<< /Type /Page /Parent 2 0 R /Contents 10 0 R /Resources << /Font << /F1 7 0 R >> >> /Rotate 90 >>"),
            b("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
            stream("", b"BT /F1 20 Tf 72 700 Td (Alpha) Tj ET"),
            stream("", b"BT /F1 20 Tf 72 700 Td (Bravo) Tj ET"),
            stream("", b"BT /F1 20 Tf 72 700 Td (Charlie) Tj ET"),
        ],
        "",
    )
}

#[test]
fn deleting_pages_is_an_appended_update_and_undoes() {
    let original = tree_doc();
    let mut bus = Bus::new(open(&original));
    let out = bus.do_user(cmd("folio.delete_pages", Json::obj().with("pages", Json::Arr(vec![Json::from(1usize)])))).unwrap();
    assert!(out.get("step").and_then(Json::as_usize).unwrap() > 0);
    assert_eq!(landed(&out).map(|l| (l.0, l.1)), Some((1, 0)));
    let d = bus.doc();
    assert_eq!(d.page_count(), 2);
    assert_eq!((d.page_text(0).as_str(), d.page_text(1).as_str()), ("Alpha", "Charlie"));
    let saved = d.save();
    assert!(saved.starts_with(&original), "the original bytes stay a prefix");
    let back = open(&saved);
    assert_eq!(back.page_count(), 2);
    assert_eq!((back.page_text(0).as_str(), back.page_text(1).as_str()), ("Alpha", "Charlie"));
    assert_eq!(back.file.get(2).as_dict().unwrap().get("Count"), Some(&Obj::Int(2)));
    assert_eq!(back.file.get(3).as_dict().unwrap().get("Count"), Some(&Obj::Int(1)));
    assert_eq!(back.file.get(5).as_dict().unwrap().get("Contents"), Some(&Obj::Ref(9, 0)), "the deleted page object stays in the file");

    bus.undo().unwrap();
    assert_eq!(bus.doc().page_count(), 3);
    assert_eq!(bus.doc().save(), original, "undone, nothing is left to save");
    bus.redo().unwrap();
    assert_eq!(bus.doc().page_count(), 2);
    bus.undo().unwrap();

    // The ribbon's button: the page the caret is on.
    let out = bus.do_user(cmd("folio.delete_pages", Json::obj().with("sel", Selection::caret(tp(2, 0)).to_json()))).unwrap();
    assert_eq!(landed(&out).map(|l| l.0), Some(1));
    assert_eq!((bus.doc().page_text(0).as_str(), bus.doc().page_text(1).as_str()), ("Alpha", "Bravo"));
    bus.undo().unwrap();

    let all = Json::obj().with("pages", Json::Arr(vec![0usize.into(), 1usize.into(), 2usize.into()]));
    assert!(bus.do_user(cmd("folio.delete_pages", all)).unwrap_err().to_string().contains("at least one page"));
    assert!(bus.do_user(cmd("folio.delete_pages", Json::obj().with("pages", Json::Arr(vec![5usize.into()])))).is_err());
    assert_eq!(bus.doc().page_count(), 3);
}

#[test]
fn rotating_pages_writes_their_rotate_and_undoes() {
    let original = tree_doc();
    let mut bus = Bus::new(open(&original));
    let pages = Json::Arr(vec![0usize.into(), 2usize.into()]);
    bus.do_user(cmd("folio.rotate_pages", Json::obj().with("pages", pages).with("angle", 90))).unwrap();
    let d = bus.doc();
    assert_eq!((d.page_info(0).unwrap().rotate, d.page_info(1).unwrap().rotate, d.page_info(2).unwrap().rotate), (90, 0, 180));
    assert_eq!(d.page_size(0), Some((792.0, 612.0)));
    assert_eq!(d.page_text(0), "Alpha", "the text still reads after the turn");
    let saved = d.save();
    assert!(saved.starts_with(&original));
    let back = open(&saved);
    assert_eq!((back.page_info(0).unwrap().rotate, back.page_info(2).unwrap().rotate), (90, 180));
    bus.do_user(cmd("folio.rotate_pages", Json::obj().with("page", 2).with("angle", 180))).unwrap();
    assert_eq!(bus.doc().page_info(2).unwrap().rotate, 0);
    bus.undo().unwrap();
    bus.undo().unwrap();
    assert_eq!(bus.doc().save(), original);
    assert!(bus.do_user(cmd("folio.rotate_pages", Json::obj().with("page", 0).with("angle", 45))).is_err());
}

// ---------------------------------------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------------------------------------

fn form_doc() -> Vec<u8> {
    let on = stream("/Type /XObject /Subtype /Form /BBox [0 0 15 15]", b"0 g 2 2 11 11 re f");
    let off = stream("/Type /XObject /Subtype /Form /BBox [0 0 15 15]", b"");
    let dot = stream("/Type /XObject /Subtype /Form /BBox [0 0 12 12]", b"0 g 3 3 6 6 re f");
    pdf(
        &[
            b("<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>"),
            b("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>"),
            b("<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R /Annots [7 0 R 8 0 R 10 0 R 11 0 R 12 0 R 13 0 R] >>"),
            stream("", b"BT /F1 12 Tf 72 750 Td (Application form) Tj ET"),
            b("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"),
            b("<< /Fields [7 0 R 8 0 R 9 0 R 12 0 R 13 0 R] /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> /NeedAppearances true >>"),
            b("<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V (Old) /Rect [100 600 300 620] /DA (/Helv 12 Tf 0 g) >>"),
            b("<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) /V /Off /AS /Off /Rect [100 560 115 575] /AP << /N << /Yes 14 0 R /Off 15 0 R >> >> >>"),
            b("<< /FT /Btn /Ff 49152 /T (size) /V /Off /Kids [10 0 R 11 0 R] >>"),
            b("<< /Type /Annot /Subtype /Widget /Parent 9 0 R /Rect [100 500 112 512] /AS /Off /AP << /N << /S 16 0 R /Off 15 0 R >> >> >>"),
            b("<< /Type /Annot /Subtype /Widget /Parent 9 0 R /Rect [130 500 142 512] /AS /Off /AP << /N << /L 16 0 R /Off 15 0 R >> >> >>"),
            b("<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 131072 /T (country) /Opt [(MY) [(SG) (Singapore)]] /Rect [100 450 250 470] /DA (/Helv 10 Tf 0 g) >>"),
            b("<< /Type /Annot /Subtype /Widget /FT /Tx /Ff 4096 /T (notes) /Q 1 /Rect [100 300 300 400] >>"),
            on,
            off,
            dot,
        ],
        "",
    )
}

fn field<'a>(d: &'a FolioDoc, name: &str) -> &'a Field {
    d.fields().iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no field {}", name))
}

#[test]
fn form_fields_read_with_their_widgets() {
    let d = open(&form_doc());
    let names: Vec<(&str, FieldKind)> = d.fields().iter().map(|f| (f.name.as_str(), f.kind)).collect();
    assert_eq!(
        names,
        vec![("name", FieldKind::Text), ("agree", FieldKind::Check), ("size", FieldKind::Radio), ("country", FieldKind::Combo), ("notes", FieldKind::Text)]
    );
    assert_eq!(field(&d, "name").value, "Old");
    assert_eq!(field(&d, "agree").value, "Off");
    assert_eq!(field(&d, "size").widgets.iter().map(|w| w.on.clone().unwrap()).collect::<Vec<_>>(), vec!["S", "L"]);
    assert_eq!(field(&d, "country").options, vec![("MY".to_string(), "MY".to_string()), ("SG".to_string(), "Singapore".to_string())]);
    assert!(field(&d, "notes").multiline());
    assert_eq!(field(&d, "name").widgets[0].page, Some(0));
    // NeedAppearances: the text field shows its value though it has no appearance of its own.
    assert!(d.page_text(0).contains("Old"), "{}", d.page_text(0));
    let r = d.read(&Json::obj().with("fields", true)).unwrap();
    let first = r.get("fields").unwrap().at(0).unwrap();
    assert_eq!(first.get("kind").and_then(Json::as_str), Some("text"));
    assert_eq!(first.get("widgets").unwrap().at(0).unwrap().get("rect").unwrap().to_string(), "[100,172,200,20]");
    assert_eq!(d.describe().get("fields").and_then(Json::as_usize), Some(5));
}

#[test]
fn filling_fields_writes_values_and_appearances_and_undoes() {
    let original = form_doc();
    let mut bus = Bus::new(open(&original));
    let set = |bus: &mut Bus<FolioDoc>, name: &str, value: Json| bus.do_user(cmd("folio.set_field", Json::obj().with("name", name).with("value", value)));
    set(&mut bus, "name", "Kiyo Software".into()).unwrap();
    set(&mut bus, "agree", true.into()).unwrap();
    set(&mut bus, "size", "L".into()).unwrap();
    set(&mut bus, "country", "Singapore".into()).unwrap();
    set(&mut bus, "notes", "first line\nsecond line".into()).unwrap();
    assert!(set(&mut bus, "country", "Mars".into()).unwrap_err().to_string().contains("Singapore"));
    assert!(set(&mut bus, "nobody", "x".into()).unwrap_err().to_string().contains("name, agree"));
    let d = bus.doc();
    assert_eq!(field(d, "name").value, "Kiyo Software");
    assert_eq!(field(d, "agree").value, "Yes");
    assert_eq!(field(d, "size").value, "L");
    assert_eq!(field(d, "country").value, "SG");
    assert_eq!(d.file.get(11).as_dict().unwrap().get("AS"), Some(&Obj::name("L")));
    assert_eq!(d.file.get(10).as_dict().unwrap().get("AS"), Some(&Obj::name("Off")));
    assert_eq!(d.file.get(8).as_dict().unwrap().get("AS"), Some(&Obj::name("Yes")));
    let text = d.page_text(0);
    for want in ["Kiyo Software", "Singapore", "first line", "second line"] {
        assert!(text.contains(want), "{} in {}", want, text);
    }
    assert!(!text.contains("Old"));

    // Saved as an appended update, read back the same.
    let saved = d.save();
    assert!(saved.starts_with(&original));
    let back = open(&saved);
    assert_eq!(field(&back, "name").value, "Kiyo Software");
    assert_eq!(field(&back, "agree").value, "Yes");
    assert_eq!(field(&back, "size").value, "L");
    assert_eq!(field(&back, "country").value, "SG");
    assert_eq!(field(&back, "notes").value, "first line\nsecond line");
    assert!(field(&back, "name").widgets[0].has_ap, "the text field now has its own appearance");
    assert!(back.page_text(0).contains("Kiyo Software"));

    for _ in 0..5 {
        bus.undo().unwrap();
    }
    assert_eq!(field(bus.doc(), "name").value, "Old");
    assert_eq!(bus.doc().save(), original, "every fill undone, nothing left to save");
}

#[test]
fn typing_into_a_clicked_field_fills_it() {
    let mut bus = Bus::new(open(&form_doc()));
    // Click the text field (display 100..300 x 172..192).
    let pos = bus.doc().hit(0, 150.0, 180.0).unwrap();
    assert_eq!(pos.json().to_string(), r#"{"page":0,"field":"name","widget":7}"#);
    let caret = bus.doc().caret(&pos).unwrap();
    assert!(caret.x > 100.0 && caret.y >= 172.0 && caret.y + caret.height <= 192.0, "{:?}", caret);
    let at_field = Selection::caret(pos.clone()).to_json();
    let ty = |bus: &mut Bus<FolioDoc>, t: &str| bus.do_user(cmd("folio.insert_text", Json::obj().with("sel", at_field.clone()).with("text", t))).unwrap();
    let out = ty(&mut bus, "A");
    assert_eq!(out.get("selection").unwrap().get("focus").unwrap().get("field").and_then(Json::as_str), Some("name"));
    let objects = bus.doc().file.overlay.len();
    ty(&mut bus, "B");
    assert_eq!(bus.doc().file.overlay.len(), objects, "typing reuses the appearance it made");
    assert_eq!(field(bus.doc(), "name").value, "OldAB");
    let out = ty(&mut bus, "\n");
    assert_eq!(out.get("step").and_then(Json::as_usize), Some(0), "a line break in a one-line field changes nothing");
    bus.do_user(cmd("folio.backspace", Json::obj().with("sel", at_field.clone()))).unwrap();
    assert_eq!(field(bus.doc(), "name").value, "OldA");
    assert_eq!(bus.doc().copy(&Selection::caret(pos)).text, "OldA");

    // A space ticks a check box; letters pick a choice.
    let agree = bus.doc().hit(0, 107.0, 225.0).unwrap();
    assert_eq!(agree.json().get("field").and_then(Json::as_str), Some("agree"));
    bus.do_user(cmd("folio.insert_text", Json::obj().with("sel", Selection::caret(agree.clone()).to_json()).with("text", " "))).unwrap();
    assert_eq!(field(bus.doc(), "agree").value, "Yes");
    bus.do_user(cmd("folio.insert_text", Json::obj().with("sel", Selection::caret(agree).to_json()).with("text", " "))).unwrap();
    assert_eq!(field(bus.doc(), "agree").value, "Off");
    let country = bus.doc().hit(0, 120.0, 330.0).unwrap();
    bus.do_user(cmd("folio.insert_text", Json::obj().with("sel", Selection::caret(country).to_json()).with("text", "s"))).unwrap();
    assert_eq!(field(bus.doc(), "country").value, "SG");
    let radio = bus.doc().hit(0, 106.0, 285.0).unwrap();
    bus.do_user(cmd("folio.insert_text", Json::obj().with("sel", Selection::caret(radio).to_json()).with("text", " "))).unwrap();
    assert_eq!(field(bus.doc(), "size").value, "S");

    // Tab walks the fields; typing on the page's own text is refused plainly.
    let out = bus.do_user(cmd("folio.next_field", Json::obj().with("sel", at_field.clone()))).unwrap();
    assert_eq!(out.get("selection").unwrap().get("focus").unwrap().get("field").and_then(Json::as_str), Some("agree"));
    let out = bus.do_user(cmd("folio.next_field", Json::obj().with("sel", at_field).with("back", true))).unwrap();
    assert_eq!(out.get("selection").unwrap().get("focus").unwrap().get("field").and_then(Json::as_str), Some("notes"));
    let err = bus.do_user(cmd("folio.insert_text", Json::obj().with("sel", Selection::caret(tp(0, 0)).to_json()).with("text", "x"))).unwrap_err();
    assert!(err.to_string().contains("form fields"));
}

// ---------------------------------------------------------------------------------------------------
// Locked files
// ---------------------------------------------------------------------------------------------------

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02X}", x)).collect()
}

/// A one-page file locked with our own RC4 / AES: content "Secret words" (object 4) and the info
/// title (object 6) encrypted, the /Encrypt dictionary as object 7.
fn locked_doc(r: i64, user: &str, owner: &str, p: i32) -> Vec<u8> {
    let id0 = b"0123456789ABCDEF";
    let (enc, key, method) = match r {
        2 | 3 => {
            let (o, u, key) = crypt::make_r2_4(user, owner, p, id0, r, 16, true);
            let (v, len) = if r == 2 { (1, 40) } else { (2, 128) };
            (format!("<< /Filter /Standard /V {} /R {} /Length {} /O <{}> /U <{}> /P {} >>", v, r, len, hex(&o), hex(&u), p), key, Method::Rc4)
        }
        4 => {
            let (o, u, key) = crypt::make_r2_4(user, owner, p, id0, 4, 16, true);
            (
                format!(
                    "<< /Filter /Standard /V 4 /R 4 /Length 128 /CF << /StdCF << /CFM /AESV2 /Length 16 /AuthEvent /DocOpen >> >> /StmF /StdCF /StrF /StdCF /O <{}> /U <{}> /P {} >>",
                    hex(&o),
                    hex(&u),
                    p
                ),
                key,
                Method::AesV2,
            )
        }
        _ => {
            let key = [5u8; 32];
            let (o, u, oe, ue, perms) = crypt::make_r6(user, owner, p, &key);
            (
                format!(
                    "<< /Filter /Standard /V 5 /R 6 /Length 256 /CF << /StdCF << /CFM /AESV3 /Length 32 >> >> /StmF /StdCF /StrF /StdCF /O <{}> /U <{}> /OE <{}> /UE <{}> /Perms <{}> /P {} >>",
                    hex(&o),
                    hex(&u),
                    hex(&oe),
                    hex(&ue),
                    hex(&perms),
                    p
                ),
                key.to_vec(),
                Method::AesV3,
            )
        }
    };
    let content = crypt::lock_with(&key, method, 4, 0, b"BT /F1 24 Tf 72 700 Td (Secret words) Tj ET");
    let title = crypt::lock_with(&key, method, 6, 0, b"Locked title");
    pdf(
        &[
            b("<< /Type /Catalog /Pages 2 0 R >>"),
            b("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>"),
            b("<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"),
            stream("", &content),
            b("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"),
            b(&format!("<< /Title <{}> >>", hex(&title))),
            b(&enc),
        ],
        &format!("/Info 6 0 R /Encrypt 7 0 R /ID [<{}> <{}>]", hex(id0), hex(id0)),
    )
}

#[test]
fn locked_files_open_silently_with_an_empty_user_password() {
    for r in [2, 3, 4, 6] {
        let bytes = locked_doc(r, "", "owner", -4);
        let d = FolioDoc::open(bytes).unwrap_or_else(|e| panic!("R{}: {}", r, e));
        assert_eq!(d.page_text(0), "Secret words", "R{}", r);
        assert_eq!(d.title().as_deref(), Some("Locked title"), "R{}", r);
        let lock = d.describe().get("locked").cloned().unwrap();
        assert_eq!(lock.get("revision").and_then(Json::as_i64), Some(r));
        assert_eq!(lock.get("owner").and_then(Json::as_bool), Some(false));
    }
}

#[test]
fn a_user_password_is_asked_for_and_checked() {
    for r in [3, 4, 6] {
        let bytes = locked_doc(r, "user pw", "owner pw", -4);
        let err = FolioDoc::open(bytes.clone()).unwrap_err();
        assert_eq!(err, FolioError::NeedsPassword, "R{}", r);
        assert!(err.to_string().contains("password"));
        assert_eq!(open_with_password(bytes.clone(), "nope").unwrap_err(), FolioError::WrongPassword);
        let d = open_with_password(bytes.clone(), "user pw").unwrap();
        assert_eq!(d.page_text(0), "Secret words");
        let o = open_with_password(bytes, "owner pw").unwrap();
        assert_eq!(o.describe().get("locked").unwrap().get("owner").and_then(Json::as_bool), Some(true));
    }
}

#[test]
fn edits_to_a_locked_file_are_locked_with_the_same_key() {
    for r in [3, 4, 6] {
        let original = locked_doc(r, "user pw", "", -4);
        let mut bus = Bus::new(open_with_password(original.clone(), "user pw").unwrap());
        bus.do_user(cmd("folio.set_title", Json::obj().with("title", "Changed title"))).unwrap();
        bus.do_user(cmd("folio.add_note", Json::obj().with("page", 0).with("x", 10.0).with("y", 10.0).with("text", "Hidden note"))).unwrap();
        let saved = bus.doc().save();
        assert!(saved.starts_with(&original));
        let tail = &saved[original.len()..];
        let has = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
        assert!(!has(tail, b"Changed title") && !has(tail, b"Hidden note"), "R{}: the update is locked too", r);
        assert!(has(tail, b"/Encrypt 7 0 R"), "R{}: the update's trailer keeps the lock", r);
        let back = open_with_password(saved, "user pw").unwrap();
        assert_eq!(back.title().as_deref(), Some("Changed title"), "R{}", r);
        assert_eq!(back.annotations(0)[0].contents, "Hidden note");
        assert_eq!(back.page_text(0), "Secret words");
    }
}

#[test]
fn a_locked_files_limits_are_honoured() {
    // No copying (bit 5) and no organising (bit 11), opened without the owner password.
    let p = -4 & !16 & !1024;
    let bytes = locked_doc(4, "", "owner", p);
    let mut bus = Bus::new(FolioDoc::open(bytes.clone()).unwrap());
    let all = sel(tp(0, 0), tp(0, 6));
    assert_eq!(bus.doc().copy(&all).text, "", "copying is not allowed");
    assert_eq!(bus.doc().state(&all).get("can_copy").and_then(Json::as_bool), Some(false));
    let err = bus.do_user(cmd("folio.rotate_pages", Json::obj().with("page", 0).with("angle", 90))).unwrap_err();
    assert!(err.to_string().contains("owner"), "{}", err);
    // The owner may do everything.
    let mut owner = Bus::new(open_with_password(bytes, "owner").unwrap());
    assert_eq!(owner.doc().copy(&all).text, "Secret");
    owner.do_user(cmd("folio.rotate_pages", Json::obj().with("page", 0).with("angle", 90))).unwrap();
}
