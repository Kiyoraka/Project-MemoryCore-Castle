//! The parts of a new deck (one title slide, 16:9) and of a new notes page, written by us.

use crate::ground::xml::escape_text;
use crate::ground::zip::{Method, ZipWriter};

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

pub const THEME: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
    r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Scriptorium"><a:themeElements>"#,
    r#"<a:clrScheme name="Scriptorium"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>"#,
    r#"<a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1>"#,
    r#"<a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4>"#,
    r#"<a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink>"#,
    r#"<a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>"#,
    r#"<a:fontScheme name="Scriptorium"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>"#,
    r#"<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>"#,
    r#"<a:fmtScheme name="Scriptorium"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#,
    r#"<a:solidFill><a:schemeClr val="phClr"><a:tint val="80000"/></a:schemeClr></a:solidFill>"#,
    r#"<a:solidFill><a:schemeClr val="phClr"><a:shade val="80000"/></a:schemeClr></a:solidFill></a:fillStyleLst>"#,
    r#"<a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#,
    r#"<a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>"#,
    r#"<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>"#,
    r#"<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:tint val="95000"/></a:schemeClr></a:solidFill>"#,
    r#"<a:solidFill><a:schemeClr val="phClr"><a:shade val="90000"/></a:schemeClr></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
);

const CLR_MAP: &str = r#"<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#;

fn sp(id: u32, name: &str, ph: &str, xfrm: Option<(i64, i64, i64, i64)>, body_pr: &str, lst: &str, paras: &str) -> String {
    let xfrm = xfrm
        .map(|(x, y, cx, cy)| format!(r#"<a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#))
        .unwrap_or_default();
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr>{xfrm}</p:spPr><p:txBody>{body_pr}<a:lstStyle>{lst}</a:lstStyle>{paras}</p:txBody></p:sp>"#
    )
}

const TREE_HEAD: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;
const EMPTY_P: &str = r#"<a:p><a:endParaRPr lang="en-US"/></a:p>"#;

fn master() -> String {
    let tree = [
        sp(2, "Title Placeholder 1", r#"<p:ph type="title"/>"#, Some((838200, 365125, 10515600, 1325563)), r#"<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0" anchor="ctr"><a:normAutofit/></a:bodyPr>"#, "", EMPTY_P),
        sp(3, "Text Placeholder 2", r#"<p:ph type="body" idx="1"/>"#, Some((838200, 1825625, 10515600, 4351338)), r#"<a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0"><a:normAutofit/></a:bodyPr>"#, "", EMPTY_P),
        sp(4, "Date Placeholder 3", r#"<p:ph type="dt" sz="half" idx="2"/>"#, Some((838200, 6356350, 2743200, 365125)), r#"<a:bodyPr anchor="ctr"/>"#, r#"<a:lvl1pPr algn="l"><a:defRPr sz="1200"><a:solidFill><a:schemeClr val="tx1"><a:tint val="75000"/></a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>"#, EMPTY_P),
        sp(5, "Footer Placeholder 4", r#"<p:ph type="ftr" sz="quarter" idx="3"/>"#, Some((4038600, 6356350, 4114800, 365125)), r#"<a:bodyPr anchor="ctr"/>"#, r#"<a:lvl1pPr algn="ctr"><a:defRPr sz="1200"><a:solidFill><a:schemeClr val="tx1"><a:tint val="75000"/></a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>"#, EMPTY_P),
        sp(6, "Slide Number Placeholder 5", r#"<p:ph type="sldNum" sz="quarter" idx="4"/>"#, Some((8610600, 6356350, 2743200, 365125)), r#"<a:bodyPr anchor="ctr"/>"#, r#"<a:lvl1pPr algn="r"><a:defRPr sz="1200"><a:solidFill><a:schemeClr val="tx1"><a:tint val="75000"/></a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>"#, r#"<a:p><a:fld id="{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}" type="slidenum"><a:rPr lang="en-US"/><a:t>‹#›</a:t></a:fld><a:endParaRPr lang="en-US"/></a:p>"#),
    ]
    .concat();
    let body_lvl = |n: u32, mar: i64, sz: u32, ch: &str| {
        format!(
            r#"<a:lvl{n}pPr marL="{mar}" indent="-228600" algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPts val="1000"/></a:spcBef><a:buFont typeface="Arial"/><a:buChar char="{ch}"/><a:defRPr sz="{sz}" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl{n}pPr>"#
        )
    };
    let body: String = [
        body_lvl(1, 228600, 2800, "\u{2022}"),
        body_lvl(2, 685800, 2400, "\u{2022}"),
        body_lvl(3, 1143000, 2000, "\u{2022}"),
        body_lvl(4, 1600200, 1800, "\u{2022}"),
        body_lvl(5, 2057400, 1800, "\u{2022}"),
    ]
    .concat();
    format!(
        concat!(
            "{decl}<p:sldMaster {ns}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree>{head}{tree}</p:spTree></p:cSld>",
            "{clr}<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst>",
            "<p:txStyles><p:titleStyle><a:lvl1pPr algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\"><a:lnSpc><a:spcPct val=\"90000\"/></a:lnSpc><a:spcBef><a:spcPct val=\"0\"/></a:spcBef><a:buNone/>",
            "<a:defRPr sz=\"4400\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mj-lt\"/><a:ea typeface=\"+mj-ea\"/><a:cs typeface=\"+mj-cs\"/></a:defRPr></a:lvl1pPr></p:titleStyle>",
            "<p:bodyStyle>{body}</p:bodyStyle>",
            "<p:otherStyle><a:defPPr><a:defRPr lang=\"en-US\"/></a:defPPr><a:lvl1pPr marL=\"0\" algn=\"l\" defTabSz=\"914400\"><a:defRPr sz=\"1800\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:otherStyle>",
            "</p:txStyles></p:sldMaster>"
        ),
        decl = DECL,
        ns = NS,
        head = TREE_HEAD,
        tree = tree,
        clr = CLR_MAP,
        body = body
    )
}

fn layout() -> String {
    let tree = [
        sp(2, "Title 1", r#"<p:ph type="ctrTitle"/>"#, Some((1524000, 1122363, 9144000, 2387600)), r#"<a:bodyPr anchor="b"/>"#, r#"<a:lvl1pPr algn="ctr"><a:defRPr sz="6000"/></a:lvl1pPr>"#, EMPTY_P),
        sp(3, "Subtitle 2", r#"<p:ph type="subTitle" idx="1"/>"#, Some((1524000, 3602038, 9144000, 1655762)), "<a:bodyPr/>", r#"<a:lvl1pPr marL="0" indent="0" algn="ctr"><a:buNone/><a:defRPr sz="2400"/></a:lvl1pPr>"#, EMPTY_P),
    ]
    .concat();
    format!(
        "{}<p:sldLayout {} type=\"title\" preserve=\"1\"><p:cSld name=\"Title Slide\"><p:spTree>{}{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>",
        DECL, NS, TREE_HEAD, tree
    )
}

fn slide() -> String {
    let tree = [
        sp(2, "Title 1", r#"<p:ph type="ctrTitle"/>"#, None, "<a:bodyPr/>", "", EMPTY_P),
        sp(3, "Subtitle 2", r#"<p:ph type="subTitle" idx="1"/>"#, None, "<a:bodyPr/>", "", EMPTY_P),
    ]
    .concat();
    format!(
        "{}<p:sld {}><p:cSld><p:spTree>{}{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
        DECL, NS, TREE_HEAD, tree
    )
}

fn presentation() -> String {
    format!(
        concat!(
            "{}<p:presentation {} saveSubsetFonts=\"1\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>",
            "<p:sldIdLst><p:sldId id=\"256\" r:id=\"rId2\"/></p:sldIdLst><p:sldSz cx=\"12192000\" cy=\"6858000\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/>",
            "<p:defaultTextStyle><a:defPPr><a:defRPr lang=\"en-US\"/></a:defPPr><a:lvl1pPr marL=\"0\" algn=\"l\" defTabSz=\"914400\"><a:defRPr sz=\"1800\" kern=\"1200\">",
            "<a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle>",
            "</p:presentation>"
        ),
        DECL, NS
    )
}

fn rels(list: &[(&str, &str, &str)]) -> String {
    let mut s = format!("{}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">", DECL);
    for (id, ty, target) in list {
        s.push_str(&format!(
            "<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{}\" Target=\"{}\"/>",
            id, ty, target
        ));
    }
    s.push_str("</Relationships>");
    s
}

fn content_types() -> String {
    let o = |part: &str, ct: &str| format!("<Override PartName=\"{}\" ContentType=\"application/vnd.openxmlformats-officedocument.{}\"/>", part, ct);
    [
        DECL.to_string(),
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">".into(),
        "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>".into(),
        "<Default Extension=\"xml\" ContentType=\"application/xml\"/>".into(),
        "<Default Extension=\"png\" ContentType=\"image/png\"/>".into(),
        "<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>".into(),
        o("/ppt/presentation.xml", "presentationml.presentation.main+xml"),
        o("/ppt/slideMasters/slideMaster1.xml", "presentationml.slideMaster+xml"),
        o("/ppt/slideLayouts/slideLayout1.xml", "presentationml.slideLayout+xml"),
        o("/ppt/slides/slide1.xml", "presentationml.slide+xml"),
        o("/ppt/theme/theme1.xml", "theme+xml"),
        "</Types>".into(),
    ]
    .concat()
}

/// A new deck's zip: one title slide on a 16:9 page, its layout, master and theme.
pub fn blank_deck() -> Vec<u8> {
    let parts: Vec<(&str, String)> = vec![
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", rels(&[("rId1", "officeDocument", "ppt/presentation.xml")])),
        ("ppt/presentation.xml", presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            rels(&[("rId1", "slideMaster", "slideMasters/slideMaster1.xml"), ("rId2", "slide", "slides/slide1.xml"), ("rId3", "theme", "theme/theme1.xml")]),
        ),
        ("ppt/slideMasters/slideMaster1.xml", master()),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"), ("rId2", "theme", "../theme/theme1.xml")])),
        ("ppt/slideLayouts/slideLayout1.xml", layout()),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels(&[("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")])),
        ("ppt/slides/slide1.xml", slide()),
        ("ppt/slides/_rels/slide1.xml.rels", rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")])),
        ("ppt/theme/theme1.xml", THEME.to_string()),
    ];
    let mut w = ZipWriter::new();
    for (name, data) in parts {
        // Writing to memory with valid names cannot fail; a failure would only drop that entry.
        let _ = w.add(name, data.as_bytes(), Method::Deflated(6));
    }
    w.finish()
}

/// A notes master for a deck that has none.
pub const NOTES_MASTER: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n",
    r#"<p:notesMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
    r#"<p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#,
    r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg" idx="2"/></p:nvPr></p:nvSpPr>"#,
    r#"<p:spPr><a:xfrm><a:off x="685800" y="1143000"/><a:ext cx="5486400" cy="3086100"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln w="12700"><a:solidFill><a:prstClr val="black"/></a:solidFill></a:ln></p:spPr></p:sp>"#,
    r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" sz="quarter" idx="3"/></p:nvPr></p:nvSpPr>"#,
    r#"<p:spPr><a:xfrm><a:off x="685800" y="4400550"/><a:ext cx="5486400" cy="3600450"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>"#,
    r#"<p:txBody><a:bodyPr vert="horz" lIns="91440" tIns="45720" rIns="91440" bIns="45720" rtlCol="0"/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp>"#,
    r#"</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#,
    r#"<p:notesStyle><a:lvl1pPr marL="0" algn="l" defTabSz="914400"><a:defRPr sz="1200" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl1pPr></p:notesStyle>"#,
    "</p:notesMaster>"
);

/// A notes page holding `text` (lines become paragraphs).
pub fn notes_slide(text: &str) -> String {
    let paras: String = text
        .replace("\r\n", "\n")
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                r#"<a:p><a:endParaRPr lang="en-US"/></a:p>"#.to_string()
            } else {
                format!(r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>{}</a:t></a:r></a:p>"#, escape_text(line))
            }
        })
        .collect();
    format!(
        concat!(
            "{}<p:notes {}><p:cSld><p:spTree>{}",
            r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Slide Image Placeholder 1"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>"#,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes Placeholder 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/>"#,
            "<p:txBody><a:bodyPr/><a:lstStyle/>{}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"
        ),
        DECL, NS, TREE_HEAD, paras
    )
}
