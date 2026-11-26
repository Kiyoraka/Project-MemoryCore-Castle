//! Scroll M2 "write" tests: the caret and the selection on laid-out invented documents, every
//! writing command applied and undone, and round trips through save (new XML in Word's shape,
//! untouched parts byte for byte).

use super::layout::widow_plan;
use super::tests::{list_p, p, png_bytes, styled_p, Docx, SECT};
use super::*;
use crate::ground::bus::{Bus, Origin};
use crate::ground::edit::{door, Pos};
use crate::ground::font::FontIndex;
use std::sync::Arc;

const STYLES: &str = "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:rPr><w:sz w:val=\"24\"/></w:rPr></w:style>";

fn open_docx(d: &Docx) -> ScrollDoc {
    ScrollDoc::open_with_fonts(&d.bytes(), Arc::new(FontIndex::new())).expect("the invented document opens")
}

fn doc3() -> ScrollDoc {
    open_docx(&Docx::new(&format!("{}{}{}{SECT}", p("Hello world"), p("Second line here"), p("Third"))).styles(STYLES))
}

fn pos(p: usize, o: usize) -> Pos {
    Pos(Json::obj().with("p", p).with("o", o))
}

fn sel(a: (usize, usize), f: (usize, usize)) -> Json {
    Json::obj().with("anchor", pos(a.0, a.1).0).with("focus", pos(f.0, f.1).0)
}

fn at(p: usize, o: usize) -> Json {
    sel((p, o), (p, o))
}

fn xml_of(doc: &ScrollDoc, part: &str) -> String {
    String::from_utf8(doc.package().part_bytes(part).unwrap()).unwrap()
}

fn main_xml(doc: &ScrollDoc) -> String {
    xml_of(doc, "/word/document.xml")
}

fn run(bus: &mut Bus<ScrollDoc>, name: &str, args: Json) -> Json {
    bus.do_user(Command::new(name, args)).unwrap_or_else(|e| panic!("{name} failed: {e}"))
}

fn landed(done: &Json) -> (usize, usize, usize, usize) {
    let s = done.get("selection").expect("a selection");
    let g = |k: &str, f: &str| s.get(k).and_then(|x| x.get(f)).and_then(Json::as_usize).unwrap();
    (g("anchor", "p"), g("anchor", "o"), g("focus", "p"), g("focus", "o"))
}

fn state(bus: &Bus<ScrollDoc>, s: Json) -> Json {
    bus.edit("state", &Json::obj().with("sel", s)).unwrap()
}

fn text(bus: &Bus<ScrollDoc>, p: usize) -> String {
    bus.doc().paragraph_text(p).unwrap_or("").to_string()
}

// ---------------------------------------------------------------------------------------------------
// The editing ground
// ---------------------------------------------------------------------------------------------------

#[test]
fn caret_hit_selection_and_moves_on_a_laid_out_document() {
    let doc = doc3();
    let ed = doc.editing().expect("Scroll can be edited");
    let c0 = ed.caret(&pos(0, 0)).unwrap();
    let c5 = ed.caret(&pos(0, 5)).unwrap();
    assert!(c5.x > c0.x && c5.y == c0.y && c5.page == 0 && c0.height > 0.0);
    assert_eq!(ed.hit(0, c5.x + 0.1, c5.y + c5.height / 2.0), Some(pos(0, 5)));
    let c1 = ed.caret(&pos(1, 0)).unwrap();
    assert!(c1.y > c0.y);
    assert_eq!(ed.hit(0, c1.x, c1.y + 1.0), Some(pos(1, 0)));
    // Clicking far right of a line lands at its end; below everything, on the last line.
    assert_eq!(ed.hit(0, 600.0, c0.y + 1.0), Some(pos(0, 11)));
    assert_eq!(ed.hit(0, 0.0, 780.0).map(|p| p.0.get("p").and_then(Json::as_usize)), Some(Some(2)));

    let mv = |p: Pos, m: &str| ground_step(&doc, &p, m);
    assert_eq!(mv(pos(0, 11), "right"), Some(pos(1, 0)));
    assert_eq!(mv(pos(1, 0), "left"), Some(pos(0, 11)));
    assert_eq!(mv(pos(0, 0), "word_right"), Some(pos(0, 6)));
    assert_eq!(mv(pos(0, 11), "word_left"), Some(pos(0, 6)));
    assert_eq!(mv(pos(0, 2), "line_end"), Some(pos(0, 11)));
    assert_eq!(mv(pos(0, 7), "line_start"), Some(pos(0, 0)));
    assert_eq!(mv(pos(1, 3), "doc_end"), Some(pos(2, 5)));
    assert_eq!(mv(pos(1, 3), "doc_start"), Some(pos(0, 0)));
    let down = mv(pos(0, 3), "down").unwrap();
    assert_eq!(down.0.get("p").and_then(Json::as_usize), Some(1));
    let up = mv(pos(1, 3), "up").unwrap();
    assert_eq!(up.0.get("p").and_then(Json::as_usize), Some(0));
    assert_eq!(mv(pos(0, 0), "left"), None);

    let rects = ed.selection_rects(&Selection::new(pos(0, 2), pos(1, 3)));
    assert_eq!(rects.len(), 2);
    assert!(rects.iter().all(|(pg, r)| *pg == 0 && r.w > 0.0));
    assert!(ed.selection_rects(&Selection::caret(pos(0, 2))).is_empty());

    let clip = ed.copy(&Selection::new(pos(1, 6), pos(0, 6)));
    assert_eq!(clip.text, "world\nSecond");
    assert!(clip.rich.as_ref().and_then(|r| r.get("paras")).and_then(Json::as_arr).is_some_and(|a| a.len() == 2));

    // Through the ground's door, JSON in and out.
    let moved = door(&doc, "move", &Json::obj().with("pos", pos(0, 0).0).with("motion", "right")).unwrap();
    assert_eq!(moved.to_string(), r#"{"p":0,"o":1}"#);
    assert!(doc.pdf_resources().is_some());
}

fn ground_step(doc: &ScrollDoc, p: &Pos, m: &str) -> Option<Pos> {
    crate::ground::edit::step(doc.editing().unwrap(), p, m)
}

#[test]
fn line_boxes_map_wrapped_lines_and_list_labels() {
    let long = "word ".repeat(60);
    let body = format!("{}{}{SECT}", p(long.trim_end()), list_p(1, 0, "Item"));
    let numbering = "<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/><w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>";
    let doc = open_docx(&Docx::new(&body).styles(STYLES).numbering(numbering));
    let lines: Vec<&layout::LineBox> = doc.laid().lines.iter().filter(|l| l.key == layout::LineKey::Body(0)).collect();
    assert!(lines.len() > 1, "the long paragraph wraps");
    for w in lines.windows(2) {
        assert_eq!(w[0].atoms.end, w[1].atoms.start, "lines cover the characters without gaps");
        assert!(w[1].top > w[0].top);
    }
    assert_eq!(lines.last().unwrap().atoms.end, long.trim_end().chars().count());
    // The caret at a wrap point sits at the start of the next line.
    let ed = doc.editing().unwrap();
    let wrap = lines[1].atoms.start;
    let c = ed.caret(&pos(0, wrap)).unwrap();
    assert!((c.y - (lines[1].baseline - lines[1].ascent)).abs() < 0.01);
    // The list label is not a character: offset 0 is after it.
    let item = doc.laid().lines.iter().find(|l| l.key == layout::LineKey::Body(1)).unwrap();
    assert_eq!(item.atoms, 0..4);
    let c0 = ed.caret(&pos(1, 0)).unwrap();
    assert!(c0.x > item.col_x + 20.0, "the caret starts after the label");
}

// ---------------------------------------------------------------------------------------------------
// Typing
// ---------------------------------------------------------------------------------------------------

#[test]
fn typing_deleting_and_undo_put_the_xml_back_exactly() {
    let doc = doc3();
    let original = doc.package().part_bytes("/word/document.xml").unwrap();
    let styles_before = doc.package().part_bytes("/word/styles.xml").unwrap();
    let mut bus = Bus::new(doc);
    let done = run(&mut bus, "scroll.insert_text", Json::obj().with("sel", at(0, 5)).with("text", ","));
    assert_eq!(text(&bus, 0), "Hello, world");
    assert_eq!(landed(&done), (0, 6, 0, 6));
    assert!(done.get("pages").and_then(Json::as_arr).is_some_and(|p| p.iter().any(|x| x.as_usize() == Some(0))));
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", at(0, 12)).with("text", "!"));
    assert_eq!(text(&bus, 0), "Hello, world!");
    // Typed text joins the run it follows: no new runs for plain typing.
    assert!(main_xml(bus.doc()).contains("<w:r><w:t xml:space=\"preserve\">Hello, world!</w:t></w:r>"), "{}", main_xml(bus.doc()));
    let done = run(&mut bus, "scroll.delete", Json::obj().with("sel", at(0, 6)).with("back", true));
    assert_eq!(text(&bus, 0), "Hello world!");
    assert_eq!(landed(&done), (0, 5, 0, 5));
    run(&mut bus, "scroll.delete", Json::obj().with("sel", at(0, 11)).with("back", false));
    assert_eq!(text(&bus, 0), "Hello world");
    // Typing over a selection replaces it.
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", sel((0, 6), (0, 11))).with("text", "there"));
    assert_eq!(text(&bus, 0), "Hello there");

    let saved = bus.doc().save();
    let again = ScrollDoc::open_with_fonts(&saved, Arc::new(FontIndex::new())).unwrap();
    assert_eq!(again.paragraph_text(0), Some("Hello there"));
    assert_eq!(again.package().part_bytes("/word/styles.xml").unwrap(), styles_before, "untouched parts stay byte for byte");

    while bus.can_undo() {
        bus.undo().unwrap();
    }
    assert_eq!(bus.doc().package().part_bytes("/word/document.xml").unwrap(), original);
    while bus.can_redo() {
        bus.redo().unwrap();
    }
    assert_eq!(text(&bus, 0), "Hello there");
}

#[test]
fn enter_splits_backspace_joins_and_breaks() {
    let mut bus = Bus::new(doc3());
    let done = run(&mut bus, "scroll.enter", Json::obj().with("sel", at(0, 5)));
    assert_eq!((text(&bus, 0), text(&bus, 1)), ("Hello".to_string(), " world".to_string()));
    assert_eq!(bus.doc().paragraph_count(), 4);
    assert_eq!(landed(&done), (1, 0, 1, 0));
    let done = run(&mut bus, "scroll.delete", Json::obj().with("sel", at(1, 0)).with("back", true));
    assert_eq!(text(&bus, 0), "Hello world");
    assert_eq!(bus.doc().paragraph_count(), 3);
    assert_eq!(landed(&done), (0, 5, 0, 5));
    run(&mut bus, "scroll.delete", Json::obj().with("sel", at(0, 11)).with("back", false));
    assert_eq!(text(&bus, 0), "Hello worldSecond line here");
    // Enter at the end of a paragraph, then type into the new one.
    let done = run(&mut bus, "scroll.enter", Json::obj().with("sel", at(1, 5)));
    assert_eq!(landed(&done), (2, 0, 2, 0));
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", at(2, 0)).with("text", "New"));
    assert_eq!(text(&bus, 2), "New");
    run(&mut bus, "scroll.enter", Json::obj().with("sel", at(2, 1)).with("shift", true));
    assert_eq!(text(&bus, 2), "N\new");
    assert_eq!(bus.doc().page_count(), 1);
    run(&mut bus, "scroll.enter", Json::obj().with("sel", at(0, 5)).with("ctrl", true));
    assert_eq!(bus.doc().page_count(), 2, "Ctrl+Enter is a page break");
    assert!(main_xml(bus.doc()).contains("<w:br w:type=\"page\"/>"));
    // Words and selections across paragraphs.
    let mut bus = Bus::new(doc3());
    run(&mut bus, "scroll.delete", Json::obj().with("sel", at(1, 12)).with("back", true).with("word", true));
    assert_eq!(text(&bus, 1), "Second here");
    run(&mut bus, "scroll.delete", Json::obj().with("sel", sel((0, 5), (1, 6))));
    assert_eq!(text(&bus, 0), "Hello here");
    assert_eq!(bus.doc().paragraph_count(), 2);
    run(&mut bus, "scroll.tab", Json::obj().with("sel", at(1, 0)));
    assert_eq!(text(&bus, 1), "\tThird");
    // A multi-line insert makes paragraphs.
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", at(0, 0)).with("text", "A\nB\n"));
    assert_eq!((text(&bus, 0), text(&bus, 1), text(&bus, 2)), ("A".to_string(), "B".to_string(), "Hello here".to_string()));
}

#[test]
fn split_keeps_the_section_break_and_unknown_markup() {
    let body = format!(
        "<w:p><w:pPr><w:jc w:val=\"right\"/><w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr></w:pPr><w:bookmarkStart w:id=\"1\" w:name=\"b\"/><w:r><w:rPr><w:i/></w:rPr><w:t>AlphaBeta</w:t></w:r><w:bookmarkEnd w:id=\"1\"/><x14:odd/></w:p>{}{SECT}",
        p("Next section")
    );
    let mut bus = Bus::new(open_docx(&Docx::new(&body).styles(STYLES)));
    run(&mut bus, "scroll.enter", Json::obj().with("sel", at(0, 5)));
    let xml = main_xml(bus.doc());
    assert!(xml.contains("<w:p><w:pPr><w:jc w:val=\"right\"/></w:pPr><w:bookmarkStart w:id=\"1\" w:name=\"b\"/><w:r><w:rPr><w:i/></w:rPr><w:t>Alpha</w:t></w:r></w:p>"), "{xml}");
    assert!(xml.contains("<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr></w:pPr><w:r><w:rPr><w:i/></w:rPr><w:t>Beta</w:t></w:r><w:bookmarkEnd w:id=\"1\"/><x14:odd/></w:p>"), "{xml}");
    assert_eq!(bus.doc().describe().get("sections").and_then(Json::as_usize), Some(2));
}

// ---------------------------------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------------------------------

#[test]
fn bold_toggles_with_state_and_pending_format_at_a_caret() {
    let mut bus = Bus::new(doc3());
    run(&mut bus, "scroll.toggle", Json::obj().with("sel", sel((0, 0), (0, 5))).with("prop", "bold"));
    assert!(main_xml(bus.doc()).contains("<w:r><w:rPr><w:b/><w:bCs/></w:rPr><w:t xml:space=\"preserve\">Hello</w:t></w:r><w:r><w:t xml:space=\"preserve\"> world</w:t></w:r>"), "{}", main_xml(bus.doc()));
    assert_eq!(state(&bus, sel((0, 0), (0, 5))).get("bold").and_then(Json::as_bool), Some(true));
    assert_eq!(state(&bus, sel((0, 6), (0, 11))).get("bold").and_then(Json::as_bool), Some(false));
    assert_eq!(state(&bus, at(0, 3)).get("bold").and_then(Json::as_bool), Some(true));
    assert_eq!(text(&bus, 0), "Hello world");
    run(&mut bus, "scroll.toggle", Json::obj().with("sel", sel((0, 0), (0, 5))).with("prop", "bold"));
    assert_eq!(state(&bus, sel((0, 0), (0, 5))).get("bold").and_then(Json::as_bool), Some(false));
    // Formatting at a caret with no word under it waits for the typing.
    let done = run(&mut bus, "scroll.toggle", Json::obj().with("sel", at(2, 5)).with("prop", "italic"));
    assert_eq!(done.get("step").and_then(Json::as_usize), Some(0), "no undo step for a pending format");
    assert_eq!(state(&bus, at(2, 5)).get("italic").and_then(Json::as_bool), Some(true));
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", at(2, 5)).with("text", "X"));
    assert_eq!(text(&bus, 2), "ThirdX");
    assert_eq!(state(&bus, sel((2, 5), (2, 6))).get("italic").and_then(Json::as_bool), Some(true));
    assert_eq!(state(&bus, sel((2, 0), (2, 5))).get("italic").and_then(Json::as_bool), Some(false));
    // A caret inside a word formats the word.
    run(&mut bus, "scroll.toggle", Json::obj().with("sel", at(1, 2)).with("prop", "underline"));
    assert_eq!(state(&bus, sel((1, 0), (1, 6))).get("underline").and_then(Json::as_bool), Some(true));
    assert_eq!(state(&bus, sel((1, 7), (1, 11))).get("underline").and_then(Json::as_bool), Some(false));
}

#[test]
fn character_properties_are_written_in_schema_order() {
    let mut bus = Bus::new(doc3());
    run(
        &mut bus,
        "scroll.format",
        Json::obj()
            .with("sel", sel((0, 0), (0, 5)))
            .with("highlight", "yellow")
            .with("underline", "double")
            .with("size", 14.0)
            .with("color", "#1f4e79")
            .with("font", "Georgia"),
    );
    let xml = main_xml(bus.doc());
    assert!(
        xml.contains("<w:rPr><w:rFonts w:ascii=\"Georgia\" w:hAnsi=\"Georgia\" w:cs=\"Georgia\"/><w:color w:val=\"1F4E79\"/><w:sz w:val=\"28\"/><w:szCs w:val=\"28\"/><w:highlight w:val=\"yellow\"/><w:u w:val=\"double\"/></w:rPr><w:t xml:space=\"preserve\">Hello</w:t>"),
        "{xml}"
    );
    let st = state(&bus, sel((0, 0), (0, 5)));
    assert_eq!(st.get("font").and_then(Json::as_str), Some("Georgia"));
    assert_eq!(st.get("size").and_then(Json::as_f64), Some(14.0));
    assert_eq!(st.get("color").and_then(Json::as_str), Some("1F4E79"));
    assert_eq!(st.get("underline_style").and_then(Json::as_str), Some("double"));
    run(&mut bus, "scroll.toggle", Json::obj().with("sel", sel((2, 1), (2, 2))).with("prop", "subscript"));
    assert!(main_xml(bus.doc()).contains("<w:vertAlign w:val=\"subscript\"/>"));
    run(&mut bus, "scroll.format", Json::obj().with("sel", sel((1, 0), (1, 6))).with("font", "+Headings"));
    assert!(main_xml(bus.doc()).contains("<w:rFonts w:asciiTheme=\"majorHAnsi\" w:hAnsiTheme=\"majorHAnsi\"/>"));
    run(&mut bus, "scroll.font_size_step", Json::obj().with("sel", sel((2, 0), (2, 1))).with("delta", 1));
    assert_eq!(state(&bus, sel((2, 0), (2, 1))).get("size").and_then(Json::as_f64), Some(14.0));
    assert!(bus.do_user(Command::new("scroll.format", Json::obj().with("sel", at(0, 0)).with("glow", true))).is_err());
    assert!(bus.do_user(Command::new("scroll.format", Json::obj().with("sel", sel((0, 0), (0, 2))).with("color", "pinkish"))).is_err());
    run(&mut bus, "scroll.clear_format", Json::obj().with("sel", sel((0, 0), (0, 11))));
    assert_eq!(state(&bus, sel((0, 0), (0, 5))).get("font"), Some(&Json::Null));
}

#[test]
fn paragraph_properties_change_the_layout_and_the_xml() {
    let mut bus = Bus::new(doc3());
    let x0 = bus.doc().editing().unwrap().caret(&pos(0, 0)).unwrap().x;
    run(&mut bus, "scroll.align", Json::obj().with("sel", at(0, 2)).with("value", "center"));
    assert!(main_xml(bus.doc()).contains("<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>"));
    assert!(bus.doc().editing().unwrap().caret(&pos(0, 0)).unwrap().x > x0 + 50.0);
    assert_eq!(state(&bus, at(0, 0)).get("align_center").and_then(Json::as_bool), Some(true));
    let tabs = Json::Arr(vec![Json::obj().with("pos", 72.0).with("kind", "right").with("leader", "dot")]);
    run(
        &mut bus,
        "scroll.paragraph",
        Json::obj()
            .with("sel", at(1, 0))
            .with("tabs", tabs)
            .with("ind_left", 36.0)
            .with("first_line", 18.0)
            .with("before", 12.0)
            .with("after", 6.0)
            .with("line", 1.5)
            .with("widow", false)
            .with("keep_next", true),
    );
    let xml = main_xml(bus.doc());
    assert!(
        xml.contains("<w:pPr><w:keepNext/><w:widowControl w:val=\"0\"/><w:tabs><w:tab w:val=\"right\" w:pos=\"1440\" w:leader=\"dot\"/></w:tabs><w:spacing w:before=\"240\" w:after=\"120\" w:line=\"360\" w:lineRule=\"auto\"/><w:ind w:left=\"720\" w:firstLine=\"360\"/></w:pPr>"),
        "{xml}"
    );
    let st = state(&bus, at(1, 0));
    assert_eq!(st.get("keep_next").and_then(Json::as_bool), Some(true));
    assert_eq!(st.get("ind_left").and_then(Json::as_f64), Some(36.0));
    run(&mut bus, "scroll.paragraph", Json::obj().with("sel", at(1, 0)).with("hanging", 18.0).with("line", 14.0).with("line_rule", "exact"));
    let xml = main_xml(bus.doc());
    assert!(xml.contains("<w:ind w:left=\"720\" w:hanging=\"360\"/>") && xml.contains("w:line=\"280\" w:lineRule=\"exact\""), "{xml}");
    run(&mut bus, "scroll.indent", Json::obj().with("sel", at(2, 0)).with("delta", 1));
    assert!(main_xml(bus.doc()).contains("<w:ind w:left=\"720\"/>"));
    assert!(bus.do_user(Command::new("scroll.paragraph", Json::obj().with("sel", at(0, 0)).with("align", "diagonal"))).is_err());
}

#[test]
fn styles_apply_and_built_in_ones_are_added_when_missing() {
    let doc = doc3();
    let original = doc.package().part_bytes("/word/styles.xml").unwrap();
    let mut bus = Bus::new(doc);
    run(&mut bus, "scroll.style", Json::obj().with("sel", at(0, 0)).with("style", "Heading 2"));
    assert!(xml_of(bus.doc(), "/word/styles.xml").contains("w:styleId=\"Heading2\""));
    let heads = bus.doc().describe().get("headings").and_then(Json::as_arr).unwrap().to_vec();
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0].get("level").and_then(Json::as_usize), Some(2));
    assert_eq!(state(&bus, at(0, 0)).get("style").and_then(Json::as_str), Some("Heading2"));
    run(&mut bus, "scroll.style", Json::obj().with("sel", sel((1, 0), (1, 6))).with("style", "Strong"));
    assert!(main_xml(bus.doc()).contains("<w:rStyle w:val=\"Strong\"/>"));
    assert_eq!(state(&bus, sel((1, 0), (1, 6))).get("bold").and_then(Json::as_bool), Some(true));
    assert!(bus.do_user(Command::new("scroll.style", Json::obj().with("sel", at(0, 0)).with("style", "Fancy Unknown"))).is_err());
    bus.undo().unwrap();
    bus.undo().unwrap();
    assert_eq!(bus.doc().package().part_bytes("/word/styles.xml").unwrap(), original);
}

// ---------------------------------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------------------------------

#[test]
fn bullets_numbers_levels_and_restart() {
    let mut bus = Bus::new(doc3());
    run(&mut bus, "scroll.list", Json::obj().with("sel", sel((0, 0), (1, 2))).with("kind", "bullet"));
    assert!(bus.doc().package().part("/word/numbering.xml").is_some());
    assert_eq!(state(&bus, at(1, 0)).get("list_bullet").and_then(Json::as_bool), Some(true));
    assert!(bus.doc().page_text(0).unwrap().contains('\u{2022}'));
    run(&mut bus, "scroll.list", Json::obj().with("sel", sel((0, 0), (1, 2))).with("kind", "bullet"));
    assert_eq!(state(&bus, at(1, 0)).get("list"), Some(&Json::Null), "again: back to plain");
    run(&mut bus, "scroll.list", Json::obj().with("sel", sel((0, 0), (2, 2))).with("kind", "number"));
    let t = bus.doc().page_text(0).unwrap().to_string();
    assert!(t.contains("1.") && t.contains("2.") && t.contains("3."), "{t}");
    run(&mut bus, "scroll.tab", Json::obj().with("sel", at(1, 0)));
    assert_eq!(state(&bus, at(1, 0)).get("list_level").and_then(Json::as_usize), Some(1));
    assert!(bus.doc().page_text(0).unwrap().contains("a."));
    run(&mut bus, "scroll.tab", Json::obj().with("sel", at(1, 0)).with("back", true));
    assert_eq!(state(&bus, at(1, 0)).get("list_level").and_then(Json::as_usize), Some(0));
    run(&mut bus, "scroll.list_restart", Json::obj().with("sel", at(2, 0)));
    let t = bus.doc().page_text(0).unwrap().to_string();
    assert_eq!(t.matches("1.").count(), 2, "{t}");
    // Enter on an empty item leaves the list; Backspace at an item's start does too.
    run(&mut bus, "scroll.enter", Json::obj().with("sel", at(2, 5)));
    run(&mut bus, "scroll.enter", Json::obj().with("sel", at(3, 0)));
    assert_eq!(state(&bus, at(3, 0)).get("list"), Some(&Json::Null));
    run(&mut bus, "scroll.delete", Json::obj().with("sel", at(1, 0)).with("back", true));
    assert_eq!(state(&bus, at(1, 0)).get("list"), Some(&Json::Null));
    run(&mut bus, "scroll.list", Json::obj().with("sel", at(0, 0)).with("kind", "number").with("format", "upperRoman").with("text", "%1)"));
    assert!(bus.doc().page_text(0).unwrap().starts_with("I)"));
}

// ---------------------------------------------------------------------------------------------------
// Find, replace, paste
// ---------------------------------------------------------------------------------------------------

#[test]
fn find_and_replace() {
    let doc = doc3();
    let original = doc.package().part_bytes("/word/document.xml").unwrap();
    let mut bus = Bus::new(doc);
    let done = run(&mut bus, "scroll.find", Json::obj().with("sel", at(0, 0)).with("query", "LINE").with("next", true));
    assert_eq!(done.get("step").and_then(Json::as_usize), Some(0));
    assert_eq!(landed(&done), (1, 7, 1, 11));
    let done = run(&mut bus, "scroll.find", Json::obj().with("sel", at(2, 0)).with("query", "o").with("next", false));
    assert_eq!(landed(&done), (1, 3, 1, 4));
    let done = run(&mut bus, "scroll.find", Json::obj().with("sel", at(0, 0)).with("query", "absent").with("next", true));
    assert!(done.get("selection").unwrap().is_null());
    let done = run(&mut bus, "scroll.find", Json::obj().with("sel", at(0, 0)).with("query", "hello").with("next", true).with("case", true));
    assert!(done.get("selection").unwrap().is_null());
    run(&mut bus, "scroll.replace", Json::obj().with("sel", at(0, 0)).with("query", "e").with("with", "E").with("all", true));
    assert_eq!((text(&bus, 0), text(&bus, 1), text(&bus, 2)), ("HEllo world".to_string(), "SEcond linE hErE".to_string(), "Third".to_string()));
    bus.undo().unwrap();
    assert_eq!(bus.doc().package().part_bytes("/word/document.xml").unwrap(), original);
    let done = run(&mut bus, "scroll.replace", Json::obj().with("sel", sel((1, 7), (1, 11))).with("query", "line").with("with", "row").with("all", false));
    assert_eq!(text(&bus, 1), "Second row here");
    assert_eq!(landed(&done), (1, 10, 1, 10));
    run(&mut bus, "scroll.replace", Json::obj().with("sel", at(0, 0)).with("query", "world").with("with", "there").with("all", true).with("whole", true));
    assert_eq!(text(&bus, 0), "Hello there");
}

#[test]
fn copy_and_paste_keep_formatting_inside_scroll() {
    let mut bus = Bus::new(doc3());
    run(&mut bus, "scroll.toggle", Json::obj().with("sel", sel((0, 0), (0, 5))).with("prop", "bold"));
    let clip = bus.edit("copy", &Json::obj().with("sel", sel((0, 0), (0, 5)))).unwrap();
    assert_eq!(clip.get("text").and_then(Json::as_str), Some("Hello"));
    let rich = clip.get("rich").cloned().unwrap();
    let done = run(&mut bus, "scroll.paste", Json::obj().with("sel", at(2, 5)).with("text", "Hello").with("rich", rich));
    assert_eq!(text(&bus, 2), "ThirdHello");
    assert_eq!(landed(&done), (2, 10, 2, 10));
    assert_eq!(state(&bus, sel((2, 5), (2, 10))).get("bold").and_then(Json::as_bool), Some(true));
    // Two paragraphs copied paste as two.
    let clip = bus.edit("copy", &Json::obj().with("sel", sel((0, 6), (1, 6)))).unwrap();
    run(&mut bus, "scroll.paste", Json::obj().with("sel", at(2, 0)).with("text", clip.get("text").cloned().unwrap()).with("rich", clip.get("rich").cloned().unwrap()));
    assert_eq!((text(&bus, 2), text(&bus, 3)), ("world".to_string(), "SecondThirdHello".to_string()));
    run(&mut bus, "scroll.paste", Json::obj().with("sel", at(0, 0)).with("text", "a\r\nb\n"));
    assert_eq!((text(&bus, 0), text(&bus, 1), text(&bus, 2)), ("a".to_string(), "b".to_string(), "Hello world".to_string()));
    run(&mut bus, "scroll.paste", Json::obj().with("sel", at(0, 0)).with("text", "").with("html", "<p>One &amp; <b>two</b></p><p>Three</p>"));
    assert_eq!((text(&bus, 0), text(&bus, 1)), ("One & two".to_string(), "Threea".to_string()));
}

// ---------------------------------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------------------------------

#[test]
fn tables_insert_move_grow_merge_and_style() {
    let mut bus = Bus::new(doc3());
    let done = run(&mut bus, "scroll.insert_table", Json::obj().with("sel", at(2, 5)).with("rows", 2).with("cols", 3));
    assert_eq!(bus.doc().model().tables, 1);
    assert_eq!(bus.doc().paragraph_count(), 3 + 6 + 1, "six cells and the paragraph after the table");
    assert_eq!(landed(&done), (3, 0, 3, 0));
    assert!(xml_of(bus.doc(), "/word/styles.xml").contains("w:styleId=\"TableGrid\""));
    assert!(main_xml(bus.doc()).contains("<w:tblStyle w:val=\"TableGrid\"/>"));
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", at(3, 0)).with("text", "A1"));
    let done = run(&mut bus, "scroll.tab", Json::obj().with("sel", at(3, 2)));
    assert_eq!(landed(&done).0, 4, "Tab goes to the next cell");
    assert_eq!(state(&bus, at(4, 0)).get("in_table").and_then(Json::as_bool), Some(true));
    let done = run(&mut bus, "scroll.tab", Json::obj().with("sel", at(4, 0)).with("back", true));
    assert_eq!(landed(&done), (3, 0, 3, 2));
    // Tab in the last cell adds a row.
    let done = run(&mut bus, "scroll.tab", Json::obj().with("sel", at(8, 0)));
    assert_eq!(bus.doc().paragraph_count(), 13);
    assert_eq!(landed(&done), (9, 0, 9, 0));
    assert!(bus.doc().page_text(0).unwrap().contains("A1"));
    run(&mut bus, "scroll.table_insert", Json::obj().with("sel", at(3, 0)).with("what", "col_right"));
    assert_eq!(bus.doc().paragraph_count(), 16);
    assert_eq!(state(&bus, at(3, 0)).get("table").and_then(|t| t.get("cols")).and_then(Json::as_usize), Some(4));
    run(&mut bus, "scroll.table_insert", Json::obj().with("sel", at(3, 0)).with("what", "row_above"));
    assert_eq!(bus.doc().paragraph_count(), 20);
    run(&mut bus, "scroll.table_delete", Json::obj().with("sel", at(3, 0)).with("what", "row"));
    assert_eq!(bus.doc().paragraph_count(), 16);
    run(&mut bus, "scroll.table_delete", Json::obj().with("sel", at(4, 0)).with("what", "col"));
    assert_eq!(bus.doc().paragraph_count(), 13);
    run(&mut bus, "scroll.merge_cells", Json::obj().with("sel", sel((3, 0), (4, 0))));
    let xml = main_xml(bus.doc());
    assert!(xml.contains("<w:gridSpan w:val=\"2\"/>"), "{xml}");
    assert!(bus.doc().page_text(0).unwrap().contains("A1"));
    run(&mut bus, "scroll.merge_cells", Json::obj().with("sel", sel((4, 0), (6, 0))));
    assert!(main_xml(bus.doc()).contains("<w:vMerge w:val=\"restart\"/>"));
    run(&mut bus, "scroll.shading", Json::obj().with("sel", at(3, 0)).with("color", "D9D9D9"));
    assert!(main_xml(bus.doc()).contains("w:fill=\"D9D9D9\""));
    run(&mut bus, "scroll.borders", Json::obj().with("sel", at(3, 0)).with("which", "none"));
    assert!(main_xml(bus.doc()).contains("<w:insideV w:val=\"nil\""));
    run(&mut bus, "scroll.header_rows", Json::obj().with("sel", at(3, 0)).with("on", true));
    assert!(main_xml(bus.doc()).contains("<w:tblHeader/>"));
    run(&mut bus, "scroll.row_height", Json::obj().with("sel", at(3, 0)).with("height", 30.0).with("exact", true));
    assert!(main_xml(bus.doc()).contains("<w:trHeight w:val=\"600\" w:hRule=\"exact\"/>"));
    run(&mut bus, "scroll.cell_align", Json::obj().with("sel", at(3, 0)).with("v", "center").with("h", "right"));
    assert!(main_xml(bus.doc()).contains("<w:vAlign w:val=\"center\"/>"));
    run(&mut bus, "scroll.table_widths", Json::obj().with("sel", at(3, 0)).with("autofit", "window"));
    assert!(main_xml(bus.doc()).contains("<w:tblW w:w=\"5000\" w:type=\"pct\"/>"));
    run(&mut bus, "scroll.table_widths", Json::obj().with("sel", at(3, 0)).with("widths", Json::Arr(vec![Json::Num(100.0), Json::Num(200.0), Json::Num(150.0)])));
    assert!(main_xml(bus.doc()).contains("<w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"4000\"/><w:gridCol w:w=\"3000\"/>"));
    run(&mut bus, "scroll.table_widths", Json::obj().with("sel", at(3, 0)).with("autofit", "contents"));
    run(&mut bus, "scroll.table_align", Json::obj().with("sel", at(3, 0)).with("align", "center"));
    let done = run(&mut bus, "scroll.select", Json::obj().with("sel", at(3, 0)).with("what", "table"));
    assert_eq!(landed(&done).0, 3);
    run(&mut bus, "scroll.table_delete", Json::obj().with("sel", at(3, 0)).with("what", "table"));
    assert_eq!(bus.doc().model().tables, 0);
    assert_eq!(bus.doc().paragraph_count(), 4);
}

// ---------------------------------------------------------------------------------------------------
// Pages, sections, headers and footers
// ---------------------------------------------------------------------------------------------------

#[test]
fn page_setup_and_section_breaks() {
    let mut bus = Bus::new(doc3());
    run(&mut bus, "scroll.page_setup", Json::obj().with("sel", at(0, 0)).with("size", "A4").with("orientation", "landscape"));
    let dl = bus.doc().display(0);
    assert!((dl.width - 841.9).abs() < 0.1 && (dl.height - 595.3).abs() < 0.1, "{} x {}", dl.width, dl.height);
    assert_eq!(state(&bus, at(0, 0)).get("landscape").and_then(Json::as_bool), Some(true));
    run(&mut bus, "scroll.page_setup", Json::obj().with("sel", at(0, 0)).with("top", 36.0).with("left", 54.0));
    let xml = main_xml(bus.doc());
    assert!(xml.contains("w:top=\"720\"") && xml.contains("w:left=\"1080\"") && xml.contains("w:orient=\"landscape\""), "{xml}");
    let done = run(&mut bus, "scroll.section_break", Json::obj().with("sel", at(1, 0)).with("kind", "nextPage"));
    assert_eq!(landed(&done), (2, 0, 2, 0));
    assert_eq!(bus.doc().describe().get("sections").and_then(Json::as_usize), Some(2));
    assert_eq!(bus.doc().page_count(), 2);
    // Each section keeps its own page setup.
    run(&mut bus, "scroll.page_setup", Json::obj().with("sel", at(2, 0)).with("orientation", "portrait"));
    assert!(bus.doc().display(0).width > bus.doc().display(0).height);
    assert!(bus.doc().display(1).width < bus.doc().display(1).height);
    run(&mut bus, "scroll.section_break", Json::obj().with("sel", at(3, 2)).with("kind", "continuous"));
    assert_eq!(bus.doc().describe().get("sections").and_then(Json::as_usize), Some(3));
    assert_eq!(bus.doc().page_count(), 2, "a continuous break stays on the page");
    run(&mut bus, "scroll.page_break", Json::obj().with("sel", at(0, 2)));
    assert_eq!(bus.doc().page_count(), 3);
    run(&mut bus, "scroll.page_setup", Json::obj().with("sel", at(0, 0)).with("size", "Letter").with("orientation", "portrait").with("scope", "all"));
    assert!((bus.doc().display(0).width - 612.0).abs() < 0.1);
}

#[test]
fn headers_footers_and_page_numbers() {
    let mut bus = Bus::new(doc3());
    let done = run(&mut bus, "scroll.header_footer", Json::obj().with("sel", at(0, 0)).with("which", "header").with("text", "ACME Corp"));
    let page0 = bus.doc().read(&Json::obj().with("page", 0)).unwrap();
    assert_eq!(page0.get("header").and_then(Json::as_str), Some("ACME Corp"));
    let s = done.get("selection").unwrap();
    let h = s.get("focus").and_then(|f| f.get("h")).and_then(Json::as_str).unwrap().to_string();
    assert!(h.starts_with("/word/header"));
    // Typing in the header through its place.
    let hp = Json::obj().with("h", h.as_str()).with("p", 0).with("o", 0).with("pg", 0);
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", Json::obj().with("anchor", hp.clone()).with("focus", hp.clone())).with("text", "Hi "));
    assert_eq!(bus.doc().read(&Json::obj().with("page", 0)).unwrap().get("header").and_then(Json::as_str), Some("Hi ACME Corp"));
    let caret = bus.edit("caret", &Json::obj().with("pos", hp.clone())).unwrap();
    assert!(caret.get("y").and_then(Json::as_f64).unwrap() < 72.0, "the header caret sits in the top margin");
    let hit = bus.edit("hit", &Json::obj().with("page", 0).with("x", 80.0).with("y", caret.get("y").and_then(Json::as_f64).unwrap() + 2.0)).unwrap();
    assert_eq!(hit.get("h").and_then(Json::as_str), Some(h.as_str()));
    assert_eq!(state(&bus, Json::obj().with("anchor", hp.clone()).with("focus", hp)).get("in_header").and_then(Json::as_bool), Some(true));

    run(&mut bus, "scroll.page_number", Json::obj().with("sel", at(0, 0)).with("where", "footer").with("format", "page_x_of_y").with("align", "center"));
    run(&mut bus, "scroll.page_break", Json::obj().with("sel", at(1, 0)));
    let p1 = bus.doc().read(&Json::obj().with("page", 1)).unwrap();
    assert_eq!(p1.get("footer").and_then(Json::as_str), Some("Page 2 of 2"));
    assert_eq!(p1.get("header").and_then(Json::as_str), Some("Hi ACME Corp"));
    run(&mut bus, "scroll.first_page", Json::obj().with("sel", at(0, 0)).with("on", true));
    assert_eq!(bus.doc().read(&Json::obj().with("page", 0)).unwrap().get("header").and_then(Json::as_str), Some(""));
    assert_eq!(bus.doc().read(&Json::obj().with("page", 1)).unwrap().get("header").and_then(Json::as_str), Some("Hi ACME Corp"));
    run(&mut bus, "scroll.header_footer", Json::obj().with("sel", at(0, 0)).with("which", "header").with("kind", "first").with("text", "Title page"));
    assert_eq!(bus.doc().read(&Json::obj().with("page", 0)).unwrap().get("header").and_then(Json::as_str), Some("Title page"));
    run(&mut bus, "scroll.page_number", Json::obj().with("sel", at(2, 5)).with("where", "here"));
    assert_eq!(text(&bus, 2), "Third");
    assert!(bus.doc().page_text(1).unwrap().contains("Third2"));
    run(&mut bus, "scroll.page_numbering", Json::obj().with("sel", at(0, 0)).with("start", 5).with("format", "lowerRoman"));
    assert_eq!(bus.doc().read(&Json::obj().with("page", 1)).unwrap().get("footer").and_then(Json::as_str), Some("Page vi of 2"));

    let saved = bus.doc().save();
    let again = ScrollDoc::open_with_fonts(&saved, Arc::new(FontIndex::new())).unwrap();
    assert_eq!(again.read(&Json::obj().with("page", 1)).unwrap().get("header").and_then(Json::as_str), Some("Hi ACME Corp"));
    let pkg = again.package();
    assert!(pkg.parts().any(|p| p.name().starts_with("/word/footer")));
}

// ---------------------------------------------------------------------------------------------------
// Pictures and links
// ---------------------------------------------------------------------------------------------------

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn pictures_go_in_inline_and_resize_with_their_proportions() {
    assert_eq!(media::base64_decode(&b64(b"any bytes!")).unwrap(), b"any bytes!");
    assert_eq!(media::image_info(&png_bytes()).map(|i| (i.0, i.2, i.3)), Some(("png", 2, 2)));
    let mut bus = Bus::new(doc3());
    let done = run(&mut bus, "scroll.insert_picture", Json::obj().with("sel", at(2, 5)).with("data", b64(&png_bytes())).with("name", "dot.png").with("width", 72.0));
    assert_eq!(landed(&done), (2, 5, 2, 6));
    assert_eq!(bus.doc().model().pictures, 1);
    assert!(bus.doc().package().part("/word/media/image1.png").is_some());
    assert!(bus.doc().display(0).items.iter().any(|i| matches!(i, Item::Image { .. })));
    let xml = main_xml(bus.doc());
    assert!(xml.contains("<wp:extent cx=\"914400\" cy=\"914400\"/>") && xml.contains("r:embed=\"rId"), "{xml}");
    assert_eq!(state(&bus, sel((2, 5), (2, 6))).get("picture").and_then(Json::as_bool), Some(true));
    run(&mut bus, "scroll.picture_size", Json::obj().with("sel", sel((2, 5), (2, 6))).with("width", 144.0));
    let xml = main_xml(bus.doc());
    assert!(xml.contains("<wp:extent cx=\"1828800\" cy=\"1828800\"/>") && xml.contains("<a:ext cx=\"1828800\" cy=\"1828800\"/>"), "{xml}");
    let again = ScrollDoc::open_with_fonts(&bus.doc().save(), Arc::new(FontIndex::new())).unwrap();
    assert_eq!(again.model().pictures, 1);
    assert!(again.display(0).items.iter().any(|i| matches!(i, Item::Image { .. })));
    // Deleting the picture character removes the drawing.
    run(&mut bus, "scroll.delete", Json::obj().with("sel", at(2, 6)).with("back", true));
    assert_eq!(bus.doc().model().pictures, 0);
    assert!(bus.do_user(Command::new("scroll.insert_picture", Json::obj().with("sel", at(0, 0)).with("data", b64(b"not a picture")).with("name", "x"))).is_err());
}

#[test]
fn links_insert_edit_and_remove() {
    let mut bus = Bus::new(doc3());
    run(&mut bus, "scroll.link", Json::obj().with("sel", sel((0, 0), (0, 5))).with("url", "https://example.com/"));
    let xml = main_xml(bus.doc());
    assert!(xml.contains("<w:hyperlink r:id=\"rId") && xml.contains("<w:rStyle w:val=\"Hyperlink\"/>"), "{xml}");
    assert_eq!(text(&bus, 0), "Hello world");
    assert_eq!(state(&bus, at(0, 2)).get("link").and_then(Json::as_str), Some("https://example.com/"));
    assert!(bus.doc().display(0).items.iter().any(|i| matches!(i, Item::Link { target, .. } if target == "https://example.com/")));
    run(&mut bus, "scroll.link", Json::obj().with("sel", at(0, 2)).with("url", "https://other.example/"));
    assert_eq!(state(&bus, at(0, 2)).get("link").and_then(Json::as_str), Some("https://other.example/"));
    run(&mut bus, "scroll.unlink", Json::obj().with("sel", at(0, 2)));
    assert!(!main_xml(bus.doc()).contains("<w:hyperlink"));
    assert_eq!(state(&bus, at(0, 2)).get("is_link").and_then(Json::as_bool), Some(false));
    run(&mut bus, "scroll.link", Json::obj().with("sel", at(2, 5)).with("url", "#top").with("text", " site"));
    assert_eq!(text(&bus, 2), "Third site");
    assert!(main_xml(bus.doc()).contains("w:anchor=\"top\""));
    assert!(bus.do_user(Command::new("scroll.link", Json::obj().with("sel", sel((0, 0), (1, 2))).with("url", "x"))).is_err());
}

// ---------------------------------------------------------------------------------------------------
// Every command undoes to the bytes it found
// ---------------------------------------------------------------------------------------------------

#[test]
fn every_command_undoes_byte_for_byte() {
    let table = "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/></w:tblGrid>\
<w:tr><w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr>\
<w:tr><w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>d</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
    let numbering = "<w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/></w:lvl></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>";
    let body = format!(
        "{}{}<w:p><w:r><w:rPr><w:i/></w:rPr><w:t>Mixed </w:t></w:r><w:r><w:t>runs</w:t></w:r><x14:keep/></w:p>{table}{}{}{SECT}",
        styled_p("Normal", "Hello world"),
        list_p(1, 0, "Item"),
        p("After table"),
        p("Last line")
    );
    let docx = Docx::new(&body).styles(STYLES).numbering(numbering).header("rIdHd", "header1.xml", "<w:p><w:r><w:t>Head</w:t></w:r></w:p>");
    let base = open_docx(&docx);
    let parts_before: Vec<(String, Vec<u8>)> = base
        .package()
        .part_names()
        .into_iter()
        .filter(|n| n.ends_with(".xml") && !n.contains("_rels") && !n.contains("Content_Types"))
        .map(|n| {
            let b = base.package().part_bytes(&n).unwrap();
            (n, b)
        })
        .collect();
    let cell = |p: usize| at(p, 0);
    let cmds: Vec<(&str, Json)> = vec![
        ("scroll.insert_text", Json::obj().with("sel", at(0, 5)).with("text", " there\nnew para")),
        ("scroll.delete", Json::obj().with("sel", sel((0, 3), (2, 2)))),
        ("scroll.delete", Json::obj().with("sel", at(1, 0)).with("back", true)),
        ("scroll.enter", Json::obj().with("sel", at(2, 6))),
        ("scroll.tab", Json::obj().with("sel", at(0, 0))),
        ("scroll.paste", Json::obj().with("sel", at(0, 0)).with("text", "x\ny")),
        ("scroll.format", Json::obj().with("sel", sel((2, 2), (2, 8))).with("bold", true).with("color", "FF0000")),
        ("scroll.toggle", Json::obj().with("sel", sel((0, 0), (0, 5))).with("prop", "italic")),
        ("scroll.font_size_step", Json::obj().with("sel", sel((0, 0), (0, 5))).with("delta", 1)),
        ("scroll.clear_format", Json::obj().with("sel", sel((2, 0), (2, 10)))),
        ("scroll.paragraph", Json::obj().with("sel", sel((0, 0), (2, 0))).with("align", "justify").with("after", 6.0)),
        ("scroll.align", Json::obj().with("sel", at(1, 0)).with("value", "right")),
        ("scroll.indent", Json::obj().with("sel", at(0, 0)).with("delta", 1)),
        ("scroll.style", Json::obj().with("sel", at(0, 0)).with("style", "Title")),
        ("scroll.list", Json::obj().with("sel", sel((0, 0), (2, 1))).with("kind", "bullet")),
        ("scroll.list_level", Json::obj().with("sel", at(1, 0)).with("delta", 1)),
        ("scroll.list_restart", Json::obj().with("sel", at(1, 0))),
        ("scroll.replace", Json::obj().with("sel", at(0, 0)).with("query", "l").with("with", "L").with("all", true)),
        ("scroll.insert_table", Json::obj().with("sel", at(0, 5)).with("rows", 2).with("cols", 2)),
        ("scroll.table_insert", Json::obj().with("sel", cell(3)).with("what", "row_below")),
        ("scroll.table_insert", Json::obj().with("sel", cell(3)).with("what", "col_left")),
        ("scroll.table_delete", Json::obj().with("sel", cell(3)).with("what", "row")),
        ("scroll.table_delete", Json::obj().with("sel", cell(4)).with("what", "col")),
        ("scroll.table_delete", Json::obj().with("sel", cell(3)).with("what", "table")),
        ("scroll.merge_cells", Json::obj().with("sel", sel((3, 0), (6, 0)))),
        ("scroll.table_widths", Json::obj().with("sel", cell(3)).with("autofit", "window")),
        ("scroll.row_height", Json::obj().with("sel", cell(3)).with("height", 20.0)),
        ("scroll.row_break", Json::obj().with("sel", cell(3)).with("allow", false)),
        ("scroll.header_rows", Json::obj().with("sel", cell(3)).with("on", true)),
        ("scroll.cell_align", Json::obj().with("sel", cell(3)).with("v", "bottom")),
        ("scroll.shading", Json::obj().with("sel", cell(3)).with("color", "EEEEEE")),
        ("scroll.borders", Json::obj().with("sel", sel((3, 0), (4, 0))).with("which", "outside").with("style", "double")),
        ("scroll.table_align", Json::obj().with("sel", cell(3)).with("align", "right")),
        ("scroll.tab", Json::obj().with("sel", cell(6))),
        ("scroll.page_break", Json::obj().with("sel", at(1, 2))),
        ("scroll.section_break", Json::obj().with("sel", at(1, 2)).with("kind", "oddPage")),
        ("scroll.page_setup", Json::obj().with("sel", at(0, 0)).with("size", "Legal").with("bottom", 50.0)),
        ("scroll.header_footer", Json::obj().with("sel", at(0, 0)).with("which", "header").with("text", "New head")),
        ("scroll.header_footer", Json::obj().with("sel", at(0, 0)).with("which", "footer").with("kind", "even")),
        ("scroll.page_number", Json::obj().with("sel", at(0, 0)).with("where", "footer")),
        ("scroll.page_number", Json::obj().with("sel", at(0, 2)).with("where", "here").with("format", "x_of_y")),
        ("scroll.page_numbering", Json::obj().with("sel", at(0, 0)).with("start", 3)),
        ("scroll.first_page", Json::obj().with("sel", at(0, 0)).with("on", true)),
        ("scroll.insert_picture", Json::obj().with("sel", at(0, 1)).with("data", b64(&png_bytes())).with("name", "p.png")),
        ("scroll.link", Json::obj().with("sel", sel((0, 0), (0, 5))).with("url", "https://example.org/")),
        ("scroll.set_paragraph_text", Json::obj().with("index", 2).with("text", "Replaced")),
    ];
    for (name, args) in cmds {
        let mut bus = Bus::new(open_docx(&docx));
        let done = bus.do_one(Command::new(name, args.clone()), Origin::User).unwrap_or_else(|e| panic!("{name} {args}: {e}"));
        assert!(done.step > 0, "{name} changed something");
        let changed_main = bus.doc().package().part_bytes("/word/document.xml").unwrap();
        bus.undo().unwrap_or_else(|e| panic!("undo {name}: {e}"));
        for (part, bytes) in &parts_before {
            assert_eq!(&bus.doc().package().part_bytes(part).unwrap(), bytes, "{name}: undo left {part} different");
        }
        // Redo does it again exactly.
        bus.redo().unwrap();
        assert_eq!(bus.doc().package().part_bytes("/word/document.xml").unwrap(), changed_main, "{name}: redo differs");
        // And the result saves and opens.
        let saved = bus.doc().save();
        assert!(ScrollDoc::open_with_fonts(&saved, Arc::new(FontIndex::new())).is_ok(), "{name}: the saved file opens");
    }
}

#[test]
fn every_writing_command_is_listed_for_the_ai() {
    let doc = doc3();
    let names: Vec<String> = doc.commands().into_iter().map(|c| c.name).collect();
    for n in [
        "scroll.insert_text", "scroll.delete", "scroll.enter", "scroll.tab", "scroll.paste", "scroll.format", "scroll.toggle", "scroll.paragraph", "scroll.style",
        "scroll.list", "scroll.find", "scroll.replace", "scroll.insert_table", "scroll.merge_cells", "scroll.page_setup", "scroll.section_break",
        "scroll.header_footer", "scroll.page_number", "scroll.insert_picture", "scroll.link", "scroll.unlink",
    ] {
        assert!(names.iter().any(|x| x == n), "{n} is listed");
    }
    assert!(!names.iter().any(|x| x == "scroll.restore"), "the undo-only command is not offered");
    let d = doc.describe();
    assert!(d.get("styles").and_then(Json::as_arr).is_some_and(|s| s.iter().any(|x| x.get("id").and_then(Json::as_str) == Some("Normal"))));
}

// ---------------------------------------------------------------------------------------------------
// Pagination rules
// ---------------------------------------------------------------------------------------------------

#[test]
fn widow_and_orphan_plans() {
    let h = [10.0; 5];
    assert_eq!(widow_plan(&h, 0, 100.0, true), None, "all fit");
    assert_eq!(widow_plan(&h, 0, 25.0, true), Some(2), "two here, three on");
    assert_eq!(widow_plan(&h, 0, 45.0, true), Some(3), "no last line alone on the next page");
    assert_eq!(widow_plan(&h, 0, 15.0, true), Some(0), "no first line alone at the foot");
    assert_eq!(widow_plan(&h, 0, 15.0, false), None, "a fresh page cannot do better");
    assert_eq!(widow_plan(&[10.0, 10.0], 0, 15.0, true), Some(0));
}

#[test]
fn keep_with_next_moves_a_heading_to_its_paragraph() {
    // Find how many filler paragraphs put the heading last on page one and its body on page two.
    let build = |n: usize, keep: bool| {
        let mut body = String::new();
        for i in 0..n {
            body.push_str(&p(&format!("Filler {i}")));
        }
        let ppr = if keep { "<w:keepNext/><w:widowControl w:val=\"0\"/>" } else { "<w:widowControl w:val=\"0\"/>" };
        body.push_str(&format!("<w:p><w:pPr>{ppr}</w:pPr><w:r><w:t>Heading</w:t></w:r></w:p>"));
        body.push_str(&format!("<w:p><w:pPr><w:widowControl w:val=\"0\"/></w:pPr><w:r><w:t>Body</w:t></w:r></w:p>{SECT}"));
        open_docx(&Docx::new(&body).styles(STYLES))
    };
    let n = (1..400)
        .find(|n| {
            let d = build(*n, false);
            let pp = &d.laid().para_page;
            pp[*n] == 0 && pp[*n + 1] == 1
        })
        .expect("some count splits them");
    let d = build(n, true);
    assert_eq!((d.laid().para_page[n], d.laid().para_page[n + 1]), (1, 1), "the heading goes with its paragraph");
}

#[test]
fn a_blank_document_takes_typing() {
    let mut bus = Bus::new(ScrollDoc::new_blank());
    assert_eq!(bus.doc().editing().unwrap().caret(&pos(0, 0)).map(|c| c.page), Some(0));
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", at(0, 0)).with("text", "Dear reader,"));
    assert_eq!(text(&bus, 0), "Dear reader,");
    run(&mut bus, "scroll.enter", Json::obj().with("sel", at(0, 12)));
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", at(1, 0)).with("text", "Hello"));
    run(&mut bus, "scroll.delete", Json::obj().with("sel", sel((1, 0), (1, 5))));
    assert_eq!(text(&bus, 1), "");
    run(&mut bus, "scroll.delete", Json::obj().with("sel", at(1, 0)).with("back", true));
    assert_eq!(bus.doc().paragraph_count(), 1);
    let again = ScrollDoc::open(&bus.doc().save()).unwrap();
    assert_eq!(again.paragraph_text(0), Some("Dear reader,"));
}

#[test]
fn typing_over_a_selection_keeps_its_first_characters_formatting() {
    let body = format!("<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Bold</w:t></w:r><w:r><w:t xml:space=\"preserve\"> plain</w:t></w:r></w:p>{SECT}");
    let mut bus = Bus::new(open_docx(&Docx::new(&body).styles(STYLES)));
    run(&mut bus, "scroll.insert_text", Json::obj().with("sel", sel((0, 0), (0, 10))).with("text", "New"));
    assert_eq!(text(&bus, 0), "New");
    assert_eq!(state(&bus, sel((0, 0), (0, 3))).get("bold").and_then(Json::as_bool), Some(true));
}
