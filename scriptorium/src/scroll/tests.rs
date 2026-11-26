//! Scroll M1 tests. Every document here is invented: built from XML strings with our own zip writer.

use super::numbering::format_number;
use super::*;
use crate::ground::bus::{Bus, Origin};
use crate::ground::picture::png;
use crate::ground::zip::{Method, ZipWriter};

const NS: &str = concat!(
    "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
    "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" ",
    "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" ",
    "xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" ",
    "xmlns:x14=\"urn:invented:extension\""
);

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// An invented .docx, part by part.
pub(super) struct Docx {
    body: String,
    styles: Option<String>,
    numbering: Option<String>,
    settings: Option<String>,
    title: Option<String>,
    /// (rId, file name under word/, xml, is_header)
    hf: Vec<(String, String, String, bool)>,
    /// (rId, file name under word/media/, bytes)
    media: Vec<(String, String, Vec<u8>)>,
    /// An extra part nobody understands.
    unknown: bool,
}

impl Docx {
    pub(super) fn new(body: &str) -> Docx {
        Docx { body: body.to_string(), styles: None, numbering: None, settings: None, title: None, hf: Vec::new(), media: Vec::new(), unknown: false }
    }

    pub(super) fn styles(mut self, inner: &str) -> Docx {
        self.styles = Some(inner.to_string());
        self
    }

    pub(super) fn numbering(mut self, inner: &str) -> Docx {
        self.numbering = Some(inner.to_string());
        self
    }

    pub(super) fn settings(mut self, inner: &str) -> Docx {
        self.settings = Some(inner.to_string());
        self
    }

    fn title(mut self, t: &str) -> Docx {
        self.title = Some(t.to_string());
        self
    }

    pub(super) fn header(mut self, id: &str, file: &str, inner: &str) -> Docx {
        self.hf.push((id.into(), file.into(), inner.into(), true));
        self
    }

    pub(super) fn footer(mut self, id: &str, file: &str, inner: &str) -> Docx {
        self.hf.push((id.into(), file.into(), inner.into(), false));
        self
    }

    pub(super) fn media(mut self, id: &str, file: &str, bytes: Vec<u8>) -> Docx {
        self.media.push((id.into(), file.into(), bytes));
        self
    }

    pub(super) fn unknown_part(mut self) -> Docx {
        self.unknown = true;
        self
    }

    pub(super) fn bytes(&self) -> Vec<u8> {
        let mut ct = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             <Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/>\
             <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
             <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
        );
        let mut rels = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
        if self.styles.is_some() {
            ct.push_str("<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>");
            rels.push_str(&format!("<Relationship Id=\"rIdS\" Type=\"{REL}/styles\" Target=\"styles.xml\"/>"));
        }
        if self.numbering.is_some() {
            ct.push_str("<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>");
            rels.push_str(&format!("<Relationship Id=\"rIdN\" Type=\"{REL}/numbering\" Target=\"numbering.xml\"/>"));
        }
        if self.settings.is_some() {
            ct.push_str("<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>");
            rels.push_str(&format!("<Relationship Id=\"rIdT\" Type=\"{REL}/settings\" Target=\"settings.xml\"/>"));
        }
        for (id, file, _, is_header) in &self.hf {
            let kind = if *is_header { "header" } else { "footer" };
            ct.push_str(&format!(
                "<Override PartName=\"/word/{file}\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml\"/>"
            ));
            rels.push_str(&format!("<Relationship Id=\"{id}\" Type=\"{REL}/{kind}\" Target=\"{file}\"/>"));
        }
        for (id, file, _) in &self.media {
            rels.push_str(&format!("<Relationship Id=\"{id}\" Type=\"{REL}/image\" Target=\"media/{file}\"/>"));
        }
        rels.push_str(&format!("<Relationship Id=\"rIdH\" Type=\"{REL}/hyperlink\" Target=\"https://example.invalid/\" TargetMode=\"External\"/>"));
        rels.push_str("</Relationships>");
        if self.unknown {
            ct.push_str("<Override PartName=\"/word/invented.xml\" ContentType=\"application/x-invented+xml\"/>");
        }
        ct.push_str("</Types>");

        let mut z = ZipWriter::new();
        let add = |z: &mut ZipWriter, name: &str, data: &[u8]| z.add(name, data, Method::Deflated(6)).unwrap();
        add(&mut z, "[Content_Types].xml", ct.as_bytes());
        add(
            &mut z,
            "_rels/.rels",
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
                 <Relationship Id=\"rId1\" Type=\"{REL}/officeDocument\" Target=\"word/document.xml\"/>\
                 <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
                 </Relationships>"
            )
            .as_bytes(),
        );
        let doc = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:document {NS}><w:body>{}</w:body></w:document>", self.body);
        add(&mut z, "word/document.xml", doc.as_bytes());
        add(&mut z, "word/_rels/document.xml.rels", rels.as_bytes());
        if let Some(s) = &self.styles {
            add(&mut z, "word/styles.xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<w:styles {NS}>{s}</w:styles>").as_bytes());
        }
        if let Some(n) = &self.numbering {
            add(&mut z, "word/numbering.xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<w:numbering {NS}>{n}</w:numbering>").as_bytes());
        }
        if let Some(s) = &self.settings {
            add(&mut z, "word/settings.xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<w:settings {NS}>{s}</w:settings>").as_bytes());
        }
        for (_, file, inner, is_header) in &self.hf {
            let root = if *is_header { "hdr" } else { "ftr" };
            add(&mut z, &format!("word/{file}"), format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<w:{root} {NS}>{inner}</w:{root}>").as_bytes());
        }
        for (_, file, bytes) in &self.media {
            z.add(&format!("word/media/{file}"), bytes, Method::Stored).unwrap();
        }
        let title = self.title.as_ref().map(|t| format!("<dc:title>{t}</dc:title>")).unwrap_or_default();
        add(
            &mut z,
            "docProps/core.xml",
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<cp:coreProperties \
                 xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
                 xmlns:dc=\"http://purl.org/dc/elements/1.1/\">{title}<dc:creator>Invented Author</dc:creator></cp:coreProperties>"
            )
            .as_bytes(),
        );
        if self.unknown {
            add(&mut z, "word/invented.xml", b"<invented><keep me=\"exactly\"/></invented>");
        }
        z.finish()
    }

    fn open(&self) -> ScrollDoc {
        ScrollDoc::open(&self.bytes()).expect("the invented document opens")
    }
}

pub(super) fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>")
}

pub(super) fn styled_p(style: &str, text: &str) -> String {
    format!("<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
}

pub(super) const SECT: &str = "<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>";

fn page_text(doc: &ScrollDoc, page: usize) -> String {
    doc.read(&Json::obj().with("page", page)).unwrap().get("text").and_then(Json::as_str).unwrap().to_string()
}

fn has_glyphs(dl: &DisplayList) -> bool {
    dl.items.iter().any(|i| matches!(i, Item::Glyphs { .. }))
}

// ---------------------------------------------------------------------------------------------------
// Round trips
// ---------------------------------------------------------------------------------------------------

#[test]
fn open_then_save_unchanged_is_byte_identical() {
    let body = format!("{}{}<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Bold</w:t></w:r><x14:unknown keep=\"1\"/></w:p>{SECT}", p("Hello"), p("World"));
    let bytes = Docx::new(&body).styles("<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>").unknown_part().bytes();
    let doc = ScrollDoc::open(&bytes).unwrap();
    assert_eq!(doc.save(), bytes);
    assert_eq!(doc.paragraph_count(), 3);
    assert_eq!(doc.paragraph_text(2), Some("Bold"));
}

#[test]
fn edit_a_paragraph_then_reopen_shows_it_and_other_parts_are_untouched() {
    let body = format!(
        "{}<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:rPr><w:b/><w:color w:val=\"FF0000\"/></w:rPr><w:t>Old </w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>words</w:t></w:r></w:p>{}<w:p><x14:unknown keep=\"1\"><x14:child/></x14:unknown><w:r><w:t>Keep</w:t></w:r></w:p>{SECT}",
        p("First"),
        p("Third")
    );
    let bytes = Docx::new(&body).styles("<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>").unknown_part().title("Invented").bytes();
    let before = Package::open(&bytes).unwrap();
    let mut doc = ScrollDoc::open(&bytes).unwrap();
    let change = doc.apply(&Command::new("scroll.set_paragraph_text", Json::obj().with("index", 1).with("text", "New text"))).unwrap();
    assert_eq!(change.undo.len(), 1);
    assert_eq!(change.undo[0].name, "scroll.restore_paragraph");
    let saved = doc.save();
    let again = ScrollDoc::open(&saved).unwrap();
    assert_eq!(again.paragraph_text(1), Some("New text"));
    assert_eq!(again.paragraph_text(0), Some("First"));
    assert_eq!(again.paragraph_text(2), Some("Third"));
    assert_eq!(again.paragraph_text(3), Some("Keep"));
    let after = Package::open(&saved).unwrap();
    for name in before.part_names() {
        if opc::same_part(&name, "/word/document.xml") {
            continue;
        }
        assert_eq!(before.part_bytes(&name).unwrap(), after.part_bytes(&name).unwrap(), "{name} changed");
        if let Some(raw) = before.raw_part(&name) {
            assert_eq!(Some(raw), after.raw_part(&name), "{name}'s stored bytes changed");
        }
    }
    let xml = String::from_utf8(after.part_bytes("/word/document.xml").unwrap()).unwrap();
    // The unknown element and the untouched paragraphs are byte for byte the same.
    assert!(xml.contains("<x14:unknown keep=\"1\"><x14:child/></x14:unknown>"));
    assert!(xml.contains(&p("First")));
    // The paragraph keeps its properties and the first run's formatting.
    assert!(xml.contains("<w:jc w:val=\"center\"/>"));
    assert!(xml.contains("<w:rPr><w:b/><w:color w:val=\"FF0000\"/></w:rPr><w:t>New text</w:t>"));
    assert!(!xml.contains("words"));
}

#[test]
fn tabs_and_breaks_in_new_text_become_elements() {
    let mut doc = Docx::new(&format!("{}{SECT}", p("x"))).open();
    doc.apply(&Command::new("scroll.set_paragraph_text", Json::obj().with("index", 0).with("text", " a\tb\nc "))).unwrap();
    assert_eq!(doc.paragraph_text(0), Some(" a\tb\nc "));
    let xml = String::from_utf8(doc.package().part_bytes("/word/document.xml").unwrap()).unwrap();
    assert!(xml.contains("<w:t xml:space=\"preserve\"> a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t xml:space=\"preserve\">c </w:t>"), "{xml}");
}

#[test]
fn new_blank_opens_saves_and_reopens() {
    let doc = ScrollDoc::new_blank();
    assert_eq!(doc.page_count(), 1);
    assert_eq!(doc.paragraph_count(), 1);
    let dl = doc.display(0);
    assert_eq!((dl.width, dl.height), (612.0, 792.0));
    let again = ScrollDoc::open(&doc.save()).unwrap();
    assert_eq!(again.paragraph_count(), 1);
    assert_eq!(again.describe().get("properties").and_then(|p| p.get("creator")).and_then(Json::as_str), Some("Scriptorium"));
}

#[test]
fn not_a_document_is_an_error() {
    assert!(ScrollDoc::open(b"plain text").is_err());
    let mut z = ZipWriter::new();
    z.add("[Content_Types].xml", b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>", Method::Stored).unwrap();
    assert!(ScrollDoc::open(&z.finish()).is_err());
}

// ---------------------------------------------------------------------------------------------------
// Styles and numbering
// ---------------------------------------------------------------------------------------------------

const INHERIT_STYLES: &str = concat!(
    "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\"/><w:sz w:val=\"20\"/></w:rPr></w:rPrDefault>",
    "<w:pPrDefault><w:pPr><w:spacing w:after=\"120\"/></w:pPr></w:pPrDefault></w:docDefaults>",
    "<w:latentStyles w:count=\"1\"><w:lsdException w:name=\"heading 1\"/></w:latentStyles>",
    "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Base\"><w:name w:val=\"Base\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:jc w:val=\"center\"/><w:ind w:left=\"720\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"FF0000\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Derived\"><w:name w:val=\"Derived\"/><w:basedOn w:val=\"Base\"/>",
    "<w:pPr><w:ind w:left=\"1440\" w:hanging=\"360\"/></w:pPr><w:rPr><w:i/><w:sz w:val=\"32\"/><w:color w:val=\"accent1\" w:themeColor=\"accent1\"/></w:rPr></w:style>",
    "<w:style w:type=\"character\" w:styleId=\"Strong\"><w:name w:val=\"Strong\"/><w:rPr><w:b/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"LoopA\"><w:name w:val=\"LoopA\"/><w:basedOn w:val=\"LoopB\"/></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"LoopB\"><w:name w:val=\"LoopB\"/><w:basedOn w:val=\"LoopA\"/></w:style>",
);

#[test]
fn style_inheritance_resolves_through_based_on() {
    let body = format!("{}<w:p><w:pPr><w:pStyle w:val=\"Derived\"/></w:pPr><w:r><w:rPr><w:rStyle w:val=\"Strong\"/></w:rPr><w:t>Toggled</w:t></w:r></w:p>{SECT}", styled_p("Derived", "Big"));
    let doc = Docx::new(&body).styles(INHERIT_STYLES).open();
    let st = &doc.model().styles;
    let (pp, rp) = st.folded("Derived");
    assert_eq!(rp.bold, Some(true), "bold from Base");
    assert_eq!(rp.italic, Some(true));
    assert_eq!(rp.size, Some(16.0));
    // Theme colour accent1 of the default Office theme.
    assert_eq!(rp.color, Some(Some(Color::rgb(0x44, 0x72, 0xC4))));
    assert_eq!(pp.align, Some(crate::ground::text::Align::Center));
    assert_eq!(pp.ind_left, Some(72.0));
    assert_eq!(pp.ind_first, Some(-18.0));
    assert_eq!(st.doc_rpr.font.as_deref(), Some("Arial"));
    assert_eq!(st.doc_ppr.after, Some(6.0));
    // A cycle does not hang.
    assert!(st.chain("LoopA").len() <= 2);
    // The character style's bold toggles the paragraph style's bold off.
    let mut base = rp.clone();
    base.over_toggling(&st.char_rpr("Strong"));
    assert_eq!(base.bold, Some(false));
    // On the page: the glyphs are drawn at 16 pt in accent1.
    let dl = doc.display(0);
    if has_glyphs(&dl) {
        assert!(dl.items.iter().any(|i| matches!(i, Item::Glyphs { size, color, .. } if *size == 16.0 && *color == Color::rgb(0x44, 0x72, 0xC4))));
    }
    assert!(page_text(&doc, 0).contains("Big"));
}

const NUMBERING: &str = concat!(
    "<w:abstractNum w:abstractNumId=\"0\">",
    "<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/><w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr></w:lvl>",
    "<w:lvl w:ilvl=\"1\"><w:start w:val=\"1\"/><w:numFmt w:val=\"lowerLetter\"/><w:lvlText w:val=\"%2)\"/><w:pPr><w:ind w:left=\"1440\" w:hanging=\"360\"/></w:pPr></w:lvl>",
    "<w:lvl w:ilvl=\"2\"><w:start w:val=\"1\"/><w:numFmt w:val=\"lowerRoman\"/><w:lvlText w:val=\"%1.%2.%3\"/></w:lvl>",
    "</w:abstractNum>",
    "<w:abstractNum w:abstractNumId=\"1\">",
    "<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"\u{F0B7}\"/><w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr>",
    "<w:rPr><w:rFonts w:ascii=\"Symbol\" w:hAnsi=\"Symbol\" w:hint=\"default\"/></w:rPr></w:lvl>",
    "</w:abstractNum>",
    "<w:abstractNum w:abstractNumId=\"2\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"upperRoman\"/><w:lvlText w:val=\"%1.\"/></w:lvl></w:abstractNum>",
    "<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>",
    "<w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num>",
    "<w:num w:numId=\"3\"><w:abstractNumId w:val=\"0\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"7\"/></w:lvlOverride></w:num>",
    "<w:num w:numId=\"4\"><w:abstractNumId w:val=\"2\"/></w:num>",
);

pub(super) fn list_p(num: u32, lvl: u32, text: &str) -> String {
    format!("<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{lvl}\"/><w:numId w:val=\"{num}\"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
}

#[test]
fn numbering_counters_give_labels() {
    let body = [
        list_p(1, 0, "One"),
        list_p(1, 0, "Two"),
        list_p(1, 1, "Sub a"),
        list_p(1, 1, "Sub b"),
        list_p(1, 2, "Deep"),
        list_p(1, 0, "Three"),
        list_p(1, 1, "Sub again"),
        list_p(2, 0, "Bullet"),
        list_p(3, 0, "Restarted"),
        list_p(4, 0, "Roman"),
        list_p(4, 0, "Roman two"),
        list_p(0, 0, "Not a list"),
        SECT.to_string(),
    ]
    .concat();
    let doc = Docx::new(&body).numbering(NUMBERING).open();
    let text = page_text(&doc, 0);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        vec![
            "1.\tOne",
            "2.\tTwo",
            "a)\tSub a",
            "b)\tSub b",
            "2.b.i\tDeep",
            "3.\tThree",
            "a)\tSub again",
            "\u{2022}\tBullet",
            "7.\tRestarted",
            "I.\tRoman",
            "II.\tRoman two",
            "Not a list"
        ]
    );
    // The paragraph's own text has no label.
    assert_eq!(doc.paragraph_text(0), Some("One"));
}

#[test]
fn number_formats() {
    assert_eq!(format_number(4, "lowerRoman"), "iv");
    assert_eq!(format_number(1994, "upperRoman"), "MCMXCIV");
    assert_eq!(format_number(27, "lowerLetter"), "aa");
    assert_eq!(format_number(3, "upperLetter"), "C");
    assert_eq!(format_number(5, "decimalZero"), "05");
    assert_eq!(format_number(22, "ordinal"), "22nd");
    assert_eq!(format_number(12, "ordinal"), "12th");
    assert_eq!(format_number(9, "decimal"), "9");
}

// ---------------------------------------------------------------------------------------------------
// The page engine
// ---------------------------------------------------------------------------------------------------

#[test]
fn a_long_document_paginates() {
    let para = "The quick brown fox jumps over the lazy dog while the scribes copy their pages by candlelight. ".repeat(4);
    let body: String = (0..80).map(|i| p(&format!("{i}: {para}"))).collect::<String>() + SECT;
    let doc = Docx::new(&body).open();
    assert!(doc.page_count() >= 2, "only {} page(s)", doc.page_count());
    assert!(page_text(&doc, 1).len() > 10);
    // Paragraphs go forward page by page.
    let pages = &doc.laid().para_page;
    assert!(pages.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(*pages.last().unwrap(), doc.page_count() - 1);
    // Every page is the sectPr's size.
    for i in 0..doc.page_count() {
        let dl = doc.display(i);
        assert_eq!((dl.width, dl.height), (612.0, 792.0));
    }
    // Nothing is laid below the bottom margin.
    for page in &doc.laid().pages {
        for it in &page.body_items {
            if let Some(b) = it.bounds() {
                if matches!(it, Item::Glyphs { .. }) {
                    assert!(b.y < 792.0 - 72.0 + 1.0, "glyph at {}", b.y);
                }
            }
        }
    }
}

#[test]
fn explicit_page_breaks_and_page_break_before() {
    let body = format!(
        "{}<w:p><w:r><w:t>Before</w:t><w:br w:type=\"page\"/><w:t>After</w:t></w:r></w:p><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:r><w:t>Third page</w:t></w:r></w:p>{SECT}",
        p("Start")
    );
    let doc = Docx::new(&body).open();
    assert_eq!(doc.page_count(), 3);
    assert_eq!(page_text(&doc, 0), "Start\nBefore");
    assert_eq!(page_text(&doc, 1), "After");
    assert_eq!(page_text(&doc, 2), "Third page");
}

#[test]
fn section_breaks_change_the_page() {
    let landscape = "<w:sectPr><w:pgSz w:w=\"15840\" w:h=\"12240\" w:orient=\"landscape\"/><w:pgMar w:top=\"720\" w:right=\"720\" w:bottom=\"720\" w:left=\"720\" w:header=\"360\" w:footer=\"360\" w:gutter=\"0\"/></w:sectPr>";
    let portrait = "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr>";
    let body = format!("<w:p><w:pPr>{portrait}</w:pPr><w:r><w:t>A4 page</w:t></w:r></w:p>{}{landscape}", p("Wide page"));
    let doc = Docx::new(&body).open();
    assert_eq!(doc.page_count(), 2);
    let a4 = doc.display(0);
    assert!((a4.width - 595.3).abs() < 0.1 && (a4.height - 841.9).abs() < 0.1);
    let wide = doc.display(1);
    assert_eq!((wide.width, wide.height), (792.0, 612.0));
    assert_eq!(page_text(&doc, 1), "Wide page");
    assert_eq!(doc.describe().get("sections").and_then(Json::as_usize), Some(2));
}

const TABLE: &str = concat!(
    "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr>",
    "<w:tblGrid><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/></w:tblGrid>",
    "<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:p><w:r><w:t>Cell A1</w:t></w:r></w:p></w:tc>",
    "<w:tc><w:tcPr><w:gridSpan w:val=\"2\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"FFFF00\"/></w:tcPr><w:p><w:r><w:t>Cell B1 spans</w:t></w:r></w:p></w:tc></w:tr>",
    "<w:tr><w:tc><w:tcPr><w:vMerge w:val=\"restart\"/></w:tcPr><w:p><w:r><w:t>Merged</w:t></w:r></w:p></w:tc>",
    "<w:tc><w:p><w:r><w:t>Cell B2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Cell C2</w:t></w:r></w:p><w:p><w:r><w:t>second line</w:t></w:r></w:p></w:tc></w:tr>",
    "<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>Cell B3</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Cell C3</w:t></w:r></w:p></w:tc></w:tr>",
    "</w:tbl>"
);

const TABLE_STYLES: &str = concat!(
    "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>",
    "<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:tblPr><w:tblBorders>",
    "<w:top w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/><w:left w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>",
    "<w:bottom w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/><w:right w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>",
    "<w:insideH w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>",
    "</w:tblBorders></w:tblPr></w:style>"
);

#[test]
fn a_tables_cell_text_is_on_the_page() {
    let body = format!("{}{TABLE}{}{SECT}", p("Above"), p("Below"));
    let doc = Docx::new(&body).styles(TABLE_STYLES).open();
    let text = page_text(&doc, 0);
    for t in ["Cell A1", "Cell B1 spans", "Merged", "Cell B2", "Cell C2", "second line", "Cell C3", "Below"] {
        assert!(text.contains(t), "{t} missing from {text:?}");
    }
    // Cell paragraphs count among the paragraphs, in reading order.
    assert_eq!(doc.paragraph_text(1), Some("Cell A1"));
    assert_eq!(doc.describe().get("tables").and_then(Json::as_usize), Some(1));
    let dl = doc.display(0);
    // Borders are stroked lines; the shaded cell is a yellow fill.
    let strokes = dl.items.iter().filter(|i| matches!(i, Item::Path { stroke: Some(_), .. })).count();
    assert!(strokes >= 12, "{strokes} border lines");
    assert!(dl.items.iter().any(|i| matches!(i, Item::Path { fill: Some(Paint::Solid(c)), .. } if *c == Color::rgb(255, 255, 0))));
    // The spanning cell is two grid columns wide (6000 twips = 300 pt).
    let yellow = dl
        .items
        .iter()
        .find_map(|i| match i {
            Item::Path { fill: Some(Paint::Solid(c)), .. } if *c == Color::rgb(255, 255, 0) => i.bounds(),
            _ => None,
        })
        .unwrap();
    assert!((yellow.w - 300.0).abs() < 0.5, "{}", yellow.w);
    assert!((yellow.x - (72.0 + 150.0)).abs() < 0.5);
}

#[test]
fn a_long_table_repeats_its_header_row() {
    let mut rows = String::from("<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:p><w:r><w:t>HEADER</w:t></w:r></w:p></w:tc></w:tr>");
    for i in 0..80 {
        rows.push_str(&format!("<w:tr><w:trPr><w:trHeight w:val=\"400\"/></w:trPr><w:tc><w:p><w:r><w:t>Row {i}</w:t></w:r></w:p></w:tc></w:tr>"));
    }
    let body = format!("<w:tbl><w:tblGrid><w:gridCol w:w=\"5000\"/></w:tblGrid>{rows}</w:tbl>{SECT}");
    let doc = Docx::new(&body).open();
    assert!(doc.page_count() >= 2);
    assert!(page_text(&doc, 1).starts_with("HEADER"));
    assert!(page_text(&doc, 0).contains("Row 0"));
}

pub(super) fn png_bytes() -> Vec<u8> {
    png::encode_rgba8(2, 2, &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255])
}

#[test]
fn an_inline_png_becomes_an_image_item() {
    let drawing = concat!(
        "<w:p><w:r><w:t xml:space=\"preserve\">Picture: </w:t></w:r><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">",
        "<wp:extent cx=\"914400\" cy=\"457200\"/><wp:docPr id=\"1\" name=\"Picture 1\"/>",
        "<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic>",
        "<pic:nvPicPr><pic:cNvPr id=\"0\" name=\"dot.png\"/><pic:cNvPicPr/></pic:nvPicPr>",
        "<pic:blipFill><a:blip r:embed=\"rIdImg\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>",
        "<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"914400\" cy=\"457200\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>",
        "</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r><w:r><w:t xml:space=\"preserve\"> after</w:t></w:r></w:p>"
    );
    let body = format!("{drawing}{}{SECT}", p("Next paragraph"));
    let doc = Docx::new(&body).media("rIdImg", "dot.png", png_bytes()).open();
    let dl = doc.display(0);
    let (id, rect) = dl
        .items
        .iter()
        .find_map(|i| match i {
            Item::Image { image, rect } => Some((*image, *rect)),
            _ => None,
        })
        .expect("an image item");
    assert!((rect.w - 72.0).abs() < 1e-6 && (rect.h - 36.0).abs() < 1e-6, "{rect:?}");
    assert!(rect.x > 72.0, "after the text");
    let img = doc.resources().unwrap().image(id).expect("the image is in the resources");
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
    assert_eq!(doc.describe().get("pictures").and_then(Json::as_usize), Some(1));
    // The line grew to hold the picture: the next paragraph starts below its bottom.
    assert_eq!(page_text(&doc, 0), "Picture:  after\nNext paragraph");
    assert_eq!(doc.paragraph_text(0), Some("Picture:  after"));
}

const FOOTER: &str = concat!(
    "<w:p><w:r><w:t xml:space=\"preserve\">Page </w:t></w:r><w:fldSimple w:instr=\" PAGE \"><w:r><w:t>9</w:t></w:r></w:fldSimple>",
    "<w:r><w:t xml:space=\"preserve\"> of </w:t></w:r><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> NUMPAGES </w:instrText></w:r>",
    "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>99</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>"
);

#[test]
fn headers_and_footers_with_page_fields_on_every_page() {
    let sect = "<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdHd\"/><w:footerReference w:type=\"default\" r:id=\"rIdFt\"/>\
                <w:headerReference w:type=\"first\" r:id=\"rIdHf\"/><w:titlePg/>\
                <w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>";
    let body = format!(
        "{}<w:p><w:r><w:br w:type=\"page\"/></w:r><w:r><w:t>Two</w:t></w:r></w:p><w:p><w:r><w:br w:type=\"page\"/><w:t>Three, page </w:t></w:r><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>{sect}",
        p("One")
    );
    let doc = Docx::new(&body)
        .header("rIdHd", "header1.xml", &p("Running header"))
        .header("rIdHf", "header2.xml", &p("First page header"))
        .footer("rIdFt", "footer1.xml", FOOTER)
        .open();
    assert_eq!(doc.page_count(), 3);
    let read = |n: usize| doc.read(&Json::obj().with("page", n)).unwrap();
    assert_eq!(read(0).get("header").and_then(Json::as_str), Some("First page header"));
    assert_eq!(read(1).get("header").and_then(Json::as_str), Some("Running header"));
    assert_eq!(read(2).get("header").and_then(Json::as_str), Some("Running header"));
    // titlePg with no first-page footer: Word leaves the first page's footer blank.
    assert_eq!(read(0).get("footer").and_then(Json::as_str), Some(""));
    for n in 1..3 {
        assert_eq!(read(n).get("footer").and_then(Json::as_str), Some(format!("Page {} of 3", n + 1).as_str()));
    }
    // A PAGE field in the body shows the page it lands on, not its cached result.
    assert_eq!(page_text(&doc, 2), "Three, page 3");
    // Header and footer items are drawn near the page's top and bottom.
    let laid = &doc.laid().pages[1];
    if laid.header_items.iter().any(|i| matches!(i, Item::Glyphs { .. })) {
        let hb = laid.header_items.iter().find_map(|i| i.bounds()).unwrap();
        assert!(hb.y < 72.0);
        let fb = laid.footer_items.iter().find_map(|i| i.bounds()).unwrap();
        assert!(fb.y > 792.0 - 72.0);
    }
}

#[test]
fn even_page_headers_follow_settings() {
    let sect = "<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdHd\"/><w:headerReference w:type=\"even\" r:id=\"rIdHe\"/>\
                <w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>";
    let body = format!("{}<w:p><w:r><w:br w:type=\"page\"/><w:t>Two</w:t></w:r></w:p>{sect}", p("One"));
    let doc = Docx::new(&body)
        .settings("<w:evenAndOddHeaders/><w:defaultTabStop w:val=\"720\"/>")
        .header("rIdHd", "header1.xml", &p("Odd"))
        .header("rIdHe", "header2.xml", &p("Even"))
        .open();
    let h = |n: usize| doc.read(&Json::obj().with("page", n)).unwrap().get("header").and_then(Json::as_str).unwrap().to_string();
    assert_eq!(h(0), "Odd");
    assert_eq!(h(1), "Even");
}

#[test]
fn hyperlinks_hidden_text_symbols_and_caps() {
    let body = concat!(
        "<w:p><w:hyperlink r:id=\"rIdH\"><w:r><w:t>link</w:t></w:r></w:hyperlink>",
        "<w:r><w:rPr><w:vanish/></w:rPr><w:t>HIDDEN</w:t></w:r>",
        "<w:r><w:rPr><w:caps/></w:rPr><w:t xml:space=\"preserve\"> shout</w:t></w:r>",
        "<w:r><w:tab/><w:sym w:font=\"Wingdings\" w:char=\"F0E0\"/></w:r>",
        "<w:del w:id=\"1\" w:author=\"x\"><w:r><w:delText>gone</w:delText></w:r></w:del>",
        "<w:ins w:id=\"2\" w:author=\"x\"><w:r><w:t>new</w:t></w:r></w:ins></w:p>"
    );
    let doc = Docx::new(&format!("{body}{SECT}")).open();
    let text = page_text(&doc, 0);
    assert_eq!(text, "link SHOUT\t\u{F0E0}new");
    let dl = doc.display(0);
    assert!(dl.items.iter().any(|i| matches!(i, Item::Link { target, .. } if target == "https://example.invalid/")));
}

// ---------------------------------------------------------------------------------------------------
// The door
// ---------------------------------------------------------------------------------------------------

const HEADING_STYLES: &str = concat!(
    "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:rPr><w:sz w:val=\"56\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:outlineLvl w:val=\"0\"/></w:pPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:outlineLvl w:val=\"1\"/></w:pPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Kop3\"><w:name w:val=\"Eigen kop\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:outlineLvl w:val=\"2\"/></w:pPr></w:style>",
);

#[test]
fn describe_lists_headings_counts_and_properties() {
    let body = [
        styled_p("Title", "The Invented Report"),
        styled_p("Heading1", "Introduction"),
        p("Body text."),
        styled_p("Heading2", "Background"),
        p("More text."),
        styled_p("Kop3", "Localised heading"),
        TABLE.to_string(),
        SECT.to_string(),
    ]
    .concat();
    let doc = Docx::new(&body).styles(HEADING_STYLES).title("Invented Title").open();
    let d = doc.describe();
    assert_eq!(d.get("pages").and_then(Json::as_usize), Some(1));
    assert_eq!(d.get("tables").and_then(Json::as_usize), Some(1));
    assert_eq!(d.get("pictures").and_then(Json::as_usize), Some(0));
    assert_eq!(d.get("paragraphs").and_then(Json::as_usize), Some(6 + 9));
    let props = d.get("properties").unwrap();
    assert_eq!(props.get("title").and_then(Json::as_str), Some("Invented Title"));
    assert_eq!(props.get("creator").and_then(Json::as_str), Some("Invented Author"));
    let heads = d.get("headings").and_then(Json::as_arr).unwrap();
    let got: Vec<(usize, usize, &str)> = heads
        .iter()
        .map(|h| (h.get("index").and_then(Json::as_usize).unwrap(), h.get("level").and_then(Json::as_usize).unwrap(), h.get("text").and_then(Json::as_str).unwrap()))
        .collect();
    assert_eq!(got, vec![(0, 0, "The Invented Report"), (1, 1, "Introduction"), (3, 2, "Background"), (5, 3, "Localised heading")]);
}

#[test]
fn read_paragraph_range_and_page() {
    let body = [styled_p("Heading1", "Intro"), p("Alpha"), p("Beta"), p("Gamma"), SECT.to_string()].concat();
    let doc = Docx::new(&body).styles(HEADING_STYLES).open();
    let one = doc.read(&Json::obj().with("paragraph", 0)).unwrap();
    assert_eq!(one.get("text").and_then(Json::as_str), Some("Intro"));
    assert_eq!(one.get("style").and_then(Json::as_str), Some("Heading1"));
    assert_eq!(one.get("style_name").and_then(Json::as_str), Some("heading 1"));
    assert_eq!(one.get("heading_level").and_then(Json::as_usize), Some(1));
    let normal = doc.read(&Json::obj().with("paragraph", 1)).unwrap();
    assert_eq!(normal.get("style").and_then(Json::as_str), Some("Normal"));
    let range = doc.read(&Json::obj().with("range", Json::arr().item(1).item(9))).unwrap();
    let texts: Vec<&str> = range.get("paragraphs").and_then(Json::as_arr).unwrap().iter().map(|p| p.get("text").and_then(Json::as_str).unwrap()).collect();
    assert_eq!(texts, vec!["Alpha", "Beta", "Gamma"]);
    assert_eq!(page_text(&doc, 0), "Intro\nAlpha\nBeta\nGamma");
    assert!(doc.read(&Json::obj().with("paragraph", 99)).is_err());
    assert!(doc.read(&Json::obj().with("page", 5)).is_err());
    assert!(doc.read(&Json::obj().with("nonsense", 1)).is_err());
}

#[test]
fn the_door_does_set_paragraph_text_and_undo_restores_the_xml_exactly() {
    let body = format!(
        "{}<w:p w14:paraId=\"1A2B\" xmlns:w14=\"urn:invented:w14\"><w:pPr><w:spacing w:after=\"0\"/></w:pPr><w:bookmarkStart w:id=\"0\" w:name=\"mark\"/><w:r><w:rPr><w:b/></w:rPr><w:t>Second</w:t></w:r><w:r><w:t xml:space=\"preserve\"> paragraph</w:t></w:r><w:bookmarkEnd w:id=\"0\"/><x14:odd/></w:p>{}{SECT}",
        p("First"),
        p("Third")
    );
    let bytes = Docx::new(&body).title("Before").bytes();
    let doc = ScrollDoc::open(&bytes).unwrap();
    let original_xml = doc.package().part_bytes("/word/document.xml").unwrap();
    let mut bus = Bus::new(doc);
    let described = bus.door("describe", &Json::obj()).unwrap();
    assert_eq!(described.get("kind").and_then(Json::as_str), Some("scroll"));
    let cmds = described.get("commands").and_then(Json::as_arr).unwrap();
    assert!(cmds.iter().any(|c| c.get("name").and_then(Json::as_str) == Some("scroll.set_paragraph_text")));
    assert!(!cmds.iter().any(|c| c.get("name").and_then(Json::as_str) == Some("scroll.restore_paragraph")));

    let args = Json::parse(r#"{"commands":[{"name":"scroll.set_paragraph_text","args":{"index":1,"text":"Rewritten by the AI"}},{"name":"scroll.set_title","args":{"title":"After"}}],"label":"AI edit"}"#).unwrap();
    let done = bus.door("do", &args).unwrap();
    assert_eq!(done.get("ok").and_then(Json::as_bool), Some(true));
    let read = bus.door("read", &Json::obj().with("paragraph", 1)).unwrap();
    assert_eq!(read.get("text").and_then(Json::as_str), Some("Rewritten by the AI"));
    assert_eq!(bus.doc().describe().get("properties").and_then(|p| p.get("title")).and_then(Json::as_str), Some("After"));
    let edited = String::from_utf8(bus.doc().package().part_bytes("/word/document.xml").unwrap()).unwrap();
    assert!(edited.contains("<w:bookmarkStart w:id=\"0\" w:name=\"mark\"/>") && edited.contains("<w:bookmarkEnd w:id=\"0\"/>"));
    assert!(edited.contains("<w:r><w:rPr><w:b/></w:rPr><w:t>Rewritten by the AI</w:t></w:r>"));

    let shot = bus.door("shot", &Json::obj().with("page", 0)).unwrap();
    assert_eq!(shot.get("width").and_then(Json::as_f64), Some(612.0));

    bus.undo().unwrap();
    assert_eq!(bus.doc().paragraph_text(1), Some("Second paragraph"));
    assert_eq!(bus.doc().package().part_bytes("/word/document.xml").unwrap(), original_xml, "undo puts the XML back byte for byte");
    assert_eq!(bus.doc().describe().get("properties").and_then(|p| p.get("title")).and_then(Json::as_str), Some("Before"));

    bus.redo().unwrap();
    assert_eq!(bus.doc().paragraph_text(1), Some("Rewritten by the AI"));
    bus.undo().unwrap();
    assert_eq!(bus.doc().package().part_bytes("/word/document.xml").unwrap(), original_xml);
}

#[test]
fn bad_commands_are_refused() {
    let mut bus = Bus::new(Docx::new(&format!("{}{SECT}", p("Only"))).open());
    assert!(bus.do_one(Command::new("scroll.set_paragraph_text", Json::obj().with("index", 5).with("text", "x")), Origin::User).is_err());
    assert!(bus.do_one(Command::new("scroll.set_paragraph_text", Json::obj().with("index", 0)), Origin::User).is_err());
    assert!(bus.do_one(Command::new("scroll.restore_paragraph", Json::obj().with("index", 0).with("xml", "<w:p/>")), Origin::Ai).is_err(), "the inverse is not a public command");
    assert!(bus.do_one(Command::new("scroll.set_title", Json::obj().with("title", 3)), Origin::User).is_err());
    assert!(!bus.can_undo());
    assert_eq!(bus.doc().paragraph_text(0), Some("Only"));
}

#[test]
fn set_title_round_trips_through_save() {
    let mut doc = Docx::new(&format!("{}{SECT}", p("x"))).open();
    let ch = doc.apply(&Command::new("scroll.set_title", Json::obj().with("title", "A New Title"))).unwrap();
    assert_eq!(ch.undo[0].args.get("title"), Some(&Json::Null));
    let again = ScrollDoc::open(&doc.save()).unwrap();
    assert_eq!(again.package().core().title.as_deref(), Some("A New Title"));
    assert_eq!(again.package().core().creator.as_deref(), Some("Invented Author"));
}

#[test]
fn resources_hold_the_outlines_the_pages_draw() {
    let doc = Docx::new(&format!("{}{SECT}", p("Outlines for every glyph"))).open();
    let dl = doc.display(0);
    let res = doc.resources().unwrap();
    for it in &dl.items {
        if let Item::Glyphs { font, glyphs, .. } = it {
            for g in glyphs {
                assert!(res.glyph(*font, g.gid).is_some(), "font {font} glyph {} missing", g.gid);
            }
        }
    }
    // The first item is the white page.
    assert!(matches!(&dl.items[0], Item::Path { fill: Some(Paint::Solid(c)), .. } if *c == Color::WHITE));
}

#[test]
fn lays_out_without_any_fonts() {
    let bytes = Docx::new(&format!("{}{TABLE}{SECT}", p("No fonts here"))).styles(TABLE_STYLES).bytes();
    let doc = ScrollDoc::open_with_fonts(&bytes, Arc::new(FontIndex::new())).unwrap();
    assert_eq!(doc.page_count(), 1);
    assert!(page_text(&doc, 0).starts_with("No fonts here"));
}
