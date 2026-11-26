//! Folio's tests, on invented PDFs only: hand-written bytes and files made by our own `pdfw` writer.

use std::collections::HashMap;
use std::sync::Arc;

use super::lexer::{Lexer, Parsed};
use super::*;
use crate::ground::bus::json::Json;
use crate::ground::bus::{Bus, Command, Origin};
use crate::ground::display::{Color, DisplayList, Item, Paint, PathOp, PlacedGlyph, Rect};
use crate::ground::flate::zlib_compress;
use crate::ground::pdfw::{PageResources, PdfWriter};
use crate::ground::picture::jpeg::Image;

// ---------------------------------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------------------------------

fn b(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut v = format!("<< {} /Length {} >>\nstream\n", dict, data.len()).into_bytes();
    v.extend_from_slice(data);
    v.extend_from_slice(b"\nendstream");
    v
}

/// A file with a classic cross-reference: object i+1 is bodies[i].
fn pdf(bodies: &[Vec<u8>], trailer_extra: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offs = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{:010} 00000 n \n", o).as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R {} >>\nstartxref\n{}\n%%EOF\n", bodies.len() + 1, trailer_extra, xref).as_bytes());
    out
}

/// One page (612 x 792) showing `content`, with Helvetica as /F1 (object 5) and `extra` objects from 6.
fn one_page(content: &[u8], res_extra: &str, catalog_extra: &str, extra: Vec<Vec<u8>>, trailer_extra: &str) -> Vec<u8> {
    let mut bodies = vec![
        b(&format!("<< /Type /Catalog /Pages 2 0 R {} >>", catalog_extra)),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>"),
        b(&format!("<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> {} >> /Contents 4 0 R >>", res_extra)),
        stream("", content),
        b("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"),
    ];
    bodies.extend(extra);
    pdf(&bodies, trailer_extra)
}

const HELLO: &[u8] = b"BT /F1 24 Tf 72 700 Td (Hello World) Tj ET";

fn hello() -> Vec<u8> {
    one_page(HELLO, "", "", Vec::new(), "")
}

/// `hello` with its Info dictionary.
fn hello_with_info() -> Vec<u8> {
    one_page(HELLO, "", "", vec![b("<< /Title (First title) /Author (Kiyo) >>")], "/Info 6 0 R")
}

fn open(bytes: &[u8]) -> FolioDoc {
    FolioDoc::open(bytes.to_vec()).unwrap_or_else(|e| panic!("open failed: {}", e))
}

fn has_system_helvetica() -> bool {
    fonts::system_face("Helvetica", 32).map_or(false, |f| f.glyph_id('H').is_some())
}

fn paths(dl: &DisplayList) -> Vec<&Item> {
    dl.items.iter().filter(|i| matches!(i, Item::Path { .. })).collect()
}

fn ascii85_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut buf = [0u8; 4];
        buf[..chunk.len()].copy_from_slice(chunk);
        let v = u32::from_be_bytes(buf);
        if chunk.len() == 4 && v == 0 {
            out.push(b'z');
            continue;
        }
        let mut digits = [0u8; 5];
        let mut x = v;
        for d in digits.iter_mut().rev() {
            *d = (x % 85) as u8 + b'!';
            x /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// LZW with early change, matching the decoder's code widths.
fn lzw_encode(data: &[u8]) -> Vec<u8> {
    let mut dict: HashMap<Vec<u8>, u32> = (0..256u32).map(|i| (vec![i as u8], i)).collect();
    let mut size = 258u32;
    let width = |n: u32| if n >= 2048 { 12 } else if n >= 1024 { 11 } else if n >= 512 { 10 } else { 9 };
    let mut codes: Vec<(u32, u32)> = vec![(256, 9)];
    let mut w: Vec<u8> = Vec::new();
    for &c in data {
        let mut wc = w.clone();
        wc.push(c);
        if dict.contains_key(&wc) {
            w = wc;
        } else {
            codes.push((dict[&w], width(size)));
            if size < 4095 {
                dict.insert(wc, size);
                size += 1;
            }
            w = vec![c];
        }
    }
    if !w.is_empty() {
        codes.push((dict[&w], width(size)));
        size += 1;
    }
    codes.push((257, width(size)));
    let mut out = Vec::new();
    let mut acc: u64 = 0;
    let mut n = 0;
    for (code, bits) in codes {
        acc = (acc << bits) | code as u64;
        n += bits;
        while n >= 8 {
            out.push((acc >> (n - 8)) as u8);
            n -= 8;
        }
    }
    if n > 0 {
        out.push((acc << (8 - n)) as u8);
    }
    out
}

// ---------------------------------------------------------------------------------------------------
// Objects and filters
// ---------------------------------------------------------------------------------------------------

#[test]
fn lexer_reads_every_object_kind() {
    let src = b"<< /A 1 /B -2.5 /C (a\\(b\\)\\n\\101(x)) /D <48 65 6C 6> /E /N#41me /F [1 2 true false null .5] /G 3 0 R /H<</I 4>> >>";
    let mut lx = Lexer::new(src, 0);
    let Parsed::Obj(Obj::Dict(d)) = lx.parse() else { panic!("not a dict") };
    assert_eq!(d.get("A"), Some(&Obj::Int(1)));
    assert_eq!(d.get("B"), Some(&Obj::Real(-2.5)));
    assert_eq!(d.get("C"), Some(&Obj::Str(b"a(b)\nA(x)".to_vec())));
    assert_eq!(d.get("D"), Some(&Obj::Str(vec![0x48, 0x65, 0x6C, 0x60])));
    assert_eq!(d.get("E"), Some(&Obj::name("NAme")));
    let f = d.get("F").and_then(Obj::as_arr).unwrap();
    assert_eq!(f[0], Obj::Int(1));
    assert_eq!(f[1], Obj::Int(2));
    assert_eq!(f[2], Obj::Bool(true));
    assert_eq!(f[3], Obj::Bool(false));
    assert_eq!(f[4], Obj::Null);
    assert_eq!(f[5], Obj::Real(0.5));
    assert_eq!(f.len(), 6);
    assert_eq!(d.get("G"), Some(&Obj::Ref(3, 0)));
    assert_eq!(d.get("H").and_then(Obj::as_dict).and_then(|h| h.get("I")), Some(&Obj::Int(4)));
}

#[test]
fn objects_round_trip_through_the_writer() {
    let o = Obj::Dict(
        Dict::new()
            .with("Name With Space", Obj::name("A/B"))
            .with("S", Obj::Str(b"(paren) \\ back\n".to_vec()))
            .with("Bin", Obj::Str(vec![0, 1, 2, 255]))
            .with("R", Obj::Real(1.25))
            .with("Arr", Obj::Arr(vec![Obj::Int(-3), Obj::Ref(7, 1), Obj::Null])),
    );
    let bytes = obj::to_bytes(&o);
    let mut lx = Lexer::new(&bytes, 0);
    let Parsed::Obj(back) = lx.parse() else { panic!() };
    assert_eq!(back, o);
}

#[test]
fn text_strings_decode() {
    assert_eq!(decode_text(b"Plain"), "Plain");
    assert_eq!(decode_text(&[0xFE, 0xFF, 0x00, 0x48, 0x00, 0xE9, 0xD8, 0x3D, 0xDE, 0x00]), "H\u{e9}\u{1F600}");
    assert_eq!(decode_text(&[0x93, 0x84]), "\u{FB01}\u{2014}");
    assert_eq!(obj::encode_text("ascii"), b"ascii".to_vec());
    assert_eq!(decode_text(&obj::encode_text("caf\u{e9}")), "caf\u{e9}");
}

#[test]
fn filters_decode() {
    // The LZW example from the PDF reference.
    assert_eq!(filters::lzw(&[0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01], true), b"-----A---B".to_vec());
    let long: Vec<u8> = (0..6000u32).map(|i| b"the quick brown fox "[(i % 20) as usize] ^ ((i / 97) as u8 & 3)).collect();
    assert_eq!(filters::lzw(&lzw_encode(&long), true), long);
    assert_eq!(filters::ascii85(&ascii85_encode(b"Hello, Folio!\0\0\0\0x")), b"Hello, Folio!\0\0\0\0x".to_vec());
    assert_eq!(filters::ascii85(b"<~87cURD]i,\"Ebo80~>"), b"Hello World!".to_vec());
    assert_eq!(filters::ascii_hex(b"48 65 6c6C 6>"), b"Hell`".to_vec());
    assert_eq!(filters::run_length(&[2, b'a', b'b', b'c', 254, b'x', 128, b'z']), b"abcxxx".to_vec());
    // A Flate stream with a PNG predictor (Up and Sub rows).
    let rows: [[u8; 3]; 3] = [[10, 20, 30], [11, 22, 33], [5, 5, 5]];
    let mut enc = Vec::new();
    enc.extend_from_slice(&[0, 10, 20, 30]);
    enc.extend_from_slice(&[2, 1, 2, 3]);
    enc.extend_from_slice(&[1, 5, 0, 0]);
    let parms = Dict::new().with("Predictor", Obj::Int(12)).with("Columns", Obj::Int(3));
    let out = filters::decode(&zlib_compress(&enc, 6), &[("FlateDecode".into(), parms)]).unwrap();
    assert_eq!(out.data, rows.concat());
    // TIFF predictor 2.
    let parms = Dict::new().with("Predictor", Obj::Int(2)).with("Columns", Obj::Int(3)).with("Colors", Obj::Int(1));
    assert_eq!(filters::predict(&[10, 1, 1, 5, 5, 5], &parms), vec![10, 11, 12, 5, 10, 15]);
    // An image codec stops the chain.
    let d = filters::decode(b"rawjpeg", &[("DCTDecode".into(), Dict::new())]).unwrap();
    assert_eq!(d.image_codec.map(|c| c.0), Some("DCTDecode".to_string()));
}

// ---------------------------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------------------------

#[test]
fn page_tree_inheritance() {
    let bytes = pdf(
        &[
            b("<< /Type /Catalog /Pages 2 0 R >>"),
            b("<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 3 /MediaBox [0 0 600 800] /Resources << /Font << /F1 7 0 R >> >> >>"),
            b("<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 /Rotate 90 >>"),
            b("<< /Type /Page /Parent 3 0 R /Contents 6 0 R >>"),
            b("<< /Type /Pages /Parent 2 0 R /Kids [8 0 R 9 0 R] /Count 2 /CropBox [10 10 310 410] >>"),
            stream("", b"BT /F1 10 Tf 20 20 Td (Inherited) Tj ET"),
            b("<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>"),
            b("<< /Type /Page /Parent 5 0 R /Contents 6 0 R >>"),
            b("<< /Type /Page /Parent 5 0 R /MediaBox [0 0 200 300] /Rotate -90 /Contents 6 0 R >>"),
        ],
        "",
    );
    let d = open(&bytes);
    assert_eq!(d.page_count(), 3);
    assert_eq!(d.page_size(0), Some((800.0, 600.0)));
    assert_eq!(d.page_info(0).unwrap().rotate, 90);
    assert_eq!(d.page_size(1), Some((300.0, 400.0)));
    // The inherited crop box is clipped to the page's own media box; -90 is 270.
    assert_eq!(d.page_info(2).unwrap().crop, [10.0, 10.0, 200.0, 300.0]);
    assert_eq!(d.page_info(2).unwrap().rotate, 270);
    // Resources are inherited from the root: the text is there on every page.
    for p in 0..3 {
        assert_eq!(d.page_text(p), "Inherited", "page {}", p);
    }
}

#[test]
fn xref_stream_with_object_stream_and_png_predictor() {
    let content = b"BT /F1 18 Tf 50 750 Td (Compressed objects) Tj ET";
    let objs: [(u32, &str); 4] = [
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"),
        (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ];
    let mut header = String::new();
    let mut body = String::new();
    for (n, o) in objs.iter() {
        header.push_str(&format!("{} {} ", n, body.len()));
        body.push_str(o);
        body.push('\n');
    }
    let first = header.len();
    let objstm = zlib_compress(format!("{}{}", header, body).as_bytes(), 6);

    let mut out = b"%PDF-1.5\n".to_vec();
    let off4 = out.len();
    out.extend_from_slice(b"4 0 obj\n");
    out.extend_from_slice(&stream("", content));
    out.extend_from_slice(b"\nendobj\n");
    let off6 = out.len();
    out.extend_from_slice(b"6 0 obj\n");
    out.extend_from_slice(&stream(&format!("/Type /ObjStm /N 4 /First {} /Filter /FlateDecode", first), &objstm));
    out.extend_from_slice(b"\nendobj\n");
    let off7 = out.len();
    let rows: Vec<[u8; 4]> = vec![
        [0, 0, 0, 0],
        [2, 0, 6, 0],
        [2, 0, 6, 1],
        [2, 0, 6, 2],
        [1, (off4 >> 8) as u8, off4 as u8, 0],
        [2, 0, 6, 3],
        [1, (off6 >> 8) as u8, off6 as u8, 0],
        [1, (off7 >> 8) as u8, off7 as u8, 0],
    ];
    // PNG "Up" rows.
    let mut pred = Vec::new();
    let mut prev = [0u8; 4];
    for r in &rows {
        pred.push(2);
        for k in 0..4 {
            pred.push(r[k].wrapping_sub(prev[k]));
        }
        prev = *r;
    }
    let xdata = zlib_compress(&pred, 6);
    out.extend_from_slice(b"7 0 obj\n");
    out.extend_from_slice(&stream("/Type /XRef /Size 8 /W [1 2 1] /Root 1 0 R /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 4 >>", &xdata));
    out.extend_from_slice(format!("\nendobj\nstartxref\n{}\n%%EOF\n", off7).as_bytes());

    let d = open(&out);
    assert!(!d.was_repaired());
    assert_eq!(d.page_count(), 1);
    assert_eq!(d.page_size(0), Some((595.0, 842.0)));
    assert_eq!(d.page_text(0), "Compressed objects");
    assert_eq!(d.version(), "1.5");
    // Saving an edit on an xref-stream file writes an xref stream too, and reopens.
    let mut d = d;
    d.set_title(Some("Streams"));
    let saved = d.save();
    assert!(saved.starts_with(&out));
    let again = open(&saved);
    assert_eq!(again.title().as_deref(), Some("Streams"));
    assert_eq!(again.page_text(0), "Compressed objects");
}

#[test]
fn broken_xref_is_repaired() {
    let good = hello_with_info();
    // Every offset in the table wrong, and startxref pointing nowhere useful.
    let s = String::from_utf8_lossy(&good).into_owned();
    let xref_at = s.find("xref\n").unwrap();
    let (head, tail) = s.split_at(xref_at);
    let mut tail: String = tail
        .lines()
        .map(|l| if l.len() >= 18 && l.ends_with(" n ") { format!("{:010} 00000 n ", 3) } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n");
    let sx = tail.rfind("startxref\n").unwrap();
    tail.replace_range(sx.., "startxref\n12\n%%EOF\n");
    let mut broken = head.as_bytes().to_vec();
    broken.extend_from_slice(tail.as_bytes());
    let d = open(&broken);
    assert!(d.was_repaired());
    assert_eq!(d.page_count(), 1);
    assert_eq!(d.page_text(0), "Hello World");
    assert_eq!(d.title().as_deref(), Some("First title"));

    // No cross-reference or trailer at all: the catalog is found by scanning.
    let cut = s.find("xref\n").unwrap();
    let bare = good[..cut].to_vec();
    let d = open(&bare);
    assert!(d.was_repaired());
    assert_eq!(d.page_text(0), "Hello World");

    // A wrong stream /Length is survived (the data runs to endstream).
    let wrong = String::from_utf8_lossy(&good).replace(&format!("/Length {}", HELLO.len()), "/Length 9999").into_bytes();
    let d = open(&wrong);
    assert_eq!(d.page_text(0), "Hello World");

    // A repaired file saves with a complete cross-reference and reopens.
    let mut d = open(&broken);
    d.set_title(Some("Fixed"));
    let saved = d.save();
    assert!(saved.starts_with(&broken));
    let again = open(&saved);
    assert_eq!(again.title().as_deref(), Some("Fixed"));
    assert_eq!(again.page_text(0), "Hello World");
}

#[test]
fn incremental_update_newest_wins() {
    let base = one_page(b"BT /F1 12 Tf 72 700 Td (Old text) Tj ET", "", "", vec![b("<< /Title (Old) >>")], "/Info 6 0 R");
    let prev = {
        let s = String::from_utf8_lossy(&base).into_owned();
        let p = s.rfind("startxref\n").unwrap();
        s[p + 10..].lines().next().unwrap().trim().parse::<usize>().unwrap()
    };
    let mut f = base.clone();
    let off4 = f.len();
    f.extend_from_slice(b"4 0 obj\n");
    f.extend_from_slice(&stream("", b"BT /F1 12 Tf 72 700 Td (New text) Tj ET"));
    f.extend_from_slice(b"\nendobj\n");
    let off6 = f.len();
    f.extend_from_slice(b"6 0 obj\n<< /Title (Updated) >>\nendobj\n");
    let x = f.len();
    f.extend_from_slice(format!("xref\n4 1\n{:010} 00000 n \n6 1\n{:010} 00000 n \ntrailer\n<< /Size 7 /Root 1 0 R /Info 6 0 R /Prev {} >>\nstartxref\n{}\n%%EOF\n", off4, off6, prev, x).as_bytes());
    let d = open(&f);
    assert!(!d.was_repaired());
    assert_eq!(d.page_text(0), "New text");
    assert_eq!(d.title().as_deref(), Some("Updated"));
}

#[test]
fn encrypted_files_are_refused_clearly() {
    let bytes = one_page(
        b"BT /F1 12 Tf 72 700 Td (Secret) Tj ET",
        "",
        "",
        vec![b("<< /Filter /Standard /V 1 /R 2 /O <00> /U <00> /P -4 >>")],
        "/Encrypt 6 0 R /ID [<01><01>]",
    );
    let err = FolioDoc::open(bytes).unwrap_err();
    assert_eq!(err, FolioError::Encrypted);
    assert!(err.to_string().contains("password-protected"));
    assert!(err.to_string().contains("arrives later"));
    assert_eq!(FolioDoc::open(b"just some text".to_vec()).unwrap_err(), FolioError::NotPdf);
}

// ---------------------------------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------------------------------

#[test]
fn standard_14_helvetica_text_with_afm_advances() {
    let d = open(&hello());
    assert_eq!(d.page_text(0), "Hello World");
    let g = d.page_glyphs(0);
    assert_eq!(g.len(), 11);
    // y down: baseline 700 from the bottom of a 792 page.
    assert!((g[0].y - 92.0).abs() < 1e-9);
    assert!((g[0].x - 72.0).abs() < 1e-9);
    // H = 722, e = 556 (Helvetica AFM).
    assert!((g[1].x - (72.0 + 0.722 * 24.0)).abs() < 1e-6, "{}", g[1].x);
    assert!((g[2].x - (72.0 + (0.722 + 0.556) * 24.0)).abs() < 1e-6);
    if has_system_helvetica() {
        let dl = d.display(0);
        let runs: Vec<_> = dl.items.iter().filter_map(|i| if let Item::Glyphs { font, size, glyphs, .. } = i { Some((*font, *size, glyphs.clone())) } else { None }).collect();
        assert_eq!(runs.len(), 1);
        let (font, size, glyphs) = &runs[0];
        assert!((size - 24.0).abs() < 1e-9);
        assert_eq!(glyphs.len(), 11);
        let res = d.resources().unwrap();
        let fr = res.font(*font).unwrap();
        assert!(fr.units_per_em >= 16.0);
        // Every glyph's outline is registered; only the space's is empty.
        assert!(glyphs.iter().all(|g| res.glyph(*font, g.gid).is_some()));
        assert_eq!(glyphs.iter().filter(|g| res.glyph(*font, g.gid).map_or(false, |o| !o.is_empty())).count(), 10);
        assert!((glyphs[1].x - (72.0 + 0.722 * 24.0)).abs() < 1e-6 && (glyphs[1].y - 92.0).abs() < 1e-9);
    }
}

#[test]
fn tj_kerning_and_word_spacing_become_words() {
    let d = open(&one_page(b"BT /F1 12 Tf 72 700 Td [(Hel) -20 (lo) -280 (World)] TJ 0 -20 Td 3 Tw (two words) Tj ET", "", "", Vec::new(), ""));
    assert_eq!(d.page_text(0), "Hello World\ntwo words");
}

#[test]
fn paths_colours_and_clips() {
    let content = b"q 1 0 0 rg 10 10 100 50 re f Q 0 0 1 RG 2 w 0 0 m 100 100 l S 0.5 g 0 0 1 0 k 200 200 50 50 re B q 300 300 10 10 re W n 0 1 0 rg 0 0 612 792 re f Q";
    let d = open(&one_page(content, "", "", Vec::new(), ""));
    let dl = d.display(0);
    let ps = paths(&dl);
    assert_eq!(ps.len(), 4);
    match ps[0] {
        Item::Path { ops, fill: Some(Paint::Solid(c)), stroke: None } => {
            assert_eq!(*c, Color::rgb(255, 0, 0));
            assert_eq!(ops[0], PathOp::MoveTo(10.0, 782.0));
            assert_eq!(ops[2], PathOp::LineTo(110.0, 732.0));
        }
        other => panic!("{:?}", other),
    }
    match ps[1] {
        Item::Path { fill: None, stroke: Some(s), .. } => {
            assert_eq!(s.color, Color::rgb(0, 0, 255));
            assert!((s.width - 2.0).abs() < 1e-9);
        }
        other => panic!("{:?}", other),
    }
    // CMYK (0 0 1 0) is yellow.
    match ps[2] {
        Item::Path { fill: Some(Paint::Solid(c)), stroke: Some(_), .. } => assert_eq!(*c, Color::rgb(255, 255, 0)),
        other => panic!("{:?}", other),
    }
    // The clip is pushed before the green fill and popped at Q.
    let clip = dl.items.iter().position(|i| matches!(i, Item::PushClip(_))).unwrap();
    let pop = dl.items.iter().position(|i| matches!(i, Item::PopClip)).unwrap();
    assert!(clip < pop);
    assert!(matches!(dl.items[pop - 1], Item::Path { .. }));
}

#[test]
fn rotation_is_applied() {
    let bytes = pdf(
        &[
            b("<< /Type /Catalog /Pages 2 0 R >>"),
            b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            b("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate 90 /Contents 4 0 R >>"),
            stream("", b"0 0 10 10 re f"),
        ],
        "",
    );
    let d = open(&bytes);
    let dl = d.display(0);
    assert_eq!((dl.width, dl.height), (792.0, 612.0));
    // The page's bottom-left corner shows at the top-left after a clockwise quarter turn.
    let Item::Path { ops, .. } = paths(&dl)[0] else { panic!() };
    let r = crate::ground::display::path_bounds(ops).unwrap();
    assert!(r.x.abs() < 1e-9 && r.y.abs() < 1e-9 && (r.w - 10.0).abs() < 1e-9 && (r.h - 10.0).abs() < 1e-9, "{:?}", r);
}

#[test]
fn images_inline_xobject_smask_and_stencil() {
    let rgb = [255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
    let content = b"q 100 0 0 50 10 20 cm /Im1 Do Q q 20 0 0 20 200 200 cm BI /W 2 /H 1 /CS /G /BPC 8 ID \x00\xff EI Q q 0 0 1 rg 10 0 0 10 300 300 cm /M1 Do Q";
    let bytes = one_page(
        content,
        "/XObject << /Im1 6 0 R /M1 8 0 R >>",
        "",
        vec![
            stream("/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 7 0 R", &rgb),
            stream("/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8", &[255, 128, 0, 255]),
            stream("/Type /XObject /Subtype /Image /Width 8 /Height 1 /ImageMask true", &[0b1010_0000]),
        ],
        "",
    );
    let d = open(&bytes);
    let dl = d.display(0);
    let imgs: Vec<u32> = dl.items.iter().filter_map(|i| if let Item::Image { image, .. } = i { Some(*image) } else { None }).collect();
    assert_eq!(imgs.len(), 3);
    let res = d.resources().unwrap();
    let im1 = res.image(imgs[0]).unwrap();
    assert_eq!((im1.width, im1.height), (2, 2));
    assert_eq!(&im1.rgba[..8], &[255, 0, 0, 255, 0, 255, 0, 128]);
    assert_eq!(im1.rgba[11], 0);
    let inline = res.image(imgs[1]).unwrap();
    assert_eq!(inline.rgba, vec![0, 0, 0, 255, 255, 255, 255, 255]);
    let stencil = res.image(imgs[2]).unwrap();
    // 1 bits are left unpainted; 0 bits take the fill colour (blue).
    assert_eq!(&stencil.rgba[..8], &[0, 0, 255, 0, 0, 0, 255, 255]);
    // The image's unit square is mapped by the CTM (and flipped to y down).
    let t = dl.items.iter().find_map(|i| if let Item::PushTransform(m) = i { Some(*m) } else { None }).unwrap();
    let (x0, y0) = t.apply(0.0, 0.0);
    let (x1, y1) = t.apply(1.0, 1.0);
    assert!((x0 - 10.0).abs() < 1e-9 && (y0 - 722.0).abs() < 1e-9, "{} {}", x0, y0);
    assert!((x1 - 110.0).abs() < 1e-9 && (y1 - 772.0).abs() < 1e-9);
}

#[test]
fn forms_extgstate_alpha_and_shading() {
    let content = b"/GS1 gs /Fm1 Do /Sh1 sh";
    let bytes = one_page(
        content,
        "/XObject << /Fm1 6 0 R >> /ExtGState << /GS1 << /ca 0.5 /CA 0.25 >> >> /Shading << /Sh1 7 0 R >>",
        "",
        vec![
            stream("/Type /XObject /Subtype /Form /BBox [0 0 50 50] /Matrix [1 0 0 1 100 100] /Resources << /Font << /F2 8 0 R >> >>", b"1 0 0 rg 0 0 50 50 re f BT /F2 10 Tf 5 5 Td (In a form) Tj ET"),
            b("<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 100 0] /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>"),
            b("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>"),
        ],
        "",
    );
    let d = open(&bytes);
    let dl = d.display(0);
    assert_eq!(d.page_text(0), "In a form");
    let ps = paths(&dl);
    match ps[0] {
        Item::Path { fill: Some(Paint::Solid(c)), ops, .. } => {
            assert_eq!((c.r, c.g, c.b, c.a), (255, 0, 0, 128));
            assert_eq!(ops[0], PathOp::MoveTo(100.0, 692.0));
        }
        other => panic!("{:?}", other),
    }
    match ps.last().unwrap() {
        Item::Path { fill: Some(Paint::Linear { x0, x1, stops, .. }), .. } => {
            assert_eq!((*x0, *x1), (0.0, 100.0));
            assert_eq!(stops.first().unwrap().color, Color::rgb(255, 0, 0));
            assert_eq!(stops.last().unwrap().color, Color::rgb(0, 0, 255));
        }
        other => panic!("{:?}", other),
    }
    // The form's bbox clip is balanced.
    let pushes = dl.items.iter().filter(|i| matches!(i, Item::PushClip(_))).count();
    let pops = dl.items.iter().filter(|i| matches!(i, Item::PopClip)).count();
    assert_eq!(pushes, pops);
    // Courier advances are 600.
    let g = d.page_glyphs(0);
    assert!((g[1].x - g[0].x - 6.0).abs() < 1e-9);
}

#[test]
fn type3_glyphs_draw_their_procedures() {
    let bytes = one_page(
        b"BT /T3 12 Tf 100 100 Td (ab) Tj ET",
        "/Font << /F1 5 0 R /T3 6 0 R >>",
        "",
        vec![
            b("<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /sq 7 0 R >> /Encoding << /Type /Encoding /Differences [97 /sq /sq] >> /FirstChar 97 /LastChar 98 /Widths [1000 1000] >>"),
            stream("", b"1000 0 d0 0 0 800 800 re f"),
        ],
        "",
    );
    // The page's own /Font dict replaces the builder's: keep F1 too.
    let d = open(&bytes);
    let dl = d.display(0);
    let ps = paths(&dl);
    assert_eq!(ps.len(), 2);
    let Item::Path { ops, .. } = ps[1] else { panic!() };
    let r = crate::ground::display::path_bounds(ops).unwrap();
    // Second glyph starts one em (12 pt) later; 800 units = 9.6 pt.
    assert!((r.x - 112.0).abs() < 1e-6 && (r.w - 9.6).abs() < 1e-6, "{:?}", r);
}

#[test]
fn content_streams_through_ascii85_and_lzw() {
    let text = b"BT /F1 12 Tf 72 700 Td (Through filters) Tj ET";
    let a85 = ascii85_encode(text);
    let lzw = lzw_encode(b"BT /F1 12 Tf 72 680 Td (and LZW too) Tj ET");
    let bytes = pdf(
        &[
            b("<< /Type /Catalog /Pages 2 0 R >>"),
            b("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>"),
            b("<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents [4 0 R 6 0 R] >>"),
            stream("/Filter /ASCII85Decode", &a85),
            b("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
            stream("/Filter [/LZWDecode]", &lzw),
        ],
        "",
    );
    let d = open(&bytes);
    assert_eq!(d.page_text(0), "Through filters\nand LZW too");
}

#[test]
fn two_columns_read_column_by_column() {
    // Stream order interleaves the columns; reading order must not.
    let content = b"BT /F1 10 Tf 72 700 Td (Left one) Tj 250 0 Td (Right one) Tj -250 -14 Td (Left two) Tj 250 0 Td (Right two) Tj -250 -14 Td (Left three) Tj 250 0 Td (Right three) Tj ET \
                    BT /F1 16 Tf 72 740 Td (A heading across both) Tj ET";
    let d = open(&one_page(content, "", "", Vec::new(), ""));
    assert_eq!(d.page_text(0), "A heading across both\n\nLeft one\nLeft two\nLeft three\n\nRight one\nRight two\nRight three");
}

#[test]
fn outlines_with_explicit_named_and_action_destinations() {
    let bytes = one_page(
        b"BT /F1 12 Tf 72 700 Td (Outlined) Tj ET",
        "",
        "/Outlines 6 0 R /Names << /Dests << /Names [(intro) [3 0 R /Fit]] >> >>",
        vec![
            b("<< /Type /Outlines /First 7 0 R /Last 9 0 R /Count 3 >>"),
            b("<< /Title (Chapter One) /Parent 6 0 R /Next 9 0 R /Dest [3 0 R /XYZ 0 792 0] /First 8 0 R /Last 8 0 R /Count 1 >>"),
            b("<< /Title (Section 1.1) /Parent 7 0 R /Dest (intro) >>"),
            b("<< /Title <FEFF004300680061007000740065007200200032> /Parent 6 0 R /Prev 7 0 R /A << /S /GoTo /D [3 0 R /Fit] >> >>"),
        ],
        "",
    );
    let d = open(&bytes);
    let o = d.outlines();
    assert_eq!(o.len(), 3);
    assert_eq!((o[0].title.as_str(), o[0].page, o[0].level), ("Chapter One", Some(0), 0));
    assert_eq!((o[1].title.as_str(), o[1].page, o[1].level), ("Section 1.1", Some(0), 1));
    assert_eq!((o[2].title.as_str(), o[2].page), ("Chapter 2", Some(0)));
}

#[test]
fn annotation_appearances_are_drawn_and_links_kept() {
    let bytes = pdf(
        &[
            b("<< /Type /Catalog /Pages 2 0 R >>"),
            b("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>"),
            b("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Annots [5 0 R 7 0 R 8 0 R] >>"),
            stream("", b""),
            b("<< /Type /Annot /Subtype /Square /Rect [100 100 200 150] /Contents (A box) /AP << /N 6 0 R >> >>"),
            stream("/Type /XObject /Subtype /Form /BBox [0 0 10 5]", b"0 0 1 rg 0 0 10 5 re f"),
            b("<< /Type /Annot /Subtype /Link /Rect [10 10 60 30] /A << /S /URI /URI (https://example.org/) >> >>"),
            b("<< /Type /Annot /Subtype /Square /Rect [0 0 5 5] /F 2 /AP << /N 6 0 R >> >>"),
        ],
        "",
    );
    let d = open(&bytes);
    let annots = d.annotations(0);
    assert_eq!(annots.len(), 3);
    assert_eq!(annots[0].subtype, "Square");
    assert_eq!(annots[0].contents, "A box");
    assert!(annots[0].has_appearance);
    let dl = d.display(0);
    let ps = paths(&dl);
    // Only the visible square is drawn (the second is hidden), fitted to its rect.
    assert_eq!(ps.len(), 1);
    let Item::Path { ops, .. } = ps[0] else { panic!() };
    let r = crate::ground::display::path_bounds(ops).unwrap();
    assert!((r.x - 100.0).abs() < 1e-6 && (r.y - 642.0).abs() < 1e-6 && (r.w - 100.0).abs() < 1e-6 && (r.h - 50.0).abs() < 1e-6, "{:?}", r);
    let link = dl.items.iter().find_map(|i| if let Item::Link { rect, target } = i { Some((*rect, target.clone())) } else { None }).unwrap();
    assert_eq!(link.1, "https://example.org/");
    assert_eq!(link.0, Rect::new(10.0, 762.0, 50.0, 20.0));
}

// ---------------------------------------------------------------------------------------------------
// Files from our own writer
// ---------------------------------------------------------------------------------------------------

#[test]
fn opens_what_pdfw_writes() {
    let mut dl = DisplayList::new(400.0, 300.0);
    dl.push(Item::Path {
        ops: vec![PathOp::MoveTo(20.0, 20.0), PathOp::LineTo(120.0, 20.0), PathOp::LineTo(120.0, 70.0), PathOp::LineTo(20.0, 70.0), PathOp::Close],
        fill: Some(Paint::Solid(Color::rgb(0, 128, 255))),
        stroke: None,
    });
    dl.push(Item::Image { image: 9, rect: Rect::new(200.0, 100.0, 40.0, 40.0) });
    let mut res = PageResources::new();
    let mut pixels = Vec::new();
    for i in 0..16u8 {
        pixels.extend_from_slice(&[i * 16, 255 - i * 16, 7, 255]);
    }
    res.add_rgba(9, Image { width: 4, height: 4, pixels: pixels.clone() });
    let face = fonts::system_face("Arial", 32).filter(|f| f.has_truetype_outlines() && f.glyph_id('H').is_some());
    let words = "Hello PDF";
    if let Some(face) = &face {
        let mut x = 30.0;
        let mut glyphs = Vec::new();
        for ch in words.chars() {
            let gid = face.glyph_id(ch).unwrap_or(0);
            glyphs.push(PlacedGlyph { gid, x, y: 200.0 });
            x += face.advance(gid) as f64 * 20.0 / face.units_per_em() as f64;
        }
        dl.push(Item::Glyphs { font: 1, size: 20.0, color: Color::BLACK, glyphs });
        res.add_font(1, Arc::clone(face));
    }
    let mut w = PdfWriter::new();
    w.set_info("Made by pdfw", "Folio tests", "", "", "", "");
    w.set_date(1_700_000_000);
    w.add_page(&dl, &res);
    w.add_page(&DisplayList::new(200.0, 200.0), &PageResources::new());
    w.add_outline("First page", 0, 0.0);
    let bytes = w.finish();

    let d = open(&bytes);
    assert_eq!(d.page_count(), 2);
    assert_eq!(d.page_size(0), Some((400.0, 300.0)));
    assert_eq!(d.title().as_deref(), Some("Made by pdfw"));
    assert_eq!(d.outlines()[0].title, "First page");
    assert_eq!(d.outlines()[0].page, Some(0));
    let out = d.display(0);
    // The blue rectangle comes back where it was.
    let blue = paths(&out).into_iter().find_map(|p| match p {
        Item::Path { ops, fill: Some(Paint::Solid(c)), .. } if *c == Color::rgb(0, 128, 255) => crate::ground::display::path_bounds(ops),
        _ => None,
    });
    let r = blue.expect("the rectangle");
    assert!((r.x - 20.0).abs() < 0.01 && (r.y - 20.0).abs() < 0.01 && (r.w - 100.0).abs() < 0.01 && (r.h - 50.0).abs() < 0.01, "{:?}", r);
    // The picture's pixels survive.
    let img = out.items.iter().find_map(|i| if let Item::Image { image, .. } = i { Some(*image) } else { None }).expect("an image");
    let ir = d.resources().unwrap().image(img).unwrap();
    assert_eq!((ir.width, ir.height), (4, 4));
    assert_eq!(ir.rgba, pixels);
    if face.is_some() {
        // Type0 / CIDFontType2 / Identity-H with an embedded subset and a ToUnicode map.
        assert_eq!(d.page_text(0), words);
        let runs: Vec<&Item> = out.items.iter().filter(|i| matches!(i, Item::Glyphs { .. })).collect();
        assert!(!runs.is_empty());
        if let Item::Glyphs { font, glyphs, .. } = runs[0] {
            let g0 = glyphs[0];
            assert!((g0.x - 30.0).abs() < 0.05 && (g0.y - 200.0).abs() < 0.05, "{:?}", g0);
            assert!(d.resources().unwrap().glyph(*font, g0.gid).map_or(false, |o| !o.is_empty()));
        }
    }
    assert!(d.describe().get("has_text").and_then(Json::as_bool) == Some(face.is_some()));
}

// ---------------------------------------------------------------------------------------------------
// Edits and saving
// ---------------------------------------------------------------------------------------------------

#[test]
fn unchanged_save_is_the_original_bytes() {
    let bytes = hello_with_info();
    let d = open(&bytes);
    assert!(!d.is_modified());
    assert_eq!(d.save(), bytes);
}

#[test]
fn set_title_is_an_incremental_update() {
    let bytes = hello_with_info();
    let mut bus = Bus::new(open(&bytes));
    bus.do_one(Command::new("folio.set_title", Json::obj().with("title", "Kiyo's report \u{2014} draft")), Origin::Ai).unwrap();
    let saved = bus.doc().save();
    assert!(saved.len() > bytes.len());
    assert_eq!(&saved[..bytes.len()], &bytes[..], "the original bytes are an exact prefix");
    let again = open(&saved);
    assert_eq!(again.title().as_deref(), Some("Kiyo's report \u{2014} draft"));
    assert_eq!(again.author().as_deref(), Some("Kiyo"), "the rest of the info dictionary is kept");
    assert_eq!(again.page_text(0), "Hello World");
    // Undo restores the old title and nothing is left to save.
    bus.undo().unwrap();
    assert_eq!(bus.doc().title().as_deref(), Some("First title"));
    assert_eq!(bus.doc().save(), bytes);
    // A file without an info dictionary gets one, and loses it again on undo.
    let plain = one_page(b"BT /F1 12 Tf 72 700 Td (x) Tj ET", "", "", Vec::new(), "");
    let mut bus = Bus::new(open(&plain));
    bus.do_one(Command::new("folio.set_title", Json::obj().with("title", "New")), Origin::User).unwrap();
    let again = open(&bus.doc().save());
    assert_eq!(again.title().as_deref(), Some("New"));
    bus.undo().unwrap();
    assert_eq!(bus.doc().save(), plain);
}

#[test]
fn add_note_round_trip() {
    let bytes = hello_with_info();
    let mut bus = Bus::new(open(&bytes));
    let before = bus.doc().display(0).items.len();
    let done = bus.do_one(Command::new("folio.add_note", Json::obj().with("page", 0).with("x", 300).with("y", 50).with("text", "Check this")), Origin::Ai).unwrap();
    assert_eq!(done.pages_touched, vec![0]);
    let annots = bus.doc().annotations(0);
    assert_eq!(annots.len(), 1);
    assert_eq!(annots[0].subtype, "Text");
    assert_eq!(annots[0].contents, "Check this");
    // Display (300, 50) from the top is PDF (300, 742) at the icon's top-left.
    assert_eq!(annots[0].rect, [300.0, 722.0, 320.0, 742.0]);
    // Its appearance is drawn.
    let after = bus.doc().display(0);
    assert!(after.items.len() > before);
    assert!(paths(&after).iter().any(|p| matches!(p, Item::Path { fill: Some(Paint::Solid(c)), .. } if c.r == 255 && c.g == 217)));
    let saved = bus.doc().save();
    assert_eq!(&saved[..bytes.len()], &bytes[..]);
    let again = open(&saved);
    let a = again.annotations(0);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].contents, "Check this");
    assert!(a[0].has_appearance);
    // The door reads it.
    let r = bus.door("read", &Json::obj().with("annotations", 0)).unwrap();
    let note = r.get("annotations").and_then(|l| l.at(0)).and_then(|n| n.get("note")).and_then(Json::as_usize).unwrap();
    // Undo removes it completely: the file is the original again.
    bus.undo().unwrap();
    assert!(bus.doc().annotations(0).is_empty());
    assert_eq!(bus.doc().save(), bytes);
    // Redo, then remove it by command, then undo that.
    bus.redo().unwrap();
    assert_eq!(bus.doc().annotations(0).len(), 1);
    bus.do_one(Command::new("folio.remove_note", Json::obj().with("page", 0).with("note", note)), Origin::User).unwrap();
    assert!(bus.doc().annotations(0).is_empty());
    bus.undo().unwrap();
    let back = bus.doc().annotations(0);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].contents, "Check this");
    assert_eq!(back[0].rect, [300.0, 722.0, 320.0, 742.0]);
    // A page that does not exist is refused, and nothing changes.
    assert!(bus.door("do", &Json::parse(r#"{"commands":[{"name":"folio.add_note","args":{"page":5,"x":1,"y":1,"text":"no"}}]}"#).unwrap()).is_err());
}

#[test]
fn notes_on_an_existing_annots_array_object() {
    let bytes = pdf(
        &[
            b("<< /Type /Catalog /Pages 2 0 R >>"),
            b("<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>"),
            b("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Annots 5 0 R >>"),
            stream("", b""),
            b("[6 0 R]"),
            b("<< /Type /Annot /Subtype /Text /Rect [10 10 30 30] /Contents (Old note) >>"),
        ],
        "",
    );
    let mut bus = Bus::new(open(&bytes));
    bus.do_one(Command::new("folio.add_note", Json::obj().with("page", 0).with("x", 10).with("y", 10).with("text", "New")), Origin::User).unwrap();
    assert_eq!(bus.doc().annotations(0).len(), 2);
    let again = open(&bus.doc().save());
    let a = again.annotations(0);
    assert_eq!(a.iter().map(|x| x.contents.as_str()).collect::<Vec<_>>(), vec!["Old note", "New"]);
    // Removing the file's own note and undoing puts it back in place.
    bus.do_one(Command::new("folio.remove_note", Json::obj().with("page", 0).with("note", 6)), Origin::User).unwrap();
    assert_eq!(bus.doc().annotations(0).len(), 1);
    bus.undo().unwrap();
    assert_eq!(bus.doc().annotations(0)[0].contents, "Old note");
    bus.undo().unwrap();
    assert_eq!(bus.doc().save(), bytes);
}

// ---------------------------------------------------------------------------------------------------
// The door
// ---------------------------------------------------------------------------------------------------

#[test]
fn door_describe_read_search_shot() {
    let bytes = hello_with_info();
    let mut bus = Bus::new(open(&bytes));
    let d = bus.door("describe", &Json::obj()).unwrap();
    assert_eq!(d.get("kind").and_then(Json::as_str), Some("folio"));
    let doc = d.get("document").unwrap();
    assert_eq!(doc.get("page_count").and_then(Json::as_usize), Some(1));
    assert_eq!(doc.get("title").and_then(Json::as_str), Some("First title"));
    assert_eq!(doc.get("author").and_then(Json::as_str), Some("Kiyo"));
    assert_eq!(doc.get("has_text").and_then(Json::as_bool), Some(true));
    assert_eq!(doc.get("seems_scanned").and_then(Json::as_bool), Some(false));
    let p0 = doc.get("pages").and_then(|p| p.at(0)).unwrap();
    assert_eq!(p0.get("width").and_then(Json::as_f64), Some(612.0));
    let r = bus.door("read", &Json::obj().with("page", 0)).unwrap();
    assert_eq!(r.get("text").and_then(Json::as_str), Some("Hello World"));
    let s = bus.door("read", &Json::obj().with("search", "world")).unwrap();
    assert_eq!(s.get("count").and_then(Json::as_usize), Some(1));
    assert_eq!(s.get("matches").and_then(|m| m.at(0)).and_then(|m| m.get("page")).and_then(Json::as_usize), Some(0));
    assert!(bus.door("read", &Json::obj().with("page", 3)).is_err());
    let shot = bus.door("shot", &Json::obj().with("page", 0)).unwrap();
    assert_eq!(shot.get("width").and_then(Json::as_f64), Some(612.0));
    let t = bus.doc().thumbnail(0, 153.0);
    assert_eq!((t.width, t.height), (153.0, 198.0));
    assert!(matches!(t.items.first(), Some(Item::PushTransform(m)) if (m.a - 0.25).abs() < 1e-12));
}

#[test]
fn scanned_pages_are_recognised() {
    let bytes = one_page(b"q 612 0 0 792 0 0 cm /Im1 Do Q", "/XObject << /Im1 6 0 R >>", "", vec![stream("/Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[200])], "");
    let d = open(&bytes);
    let desc = d.describe();
    assert_eq!(desc.get("has_text").and_then(Json::as_bool), Some(false));
    assert_eq!(desc.get("seems_scanned").and_then(Json::as_bool), Some(true));
}

// ---------------------------------------------------------------------------------------------------
// Robustness
// ---------------------------------------------------------------------------------------------------

#[test]
fn corrupted_inputs_never_panic() {
    let mut corpus = vec![hello_with_info(), one_page(b"BT /F1 12 Tf 72 700 Td [(A) -300 (B)] TJ ET q 1 0 0 rg 0 0 10 10 re f Q", "", "/Outlines 6 0 R", vec![b("<< /First 7 0 R >>"), b("<< /Title (T) /Next 7 0 R /Dest [3 0 R] >>")], "")];
    // An xref-stream file and an image file join the corpus.
    corpus.push(one_page(b"q 10 0 0 10 0 0 cm /Im1 Do BI /W 2 /H 2 /BPC 1 /IM true ID \x80\x40 EI Q", "/XObject << /Im1 6 0 R >>", "", vec![stream("/Subtype /Image /Width 2 /Height 2 /ColorSpace [/Indexed /DeviceRGB 1 <FF000000FF00>] /BitsPerComponent 1", &[0x80, 0x40])], ""));
    {
        let mut w = PdfWriter::new();
        w.set_date(1);
        let mut dl = DisplayList::new(100.0, 100.0);
        dl.push(Item::Path { ops: vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(50.0, 50.0)], fill: None, stroke: Some(crate::ground::display::Stroke::new(1.0, Color::BLACK)) });
        w.add_page(&dl, &PageResources::new());
        corpus.push(w.finish());
    }
    let mut seed: u64 = 0x5EED_F011_0000_0001;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut opened = 0;
    for i in 0..400 {
        let src = &corpus[i % corpus.len()];
        let mut m = src.clone();
        let edits = 1 + rnd() % 8;
        for _ in 0..edits {
            if m.is_empty() {
                break;
            }
            let pos = (rnd() as usize) % m.len();
            match rnd() % 6 {
                0 | 1 => m[pos] = rnd() as u8,
                2 => {
                    let len = ((rnd() as usize) % 64).min(m.len() - pos);
                    m.drain(pos..pos + len);
                }
                3 => m.truncate(pos),
                4 => {
                    let tok: &[u8] = [b"(" as &[u8], b"<<", b"[", b"0 0 R", b"999999999999 0 obj", b"endstream", b"/Length 99999999", b"-1", b"BI", b"Do"][(rnd() % 10) as usize];
                    let at = pos.min(m.len());
                    m.splice(at..at, tok.iter().copied());
                }
                _ => {
                    let len = ((rnd() as usize) % 32).min(m.len() - pos);
                    let chunk: Vec<u8> = m[pos..pos + len].to_vec();
                    let at = (rnd() as usize) % m.len();
                    m.splice(at..at, chunk);
                }
            }
        }
        if let Ok(d) = FolioDoc::open(m) {
            opened += 1;
            for p in 0..d.page_count().min(4) {
                let _ = d.display(p);
                let _ = d.page_text(p);
                let _ = d.thumbnail(p, 50.0);
            }
            let _ = d.describe();
            let _ = d.search("a");
            let _ = d.save();
        }
    }
    assert!(opened > 50, "most mutated files still open ({} did)", opened);
    // Pathological inputs.
    for junk in [b"%PDF-1.7".to_vec(), b"%PDF-".to_vec(), vec![b')'; 200_000], b"%PDF-1.4\n1 0 obj << /Length 1 0 R >> stream\nxx\nendstream endobj trailer << /Root 1 0 R >>".to_vec(), [b"%PDF-1.4 ".to_vec(), vec![b'['; 100_000]].concat()] {
        let _ = FolioDoc::open(junk);
    }
}

#[test]
fn self_referencing_object_streams_do_not_loop() {
    let mut out = b"%PDF-1.5\n".to_vec();
    let off = out.len();
    out.extend_from_slice(b"6 0 obj\n<< /Type /ObjStm /N 1 /First 4 /Length 9 0 R >>\nstream\n1 0 << /Type /Catalog /Pages 2 0 R >>\nendstream\nendobj\n");
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 1\n0000000000 65535 f \n6 1\n{:010} 00000 n \ntrailer\n<< /Size 10 /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", off, x).as_bytes());
    let _ = FolioDoc::open(out);
}

// ---------------------------------------------------------------------------------------------------
// Embedded and composite fonts
// ---------------------------------------------------------------------------------------------------

/// An installed face's bytes, for embedding in an invented PDF (never written to the repository).
fn installed(pred: impl Fn(&crate::ground::font::Font) -> bool) -> Option<Arc<crate::ground::font::Font>> {
    let idx = fonts::system_index();
    for f in idx.faces() {
        if f.font.index != 0 {
            continue;
        }
        if let Some(face) = idx.load(&f.font) {
            if pred(&face) && face.glyph_id('A').is_some() && face.glyph_id('z').is_some() {
                return Some(face);
            }
        }
    }
    None
}

#[test]
fn embedded_truetype_simple_font() {
    let Some(face) = fonts::system_face("Arial", 32).filter(|f| f.has_truetype_outlines() && f.face_index() == 0) else { return };
    let data = zlib_compress(face.data(), 6);
    let bytes = one_page(
        b"BT /TT 20 Tf 50 700 Td (Caf\xe9 Zone) Tj ET",
        "/Font << /TT 6 0 R >>",
        "",
        vec![
            b("<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+ArialMT /FirstChar 32 /LastChar 255 /Encoding /WinAnsiEncoding /FontDescriptor 7 0 R >>"),
            b("<< /Type /FontDescriptor /FontName /ABCDEF+ArialMT /Flags 32 /FontFile2 8 0 R >>"),
            stream("/Filter /FlateDecode", &data),
        ],
        "",
    );
    let d = open(&bytes);
    assert_eq!(d.page_text(0), "Caf\u{e9} Zone");
    let dl = d.display(0);
    let Some(Item::Glyphs { glyphs, font, .. }) = dl.items.iter().find(|i| matches!(i, Item::Glyphs { .. })) else { panic!("no glyphs") };
    assert_eq!(glyphs[0].gid, face.glyph_id('C').unwrap());
    assert_eq!(glyphs[3].gid, face.glyph_id('\u{e9}').unwrap());
    // No /Widths: advances come from the embedded font itself.
    let g = d.page_glyphs(0);
    let want = face.advance(face.glyph_id('C').unwrap()) as f64 * 20.0 / face.units_per_em() as f64;
    assert!((g[1].x - g[0].x - want).abs() < 1e-6);
    assert!(d.resources().unwrap().glyph(*font, glyphs[0].gid).map_or(false, |o| !o.is_empty()));
}

#[test]
fn embedded_bare_cff_through_the_opentype_wrapper() {
    let Some(face) = installed(|f| f.is_cff() && f.table(b"CFF ").is_some()) else { return };
    let cff_bytes = face.table(b"CFF ").unwrap().to_vec();
    let info = cff::parse(&cff_bytes).expect("the CFF parses");
    assert_eq!(info.num_glyphs as usize, face.num_glyphs() as usize);
    let a = info.gid_for_name("A");
    let wrapped = crate::ground::font::Font::parse(cff::wrap_otf(&cff_bytes, info.num_glyphs, info.upem), 0).expect("the wrapper parses");
    if let Some(a) = a {
        assert_eq!(wrapped.outline(a), face.outline(a));
        let bytes = one_page(
            b"BT /C1 30 Tf 50 700 Td (AAA) Tj ET",
            "/Font << /C1 6 0 R >>",
            "",
            vec![
                b("<< /Type /Font /Subtype /Type1 /BaseFont /XYZABC+Invented /Encoding << /Differences [65 /A] >> /FontDescriptor 7 0 R /FirstChar 65 /LastChar 65 /Widths [700] >>"),
                b("<< /Type /FontDescriptor /FontName /XYZABC+Invented /Flags 32 /FontFile3 8 0 R >>"),
                stream("/Subtype /Type1C", &cff_bytes),
            ],
            "",
        );
        let d = open(&bytes);
        assert_eq!(d.page_text(0), "AAA");
        let dl = d.display(0);
        let Some(Item::Glyphs { glyphs, font, .. }) = dl.items.iter().find(|i| matches!(i, Item::Glyphs { .. })) else { panic!("no glyphs") };
        assert_eq!(glyphs.len(), 3);
        assert_eq!(glyphs[0].gid, a);
        assert!((glyphs[1].x - glyphs[0].x - 21.0).abs() < 1e-6);
        assert!(d.resources().unwrap().glyph(*font, a).map_or(false, |o| !o.is_empty()));
    }
}

#[test]
fn type0_identity_h_with_tounicode_ranges() {
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
                 2 beginbfrange <0003> <0005> <0041> <0010> <0011> [<00660069> <D83DDE00>] endbfrange\n1 beginbfchar <0020> <0020> endbfchar\nendcmap end end";
    let bytes = one_page(
        b"BT /T0 10 Tf 72 700 Td <000300040005> Tj <0020> Tj <00100011> Tj ET",
        "/Font << /T0 6 0 R >>",
        "",
        vec![
            b("<< /Type /Font /Subtype /Type0 /BaseFont /Invented-Sans /Encoding /Identity-H /DescendantFonts [7 0 R] /ToUnicode 8 0 R >>"),
            b("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Invented-Sans /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 500 /W [3 [600 700] 16 17 400] >>"),
            stream("", cmap),
        ],
        "",
    );
    let d = open(&bytes);
    assert_eq!(d.page_text(0), "ABC fi\u{1F600}");
    let g = d.page_glyphs(0);
    // CID 3 is 600 wide, CID 4 700, CID 5 the default 500.
    assert!((g[1].x - g[0].x - 6.0).abs() < 1e-9);
    assert!((g[2].x - g[1].x - 7.0).abs() < 1e-9);
    assert!((g[3].x - g[2].x - 5.0).abs() < 1e-9);
}

#[test]
fn many_pages_open() {
    let n = 60;
    let mut bodies = vec![b("<< /Type /Catalog /Pages 2 0 R >>")];
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    bodies.push(b(&format!("<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> >>", kids.join(" "), n)));
    bodies.push(b("<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>"));
    for i in 0..n {
        bodies.push(b(&format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R >>", 5 + 2 * i)));
        let mut c = String::from("BT /F1 11 Tf 72 740 Td 13 TL ");
        for l in 0..50 {
            c.push_str(&format!("(Page {} line {} of the invented book) ' ", i + 1, l + 1));
        }
        c.push_str("ET");
        bodies.push(stream("", c.as_bytes()));
    }
    let d = open(&pdf(&bodies, ""));
    assert_eq!(d.page_count(), n);
    assert!(d.page_text(n - 1).starts_with(&format!("Page {} line 1 of", n)));
    assert_eq!(d.search("line 50 of").len(), n);
}

#[test]
fn documents_can_move_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FolioDoc>();
    let bytes = hello();
    let handle = std::thread::spawn(move || open(&bytes).page_text(0));
    assert_eq!(handle.join().unwrap(), "Hello World");
}
