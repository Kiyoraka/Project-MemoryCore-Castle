//! Interactive forms (AcroForm): the field tree with its inherited values, the widgets that show each
//! field on its page, filling a field, and the appearance a filled field is drawn with.
//!
//! Filling writes the field's /V and each widget's appearance (/AP /N, and /AS for check boxes and
//! radio buttons) into the overlay, so the save appends them as an incremental update. A text field's
//! appearance is built here from its /DA (font and size; size 0 fits the box), its rectangle and its
//! alignment /Q, wrapping words for a multi-line field. When the form says /NeedAppearances, or a
//! widget has no appearance, the page shows one made the same way (only on screen, nothing is
//! written until the member fills a field).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::afm::Std14;
use super::doc::PageInfo;
use super::encoding::ascii_name;
use super::obj::{encode_text, fmt_num, write_string, Dict, Obj, Stream};
use super::parse::PdfFile;
use super::select::At;
use super::FolioDoc;
use crate::ground::bus::json::Json;

/// The kinds of field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Check,
    Radio,
    Push,
    Combo,
    List,
    Signature,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Check => "checkbox",
            Kind::Radio => "radio",
            Kind::Push => "button",
            Kind::Combo => "combo",
            Kind::List => "list",
            Kind::Signature => "signature",
        }
    }
}

/// One widget: where a field shows on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    /// The widget annotation's object number (0 when direct).
    pub num: u32,
    pub page: Option<usize>,
    /// PDF user space: x0, y0, x1, y1.
    pub rect: [f64; 4],
    /// A check box's or radio button's "on" state name.
    pub on: Option<String>,
    /// The widget has a normal appearance of its own.
    pub has_ap: bool,
}

/// One field (a terminal of the field tree).
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// The full name ("address.street").
    pub name: String,
    /// The object holding /V (0 when direct; then it cannot be filled).
    pub num: u32,
    pub kind: Kind,
    /// Text and choice: the value as text; check and radio: the state name ("Off" when off).
    pub value: String,
    /// Choices: (export value, shown text).
    pub options: Vec<(String, String)>,
    pub widgets: Vec<Widget>,
    pub flags: i64,
    pub da: String,
    /// Alignment: 0 left, 1 centre, 2 right.
    pub q: i64,
    pub max_len: Option<usize>,
}

impl Field {
    pub fn read_only(&self) -> bool {
        self.flags & 1 != 0
    }

    pub fn multiline(&self) -> bool {
        self.kind == Kind::Text && self.flags & (1 << 12) != 0
    }

    pub fn password(&self) -> bool {
        self.kind == Kind::Text && self.flags & (1 << 13) != 0
    }

    /// A combo box the member may type into.
    pub fn editable(&self) -> bool {
        self.kind == Kind::Combo && self.flags & (1 << 18) != 0
    }

    /// The member can fill it by hand.
    pub fn fillable(&self) -> bool {
        !self.read_only() && self.num != 0 && !matches!(self.kind, Kind::Push | Kind::Signature)
    }

    /// The text a choice shows for its value.
    pub fn shown_value(&self) -> String {
        self.options.iter().find(|(e, _)| *e == self.value).map(|(_, d)| d.clone()).unwrap_or_else(|| self.value.clone())
    }

    pub fn to_json(&self, doc: &FolioDoc) -> Json {
        let mut widgets = Json::arr();
        for w in &self.widgets {
            let r = w.page.and_then(|p| doc.widget_rect(p, &w.rect));
            let round = |v: f64| (v * 100.0).round() / 100.0;
            widgets.push(
                Json::obj()
                    .with("widget", w.num)
                    .with("page", w.page.map(Json::from).unwrap_or(Json::Null))
                    .with("rect", r.map(|r| Json::Arr(vec![round(r.x).into(), round(r.y).into(), round(r.w).into(), round(r.h).into()])).unwrap_or(Json::Null))
                    .with("on", w.on.clone().map(Json::from).unwrap_or(Json::Null)),
            );
        }
        let mut options = Json::arr();
        for (e, d) in &self.options {
            options.push(Json::obj().with("value", e.as_str()).with("text", d.as_str()));
        }
        Json::obj()
            .with("name", self.name.as_str())
            .with("kind", self.kind.name())
            .with("value", if self.password() { Json::from("*".repeat(self.value.chars().count())) } else { Json::from(self.value.as_str()) })
            .with("checked", matches!(self.kind, Kind::Check | Kind::Radio) && self.value != "Off" && !self.value.is_empty())
            .with("options", options)
            .with("read_only", self.read_only())
            .with("multiline", self.multiline())
            .with("fillable", self.fillable())
            .with("widgets", widgets)
    }
}

#[derive(Clone, Default)]
struct Inh {
    ft: Option<String>,
    ff: i64,
    v: Option<Obj>,
    da: Option<String>,
    q: Option<i64>,
    opt: Option<Obj>,
    max_len: Option<i64>,
}

/// The catalog's /AcroForm.
pub fn acro_form(file: &PdfFile) -> Option<Dict> {
    let t = file.trailer();
    let root = file.resolve(t.get("Root")?);
    file.dict_of(root.as_dict()?, "AcroForm")
}

/// Every field, and whether the form asks viewers to make the appearances (/NeedAppearances).
pub fn collect(file: &PdfFile, pages: &[PageInfo]) -> (Vec<Field>, bool) {
    let Some(acro) = acro_form(file) else { return (Vec::new(), false) };
    let need = file.dget(&acro, "NeedAppearances").as_bool().unwrap_or(false);
    let mut annot_page: HashMap<u32, usize> = HashMap::new();
    let page_of: HashMap<u32, usize> = pages.iter().enumerate().filter(|(_, p)| p.num != 0).map(|(i, p)| (p.num, i)).collect();
    for (i, p) in pages.iter().enumerate() {
        if let Obj::Arr(a) = file.dget(&p.dict, "Annots") {
            for item in a.iter().take(10_000) {
                if let Some((n, _)) = item.as_ref() {
                    annot_page.entry(n).or_insert(i);
                }
            }
        }
    }
    let inh = Inh { da: file.dget(&acro, "DA").as_text(), ..Inh::default() };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Obj::Arr(fields) = file.dget(&acro, "Fields") {
        for f in fields.iter().take(10_000) {
            walk(file, f, "", &inh, &annot_page, &page_of, &mut seen, 0, &mut out);
        }
    }
    (out, need)
}

#[allow(clippy::too_many_arguments)]
fn walk(file: &PdfFile, node: &Obj, parent: &str, inh: &Inh, annot_page: &HashMap<u32, usize>, page_of: &HashMap<u32, usize>, seen: &mut HashSet<u32>, depth: u32, out: &mut Vec<Field>) {
    if depth > 32 || out.len() > 10_000 {
        return;
    }
    let num = node.as_ref().map_or(0, |r| r.0);
    if num != 0 && !seen.insert(num) {
        return;
    }
    let o = file.resolve(node);
    let Some(d) = o.as_dict() else { return };
    let mut inh = inh.clone();
    if let Some(ft) = file.dget(d, "FT").as_name() {
        inh.ft = Some(ft.to_string());
    }
    if let Some(ff) = file.dget(d, "Ff").as_i64() {
        inh.ff = ff;
    }
    if let Some(v) = d.get("V") {
        inh.v = Some(file.resolve(v));
    }
    if let Some(da) = file.dget(d, "DA").as_text() {
        inh.da = Some(da);
    }
    if let Some(q) = file.dget(d, "Q").as_i64() {
        inh.q = Some(q);
    }
    if let Some(opt) = d.get("Opt") {
        inh.opt = Some(file.resolve(opt));
    }
    if let Some(m) = file.dget(d, "MaxLen").as_i64() {
        inh.max_len = Some(m);
    }
    let partial = file.dget(d, "T").as_text();
    let name = match (&partial, parent.is_empty()) {
        (Some(t), true) => t.clone(),
        (Some(t), false) => format!("{}.{}", parent, t),
        (None, _) => parent.to_string(),
    };
    let kids: Vec<Obj> = file.dget(d, "Kids").as_arr().map(|a| a.to_vec()).unwrap_or_default();
    let field_kids: Vec<&Obj> = kids.iter().filter(|k| file.resolve(k).as_dict().map_or(false, |kd| kd.contains("T"))).collect();
    if !field_kids.is_empty() {
        for k in field_kids {
            walk(file, k, &name, &inh, annot_page, page_of, seen, depth + 1, out);
        }
        return;
    }
    // A terminal field: its widgets are itself (merged) and its kids.
    let mut widget_objs: Vec<(u32, Dict)> = Vec::new();
    if d.name("Subtype") == Some("Widget") || kids.is_empty() {
        widget_objs.push((num, d.clone()));
    }
    for k in &kids {
        if let Some(kd) = file.resolve(k).as_dict() {
            widget_objs.push((k.as_ref().map_or(0, |r| r.0), kd.clone()));
        }
    }
    let ff = inh.ff;
    let kind = match inh.ft.as_deref() {
        Some("Tx") => Kind::Text,
        Some("Btn") if ff & (1 << 16) != 0 => Kind::Push,
        Some("Btn") if ff & (1 << 15) != 0 => Kind::Radio,
        Some("Btn") => Kind::Check,
        Some("Ch") if ff & (1 << 17) != 0 => Kind::Combo,
        Some("Ch") => Kind::List,
        Some("Sig") => Kind::Signature,
        _ => return,
    };
    let mut widgets = Vec::new();
    for (wn, wd) in widget_objs {
        let r = file.dget(&wd, "Rect").nums();
        let rect = if r.len() >= 4 { [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])] } else { [0.0; 4] };
        let page = wd.get("P").and_then(Obj::as_ref).and_then(|(p, _)| page_of.get(&p).copied()).or_else(|| annot_page.get(&wn).copied());
        let ap_n = file.dict_of(&wd, "AP").map(|ap| file.dget(&ap, "N")).unwrap_or(Obj::Null);
        let on = match (&ap_n, kind) {
            (Obj::Dict(states), Kind::Check | Kind::Radio) => states.0.iter().map(|(k, _)| k.clone()).find(|k| k != "Off"),
            _ => None,
        }
        .or_else(|| if matches!(kind, Kind::Check | Kind::Radio) { file.dget(&wd, "AS").as_name().filter(|s| *s != "Off").map(str::to_string) } else { None })
        .or_else(|| if kind == Kind::Check { Some("Yes".to_string()) } else { None });
        widgets.push(Widget { num: wn, page, rect, on, has_ap: !ap_n.is_null() });
    }
    let value = match (&inh.v, kind) {
        (Some(Obj::Name(n)), _) => n.clone(),
        (Some(Obj::Str(s)), _) => super::obj::decode_text(s),
        (Some(Obj::Arr(a)), _) => a.first().and_then(|v| v.as_text()).unwrap_or_default(),
        (_, Kind::Check | Kind::Radio) => "Off".to_string(),
        _ => String::new(),
    };
    let options = match &inh.opt {
        Some(Obj::Arr(a)) => a
            .iter()
            .filter_map(|o| match file.resolve(o) {
                Obj::Str(s) => {
                    let t = super::obj::decode_text(&s);
                    Some((t.clone(), t))
                }
                Obj::Arr(pair) if pair.len() >= 2 => Some((file.resolve(&pair[0]).as_text().unwrap_or_default(), file.resolve(&pair[1]).as_text().unwrap_or_default())),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let name = if name.is_empty() { format!("field{}", num) } else { name };
    out.push(Field {
        name,
        num,
        kind,
        value,
        options,
        widgets,
        flags: ff,
        da: inh.da.unwrap_or_else(|| "/Helv 0 Tf 0 g".into()),
        q: inh.q.unwrap_or(0),
        max_len: inh.max_len.filter(|m| *m > 0).map(|m| m as usize),
    });
}

// ---------------------------------------------------------------------------------------------------
// Appearances
// ---------------------------------------------------------------------------------------------------

/// A field's default appearance: font resource name, size (0 = fit) and the colour operator.
#[derive(Debug, Clone, PartialEq)]
pub struct Da {
    pub font: String,
    pub size: f64,
    pub colour: String,
}

pub fn parse_da(s: &str) -> Da {
    let mut font = "Helv".to_string();
    let mut size = 0.0;
    let mut colour = "0 g".to_string();
    let mut nums: Vec<String> = Vec::new();
    let mut last_name: Option<String> = None;
    for tok in s.split_whitespace() {
        if let Some(n) = tok.strip_prefix('/') {
            last_name = Some(n.to_string());
            nums.clear();
        } else if tok.parse::<f64>().is_ok() {
            nums.push(tok.to_string());
        } else {
            match tok {
                "Tf" => {
                    if let Some(n) = last_name.take() {
                        font = n;
                    }
                    size = nums.last().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0).abs();
                }
                "g" | "rg" | "k" => {
                    let want = match tok {
                        "g" => 1,
                        "rg" => 3,
                        _ => 4,
                    };
                    if nums.len() >= want {
                        colour = format!("{} {}", nums[nums.len() - want..].join(" "), tok);
                    }
                }
                _ => {}
            }
            nums.clear();
        }
    }
    Da { font, size, colour }
}

/// The width of `s` in Helvetica at `size` points.
pub fn text_width(s: &str, size: f64) -> f64 {
    let f = Std14::Helvetica { bold: false, italic: false };
    s.chars().map(|c| f.width(ascii_name(c).unwrap_or(""))).sum::<f64>() * size / 1000.0
}

/// WinAnsiEncoding for a character ('?' when it has none).
pub fn win_ansi(c: char) -> u8 {
    let u = c as u32;
    if (0x20..0x7F).contains(&u) || (0xA0..=0xFF).contains(&u) {
        return u as u8;
    }
    const HIGH: [(char, u8); 27] = [
        ('€', 0x80), ('‚', 0x82), ('ƒ', 0x83), ('„', 0x84), ('…', 0x85), ('†', 0x86), ('‡', 0x87), ('ˆ', 0x88), ('‰', 0x89), ('Š', 0x8A),
        ('‹', 0x8B), ('Œ', 0x8C), ('Ž', 0x8E), ('‘', 0x91), ('’', 0x92), ('“', 0x93), ('”', 0x94), ('•', 0x95), ('–', 0x96), ('—', 0x97),
        ('˜', 0x98), ('™', 0x99), ('š', 0x9A), ('›', 0x9B), ('œ', 0x9C), ('ž', 0x9E), ('Ÿ', 0x9F),
    ];
    HIGH.iter().find(|(h, _)| *h == c).map(|(_, b)| *b).unwrap_or(b'?')
}

/// The size a text or combo field's text is drawn at (the /DA size, or fitted when it is 0).
pub fn shown_size(f: &Field, rect: &[f64; 4]) -> f64 {
    let da = parse_da(&f.da);
    if da.size > 0.0 {
        return da.size;
    }
    let h = (rect[3] - rect[1]).abs();
    let w = (rect[2] - rect[0]).abs();
    if f.multiline() || f.kind == Kind::List {
        return 10.0f64.min((h - 4.0).max(4.0));
    }
    let mut size = ((h - 3.0) * 0.72).clamp(4.0, 14.0);
    let unit = text_width(&f.shown_value(), 1.0);
    if unit > 0.0 {
        size = size.min((w - 4.0) / unit).max(4.0);
    }
    size
}

/// Words of `text` wrapped to `width` at `size` (explicit line breaks kept).
fn wrap(text: &str, width: f64, size: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split(' ') {
            let trial = if cur.is_empty() { word.to_string() } else { format!("{} {}", cur, word) };
            if !cur.is_empty() && text_width(&trial, size) > width {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = trial;
            }
        }
        lines.push(cur);
    }
    lines
}

fn lit(s: &str) -> Vec<u8> {
    let bytes: Vec<u8> = s.chars().map(win_ansi).collect();
    let mut out = Vec::new();
    write_string(&mut out, &bytes);
    out
}

/// The font a field's appearance uses: the form's /DR font of that name when it is one of the
/// standard 14 (so its widths are known), else Helvetica.
fn appearance_font(file: &PdfFile, acro: Option<&Dict>, name: &str) -> Obj {
    if let Some(dr) = acro.and_then(|a| file.dict_of(a, "DR")) {
        if let Some(fonts) = file.dict_of(&dr, "Font") {
            if let Some(f) = fonts.get(name) {
                let fd = file.resolve(f);
                if fd.as_dict().and_then(|d| d.name("BaseFont")).map_or(false, |b| Std14::from_name(b).is_some_and(|s| !s.is_symbolic())) {
                    return f.clone();
                }
            }
        }
    }
    Obj::Dict(Dict::new().with("Type", Obj::name("Font")).with("Subtype", Obj::name("Type1")).with("BaseFont", Obj::name("Helvetica")).with("Encoding", Obj::name("WinAnsiEncoding")))
}

fn form_xobject(w: f64, h: f64, resources: Option<Dict>, content: Vec<u8>) -> Stream {
    let mut d = Dict::new()
        .with("Type", Obj::name("XObject"))
        .with("Subtype", Obj::name("Form"))
        .with("BBox", Obj::Arr(vec![Obj::Int(0), Obj::Int(0), Obj::Real(w), Obj::Real(h)]));
    if let Some(r) = resources {
        d.set("Resources", Obj::Dict(r));
    }
    Stream { dict: d, data: content }
}

/// The normal appearance of a text, combo or list field's widget showing `f`'s value.
pub fn text_appearance(file: &PdfFile, f: &Field, rect: &[f64; 4]) -> Stream {
    let acro = acro_form(file);
    let da = parse_da(&f.da);
    let w = (rect[2] - rect[0]).abs().max(1.0);
    let h = (rect[3] - rect[1]).abs().max(1.0);
    let size = shown_size(f, rect);
    let n = fmt_num;
    let mut c: Vec<u8> = Vec::new();
    c.extend_from_slice(format!("/Tx BMC\nq\n1 1 {} {} re W n\n", n(w - 2.0), n(h - 2.0)).as_bytes());
    let font_line = format!("/{} {} Tf\n", da.font, n(size));
    let lead = size * 1.15;
    let line_at = |c: &mut Vec<u8>, text: &str, x: f64, y: f64| {
        c.extend_from_slice(format!("1 0 0 1 {} {} Tm ", n(x), n(y)).as_bytes());
        c.extend_from_slice(&lit(text));
        c.extend_from_slice(b" Tj\n");
    };
    let x_for = |tw: f64| match f.q {
        1 => (w - tw) / 2.0,
        2 => w - 2.0 - tw,
        _ => 2.0,
    };
    if f.kind == Kind::List {
        // Every choice a row; the chosen one highlighted.
        let mut y = h - 2.0 - lead;
        for (e, _) in &f.options {
            if *e == f.value {
                c.extend_from_slice(format!("0.6 0.75 0.95 rg 1 {} {} {} re f\n", n(y - size * 0.25), n(w - 2.0), n(lead)).as_bytes());
            }
            y -= lead;
            if y < -lead {
                break;
            }
        }
        c.extend_from_slice(format!("BT\n{}\n{}", da.colour, font_line).as_bytes());
        let mut y = h - 2.0 - lead + size * 0.15;
        for (_, d) in &f.options {
            line_at(&mut c, d, 2.0, y);
            y -= lead;
            if y < -lead {
                break;
            }
        }
        c.extend_from_slice(b"ET\n");
    } else {
        let shown = if f.password() { "*".repeat(f.value.chars().count()) } else { f.shown_value() };
        c.extend_from_slice(format!("BT\n{}\n{}", da.colour, font_line).as_bytes());
        if f.multiline() {
            let mut y = h - 2.0 - size * 0.9;
            for line in wrap(&shown, w - 4.0, size) {
                line_at(&mut c, &line, x_for(text_width(&line, size)), y);
                y -= lead;
                if y < -lead {
                    break;
                }
            }
        } else {
            let one = shown.replace('\n', " ");
            let y = (h - size * 0.72) / 2.0;
            line_at(&mut c, &one, x_for(text_width(&one, size)), y);
        }
        c.extend_from_slice(b"ET\n");
    }
    c.extend_from_slice(b"Q\nEMC");
    let res = Dict::new().with("Font", Obj::Dict(Dict::new().with(&da.font, appearance_font(file, acro.as_ref(), &da.font))));
    form_xobject(w, h, Some(res), c)
}

/// A check box's (a tick) or radio button's (a dot) "on" appearance, and the empty "off" one.
pub fn check_appearance(rect: &[f64; 4], radio: bool) -> (Stream, Stream) {
    let w = (rect[2] - rect[0]).abs().max(1.0);
    let h = (rect[3] - rect[1]).abs().max(1.0);
    let s = w.min(h);
    let (ox, oy) = ((w - s) / 2.0, (h - s) / 2.0);
    let n = fmt_num;
    let on = if radio {
        let (cx, cy, r) = (ox + s / 2.0, oy + s / 2.0, s * 0.25);
        let k = r * 0.5523;
        format!(
            "q 0 0 0 rg {} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c f Q",
            n(cx + r), n(cy),
            n(cx + r), n(cy + k), n(cx + k), n(cy + r), n(cx), n(cy + r),
            n(cx - k), n(cy + r), n(cx - r), n(cy + k), n(cx - r), n(cy),
            n(cx - r), n(cy - k), n(cx - k), n(cy - r), n(cx), n(cy - r),
            n(cx + k), n(cy - r), n(cx + r), n(cy - k), n(cx + r), n(cy)
        )
    } else {
        format!(
            "q 0 0 0 RG {} w 1 J 1 j {} {} m {} {} l {} {} l S Q",
            n(s * 0.12),
            n(ox + s * 0.2),
            n(oy + s * 0.5),
            n(ox + s * 0.42),
            n(oy + s * 0.25),
            n(ox + s * 0.8),
            n(oy + s * 0.78)
        )
    };
    (form_xobject(w, h, None, on.into_bytes()), form_xobject(w, h, None, Vec::new()))
}

/// For the screen only: a widget's dictionary with an appearance made here, when the form asks for
/// one (/NeedAppearances) or the widget has none. `None` keeps the file's own.
pub fn shown_widget(file: &PdfFile, f: &Field, w: &Widget, wd: &Dict, need: bool) -> Option<Dict> {
    let mut d = wd.clone();
    match f.kind {
        Kind::Text | Kind::Combo | Kind::List => {
            if !(need || !w.has_ap) {
                return None;
            }
            let s = text_appearance(file, f, &w.rect);
            d.set("AP", Obj::Dict(Dict::new().with("N", Obj::Stream(Arc::new(s)))));
        }
        Kind::Check | Kind::Radio => {
            if w.has_ap {
                return None;
            }
            let on = w.on.clone().unwrap_or_else(|| "Yes".into());
            let (s_on, s_off) = check_appearance(&w.rect, f.kind == Kind::Radio);
            d.set("AP", Obj::Dict(Dict::new().with("N", Obj::Dict(Dict::new().with(&on, Obj::Stream(Arc::new(s_on))).with("Off", Obj::Stream(Arc::new(s_off)))))));
            d.set("AS", Obj::name(if f.value == on { &on } else { "Off" }));
        }
        _ => return None,
    }
    Some(d)
}

fn truthy(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" | "checked" | "x" => Some(true),
        "false" | "off" | "no" | "0" | "" | "unchecked" => Some(false),
        _ => None,
    }
}

impl FolioDoc {
    /// The form's fields.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Put an appearance stream on a widget: into the object made for it before, else a new one.
    fn widget_ap(&mut self, wd: &Dict, ap: Obj) -> Obj {
        // Reuse the streams this session made (typing would otherwise leave one per key).
        fn reuse(created: &std::collections::BTreeSet<u32>, o: Option<&Obj>) -> Option<u32> {
            match o {
                Some(Obj::Ref(n, _)) if created.contains(n) => Some(*n),
                _ => None,
            }
        }
        let old = self.file.dict_of(wd, "AP").and_then(|a| a.get("N").cloned());
        match ap {
            Obj::Stream(s) => {
                let n = match reuse(&self.created, old.as_ref()) {
                    Some(n) => {
                        self.file.overlay.insert(n, Obj::Stream(s));
                        n
                    }
                    None => self.new_object(Obj::Stream(s)),
                };
                Obj::Ref(n, 0)
            }
            Obj::Dict(states) => {
                // A check box's states: reuse the state streams this session made, by name.
                let olds = old.map(|o| self.file.resolve(&o)).and_then(|o| o.as_dict().cloned()).unwrap_or_default();
                let mut out = Dict::new();
                for (k, v) in states.0 {
                    let n = match reuse(&self.created, olds.get(&k)) {
                        Some(n) => {
                            self.file.overlay.insert(n, v);
                            n
                        }
                        None => self.new_object(v),
                    };
                    out.set(&k, Obj::Ref(n, 0));
                }
                Obj::Dict(out)
            }
            other => other,
        }
    }

    /// Fill field `name` with `value`: text for a text field, the choice (its value or its shown text)
    /// for a combo or list, true/false (or the state name) for a check box, the state name of one
    /// button for a radio group ("Off" for none).
    pub fn set_field(&mut self, name: &str, value: &str) -> Result<(), String> {
        let f = self.fields.iter().find(|f| f.name == name).cloned().ok_or_else(|| {
            let names: Vec<&str> = self.fields.iter().take(30).map(|f| f.name.as_str()).collect();
            if names.is_empty() {
                "this document has no form fields".to_string()
            } else {
                format!("no field '{}'; the fields are: {}", name, names.join(", "))
            }
        })?;
        if f.num == 0 {
            return Err(format!("field '{}' cannot be filled (it is not an object of its own)", name));
        }
        if f.read_only() {
            return Err(format!("field '{}' is read-only", name));
        }
        let new_v: Obj;
        let mut filled = f.clone();
        match f.kind {
            Kind::Text => {
                let mut v: String = if f.multiline() { value.replace("\r\n", "\n").replace('\r', "\n") } else { value.replace(['\r', '\n'], " ") };
                if let Some(m) = f.max_len {
                    v = v.chars().take(m).collect();
                }
                new_v = Obj::Str(encode_text(&v));
                filled.value = v;
            }
            Kind::Check => {
                let on = f.widgets.iter().find_map(|w| w.on.clone()).unwrap_or_else(|| "Yes".into());
                let state = if value == on {
                    on.clone()
                } else {
                    match truthy(value) {
                        Some(true) => on.clone(),
                        Some(false) => "Off".into(),
                        None => return Err(format!("check box '{}' takes true or false", name)),
                    }
                };
                new_v = Obj::name(&state);
                filled.value = state;
            }
            Kind::Radio => {
                let states: Vec<String> = f.widgets.iter().filter_map(|w| w.on.clone()).collect();
                let state = if truthy(value) == Some(false) {
                    "Off".to_string()
                } else if let Some(s) = states.iter().find(|s| **s == value) {
                    s.clone()
                } else if let Some(i) = f.options.iter().position(|(e, d)| e == value || d == value) {
                    states.get(i).cloned().ok_or_else(|| format!("radio group '{}' has no button {}", name, value))?
                } else {
                    return Err(format!("radio group '{}' takes one of: {}", name, states.join(", ")));
                };
                new_v = Obj::name(&state);
                filled.value = state;
            }
            Kind::Combo | Kind::List => {
                let chosen = f.options.iter().find(|(e, d)| e == value || d == value).map(|(e, _)| e.clone());
                let v = match chosen {
                    Some(e) => e,
                    None if f.editable() || f.options.is_empty() || value.is_empty() => value.to_string(),
                    None => {
                        let shown: Vec<&str> = f.options.iter().take(30).map(|(_, d)| d.as_str()).collect();
                        return Err(format!("'{}' is not a choice of '{}'; the choices are: {}", value, name, shown.join(", ")));
                    }
                };
                new_v = Obj::Str(encode_text(&v));
                filled.value = v;
            }
            Kind::Push | Kind::Signature => return Err(format!("field '{}' is a {} and cannot be filled", name, f.kind.name())),
        }
        let mut fd = self.file.get(f.num).as_dict().cloned().ok_or("the field object is missing")?;
        fd.set("V", new_v);
        self.set_object(f.num, Obj::Dict(fd));
        let mut pages = Vec::new();
        for w in &f.widgets {
            if w.num == 0 {
                continue;
            }
            let Some(mut wd) = self.file.get(w.num).as_dict().cloned() else { continue };
            match f.kind {
                Kind::Check | Kind::Radio => {
                    let on = w.on.clone().unwrap_or_else(|| "Yes".into());
                    if !w.has_ap {
                        let (s_on, s_off) = check_appearance(&w.rect, f.kind == Kind::Radio);
                        let states = Dict::new().with(&on, Obj::Stream(Arc::new(s_on))).with("Off", Obj::Stream(Arc::new(s_off)));
                        let n = self.widget_ap(&wd, Obj::Dict(states));
                        let mut ap = self.file.dict_of(&wd, "AP").unwrap_or_default();
                        ap.set("N", n);
                        wd.set("AP", Obj::Dict(ap));
                    }
                    wd.set("AS", Obj::name(if filled.value == on { &on } else { "Off" }));
                }
                _ => {
                    let s = text_appearance(&self.file, &filled, &w.rect);
                    let n = self.widget_ap(&wd, Obj::Stream(Arc::new(s)));
                    let mut ap = self.file.dict_of(&wd, "AP").unwrap_or_default();
                    ap.set("N", n);
                    wd.set("AP", Obj::Dict(ap));
                }
            }
            self.set_object(w.num, Obj::Dict(wd));
            if let Some(p) = w.page {
                if !pages.contains(&p) {
                    pages.push(p);
                }
            }
        }
        self.reload_structure();
        for p in pages {
            self.rerender(p);
        }
        Ok(())
    }

    /// What typing does in a field the member clicked: a text field (or a combo box one may type
    /// into) takes the text at its end; a space ticks or clears a check box and picks a radio button;
    /// a list or combo box picks the next choice starting with the typed letters.
    pub(crate) fn type_into(&mut self, at: &At, text: &str) -> Result<(), String> {
        let At::Field { name, widget, .. } = at else { return Err("click a field first".into()) };
        let f = self.fields.iter().find(|f| &f.name == name).cloned().ok_or_else(|| format!("no field '{}'", name))?;
        if text.is_empty() {
            return Ok(());
        }
        match f.kind {
            Kind::Text => {
                if (text == "\n" || text == "\r\n") && !f.multiline() {
                    return Ok(());
                }
                if f.max_len.map_or(false, |m| f.value.chars().count() >= m) {
                    return Ok(());
                }
                let v = format!("{}{}", f.value, text);
                self.set_field(name, &v)
            }
            Kind::Combo if f.editable() => {
                let v = format!("{}{}", f.value, text.replace('\n', ""));
                self.set_field(name, &v)
            }
            Kind::Combo | Kind::List => {
                let t = text.trim().to_lowercase();
                if t.is_empty() || f.options.is_empty() {
                    return Ok(());
                }
                let starts: Vec<&(String, String)> = f.options.iter().filter(|(_, d)| d.to_lowercase().starts_with(&t)).collect();
                let Some(first) = starts.first() else { return Ok(()) };
                // Pressing the same letter again walks through the choices starting with it.
                let pick = starts.iter().position(|(e, _)| *e == f.value).and_then(|i| starts.get(i + 1)).unwrap_or(first);
                let v = pick.0.clone();
                self.set_field(name, &v)
            }
            Kind::Check => {
                if text != " " && text.to_lowercase() != "x" {
                    return Ok(());
                }
                let to = if f.value == "Off" { "true" } else { "false" };
                self.set_field(name, to)
            }
            Kind::Radio => {
                if text != " " {
                    return Ok(());
                }
                let on = f.widgets.iter().find(|w| w.num == *widget).and_then(|w| w.on.clone()).or_else(|| f.widgets.iter().find_map(|w| w.on.clone()));
                match on {
                    Some(on) => self.set_field(name, &on),
                    None => Ok(()),
                }
            }
            Kind::Push | Kind::Signature => Ok(()),
        }
    }

    /// Backspace in a field: the last character off a text field (or a combo box one types into).
    pub(crate) fn erase(&mut self, at: &At) -> Result<(), String> {
        let At::Field { name, .. } = at else { return Err("click a field first".into()) };
        let f = self.fields.iter().find(|f| &f.name == name).cloned().ok_or_else(|| format!("no field '{}'", name))?;
        if f.kind == Kind::Text || f.editable() {
            if f.value.is_empty() {
                return Ok(());
            }
            let mut v = f.value.clone();
            v.pop();
            return self.set_field(name, &v);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn da_strings_parse() {
        assert_eq!(parse_da("/Helv 12 Tf 0 g"), Da { font: "Helv".into(), size: 12.0, colour: "0 g".into() });
        assert_eq!(parse_da("0 0 1 rg /TiRo 0 Tf"), Da { font: "TiRo".into(), size: 0.0, colour: "0 0 1 rg".into() });
        assert_eq!(parse_da(""), Da { font: "Helv".into(), size: 0.0, colour: "0 g".into() });
    }

    #[test]
    fn widths_and_wrapping() {
        assert!((text_width("Hello", 10.0) - 22.78).abs() < 0.01, "{}", text_width("Hello", 10.0));
        assert_eq!(wrap("one two three", text_width("one two", 10.0) + 0.1, 10.0), vec!["one two", "three"]);
        assert_eq!(wrap("a\nb", 100.0, 10.0), vec!["a", "b"]);
        assert_eq!(win_ansi('é'), 0xE9);
        assert_eq!(win_ansi('€'), 0x80);
        assert_eq!(win_ansi('中'), b'?');
    }
}
