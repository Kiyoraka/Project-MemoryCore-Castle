//! Saving edits as an incremental update: the original bytes stay exactly as they were, and the
//! changed and new objects are appended with a new cross-reference section (a classic table, or a
//! cross-reference stream when the file uses them) and a trailer whose /Prev points at the old one.
//! A repaired file gets a complete cross-reference instead of a /Prev into its broken one.

use super::obj::{write_obj, Dict, Obj};
use super::parse::{PdfFile, XRef};
use crate::ground::flate::zlib_compress;

/// The file with its edits appended (the original bytes when there are none).
pub fn incremental_update(file: &PdfFile) -> Vec<u8> {
    if file.overlay.is_empty() && file.trailer_edits.0.is_empty() {
        return file.data.clone();
    }
    let mut out = file.data.clone();
    if !out.ends_with(b"\n") && !out.ends_with(b"\r") {
        out.push(b'\n');
    }
    // (number, generation, where): Offset for objects written here.
    let mut entries: Vec<(u32, u16, XRef)> = Vec::new();
    for (&num, obj) in &file.overlay {
        let gen = file.generation(num);
        let off = out.len();
        out.extend_from_slice(format!("{} {} obj\n", num, gen).as_bytes());
        // A locked file's update is locked with the same key.
        write_obj(&mut out, &file.locked(obj, num, gen));
        out.extend_from_slice(b"\nendobj\n");
        entries.push((num, gen, XRef::Offset(off, gen)));
    }
    let full = file.repaired;
    if full {
        // Every object the file has, so the new section stands alone.
        for num in file.object_numbers() {
            if file.overlay.contains_key(&num) {
                continue;
            }
            if let Some(x) = file.xref_entry(num) {
                entries.push((num, file.generation(num), x));
            }
        }
        entries.sort_by_key(|e| e.0);
    }
    let base = file.trailer();
    let mut trailer = Dict::new();
    for k in ["Root", "Info", "ID", "Encrypt"] {
        if let Some(v) = base.get(k) {
            trailer.set(k, v.clone());
        }
    }
    if !full {
        trailer.set("Prev", Obj::Int(file.startxref as i64));
    }
    let use_stream = file.xref_is_stream || entries.iter().any(|e| matches!(e.2, XRef::Compressed(..)));
    let xref_off;
    if use_stream {
        let own = file.size();
        let size = own + 1;
        xref_off = out.len();
        entries.push((own, 0, XRef::Offset(xref_off, 0)));
        entries.sort_by_key(|e| e.0);
        let mut data = Vec::with_capacity(entries.len() * 7);
        let mut index = Vec::new();
        let mut i = 0;
        while i < entries.len() {
            let start = entries[i].0;
            let mut j = i;
            while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
                j += 1;
            }
            index.push(Obj::Int(start as i64));
            index.push(Obj::Int((j - i + 1) as i64));
            for e in &entries[i..=j] {
                match e.2 {
                    XRef::Offset(off, gen) => {
                        data.push(1);
                        data.extend_from_slice(&(off as u32).to_be_bytes());
                        data.extend_from_slice(&gen.to_be_bytes());
                    }
                    XRef::Compressed(stm, idx) => {
                        data.push(2);
                        data.extend_from_slice(&stm.to_be_bytes());
                        data.extend_from_slice(&(idx.min(65535) as u16).to_be_bytes());
                    }
                }
            }
            i = j + 1;
        }
        let mut d = trailer.clone();
        d.set("Type", Obj::name("XRef"));
        d.set("Size", Obj::Int(size as i64));
        d.set("W", Obj::Arr(vec![Obj::Int(1), Obj::Int(4), Obj::Int(2)]));
        d.set("Index", Obj::Arr(index));
        d.set("Filter", Obj::name("FlateDecode"));
        let compressed = zlib_compress(&data, 6);
        let s = Obj::Stream(std::sync::Arc::new(super::obj::Stream { dict: d, data: compressed }));
        out.extend_from_slice(format!("{} 0 obj\n", own).as_bytes());
        write_obj(&mut out, &s);
        out.extend_from_slice(b"\nendobj\n");
    } else {
        xref_off = out.len();
        out.extend_from_slice(b"xref\n");
        let mut i = 0;
        while i < entries.len() {
            let mut j = i;
            while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
                j += 1;
            }
            out.extend_from_slice(format!("{} {}\n", entries[i].0, j - i + 1).as_bytes());
            for e in &entries[i..=j] {
                if let XRef::Offset(off, gen) = e.2 {
                    out.extend_from_slice(format!("{:010} {:05} n\r\n", off, gen).as_bytes());
                }
            }
            i = j + 1;
        }
        trailer.set("Size", Obj::Int(file.size() as i64));
        out.extend_from_slice(b"trailer\n");
        write_obj(&mut out, &Obj::Dict(trailer));
        out.push(b'\n');
    }
    out.extend_from_slice(format!("startxref\n{}\n%%EOF\n", xref_off).as_bytes());
    out
}
