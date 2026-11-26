//! Ledger tests on invented workbooks, built here with our own zip writer and XML strings.

use super::grid::{row_header_width, HEADER_H};
use super::*;
use crate::ground::bus::{Bus, Origin};
use crate::ground::display::{path_bounds, Color, Item, Paint, PathOp, Rect};
use crate::ground::text::FontCache;
use crate::ground::zip::{Method, ZipArchive, ZipWriter};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

fn sheet_xml(body: &str) -> String {
    format!(
        "{DECL}<worksheet xmlns=\"{MAIN}\" xmlns:r=\"{R}\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
xmlns:x14ac=\"http://schemas.microsoft.com/office/spreadsheetml/2009/9/ac\" mc:Ignorable=\"x14ac\">{body}</worksheet>"
    )
}

/// Builds an `.xlsx`: sheets (name, worksheet XML, state), optional shared strings and styles,
/// workbookPr attributes and definedNames XML. A core-properties part and an unknown customXml
/// part ride along to prove they survive.
fn book(sheets: &[(&str, &str, &str)], sst: Option<&str>, styles: Option<&str>, wb_pr: &str, names: &str) -> Vec<u8> {
    let mut ct = format!(
        "{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>"
    );
    let mut rels = format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    let mut list = String::new();
    for (i, (name, _, state)) in sheets.iter().enumerate() {
        ct.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",
            i + 1
        ));
        rels.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"{R}/worksheet\" Target=\"worksheets/sheet{}.xml\"/>",
            i + 1,
            i + 1
        ));
        let st = if state.is_empty() { String::new() } else { format!(" state=\"{state}\"") };
        list.push_str(&format!("<sheet name=\"{name}\" sheetId=\"{}\"{st} r:id=\"rId{}\"/>", i + 1, i + 1));
    }
    let n = sheets.len();
    if styles.is_some() {
        ct.push_str("<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>");
        rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"{R}/styles\" Target=\"styles.xml\"/>", n + 1));
    }
    if sst.is_some() {
        ct.push_str("<Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/>");
        rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"{R}/sharedStrings\" Target=\"sharedStrings.xml\"/>", n + 2));
    }
    ct.push_str("</Types>");
    rels.push_str("</Relationships>");
    let root_rels = format!(
        "{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"{R}/officeDocument\" Target=\"xl/workbook.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
</Relationships>"
    );
    let workbook = format!(
        "{DECL}<workbook xmlns=\"{MAIN}\" xmlns:r=\"{R}\"><workbookPr {wb_pr}/><bookViews><workbookView activeTab=\"0\"/></bookViews>\
<sheets>{list}</sheets>{names}<calcPr calcId=\"191029\"/></workbook>"
    );
    let core = format!(
        "{DECL}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>Invented</dc:title></cp:coreProperties>"
    );
    let mut z = ZipWriter::new();
    let mut add = |name: &str, body: &str| z.add(name, body.as_bytes(), Method::Deflated(6)).unwrap();
    add("[Content_Types].xml", &ct);
    add("_rels/.rels", &root_rels);
    add("docProps/core.xml", &core);
    add("xl/workbook.xml", &workbook);
    add("xl/_rels/workbook.xml.rels", &rels);
    for (i, (_, xml, _)) in sheets.iter().enumerate() {
        add(&format!("xl/worksheets/sheet{}.xml", i + 1), xml);
    }
    if let Some(s) = styles {
        add("xl/styles.xml", s);
    }
    if let Some(s) = sst {
        add("xl/sharedStrings.xml", s);
    }
    add("customXml/item1.xml", "<root><keep>me</keep></root>");
    z.finish()
}

const SST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" count=\"5\" uniqueCount=\"4\">\
<si><t>Name</t></si>\
<si><r><t xml:space=\"preserve\">Hello </t></r><r><rPr><b/></rPr><t>World</t></r><rPh sb=\"0\" eb=\"1\"><t>HA</t></rPh></si>\
<si><t>007</t></si>\
<si><t>Line_x000D_Two</t></si></sst>";

const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
<numFmts count=\"2\"><numFmt numFmtId=\"164\" formatCode=\"&quot;$&quot;#,##0.00;[Red]\\-&quot;$&quot;#,##0.00\"/><numFmt numFmtId=\"165\" formatCode=\"yyyy\\-mm\\-dd\"/></numFmts>\
<fonts count=\"2\"><font><sz val=\"11\"/><color theme=\"1\"/><name val=\"Calibri\"/></font><font><b/><sz val=\"14\"/><color rgb=\"FFFF0000\"/><name val=\"Arial\"/></font></fonts>\
<fills count=\"4\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill>\
<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FFFFFF00\"/><bgColor indexed=\"64\"/></patternFill></fill>\
<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF00B050\"/></patternFill></fill></fills>\
<borders count=\"2\"><border><left/><right/><top/><bottom/><diagonal/></border>\
<border><left style=\"thin\"><color rgb=\"FF0000FF\"/></left><right style=\"thin\"><color rgb=\"FF0000FF\"/></right><top style=\"medium\"><color rgb=\"FF0000FF\"/></top><bottom style=\"dashed\"><color rgb=\"FF0000FF\"/></bottom><diagonal/></border></borders>\
<cellXfs count=\"15\">\
<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"2\" fontId=\"0\" fillId=\"0\" borderId=\"0\" applyNumberFormat=\"1\"/>\
<xf numFmtId=\"3\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"9\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"164\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"165\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"15\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"18\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"46\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"11\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"12\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"49\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
<xf numFmtId=\"0\" fontId=\"1\" fillId=\"2\" borderId=\"1\"><alignment horizontal=\"center\"/></xf>\
<xf numFmtId=\"0\" fontId=\"0\" fillId=\"3\" borderId=\"0\"><alignment horizontal=\"center\" vertical=\"center\"/></xf>\
<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"><alignment wrapText=\"1\"/></xf>\
</cellXfs></styleSheet>";

fn data_sheet() -> String {
    sheet_xml(
        "<dimension ref=\"A1:C4\"/>\
<sheetViews><sheetView tabSelected=\"1\" workbookViewId=\"0\"><pane xSplit=\"1\" ySplit=\"1\" topLeftCell=\"B2\" activePane=\"bottomRight\" state=\"frozen\"/>\
<selection pane=\"bottomRight\" activeCell=\"B2\" sqref=\"B2\"/></sheetView></sheetViews>\
<sheetFormatPr defaultRowHeight=\"15\" x14ac:dyDescent=\"0.25\"/>\
<cols><col min=\"1\" max=\"1\" width=\"20.7109375\" customWidth=\"1\"/><col min=\"4\" max=\"4\" width=\"9.140625\" hidden=\"1\"/></cols>\
<sheetData>\
<row r=\"1\" spans=\"1:3\" x14ac:dyDescent=\"0.25\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"s\"><v>1</v></c><c r=\"C1\" t=\"inlineStr\"><is><t>Inline</t></is></c></row>\
<row r=\"2\" spans=\"1:3\"><c r=\"A2\"><v>10</v></c><c r=\"B2\"><v>2.5</v></c><c r=\"C2\"><f>A2*B2</f><v>25</v></c></row>\
<row r=\"3\" spans=\"1:3\"><c r=\"A3\"><v>20</v></c><c r=\"B3\" t=\"b\"><v>1</v></c><c r=\"C3\"><f t=\"shared\" ref=\"C3:C4\" si=\"0\">A3*2</f><v>40</v></c></row>\
<row r=\"4\" spans=\"1:3\" ht=\"30\" customHeight=\"1\"><c r=\"A4\" t=\"e\"><v>#DIV/0!</v></c><c r=\"B4\" t=\"str\"><f>\"x\"&amp;\"y\"</f><v>xy</v></c><c r=\"C4\"><f t=\"shared\" si=\"0\"/><v>0</v></c></row>\
</sheetData>\
<pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/>\
<extLst><ext uri=\"{01252117-D84E-4E92-8308-4BE1C098FCBB}\"><x14ac:unknownThing val=\"1\"/></ext></extLst>",
    )
}

fn fmt_sheet() -> String {
    let cells: &[(&str, &str, u32)] = &[
        ("3.14159", "", 1),
        ("1234567.891", "", 2),
        ("0.256", "", 3),
        ("-1234.5", "", 4),
        ("45000", "", 5),
        ("45000", "", 6),
        ("0.75", "", 7),
        ("1.5", "", 8),
        ("12345.678", "", 9),
        ("1.5", "", 10),
        ("2", "s", 11),
        ("1234567.891", "", 0),
        ("0.0001", "", 0),
        ("1234.5", "", 4),
        ("1", "", 5),
        ("60", "", 5),
        ("61", "", 5),
        ("2023-03-15T18:00:00", "d", 7),
    ];
    let mut rows = String::new();
    for (i, (v, t, s)) in cells.iter().enumerate() {
        let t = if t.is_empty() { String::new() } else { format!(" t=\"{t}\"") };
        rows.push_str(&format!("<row r=\"{}\"><c r=\"A{}\" s=\"{s}\"{t}><v>{v}</v></c></row>", i + 1, i + 1));
    }
    sheet_xml(&format!("<dimension ref=\"A1:A{}\"/><sheetData>{rows}</sheetData>", cells.len()))
}

const NAMES: &str = "<definedNames><definedName name=\"Rate\">Data!$B$2</definedName>\
<definedName name=\"_xlnm.Print_Area\" localSheetId=\"0\">Data!$A$1:$C$4</definedName>\
<definedName name=\"Secret\" hidden=\"1\">42</definedName></definedNames>";

fn main_book() -> Vec<u8> {
    let data = data_sheet();
    let fmt = fmt_sheet();
    let hidden = sheet_xml("<sheetData/>");
    book(&[("Data", &data, ""), ("Fmt", &fmt, ""), ("Secret Tab", &hidden, "hidden")], Some(SST), Some(STYLES), "defaultThemeVersion=\"166925\"", NAMES)
}

fn open(bytes: &[u8]) -> LedgerDoc {
    LedgerDoc::open_with_fonts(bytes, FontCache::empty()).unwrap()
}

/// Every zip entry: (name, uncompressed bytes, stored bytes).
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>, Vec<u8>)> {
    let a = ZipArchive::read(bytes).unwrap();
    a.entries.iter().map(|e| (e.name.clone(), a.data(bytes, e).unwrap(), a.raw_data(bytes, e).unwrap().to_vec())).collect()
}

fn part(bytes: &[u8], name: &str) -> String {
    let e = entries(bytes);
    String::from_utf8(e.into_iter().find(|x| x.0 == name).unwrap().1).unwrap()
}

fn set(doc: &mut LedgerDoc, sheet: &str, cell: &str, value: Json) -> Change {
    doc.apply(&Command::new("ledger.set_cell", Json::obj().with("sheet", sheet).with("cell", cell).with("value", value))).unwrap()
}

fn fills(dl: &DisplayList, c: Color) -> Vec<Rect> {
    dl.items
        .iter()
        .filter_map(|i| match i {
            Item::Path { ops, fill: Some(Paint::Solid(f)), .. } if *f == c => path_bounds(ops),
            _ => None,
        })
        .collect()
}

fn strokes(dl: &DisplayList, c: Color) -> Vec<Vec<PathOp>> {
    dl.items
        .iter()
        .filter_map(|i| match i {
            Item::Path { ops, stroke: Some(s), .. } if s.color == c => Some(ops.clone()),
            _ => None,
        })
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

// ---------------------------------------------------------------------------------------------------

#[test]
fn open_then_save_unchanged_is_byte_identical() {
    let bytes = main_book();
    let doc = open(&bytes);
    assert_eq!(doc.save(), bytes);
    // Scrolling is a view change, not a file change.
    let mut doc = doc;
    doc.apply(&Command::new("ledger.scroll_to", Json::obj().with("sheet", "Data").with("cell", "C9"))).unwrap();
    assert_eq!(doc.save(), bytes);
}

#[test]
fn workbook_model_sheets_names_and_views() {
    let doc = open(&main_book());
    let names: Vec<&str> = doc.sheets().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Data", "Fmt", "Secret Tab"]);
    assert_eq!(doc.sheets()[2].state, SheetState::Hidden);
    assert_eq!(doc.sheets()[0].rel_id, "rId1");
    assert_eq!(doc.sheets()[1].sheet_id, 2);
    assert_eq!(doc.sheets()[0].part.as_deref(), Some("/xl/worksheets/sheet1.xml"));
    assert!(!doc.date1904());
    let dn = doc.defined_names();
    assert_eq!(dn.len(), 3);
    assert_eq!((dn[1].name.as_str(), dn[1].local_sheet, dn[1].value.as_str()), ("_xlnm.Print_Area", Some(0), "Data!$A$1:$C$4"));
    assert!(dn[2].hidden);
    let s = &doc.sheets()[0];
    assert_eq!(s.frozen(), (1, 1));
    assert_eq!((s.view.row, s.view.col), (1, 1));
    assert_eq!(s.active_cell, Some((1, 1)));
    assert_eq!(s.dimension.as_deref(), Some("A1:C4"));
    assert!(close(s.col_width(0), 108.75), "{}", s.col_width(0)); // 20.71 stored = 20 digits + padding = 145 px
    assert_eq!(s.col_width(3), 0.0);
    assert!(close(s.col_width(1), 48.0));
    assert_eq!(s.row_height(3), 30.0);
    assert_eq!(s.row_height(1), 15.0);
}

#[test]
fn cell_types_formulas_and_shared_formulas() {
    let doc = open(&main_book());
    let s = &doc.sheets()[0];
    let v = |r, c| s.cell(r, c).map(|c| c.value.clone()).unwrap_or(CellValue::Empty);
    assert_eq!(v(0, 0), CellValue::Text("Name".into()));
    assert_eq!(v(0, 2), CellValue::Text("Inline".into()));
    assert_eq!(v(1, 1), CellValue::Number(2.5));
    assert_eq!(v(2, 1), CellValue::Bool(true));
    assert_eq!(v(3, 0), CellValue::Error("#DIV/0!".into()));
    assert_eq!(v(3, 1), CellValue::Text("xy".into()));
    assert_eq!(s.cell(1, 2).unwrap().formula.as_deref(), Some("A2*B2"));
    assert_eq!(s.cell(3, 1).unwrap().formula.as_deref(), Some("\"x\"&\"y\""));
    assert_eq!(s.cell(2, 2).unwrap().formula.as_deref(), Some("A3*2"));
    // The shared formula's child, expanded for the formula bar; its cached value is what shows.
    assert_eq!(s.cell(3, 2).unwrap().formula.as_deref(), Some("A4*2"));
    assert_eq!(v(3, 2), CellValue::Number(0.0));
    assert_eq!(s.used_range().unwrap().to_string(), "A1:C4");
}

#[test]
fn shared_strings_rich_runs_and_escapes() {
    let doc = open(&main_book());
    assert_eq!(doc.shared_strings(), ["Name", "Hello World", "007", "Line\rTwo"]);
    assert_eq!(doc.cell_text(0, 0, 1), "Hello World");
}

#[test]
fn number_formats_through_styles() {
    let doc = open(&main_book());
    let t = |r| doc.cell_text(1, r, 0);
    assert_eq!(t(0), "3.14");
    assert_eq!(t(1), "1,234,568");
    assert_eq!(t(2), "26%");
    assert_eq!(t(3), "-$1,234.50");
    assert_eq!(t(4), "2023-03-15");
    assert_eq!(t(5), "15-Mar-23");
    assert_eq!(t(6), "6:00 PM");
    assert_eq!(t(7), "36:00:00");
    assert_eq!(t(8), "1.23E+04");
    assert_eq!(t(9), "1 1/2");
    assert_eq!(t(10), "007");
    assert_eq!(t(11), "1234567.891");
    assert_eq!(t(12), "0.0001");
    assert_eq!(t(13), "$1,234.50");
    assert_eq!(t(14), "1900-01-01");
    assert_eq!(t(15), "1900-02-29");
    assert_eq!(t(16), "1900-03-01");
    // A `t="d"` cell is read as its serial.
    assert_eq!(doc.sheets()[1].cell(17, 0).unwrap().value, CellValue::Number(45000.75));
    assert_eq!(t(17), "6:00 PM");
    // The negative section is red.
    let f = format_value_full(&CellValue::Number(-1234.5), doc.styles().format_code(164), false);
    assert_eq!(f.color, Some(Color::rgb(255, 0, 0)));
}

#[test]
fn the_1904_date_system() {
    let sheet = sheet_xml("<sheetData><row r=\"1\"><c r=\"A1\" s=\"5\"><v>0</v></c><c r=\"B1\" s=\"5\"><v>43538</v></c></row></sheetData>");
    let bytes = book(&[("S", &sheet, "")], None, Some(STYLES), "date1904=\"1\"", "");
    let doc = open(&bytes);
    assert!(doc.date1904());
    assert_eq!(doc.cell_text(0, 0, 0), "1904-01-01");
    assert_eq!(doc.cell_text(0, 0, 1), "2023-03-15");
    assert_eq!(doc.describe().get("date1904"), Some(&Json::Bool(true)));
}

#[test]
fn set_cell_then_reopen_and_other_parts_untouched() {
    let bytes = main_book();
    let mut doc = open(&bytes);
    set(&mut doc, "Data", "B5", Json::Num(99.0));
    set(&mut doc, "Data", "A1", Json::from("Renamed & <ok>"));
    set(&mut doc, "Data", "C1", Json::Num(-3.25));
    set(&mut doc, "Data", "E2", Json::Bool(false));
    let saved = doc.save();
    assert_ne!(saved, bytes);

    let re = open(&saved);
    let s = &re.sheets()[0];
    assert_eq!(s.cell(4, 1).unwrap().value, CellValue::Number(99.0));
    assert_eq!(s.cell(0, 0).unwrap().value, CellValue::Text("Renamed & <ok>".into()));
    assert_eq!(s.cell(0, 2).unwrap().value, CellValue::Number(-3.25));
    assert_eq!(s.cell(1, 4).unwrap().value, CellValue::Bool(false));
    assert_eq!(s.cell(1, 2).unwrap().formula.as_deref(), Some("A2*B2"));
    assert_eq!(s.dimension.as_deref(), Some("A1:E5"));
    // The in-memory model agrees with the reopened file.
    assert_eq!(doc.sheets()[0].cell(4, 1).unwrap().value, CellValue::Number(99.0));

    // Every other part is the same, down to its compressed bytes.
    let before = entries(&bytes);
    let after = entries(&saved);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(a.0, b.0, "entry order kept");
        if a.0 != "xl/worksheets/sheet1.xml" {
            assert_eq!(a.1, b.1, "{} unchanged", a.0);
            assert_eq!(a.2, b.2, "{} stored bytes unchanged", a.0);
        }
    }
    // Inside the sheet, what the edits did not touch is byte for byte the same.
    let xml = part(&saved, "xl/worksheets/sheet1.xml");
    assert!(xml.contains("<row r=\"3\" spans=\"1:3\"><c r=\"A3\"><v>20</v></c>"));
    assert!(xml.contains("<extLst><ext uri=\"{01252117-D84E-4E92-8308-4BE1C098FCBB}\"><x14ac:unknownThing val=\"1\"/></ext></extLst>"));
    assert!(xml.contains("<sheetFormatPr defaultRowHeight=\"15\" x14ac:dyDescent=\"0.25\"/>"));
    assert!(xml.contains("<c r=\"A1\" t=\"inlineStr\"><is><t>Renamed &amp; &lt;ok&gt;</t></is></c>"));
    assert!(xml.contains("<row r=\"5\"><c r=\"B5\"><v>99</v></c></row></sheetData>"));
    assert!(xml.contains("<row r=\"2\" spans=\"1:5\">"));
    assert!(xml.starts_with(DECL));
}

#[test]
fn set_cell_keeps_the_style_and_clears() {
    let bytes = main_book();
    let mut doc = open(&bytes);
    set(&mut doc, "Fmt", "A1", Json::Num(2.71828));
    assert_eq!(doc.cell_text(1, 0, 0), "2.72");
    set(&mut doc, "Fmt", "A1", Json::Null);
    let c = doc.sheets()[1].cell(0, 0).unwrap();
    assert_eq!((c.value.clone(), c.style), (CellValue::Empty, 1));
    let xml = part(&doc.save(), "xl/worksheets/sheet2.xml");
    // The emptied cell keeps its own tags (patch semantics) and its style.
    assert!(xml.contains("<row r=\"1\"><c r=\"A1\" s=\"1\"></c></row>"), "{xml}");
    // Clearing an unformatted cell removes it; clearing a cell that never existed changes nothing.
    set(&mut doc, "Data", "A2", Json::Null);
    assert!(doc.sheets()[0].cell(1, 0).is_none());
    let before = doc.save();
    set(&mut doc, "Data", "Z99", Json::Null);
    assert_eq!(doc.save(), before);
}

#[test]
fn overwriting_a_shared_formula_master_hands_it_on() {
    let mut doc = open(&main_book());
    set(&mut doc, "Data", "C3", Json::Num(5.0));
    let xml = part(&doc.save(), "xl/worksheets/sheet1.xml");
    assert!(xml.contains("<c r=\"C4\"><f t=\"shared\" si=\"0\" ref=\"C4\">A4*2</f><v>0</v></c>"), "{xml}");
    let re = open(&doc.save());
    let s = &re.sheets()[0];
    assert_eq!(s.cell(2, 2).unwrap().formula, None);
    assert_eq!(s.cell(2, 2).unwrap().value, CellValue::Number(5.0));
    assert_eq!(s.cell(3, 2).unwrap().formula.as_deref(), Some("A4*2"));
}

#[test]
fn set_cell_inverse_restores_the_old_cell() {
    let bytes = main_book();
    let original = part(&bytes, "xl/worksheets/sheet1.xml");
    let mut doc = open(&bytes);
    let ch = set(&mut doc, "Data", "C3", Json::from("text now"));
    assert_eq!(ch.pages_touched, vec![0]);
    assert_eq!(ch.undo.len(), 1);
    assert_eq!(ch.undo[0].name, "ledger.restore_rows");
    let ch2 = doc.apply(&ch.undo[0]).unwrap();
    assert_eq!(part(&doc.save(), "xl/worksheets/sheet1.xml"), original);
    assert_eq!(doc.sheets()[0].cell(2, 2).unwrap().formula.as_deref(), Some("A3*2"));
    assert_eq!(doc.sheets()[0].cell(3, 2).unwrap().formula.as_deref(), Some("A4*2"));
    // And the inverse of the inverse does it again.
    doc.apply(&ch2.undo[0]).unwrap();
    assert_eq!(doc.sheets()[0].cell(2, 2).unwrap().value, CellValue::Text("text now".into()));

    // A new row and a grown dimension go away again.
    let mut doc = open(&bytes);
    let ch = set(&mut doc, "Data", "F10", Json::Num(1.0));
    assert_eq!(doc.sheets()[0].dimension.as_deref(), Some("A1:F10"));
    doc.apply(&ch.undo[0]).unwrap();
    assert_eq!(part(&doc.save(), "xl/worksheets/sheet1.xml"), original);
    assert_eq!(doc.sheets()[0].dimension.as_deref(), Some("A1:C4"));
    assert!(doc.sheets()[0].rows.get(&9).is_none());
}

#[test]
fn set_cell_refuses_bad_args() {
    let mut doc = open(&main_book());
    let bad = |doc: &mut LedgerDoc, args: Json| doc.apply(&Command::new("ledger.set_cell", args)).unwrap_err().to_string();
    assert!(bad(&mut doc, Json::obj().with("cell", "B0").with("value", 1)).contains("not a cell reference"));
    assert!(bad(&mut doc, Json::obj().with("value", 1)).contains("\"cell\""));
    assert!(bad(&mut doc, Json::obj().with("sheet", "Nope").with("cell", "A1").with("value", 1)).contains("Data, Fmt, Secret Tab"));
    assert!(bad(&mut doc, Json::obj().with("sheet", 9).with("cell", "A1").with("value", 1)).contains("index"));
    assert!(bad(&mut doc, Json::obj().with("cell", "A1").with("value", Json::arr())).contains("\"value\""));
    assert!(doc.apply(&Command::new("ledger.explode", Json::obj())).is_err());
    assert_eq!(doc.save(), main_book());
}

#[test]
fn read_ranges_for_the_ai() {
    let doc = open(&main_book());
    let r = doc.read(&Json::obj().with("sheet", "Data").with("range", "A1:C2")).unwrap();
    let rows = r.as_arr().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].as_arr().unwrap().len(), 3);
    assert_eq!(rows[0].at(1).unwrap().get("v").unwrap().as_str(), Some("Hello World"));
    let c2 = rows[1].at(2).unwrap();
    assert_eq!(c2.get("v").unwrap().as_f64(), Some(25.0));
    assert_eq!(c2.get("f").unwrap().as_str(), Some("A2*B2"));
    assert_eq!(c2.get("t").unwrap().as_str(), Some("25"));
    assert!(rows[0].at(0).unwrap().get("f").is_none());
    // Formatted text, a single cell, a sheet by index, an empty cell.
    let one = doc.read(&Json::obj().with("sheet", 1).with("cell", "A4")).unwrap();
    assert_eq!(one.to_string(), r#"[[{"v":-1234.5,"t":"-$1,234.50"}]]"#);
    let empty = doc.read(&Json::obj().with("sheet", "Data").with("range", "Z9")).unwrap();
    assert_eq!(empty.to_string(), r#"[[{"v":null,"t":""}]]"#);
    // Default: the used range of the active sheet.
    let all = doc.read(&Json::obj()).unwrap();
    assert_eq!(all.as_arr().unwrap().len(), 4);
    // A whole column stops where the cells stop.
    let col = doc.read(&Json::obj().with("sheet", "Data").with("range", "B:B")).unwrap();
    assert_eq!(col.as_arr().unwrap().len(), 4);
    assert_eq!(col.at(2).unwrap().at(0).unwrap().get("v"), Some(&Json::Bool(true)));
    // A huge range is cut down to where the cells are.
    assert_eq!(doc.read(&Json::obj().with("range", "A1:Z9999")).unwrap().as_arr().unwrap().len(), 4);
    assert!(doc.read(&Json::obj().with("range", "what")).is_err());
}

#[test]
fn describe_for_the_ai() {
    let doc = open(&main_book());
    let d = doc.describe();
    let sheets = d.get("sheets").unwrap().as_arr().unwrap();
    assert_eq!(sheets.len(), 3);
    let s0 = &sheets[0];
    assert_eq!(s0.get("name").unwrap().as_str(), Some("Data"));
    assert_eq!(s0.get("used_range").unwrap().as_str(), Some("A1:C4"));
    assert_eq!(s0.get("frozen").unwrap().to_string(), r#"{"rows":1,"cols":1}"#);
    assert_eq!(s0.get("active_cell").unwrap().as_str(), Some("B2"));
    assert_eq!(sheets[1].get("frozen"), Some(&Json::Null));
    assert_eq!(sheets[2].get("state").unwrap().as_str(), Some("hidden"));
    assert_eq!(sheets[2].get("used_range"), Some(&Json::Null));
    let names = d.get("defined_names").unwrap().as_arr().unwrap();
    assert_eq!(names[0].to_string(), r#"{"name":"Rate","value":"Data!$B$2","scope":null,"hidden":false}"#);
    assert_eq!(names[1].get("scope").unwrap().as_str(), Some("Data"));
    assert_eq!(doc.kind(), "ledger");
    assert_eq!(doc.page_count(), 3);
    let cmds: Vec<String> = doc.commands().into_iter().map(|c| c.name).collect();
    assert_eq!(cmds, ["ledger.set_cell", "ledger.scroll_to"]);
}

#[test]
fn the_door_through_the_bus_with_undo() {
    let bytes = main_book();
    let mut bus = Bus::new(open(&bytes));
    let d = bus.door("describe", &Json::Null).unwrap();
    assert_eq!(d.get("kind").unwrap().as_str(), Some("ledger"));
    assert_eq!(d.get("pages").unwrap().as_i64(), Some(3));
    let args = Json::parse(r#"{"commands":[{"name":"ledger.set_cell","args":{"sheet":"Data","cell":"B2","value":"changed"}},
        {"name":"ledger.set_cell","args":{"sheet":"Data","cell":"H20","value":7}}],"label":"AI edit"}"#)
    .unwrap();
    let r = bus.door("do", &args).unwrap();
    assert_eq!(r.get("ok"), Some(&Json::Bool(true)));
    let read = |bus: &mut Bus<LedgerDoc>, cell: &str| bus.door("read", &Json::obj().with("sheet", "Data").with("cell", cell)).unwrap().at(0).unwrap().at(0).unwrap().get("v").unwrap().clone();
    assert_eq!(read(&mut bus, "B2"), Json::from("changed"));
    assert_eq!(read(&mut bus, "H20"), Json::Num(7.0));
    assert_eq!(bus.history()[0].origin, Origin::Ai);
    bus.undo().unwrap();
    assert_eq!(read(&mut bus, "B2"), Json::Num(2.5));
    assert_eq!(read(&mut bus, "H20"), Json::Null);
    assert_eq!(part(&bus.doc().save(), "xl/worksheets/sheet1.xml"), part(&bytes, "xl/worksheets/sheet1.xml"));
    bus.redo().unwrap();
    assert_eq!(read(&mut bus, "B2"), Json::from("changed"));
    // The internal restore command is not open to the AI.
    let sneaky = Json::parse(r#"{"commands":[{"name":"ledger.restore_rows","args":{"sheet":0,"rows":[]}}]}"#).unwrap();
    assert!(bus.door("do", &sneaky).is_err());
    let shot = bus.door("shot", &Json::obj().with("page", 1)).unwrap();
    assert_eq!(shot.get("width").unwrap().as_f64(), Some(1200.0));
    assert_eq!(shot.get("height").unwrap().as_f64(), Some(800.0));
}

#[test]
fn new_blank_workbook() {
    let mut doc = LedgerDoc::new_blank_with_fonts(FontCache::empty());
    assert_eq!(doc.page_count(), 1);
    assert_eq!(doc.sheets()[0].name, "Sheet1");
    set(&mut doc, "Sheet1", "B3", Json::from("hello"));
    set(&mut doc, "Sheet1", "A1", Json::Num(1.5));
    let re = open(&doc.save());
    assert_eq!(re.cell_text(0, 2, 1), "hello");
    assert_eq!(re.cell_text(0, 0, 0), "1.5");
    assert_eq!(re.sheets()[0].dimension.as_deref(), Some("A1:B3"));
    let blank = blank_xlsx();
    assert_eq!(open(&blank).save(), blank);
}

#[test]
fn not_a_workbook() {
    assert!(LedgerDoc::open_with_fonts(b"hello", FontCache::empty()).is_err());
    let mut z = ZipWriter::new();
    z.add("[Content_Types].xml", b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>", Method::Stored).unwrap();
    assert!(LedgerDoc::open_with_fonts(&z.finish(), FontCache::empty()).unwrap_err().contains("not a workbook"));
}

// --- the grid ---------------------------------------------------------------------------------------

fn grid_book() -> Vec<u8> {
    let grid = sheet_xml(
        "<dimension ref=\"A1:D100\"/><sheetViews><sheetView workbookViewId=\"0\">\
<pane xSplit=\"1\" ySplit=\"2\" topLeftCell=\"B3\" activePane=\"bottomRight\" state=\"frozen\"/></sheetView></sheetViews>\
<sheetFormatPr defaultRowHeight=\"15\"/><cols><col min=\"3\" max=\"3\" width=\"9.140625\" hidden=\"1\"/></cols>\
<sheetData><row r=\"1\"><c r=\"A1\" s=\"12\"><v>1</v></c></row>\
<row r=\"4\"><c r=\"B4\" s=\"13\" t=\"inlineStr\"><is><t>Merged</t></is></c><c r=\"D4\" s=\"13\"/></row>\
<row r=\"100\"><c r=\"B100\" s=\"12\"/></row></sheetData>\
<mergeCells count=\"1\"><mergeCell ref=\"B4:D5\"/></mergeCells>",
    );
    book(&[("Grid", &grid, "")], None, Some(STYLES), "", "")
}

const YELLOW: Color = Color { r: 255, g: 255, b: 0, a: 255 };
const GREEN: Color = Color { r: 0, g: 0xB0, b: 0x50, a: 255 };
const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };
const FREEZE: Color = Color { r: 0x7F, g: 0x7F, b: 0x7F, a: 255 };

#[test]
fn grid_merged_cells_fills_borders() {
    let doc = open(&grid_book());
    let dl = doc.display(0);
    assert_eq!((dl.width, dl.height), (1200.0, 800.0));
    let s = &doc.sheets()[0];
    assert_eq!(s.merges, vec![CellRange::parse("B4:D5").unwrap()]);
    // The merged area is one green rect over B..D (C hidden) and rows 4..5.
    let green = fills(&dl, GREEN);
    assert_eq!(green.len(), 1, "{green:?}");
    let header_w = row_header_width(52);
    let m = green[0];
    assert!(close(m.w, 96.0) && close(m.h, 30.0), "{m:?}");
    assert!(close(m.x, header_w + 48.0), "{m:?} {header_w}");
    assert!(close(m.y, HEADER_H + 30.0 + 15.0), "{m:?}");
    // A1's yellow fill in the frozen corner, with its blue borders.
    let yellow = fills(&dl, YELLOW);
    assert_eq!(yellow.len(), 1);
    assert!(close(yellow[0].x, header_w) && close(yellow[0].y, HEADER_H) && close(yellow[0].w, 48.0));
    assert_eq!(strokes(&dl, BLUE).len(), 4);
    // Hidden column C has no width.
    assert_eq!(s.col_width(2), 0.0);
}

#[test]
fn grid_frozen_panes_and_scrolling() {
    let mut doc = open(&grid_book());
    let dl = doc.display(0);
    let lines = strokes(&dl, FREEZE);
    assert_eq!(lines.len(), 1);
    let header_w = row_header_width(52);
    let ops = &lines[0];
    assert!(ops.contains(&PathOp::MoveTo(0.0, HEADER_H + 30.0)), "{ops:?}");
    assert!(ops.contains(&PathOp::MoveTo(header_w + 48.0, 0.0)), "{ops:?}");
    // Row 100 is out of view; scrolling brings it in, and the frozen A1 stays.
    assert!(fills(&doc.display(0), YELLOW).len() == 1);
    let ch = doc.apply(&Command::new("ledger.scroll_to", Json::obj().with("sheet", "Grid").with("cell", "B95"))).unwrap();
    assert!(ch.undo.is_empty());
    assert_eq!(ch.pages_touched, vec![0]);
    let dl = doc.display(0);
    let yellow = fills(&dl, YELLOW);
    assert_eq!(yellow.len(), 2, "{yellow:?}");
    // B100 sits 5 rows below the scrolled top (B95), under the 2 frozen rows.
    assert!(yellow.iter().any(|r| close(r.y, HEADER_H + 30.0 + 5.0 * 15.0)), "{yellow:?}");
    // Scrolling into the frozen rows stops at the split.
    doc.apply(&Command::new("ledger.scroll_to", Json::obj().with("cell", "A1").with("width", 600).with("height", 300))).unwrap();
    let s = &doc.sheets()[0];
    assert_eq!((s.view.row, s.view.col, s.view.width, s.view.height), (2, 1, 600.0, 300.0));
    assert_eq!(doc.display(0).width, 600.0);
    assert!(doc.apply(&Command::new("ledger.scroll_to", Json::obj().with("cell", "A1").with("width", -5))).is_err());
}

#[test]
fn grid_gridlines_follow_the_view() {
    let doc = open(&grid_book());
    let dl = doc.display(0);
    let grid = strokes(&dl, Color::rgb(0xD4, 0xD4, 0xD4));
    assert_eq!(grid.len(), 1);
    let moves = grid[0].iter().filter(|o| matches!(o, PathOp::MoveTo(..))).count();
    let s = &doc.sheets()[0];
    // Visible rows: 2 frozen + rows 3.. down to the bottom; visible columns likewise.
    let rows = ((800.0 - HEADER_H - 30.0) / 15.0_f64).ceil() as usize + 2;
    let header_w = row_header_width(52);
    let mut cols = 1; // A (frozen)
    let mut x = header_w + 48.0;
    let mut c = 1;
    while x < 1200.0 {
        let w = s.col_width(c);
        if w > 0.0 {
            cols += 1;
        }
        x += w;
        c += 1;
    }
    assert_eq!(moves, rows + cols);
    // Gridlines off.
    let off = sheet_xml("<sheetViews><sheetView showGridLines=\"0\" workbookViewId=\"0\"/></sheetViews><sheetData/>");
    let doc = open(&book(&[("S", &off, "")], None, None, "", ""));
    assert!(strokes(&doc.display(0), Color::rgb(0xD4, 0xD4, 0xD4)).is_empty());
}

#[test]
fn a_cell_at_xfd1048576_is_cheap() {
    let sheet = sheet_xml("<sheetData><row r=\"1\"><c r=\"A1\"><v>1</v></c></row><row r=\"1048576\"><c r=\"XFD1048576\"><v>7</v></c></row></sheetData>");
    let mut doc = open(&book(&[("Big", &sheet, "")], None, None, "", ""));
    let s = &doc.sheets()[0];
    assert_eq!(s.rows.len(), 2);
    assert_eq!(s.rows.values().map(|r| r.cells.len()).sum::<usize>(), 2);
    assert_eq!(s.used_range().unwrap().to_string(), "A1:XFD1048576");
    let started = std::time::Instant::now();
    let dl = doc.display(0);
    assert!(!dl.items.is_empty());
    doc.apply(&Command::new("ledger.scroll_to", Json::obj().with("cell", "XFD1048576"))).unwrap();
    let dl = doc.display(0);
    assert!(!dl.items.is_empty());
    assert!(started.elapsed().as_secs() < 5, "virtualised: no walk over empty cells");
    let v = doc.read(&Json::obj().with("cell", "XFD1048576")).unwrap();
    assert_eq!(v.to_string(), r#"[[{"v":7,"t":"7"}]]"#);
    // The whole used range is far too big to read at once.
    assert!(doc.read(&Json::obj()).unwrap_err().to_string().contains("read at most"));
    set(&mut doc, "Big", "XFD1048575", Json::Num(6.0));
    assert_eq!(open(&doc.save()).cell_text(0, 1_048_574, 16_383), "6");
}

/// Text needs real fonts; skipped quietly on a machine without any.
#[test]
fn grid_text_alignment_and_overflow_with_system_fonts() {
    let sheet = sheet_xml(
        "<sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>A long piece of text that overflows</t></is></c>\
<c r=\"C1\"><v>3</v></c></row><row r=\"2\"><c r=\"B2\"><v>5</v></c><c r=\"C2\" s=\"12\" t=\"inlineStr\"><is><t>mid</t></is></c></row>\
<row r=\"3\"><c r=\"A3\" s=\"2\"><v>123456789012</v></c><c r=\"B3\"><v>0.000123456789</v></c></row></sheetData>",
    );
    let doc = LedgerDoc::open(&book(&[("T", &sheet, "")], None, Some(STYLES), "", "")).unwrap();
    let dl = doc.display(0);
    // (clip box, first glyph x) of each text run.
    let mut runs: Vec<(Rect, f64, f64)> = Vec::new();
    for w in dl.items.windows(2) {
        if let (Item::PushClip(ops), Item::Glyphs { glyphs, .. }) = (&w[0], &w[1]) {
            let last = glyphs.last().map(|g| g.x).unwrap_or(0.0);
            runs.push((path_bounds(ops).unwrap(), glyphs[0].x, last));
        }
    }
    if runs.is_empty() {
        return; // no fonts on this machine
    }
    let header_w = row_header_width(52);
    let row1 = HEADER_H;
    // A1 overflows over the empty B1 and stops at C1.
    let a1 = runs.iter().find(|r| close(r.0.y, row1) && close(r.0.x, header_w)).expect("A1 text");
    assert!(close(a1.0.w, 96.0), "{:?}", a1.0);
    // B2's number is right-aligned in its cell.
    let b2 = runs.iter().find(|r| close(r.0.y, row1 + 15.0) && close(r.0.x, header_w + 48.0)).expect("B2 text");
    assert!(b2.1 > header_w + 48.0 + 24.0, "{:?}", b2);
    // C2 is centred.
    let c2 = runs.iter().find(|r| close(r.0.y, row1 + 15.0) && close(r.0.x, header_w + 96.0)).expect("C2 text");
    let left = c2.1 - (header_w + 96.0);
    assert!(left > 8.0 && left < 30.0, "{:?}", c2);
    // A3 with #,##0 is too wide for 48 pt: hashes; B3 in General loses digits instead.
    let glyph_runs = dl.items.iter().filter(|i| matches!(i, Item::Glyphs { .. })).count();
    assert!(glyph_runs >= 6);
    // Resources hold every glyph the page uses.
    let res = doc.resources().unwrap();
    for i in &dl.items {
        if let Item::Glyphs { font, glyphs, .. } = i {
            for g in glyphs {
                assert!(res.glyph(*font, g.gid).is_some(), "glyph {} of font {} kept", g.gid, font);
            }
        }
    }
}

#[test]
fn chartsheets_and_missing_parts_are_tolerated() {
    let sheet = sheet_xml("<sheetData/>");
    let mut bytes_book = book(&[("S", &sheet, "")], None, None, "", "");
    // A workbook naming a sheet whose relationship is missing still opens.
    let doc = open(&bytes_book);
    assert_eq!(doc.sheets()[0].kind, SheetKind::Worksheet);
    let wb = part(&bytes_book, "xl/workbook.xml").replace("</sheets>", "<sheet name=\"Ghost\" sheetId=\"9\" r:id=\"rId99\"/></sheets>");
    let mut z = ZipWriter::new();
    for (name, data, _) in entries(&bytes_book) {
        let data = if name == "xl/workbook.xml" { wb.clone().into_bytes() } else { data };
        z.add(&name, &data, Method::Deflated(6)).unwrap();
    }
    bytes_book = z.finish();
    let mut doc = open(&bytes_book);
    assert_eq!(doc.page_count(), 2);
    assert_eq!(doc.sheets()[1].kind, SheetKind::Other);
    assert!(!doc.display(1).items.is_empty());
    let err = doc.apply(&Command::new("ledger.set_cell", Json::obj().with("sheet", "Ghost").with("cell", "A1").with("value", 1))).unwrap_err();
    assert!(err.to_string().contains("not a worksheet"));
}

#[test]
fn ledger_doc_is_send() {
    fn send<T: Send>() {}
    send::<LedgerDoc>();
}
