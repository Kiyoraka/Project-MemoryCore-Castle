//! The number format engine: Excel's format codes, interpreted by us.
//!
//! - Sections `positive;negative;zero;text`, conditions (`[>100]`), colours (`[Red]`, `[Color10]`).
//! - Digit placeholders `0 # ?`, the decimal point, thousands separators and scaling commas,
//!   percent, scientific `E+`/`E-` (engineering when the integer part is `##0`), fractions
//!   (`# ?/?`, `# ??/??`, fixed denominators `?/8`), literals (`"..."`, `\x`, the plain ones), `@`,
//!   `_x` padding (one space) and `*x` fill (reported so the grid can repeat it to the cell's width).
//! - Dates and times: `yyyy yy mmmm mmm mm m dddd ddd dd d hh h mm ss AM/PM A/P .0`, elapsed
//!   `[h] [mm] [ss]`, on Excel's serial dates (the 1900 leap-year bug kept, the 1904 system on
//!   request).
//! - General: up to 11 characters, as Excel shows a number in a standard column.
//!
//! Numbers are taken to 15 significant digits first (Excel's display precision) and rounded half
//! away from zero in decimal, so `0.1+0.2` shows `0.3` and `1.005` with `0.00` shows `1.01`.
//!
//! Not yet: locale tags beyond the currency text of `[$€-x]`, calendars other than Gregorian
//! (`B2`, the Japanese era), `[DBNum]` numerals.

use super::model::CellValue;
use crate::ground::display::Color;

/// A formatted value.
#[derive(Debug, Clone, PartialEq)]
pub struct Formatted {
    pub text: String,
    /// The section's colour (`[Red]`), if it has one.
    pub color: Option<Color>,
    /// A `*x` fill: the byte offset in `text` where `x` repeats to fill the cell, and `x`.
    pub fill: Option<(usize, char)>,
    /// True when the value cannot be shown in this format (a negative date): show `#####`.
    pub hashes: bool,
}

impl Formatted {
    fn plain(text: String) -> Formatted {
        Formatted { text, color: None, fill: None, hashes: false }
    }

    fn hashes() -> Formatted {
        Formatted { text: "#".repeat(11), color: None, fill: None, hashes: true }
    }
}

/// Formats a value with a format code (1900 date system). The `*` fill character is left out; use
/// [`format_value_full`] to get its position.
pub fn format_value(value: &CellValue, code: &str) -> (String, Option<Color>) {
    let f = format_value_full(value, code, false);
    (f.text, f.color)
}

/// Formats a value with a format code, in the 1900 or 1904 date system.
pub fn format_value_full(value: &CellValue, code: &str, date1904: bool) -> Formatted {
    NumberFormat::parse(code).format(value, date1904)
}

/// The format code of a built-in `numFmtId` (en-US), `None` for an id with no built-in code.
pub fn builtin_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "$#,##0_);($#,##0)",
        6 => "$#,##0_);[Red]($#,##0)",
        7 => "$#,##0.00_);($#,##0.00)",
        8 => "$#,##0.00_);[Red]($#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "m/d/yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yyyy h:mm",
        27..=36 | 50..=58 => "m/d/yyyy",
        37 => "#,##0_);(#,##0)",
        38 => "#,##0_);[Red](#,##0)",
        39 => "#,##0.00_);(#,##0.00)",
        40 => "#,##0.00_);[Red](#,##0.00)",
        41 => r#"_(* #,##0_);_(* \(#,##0\);_(* "-"_);_(@_)"#,
        42 => r#"_("$"* #,##0_);_("$"* \(#,##0\);_("$"* "-"_);_(@_)"#,
        43 => r#"_(* #,##0.00_);_(* \(#,##0.00\);_(* "-"??_);_(@_)"#,
        44 => r#"_("$"* #,##0.00_);_("$"* \(#,##0.00\);_("$"* "-"??_);_(@_)"#,
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// True when the code formats numbers as dates or times (its first section).
pub fn is_date_format(code: &str) -> bool {
    NumberFormat::parse(code).sections.first().is_some_and(|s| s.kind == Kind::Date)
}

// ---------------------------------------------------------------------------------------------------
// Decimal digits
// ---------------------------------------------------------------------------------------------------

/// |x| to 15 significant digits: the digits (first one nonzero) and the power of ten of the first.
/// Zero gives `([0], 0)`.
fn sig15(x: f64) -> (Vec<u8>, i32) {
    let a = x.abs();
    if a == 0.0 || !a.is_finite() {
        return (vec![0], 0);
    }
    let s = format!("{:.14e}", a);
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let digits: Vec<u8> = m.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    (digits, e.parse().unwrap_or(0))
}

/// Rounds the decimal `digits` x 10^exp (first digit's power) at `keep` digits, half away from zero.
fn round_at(mut d: Vec<u8>, mut e: i32, keep: i64) -> (Vec<u8>, i32) {
    if keep < 0 {
        return (Vec::new(), e);
    }
    let keep = keep as usize;
    if keep < d.len() {
        let up = d[keep] >= 5;
        d.truncate(keep);
        if up {
            let mut i = keep;
            loop {
                if i == 0 {
                    d.insert(0, 1);
                    e += 1;
                    break;
                }
                i -= 1;
                if d[i] == 9 {
                    d[i] = 0;
                } else {
                    d[i] += 1;
                    break;
                }
            }
        }
    }
    (d, e)
}

/// Integer digits (no leading zeros; empty for 0) and exactly `n` decimals of a decimal number.
fn split_dec(d: &[u8], e: i32, n: usize) -> (String, String) {
    let (d, e) = round_at(d.to_vec(), e, e as i64 + n as i64 + 1);
    let digit = |k: i64| -> char {
        if k >= 0 {
            (b'0' + d.get(k as usize).copied().unwrap_or(0)) as char
        } else {
            '0'
        }
    };
    let mut int = String::new();
    if e >= 0 {
        for k in 0..=e as i64 {
            int.push(digit(k));
        }
    }
    let int = int.trim_start_matches('0').to_string();
    let mut frac = String::new();
    for j in 1..=n as i64 {
        frac.push(digit(e as i64 + j));
    }
    (int, frac)
}

/// |v| rounded to `n` decimals: integer digits and decimals.
fn round_dec(v: f64, n: usize) -> (String, String) {
    let (d, e) = sig15(v);
    split_dec(&d, e, n)
}

/// |v| to `p` significant digits.
fn round_sig(v: f64, p: usize) -> (Vec<u8>, i32) {
    let (d, e) = sig15(v);
    let (mut d, e) = round_at(d, e, p as i64);
    d.resize(p, 0);
    (d, e)
}

fn sign(v: f64) -> &'static str {
    if v < 0.0 {
        "-"
    } else {
        ""
    }
}

/// JavaScript's `toFixed` on 15 digits.
fn to_fixed(v: f64, n: usize) -> String {
    let (i, f) = round_dec(v, n);
    let i = if i.is_empty() { "0".to_string() } else { i };
    if n == 0 {
        format!("{}{}", sign(v), i)
    } else {
        format!("{}{}.{}", sign(v), i, f)
    }
}

fn digits_str(d: &[u8]) -> String {
    d.iter().map(|x| (b'0' + x) as char).collect()
}

/// JavaScript's `toExponential(n)`: `d.ddde+x`.
fn to_exponential(v: f64, n: usize) -> String {
    if v == 0.0 {
        return format!("0{}e+0", if n > 0 { format!(".{}", "0".repeat(n)) } else { String::new() });
    }
    let (d, e) = round_sig(v, n + 1);
    let s = digits_str(&d);
    let m = if n > 0 { format!("{}.{}", &s[..1], &s[1..]) } else { s[..1].to_string() };
    format!("{}{}e{}{}", sign(v), m, if e < 0 { '-' } else { '+' }, e.abs())
}

/// JavaScript's `toPrecision(p)`.
fn to_precision(v: f64, p: usize) -> String {
    let p = p.max(1);
    if v == 0.0 {
        return to_fixed(0.0, p - 1);
    }
    let (d, e) = round_sig(v, p);
    if e < -6 || e >= p as i32 {
        return to_exponential(v, p - 1);
    }
    let s = digits_str(&d);
    let body = if e >= 0 {
        let k = e as usize + 1;
        if k >= s.len() {
            s.clone()
        } else {
            format!("{}.{}", &s[..k], &s[k..])
        }
    } else {
        format!("0.{}{}", "0".repeat((-e - 1) as usize), s)
    };
    format!("{}{}", sign(v), body)
}

/// Drops trailing zeros of a decimal (and of an exponent form's mantissa).
fn strip_decimal(o: &str) -> String {
    let (m, ex) = match o.find(['E', 'e']) {
        Some(i) => (&o[..i], &o[i..]),
        None => (o, ""),
    };
    let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.') } else { m };
    format!("{}{}", m, ex)
}

/// `1e+5` -> `1E+05`.
fn normalize_exp(o: &str) -> String {
    match o.find(['E', 'e']) {
        Some(i) => {
            let (m, rest) = (&o[..i], &o[i + 1..]);
            let (sg, digits) = match rest.chars().next() {
                Some(c @ ('+' | '-')) => (c, &rest[1..]),
                _ => ('+', rest),
            };
            format!("{}E{}{:0>2}", m, sg, digits)
        }
        None => o.to_string(),
    }
}

/// Excel's General format: as many digits as fit in 11 characters.
pub fn general(v: f64) -> String {
    if !v.is_finite() {
        return "#NUM!".into();
    }
    if v == 0.0 {
        return "0".into();
    }
    let w = if v < 0.0 { 12 } else { 11 };
    let (_, e) = sig15(v);
    let o = if (-4..=-1).contains(&e) {
        to_precision(v, (10 + e) as usize)
    } else if e.abs() <= 9 {
        let o = strip_decimal(&to_fixed(v, 12));
        if o.len() <= w {
            o
        } else {
            let o = to_precision(v, 10);
            if strip_decimal(&o).len() <= w {
                o
            } else {
                to_exponential(v, 5)
            }
        }
    } else if e == 10 {
        let s = to_fixed(v, 10);
        s.chars().take(12).collect()
    } else {
        let o = strip_decimal(&to_fixed(v, 11));
        if o.len() > w || o == "0" || o == "-0" {
            to_precision(v, 6)
        } else {
            o
        }
    };
    strip_decimal(&normalize_exp(&o.to_uppercase()))
}

/// General squeezed into at most `max_chars` characters (a narrow column), or `None` when even
/// the shortest exponent form does not fit.
pub fn general_fit(v: f64, max_chars: usize) -> Option<String> {
    let g = general(v);
    if g.chars().count() <= max_chars {
        return Some(g);
    }
    for p in (1..=10).rev() {
        let s = strip_decimal(&normalize_exp(&to_precision(v, p).to_uppercase()));
        if s.chars().count() <= max_chars && !s.contains('E') {
            return Some(s);
        }
    }
    for n in (0..=5).rev() {
        let s = strip_decimal(&normalize_exp(&to_exponential(v, n).to_uppercase()));
        if s.chars().count() <= max_chars {
            return Some(s);
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------------
// Serial dates
// ---------------------------------------------------------------------------------------------------

/// Days since 1970-01-01 to (year, month, day) (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day) to days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A serial day number as (year, month, day, weekday 0 = Sunday), Excel's way: in the 1900 system
/// serial 0 is "1900-01-00", 60 is the 29 February 1900 that never was, and the weekdays before
/// March 1900 follow Excel's count.
pub fn serial_to_ymd(days: i64, date1904: bool) -> (i64, u32, u32, u32) {
    if date1904 {
        let z = days + days_from_civil(1904, 1, 1);
        let (y, m, d) = civil_from_days(z);
        return (y, m, d, (z + 4).rem_euclid(7) as u32);
    }
    let wd = (days + 6).rem_euclid(7) as u32;
    if days == 0 {
        return (1900, 1, 0, wd);
    }
    if days == 60 {
        return (1900, 2, 29, wd);
    }
    let base = if days < 60 { days_from_civil(1899, 12, 31) } else { days_from_civil(1899, 12, 30) };
    let (y, m, d) = civil_from_days(base + days);
    (y, m, d, wd)
}

/// (year, month, day) to Excel's serial day number (inverse of [`serial_to_ymd`]).
pub fn ymd_to_serial(y: i64, m: u32, d: u32, date1904: bool) -> i64 {
    let z = days_from_civil(y, m, d);
    if date1904 {
        return z - days_from_civil(1904, 1, 1);
    }
    let s = z - days_from_civil(1899, 12, 30);
    if s <= 60 {
        s - 1
    } else {
        s
    }
}

/// Reads an ISO 8601 date-time (`t="d"` cells: `2026-11-26`, `2026-11-26T08:30:00`, `T08:30`) as a
/// serial number.
pub fn iso_to_serial(s: &str, date1904: bool) -> Option<f64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = match s.find('T') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let mut serial = 0.0;
    if !date.is_empty() {
        let mut it = date.split('-');
        let y: i64 = it.next()?.parse().ok()?;
        let m: u32 = it.next()?.parse().ok()?;
        let d: u32 = it.next().unwrap_or("1").parse().ok()?;
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        serial = ymd_to_serial(y, m, d, date1904) as f64;
    }
    if !time.is_empty() {
        let mut it = time.split(':');
        let h: f64 = it.next()?.parse().ok()?;
        let mi: f64 = it.next().unwrap_or("0").parse().ok()?;
        let sec: f64 = it.next().unwrap_or("0").parse().ok()?;
        serial += (h * 3600.0 + mi * 60.0 + sec) / 86400.0;
    }
    Some(serial)
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

// ---------------------------------------------------------------------------------------------------
// Parsing format codes
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Lit(String),
    /// `0`, `#` or `?`.
    Digit(char),
    Point,
    Comma,
    Percent,
    Exp { plus: bool, upper: bool },
    Slash,
    At,
    Fill(char),
    Pad(char),
    General,
    Year(usize),
    Month(usize),
    Minute(usize),
    Day(usize),
    Hour(usize),
    Second(usize),
    /// `AM/PM` (`full`) or `A/P`, with the case of its first letter.
    AmPm { full: bool, upper: bool },
    /// `[h]`, `[mm]`, `[ss]`: the char and how many.
    Elapsed(char, usize),
    SubSec(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Cmp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl Cmp {
    fn test(self, v: f64, x: f64) -> bool {
        match self {
            Cmp::Lt => v < x,
            Cmp::Le => v <= x,
            Cmp::Gt => v > x,
            Cmp::Ge => v >= x,
            Cmp::Eq => v == x,
            Cmp::Ne => v != x,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Number,
    Date,
    Text,
    General,
}

#[derive(Debug, Clone)]
struct Section {
    toks: Vec<Tok>,
    color: Option<Color>,
    cond: Option<(Cmp, f64)>,
    kind: Kind,
}

/// A parsed format code.
#[derive(Debug, Clone)]
pub struct NumberFormat {
    sections: Vec<Section>,
}

fn split_sections(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = code.chars();
    let (mut in_q, mut in_b) = (false, false);
    while let Some(c) = it.next() {
        if in_q {
            cur.push(c);
            in_q = c != '"';
            continue;
        }
        if in_b {
            cur.push(c);
            in_b = c != ']';
            continue;
        }
        match c {
            '"' => {
                in_q = true;
                cur.push(c);
            }
            '[' => {
                in_b = true;
                cur.push(c);
            }
            '\\' | '_' | '*' => {
                cur.push(c);
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            ';' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// The legacy 56-colour palette `[ColorN]` and indexed colours use (index 8 = Color1).
pub const INDEXED: [u32; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF,
    0xFFFF00, 0xFF00FF, 0x00FFFF, 0x800000, 0x008000, 0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, 0x9999FF, 0x993366,
    0xFFFFCC, 0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF, 0x800080, 0x800000, 0x008080,
    0x0000FF, 0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99, 0x99CCFF, 0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00,
    0xFF9900, 0xFF6600, 0x666699, 0x969696, 0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399, 0x333333,
];

pub fn hex_color(v: u32) -> Color {
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn starts_ci(ch: &[char], i: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    i + w.len() <= ch.len() && ch[i..i + w.len()].iter().zip(&w).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn is_date_tok(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Year(_) | Tok::Month(_) | Tok::Minute(_) | Tok::Day(_) | Tok::Hour(_) | Tok::Second(_) | Tok::AmPm { .. } | Tok::Elapsed(..) | Tok::SubSec(_)
    )
}

fn parse_section(s: &str) -> Section {
    let ch: Vec<char> = s.chars().collect();
    let n = ch.len();
    let mut toks: Vec<Tok> = Vec::new();
    let mut color = None;
    let mut cond = None;
    let mut i = 0;
    let run = |i: usize, c: char| -> usize {
        let mut j = i;
        while j < n && ch[j].eq_ignore_ascii_case(&c) {
            j += 1;
        }
        j - i
    };
    while i < n {
        let c = ch[i];
        match c {
            '"' => {
                let mut j = i + 1;
                let mut lit = String::new();
                while j < n && ch[j] != '"' {
                    lit.push(ch[j]);
                    j += 1;
                }
                toks.push(Tok::Lit(lit));
                i = j + 1;
            }
            '\\' => {
                if i + 1 < n {
                    toks.push(Tok::Lit(ch[i + 1].to_string()));
                }
                i += 2;
            }
            '_' => {
                if i + 1 < n {
                    toks.push(Tok::Pad(ch[i + 1]));
                }
                i += 2;
            }
            '*' => {
                if i + 1 < n {
                    toks.push(Tok::Fill(ch[i + 1]));
                }
                i += 2;
            }
            '[' => {
                let Some(off) = ch[i + 1..].iter().position(|&x| x == ']') else {
                    toks.push(Tok::Lit(ch[i..].iter().collect()));
                    break;
                };
                let content: String = ch[i + 1..i + 1 + off].iter().collect();
                i += off + 2;
                let low = content.to_ascii_lowercase();
                let named = match low.as_str() {
                    "black" => Some(0x000000),
                    "white" => Some(0xFFFFFF),
                    "red" => Some(0xFF0000),
                    "green" => Some(0x00FF00),
                    "blue" => Some(0x0000FF),
                    "yellow" => Some(0xFFFF00),
                    "magenta" => Some(0xFF00FF),
                    "cyan" => Some(0x00FFFF),
                    _ => None,
                };
                if let Some(v) = named {
                    color = Some(hex_color(v));
                } else if let Some(num) = low.strip_prefix("color") {
                    if let Ok(k) = num.trim().parse::<usize>() {
                        if (1..=56).contains(&k) {
                            color = Some(hex_color(INDEXED[k + 7]));
                        }
                    }
                } else if low.starts_with(['<', '>', '=']) {
                    let (op, rest) = if let Some(r) = low.strip_prefix("<=") {
                        (Cmp::Le, r)
                    } else if let Some(r) = low.strip_prefix(">=") {
                        (Cmp::Ge, r)
                    } else if let Some(r) = low.strip_prefix("<>") {
                        (Cmp::Ne, r)
                    } else if let Some(r) = low.strip_prefix('<') {
                        (Cmp::Lt, r)
                    } else if let Some(r) = low.strip_prefix('>') {
                        (Cmp::Gt, r)
                    } else {
                        (Cmp::Eq, &low[1..])
                    };
                    if let Ok(x) = rest.trim().parse::<f64>() {
                        cond = Some((op, x));
                    }
                } else if !low.is_empty() && low.chars().all(|x| x == 'h') {
                    toks.push(Tok::Elapsed('h', low.len()));
                } else if !low.is_empty() && low.chars().all(|x| x == 'm') {
                    toks.push(Tok::Elapsed('m', low.len()));
                } else if !low.is_empty() && low.chars().all(|x| x == 's') {
                    toks.push(Tok::Elapsed('s', low.len()));
                } else if let Some(cur) = content.strip_prefix('$') {
                    let sym = cur.split('-').next().unwrap_or("");
                    if !sym.is_empty() {
                        toks.push(Tok::Lit(sym.to_string()));
                    }
                }
            }
            '0' | '#' | '?' => {
                toks.push(Tok::Digit(c));
                i += 1;
            }
            '.' => {
                let after_seconds = toks.iter().rev().find(|t| is_date_tok(t)).is_some_and(|t| matches!(t, Tok::Second(_) | Tok::Elapsed('s', _)));
                if after_seconds && i + 1 < n && ch[i + 1] == '0' {
                    let z = run(i + 1, '0');
                    toks.push(Tok::SubSec(z.min(3)));
                    i += 1 + z;
                } else {
                    toks.push(Tok::Point);
                    i += 1;
                }
            }
            ',' => {
                toks.push(Tok::Comma);
                i += 1;
            }
            '%' => {
                toks.push(Tok::Percent);
                i += 1;
            }
            'E' | 'e' if i + 1 < n && (ch[i + 1] == '+' || ch[i + 1] == '-') => {
                toks.push(Tok::Exp { plus: ch[i + 1] == '+', upper: c == 'E' });
                i += 2;
            }
            '/' => {
                toks.push(Tok::Slash);
                i += 1;
            }
            '@' => {
                toks.push(Tok::At);
                i += 1;
            }
            'G' | 'g' if starts_ci(&ch, i, "general") => {
                toks.push(Tok::General);
                i += 7;
            }
            'A' | 'a' if starts_ci(&ch, i, "am/pm") => {
                toks.push(Tok::AmPm { full: true, upper: c == 'A' });
                i += 5;
            }
            'A' | 'a' if starts_ci(&ch, i, "a/p") => {
                toks.push(Tok::AmPm { full: false, upper: c == 'A' });
                i += 3;
            }
            'y' | 'Y' | 'e' => {
                let k = run(i, c);
                toks.push(Tok::Year(if c == 'e' { 4 } else { k }));
                i += k;
            }
            'm' | 'M' => {
                let k = run(i, 'm');
                toks.push(Tok::Month(k));
                i += k;
            }
            'd' | 'D' => {
                let k = run(i, 'd');
                toks.push(Tok::Day(k));
                i += k;
            }
            'h' | 'H' => {
                let k = run(i, 'h');
                toks.push(Tok::Hour(k));
                i += k;
            }
            's' | 'S' => {
                let k = run(i, 's');
                toks.push(Tok::Second(k));
                i += k;
            }
            'B' | 'b' if i + 1 < n && (ch[i + 1] == '1' || ch[i + 1] == '2') => {
                i += 2;
            }
            _ => {
                toks.push(Tok::Lit(c.to_string()));
                i += 1;
            }
        }
    }
    let is_date = toks.iter().any(is_date_tok);
    if is_date {
        // `m`/`mm` right after an hour or right before a second is minutes.
        let kinds = toks.clone();
        let date_idx: Vec<usize> = (0..kinds.len()).filter(|&k| is_date_tok(&kinds[k])).collect();
        for (pos, &k) in date_idx.iter().enumerate() {
            if let Tok::Month(len) = kinds[k] {
                if len > 2 {
                    continue;
                }
                let prev = pos.checked_sub(1).map(|p| &kinds[date_idx[p]]);
                let next = date_idx.get(pos + 1).map(|&p| &kinds[p]);
                let minute = matches!(prev, Some(Tok::Hour(_)) | Some(Tok::Elapsed('h', _)))
                    || matches!(next, Some(Tok::Second(_)) | Some(Tok::Elapsed('s', _)));
                if minute {
                    toks[k] = Tok::Minute(len);
                }
            }
        }
        for t in toks.iter_mut() {
            let lit = match t {
                Tok::Digit(c) => Some(c.to_string()),
                Tok::Point => Some(".".into()),
                Tok::Comma => Some(",".into()),
                Tok::Slash => Some("/".into()),
                Tok::Percent => Some("%".into()),
                _ => None,
            };
            if let Some(l) = lit {
                *t = Tok::Lit(l);
            }
        }
    }
    let has_digits = toks.iter().any(|t| matches!(t, Tok::Digit(_)));
    let kind = if is_date {
        Kind::Date
    } else if toks.contains(&Tok::General) {
        Kind::General
    } else if toks.contains(&Tok::At) && !has_digits {
        Kind::Text
    } else {
        Kind::Number
    };
    Section { toks, color, cond, kind }
}

impl NumberFormat {
    pub fn parse(code: &str) -> NumberFormat {
        let sections = split_sections(code).iter().map(|s| parse_section(s)).collect();
        NumberFormat { sections }
    }

    /// True when the first section is a date or time.
    pub fn is_date(&self) -> bool {
        self.sections.first().is_some_and(|s| s.kind == Kind::Date)
    }

    /// The text section: the fourth, or a last section that is all text.
    fn text_section(&self) -> Option<&Section> {
        if self.sections.len() >= 4 {
            return self.sections.get(3);
        }
        self.sections.last().filter(|s| s.kind == Kind::Text)
    }

    fn numeric_sections(&self) -> Vec<&Section> {
        let n = if self.sections.len() >= 4 {
            3
        } else if self.sections.last().is_some_and(|s| s.kind == Kind::Text) {
            self.sections.len() - 1
        } else {
            self.sections.len()
        };
        self.sections[..n].iter().collect()
    }

    pub fn format(&self, value: &CellValue, date1904: bool) -> Formatted {
        match value {
            CellValue::Empty => Formatted::plain(String::new()),
            CellValue::Bool(b) => Formatted::plain(if *b { "TRUE" } else { "FALSE" }.into()),
            CellValue::Error(e) => Formatted::plain(e.clone()),
            CellValue::Text(t) => match self.text_section() {
                Some(sec) => format_text(sec, t),
                None => Formatted::plain(t.clone()),
            },
            CellValue::Number(v) => self.format_number(*v, date1904),
        }
    }

    fn format_number(&self, v: f64, date1904: bool) -> Formatted {
        if !v.is_finite() {
            return Formatted::plain("#NUM!".into());
        }
        let secs = self.numeric_sections();
        if secs.is_empty() {
            return Formatted::plain(general(v));
        }
        let conditional = secs.iter().any(|s| s.cond.is_some());
        let (sec, x, minus) = if conditional {
            match secs.iter().find(|s| s.cond.map_or(true, |(op, c)| op.test(v, c))) {
                Some(s) => {
                    // A section picked by `[<...]` shows the magnitude; any other keeps the sign.
                    let neg_cond = s.cond.is_some_and(|(op, c)| matches!(op, Cmp::Lt | Cmp::Le) && c <= 0.0);
                    (*s, v.abs(), v < 0.0 && !neg_cond)
                }
                None => return Formatted::hashes(),
            }
        } else {
            match secs.len() {
                1 => (secs[0], v.abs(), v < 0.0),
                2 => {
                    if v >= 0.0 {
                        (secs[0], v, false)
                    } else {
                        (secs[1], -v, false)
                    }
                }
                _ => {
                    if v > 0.0 {
                        (secs[0], v, false)
                    } else if v < 0.0 {
                        (secs[1], -v, false)
                    } else {
                        (secs[2], 0.0, false)
                    }
                }
            }
        };
        let mut out = match sec.kind {
            Kind::Date => {
                if v < 0.0 {
                    return Formatted::hashes();
                }
                match format_date(sec, v, date1904) {
                    Some(f) => f,
                    None => return Formatted::hashes(),
                }
            }
            Kind::General => format_general_section(sec, x, minus),
            Kind::Text => Formatted::plain(general(v)),
            Kind::Number => format_number_section(sec, x, minus),
        };
        out.color = sec.color;
        out
    }
}

fn format_text(sec: &Section, t: &str) -> Formatted {
    let mut out = String::new();
    let mut fill = None;
    for tok in &sec.toks {
        match tok {
            Tok::At => out.push_str(t),
            Tok::Lit(l) => out.push_str(l),
            Tok::Pad(_) => out.push(' '),
            Tok::Fill(c) => fill = Some((out.len(), *c)),
            _ => {}
        }
    }
    Formatted { text: out, color: sec.color, fill, hashes: false }
}

fn format_general_section(sec: &Section, x: f64, minus: bool) -> Formatted {
    let mut out = String::new();
    let mut fill = None;
    if minus {
        out.push('-');
    }
    for tok in &sec.toks {
        match tok {
            Tok::General => out.push_str(&general(x)),
            Tok::Lit(l) => out.push_str(l),
            Tok::Pad(_) => out.push(' '),
            Tok::Fill(c) => fill = Some((out.len(), *c)),
            Tok::Percent => out.push('%'),
            _ => {}
        }
    }
    Formatted { text: out, color: None, fill, hashes: false }
}

#[derive(Clone, Copy, PartialEq)]
enum CommaRole {
    Group,
    Scale,
    Literal,
}

/// Fills integer placeholders right to left with `digits`; extra digits go to the first one.
fn fill_int(ph: &[char], digits: &str, group: bool) -> Vec<String> {
    let n = ph.len();
    let d: Vec<char> = digits.chars().collect();
    let l = d.len();
    let mut out = vec![String::new(); n];
    for i in 0..n {
        // The digit(s) this placeholder takes.
        let s: String = if l > n {
            if i == 0 {
                d[..l - n + 1].iter().collect()
            } else {
                d[l - n + i].to_string()
            }
        } else if i >= n - l {
            d[i - (n - l)].to_string()
        } else {
            match ph[i] {
                '0' => "0".into(),
                '?' => " ".into(),
                _ => String::new(),
            }
        };
        out[i] = s;
    }
    if group {
        // Thousands separators: walk the digits right to left.
        let mut count = 0usize;
        let total_digits: usize = out.iter().map(|s| s.chars().filter(char::is_ascii_digit).count()).sum();
        let mut seen = 0usize;
        for s in out.iter_mut().rev() {
            let mut rebuilt: Vec<char> = Vec::new();
            for c in s.chars().rev() {
                if c.is_ascii_digit() {
                    if count > 0 && count % 3 == 0 && seen < total_digits {
                        rebuilt.push(',');
                    }
                    count += 1;
                    seen += 1;
                }
                rebuilt.push(c);
            }
            rebuilt.reverse();
            *s = rebuilt.into_iter().collect();
        }
    }
    out
}

/// Decimal placeholders left to right; trailing zeros under `#` vanish and under `?` become spaces.
fn fill_dec(ph: &[char], frac: &str) -> Vec<String> {
    let f: Vec<char> = frac.chars().collect();
    let mut out: Vec<String> = f.iter().map(|c| c.to_string()).collect();
    let mut k = ph.len();
    while k > 0 && f[k - 1] == '0' && ph[k - 1] != '0' {
        out[k - 1] = if ph[k - 1] == '?' { " ".into() } else { String::new() };
        k -= 1;
    }
    out
}

fn comma_roles(toks: &[Tok]) -> Vec<Option<CommaRole>> {
    let end_int = toks.iter().position(|t| matches!(t, Tok::Point | Tok::Exp { .. } | Tok::Slash)).unwrap_or(toks.len());
    toks.iter()
        .enumerate()
        .map(|(i, t)| {
            if *t != Tok::Comma {
                return None;
            }
            let before = toks[..i].iter().any(|t| matches!(t, Tok::Digit(_)));
            let after = i < end_int && toks[i + 1..end_int].iter().any(|t| matches!(t, Tok::Digit(_)));
            Some(if before && after {
                CommaRole::Group
            } else if before {
                CommaRole::Scale
            } else {
                CommaRole::Literal
            })
        })
        .collect()
}

fn format_number_section(sec: &Section, x: f64, minus: bool) -> Formatted {
    let toks = &sec.toks;
    let roles = comma_roles(toks);
    let group = roles.iter().any(|r| *r == Some(CommaRole::Group));
    let scale = roles.iter().filter(|r| **r == Some(CommaRole::Scale)).count() as i32;
    let pct = toks.iter().filter(|t| **t == Tok::Percent).count() as i32;
    let mut v = x;
    if pct > 0 {
        v *= 100f64.powi(pct);
    }
    if scale > 0 {
        v /= 1000f64.powi(scale);
    }
    let mut out = String::new();
    let mut fill = None;
    if minus {
        out.push('-');
    }
    let exp_at = toks.iter().position(|t| matches!(t, Tok::Exp { .. }));
    let slash_at = toks.iter().position(|t| *t == Tok::Slash).filter(|&s| s > 0 && matches!(toks[s - 1], Tok::Digit(_)));

    // What each token emits.
    let mut emit: Vec<String> = vec![String::new(); toks.len()];
    let lit_of = |t: &Tok, i: usize| -> String {
        match t {
            Tok::Lit(l) => l.clone(),
            Tok::Pad(_) => " ".into(),
            Tok::Percent => "%".into(),
            Tok::Comma => {
                if roles[i] == Some(CommaRole::Literal) {
                    ",".into()
                } else {
                    String::new()
                }
            }
            Tok::Slash => "/".into(),
            Tok::Point => ".".into(),
            _ => String::new(),
        }
    };
    for (i, t) in toks.iter().enumerate() {
        emit[i] = lit_of(t, i);
    }

    if let Some(e_at) = exp_at {
        // Scientific.
        let mant = &toks[..e_at];
        let point = mant.iter().position(|t| *t == Tok::Point);
        let int_ph: Vec<usize> = (0..e_at).filter(|&k| matches!(mant[k], Tok::Digit(_)) && point.map_or(true, |p| k < p)).collect();
        let dec_ph: Vec<usize> = (0..e_at).filter(|&k| matches!(mant[k], Tok::Digit(_)) && point.is_some_and(|p| k > p)).collect();
        let int_n = int_ph.len().max(1) as i32;
        let eng = int_ph.len() > 1 && matches!(toks[int_ph[0]], Tok::Digit('#'));
        let (d, e0) = sig15(v);
        let mut e = if v == 0.0 {
            0
        } else if eng {
            e0.div_euclid(int_n) * int_n
        } else {
            e0 - (int_n - 1)
        };
        let (mut ints, mut frac);
        loop {
            let r = if v == 0.0 { (String::new(), "0".repeat(dec_ph.len())) } else { split_dec(&d, e0 - e, dec_ph.len()) };
            ints = r.0;
            frac = r.1;
            if v != 0.0 && ints.len() as i32 > int_n {
                e += if eng { int_n } else { 1 };
                continue;
            }
            break;
        }
        let ph: Vec<char> = int_ph.iter().map(|&k| if let Tok::Digit(c) = toks[k] { c } else { '0' }).collect();
        let filled = fill_int(&ph, &ints, group);
        for (j, &k) in int_ph.iter().enumerate() {
            emit[k] = filled[j].clone();
        }
        if int_ph.is_empty() && !ints.is_empty() {
            let at = point.unwrap_or(e_at);
            emit[at] = format!("{}{}", ints, emit[at]);
        }
        let dph: Vec<char> = dec_ph.iter().map(|&k| if let Tok::Digit(c) = toks[k] { c } else { '0' }).collect();
        let dfill = fill_dec(&dph, &frac);
        for (j, &k) in dec_ph.iter().enumerate() {
            emit[k] = dfill[j].clone();
        }
        let (plus, upper) = match toks[e_at] {
            Tok::Exp { plus, upper } => (plus, upper),
            _ => (true, true),
        };
        let exp_digits: Vec<usize> = (e_at + 1..toks.len()).filter(|&k| matches!(toks[k], Tok::Digit(_))).collect();
        let min = exp_digits.iter().filter(|&&k| toks[k] == Tok::Digit('0')).count().max(1);
        let mut es = format!("{:0>width$}", e.abs(), width = min);
        let sg = if e < 0 {
            "-"
        } else if plus {
            "+"
        } else {
            ""
        };
        emit[e_at] = format!("{}{}", if upper { 'E' } else { 'e' }, sg);
        for (j, &k) in exp_digits.iter().enumerate() {
            emit[k] = if j == 0 { std::mem::take(&mut es) } else { String::new() };
        }
    } else if let Some(s) = slash_at {
        // A fraction.
        let mut ns = s;
        while ns > 0 && matches!(toks[ns - 1], Tok::Digit(_)) {
            ns -= 1;
        }
        let num_ph: Vec<usize> = (ns..s).collect();
        let int_ph: Vec<usize> = (0..ns).filter(|&k| matches!(toks[k], Tok::Digit(_))).collect();
        let mut de = s + 1;
        let mut den_str = String::new();
        let mut fixed = false;
        let mut den_toks = Vec::new();
        while de < toks.len() {
            match &toks[de] {
                Tok::Digit(c) => den_str.push(*c),
                Tok::Lit(l) if !l.is_empty() && l.chars().all(|c| c.is_ascii_digit()) => {
                    fixed = true;
                    den_str.push_str(l);
                }
                _ => break,
            }
            den_toks.push(de);
            de += 1;
        }
        let has_int = !int_ph.is_empty();
        let (mut ip, f) = if has_int { (v.trunc(), v - v.trunc()) } else { (0.0, v) };
        let (mut num, den) = if fixed {
            let den: u64 = den_str.chars().filter(char::is_ascii_digit).collect::<String>().parse().unwrap_or(1).max(1);
            ((f * den as f64).round() as u64, den)
        } else {
            let k = den_str.chars().count().clamp(1, 7) as u32;
            best_rational(f, 10u64.pow(k) - 1)
        };
        if has_int && num == den && num > 0 {
            ip += 1.0;
            num = 0;
        }
        let ph: Vec<char> = int_ph.iter().map(|&k| if let Tok::Digit(c) = toks[k] { c } else { '0' }).collect();
        let ints = if ip == 0.0 {
            if num == 0 {
                "0".to_string()
            } else {
                String::new()
            }
        } else {
            round_dec(ip, 0).0
        };
        let filled = fill_int(&ph, &ints, group);
        for (j, &k) in int_ph.iter().enumerate() {
            emit[k] = filled[j].clone();
        }
        let num_w = num_ph.len();
        let den_w = den_str.chars().count();
        if num == 0 && has_int {
            // No fraction part: spaces where it would be.
            let blank = " ".repeat(num_w + 1 + den_w);
            for &k in &num_ph {
                emit[k] = String::new();
            }
            emit[s] = blank;
            for &k in &den_toks {
                emit[k] = String::new();
            }
        } else {
            let (num, den) = if num == 0 { (0, if fixed { den } else { 1 }) } else { (num, den) };
            let ns_str = num.to_string();
            let pad_n = num_w.saturating_sub(ns_str.len());
            let lead: String = num_ph[..pad_n.min(num_ph.len())]
                .iter()
                .map(|&k| match toks[k] {
                    Tok::Digit('0') => '0',
                    Tok::Digit('?') => ' ',
                    _ => '\u{0}',
                })
                .filter(|c| *c != '\u{0}')
                .collect();
            for (j, &k) in num_ph.iter().enumerate() {
                emit[k] = if j == 0 { format!("{}{}", lead, ns_str) } else { String::new() };
            }
            let ds = den.to_string();
            let trail = if fixed { String::new() } else { " ".repeat(den_w.saturating_sub(ds.len())) };
            for (j, &k) in den_toks.iter().enumerate() {
                emit[k] = if j == 0 { format!("{}{}", ds, trail) } else { String::new() };
            }
        }
    } else {
        // Plain digits.
        let point = toks.iter().position(|t| *t == Tok::Point);
        let int_ph: Vec<usize> = (0..toks.len()).filter(|&k| matches!(toks[k], Tok::Digit(_)) && point.map_or(true, |p| k < p)).collect();
        let dec_ph: Vec<usize> = (0..toks.len()).filter(|&k| matches!(toks[k], Tok::Digit(_)) && point.is_some_and(|p| k > p)).collect();
        let (ints, frac) = round_dec(v, dec_ph.len());
        let ph: Vec<char> = int_ph.iter().map(|&k| if let Tok::Digit(c) = toks[k] { c } else { '0' }).collect();
        let filled = fill_int(&ph, &ints, group);
        for (j, &k) in int_ph.iter().enumerate() {
            emit[k] = filled[j].clone();
        }
        if int_ph.is_empty() && !ints.is_empty() {
            if let Some(p) = point {
                emit[p] = format!("{}{}", ints, emit[p]);
            }
        }
        let dph: Vec<char> = dec_ph.iter().map(|&k| if let Tok::Digit(c) = toks[k] { c } else { '0' }).collect();
        let dfill = fill_dec(&dph, &frac);
        for (j, &k) in dec_ph.iter().enumerate() {
            emit[k] = dfill[j].clone();
        }
    }

    for (i, t) in toks.iter().enumerate() {
        if let Tok::Fill(c) = t {
            fill = Some((out.len(), *c));
        }
        out.push_str(&emit[i]);
    }
    Formatted { text: out, color: None, fill, hashes: false }
}

/// The fraction nearest `x` (>= 0) with a denominator up to `maxd` (continued fractions with the
/// best semiconvergent).
fn best_rational(x: f64, maxd: u64) -> (u64, u64) {
    let maxd = maxd.max(1);
    let (mut p0, mut q0, mut p1, mut q1) = (0u64, 1u64, 1u64, 0u64);
    let mut v = x;
    for _ in 0..64 {
        let a = v.floor();
        if a > 1e15 {
            break;
        }
        let a = a as u64;
        let q2 = q0 + a * q1;
        if q2 > maxd {
            let k = (maxd - q0) / q1.max(1);
            let (pk, qk) = (p0 + k * p1, q0 + k * q1);
            let e1 = (x - p1 as f64 / q1.max(1) as f64).abs();
            let e2 = (x - pk as f64 / qk.max(1) as f64).abs();
            if q1 == 0 || e2 < e1 {
                return (pk, qk.max(1));
            }
            return (p1, q1);
        }
        let p2 = p0 + a * p1;
        p0 = p1;
        q0 = q1;
        p1 = p2;
        q1 = q2;
        let frac = v - v.floor();
        if frac < 1e-10 {
            break;
        }
        v = 1.0 / frac;
    }
    (p1, q1.max(1))
}

fn format_date(sec: &Section, v: f64, date1904: bool) -> Option<Formatted> {
    if v >= 2_958_466.0 {
        return None;
    }
    let sub_digits = sec.toks.iter().filter_map(|t| if let Tok::SubSec(n) = t { Some(*n) } else { None }).max().unwrap_or(0);
    let scale = 10i64.pow(sub_digits as u32);
    let unit = 86_400 * scale;
    let total = (v * unit as f64).round() as i64;
    let days = total / unit;
    let rem = total % unit;
    let secs = rem / scale;
    let sub = rem % scale;
    let (h, mi, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let (y, mo, d, wd) = serial_to_ymd(days, date1904);
    let ampm = sec.toks.iter().any(|t| matches!(t, Tok::AmPm { .. }));
    let total_secs = total / scale;
    let mut out = String::new();
    let mut fill = None;
    for t in &sec.toks {
        match t {
            Tok::Lit(l) => out.push_str(l),
            Tok::Pad(_) => out.push(' '),
            Tok::Fill(c) => fill = Some((out.len(), *c)),
            Tok::Year(n) => {
                if *n <= 2 {
                    out.push_str(&format!("{:02}", y.rem_euclid(100)));
                } else {
                    out.push_str(&format!("{:04}", y));
                }
            }
            Tok::Month(n) => {
                let name = MONTHS[(mo as usize).clamp(1, 12) - 1];
                match n {
                    1 => out.push_str(&mo.to_string()),
                    2 => out.push_str(&format!("{:02}", mo)),
                    3 => out.push_str(&name[..3]),
                    5 => out.push_str(&name[..1]),
                    _ => out.push_str(name),
                }
            }
            Tok::Day(n) => match n {
                1 => out.push_str(&d.to_string()),
                2 => out.push_str(&format!("{:02}", d)),
                3 => out.push_str(&DAYS[wd as usize][..3]),
                _ => out.push_str(DAYS[wd as usize]),
            },
            Tok::Hour(n) => {
                let hh = if ampm {
                    let x = h % 12;
                    if x == 0 {
                        12
                    } else {
                        x
                    }
                } else {
                    h
                };
                if *n >= 2 {
                    out.push_str(&format!("{:02}", hh));
                } else {
                    out.push_str(&hh.to_string());
                }
            }
            Tok::Minute(n) => {
                if *n >= 2 {
                    out.push_str(&format!("{:02}", mi));
                } else {
                    out.push_str(&mi.to_string());
                }
            }
            Tok::Second(n) => {
                if *n >= 2 {
                    out.push_str(&format!("{:02}", s));
                } else {
                    out.push_str(&s.to_string());
                }
            }
            Tok::Elapsed(c, n) => {
                let val = match c {
                    'h' => total_secs / 3600,
                    'm' => total_secs / 60,
                    _ => total_secs,
                };
                out.push_str(&format!("{:0>width$}", val, width = *n));
            }
            Tok::SubSec(n) => {
                let digits = format!("{:0>width$}", sub, width = sub_digits);
                out.push('.');
                out.push_str(&digits[..(*n).min(digits.len())]);
            }
            Tok::AmPm { full, upper } => {
                let pm = h >= 12;
                let s = match (full, pm) {
                    (true, false) => "AM",
                    (true, true) => "PM",
                    (false, false) => "A",
                    (false, true) => "P",
                };
                if *upper {
                    out.push_str(s);
                } else {
                    out.push_str(&s.to_lowercase());
                }
            }
            Tok::Digit(c) => out.push(*c),
            Tok::At | Tok::General | Tok::Point | Tok::Comma | Tok::Percent | Tok::Exp { .. } | Tok::Slash => {}
        }
    }
    Some(Formatted { text: out, color: None, fill, hashes: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f64, code: &str) -> String {
        format_value(&CellValue::Number(v), code).0
    }

    #[test]
    fn digits_rounding_and_grouping() {
        assert_eq!(f(3.14159, "0.00"), "3.14");
        assert_eq!(f(1.005, "0.00"), "1.01");
        assert_eq!(f(2.5, "0"), "3");
        assert_eq!(f(-2.5, "0"), "-3");
        assert_eq!(f(0.1 + 0.2, "0.000000000000000"), "0.300000000000000");
        assert_eq!(f(1234567.891, "#,##0"), "1,234,568");
        assert_eq!(f(1234567.891, "#,##0.00"), "1,234,567.89");
        assert_eq!(f(999.0, "#,##0"), "999");
        assert_eq!(f(0.0, "#,##0"), "0");
        assert_eq!(f(0.5, "#.##"), ".5");
        assert_eq!(f(5.0, "0.0#"), "5.0");
        assert_eq!(f(5.25, "??.??"), " 5.25");
        assert_eq!(f(5.2, "0.0?"), "5.2 ");
        assert_eq!(f(12345.0, "0,"), "12");
        assert_eq!(f(12345678.0, "#,##0.0,,\"M\""), "12.3M");
        assert_eq!(f(5555551234.0, "(000) 000-0000"), "(555) 555-1234");
        assert_eq!(f(7.0, "000"), "007");
    }

    #[test]
    fn percent_scientific_fraction() {
        assert_eq!(f(0.256, "0%"), "26%");
        assert_eq!(f(0.25678, "0.00%"), "25.68%");
        assert_eq!(f(12345.678, "0.00E+00"), "1.23E+04");
        assert_eq!(f(0.000123, "0.00E+00"), "1.23E-04");
        assert_eq!(f(0.0, "0.00E+00"), "0.00E+00");
        assert_eq!(f(12345.0, "##0.0E+0"), "12.3E+3");
        assert_eq!(f(9.999, "0.00E+00"), "1.00E+01");
        assert_eq!(f(1.5, "# ?/?"), "1 1/2");
        assert_eq!(f(0.75, "# ?/?").trim(), "3/4");
        assert_eq!(f(0.3333, "?/?"), "1/3");
        assert_eq!(f(2.0, "# ?/?").trim_end(), "2");
        assert_eq!(f(0.5, "?/8"), "4/8");
        assert_eq!(f(1.25, "?/?"), "5/4");
    }

    #[test]
    fn fraction_two_digits() {
        // pi with up to 99 as denominator is 311/99 -> 3 14/99.
        assert_eq!(f(3.14159, "# ??/??"), "3 14/99");
    }

    #[test]
    fn sections_colours_conditions() {
        let code = "$#,##0.00;[Red]-$#,##0.00";
        assert_eq!(format_value(&CellValue::Number(1234.5), code), ("$1,234.50".to_string(), None));
        assert_eq!(format_value(&CellValue::Number(-1234.5), code), ("-$1,234.50".to_string(), Some(Color::rgb(255, 0, 0))));
        assert_eq!(f(-5.0, "0;(0)"), "(5)");
        assert_eq!(f(0.0, "0;-0;\"zero\""), "zero");
        assert_eq!(f(-5.0, "$#,##0"), "-$5");
        assert_eq!(f(150.0, "[>100]\"big\";[<=100]0"), "big");
        assert_eq!(f(50.0, "[>100]\"big\";[<=100]0"), "50");
        let (_, c) = format_value(&CellValue::Number(5.0), "[Color10]0");
        assert_eq!(c, Some(hex_color(INDEXED[17])));
        assert_eq!(format_value(&CellValue::Text("abc".into()), "0;-0;0;\"<\"@\">\"").0, "<abc>");
        assert_eq!(format_value(&CellValue::Text("abc".into()), "0.00").0, "abc");
        assert_eq!(format_value(&CellValue::Text("007".into()), "@").0, "007");
        assert_eq!(f(42.0, "@"), "42");
        assert_eq!(format_value(&CellValue::Bool(true), "0.00").0, "TRUE");
        assert_eq!(f(1234.5, "[$€-x-euro]#,##0.00"), "€1,234.50");
    }

    #[test]
    fn literals_padding_fill() {
        assert_eq!(f(5.0, "0\" kg\""), "5 kg");
        assert_eq!(f(5.0, "\\$0"), "$5");
        assert_eq!(f(5.0, "0_)"), "5 ");
        let full = format_value_full(&CellValue::Number(5.0), "$* #,##0", false);
        assert_eq!(full.text, "$5");
        assert_eq!(full.fill, Some((1, ' ')));
        // Accounting: zero is a dash.
        assert!(f(0.0, builtin_format(44).unwrap()).contains('-'));
        assert_eq!(f(-1234.0, builtin_format(41).unwrap()).trim(), "(1,234)");
    }

    #[test]
    fn general_format() {
        assert_eq!(general(1234567.891), "1234567.891");
        assert_eq!(general(0.0001), "0.0001");
        assert_eq!(general(0.1 + 0.2), "0.3");
        assert_eq!(general(123456789012.0), "1.23457E+11");
        assert_eq!(general(12345678901.0), "12345678901");
        assert_eq!(general(1.0 / 3.0), "0.333333333");
        assert_eq!(general(-1.0 / 3.0), "-0.333333333");
        assert_eq!(general(1e-10), "1E-10");
        assert_eq!(general(100.0), "100");
        assert_eq!(general(-42.5), "-42.5");
        assert_eq!(f(1234567.891, "General"), "1234567.891");
        assert_eq!(f(0.0001, "General"), "0.0001");
        assert_eq!(general_fit(1234567.891, 7).as_deref(), Some("1234568"));
        assert_eq!(general_fit(123456789.0, 5).as_deref(), Some("1E+08"));
    }

    #[test]
    fn serial_dates_1900_and_1904() {
        assert_eq!(serial_to_ymd(1, false), (1900, 1, 1, 0));
        assert_eq!(serial_to_ymd(59, false), (1900, 2, 28, 2));
        assert_eq!(serial_to_ymd(60, false), (1900, 2, 29, 3));
        assert_eq!(serial_to_ymd(61, false), (1900, 3, 1, 4));
        assert_eq!(serial_to_ymd(45000, false), (2023, 3, 15, 3));
        assert_eq!(serial_to_ymd(0, true), (1904, 1, 1, 5));
        assert_eq!(f(1.0, "yyyy-mm-dd"), "1900-01-01");
        assert_eq!(f(60.0, "yyyy-mm-dd"), "1900-02-29");
        assert_eq!(f(61.0, "yyyy-mm-dd"), "1900-03-01");
        assert_eq!(f(45000.0, "yyyy-mm-dd"), "2023-03-15");
        assert_eq!(format_value_full(&CellValue::Number(45000.0 - 1462.0), "yyyy-mm-dd", true).text, "2023-03-15");
        assert_eq!(ymd_to_serial(2023, 3, 15, false), 45000);
        assert_eq!(ymd_to_serial(1900, 1, 1, false), 1);
        assert_eq!(ymd_to_serial(1900, 3, 1, false), 61);
        assert_eq!(iso_to_serial("2023-03-15T12:00:00", false), Some(45000.5));
    }

    #[test]
    fn date_and_time_codes() {
        assert_eq!(f(45000.0, "d-mmm-yy"), "15-Mar-23");
        assert_eq!(f(45000.0, "dddd, mmmm d, yyyy"), "Wednesday, March 15, 2023");
        assert_eq!(f(45000.0, "ddd dd mmm"), "Wed 15 Mar");
        assert_eq!(f(45000.0, "mmmmm"), "M");
        assert_eq!(f(45000.0, builtin_format(14).unwrap()), "3/15/2023");
        assert_eq!(f(45000.75, "h:mm AM/PM"), "6:00 PM");
        assert_eq!(f(45000.25, "h:mm am/pm"), "6:00 am");
        assert_eq!(f(0.5, "hh:mm:ss"), "12:00:00");
        assert_eq!(f(0.0, "h:mm AM/PM"), "12:00 AM");
        assert_eq!(f(1.5, "[h]:mm:ss"), "36:00:00");
        assert_eq!(f(1.5 + 30.0 / 86400.0, "[h]:mm:ss"), "36:00:30");
        assert_eq!(f(0.04, "[mm]:ss"), "57:36");
        assert_eq!(f(0.5 + 1.25 / 86400.0, "h:mm:ss.00"), "12:00:01.25");
        assert_eq!(f(45000.5, "m/d/yyyy h:mm"), "3/15/2023 12:00");
        assert_eq!(f(0.0104166666, "mm:ss"), "15:00");
        assert_eq!(f(-1.0, "yyyy-mm-dd"), "#".repeat(11));
        assert!(is_date_format("yyyy-mm-dd") && !is_date_format("0.00") && is_date_format("[h]:mm"));
    }
}
