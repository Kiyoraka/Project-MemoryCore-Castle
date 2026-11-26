//! Stage tests, on invented decks only: every part below is written here, by hand.

use super::*;
use crate::ground::bus::{Bus, Origin};
use crate::ground::display::{Color, Paint};
use crate::ground::picture::png;
use crate::ground::text::{Align, LineSpacing};
use crate::ground::zip::{Method, ZipWriter};

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";
const HEAD: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#;

const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Test Dusk"><a:themeElements>
<a:clrScheme name="Dusk"><a:dk1><a:srgbClr val="101010"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F2A44"/></a:dk2>
<a:lt2><a:srgbClr val="EEEAE0"/></a:lt2><a:accent1><a:srgbClr val="7B4FA0"/></a:accent1><a:accent2><a:srgbClr val="C0504D"/></a:accent2>
<a:accent3><a:srgbClr val="9BBB59"/></a:accent3><a:accent4><a:srgbClr val="8064A2"/></a:accent4><a:accent5><a:srgbClr val="4BACC6"/></a:accent5>
<a:accent6><a:srgbClr val="F79646"/></a:accent6><a:hlink><a:srgbClr val="0000EE"/></a:hlink><a:folHlink><a:srgbClr val="551A8B"/></a:folHlink></a:clrScheme>
<a:fontScheme name="Dusk"><a:majorFont><a:latin typeface="Georgia"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>
<a:minorFont><a:latin typeface="Verdana"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>
<a:fmtScheme name="Dusk"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:tint val="50000"/></a:schemeClr></a:solidFill>
<a:solidFill><a:schemeClr val="phClr"><a:shade val="50000"/></a:schemeClr></a:solidFill></a:fillStyleLst>
<a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="25400"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
<a:ln w="38100"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>
<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst>
</a:fmtScheme></a:themeElements></a:theme>"#;

const CLR_MAP_ATTRS: &str = r#"bg1="lt1" tx1="dk1" bg2="lt2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink""#;

fn xfrm(x: i64, y: i64, cx: i64, cy: i64) -> String {
    format!(r#"<a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#)
}

fn ph_sp(id: u32, name: &str, ph: &str, sp_pr: &str, body: &str) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr>{sp_pr}</p:spPr>{body}</p:sp>"#
    )
}

fn master() -> String {
    let tree = [
        ph_sp(2, "Title Placeholder 1", r#"<p:ph type="title"/>"#, &xfrm(457200, 274638, 8229600, 1143000), r#"<p:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody>"#),
        ph_sp(3, "Text Placeholder 2", r#"<p:ph type="body" idx="1"/>"#, &xfrm(457200, 1600200, 8229600, 4525963), r#"<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody>"#),
        format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="7" name="Master Band"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></p:spPr></p:sp>"#, xfrm(0, 6400800, 9144000, 457200)),
    ]
    .concat();
    format!(
        concat!(
            "{decl}<p:sldMaster {ns}><p:cSld><p:bg><p:bgPr><a:solidFill><a:schemeClr val=\"bg2\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>{head}{tree}</p:spTree></p:cSld>",
            "<p:clrMap {clr} tx2=\"dk2\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/><p:sldLayoutId id=\"2147483650\" r:id=\"rId2\"/></p:sldLayoutIdLst>",
            "<p:txStyles><p:titleStyle><a:lvl1pPr algn=\"l\"><a:buNone/><a:defRPr sz=\"4000\"><a:solidFill><a:schemeClr val=\"tx2\"/></a:solidFill><a:latin typeface=\"+mj-lt\"/></a:defRPr></a:lvl1pPr></p:titleStyle>",
            "<p:bodyStyle><a:lvl1pPr marL=\"342900\" indent=\"-342900\"><a:buFont typeface=\"Arial\"/><a:buChar char=\"\u{2022}\"/><a:defRPr sz=\"2800\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:lvl1pPr>",
            "<a:lvl2pPr marL=\"742950\" indent=\"-285750\"><a:buChar char=\"\u{2013}\"/><a:defRPr sz=\"2400\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:lvl2pPr></p:bodyStyle>",
            "<p:otherStyle><a:lvl1pPr><a:defRPr sz=\"1800\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>"
        ),
        decl = DECL,
        ns = NS,
        head = HEAD,
        tree = tree,
        clr = CLR_MAP_ATTRS
    )
}

fn layout1() -> String {
    let tree = [
        ph_sp(2, "Title 1", r#"<p:ph type="title"/>"#, &xfrm(914400, 457200, 7315200, 1371600), r#"<p:txBody><a:bodyPr/><a:lstStyle><a:lvl1pPr algn="ctr"><a:defRPr b="1"/></a:lvl1pPr></a:lstStyle><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit</a:t></a:r></a:p></p:txBody>"#),
        ph_sp(3, "Content Placeholder 2", r#"<p:ph idx="1"/>"#, "", r#"<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody>"#),
    ]
    .concat();
    format!("{DECL}<p:sldLayout {NS} type=\"obj\" preserve=\"1\"><p:cSld name=\"Title and Content\"><p:spTree>{HEAD}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>")
}

fn layout2() -> String {
    let tree = ph_sp(2, "Title 1", r#"<p:ph type="title"/>"#, "", r#"<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody>"#);
    format!(
        "{DECL}<p:sldLayout {NS} preserve=\"1\"><p:cSld name=\"Accent Title\"><p:spTree>{HEAD}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:overrideClrMapping {CLR_MAP_ATTRS} tx2=\"accent2\"/></p:clrMapOvr></p:sldLayout>"
    )
}

fn slide1() -> String {
    let tree = [
        ph_sp(2, "Title 1", r#"<p:ph type="title"/>"#, "", r#"<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-GB" dirty="0"/><a:t>Hello Stage</a:t></a:r></a:p></p:txBody>"#),
        ph_sp(
            3,
            "Content 2",
            r#"<p:ph idx="1"/>"#,
            "",
            concat!(
                r#"<p:txBody><a:bodyPr><a:normAutofit fontScale="62500" lnSpcReduction="20000"/></a:bodyPr><a:lstStyle/>"#,
                r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>First point</a:t></a:r></a:p>"#,
                r#"<a:p><a:pPr lvl="1"/><a:r><a:rPr lang="en-US"/><a:t>Second point</a:t></a:r></a:p>"#,
                r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>Third point</a:t></a:r></a:p></p:txBody>"#
            ),
        ),
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="4" name="Box"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:ln w="25400"><a:solidFill><a:srgbClr val="0000FF"/></a:solidFill></a:ln></p:spPr></p:sp>"#,
            xfrm(1270000, 1270000, 2540000, 1270000)
        ),
        format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="5" name="Number"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap="none"/><a:lstStyle/><a:p><a:fld id="{{11111111-2222-3333-4444-555555555555}}" type="slidenum"><a:rPr lang="en-US"/><a:t>‹#›</a:t></a:fld></a:p></p:txBody></p:sp>"#,
            xfrm(8001000, 6477000, 635000, 254000)
        ),
        format!(
            concat!(
                r#"<p:sp><p:nvSpPr><p:cNvPr id="6" name="Numbered"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>"#,
                r#"<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr marL="342900" indent="-342900"><a:buAutoNum type="arabicPeriod"/></a:pPr><a:r><a:rPr lang="en-US"/><a:t>One</a:t></a:r></a:p>"#,
                r#"<a:p><a:pPr marL="342900" indent="-342900"><a:buAutoNum type="arabicPeriod"/></a:pPr><a:r><a:rPr lang="en-US"/><a:t>Two</a:t></a:r></a:p></p:txBody></p:sp>"#
            ),
            xfrm(5080000, 1270000, 2540000, 1270000)
        ),
    ]
    .concat();
    format!(
        "{DECL}<p:sld {NS}><p:cSld><p:spTree>{HEAD}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr><p:extLst><p:ext uri=\"{{TEST-UNKNOWN}}\"><x:mine xmlns:x=\"urn:test\" keep=\"me\"/></p:ext></p:extLst></p:sld>"
    )
}

fn slide2() -> String {
    let pic = format!(
        concat!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="3" name="Logo"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>"#,
            r#"<p:blipFill><a:blip r:embed="rId2"/><a:srcRect l="25000"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>"#,
            r#"<p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#
        ),
        xfrm(0, 0, 1270000, 1270000)
    );
    let group = format!(
        concat!(
            r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="4" name="Group 1"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>"#,
            r#"<p:grpSpPr><a:xfrm><a:off x="2540000" y="2540000"/><a:ext cx="2540000" cy="1270000"/><a:chOff x="0" y="0"/><a:chExt cx="1270000" cy="635000"/></a:xfrm></p:grpSpPr>"#,
            r#"<p:sp><p:nvSpPr><p:cNvPr id="5" name="Child"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></p:spPr></p:sp>"#,
            r#"</p:grpSp>"#
        ),
        xfrm(127000, 127000, 254000, 254000)
    );
    let cell = |t: &str, pr: &str| format!(r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>{t}</a:t></a:r></a:p></a:txBody><a:tcPr>{pr}</a:tcPr></a:tc>"#);
    let table = format!(
        concat!(
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="6" name="Table 1"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>"#,
            r#"<p:xfrm><a:off x="0" y="3810000"/><a:ext cx="2540000" cy="762000"/></p:xfrm>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="1" bandRow="1"><a:tableStyleId>{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}</a:tableStyleId></a:tblPr>"#,
            r#"<a:tblGrid><a:gridCol w="1270000"/><a:gridCol w="1270000"/></a:tblGrid>"#,
            r#"<a:tr h="381000">{}{}</a:tr><a:tr h="381000">{}{}</a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#
        ),
        cell("a", ""),
        cell("b", ""),
        cell("c", r#"<a:solidFill><a:srgbClr val="FFFF00"/></a:solidFill>"#),
        cell("d", "")
    );
    let styled = format!(
        concat!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="7" name="Styled"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="roundRect"><a:avLst/></a:prstGeom></p:spPr>"#,
            r#"<p:style><a:lnRef idx="1"><a:schemeClr val="accent1"/></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></p:style>"#,
            r#"<p:txBody><a:bodyPr rtlCol="0" anchor="ctr"/><a:lstStyle/><a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US"/><a:t>Styled text</a:t></a:r></a:p></p:txBody></p:sp>"#
        ),
        xfrm(5080000, 2540000, 2540000, 1270000)
    );
    let connector = format!(
        r#"<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="8" name="Connector"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr>{}<a:prstGeom prst="line"><a:avLst/></a:prstGeom><a:ln w="12700"><a:solidFill><a:srgbClr val="333333"/></a:solidFill></a:ln></p:spPr></p:cxnSp>"#,
        xfrm(0, 5080000, 2540000, 0)
    );
    let title = ph_sp(2, "Title 1", r#"<p:ph type="title"/>"#, "", r#"<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Override</a:t></a:r></a:p></p:txBody>"#);
    format!("{DECL}<p:sld {NS}><p:cSld><p:spTree>{HEAD}{title}{pic}{group}{table}{styled}{connector}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>")
}

fn notes1() -> String {
    let tree = ph_sp(3, "Notes Placeholder 2", r#"<p:ph type="body" idx="1"/>"#, "", r#"<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Remember the demo</a:t></a:r></a:p></p:txBody>"#);
    format!("{DECL}<p:notes {NS}><p:cSld><p:spTree>{HEAD}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>")
}

fn rels(list: &[(&str, &str, &str)]) -> String {
    let mut s = format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for (id, ty, target) in list {
        s.push_str(&format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{ty}\" Target=\"{target}\"/>"));
    }
    s + "</Relationships>"
}

fn content_types() -> String {
    let o = |part: &str, ct: &str| format!("<Override PartName=\"{part}\" ContentType=\"application/vnd.openxmlformats-officedocument.{ct}\"/>");
    [
        DECL.to_string(),
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">".into(),
        "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>".into(),
        "<Default Extension=\"xml\" ContentType=\"application/xml\"/>".into(),
        "<Default Extension=\"png\" ContentType=\"image/png\"/>".into(),
        o("/ppt/presentation.xml", "presentationml.presentation.main+xml"),
        o("/ppt/slideMasters/slideMaster1.xml", "presentationml.slideMaster+xml"),
        o("/ppt/slideLayouts/slideLayout1.xml", "presentationml.slideLayout+xml"),
        o("/ppt/slideLayouts/slideLayout2.xml", "presentationml.slideLayout+xml"),
        o("/ppt/slides/slide1.xml", "presentationml.slide+xml"),
        o("/ppt/slides/slide2.xml", "presentationml.slide+xml"),
        o("/ppt/notesSlides/notesSlide1.xml", "presentationml.notesSlide+xml"),
        o("/ppt/theme/theme1.xml", "theme+xml"),
        "</Types>".into(),
    ]
    .concat()
}

fn presentation() -> String {
    format!(
        concat!(
            "{}<p:presentation {}><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>",
            "<p:sldIdLst><p:sldId id=\"256\" r:id=\"rId2\"/><p:sldId id=\"257\" r:id=\"rId3\"/></p:sldIdLst>",
            "<p:sldSz cx=\"9144000\" cy=\"6858000\" type=\"screen4x3\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/>",
            "<p:defaultTextStyle><a:lvl1pPr><a:defRPr sz=\"1800\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle>",
            "</p:presentation>"
        ),
        DECL, NS
    )
}

/// The invented deck: two slides, a master with two layouts, a theme, a picture and a notes page.
fn deck_bytes() -> Vec<u8> {
    let image = png::encode_rgba8(2, 2, &[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]);
    let parts: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", content_types().into_bytes()),
        ("_rels/.rels", rels(&[("rId1", "officeDocument", "ppt/presentation.xml")]).into_bytes()),
        ("ppt/presentation.xml", presentation().into_bytes()),
        (
            "ppt/_rels/presentation.xml.rels",
            rels(&[("rId1", "slideMaster", "slideMasters/slideMaster1.xml"), ("rId2", "slide", "slides/slide1.xml"), ("rId3", "slide", "slides/slide2.xml"), ("rId4", "theme", "theme/theme1.xml")]).into_bytes(),
        ),
        ("ppt/slideMasters/slideMaster1.xml", master().into_bytes()),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"), ("rId2", "slideLayout", "../slideLayouts/slideLayout2.xml"), ("rId3", "theme", "../theme/theme1.xml")]).into_bytes(),
        ),
        ("ppt/slideLayouts/slideLayout1.xml", layout1().into_bytes()),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels(&[("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")]).into_bytes()),
        ("ppt/slideLayouts/slideLayout2.xml", layout2().into_bytes()),
        ("ppt/slideLayouts/_rels/slideLayout2.xml.rels", rels(&[("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")]).into_bytes()),
        ("ppt/slides/slide1.xml", slide1().into_bytes()),
        ("ppt/slides/_rels/slide1.xml.rels", rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"), ("rId2", "notesSlide", "../notesSlides/notesSlide1.xml")]).into_bytes()),
        ("ppt/slides/slide2.xml", slide2().into_bytes()),
        ("ppt/slides/_rels/slide2.xml.rels", rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout2.xml"), ("rId2", "image", "../media/image1.png")]).into_bytes()),
        ("ppt/notesSlides/notesSlide1.xml", notes1().into_bytes()),
        ("ppt/notesSlides/_rels/notesSlide1.xml.rels", rels(&[("rId1", "slide", "../slides/slide1.xml")]).into_bytes()),
        ("ppt/theme/theme1.xml", THEME.as_bytes().to_vec()),
        ("ppt/media/image1.png", image),
        ("customXml/item1.xml", b"<mine xmlns=\"urn:test\">kept as is</mine>".to_vec()),
    ];
    let mut w = ZipWriter::new();
    for (name, data) in parts {
        w.add(name, &data, Method::Deflated(6)).unwrap();
    }
    w.finish()
}

fn open(bytes: &[u8]) -> StageDoc {
    StageDoc::open_with_fonts(bytes, FontCache::empty()).unwrap()
}

fn hex(c: Color) -> String {
    format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn solid_fills(dl: &DisplayList) -> Vec<String> {
    dl.items
        .iter()
        .filter_map(|i| match i {
            Item::Path { fill: Some(Paint::Solid(c)), .. } => Some(hex(*c)),
            _ => None,
        })
        .collect()
}

fn cmd(name: &str, args: Json) -> Command {
    Command::new(name, args)
}

#[test]
fn open_and_save_unchanged_is_byte_identical() {
    let bytes = deck_bytes();
    let doc = open(&bytes);
    assert_eq!(doc.slide_count(), 2);
    assert_eq!(doc.page_count(), 2);
    assert_eq!(doc.kind(), "stage");
    assert_eq!(doc.slide_size_pt(), (720.0, 540.0));
    assert_eq!(doc.save(), bytes);
}

#[test]
fn set_shape_text_reopens_with_it_and_leaves_other_parts_untouched() {
    let bytes = deck_bytes();
    let mut doc = open(&bytes);
    doc.apply(&cmd("stage.set_shape_text", Json::obj().with("slide", 0).with("shape", "Title 1").with("text", "New title"))).unwrap();
    let saved = doc.save();
    assert_ne!(saved, bytes);
    let again = open(&saved);
    let info = again.shape_info(0, "Title 1").unwrap();
    assert_eq!(info.paragraphs.len(), 1);
    assert_eq!(info.paragraphs[0].runs[0].text, "New title");
    let xml = String::from_utf8(again.package().part_bytes("/ppt/slides/slide1.xml").unwrap()).unwrap();
    assert!(xml.contains(r#"<a:rPr lang="en-GB" dirty="0"/><a:t>New title</a:t>"#), "first run's properties kept: {}", xml);
    assert!(xml.contains(r#"<x:mine xmlns:x="urn:test" keep="me"/>"#), "unknown extension kept");
    assert!(xml.contains("First point"), "other shapes untouched");
    let before = Package::open(&bytes).unwrap();
    for name in before.part_names() {
        if name == "/ppt/slides/slide1.xml" {
            continue;
        }
        assert_eq!(before.raw_part(&name), again.package().raw_part(&name), "{} must be byte-identical", name);
    }
}

#[test]
fn title_placeholder_inherits_position_font_size_colour_and_bold() {
    let doc = open(&deck_bytes());
    let info = doc.shape_info(0, "Title 1").unwrap();
    assert_eq!(info.placeholder.as_ref().unwrap().ty, "title");
    // Position from the layout placeholder (the slide's own spPr is empty).
    let r = info.rect.unwrap();
    assert!(near(r.x, 72.0) && near(r.y, 36.0) && near(r.w, 576.0) && near(r.h, 108.0), "{:?}", r);
    // Anchor from the master placeholder's bodyPr.
    assert_eq!(info.body.anchor.as_deref(), Some("ctr"));
    let p = &info.paragraphs[0];
    assert_eq!(p.align, Align::Center, "layout lstStyle");
    let st = &p.runs[0].style;
    assert_eq!(st.family, "Georgia", "+mj-lt from the master title style through the theme");
    assert!(near(st.size_pt, 40.0), "master title style size");
    assert_eq!(st.weight, 700, "bold from the layout lstStyle");
    assert_eq!(hex(st.color), "1F2A44", "tx2 -> dk2 through the master clrMap");
    assert!(p.bullet.is_none(), "titles have buNone");
}

#[test]
fn layout_colour_map_override_changes_the_title_colour() {
    let doc = open(&deck_bytes());
    let info = doc.shape_info(1, "Title 1").unwrap();
    assert_eq!(hex(info.paragraphs[0].runs[0].style.color), "C0504D", "tx2 -> accent2 by the layout's override");
    // Position from the master (layout 2's title has none).
    let r = info.rect.unwrap();
    assert!(near(r.x, 36.0) && near(r.y, 274638.0 / 12700.0), "{:?}", r);
}

#[test]
fn rectangle_with_solid_fill_and_line_becomes_paths_with_those_colours() {
    let doc = open(&deck_bytes());
    let dl = doc.display(0);
    assert!(near(dl.width, 720.0) && near(dl.height, 540.0));
    // Background first: bg2 -> lt2 from the master's bgPr.
    match &dl.items[0] {
        Item::Path { fill: Some(Paint::Solid(c)), .. } => assert_eq!(hex(*c), "EEEAE0"),
        other => panic!("background expected, got {:?}", other),
    }
    let boxed = dl
        .items
        .iter()
        .find_map(|i| match i {
            Item::Path { fill: Some(Paint::Solid(c)), stroke: Some(s), .. } if hex(*c) == "FF0000" => Some(s.clone()),
            _ => None,
        })
        .expect("the red box");
    assert_eq!(hex(boxed.color), "0000FF");
    assert!(near(boxed.width, 2.0));
    // Its place on the slide.
    let wb = dl.world_bounds();
    let i = dl.items.iter().position(|i| matches!(i, Item::Path { fill: Some(Paint::Solid(c)), .. } if hex(*c) == "FF0000")).unwrap();
    let b = wb[i].unwrap();
    assert!(near(b.x, 99.0) && near(b.y, 99.0) && near(b.w, 202.0) && near(b.h, 102.0), "{:?}", b);
    // The master's own decorative shape is drawn on the slide; its placeholders are not.
    assert!(solid_fills(&dl).contains(&"7B4FA0".to_string()));
}

#[test]
fn picture_becomes_a_cropped_image_item() {
    let doc = open(&deck_bytes());
    let dl = doc.display(1);
    let (id, rect) = dl
        .items
        .iter()
        .find_map(|i| match i {
            Item::Image { image, rect } => Some((*image, *rect)),
            _ => None,
        })
        .expect("an image item");
    // srcRect l=25%: the visible 75% fills the 100 pt frame, so the whole picture is 133.3 pt wide.
    assert!(near(rect.w, 100.0 / 0.75) && near(rect.x, -100.0 / 3.0) && near(rect.h, 100.0), "{:?}", rect);
    let img = doc.resources().unwrap().image(id).expect("decoded into the resources");
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
    let i = dl.items.iter().position(|i| matches!(i, Item::Image { .. })).unwrap();
    assert!(matches!(dl.items[i - 1], Item::PushClip(_)) && matches!(dl.items[i + 1], Item::PopClip));
}

#[test]
fn group_children_are_placed_through_the_group_transform() {
    let doc = open(&deck_bytes());
    let dl = doc.display(1);
    let i = dl.items.iter().position(|i| matches!(i, Item::Path { fill: Some(Paint::Solid(c)), .. } if hex(*c) == "00FF00")).expect("the group's child");
    let b = dl.world_bounds()[i].unwrap();
    // Child space 100x50 pt mapped onto a 200x100 pt group at (200, 200): the child at (10,10) 20x20 lands at (220,220) 40x40.
    assert!(near(b.x, 220.0) && near(b.y, 220.0) && near(b.w, 40.0) && near(b.h, 40.0), "{:?}", b);
}

#[test]
fn bullets_levels_and_autofit_font_scale_are_applied() {
    let doc = open(&deck_bytes());
    let info = doc.shape_info(0, "Content 2").unwrap();
    // The slide's `<p:ph idx="1"/>` finds the layout's content placeholder, then the master's body.
    let r = info.rect.unwrap();
    assert!(near(r.x, 36.0) && near(r.y, 126.0), "{:?}", r);
    let p = &info.paragraphs;
    assert_eq!(p.len(), 3);
    let b0 = p[0].bullet.as_ref().unwrap();
    assert_eq!(b0.text, "\u{2022}\t");
    assert_eq!(b0.style.family, "Arial", "buFont");
    assert!(near(p[0].runs[0].style.size_pt, 28.0 * 0.625), "fontScale 62.5%: {}", p[0].runs[0].style.size_pt);
    assert_eq!(p[0].runs[0].style.family, "Verdana");
    assert!(near(p[0].mar_l, 27.0) && near(p[0].indent, -27.0));
    assert_eq!(p[0].line_spacing, LineSpacing::Multiple(0.8), "lnSpcReduction 20%");
    assert_eq!(p[1].level, 1);
    assert_eq!(p[1].bullet.as_ref().unwrap().text, "\u{2013}\t");
    assert!(near(p[1].runs[0].style.size_pt, 15.0));
    assert!(near(p[1].mar_l, 58.5));
    assert_eq!(p[2].bullet.as_ref().unwrap().text, "\u{2022}\t");
    // Numbered paragraphs count.
    let n = doc.shape_info(0, "Numbered").unwrap();
    let nums: Vec<String> = n.paragraphs.iter().map(|p| p.bullet.as_ref().unwrap().text.clone()).collect();
    assert_eq!(nums, vec!["1.\t", "2.\t"]);
    // A text box takes the master's other style.
    assert!(near(n.paragraphs[0].runs[0].style.size_pt, 18.0));
}

#[test]
fn slide_number_field_shows_the_number() {
    let doc = open(&deck_bytes());
    let n = doc.shape_info(0, "Number").unwrap();
    assert_eq!(n.paragraphs[0].runs[0].text, "1");
    let read = doc.read(&Json::obj().with("slide", 0)).unwrap();
    let shapes = read.get("shapes").unwrap().as_arr().unwrap();
    let num = shapes.iter().find(|s| s.get("name").and_then(Json::as_str) == Some("Number")).unwrap();
    assert_eq!(num.get("text").and_then(Json::as_str), Some("1"));
}

#[test]
fn shape_style_references_give_fill_line_and_text_colour() {
    let doc = open(&deck_bytes());
    let s = doc.shape_info(1, "Styled").unwrap();
    match s.fill {
        Some(Fill::Solid(c)) => assert_eq!(hex(c), "7B4FA0"),
        other => panic!("fillRef idx 1 with accent1: {:?}", other),
    }
    let st = s.line.unwrap().to_stroke().unwrap();
    assert_eq!(hex(st.color), "7B4FA0");
    assert!(near(st.width, 0.75));
    let run = &s.paragraphs[0].runs[0].style;
    assert_eq!(hex(run.color), "FFFFFF", "fontRef lt1 wins over the master's other style");
    assert_eq!(run.family, "Verdana");
    assert_eq!(s.body.anchor.as_deref(), Some("ctr"));
    assert!(matches!(s.geometry, Geometry::Preset { ref name, .. } if name == "roundRect"));
}

#[test]
fn tables_and_connectors_draw() {
    let doc = open(&deck_bytes());
    let dl = doc.display(1);
    let fills = solid_fills(&dl);
    assert!(fills.contains(&"FFFF00".to_string()), "the cell's own fill");
    assert!(fills.contains(&"7B4FA0".to_string()), "header row from the default table style");
    let stroked = dl.items.iter().any(|i| matches!(i, Item::Path { stroke: Some(s), fill: None, .. } if hex(s.color) == "333333"));
    assert!(stroked, "the connector line");
    let read = doc.read(&Json::obj().with("slide", 1)).unwrap();
    let shapes = read.get("shapes").unwrap().as_arr().unwrap();
    let t = shapes.iter().find(|s| s.get("name").and_then(Json::as_str) == Some("Table 1")).unwrap();
    assert_eq!(t.get("kind").and_then(Json::as_str), Some("table"));
    assert_eq!(t.get("text").and_then(Json::as_str), Some("a\tb\nc\td"));
}

#[test]
fn describe_and_read_outline_the_deck() {
    let doc = open(&deck_bytes());
    let d = doc.describe();
    let slides = d.get("slides").unwrap().as_arr().unwrap();
    assert_eq!(slides.len(), 2);
    assert_eq!(slides[0].get("layout").and_then(Json::as_str), Some("Title and Content"));
    assert_eq!(slides[0].get("title").and_then(Json::as_str), Some("Hello Stage"));
    assert_eq!(slides[0].get("notes").and_then(Json::as_str), Some("Remember the demo"));
    let phs = slides[0].get("placeholders").unwrap().as_arr().unwrap();
    assert_eq!(phs.len(), 2);
    assert_eq!(phs[1].get("text").and_then(Json::as_str), Some("First point\nSecond point\nThird point"));
    let others = slides[1].get("shapes").unwrap().as_arr().unwrap();
    let kinds: Vec<&str> = others.iter().filter_map(|s| s.get("kind").and_then(Json::as_str)).collect();
    assert_eq!(kinds, vec!["picture", "group", "shape", "table", "shape", "connector"]);
    assert_eq!(slides[1].get("notes"), Some(&Json::Null));
    let all = doc.read(&Json::obj()).unwrap();
    assert_eq!(all.get("slides").unwrap().as_arr().unwrap().len(), 2);
    assert!(doc.read(&Json::obj().with("slide", 9)).is_err());
}

#[test]
fn the_door_sets_text_and_undo_restores_the_exact_part() {
    let bytes = deck_bytes();
    let doc = open(&bytes);
    let original = doc.package().part_bytes("/ppt/slides/slide1.xml").unwrap();
    let mut bus = Bus::new(doc);
    let d = bus.door("describe", &Json::obj()).unwrap();
    assert_eq!(d.get("kind").and_then(Json::as_str), Some("stage"));
    let args = Json::parse(r#"{"commands":[{"name":"stage.set_shape_text","args":{"slide":0,"shape":"Content 2","text":"Alpha\nBeta"}}]}"#).unwrap();
    let done = bus.door("do", &args).unwrap();
    assert_eq!(done.get("pages").unwrap().as_arr().unwrap(), &[Json::from(0usize)]);
    let read = bus.door("read", &Json::obj().with("slide", 0)).unwrap();
    let shapes = read.get("shapes").unwrap().as_arr().unwrap();
    let c = shapes.iter().find(|s| s.get("name").and_then(Json::as_str) == Some("Content 2")).unwrap();
    assert_eq!(c.get("text").and_then(Json::as_str), Some("Alpha\nBeta"));
    let info = bus.doc().shape_info(0, "Content 2").unwrap();
    assert!(near(info.paragraphs[1].runs[0].style.size_pt, 17.5), "formatting and autofit kept");
    bus.undo().unwrap();
    assert_eq!(bus.doc().package().part_bytes("/ppt/slides/slide1.xml").unwrap(), original, "undo gives back the exact bytes");
    assert_eq!(bus.doc().shape_info(0, "Content 2").unwrap().paragraphs.len(), 3);
    bus.redo().unwrap();
    assert_eq!(bus.doc().shape_info(0, "Content 2").unwrap().paragraphs.len(), 2);
    // By id too, and unknown shapes are refused with the list.
    bus.do_one(cmd("stage.set_shape_text", Json::obj().with("slide", 0).with("shape", 4).with("text", "In the box")), Origin::User).unwrap();
    assert_eq!(bus.doc().shape_info(0, "Box").unwrap().paragraphs[0].runs[0].text, "In the box");
    let err = bus.do_one(cmd("stage.set_shape_text", Json::obj().with("slide", 0).with("shape", "Nope").with("text", "x")), Origin::User).unwrap_err();
    assert!(err.to_string().contains("Title 1"), "{}", err);
    let shot = bus.door("shot", &Json::obj().with("page", 1)).unwrap();
    assert!(near(shot.get("width").and_then(Json::as_f64).unwrap(), 720.0));
}

#[test]
fn notes_are_set_created_and_undone() {
    let bytes = deck_bytes();
    let doc = open(&bytes);
    let pres_before = doc.package().part_bytes("/ppt/presentation.xml").unwrap();
    let mut bus = Bus::new(doc);
    bus.do_one(cmd("stage.set_notes", Json::obj().with("slide", 0).with("text", "New notes\nline two")), Origin::Ai).unwrap();
    assert_eq!(bus.doc().deck().notes_text(0).as_deref(), Some("New notes\nline two"));
    bus.undo().unwrap();
    assert_eq!(bus.doc().deck().notes_text(0).as_deref(), Some("Remember the demo"));

    // Slide 2 has no notes page: one is made, with a notes master.
    bus.do_one(cmd("stage.set_notes", Json::obj().with("slide", 1).with("text", "Fresh <notes> & more")), Origin::Ai).unwrap();
    assert_eq!(bus.doc().deck().notes_text(1).as_deref(), Some("Fresh <notes> & more"));
    let reopened = open(&bus.doc().save());
    assert_eq!(reopened.deck().notes_text(1).as_deref(), Some("Fresh <notes> & more"));
    assert!(reopened.package().part("/ppt/notesMasters/notesMaster1.xml").is_some());
    bus.undo().unwrap();
    assert_eq!(bus.doc().deck().notes_text(1), None);
    assert!(bus.doc().package().part("/ppt/notesMasters/notesMaster1.xml").is_none());
    assert!(bus.doc().package().part("/ppt/notesSlides/notesSlide2.xml").is_none());
    assert_eq!(bus.doc().package().part_bytes("/ppt/presentation.xml").unwrap(), pres_before);
    assert_eq!(bus.doc().deck().notes_text(0).as_deref(), Some("Remember the demo"));
}

#[test]
fn new_blank_is_one_title_slide_on_a_wide_page() {
    let mut doc = StageDoc::new_blank_with_fonts(FontCache::empty());
    assert_eq!(doc.slide_count(), 1);
    assert_eq!(doc.slide_size_pt(), (960.0, 540.0));
    let d = doc.describe();
    let s0 = &d.get("slides").unwrap().as_arr().unwrap()[0];
    assert_eq!(s0.get("layout").and_then(Json::as_str), Some("Title Slide"));
    doc.apply(&cmd("stage.set_shape_text", Json::obj().with("slide", 0).with("shape", "Title 1").with("text", "My deck"))).unwrap();
    let info = doc.shape_info(0, "Title 1").unwrap();
    let st = &info.paragraphs[0].runs[0].style;
    assert_eq!(st.family, "Calibri Light");
    assert!(near(st.size_pt, 60.0));
    assert_eq!(info.paragraphs[0].align, Align::Center);
    let r = info.rect.unwrap();
    assert!(near(r.x, 120.0) && near(r.w, 720.0));
    let again = open(&doc.save());
    assert_eq!(again.shape_info(0, "Title 1").unwrap().paragraphs[0].runs[0].text, "My deck");
    assert_eq!(solid_fills(&again.display(0))[0], "FFFFFF", "bgRef 1001 with bg1");
}

#[test]
fn thumbnail_scales_the_slide() {
    let doc = open(&deck_bytes());
    let t = doc.thumbnail(0, 180.0);
    assert!(near(t.width, 180.0) && near(t.height, 135.0));
    match &t.items[0] {
        Item::PushTransform(m) => assert!(near(m.a, 0.25) && near(m.d, 0.25)),
        other => panic!("{:?}", other),
    }
    assert_eq!(t.items.len(), doc.display(0).items.len() + 2);
}

#[test]
fn junk_is_refused_without_panicking() {
    assert!(StageDoc::open_with_fonts(b"not a zip", FontCache::empty()).is_err());
    let mut w = ZipWriter::new();
    w.add("[Content_Types].xml", content_types().as_bytes(), Method::Stored).unwrap();
    assert!(StageDoc::open_with_fonts(&w.finish(), FontCache::empty()).is_err());
    let mut doc = open(&deck_bytes());
    assert!(doc.apply(&cmd("stage.set_shape_text", Json::obj().with("slide", 0).with("shape", "Title 1"))).is_err());
    assert!(doc.apply(&cmd("stage.nope", Json::obj())).is_err());
    assert!(doc.apply(&cmd("stage.set_shape_text", Json::obj().with("slide", 1).with("shape", "Logo").with("text", "x"))).is_err());
}

#[test]
fn autonumber_schemes() {
    assert_eq!(style::autonum_text("arabicPeriod", 3), "3.");
    assert_eq!(style::autonum_text("romanUcPeriod", 4), "IV.");
    assert_eq!(style::autonum_text("alphaLcParenR", 2), "b)");
    assert_eq!(style::autonum_text("arabicParenBoth", 7), "(7)");
    assert_eq!(style::autonum_text("alphaUcPeriod", 27), "AA.");
}

#[test]
fn with_system_fonts_text_becomes_glyphs_with_outlines() {
    let doc = StageDoc::open(&deck_bytes()).unwrap();
    if doc.fonts.index().faces().is_empty() {
        return; // a machine without fonts: nothing to draw letters with
    }
    let dl = doc.display(0);
    let runs: Vec<(u32, usize)> = dl
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Glyphs { font, glyphs, .. } => Some((*font, glyphs.len())),
            _ => None,
        })
        .collect();
    assert!(!runs.is_empty(), "the slide's text is drawn as glyph runs");
    let res = doc.resources().unwrap();
    for (font, _) in &runs {
        assert!(res.font(*font).is_some_and(|f| !f.glyphs.is_empty()), "outlines for font {}", font);
    }
    // The title is centred in its box: in the shape's own space (inside its transform) the glyph
    // origins sit around the box's middle, 576 / 2 = 288 (less half a glyph).
    let title_x: Vec<f64> = dl
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Glyphs { size, glyphs, .. } if (*size - 40.0).abs() < 1e-6 => Some(glyphs.iter().map(|g| g.x).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(!title_x.is_empty());
    let mid = (title_x.iter().cloned().fold(f64::MAX, f64::min) + title_x.iter().cloned().fold(f64::MIN, f64::max)) / 2.0;
    assert!(mid > 248.0 && mid < 300.0, "title centred near 288 pt, got {}", mid);
}

#[test]
fn a_deck_can_move_between_threads() {
    fn send<T: Send>() {}
    send::<StageDoc>();
}
