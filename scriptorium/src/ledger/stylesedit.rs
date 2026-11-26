//! Changing a cell's format: a new cell format (`cellXfs` entry) worked out from the one the cell
//! has, with its font, fill, border, number format or alignment changed. Everything goes into
//! `styles.xml` the way the file already holds it: a font, fill, border or `xf` that already exists
//! is used again, a new one is appended (counts kept right), custom number formats get the next id
//! from 164 up. The elements are edited as they are, so attributes and children we do not know
//! (`quotePrefix`, protection, theme colours of a font we only make bold) stay.

use super::edit::qualify;
use super::numfmt::builtin_format;
use crate::ground::xml::{Element, Node};

/// One side of a border: its style (`thin`, `medium`, ...) and colour (`RRGGBB`), or none.
pub type Side = Option<(String, Option<String>)>;

/// What a format change sets; `None` leaves that part as it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FormatChange {
    pub family: Option<String>,
    pub size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// `Some(None)`: automatic colour.
    pub color: Option<Option<String>>,
    /// `Some(None)`: no fill.
    pub fill: Option<Option<String>>,
    /// `Some(None)`: general.
    pub h_align: Option<Option<String>>,
    pub v_align: Option<Option<String>>,
    pub wrap: Option<bool>,
    pub indent: Option<u32>,
    /// A format code.
    pub num_fmt: Option<String>,
    pub left: Option<Side>,
    pub right: Option<Side>,
    pub top: Option<Side>,
    pub bottom: Option<Side>,
}

impl FormatChange {
    fn touches_font(&self) -> bool {
        self.family.is_some() || self.size.is_some() || self.bold.is_some() || self.italic.is_some() || self.underline.is_some() || self.strike.is_some() || self.color.is_some()
    }

    fn touches_border(&self) -> bool {
        self.left.is_some() || self.right.is_some() || self.top.is_some() || self.bottom.is_some()
    }

    fn touches_alignment(&self) -> bool {
        self.h_align.is_some() || self.v_align.is_some() || self.wrap.is_some() || self.indent.is_some()
    }

    pub fn is_empty(&self) -> bool {
        *self == FormatChange::default()
    }
}

/// `#RRGGBB`, `RRGGBB` or `AARRGGBB` to `FFRRGGBB`; a few colour names too.
pub fn argb(text: &str) -> Option<String> {
    let t = text.trim().trim_start_matches('#');
    let named = match t.to_ascii_lowercase().as_str() {
        "black" => Some("000000"),
        "white" => Some("FFFFFF"),
        "red" => Some("FF0000"),
        "green" => Some("00B050"),
        "blue" => Some("0070C0"),
        "yellow" => Some("FFFF00"),
        "orange" => Some("FFC000"),
        "purple" => Some("7030A0"),
        "gray" | "grey" => Some("808080"),
        "light blue" | "lightblue" => Some("DDEBF7"),
        "light green" | "lightgreen" => Some("E2EFDA"),
        "light yellow" | "lightyellow" => Some("FFF2CC"),
        _ => None,
    };
    let t = named.unwrap_or(t);
    if !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match t.len() {
        6 => Some(format!("FF{}", t.to_ascii_uppercase())),
        8 => Some(t.to_ascii_uppercase()),
        3 => {
            let d: String = t.chars().flat_map(|c| [c, c]).collect();
            Some(format!("FF{}", d.to_ascii_uppercase()))
        }
        _ => None,
    }
}

fn index_of(e: &Element, local: &str) -> Option<usize> {
    e.children.iter().position(|n| matches!(n, Node::Element(x) if x.local() == local))
}

fn kids_mut<'a>(e: &'a mut Element, local: &str) -> Vec<&'a mut Element> {
    e.children
        .iter_mut()
        .filter_map(|n| match n {
            Node::Element(x) if x.local() == local => Some(x),
            _ => None,
        })
        .collect()
}

fn nth(e: &Element, local: &str, i: usize) -> Option<Element> {
    e.children_named(local).nth(i).cloned()
}

/// The child `list` of the style sheet (made in its place in the schema's order when missing).
fn list_mut<'a>(root: &'a mut Element, list: &str) -> &'a mut Element {
    const ORDER: [&str; 11] = ["numFmts", "fonts", "fills", "borders", "cellStyleXfs", "cellXfs", "cellStyles", "dxfs", "tableStyles", "colors", "extLst"];
    if index_of(root, list).is_none() {
        let prefix = root.prefix().map(str::to_string);
        let me = ORDER.iter().position(|n| *n == list).unwrap_or(ORDER.len());
        let at = root
            .children
            .iter()
            .position(|n| matches!(n, Node::Element(x) if ORDER.iter().position(|o| *o == x.local()).is_some_and(|k| k > me)))
            .unwrap_or(root.children.len());
        let mut e = Element::new(qualify(&prefix, list));
        e.attrs.push(("count".into(), "0".into()));
        root.children.insert(at, Node::Element(e));
    }
    let i = index_of(root, list).expect("just made");
    match &mut root.children[i] {
        Node::Element(e) => e,
        _ => unreachable!(),
    }
}

/// The index of `item` in `list` (an equal element already there), else appended.
fn find_or_add(root: &mut Element, list: &str, item_local: &str, item: Element) -> usize {
    let l = list_mut(root, list);
    if let Some(i) = l.children_named(item_local).position(|x| *x == item) {
        return i;
    }
    let n = l.children_named(item_local).count();
    let at = l.children.iter().rposition(|x| matches!(x, Node::Element(e) if e.local() == item_local)).map(|i| i + 1).unwrap_or(l.children.len());
    l.children.insert(at, Node::Element(item));
    l.set_attr("count", (n + 1).to_string());
    n
}

fn child_el(prefix: &Option<String>, local: &str, attrs: &[(&str, String)]) -> Element {
    let mut e = Element::new(qualify(prefix, local));
    for (k, v) in attrs {
        e.attrs.push((k.to_string(), v.clone()));
    }
    e
}

fn set_flag(font: &mut Element, prefix: &Option<String>, local: &str, on: bool) {
    let has = index_of(font, local);
    match (has, on) {
        (Some(i), true) => {
            if let Node::Element(e) = &mut font.children[i] {
                e.attrs.retain(|(k, _)| k != "val");
            }
        }
        (Some(i), false) => {
            font.children.remove(i);
        }
        (None, true) => font.children.insert(0, Node::Element(child_el(prefix, local, &[]))),
        (None, false) => {}
    }
}

fn set_val(font: &mut Element, prefix: &Option<String>, local: &str, val: String, before: &[&str]) {
    if let Some(e) = kids_mut(font, local).into_iter().next() {
        e.attrs = vec![("val".into(), val)];
        return;
    }
    let at = font.children.iter().position(|n| matches!(n, Node::Element(x) if before.contains(&x.local()))).unwrap_or(font.children.len());
    font.children.insert(at, Node::Element(child_el(prefix, local, &[("val", val)])));
}

/// The `numFmtId` for a format code: a built-in id, the file's own id for that code, or a new one.
pub fn num_fmt_id(root: &mut Element, code: &str) -> u32 {
    if code.eq_ignore_ascii_case("general") {
        return 0;
    }
    if let Some(id) = (1..=49).find(|&i| builtin_format(i) == Some(code) && !matches!(i, 27..=36)) {
        // A built-in id is only safe when the file does not give that id its own code.
        let own = root.child("numFmts").and_then(|n| n.children_named("numFmt").find(|f| f.attr("numFmtId") == Some(&id.to_string())).cloned());
        if own.is_none() {
            return id;
        }
    }
    let prefix = root.prefix().map(str::to_string);
    let list = list_mut(root, "numFmts");
    let mut max = 163;
    for f in list.children_named("numFmt") {
        let id: u32 = f.attr("numFmtId").and_then(|v| v.parse().ok()).unwrap_or(0);
        if f.attr("formatCode") == Some(code) {
            return id;
        }
        max = max.max(id);
    }
    let id = max + 1;
    let item = child_el(&prefix, "numFmt", &[("numFmtId", id.to_string()), ("formatCode", code.to_string())]);
    let n = list.children_named("numFmt").count();
    list.children.push(Node::Element(item));
    list.set_attr("count", (n + 1).to_string());
    id
}

fn side_el(prefix: &Option<String>, local: &str, side: &Side) -> Element {
    let mut e = Element::new(qualify(prefix, local));
    if let Some((style, color)) = side {
        e.attrs.push(("style".into(), style.clone()));
        let mut c = Element::new(qualify(prefix, "color"));
        match color {
            Some(rgb) => c.attrs.push(("rgb".into(), rgb.clone())),
            None => c.attrs.push(("indexed".into(), "64".into())),
        }
        e.children.push(Node::Element(c));
    }
    e
}

/// `applyX="1"` when the part is not the default one; dropped when it is.
fn apply_flag(xf: &mut Element, name: &str, on: bool) {
    if on {
        xf.set_attr(name, "1");
    } else {
        xf.attrs.retain(|(k, _)| k != name);
    }
}

/// The `cellXfs` index of `base` with `ch` applied (an existing equal entry, else a new one).
pub fn derive(root: &mut Element, base: u32, ch: &FormatChange) -> u32 {
    let prefix = root.prefix().map(str::to_string);
    let mut xf = root
        .child("cellXfs")
        .and_then(|l| nth(l, "xf", base as usize))
        .unwrap_or_else(|| child_el(&prefix, "xf", &[("numFmtId", "0".into()), ("fontId", "0".into()), ("fillId", "0".into()), ("borderId", "0".into()), ("xfId", "0".into())]));
    let num = |x: &Element, a: &str| x.attr(a).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);

    if ch.touches_font() {
        let mut font = root.child("fonts").and_then(|l| nth(l, "font", num(&xf, "fontId"))).unwrap_or_else(|| {
            let mut f = child_el(&prefix, "font", &[]);
            f.children.push(Node::Element(child_el(&prefix, "sz", &[("val", "11".into())])));
            f.children.push(Node::Element(child_el(&prefix, "name", &[("val", "Calibri".into())])));
            f
        });
        if let Some(b) = ch.bold {
            set_flag(&mut font, &prefix, "b", b);
        }
        if let Some(b) = ch.italic {
            set_flag(&mut font, &prefix, "i", b);
        }
        if let Some(b) = ch.strike {
            set_flag(&mut font, &prefix, "strike", b);
        }
        if let Some(b) = ch.underline {
            set_flag(&mut font, &prefix, "u", b);
        }
        if let Some(sz) = ch.size {
            let v = if sz.fract() == 0.0 { format!("{}", sz as i64) } else { format!("{sz}") };
            set_val(&mut font, &prefix, "sz", v, &["color", "name", "family", "charset", "scheme"]);
        }
        if let Some(c) = &ch.color {
            if let Some(i) = index_of(&font, "color") {
                font.children.remove(i);
            }
            if let Some(rgb) = c {
                let at = font.children.iter().position(|n| matches!(n, Node::Element(x) if matches!(x.local(), "name" | "family" | "charset" | "scheme"))).unwrap_or(font.children.len());
                font.children.insert(at, Node::Element(child_el(&prefix, "color", &[("rgb", rgb.clone())])));
            }
        }
        if let Some(fam) = &ch.family {
            set_val(&mut font, &prefix, "name", fam.clone(), &["family", "charset", "scheme"]);
            // A theme font scheme would win over the name.
            if let Some(i) = index_of(&font, "scheme") {
                font.children.remove(i);
            }
        }
        let id = find_or_add(root, "fonts", "font", font);
        xf.set_attr("fontId", id.to_string());
        apply_flag(&mut xf, "applyFont", id != 0);
    }

    if let Some(fill) = &ch.fill {
        let id = match fill {
            None => 0,
            Some(rgb) => {
                // The two fills every file starts with must be there first.
                if root.child("fills").map_or(0, |l| l.children_named("fill").count()) < 2 {
                    for pt in ["none", "gray125"] {
                        let mut f = child_el(&prefix, "fill", &[]);
                        f.children.push(Node::Element(child_el(&prefix, "patternFill", &[("patternType", pt.into())])));
                        let l = list_mut(root, "fills");
                        if l.children_named("fill").count() < 2 {
                            let n = l.children_named("fill").count();
                            l.children.push(Node::Element(f));
                            l.set_attr("count", (n + 1).to_string());
                        }
                    }
                }
                let mut f = child_el(&prefix, "fill", &[]);
                let mut p = child_el(&prefix, "patternFill", &[("patternType", "solid".into())]);
                p.children.push(Node::Element(child_el(&prefix, "fgColor", &[("rgb", rgb.clone())])));
                p.children.push(Node::Element(child_el(&prefix, "bgColor", &[("indexed", "64".into())])));
                f.children.push(Node::Element(p));
                find_or_add(root, "fills", "fill", f)
            }
        };
        xf.set_attr("fillId", id.to_string());
        apply_flag(&mut xf, "applyFill", id != 0);
    }

    if ch.touches_border() {
        let old = root.child("borders").and_then(|l| nth(l, "border", num(&xf, "borderId"))).unwrap_or_else(|| child_el(&prefix, "border", &[]));
        let mut b = Element { name: old.name.clone(), attrs: old.attrs.clone(), children: Vec::new() };
        let pick = |names: &[&str]| old.elements().find(|e| names.contains(&e.local())).cloned();
        let parts: [(&str, &[&str], &Option<Side>); 4] =
            [("left", &["left", "start"], &ch.left), ("right", &["right", "end"], &ch.right), ("top", &["top"], &ch.top), ("bottom", &["bottom"], &ch.bottom)];
        for (local, names, want) in parts {
            let el = match want {
                Some(side) => Some(side_el(&prefix, local, side)),
                None => pick(names).or_else(|| Some(child_el(&prefix, local, &[]))),
            };
            if let Some(e) = el {
                b.children.push(Node::Element(e));
            }
        }
        for rest in ["diagonal", "vertical", "horizontal"] {
            if let Some(e) = pick(&[rest]) {
                b.children.push(Node::Element(e));
            } else if rest == "diagonal" {
                b.children.push(Node::Element(child_el(&prefix, "diagonal", &[])));
            }
        }
        let id = find_or_add(root, "borders", "border", b);
        xf.set_attr("borderId", id.to_string());
        apply_flag(&mut xf, "applyBorder", id != 0);
    }

    if let Some(code) = &ch.num_fmt {
        let id = num_fmt_id(root, code);
        xf.set_attr("numFmtId", id.to_string());
        apply_flag(&mut xf, "applyNumberFormat", id != 0);
    }

    if ch.touches_alignment() {
        let ai = index_of(&xf, "alignment");
        let mut a = match ai {
            Some(i) => match &xf.children[i] {
                Node::Element(e) => e.clone(),
                _ => unreachable!(),
            },
            None => child_el(&prefix, "alignment", &[]),
        };
        let mut set = |k: &str, v: Option<String>| match v {
            Some(v) => a.set_attr(k, v),
            None => a.attrs.retain(|(x, _)| x != k),
        };
        if let Some(h) = &ch.h_align {
            set("horizontal", h.clone().filter(|h| h != "general"));
        }
        if let Some(v) = &ch.v_align {
            set("vertical", v.clone().filter(|v| v != "bottom"));
        }
        if let Some(w) = ch.wrap {
            set("wrapText", w.then(|| "1".to_string()));
        }
        if let Some(n) = ch.indent {
            set("indent", (n > 0).then(|| n.to_string()));
        }
        match (ai, a.attrs.is_empty()) {
            (Some(i), true) => {
                xf.children.remove(i);
            }
            (Some(i), false) => xf.children[i] = Node::Element(a),
            (None, true) => {}
            (None, false) => xf.children.insert(0, Node::Element(a)),
        }
        let has = index_of(&xf, "alignment").is_some();
        apply_flag(&mut xf, "applyAlignment", has);
    }
    find_or_add(root, "cellXfs", "xf", xf) as u32
}

/// A format code with one more (`delta` > 0) or one fewer decimal place in each section.
pub fn change_decimals(code: &str, delta: i32) -> String {
    if code.eq_ignore_ascii_case("general") {
        return code.to_string();
    }
    let mut out = String::new();
    let mut sec = String::new();
    let (mut in_q, mut in_b) = (false, false);
    let mut chars = code.chars().peekable();
    let flush = |sec: &mut String, out: &mut String| {
        out.push_str(&section_decimals(sec, delta));
        sec.clear();
    };
    while let Some(c) = chars.next() {
        if in_q {
            sec.push(c);
            in_q = c != '"';
            continue;
        }
        if in_b {
            sec.push(c);
            in_b = c != ']';
            continue;
        }
        match c {
            '"' => {
                in_q = true;
                sec.push(c);
            }
            '[' => {
                in_b = true;
                sec.push(c);
            }
            '\\' | '_' | '*' => {
                sec.push(c);
                if let Some(n) = chars.next() {
                    sec.push(n);
                }
            }
            ';' => {
                flush(&mut sec, &mut out);
                out.push(';');
            }
            _ => sec.push(c),
        }
    }
    flush(&mut sec, &mut out);
    out
}

/// One section: find the number's digits (outside quotes and brackets) and grow or shrink the
/// decimals after its point. A section with no digit placeholders (text, dates) stays.
fn section_decimals(sec: &str, delta: i32) -> String {
    let b: Vec<char> = sec.chars().collect();
    let mut lit = vec![false; b.len()];
    let (mut in_q, mut in_b) = (false, false);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if in_q {
            lit[i] = true;
            in_q = c != '"';
        } else if in_b {
            lit[i] = true;
            in_b = c != ']';
        } else if c == '"' {
            lit[i] = true;
            in_q = true;
        } else if c == '[' {
            lit[i] = true;
            in_b = true;
        } else if c == '\\' || c == '_' || c == '*' {
            lit[i] = true;
            if i + 1 < b.len() {
                lit[i + 1] = true;
            }
            i += 1;
        }
        i += 1;
    }
    let is_digit = |k: usize| !lit[k] && matches!(b[k], '0' | '#' | '?');
    let has_date = (0..b.len()).any(|k| !lit[k] && matches!(b[k].to_ascii_lowercase(), 'y' | 'd' | 'h' | 's' | 'm'));
    let Some(last) = (0..b.len()).rev().find(|&k| is_digit(k)) else { return sec.to_string() };
    if has_date && !(0..b.len()).any(|k| !lit[k] && b[k] == '0' && k > 0 && b[k - 1] == '.') {
        return sec.to_string();
    }
    // An exponent: the decimals are those of the mantissa, before the E.
    let e_at = (0..b.len()).find(|&k| !lit[k] && (b[k] == 'E' || b[k] == 'e'));
    let last = match e_at {
        Some(e) => match (0..e).rev().find(|&k| is_digit(k)) {
            Some(k) => k,
            None => return sec.to_string(),
        },
        None => last,
    };
    let point = (0..=last).rev().find(|&k| !lit[k] && b[k] == '.');
    let mut out: Vec<char> = b.clone();
    match point {
        Some(p) => {
            if delta > 0 {
                for _ in 0..delta {
                    out.insert(last + 1, '0');
                }
            } else {
                let mut digits = last - p;
                let mut end = last;
                for _ in 0..(-delta) {
                    if digits == 0 {
                        break;
                    }
                    out.remove(end);
                    end -= 1;
                    digits -= 1;
                }
                if digits == 0 {
                    out.remove(p);
                }
            }
        }
        None => {
            if delta > 0 {
                let ins: Vec<char> = std::iter::once('.').chain(std::iter::repeat('0').take(delta as usize)).collect();
                for (k, c) in ins.into_iter().enumerate() {
                    out.insert(last + 1 + k, c);
                }
            }
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground::xml::parse_dom;

    #[test]
    fn decimals_grow_and_shrink() {
        assert_eq!(change_decimals("0", 1), "0.0");
        assert_eq!(change_decimals("0.00", 1), "0.000");
        assert_eq!(change_decimals("0.0", -1), "0");
        assert_eq!(change_decimals("#,##0.00", -1), "#,##0.0");
        assert_eq!(change_decimals("0%", 1), "0.0%");
        assert_eq!(change_decimals("$#,##0_);($#,##0)", 2), "$#,##0.00_);($#,##0.00)");
        assert_eq!(change_decimals("0.00E+00", -1), "0.0E+00");
        assert_eq!(change_decimals("\"0.\"0", 1), "\"0.\"0.0");
        assert_eq!(change_decimals("m/d/yyyy", 1), "m/d/yyyy");
        assert_eq!(change_decimals("@", 1), "@");
    }

    #[test]
    fn colours() {
        assert_eq!(argb("#ff0000").as_deref(), Some("FFFF0000"));
        assert_eq!(argb("80FF0000").as_deref(), Some("80FF0000"));
        assert_eq!(argb("red").as_deref(), Some("FFFF0000"));
        assert_eq!(argb("#abc").as_deref(), Some("FFAABBCC"));
        assert_eq!(argb("nope"), None);
    }

    #[test]
    fn derive_reuses_and_appends() {
        let xml = br#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><scheme val="minor"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" quotePrefix="1"/></cellXfs></styleSheet>"#;
        let mut root = parse_dom(xml).unwrap();
        let bold = FormatChange { bold: Some(true), ..Default::default() };
        let a = derive(&mut root, 0, &bold);
        assert_eq!(a, 1);
        assert_eq!(derive(&mut root, 0, &bold), 1, "the same change finds the same format");
        let fonts = root.child("fonts").unwrap();
        assert_eq!(fonts.attr("count"), Some("2"));
        let f = fonts.children_named("font").nth(1).unwrap();
        assert!(f.child("b").is_some() && f.child("color").unwrap().attr("theme") == Some("1"));
        let xf = root.child("cellXfs").unwrap().children_named("xf").nth(1).unwrap();
        assert_eq!(xf.attr("quotePrefix"), Some("1"), "unknown attributes stay");
        assert_eq!(xf.attr("fontId"), Some("1"));
        // Undoing the bold finds the first format again.
        assert_eq!(derive(&mut root, 1, &FormatChange { bold: Some(false), ..Default::default() }), 0);
        // A money format gets a custom id; a built-in code its built-in id.
        let money = derive(&mut root, 0, &FormatChange { num_fmt: Some("\"€\"#,##0.00".into()), ..Default::default() });
        assert_eq!(root.child("numFmts").unwrap().children_named("numFmt").next().unwrap().attr("numFmtId"), Some("164"));
        assert_eq!(root.children.iter().position(|n| matches!(n, Node::Element(e) if e.local() == "numFmts")), Some(0));
        let pct = derive(&mut root, money, &FormatChange { num_fmt: Some("0%".into()), ..Default::default() });
        assert_eq!(root.child("cellXfs").unwrap().children_named("xf").nth(pct as usize).unwrap().attr("numFmtId"), Some("9"));
        // Fill, border, alignment.
        let x = derive(
            &mut root,
            0,
            &FormatChange {
                fill: Some(Some("FFFFFF00".into())),
                bottom: Some(Some(("thin".into(), None))),
                h_align: Some(Some("center".into())),
                wrap: Some(true),
                ..Default::default()
            },
        );
        let xf = root.child("cellXfs").unwrap().children_named("xf").nth(x as usize).unwrap().clone();
        assert_eq!(xf.child("alignment").unwrap().attr("horizontal"), Some("center"));
        assert_eq!(xf.attr("fillId"), Some("2"));
        let border = root.child("borders").unwrap().children_named("border").nth(1).unwrap();
        assert_eq!(border.child("bottom").unwrap().attr("style"), Some("thin"));
        let names: Vec<&str> = border.elements().map(|e| e.local()).collect();
        assert_eq!(names, ["left", "right", "top", "bottom", "diagonal"]);
        let fam = derive(&mut root, 0, &FormatChange { family: Some("Arial".into()), ..Default::default() });
        let f = root.child("fonts").unwrap().children_named("font").nth(root.child("cellXfs").unwrap().children_named("xf").nth(fam as usize).unwrap().attr("fontId").unwrap().parse().unwrap()).unwrap();
        assert!(f.child("scheme").is_none() && f.child("name").unwrap().attr("val") == Some("Arial"));
    }
}
