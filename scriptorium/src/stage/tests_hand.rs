//! Stage by hand: the caret, typing, formatting, inserting, arranging and the slide list, on the
//! invented deck of `tests.rs` (no fonts: every glyph is half an em wide, so layout is predictable).

use super::*;
use crate::ground::bus::Bus;
use crate::ground::opc;
use super::super::edit::{el_at, find_shape};
use crate::ground::edit::{Editing, Pos, Selection};
use crate::ground::picture::png;

fn bus() -> Bus<StageDoc> {
    Bus::new(open(&deck_bytes()))
}

fn tpos(slide: usize, shape: u32, para: usize, ch: usize) -> Json {
    Json::obj().with("slide", slide).with("shape", shape).with("para", para).with("char", ch)
}

fn caret(slide: usize, shape: u32, para: usize, ch: usize) -> Json {
    Json::obj().with("anchor", tpos(slide, shape, para, ch))
}

fn range(slide: usize, shape: u32, a: (usize, usize), b: (usize, usize)) -> Json {
    Json::obj().with("anchor", tpos(slide, shape, a.0, a.1)).with("focus", tpos(slide, shape, b.0, b.1))
}

fn shapes(slide: usize, ids: &[u32]) -> Json {
    let p = Json::obj().with("slide", slide).with("shape", ids[0]).with("shapes", Json::Arr(ids.iter().map(|i| Json::from(*i)).collect()));
    Json::obj().with("anchor", p.clone()).with("focus", p)
}

fn run(b: &mut Bus<StageDoc>, name: &str, args: Json) -> Json {
    b.do_user(Command::new(name, args)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn text(b: &Bus<StageDoc>, slide: usize, shape: u32) -> String {
    b.doc().body_of(&editing::TextPos { slide, shape, cell: None, at: text::At::new(0, 0) }).map(|t| t.text()).unwrap_or_default()
}

fn slide_xml(b: &Bus<StageDoc>, slide: usize) -> String {
    let part = b.doc().deck().slides[slide].part.clone();
    String::from_utf8(b.doc().package().part_bytes(&part).unwrap()).unwrap()
}

fn landed(done: &Json) -> Json {
    done.get("selection").cloned().unwrap_or(Json::Null)
}

fn png_b64() -> String {
    snap::b64_encode(&png::encode_rgba8(4, 2, &[200u8; 32]))
}

#[test]
fn the_caret_lands_in_text_and_shapes_are_selected_whole() {
    let doc = open(&deck_bytes());
    assert!(doc.editing().is_some());
    // The title (layout box 72,36 576x108): its text takes the caret.
    let p = doc.hit(0, 360.0, 90.0).expect("a place in the title");
    assert_eq!(p.json().get("shape").and_then(Json::as_i64), Some(2));
    assert!(p.json().get("para").is_some(), "a text place: {}", p.json());
    // The red box (100,100 200x100) has no text: the shape as a whole.
    let b = doc.hit(0, 150.0, 140.0).unwrap();
    assert_eq!(b.json().to_string(), r#"{"slide":0,"shape":4}"#);
    let blank = doc.hit(0, 5.0, 530.0).unwrap();
    assert_eq!(blank.json().to_string(), r#"{"slide":0}"#, "beside the shapes: the slide, nothing selected");
    let bs = Selection::caret(blank.clone());
    assert_eq!(doc.state(&bs).get("kind").and_then(Json::as_str), Some("blank"));
    assert!(doc.caret(&blank).is_some_and(|c| c.height == 0.0));
    assert!(doc.selection_rects(&bs).is_empty());
    // The caret sits inside the title box, a line tall.
    let c = doc.caret(&Pos(tpos(0, 2, 0, 0))).unwrap();
    assert_eq!(c.page, 0);
    assert!(c.x >= 72.0 && c.x <= 648.0 && c.y >= 36.0 && c.y <= 144.0 && c.height > 10.0, "{:?}", c);
    let c5 = doc.caret(&Pos(tpos(0, 2, 0, 5))).unwrap();
    assert!(c5.x > c.x, "five characters on");
    // Steps: right, word right, line end, doc end, and back.
    let st = |p: Json, m: &str| doc.step(&Pos(p), m).map(|p| p.json().get("char").and_then(Json::as_usize).unwrap());
    assert_eq!(st(tpos(0, 2, 0, 0), "right"), Some(1));
    assert_eq!(st(tpos(0, 2, 0, 0), "word_right"), Some(6));
    assert_eq!(st(tpos(0, 2, 0, 3), "line_end"), Some(11));
    assert_eq!(st(tpos(0, 2, 0, 3), "line_start"), Some(0));
    assert_eq!(st(tpos(0, 3, 0, 4), "doc_end"), Some(11));
    let down = doc.step(&Pos(tpos(0, 3, 0, 2)), "down").unwrap();
    assert_eq!(down.json().get("para").and_then(Json::as_usize), Some(1), "down goes to the next paragraph");
    let left = doc.step(&Pos(tpos(0, 3, 1, 0)), "left").unwrap();
    assert_eq!(left.to_json().to_string(), tpos(0, 3, 0, 11).to_string());
    // Selection boxes: a text range, and a shape's box with handles.
    let rects = doc.selection_rects(&Selection::from_json(&range(0, 2, (0, 0), (0, 5))).unwrap());
    assert_eq!(rects.len(), 1);
    assert!(rects[0].1.w > 0.0);
    let boxes = doc.selection_rects(&Selection::from_json(&shapes(0, &[4])).unwrap());
    assert_eq!(boxes.len(), 12, "four edges and eight handles");
    // State: the title is bold (layout list style), 40 pt, centred.
    let st = doc.state(&Selection::from_json(&caret(0, 2, 0, 3)).unwrap());
    assert_eq!(st.get("bold").and_then(Json::as_bool), Some(true));
    assert_eq!(st.get("size").and_then(Json::as_f64), Some(40.0));
    assert_eq!(st.get("align").and_then(|a| a.get("center")).and_then(Json::as_bool), Some(true));
    let sh = doc.state(&Selection::from_json(&shapes(0, &[4])).unwrap());
    assert_eq!(sh.get("fill").and_then(Json::as_str), Some("FF0000"));
    assert_eq!(sh.get("count").and_then(Json::as_usize), Some(1));
    // Copy: text with its formatting, and shapes.
    let clip = doc.copy(&Selection::from_json(&range(0, 3, (0, 6), (1, 6))).unwrap());
    assert_eq!(clip.text, "point\nSecond");
    assert_eq!(clip.rich.as_ref().and_then(|r| r.get("kind")).and_then(Json::as_str), Some("stage.text"));
    assert!(doc.pdf_resources().is_some());
}

#[test]
fn typing_enter_and_delete_with_exact_undo() {
    let mut b = bus();
    let before = slide_xml(&b, 0);
    let done = run(&mut b, "stage.insert_text", Json::obj().with("sel", caret(0, 2, 0, 5)).with("text", ", dear"));
    assert_eq!(text(&b, 0, 2), "Hello, dear Stage");
    assert_eq!(landed(&done).get("focus").and_then(|f| f.get("char")).and_then(Json::as_usize), Some(11));
    assert!(slide_xml(&b, 0).contains(r#"<a:rPr lang="en-GB" dirty="0"/><a:t>Hello, dear Stage</a:t>"#), "typed into the run, its formatting kept: {}", slide_xml(&b, 0));
    // Replace a selection.
    run(&mut b, "stage.insert_text", Json::obj().with("sel", range(0, 2, (0, 0), (0, 5))).with("text", "Hi"));
    assert_eq!(text(&b, 0, 2), "Hi, dear Stage");
    // Enter and Shift+Enter.
    let done = run(&mut b, "stage.enter", Json::obj().with("sel", caret(0, 2, 0, 3)));
    assert_eq!(text(&b, 0, 2), "Hi,\n dear Stage");
    assert_eq!(landed(&done).get("focus").unwrap().to_string(), tpos(0, 2, 1, 0).to_string());
    run(&mut b, "stage.enter", Json::obj().with("sel", caret(0, 2, 1, 5)).with("shift", true));
    assert!(slide_xml(&b, 0).contains("<a:br>"), "a line break element");
    // Backspace at a paragraph start joins; Ctrl+Backspace takes a word.
    run(&mut b, "stage.delete", Json::obj().with("sel", caret(0, 2, 1, 0)).with("back", true));
    assert_eq!(text(&b, 0, 2).split('\n').count(), 1);
    let t = text(&b, 0, 2);
    let n = t.chars().count();
    run(&mut b, "stage.delete", Json::obj().with("sel", caret(0, 2, 0, n)).with("back", true).with("word", true));
    assert_eq!(text(&b, 0, 2), t[..t.rfind("Stage").unwrap()].to_string());
    // Undo all the way gives back the exact part.
    while b.can_undo() {
        b.undo().unwrap();
    }
    assert_eq!(slide_xml(&b, 0), before);
    b.redo().unwrap();
    assert_eq!(text(&b, 0, 2), "Hello, dear Stage");
    // A delete with nothing to delete leaves no step.
    let steps = b.history().len();
    run(&mut b, "stage.delete", Json::obj().with("sel", caret(0, 2, 0, 0)).with("back", true));
    assert_eq!(b.history().len(), steps);
    // Typing into an empty shape makes its text body.
    run(&mut b, "stage.insert_text", Json::obj().with("sel", shapes(0, &[4])).with("text", "In the box"));
    assert_eq!(text(&b, 0, 4), "In the box");
    let again = open(&b.doc().save());
    assert_eq!(again.shape_info(0, "Box").unwrap().paragraphs[0].runs[0].text, "In the box");
}

#[test]
fn run_and_paragraph_formatting() {
    let mut b = bus();
    run(&mut b, "stage.bold", Json::obj().with("sel", range(0, 3, (0, 0), (0, 5))));
    assert!(slide_xml(&b, 0).contains(r#"<a:rPr lang="en-US" b="1"/><a:t>First</a:t>"#), "{}", slide_xml(&b, 0));
    let st = b.doc().state(&Selection::from_json(&range(0, 3, (0, 0), (0, 5))).unwrap());
    assert_eq!(st.get("bold").and_then(Json::as_bool), Some(true));
    run(&mut b, "stage.bold", Json::obj().with("sel", range(0, 3, (0, 0), (0, 5))));
    assert!(slide_xml(&b, 0).contains(r#"b="0""#), "toggled off");
    run(&mut b, "stage.format", Json::obj().with("sel", range(0, 3, (1, 0), (1, 6))).with("underline", "double").with("font", "Georgia").with("size", 31.0).with("color", "#00aa11").with("italic", true));
    let xml = slide_xml(&b, 0);
    assert!(xml.contains(r#"u="dbl""#) && xml.contains(r#"sz="3100""#) && xml.contains(r#"<a:solidFill><a:srgbClr val="00AA11"/></a:solidFill><a:latin typeface="Georgia"/>"#), "{xml}");
    let info = b.doc().shape_info(0, "Content 2").unwrap();
    assert!(info.paragraphs[1].runs[0].style.italic);
    run(&mut b, "stage.grow_font", Json::obj().with("sel", range(0, 3, (2, 0), (2, 5))));
    let st = b.doc().state(&Selection::from_json(&range(0, 3, (2, 0), (2, 5))).unwrap());
    assert_eq!(st.get("size").and_then(Json::as_f64), Some(32.0), "28 grows to 32");
    // Paragraphs: align, numbering, bullets off, level, line spacing.
    run(&mut b, "stage.align", Json::obj().with("sel", caret(0, 3, 0, 1)).with("align", "center"));
    assert!(slide_xml(&b, 0).contains(r#"<a:pPr algn="ctr"/>"#));
    run(&mut b, "stage.numbering", Json::obj().with("sel", range(0, 3, (0, 0), (2, 0))));
    let info = b.doc().shape_info(0, "Content 2").unwrap();
    assert_eq!(info.paragraphs[0].bullet.as_ref().unwrap().text, "1.\t");
    run(&mut b, "stage.bullets", Json::obj().with("sel", caret(0, 3, 2, 0)).with("on", false));
    assert!(b.doc().shape_info(0, "Content 2").unwrap().paragraphs[2].bullet.is_none());
    run(&mut b, "stage.tab", Json::obj().with("sel", caret(0, 3, 0, 0)));
    assert_eq!(b.doc().shape_info(0, "Content 2").unwrap().paragraphs[0].level, 1, "Tab at a list paragraph's start demotes it");
    run(&mut b, "stage.tab", Json::obj().with("sel", caret(0, 3, 0, 0)).with("back", true));
    assert_eq!(b.doc().shape_info(0, "Content 2").unwrap().paragraphs[0].level, 0);
    run(&mut b, "stage.line_spacing", Json::obj().with("sel", caret(0, 6, 0, 0)).with("value", 1.5));
    assert!(slide_xml(&b, 0).contains(r#"<a:lnSpc><a:spcPct val="150000"/></a:lnSpc>"#));
    // A whole shape at once.
    run(&mut b, "stage.italic", Json::obj().with("sel", shapes(0, &[6])));
    assert!(b.doc().shape_info(0, "Numbered").unwrap().paragraphs.iter().all(|p| p.runs[0].style.italic));
    // The undo of every step is exact.
    let reopened = open(&b.doc().save());
    assert_eq!(reopened.slide_count(), 2);
    while b.can_undo() {
        b.undo().unwrap();
    }
    assert_eq!(b.doc().save(), deck_bytes(), "everything undone: the deck is its old self, byte for byte");
}

#[test]
fn inserting_text_boxes_shapes_lines_pictures_and_tables() {
    let mut b = bus();
    let done = run(&mut b, "stage.insert_text_box", Json::obj().with("slide", 1).with("x", 20.0).with("y", 400.0).with("w", 200.0).with("h", 20.0).with("text", "Caption"));
    let tb = landed(&done).get("focus").and_then(|f| f.get("shape")).and_then(Json::as_i64).unwrap() as u32;
    assert_eq!(text(&b, 1, tb), "Caption");
    let xml = slide_xml(&b, 1);
    assert!(xml.contains(r#"<p:cNvSpPr txBox="1"/>"#) && xml.contains("<a:spAutoFit/>"));
    // Resize to fit: more lines make the box taller.
    let h0 = b.doc().geom(1, tb).unwrap().h;
    run(&mut b, "stage.insert_text", Json::obj().with("sel", caret(1, tb, 0, 7)).with("text", "\nline two\nline three"));
    let h1 = b.doc().geom(1, tb).unwrap().h;
    assert!(h1 > h0 + 20.0, "spAutoFit grew the box: {h0} -> {h1}");

    let done = run(&mut b, "stage.insert_shape", Json::obj().with("slide", 1).with("preset", "ellipse").with("x", 400.0).with("y", 300.0).with("w", 100.0).with("h", 50.0));
    let el = landed(&done).get("focus").and_then(|f| f.get("shape")).and_then(Json::as_i64).unwrap() as u32;
    assert!(slide_xml(&b, 1).contains(r#"<a:prstGeom prst="ellipse">"#));
    let info = b.doc().shape_info(1, &el.to_string()).unwrap();
    assert!(matches!(info.fill, Some(Fill::Solid(_))), "the theme's accent fill");
    assert!(b.do_user(Command::new("stage.insert_shape", Json::obj().with("slide", 1).with("preset", "nope"))).is_err());

    run(&mut b, "stage.insert_line", Json::obj().with("slide", 1).with("x1", 600.0).with("y1", 100.0).with("x2", 500.0).with("y2", 150.0).with("tail", "triangle"));
    let xml = slide_xml(&b, 1);
    assert!(xml.contains(r#"<a:xfrm flipH="1">"#) && xml.contains(r#"<a:tailEnd type="triangle"/>"#), "{xml}");

    let done = run(&mut b, "stage.insert_picture", Json::obj().with("slide", 1).with("data", png_b64()).with("name", "dots.png"));
    let pic = landed(&done).get("focus").and_then(|f| f.get("shape")).and_then(Json::as_i64).unwrap() as u32;
    let media: Vec<String> = b.doc().package().part_names().into_iter().filter(|n| n.starts_with("/ppt/media/")).collect();
    assert_eq!(media.len(), 2, "a new media part: {:?}", media);
    let info = b.doc().shape_info(1, &pic.to_string()).unwrap();
    assert_eq!(info.kind, "picture");
    assert!(b.doc().display(1).items.iter().filter(|i| matches!(i, Item::Image { .. })).count() >= 2);

    let done = run(&mut b, "stage.insert_table", Json::obj().with("slide", 1).with("rows", 2).with("cols", 3));
    let tbl = landed(&done).get("focus").and_then(|f| f.get("shape")).and_then(Json::as_i64).unwrap() as u32;
    assert!(landed(&done).get("focus").unwrap().get("cell").is_some(), "the caret in the first cell");
    let cell = |r: usize, c: usize, ch: usize| Json::obj().with("anchor", tpos(1, tbl, 0, ch).with("cell", Json::Arr(vec![Json::from(r), Json::from(c)])));
    run(&mut b, "stage.insert_text", Json::obj().with("sel", cell(0, 0, 0)).with("text", "Q1"));
    let tab = run(&mut b, "stage.tab", Json::obj().with("sel", cell(0, 0, 2)));
    assert_eq!(landed(&tab).get("focus").and_then(|f| f.get("cell")).unwrap().to_string(), "[0,1]");
    run(&mut b, "stage.table_insert", Json::obj().with("sel", cell(0, 1, 0)).with("what", "row"));
    run(&mut b, "stage.table_insert", Json::obj().with("sel", cell(0, 1, 0)).with("what", "col").with("after", false));
    let read = b.door("read", &Json::obj().with("slide", 1)).unwrap();
    let t = read.get("shapes").unwrap().as_arr().unwrap().iter().find(|s| s.get("id").and_then(Json::as_i64) == Some(tbl as i64)).unwrap().clone();
    assert_eq!(t.get("text").and_then(Json::as_str), Some("Q1\t\t\t\n\t\t\t\n\t\t\t"), "3 rows x 4 columns");
    run(&mut b, "stage.table_delete", Json::obj().with("sel", cell(1, 0, 0)).with("what", "row"));
    run(&mut b, "stage.table_size", Json::obj().with("sel", cell(0, 0, 0)).with("width", 150.0));
    run(&mut b, "stage.cell_fill", Json::obj().with("sel", cell(0, 0, 0)).with("color", "112233"));
    let xml = slide_xml(&b, 1);
    assert!(xml.contains(r#"<a:gridCol w="1905000"/>"#) && xml.contains(r#"<a:tcPr><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:tcPr>"#), "{xml}");
    // Tab in the last cell adds a row.
    let (rows, cols) = (2, 4);
    run(&mut b, "stage.tab", Json::obj().with("sel", cell(rows - 1, cols - 1, 0)));
    let root = b.doc().deck().sheet(&b.doc().deck().slides[1].part).unwrap().clone();
    let frame = el_at(&root, &find_shape(&root, &ShapeKey::Id(tbl)).unwrap()).unwrap().clone();
    assert_eq!(ops::table_dims(&frame), (3, 4));

    // The file: everything reopens, the PNG's content type is there, other parts untouched.
    let saved = b.doc().save();
    let again = open(&saved);
    assert_eq!(again.read(&Json::obj().with("slide", 1)).unwrap().get("shapes").unwrap().as_arr().unwrap().len(), 12);
    assert_eq!(again.package().content_type("/ppt/media/image2.png"), Some("image/png"));
    let before = Package::open(&deck_bytes()).unwrap();
    for name in before.part_names() {
        if name.contains("slide2.xml") || name.contains("Content_Types") {
            continue;
        }
        assert_eq!(before.part_bytes(&name).unwrap(), again.package().part_bytes(&name).unwrap(), "{name} untouched");
    }
    // Undo the lot: the media part goes with the picture.
    while b.can_undo() {
        b.undo().unwrap();
    }
    assert!(b.doc().package().part("/ppt/media/image2.png").is_none());
    assert_eq!(slide_xml(&b, 1), String::from_utf8(before.part_bytes("/ppt/slides/slide2.xml").unwrap()).unwrap());
}

#[test]
fn a_picture_fills_an_empty_placeholder() {
    let mut b = bus();
    run(&mut b, "stage.new_slide", Json::obj().with("after", 0).with("layout", "Title and Content"));
    // The new slide's content placeholder (id 3) is empty: the picture takes its place, cropped to fill.
    run(&mut b, "stage.insert_picture", Json::obj().with("sel", shapes(1, &[3])).with("data", png_b64()));
    let xml = slide_xml(&b, 1);
    assert!(xml.contains(r#"<p:pic><p:nvPicPr><p:cNvPr id="3""#), "{xml}");
    assert!(xml.contains(r#"<p:ph idx="1"/>"#), "the placeholder kept");
    assert!(xml.contains("<a:srcRect"), "cropped to the placeholder's shape");
    assert!(b.doc().package().content_types().default_for("png").is_some());
    let g = b.doc().geom(1, 3).unwrap();
    assert!(near(g.w, 648.0), "the placeholder's box from the master: {}", g.w);
}

#[test]
fn arranging_shapes() {
    let mut b = bus();
    let order = |b: &Bus<StageDoc>| b.doc().geo[0].iter().map(|g| g.id as i64).collect::<Vec<_>>();
    assert_eq!(order(&b), vec![2, 3, 4, 5, 6]);
    run(&mut b, "stage.order", Json::obj().with("sel", shapes(0, &[4])).with("to", "front"));
    assert_eq!(order(&b), vec![2, 3, 5, 6, 4], "{}", slide_xml(&b, 0));
    run(&mut b, "stage.order", Json::obj().with("sel", shapes(0, &[4])).with("to", "backward"));
    assert_eq!(order(&b), vec![2, 3, 5, 4, 6]);
    let d = run(&mut b, "stage.order", Json::obj().with("sel", shapes(0, &[4])).with("to", "back"));
    assert_eq!(order(&b), vec![4, 2, 3, 5, 6], "{} {}", d, slide_xml(&b, 0));
    // Position, size, nudge, rotate, flip.
    run(&mut b, "stage.position", Json::obj().with("sel", shapes(0, &[4])).with("x", 10.0).with("y", 20.0));
    let bx = b.doc().shape_box(0, 4).unwrap();
    assert!(near(bx.x, 10.0) && near(bx.y, 20.0), "{:?}", bx);
    run(&mut b, "stage.size", Json::obj().with("sel", shapes(0, &[4])).with("w", 100.0).with("lock", true));
    let g = b.doc().geom(0, 4).unwrap();
    assert!(near(g.w, 100.0) && near(g.h, 50.0), "aspect kept: {} x {}", g.w, g.h);
    run(&mut b, "stage.arrow", Json::obj().with("sel", shapes(0, &[4])).with("dir", "right"));
    assert!(near(b.doc().shape_box(0, 4).unwrap().x, 16.0), "nudged 6 pt");
    run(&mut b, "stage.rotate", Json::obj().with("sel", shapes(0, &[4])).with("by", 90.0));
    assert!(slide_xml(&b, 0).contains(r#"rot="5400000""#));
    run(&mut b, "stage.flip", Json::obj().with("sel", shapes(0, &[4])).with("axis", "v"));
    assert!(slide_xml(&b, 0).contains(r#"flipV="1""#));
    // Align and distribute.
    run(&mut b, "stage.align_shapes", Json::obj().with("sel", shapes(0, &[5, 6])).with("how", "top"));
    let (b5, b6) = (b.doc().shape_box(0, 5).unwrap(), b.doc().shape_box(0, 6).unwrap());
    assert!(near(b5.y.min(b6.y), 100.0) && near(b5.y, b6.y), "{:?} {:?}", b5, b6);
    run(&mut b, "stage.align_shapes", Json::obj().with("sel", shapes(0, &[6])).with("how", "center"));
    let b6 = b.doc().shape_box(0, 6).unwrap();
    assert!(near(b6.x + b6.w / 2.0, 360.0), "one shape centres on the slide");
    // Fill and outline.
    run(&mut b, "stage.fill", Json::obj().with("sel", shapes(0, &[6])).with("color", "00FF00"));
    run(&mut b, "stage.outline", Json::obj().with("sel", shapes(0, &[6])).with("color", "123456").with("width", 3.0));
    let info = b.doc().shape_info(0, "Numbered").unwrap();
    assert!(matches!(info.fill, Some(Fill::Solid(c)) if hex(c) == "00FF00"));
    assert!(near(info.line.unwrap().to_stroke().unwrap().width, 3.0));
    run(&mut b, "stage.fill", Json::obj().with("sel", shapes(0, &[6])).with("color", Json::Null));
    assert!(matches!(b.doc().shape_info(0, "Numbered").unwrap().fill, Some(Fill::None)));
    // Group, ungroup, regroup.
    let g = run(&mut b, "stage.group", Json::obj().with("sel", shapes(0, &[5, 6])));
    let gid = landed(&g).get("focus").and_then(|f| f.get("shape")).and_then(Json::as_i64).unwrap() as u32;
    assert_eq!(b.doc().geom(0, gid).unwrap().kind, "group");
    assert_eq!(b.doc().geom(0, 6).unwrap().group, Some(gid));
    let b6g = b.doc().shape_box(0, 6).unwrap();
    assert!(near(b6g.x, b6.x) && near(b6g.y, b6.y), "grouping moves nothing");
    // A click in the group selects the group.
    let hit = b.doc().hit(0, b6g.x + 2.0, b6g.y + b6g.h - 2.0).unwrap();
    assert_eq!(hit.json().get("shape").and_then(Json::as_i64), Some(gid as i64));
    run(&mut b, "stage.ungroup", Json::obj().with("sel", shapes(0, &[gid])));
    assert!(b.doc().geom(0, gid).is_none());
    let b6u = b.doc().shape_box(0, 6).unwrap();
    assert!(near(b6u.x, b6.x) && near(b6u.y, b6.y), "ungrouping moves nothing");
    run(&mut b, "stage.regroup", Json::obj().with("sel", shapes(0, &[5])));
    assert_eq!(b.doc().geom(0, gid).map(|g| g.kind), Some("group"), "the same group again");
    // Duplicate, select all, delete.
    let d = run(&mut b, "stage.duplicate", Json::obj().with("sel", shapes(0, &[4])));
    let copy = landed(&d).get("focus").and_then(|f| f.get("shape")).and_then(Json::as_i64).unwrap() as u32;
    assert_ne!(copy, 4);
    let all = run(&mut b, "stage.select_all", Json::obj().with("sel", shapes(0, &[4])));
    assert!(landed(&all).get("anchor").and_then(|a| a.get("shapes")).unwrap().as_arr().unwrap().len() >= 5);
    run(&mut b, "stage.delete", Json::obj().with("sel", shapes(0, &[copy])));
    assert!(b.doc().geom(0, copy).is_none());
    while b.can_undo() {
        b.undo().unwrap();
    }
    assert_eq!(b.doc().save(), deck_bytes());
}

#[test]
fn slides_new_duplicate_move_delete_and_layout() {
    let mut b = bus();
    assert_eq!(b.doc().layout_names(), vec!["Title and Content", "Accent Title"]);
    let done = run(&mut b, "stage.new_slide", Json::obj().with("after", 0).with("layout", "Title and Content").with("title", "Roadmap"));
    assert_eq!(b.doc().slide_count(), 3);
    assert_eq!(landed(&done).get("anchor").and_then(|a| a.get("slide")).and_then(Json::as_usize), Some(1));
    let d = b.doc().describe();
    let s1 = &d.get("slides").unwrap().as_arr().unwrap()[1];
    assert_eq!(s1.get("title").and_then(Json::as_str), Some("Roadmap"));
    assert_eq!(s1.get("layout").and_then(Json::as_str), Some("Title and Content"));
    assert_eq!(s1.get("placeholders").unwrap().as_arr().unwrap().len(), 2);
    // The new slide's empty content placeholder takes a caret.
    let content = b.doc().hit(1, 300.0, 300.0).unwrap();
    assert!(content.json().get("para").is_some());
    run(&mut b, "stage.duplicate_slide", Json::obj().with("slide", 0));
    assert_eq!(b.doc().slide_count(), 4);
    assert_eq!(b.doc().deck().notes_text(1).as_deref(), Some("Remember the demo"), "notes come along");
    run(&mut b, "stage.move_slide", Json::obj().with("from", 3).with("to", 0));
    assert_eq!(b.doc().describe().get("slides").unwrap().as_arr().unwrap()[0].get("layout").and_then(Json::as_str), Some("Accent Title"));
    run(&mut b, "stage.delete_slide", Json::obj().with("slide", 0));
    assert_eq!(b.doc().slide_count(), 3);
    run(&mut b, "stage.set_layout", Json::obj().with("slide", 0).with("layout", 1));
    assert_eq!(b.doc().deck().layout_name(0), "Accent Title");
    // Saved, everything is consistent: each slide part has its content type and relationship.
    let again = open(&b.doc().save());
    assert_eq!(again.slide_count(), 3);
    for s in &again.deck().slides {
        assert_eq!(again.package().content_type(&s.part), Some(opc::CT_SLIDE));
        assert!(s.layout.is_some());
    }
    while b.can_undo() {
        b.undo().unwrap();
    }
    assert_eq!(b.doc().slide_count(), 2);
    let before = Package::open(&deck_bytes()).unwrap();
    for name in before.part_names() {
        assert_eq!(before.part_bytes(&name).unwrap(), b.doc().package().part_bytes(&name).unwrap(), "{name} back as it was");
    }
    assert!(b.do_user(Command::new("stage.delete_slide", Json::obj().with("slide", 9))).is_err());
}

#[test]
fn slides_and_shapes_copy_between_decks() {
    let a = open(&deck_bytes());
    // Slide 2 (picture, group, table) copied whole.
    let clip = a.copy(&editing::slide_sel(1));
    assert_eq!(clip.rich.as_ref().and_then(|r| r.get("kind")).and_then(Json::as_str), Some("stage.slides"));
    let shapes_clip = a.copy(&Selection::from_json(&shapes(1, &[3])).unwrap());
    let mut b = Bus::new(StageDoc::new_blank_with_fonts(FontCache::empty()));
    let sel = editing::slide_sel(0).to_json();
    run(&mut b, "stage.paste", Json::obj().with("sel", sel.clone()).with("text", clip.text.as_str()).with("rich", clip.rich.clone().unwrap()));
    assert_eq!(b.doc().slide_count(), 2);
    let kinds: Vec<String> = b.doc().read(&Json::obj().with("slide", 1)).unwrap().get("shapes").unwrap().as_arr().unwrap().iter().map(|s| s.get("kind").and_then(Json::as_str).unwrap().to_string()).collect();
    assert!(kinds.contains(&"picture".to_string()) && kinds.contains(&"table".to_string()), "{:?}", kinds);
    assert!(b.doc().display(1).items.iter().any(|i| matches!(i, Item::Image { .. })), "the picture came along and draws");
    // The picture shape alone onto the first slide.
    run(&mut b, "stage.paste", Json::obj().with("sel", sel).with("text", "").with("rich", shapes_clip.rich.unwrap()));
    assert!(b.doc().display(0).items.iter().any(|i| matches!(i, Item::Image { .. })));
    let again = open(&b.doc().save());
    assert_eq!(again.slide_count(), 2);
    assert!(again.display(1).items.iter().any(|i| matches!(i, Item::Image { .. })));
    // Plain text onto a slide becomes a text box.
    run(&mut b, "stage.paste", Json::obj().with("sel", editing::slide_sel(0).to_json()).with("text", "pasted words"));
    let shapes0 = b.doc().read(&Json::obj().with("slide", 0)).unwrap();
    assert!(shapes0.to_string().contains("pasted words"));
}

#[test]
fn autofit_shrinks_text_on_overflow() {
    let mut b = bus();
    let long = "a long line of words that will need many lines in this box ".repeat(40);
    run(&mut b, "stage.insert_text", Json::obj().with("sel", caret(0, 3, 2, 11)).with("text", long.as_str()));
    let info = b.doc().shape_info(0, "Content 2").unwrap();
    let (fs, _) = info.body.shrink();
    assert!(fs < 0.5, "the font scale went down: {fs}");
    // And back up when the text goes.
    b.undo().unwrap();
    run(&mut b, "stage.delete", Json::obj().with("sel", range(0, 3, (0, 0), (2, 11))));
    let (fs, lr) = b.doc().shape_info(0, "Content 2").unwrap().body.shrink();
    assert!(near(fs, 1.0) && near(lr, 0.0), "fits again at 100%: {fs} {lr}");
    run(&mut b, "stage.autofit", Json::obj().with("sel", shapes(0, &[3])).with("mode", "none"));
    assert!(slide_xml(&b, 0).contains("<a:noAutofit/>"));
    run(&mut b, "stage.wrap", Json::obj().with("sel", shapes(0, &[5])).with("on", true));
    assert!(slide_xml(&b, 0).contains(r#"<a:bodyPr wrap="square"/>"#));
}

#[test]
fn prompts_show_in_empty_placeholders_only_while_editing() {
    let ph = Ph { ty: "title".into(), idx: None };
    assert_eq!(render::prompt_text(&ph, None).as_deref(), Some("Click to add title"));
    assert_eq!(render::prompt_text(&Ph { ty: "sldNum".into(), idx: Some(4) }, None), None);
    if StageDoc::open(&deck_bytes()).map(|d| d.fonts.index().faces().is_empty()).unwrap_or(true) {
        return;
    }
    let mut doc = StageDoc::new_blank();
    let edit = doc.display(0);
    let show = doc.show_display(0);
    let prompt = |dl: &DisplayList| dl.items.iter().any(|i| matches!(i, Item::Glyphs { color, .. } if color.a == render::PROMPT_ALPHA));
    assert!(prompt(&edit), "the editing view shows the prompt");
    assert!(!prompt(&show), "the show does not");
}

#[test]
fn crop_scale_and_moving_a_slide_by_one() {
    let mut b = bus();
    run(&mut b, "stage.crop", Json::obj().with("sel", shapes(1, &[3])).with("l", 0.1).with("b", 0.2));
    let xml = slide_xml(&b, 1);
    assert!(xml.contains(r#"<a:srcRect l="10000" b="20000"/>"#), "{xml}");
    assert!(b.do_user(Command::new("stage.crop", Json::obj().with("sel", shapes(1, &[7])).with("l", 0.1))).is_err(), "only pictures crop");
    let before = b.doc().shape_box(1, 3).unwrap();
    run(&mut b, "stage.size", Json::obj().with("sel", shapes(1, &[3])).with("scale", 2.0));
    let after = b.doc().shape_box(1, 3).unwrap();
    assert!(near(after.w, before.w * 2.0) && near(after.x + after.w / 2.0, before.x + before.w / 2.0), "scaled about the centre");
    let done = run(&mut b, "stage.move_slide", Json::obj().with("sel", editing::slide_sel(0).to_json()).with("by", 1));
    assert_eq!(b.doc().deck().layout_name(1), "Title and Content");
    assert_eq!(landed(&done).get("anchor").and_then(|a| a.get("slide")).and_then(Json::as_usize), Some(1));
    let st = b.doc().state(&editing::slide_sel(1));
    assert_eq!(st.get("whole").and_then(Json::as_bool), Some(true));
    // Every command on the bus has a spec, and no spec is listed twice.
    let names: Vec<String> = b.doc().commands().into_iter().map(|c| c.name).collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len());
    assert!(names.len() > 50);
}

#[test]
fn find_and_replace_across_the_deck() {
    let mut b = bus();
    let f = run(&mut b, "stage.find", Json::obj().with("query", "POINT").with("next", true).with("sel", caret(0, 2, 0, 0)));
    let sel = landed(&f);
    assert_eq!(sel.get("anchor").unwrap().to_string(), tpos(0, 3, 0, 6).to_string());
    assert_eq!(b.history().len(), 0, "a find leaves no step");
    let f2 = run(&mut b, "stage.find", Json::obj().with("query", "point").with("next", true).with("sel", sel.clone()));
    assert_eq!(landed(&f2).get("anchor").unwrap().to_string(), tpos(0, 3, 1, 7).to_string());
    run(&mut b, "stage.replace", Json::obj().with("query", "point").with("with", "idea").with("sel", sel));
    assert!(text(&b, 0, 3).starts_with("First idea\n"));
    run(&mut b, "stage.replace", Json::obj().with("query", "point").with("with", "idea").with("all", true));
    assert_eq!(text(&b, 0, 3), "First idea\nSecond idea\nThird idea");
    b.undo().unwrap();
    b.undo().unwrap();
    assert_eq!(text(&b, 0, 3), "First point\nSecond point\nThird point");
    // Plain text with nothing selected but the slide: a text box; Delete there does nothing.
    let blank = Json::obj().with("anchor", Json::obj().with("slide", 1));
    run(&mut b, "stage.delete", Json::obj().with("sel", blank.clone()).with("back", true));
    assert_eq!(b.doc().slide_count(), 2);
    run(&mut b, "stage.insert_text_box", Json::obj().with("sel", blank).with("text", "here"));
    assert!(b.doc().read(&Json::obj().with("slide", 1)).unwrap().to_string().contains("here"));
}

#[test]
fn the_companion_names_shapes_instead_of_a_selection() {
    let mut b = bus();
    let args = Json::parse(r#"{"commands":[
        {"name":"stage.format","args":{"slide":0,"shape":"Content 2","bold":true}},
        {"name":"stage.align","args":{"slide":0,"shape":3,"align":"right"}},
        {"name":"stage.insert_shape","args":{"slide":1,"preset":"star5","x":10,"y":10,"w":50,"h":50}},
        {"name":"stage.new_slide","args":{"after":1,"layout":"Accent Title","title":"Thanks"}}
    ],"label":"tidy up"}"#)
    .unwrap();
    let done = b.door("do", &args).unwrap();
    assert_eq!(done.get("ok").and_then(Json::as_bool), Some(true));
    let info = b.doc().shape_info(0, "Content 2").unwrap();
    assert!(info.paragraphs.iter().all(|p| p.runs[0].style.weight == 700 && p.align == Align::Right));
    assert_eq!(b.doc().slide_count(), 3);
    assert!(b.door("describe", &Json::obj()).unwrap().get("document").unwrap().get("places").is_some());
    b.undo().unwrap();
    assert_eq!(b.doc().save(), deck_bytes(), "one step, all undone exactly");
}
