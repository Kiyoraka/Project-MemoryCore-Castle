//! Exact undo for every hand edit: the bytes of the parts a command may touch are kept before it runs,
//! and its inverse (`stage.restore_parts`) puts back exactly those bytes, removes the parts it added
//! and restores those it removed. Nothing is re-derived, so undo is byte-exact by construction.

use crate::ground::bus::json::Json;
use crate::ground::bus::{BusError, Command};
use crate::ground::opc::{name_key, Package, CONTENT_TYPES_NAME};

/// Parts as they were: name -> (content type, bytes).
#[derive(Debug, Clone, Default)]
pub struct Snap {
    parts: Vec<(String, String, Vec<u8>)>,
    /// Every part name before (to find the ones a command added).
    names: Option<Vec<String>>,
}

fn is_xml_name(n: &str) -> bool {
    let l = n.to_ascii_lowercase();
    l.ends_with(".xml") || l.ends_with(".rels")
}

impl Snap {
    /// Keep these parts (a missing part is remembered as missing only for whole-package snaps).
    pub fn of(pkg: &Package, names: &[&str]) -> Snap {
        let mut s = Snap::default();
        for n in names {
            if let Ok(b) = pkg.part_bytes(n) {
                s.parts.push((n.to_string(), pkg.content_type(n).unwrap_or("application/xml").to_string(), b));
            }
        }
        s
    }

    /// Keep every XML part (and relationships, and the content types) and the list of all parts.
    pub fn whole(pkg: &Package) -> Snap {
        let names = pkg.part_names();
        let mut s = Snap { parts: Vec::new(), names: Some(names.clone()) };
        for n in &names {
            if is_xml_name(n) || n.eq_ignore_ascii_case(CONTENT_TYPES_NAME) {
                if let Ok(b) = pkg.part_bytes(n) {
                    s.parts.push((n.clone(), pkg.content_type(n).unwrap_or("application/xml").to_string(), b));
                }
            }
        }
        s
    }

    /// The inverse of what happened since the snap, or `None` when nothing changed.
    pub fn undo(&self, pkg: &Package) -> Option<Command> {
        let mut list = Vec::new();
        for (name, ct, bytes) in &self.parts {
            match pkg.part_bytes(name) {
                Ok(now) if &now == bytes => {}
                _ => list.push(part_json(name, ct, bytes)),
            }
        }
        if let Some(before) = &self.names {
            let had: std::collections::HashSet<String> = before.iter().map(|n| name_key(n)).collect();
            for n in pkg.part_names() {
                if !had.contains(&name_key(&n)) {
                    list.push(Json::obj().with("name", n).with("remove", true));
                }
            }
        }
        if list.is_empty() {
            return None;
        }
        Some(Command::new("stage.restore_parts", Json::obj().with("parts", Json::Arr(list))))
    }
}

fn part_json(name: &str, ct: &str, bytes: &[u8]) -> Json {
    let j = Json::obj().with("name", name).with("ct", ct);
    match std::str::from_utf8(bytes) {
        Ok(s) => j.with("xml", s),
        Err(_) => j.with("b64", b64_encode(bytes)),
    }
}

/// What a restore changed: the part names written or removed.
pub struct Restored {
    pub names: Vec<String>,
    /// True when anything but a slide-like part changed (relationships, content types, the
    /// presentation, parts added or removed): the deck must be read again.
    pub structural: bool,
}

/// Apply `stage.restore_parts`; returns its own inverse and what changed.
pub fn restore(pkg: &mut Package, args: &Json) -> Result<(Command, Restored), BusError> {
    let list = args.get("parts").and_then(Json::as_arr).ok_or_else(|| BusError::bad_args("stage.restore_parts", "\"parts\" is a list"))?;
    // The inverse: snap what is there now (for every named part), plus the parts we will remove.
    let mut back = Snap::default();
    let mut removed_now = Vec::new();
    let mut added_now = Vec::new();
    for p in list {
        let name = p.get("name").and_then(Json::as_str).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        match pkg.part_bytes(name) {
            Ok(b) => back.parts.push((name.to_string(), pkg.content_type(name).unwrap_or("application/xml").to_string(), b)),
            Err(_) => added_now.push(name.to_string()),
        }
        if p.get("remove").and_then(Json::as_bool) == Some(true) {
            removed_now.push(name.to_string());
        }
    }
    let mut names = Vec::new();
    let mut structural = false;
    // Removals first (they also drop relationships pointing at them; those parts are restored below).
    for p in list {
        if p.get("remove").and_then(Json::as_bool) == Some(true) {
            if let Some(n) = p.get("name").and_then(Json::as_str) {
                pkg.remove_part(n);
                names.push(n.to_string());
                structural = true;
            }
        }
    }
    let mut ct_bytes: Option<Vec<u8>> = None;
    for p in list {
        if p.get("remove").and_then(Json::as_bool) == Some(true) {
            continue;
        }
        let Some(name) = p.get("name").and_then(Json::as_str) else { continue };
        let bytes = if let Some(x) = p.get("xml").and_then(Json::as_str) {
            x.as_bytes().to_vec()
        } else if let Some(b) = p.get("b64").and_then(Json::as_str) {
            b64_decode(b).ok_or_else(|| BusError::failed("a part's bytes are not readable"))?
        } else {
            continue;
        };
        if name.eq_ignore_ascii_case(CONTENT_TYPES_NAME) {
            ct_bytes = Some(bytes);
            structural = true;
            continue;
        }
        let ct = p.get("ct").and_then(Json::as_str).unwrap_or("application/xml");
        let l = name.to_ascii_lowercase();
        if l.ends_with(".rels") || l.ends_with("presentation.xml") {
            structural = true;
        }
        if pkg.part(name).is_some() {
            pkg.set_part_bytes(name, bytes).map_err(|e| BusError::failed(e.to_string()))?;
        } else {
            pkg.add_part(name, ct, bytes).map_err(|e| BusError::failed(e.to_string()))?;
            structural = true;
        }
        names.push(name.to_string());
    }
    if let Some(b) = ct_bytes {
        pkg.set_part_bytes(CONTENT_TYPES_NAME, b).map_err(|e| BusError::failed(e.to_string()))?;
        names.push(CONTENT_TYPES_NAME.to_string());
    }
    // The inverse: put back what was there, remove what was not.
    let mut inv: Vec<Json> = back.parts.iter().map(|(n, ct, b)| part_json(n, ct, b)).collect();
    for n in added_now {
        inv.push(Json::obj().with("name", n).with("remove", true));
    }
    let _ = removed_now;
    Ok((Command::new("stage.restore_parts", Json::obj().with("parts", Json::Arr(inv))), Restored { names, structural }))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Base64 (standard or URL-safe, padding optional, whitespace and a `data:...;base64,` head ignored).
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let s = match s.find("base64,") {
        Some(i) if s.starts_with("data:") => &s[i + 7..],
        _ => s,
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0, 255, 128, 7]] {
            let e = b64_encode(data);
            assert_eq!(b64_decode(&e).unwrap(), data);
        }
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_decode("data:image/png;base64,Zm9v").unwrap(), b"foo");
        assert!(b64_decode("**").is_none());
    }
}
