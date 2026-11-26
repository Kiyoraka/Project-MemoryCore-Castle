//! Ledger M2 tests: editing by hand and by chat on invented workbooks (built by the tests), each
//! change saved, reopened and compared, and every command undone back to the file it started from.

use super::tests::{book, entries, open, part, sheet_xml, SST, STYLES};
use super::*;
use crate::ground::bus::{Bus, Origin};
use crate::ground::edit::{Pos, Selection};
use crate::ground::text::FontCache;

fn s_sheet() -> String {
    sheet_xml(
        "<dimension ref=\"A1:D4\"/><sheetViews><sheetView workbookViewId=\"0\"><selection activeCell=\"A1\" sqref=\"A1\"/></sheetView></sheetViews>\
<sheetFormatPr defaultRowHeight=\"15\"/><sheetData>\
<row r=\"1\"><c r=\"A1\"><v>1</v></c><c r=\"B1\"><f>A1*10</f><v>10</v></c></row>\
<row r=\"2\"><c r=\"A2\"><v>2</v></c></row>\
<row r=\"3\"><c r=\"A3\"><v>3</v></c><c r=\"C3\" s=\"12\" t=\"s\"><v>0</v></c></row>\
<row r=\"4\"><c r=\"B4\"><f>SUM(A1:A3)</f><v>6</v></c></row>\
</sheetData><mergeCells count=\"1\"><mergeCell ref=\"C1:D1\"/></mergeCells>\
<conditionalFormatting sqref=\"A1:A3\"><cfRule type=\"expression\" dxfId=\"0\" priority=\"1\"><formula>A1&gt;B4</formula></cfRule></conditionalFormatting>\
<pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/>",
    )
}

fn t_sheet() -> String {
    sheet_xml("<dimension ref=\"A1\"/><sheetData><row r=\"1\"><c r=\"A1\"><f>S!A2+1</f><v>3</v></c></row></sheetData>")
}

const NAMES: &str = "<definedNames><definedName name=\"Total\">S!$B$4</definedName><definedName name=\"Local\" localSheetId=\"1\">T!$A$1</definedName></definedNames>";

fn test_book() -> Vec<u8> {
    book(&[("S", &s_sheet(), ""), ("T", &t_sheet(), "")], Some(SST), Some(STYLES), "", NAMES)
}

fn bus() -> Bus<LedgerDoc> {
    Bus::new(open(&test_book()))
}

fn p(sheet: usize, cell: &str) -> Json {
    let (r, c) = parse_cell(cell).unwrap();
    editing::Place::new(sheet, r, c).pos().to_json()
}

fn sel(sheet: usize, a: &str, f: &str) -> Json {
    Json::obj().with("anchor", p(sheet, a)).with("focus", p(sheet, f))
}

fn run(bus: &mut Bus<LedgerDoc>, name: &str, args: Json) -> Json {
    bus.do_user(Command::new(name, args)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn val(doc: &LedgerDoc, sheet: usize, cell: &str) -> CellValue {
    let (r, c) = parse_cell(cell).unwrap();
    doc.sheets()[sheet].cell(r, c).map(|x| x.value.clone()).unwrap_or(CellValue::Empty)
}

fn formula(doc: &LedgerDoc, sheet: usize, cell: &str) -> Option<String> {
    let (r, c) = parse_cell(cell).unwrap();
    doc.sheets()[sheet].cell(r, c).and_then(|x| x.formula.clone())
}

fn reopen(doc: &LedgerDoc) -> LedgerDoc {
    open(&doc.save())
}

/// Every part's text, with the one mark a formula entry leaves for good taken out.
fn contents(bytes: &[u8]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = entries(bytes)
        .into_iter()
        .filter(|e| e.0 != "xl/sharedStrings.xml")
        .map(|e| (e.0, String::from_utf8_lossy(&e.1).replace(" fullCalcOnLoad=\"1\"", "")))
        .collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------------------------

#[test]
fn editing_hit_caret_step_and_boxes() {
    let doc = open(&test_book());
    let ed = doc.editing().expect("Ledger takes a caret");
    // Columns 48 pt wide from the row headers (28 pt), rows 15 pt from the column headers.
    let a1 = ed.hit(0, 30.0, 20.0).unwrap();
    assert_eq!(a1.to_json().to_string(), r#"{"sheet":0,"row":0,"col":0}"#);
    let b2 = ed.hit(0, 80.0, 35.0).unwrap();
    assert_eq!(b2.to_json().to_string(), r#"{"sheet":0,"row":1,"col":1}"#);
    // A merged area answers its top-left cell.
    assert_eq!(ed.hit(0, 180.0, 20.0).unwrap().to_json().to_string(), r#"{"sheet":0,"row":0,"col":2}"#);
    // A column header answers that column's first shown cell.
    assert_eq!(ed.hit(0, 80.0, 5.0).unwrap().to_json().to_string(), r#"{"sheet":0,"row":0,"col":1}"#);
    let c = ed.caret(&b2).unwrap();
    assert_eq!((c.page, c.x, c.y, c.height), (0, 76.0, 30.0, 15.0));
    // Steps: over the merge, to the data edge, to the last cell.
    let step = |from: &str, m: &str| ed.step(&Pos(p(0, from)), m).unwrap().to_json().to_string();
    assert_eq!(step("B1", "right"), r#"{"sheet":0,"row":0,"col":2}"#);
    assert_eq!(step("C1", "right"), r#"{"sheet":0,"row":0,"col":4}"#);
    assert_eq!(step("A1", "word_right"), r#"{"sheet":0,"row":0,"col":1}"#);
    assert_eq!(step("A1", "doc_end"), r#"{"sheet":0,"row":3,"col":2}"#);
    assert_eq!(step("A1", "up"), r#"{"sheet":0,"row":0,"col":0}"#);
    assert_eq!(step("B3", "line_start"), r#"{"sheet":0,"row":2,"col":0}"#);
    // The selection's box; none for a single cell.
    let s = Selection::from_json(&sel(0, "A1", "B2")).unwrap();
    let rects = ed.selection_rects(&s);
    assert_eq!(rects.len(), 1);
    assert_eq!((rects[0].1.x, rects[0].1.y, rects[0].1.w, rects[0].1.h), (28.0, 15.0, 96.0, 30.0));
    assert!(ed.selection_rects(&Selection::from_json(&sel(0, "A1", "A1")).unwrap()).is_empty());
    // The state for the ribbon and a formula bar.
    let st = ed.state(&Selection::from_json(&sel(0, "B1", "B1")).unwrap());
    assert_eq!(st.get("input").unwrap().as_str(), Some("=A1*10"));
    assert_eq!(st.get("formula"), Some(&Json::Bool(true)));
    assert_eq!(st.get("bold"), Some(&Json::Bool(false)));
    assert_eq!(st.get("format").unwrap().get("general"), Some(&Json::Bool(true)));
    let st = ed.state(&s);
    assert_eq!(st.get("stats").unwrap().get("sum").unwrap().as_f64(), Some(13.0));
    // Copy: tab-separated shown text and our own rich form.
    let clip = ed.copy(&s);
    assert_eq!(clip.text, "1\t10\r\n2\t");
    let rich = clip.rich.unwrap();
    assert_eq!(rich.get("cells").unwrap().at(0).unwrap().at(1).unwrap().get("f").unwrap().as_str(), Some("A1*10"));
}

#[test]
fn typing_enter_and_backspace() {
    let mut bus = bus();
    let mut s = sel(0, "A5", "A5");
    for ch in ["1", "2", ".", "5"] {
        let done = run(&mut bus, "ledger.insert_text", Json::obj().with("sel", s.clone()).with("text", ch));
        s = done.get("selection").unwrap().clone();
    }
    assert_eq!(s.get("focus").unwrap().get("edit").unwrap().as_str(), Some("12.5"));
    assert_eq!(val(bus.doc(), 0, "A5"), CellValue::Number(12.5));
    // Enter moves down and leaves no step.
    let steps = bus.history().len();
    let done = run(&mut bus, "ledger.enter", Json::obj().with("sel", s.clone()));
    assert_eq!(done.get("step").unwrap().as_f64(), Some(0.0));
    assert_eq!(done.get("selection").unwrap().get("focus").unwrap().to_string(), p(0, "A6").to_string());
    assert_eq!(bus.history().len(), steps);
    // A formula is worked out; percent, money and dates pick their formats.
    run(&mut bus, "ledger.insert_text", Json::obj().with("sel", sel(0, "B5", "B5")).with("text", "=A5*2"));
    assert_eq!(formula(bus.doc(), 0, "B5").as_deref(), Some("A5*2"));
    assert_eq!(val(bus.doc(), 0, "B5"), CellValue::Number(25.0));
    run(&mut bus, "ledger.insert_text", Json::obj().with("sel", sel(0, "C5", "C5")).with("text", "10%"));
    assert_eq!(bus.doc().cell_text(0, 4, 2), "10%");
    run(&mut bus, "ledger.insert_text", Json::obj().with("sel", sel(0, "D5", "D5")).with("text", "2026-11-26"));
    assert_eq!(bus.doc().cell_text(0, 4, 3), "11/26/2026");
    run(&mut bus, "ledger.insert_text", Json::obj().with("sel", sel(0, "E5", "E5")).with("text", "'007"));
    assert_eq!(val(bus.doc(), 0, "E5"), CellValue::Text("007".into()));
    // A dependant follows a change.
    run(&mut bus, "ledger.insert_text", Json::obj().with("sel", sel(0, "A1", "A1")).with("text", "4"));
    assert_eq!(val(bus.doc(), 0, "B1"), CellValue::Number(40.0));
    assert_eq!(val(bus.doc(), 0, "B4"), CellValue::Number(9.0));
    assert_eq!(val(bus.doc(), 1, "A1"), CellValue::Number(3.0));
    // Backspace on a cell not being typed in empties it and starts typing.
    let done = run(&mut bus, "ledger.backspace", Json::obj().with("sel", sel(0, "A2", "A2")));
    assert_eq!(val(bus.doc(), 0, "A2"), CellValue::Empty);
    assert_eq!(done.get("selection").unwrap().get("focus").unwrap().get("edit").unwrap().as_str(), Some(""));
    assert_eq!(val(bus.doc(), 1, "A1"), CellValue::Number(1.0));
    // The results are in the saved file.
    let re = reopen(bus.doc());
    assert_eq!(val(&re, 0, "B1"), CellValue::Number(40.0));
    assert_eq!(val(&re, 0, "B5"), CellValue::Number(25.0));
    assert!(part(&bus.doc().save(), "xl/workbook.xml").contains("fullCalcOnLoad=\"1\""));
    // All undone: the sheets are as they came.
    while bus.can_undo() {
        bus.undo().unwrap();
    }
    let orig = test_book();
    for name in ["xl/worksheets/sheet1.xml", "xl/worksheets/sheet2.xml", "xl/styles.xml"] {
        assert_eq!(part(&bus.doc().save(), name), part(&orig, name), "{name}");
    }
}

#[test]
fn formats_append_to_styles_and_undo() {
    let mut bus = bus();
    let orig = test_book();
    run(&mut bus, "ledger.toggle", Json::obj().with("sel", sel(0, "A1", "B2")).with("what", "bold"));
    let doc = bus.doc();
    let st = doc.styles();
    assert!(st.font(st.xf(doc.sheets()[0].cell(0, 0).unwrap().style).font).bold);
    assert!(st.font(st.xf(doc.sheets()[0].effective_style(1, 1)).font).bold, "an empty cell gets the format too");
    let styles = part(&doc.save(), "xl/styles.xml");
    assert!(styles.contains("<cellXfs count=\"16\">"), "{styles}");
    let re = reopen(doc);
    assert!(re.styles().font(re.styles().xf(re.sheets()[0].cell(0, 0).unwrap().style).font).bold);
    bus.undo().unwrap();
    assert_eq!(part(&bus.doc().save(), "xl/styles.xml"), part(&orig, "xl/styles.xml"));
    assert_eq!(part(&bus.doc().save(), "xl/worksheets/sheet1.xml"), part(&orig, "xl/worksheets/sheet1.xml"));

    run(&mut bus, "ledger.format", Json::obj().with("sel", sel(0, "A1", "A1")).with("fill", "#FFFF00").with("number_format", "currency").with("h_align", "center"));
    run(&mut bus, "ledger.format", Json::obj().with("sel", sel(0, "A1", "A1")).with("decimals", 1));
    let d = bus.doc();
    let xf = d.styles().xf(d.sheets()[0].cell(0, 0).unwrap().style).clone();
    assert_eq!(d.styles().format_code(xf.num_fmt), "$#,##0.000_);($#,##0.000)");
    assert_eq!(d.styles().fill(xf.fill), Some(crate::ground::display::Color::rgb(255, 255, 0)));
    assert_eq!(d.cell_text(0, 0, 0).trim_end(), "$1.000");
    run(&mut bus, "ledger.borders", Json::obj().with("sel", sel(0, "A1", "B2")).with("preset", "outside"));
    let d = bus.doc();
    let b = d.styles().border(d.styles().xf(d.sheets()[0].effective_style(0, 0)).border).unwrap().clone();
    assert!(b.top.is_some() && b.left.is_some() && b.right.is_none() && b.bottom.is_none());
    let b = d.styles().border(d.styles().xf(d.sheets()[0].effective_style(1, 1)).border).unwrap().clone();
    assert!(b.bottom.is_some() && b.right.is_some() && b.top.is_none());
    // Format Painter: take A1's formats, paint them on E7.
    run(&mut bus, "ledger.format_painter", Json::obj().with("sel", sel(0, "A1", "A1")));
    assert_eq!(bus.doc().edit_state(&Selection::from_json(&sel(0, "E7", "E7")).unwrap()).get("painter"), Some(&Json::Bool(true)));
    run(&mut bus, "ledger.format_painter", Json::obj().with("sel", sel(0, "E7", "E7")));
    assert_eq!(bus.doc().sheets()[0].effective_style(6, 4), bus.doc().sheets()[0].cell(0, 0).unwrap().style);
    // Merge & Center, then unmerge.
    run(&mut bus, "ledger.merge", Json::obj().with("sel", sel(0, "A6", "C6")));
    assert!(bus.doc().sheets()[0].merges.contains(&CellRange::parse("A6:C6").unwrap()));
    run(&mut bus, "ledger.unmerge", Json::obj().with("sel", sel(0, "B6", "B6")));
    assert!(!bus.doc().sheets()[0].merges.contains(&CellRange::parse("A6:C6").unwrap()));
    // Clear formats keeps the value.
    run(&mut bus, "ledger.clear", Json::obj().with("sel", sel(0, "A1", "A1")).with("what", "formats"));
    assert_eq!((val(bus.doc(), 0, "A1"), bus.doc().sheets()[0].cell(0, 0).unwrap().style), (CellValue::Number(1.0), 0));
    while bus.can_undo() {
        bus.undo().unwrap();
    }
    assert_eq!(contents(&bus.doc().save()), contents(&orig));
}

#[test]
fn inserting_and_deleting_rows_rewrites_every_reference() {
    let mut bus = bus();
    let orig = test_book();
    run(&mut bus, "ledger.insert", Json::obj().with("sel", sel(0, "A2", "A2")).with("what", "rows"));
    let d = bus.doc();
    assert_eq!(val(d, 0, "A3"), CellValue::Number(2.0));
    assert_eq!(formula(d, 0, "B5").as_deref(), Some("SUM(A1:A4)"));
    assert_eq!(val(d, 0, "B5"), CellValue::Number(6.0));
    assert_eq!(formula(d, 1, "A1").as_deref(), Some("S!A3+1"));
    assert_eq!(d.defined_names()[0].value, "S!$B$5");
    assert!(d.sheets()[0].merges.contains(&CellRange::parse("C1:D1").unwrap()));
    let xml = part(&d.save(), "xl/worksheets/sheet1.xml");
    assert!(xml.contains("sqref=\"A1:A4\"") && xml.contains("<formula>A1&gt;B5</formula>"), "{xml}");
    assert!(xml.contains("<dimension ref=\"A1:C5\"/>"), "{xml}");
    let re = reopen(d);
    assert_eq!(formula(&re, 0, "B5").as_deref(), Some("SUM(A1:A4)"));
    bus.undo().unwrap();
    assert_eq!(contents(&bus.doc().save()), contents(&orig));

    // A deleted column breaks what pointed into it.
    run(&mut bus, "ledger.delete", Json::obj().with("sel", sel(0, "A1", "A1")).with("what", "cols"));
    assert_eq!(formula(bus.doc(), 0, "A1").as_deref(), Some("#REF!*10"));
    assert_eq!(formula(bus.doc(), 1, "A1").as_deref(), Some("#REF!+1"));
    assert!(bus.doc().sheets()[0].merges.contains(&CellRange::parse("B1:C1").unwrap()));
    bus.undo().unwrap();
    // Cells shifted down in column A only.
    run(&mut bus, "ledger.insert", Json::obj().with("sel", sel(0, "A1", "A1")).with("what", "cells_down"));
    assert_eq!(val(bus.doc(), 0, "A2"), CellValue::Number(1.0));
    assert_eq!(formula(bus.doc(), 0, "B1").as_deref(), Some("A2*10"));
    assert_eq!(formula(bus.doc(), 0, "B4").as_deref(), Some("SUM(A2:A4)"));
    bus.undo().unwrap();
    assert_eq!(contents(&bus.doc().save()), contents(&orig));
}

#[test]
fn copy_paste_cut_and_outside_text() {
    let mut bus = bus();
    let clip = bus.doc().copy_of(&Selection::from_json(&sel(0, "B1", "B1")).unwrap());
    let done = run(&mut bus, "ledger.paste", Json::obj().with("sel", sel(0, "C3", "C3")).with("text", clip.text.clone()).with("rich", clip.rich.clone().unwrap()));
    assert_eq!(formula(bus.doc(), 0, "C3").as_deref(), Some("B3*10"));
    assert_eq!(done.get("selection").unwrap().get("focus").unwrap().to_string(), p(0, "C3").to_string());
    // Values only.
    run(&mut bus, "ledger.paste", Json::obj().with("sel", sel(0, "D3", "D3")).with("text", clip.text.clone()).with("rich", clip.rich.unwrap()).with("mode", "values"));
    assert_eq!((formula(bus.doc(), 0, "D3"), val(bus.doc(), 0, "D3")), (None, CellValue::Number(10.0)));
    // Text from outside, read as typed.
    run(&mut bus, "ledger.paste", Json::obj().with("sel", sel(0, "E1", "E1")).with("text", "x\t5\r\n=1+1\t\"a\tb\"\r\n"));
    assert_eq!(val(bus.doc(), 0, "E1"), CellValue::Text("x".into()));
    assert_eq!(val(bus.doc(), 0, "F1"), CellValue::Number(5.0));
    assert_eq!(val(bus.doc(), 0, "E2"), CellValue::Number(2.0));
    assert_eq!(val(bus.doc(), 0, "F2"), CellValue::Text("a\tb".into()));
    // One value pasted over a block fills it.
    run(&mut bus, "ledger.paste", Json::obj().with("sel", sel(0, "G1", "G3")).with("text", "7"));
    assert_eq!(val(bus.doc(), 0, "G3"), CellValue::Number(7.0));
    // Cut and paste moves: what pointed at the cells follows.
    let s = sel(0, "A1", "A2");
    let clip = bus.doc().copy_of(&Selection::from_json(&s).unwrap());
    run(&mut bus, "ledger.cut", Json::obj().with("sel", s));
    run(&mut bus, "ledger.paste", Json::obj().with("sel", sel(0, "H1", "H1")).with("text", clip.text).with("rich", clip.rich.unwrap()));
    let d = bus.doc();
    assert_eq!((val(d, 0, "A1"), val(d, 0, "H2")), (CellValue::Empty, CellValue::Number(2.0)));
    assert_eq!(formula(d, 0, "B1").as_deref(), Some("H1*10"));
    assert_eq!(formula(d, 1, "A1").as_deref(), Some("S!H2+1"));
    assert_eq!(formula(d, 0, "B4").as_deref(), Some("SUM(A1:A3)"), "a range only partly moved stays");
    assert_eq!(val(d, 1, "A1"), CellValue::Number(3.0));
}

#[test]
fn sheets_added_renamed_moved_deleted() {
    let mut bus = bus();
    let orig = test_book();
    let done = run(&mut bus, "ledger.add_sheet", Json::obj().with("name", "New").with("at", 1));
    assert_eq!(done.get("selection").unwrap().get("focus").unwrap().get("sheet").unwrap().as_f64(), Some(1.0));
    let names: Vec<String> = bus.doc().sheets().iter().map(|s| s.name.clone()).collect();
    assert_eq!(names, ["S", "New", "T"]);
    assert_eq!(bus.doc().defined_names()[1].local_sheet, Some(2), "a sheet-scoped name follows its sheet");
    let saved = bus.doc().save();
    assert!(part(&saved, "[Content_Types].xml").contains("/xl/worksheets/sheet3.xml"));
    assert!(part(&saved, "xl/_rels/workbook.xml.rels").contains("worksheets/sheet3.xml"));
    run(&mut bus, "ledger.set_cell", Json::obj().with("sheet", "New").with("cell", "A1").with("input", "=T!A1*2"));
    assert_eq!(val(bus.doc(), 1, "A1"), CellValue::Number(6.0));
    let re = reopen(bus.doc());
    assert_eq!(re.sheets().len(), 3);
    assert_eq!(val(&re, 1, "A1"), CellValue::Number(6.0));

    run(&mut bus, "ledger.rename_sheet", Json::obj().with("sheet", "S").with("name", "My S"));
    assert_eq!(formula(bus.doc(), 2, "A1").as_deref(), Some("'My S'!A2+1"));
    assert_eq!(bus.doc().defined_names()[0].value, "'My S'!$B$4");
    assert!(bus.do_one(Command::new("ledger.rename_sheet", Json::obj().with("sheet", "T").with("name", "bad:name")), Origin::User).is_err());
    run(&mut bus, "ledger.move_sheet", Json::obj().with("sheet", "T").with("to", 0));
    let names: Vec<String> = bus.doc().sheets().iter().map(|s| s.name.clone()).collect();
    assert_eq!(names, ["T", "My S", "New"]);
    assert_eq!(bus.doc().defined_names()[1].local_sheet, Some(0));
    run(&mut bus, "ledger.delete_sheet", Json::obj().with("sheet", "My S"));
    assert_eq!(formula(bus.doc(), 0, "A1").as_deref(), Some("#REF!+1"));
    assert!(!part(&bus.doc().save(), "[Content_Types].xml").contains("/xl/worksheets/sheet1.xml"));
    let re = reopen(bus.doc());
    assert_eq!(re.sheets().iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["T", "New"]);
    while bus.can_undo() {
        bus.undo().unwrap();
    }
    assert_eq!(contents(&bus.doc().save()), contents(&orig));
}

#[test]
fn sort_filter_find_replace() {
    let mut bus = bus();
    run(&mut bus, "ledger.set_cells", Json::obj().with("sheet", "S").with("cell", "F1").with("values", Json::parse(r#"[["Name","Score"],["cy",3],["ann",10],["bo",7]]"#).unwrap()));
    run(&mut bus, "ledger.sort", Json::obj().with("sel", sel(0, "G2", "G2")).with("descending", true));
    let d = bus.doc();
    assert_eq!(val(d, 0, "F1"), CellValue::Text("Name".into()), "the header stays");
    assert_eq!((val(d, 0, "F2"), val(d, 0, "G2")), (CellValue::Text("ann".into()), CellValue::Number(10.0)));
    assert_eq!(val(d, 0, "F4"), CellValue::Text("cy".into()));
    run(&mut bus, "ledger.sort", Json::obj().with("sheet", "S").with("range", "F1:G4").with("by", "F"));
    assert_eq!(val(bus.doc(), 0, "F2"), CellValue::Text("ann".into()));
    assert_eq!(val(bus.doc(), 0, "F3"), CellValue::Text("bo".into()));

    // AutoFilter on the region, then only "bo".
    run(&mut bus, "ledger.filter", Json::obj().with("sel", sel(0, "F1", "F1")));
    assert_eq!(bus.doc().sheets()[0].auto_filter, CellRange::parse("F1:G4"));
    run(&mut bus, "ledger.filter", Json::obj().with("sel", sel(0, "F1", "F1")).with("column", "F").with("values", Json::parse(r#"["bo"]"#).unwrap()));
    let d = bus.doc();
    assert!(d.sheets()[0].rows[&1].hidden && !d.sheets()[0].rows[&2].hidden && d.sheets()[0].rows[&3].hidden);
    let saved = d.save();
    assert!(part(&saved, "xl/worksheets/sheet1.xml").contains("<autoFilter ref=\"F1:G4\"><filterColumn colId=\"0\"><filters><filter val=\"bo\"/></filters></filterColumn></autoFilter>"));
    assert!(part(&saved, "xl/workbook.xml").contains("<definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"0\" hidden=\"1\">S!$F$1:$G$4</definedName>"));
    run(&mut bus, "ledger.clear_filter", Json::obj().with("sheet", "S"));
    assert!(!bus.doc().sheets()[0].rows[&1].hidden);
    run(&mut bus, "ledger.remove_filter", Json::obj().with("sheet", "S"));
    assert_eq!(bus.doc().sheets()[0].auto_filter, None);

    // Find goes to the match and leaves no step; replace in formulas and values.
    let done = run(&mut bus, "ledger.find", Json::obj().with("sel", sel(0, "A1", "A1")).with("query", "BO").with("next", true));
    assert_eq!(done.get("step").unwrap().as_f64(), Some(0.0));
    assert_eq!(done.get("selection").unwrap().get("focus").unwrap().to_string(), p(0, "F3").to_string());
    run(&mut bus, "ledger.replace", Json::obj().with("sel", sel(0, "A1", "A1")).with("query", "A1").with("with", "A2").with("all", true));
    assert_eq!(formula(bus.doc(), 0, "B1").as_deref(), Some("A2*10"));
    assert_eq!(val(bus.doc(), 0, "B1"), CellValue::Number(20.0));
    run(&mut bus, "ledger.replace", Json::obj().with("sel", sel(0, "F2", "F2")).with("query", "ann").with("with", "Anna").with("all", false));
    assert_eq!(val(bus.doc(), 0, "F2"), CellValue::Text("Anna".into()));
}

#[test]
fn fill_autofill_autosum_freeze_and_sizes() {
    let mut bus = bus();
    run(&mut bus, "ledger.autosum", Json::obj().with("sel", sel(0, "A4", "A4")));
    assert_eq!(formula(bus.doc(), 0, "A4").as_deref(), Some("SUM(A1:A3)"));
    assert_eq!(val(bus.doc(), 0, "A4"), CellValue::Number(6.0));
    run(&mut bus, "ledger.autofill", Json::obj().with("sel", sel(0, "A1", "A3")).with("to", "A1:A6"));
    assert_eq!((val(bus.doc(), 0, "A4"), val(bus.doc(), 0, "A6")), (CellValue::Number(4.0), CellValue::Number(6.0)));
    run(&mut bus, "ledger.set_cells", Json::obj().with("cell", "J1").with("values", Json::parse(r#"[["Item 1"],["2026-01-31"]]"#).unwrap()));
    run(&mut bus, "ledger.autofill", Json::obj().with("sel", sel(0, "J1", "J1")).with("to", "J1:J3"));
    assert_eq!(val(bus.doc(), 0, "J3"), CellValue::Text("Item 3".into()));
    run(&mut bus, "ledger.fill", Json::obj().with("sel", sel(0, "B1", "B3")).with("direction", "down"));
    assert_eq!(formula(bus.doc(), 0, "B3").as_deref(), Some("A3*10"));
    assert_eq!(val(bus.doc(), 0, "B3"), CellValue::Number(30.0));

    run(&mut bus, "ledger.freeze", Json::obj().with("sel", sel(0, "B2", "B2")));
    assert_eq!(bus.doc().sheets()[0].frozen(), (1, 1));
    assert!(part(&bus.doc().save(), "xl/worksheets/sheet1.xml").contains("<pane xSplit=\"1\" ySplit=\"1\" topLeftCell=\"B2\" activePane=\"bottomRight\" state=\"frozen\"/>"));
    run(&mut bus, "ledger.unfreeze", Json::obj().with("sheet", "S"));
    assert_eq!(bus.doc().sheets()[0].frozen(), (0, 0));

    run(&mut bus, "ledger.col_width", Json::obj().with("sel", sel(0, "B1", "C1")).with("width", 20));
    assert!(part(&bus.doc().save(), "xl/worksheets/sheet1.xml").contains("<cols><col min=\"2\" max=\"4\" width=\"20\" customWidth=\"1\"/></cols>"), "the selection grows over the merged C1:D1");
    run(&mut bus, "ledger.col_width", Json::obj().with("sel", sel(0, "J1", "J1")).with("auto", true));
    assert!(bus.doc().sheets()[0].col_width(9) > 0.0);
    run(&mut bus, "ledger.row_height", Json::obj().with("sel", sel(0, "A2", "A3")).with("height", 30));
    assert_eq!(bus.doc().sheets()[0].row_height(2), 30.0);
    run(&mut bus, "ledger.hide", Json::obj().with("sel", sel(0, "D1", "D1")).with("what", "cols").with("hidden", true));
    assert_eq!(bus.doc().sheets()[0].col_width(3), 0.0);
    run(&mut bus, "ledger.hide", Json::obj().with("sel", sel(0, "A2", "A2")).with("what", "rows").with("hidden", true));
    assert_eq!(bus.doc().sheets()[0].row_height(1), 0.0);
    // Moving down skips the hidden row.
    let ed = bus.doc().editing().unwrap();
    assert_eq!(ed.step(&Pos(p(0, "A1")), "down").unwrap().to_json().to_string(), p(0, "A3").to_string());
    let re = reopen(bus.doc());
    assert_eq!(re.sheets()[0].row_height(1), 0.0);
    assert_eq!(re.sheets()[0].row_height(2), 30.0);
}

#[test]
fn csv_open_and_save() {
    let bytes = b"\xEF\xBB\xBFname;code;n\r\nann;007;1,5\r\n\"b;o\";42;2\r\n";
    let doc = LedgerDoc::open_csv(bytes, "data", FontCache::empty()).unwrap();
    assert_eq!(doc.sheets()[0].name, "data");
    assert_eq!(val(&doc, 0, "B2"), CellValue::Text("007".into()), "leading zeros stay");
    assert_eq!(val(&doc, 0, "A3"), CellValue::Text("b;o".into()));
    assert_eq!(val(&doc, 0, "B3"), CellValue::Number(42.0));
    let out = doc.save_csv(0, true);
    assert!(out.starts_with(&[0xEF, 0xBB, 0xBF]));
    assert_eq!(String::from_utf8(out[3..].to_vec()).unwrap(), "name,code,n\r\nann,007,\"1,5\"\r\nb;o,42,2\r\n");
    assert!(!doc.save_csv(0, false).starts_with(&[0xEF]));
    // The workbook itself saves and reopens.
    let re = open(&doc.save());
    assert_eq!(val(&re, 0, "B2"), CellValue::Text("007".into()));
    // The AI can read a sheet as CSV.
    let r = doc.read(&Json::obj().with("csv", true)).unwrap();
    assert!(r.get("csv").unwrap().as_str().unwrap().starts_with("name,code,n\r\n"));
}

#[test]
fn every_command_undoes_to_the_file_it_started_from() {
    let orig = test_book();
    let cases: Vec<(&str, Json)> = vec![
        ("ledger.set_cell", Json::obj().with("cell", "E5").with("input", "=B4+1")),
        ("ledger.set_cells", Json::obj().with("cell", "E1").with("values", Json::parse(r#"[[1,"two"],[true,"5%"]]"#).unwrap())),
        ("ledger.insert_text", Json::obj().with("sel", sel(0, "C3", "C3")).with("text", "new")),
        ("ledger.backspace", Json::obj().with("sel", sel(0, "B1", "B1"))),
        ("ledger.enter", Json::obj().with("sel", sel(0, "A1", "A3")).with("ctrl", true)),
        ("ledger.clear", Json::obj().with("sel", sel(0, "A1", "C4")).with("what", "all")),
        ("ledger.fill", Json::obj().with("sel", sel(0, "A1", "B4")).with("direction", "down")),
        ("ledger.autofill", Json::obj().with("sel", sel(0, "A1", "A2")).with("to", "A1:A9")),
        ("ledger.paste", Json::obj().with("sel", sel(0, "D5", "D5")).with("text", "a\tb\r\n1\t2")),
        ("ledger.move_range", Json::obj().with("from", "A1:B2").with("to", "F7")),
        ("ledger.autosum", Json::obj().with("sel", sel(0, "A1", "A3"))),
        ("ledger.format", Json::obj().with("range", "A1:B3").with("bold", true).with("size", 14).with("color", "#C00000").with("wrap", true).with("number_format", "0.0")),
        ("ledger.format", Json::obj().with("range", "B:B").with("italic", true)),
        ("ledger.format", Json::obj().with("range", "2:2").with("fill", "#DDEBF7")),
        ("ledger.toggle", Json::obj().with("sel", sel(0, "A1", "A1")).with("what", "underline")),
        ("ledger.borders", Json::obj().with("range", "A1:C3").with("preset", "all").with("style", "double")),
        ("ledger.copy_format", Json::obj().with("range", "E5:F6").with("from", "C3")),
        ("ledger.merge", Json::obj().with("range", "A1:B2").with("mode", "across")),
        ("ledger.unmerge", Json::obj().with("range", "C1")),
        ("ledger.insert", Json::obj().with("range", "B1").with("what", "cols").with("count", 2)),
        ("ledger.delete", Json::obj().with("range", "A2:A3").with("what", "rows")),
        ("ledger.delete", Json::obj().with("range", "A1").with("what", "cells_left")),
        ("ledger.insert", Json::obj().with("range", "A2:B2").with("what", "cells_right")),
        ("ledger.row_height", Json::obj().with("range", "A1:A3").with("auto", true)),
        ("ledger.col_width", Json::obj().with("range", "A1:C1").with("auto", true)),
        ("ledger.hide", Json::obj().with("range", "A1:B1").with("what", "cols").with("hidden", true)),
        ("ledger.sort", Json::obj().with("range", "A1:B4").with("by", "A").with("descending", true).with("header", false)),
        ("ledger.filter", Json::obj().with("range", "A1:B4").with("column", "A").with("values", Json::parse(r#"["2"]"#).unwrap())),
        ("ledger.replace", Json::obj().with("sel", sel(0, "A1", "A1")).with("query", "1").with("with", "9").with("all", true).with("workbook", true)),
        ("ledger.freeze", Json::obj().with("range", "C3")),
        ("ledger.add_sheet", Json::obj()),
        ("ledger.rename_sheet", Json::obj().with("sheet", "T").with("name", "Two words")),
        ("ledger.delete_sheet", Json::obj().with("sheet", "S")),
        ("ledger.move_sheet", Json::obj().with("sheet", 1).with("to", 0)),
        ("ledger.import_csv", Json::obj().with("cell", "H1").with("text", "a,b\n1,2\n")),
    ];
    for (name, args) in cases {
        let mut bus = bus();
        let done = bus.do_one(Command::new(name, args.clone()), Origin::Ai).unwrap_or_else(|e| panic!("{name} {args}: {e}"));
        assert!(done.step > 0, "{name} {args} changed nothing");
        assert_ne!(contents(&bus.doc().save()), contents(&orig), "{name} {args}");
        // What was saved reopens.
        open(&bus.doc().save());
        bus.undo().unwrap();
        assert_eq!(contents(&bus.doc().save()), contents(&orig), "{name} {args} undone");
        bus.redo().unwrap_or_else(|e| panic!("{name} redo: {e}"));
        assert_ne!(contents(&bus.doc().save()), contents(&orig), "{name} {args} redone");
    }
}

#[test]
fn moves_and_select_all_leave_no_step() {
    let mut bus = bus();
    let done = run(&mut bus, "ledger.move", Json::obj().with("sel", sel(0, "A1", "A1")).with("motion", "down").with("extend", true));
    assert_eq!(done.get("step").unwrap().as_f64(), Some(0.0));
    let s = done.get("selection").unwrap();
    assert_eq!(s.get("anchor").unwrap().to_string(), p(0, "A1").to_string());
    assert_eq!(s.get("focus").unwrap().to_string(), p(0, "A2").to_string());
    // Far down: the view scrolls so the cell shows.
    run(&mut bus, "ledger.move", Json::obj().with("sel", sel(0, "A1", "A1")).with("motion", "doc_end"));
    let mut s = sel(0, "A60", "A60");
    for _ in 0..3 {
        s = run(&mut bus, "ledger.move", Json::obj().with("sel", s).with("motion", "down")).get("selection").unwrap().clone();
    }
    assert!(bus.doc().sheets()[0].view.row > 0, "the view followed the active cell");
    let done = run(&mut bus, "ledger.select_all", Json::obj().with("sel", sel(0, "B2", "B2")));
    assert_eq!(done.get("selection").unwrap().get("focus").unwrap().to_string(), p(0, "C4").to_string());
    assert!(!bus.can_undo());
    assert!(bus.door("describe", &Json::Null).unwrap().get("editable") == Some(&Json::Bool(true)));
    let _ = Origin::User;
}
