//! The values the evaluator works with, and the spreadsheet's own rules for turning one kind into
//! another: text that reads as a number (`"1,200"`, `"15%"`, `"$3"`, `"2026-11-26"`, `"10:30 PM"`),
//! booleans as 1 and 0, blanks as 0, `""` or FALSE depending on the other side, and the comparison
//! order numbers < text < booleans (text compared without case).

use std::cmp::Ordering;
use std::rc::Rc;

use super::super::cellref::CellRange;
use super::super::model::CellValue;
use super::super::numfmt::{serial_to_ymd, ymd_to_serial};
use super::eval::Lambda;

/// The error values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrKind {
    Null,
    Div0,
    Value,
    Ref,
    Name,
    Num,
    NA,
    Spill,
    Calc,
    Field,
    Blocked,
    Connect,
    Busy,
    GettingData,
}

const ERR_TEXT: [(ErrKind, &str); 14] = [
    (ErrKind::Null, "#NULL!"),
    (ErrKind::Div0, "#DIV/0!"),
    (ErrKind::Value, "#VALUE!"),
    (ErrKind::Ref, "#REF!"),
    (ErrKind::Name, "#NAME?"),
    (ErrKind::Num, "#NUM!"),
    (ErrKind::NA, "#N/A"),
    (ErrKind::Spill, "#SPILL!"),
    (ErrKind::Calc, "#CALC!"),
    (ErrKind::Field, "#FIELD!"),
    (ErrKind::Blocked, "#BLOCKED!"),
    (ErrKind::Connect, "#CONNECT!"),
    (ErrKind::Busy, "#BUSY!"),
    (ErrKind::GettingData, "#GETTING_DATA"),
];

impl ErrKind {
    pub fn text(self) -> &'static str {
        ERR_TEXT.iter().find(|(k, _)| *k == self).map(|(_, t)| *t).unwrap_or("#VALUE!")
    }

    /// The error a text names (any case).
    pub fn parse(s: &str) -> Option<ErrKind> {
        ERR_TEXT.iter().find(|(_, t)| t.eq_ignore_ascii_case(s)).map(|(k, _)| *k)
    }

    /// All error texts (the lexer reads them as literals).
    pub fn all() -> impl Iterator<Item = (ErrKind, &'static str)> {
        ERR_TEXT.iter().copied()
    }

    /// `ERROR.TYPE`'s number.
    pub fn number(self) -> u32 {
        match self {
            ErrKind::Null => 1,
            ErrKind::Div0 => 2,
            ErrKind::Value => 3,
            ErrKind::Ref => 4,
            ErrKind::Name => 5,
            ErrKind::Num => 6,
            ErrKind::NA => 7,
            ErrKind::GettingData => 8,
            ErrKind::Spill => 9,
            ErrKind::Connect => 10,
            ErrKind::Blocked => 11,
            ErrKind::Field => 13,
            ErrKind::Calc => 14,
            ErrKind::Busy => 12,
        }
    }
}

/// A rectangle of one sheet (a reference's value).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Area {
    pub sheet: usize,
    pub range: CellRange,
}

/// A rectangle of plain values (never a reference or an array inside), row by row.
#[derive(Debug, Clone)]
pub struct Array {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<Value>,
}

impl Array {
    pub fn new(rows: usize, cols: usize, data: Vec<Value>) -> Array {
        debug_assert_eq!(rows * cols, data.len());
        Array { rows, cols, data }
    }

    pub fn scalar(v: Value) -> Array {
        Array { rows: 1, cols: 1, data: vec![v] }
    }

    pub fn column(data: Vec<Value>) -> Array {
        Array { rows: data.len(), cols: 1, data }
    }

    pub fn row(data: Vec<Value>) -> Array {
        Array { rows: 1, cols: data.len(), data }
    }

    pub fn get(&self, r: usize, c: usize) -> &Value {
        &self.data[r * self.cols + c]
    }

    /// The element for a broadcast to a larger shape: a single row or column repeats, anything
    /// beyond the edge is `#N/A`.
    pub fn broadcast(&self, r: usize, c: usize) -> Value {
        let rr = if self.rows == 1 { 0 } else { r };
        let cc = if self.cols == 1 { 0 } else { c };
        if rr >= self.rows || cc >= self.cols {
            Value::Err(ErrKind::NA)
        } else {
            self.get(rr, cc).clone()
        }
    }

    pub fn transpose(&self) -> Array {
        let mut data = Vec::with_capacity(self.data.len());
        for c in 0..self.cols {
            for r in 0..self.rows {
                data.push(self.get(r, c).clone());
            }
        }
        Array { rows: self.cols, cols: self.rows, data }
    }
}

/// One value in the middle of a calculation.
#[derive(Debug, Clone)]
pub enum Value {
    Blank,
    Num(f64),
    Text(String),
    Bool(bool),
    Err(ErrKind),
    Array(Rc<Array>),
    Ref(Area),
    Lambda(Rc<Lambda>),
}

impl Value {
    /// True for an array or a reference of more than one cell.
    pub fn is_multi(&self) -> bool {
        match self {
            Value::Array(_) => true,
            Value::Ref(a) => !a.range.is_single(),
            _ => false,
        }
    }
}

/// A stored cell value as an evaluator value.
pub fn cell_to_value(v: &CellValue) -> Value {
    match v {
        CellValue::Empty => Value::Blank,
        CellValue::Number(n) => Value::Num(*n),
        CellValue::Text(s) => Value::Text(s.clone()),
        CellValue::Bool(b) => Value::Bool(*b),
        CellValue::Error(e) => Value::Err(ErrKind::parse(e).unwrap_or(ErrKind::Value)),
    }
}

/// A plain value as a stored cell value (an array or reference is the caller's to resolve first).
pub fn value_to_cell(v: &Value) -> CellValue {
    match v {
        Value::Blank => CellValue::Number(0.0),
        Value::Num(n) if n.is_finite() => CellValue::Number(if *n == 0.0 { 0.0 } else { *n }),
        Value::Num(_) => CellValue::Error(ErrKind::Num.text().into()),
        Value::Text(s) => CellValue::Text(s.clone()),
        Value::Bool(b) => CellValue::Bool(*b),
        Value::Err(e) => CellValue::Error(e.text().into()),
        Value::Array(_) | Value::Ref(_) => CellValue::Error(ErrKind::Value.text().into()),
        Value::Lambda(_) => CellValue::Error(ErrKind::Calc.text().into()),
    }
}

/// `x` rounded to `digits` significant digits (decimal rounding).
pub fn round_sig(x: f64, digits: usize) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    format!("{:.*e}", digits.saturating_sub(1), x).parse().unwrap_or(x)
}

/// Excel's ROUND: half away from zero, after taking the scaled value to 15 significant digits so
/// `2.675` rounds to `2.68` as written, not as stored.
pub fn round_half_away(x: f64, digits: i32) -> f64 {
    round_with(x, digits, f64::round)
}

/// Rounding with a chosen rule (`ceil` away from zero, `trunc` toward it) on the scaled value.
pub fn round_with(x: f64, digits: i32, rule: fn(f64) -> f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let digits = digits.clamp(-308, 308);
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let a = x.abs();
    if digits >= 0 {
        let s = 10f64.powi(digits);
        let y = round_sig(a * s, 15);
        if !y.is_finite() {
            return x;
        }
        sign * rule(y) / s
    } else {
        let s = 10f64.powi(-digits);
        let y = round_sig(a / s, 15);
        sign * rule(y) * s
    }
}

/// A number as text the way a formula turns it into text (`=""&n`): up to 15 significant digits,
/// no trailing zeros, scientific outside a sensible range.
pub fn num_to_text(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if !n.is_finite() {
        return ErrKind::Num.text().into();
    }
    let r = round_sig(n, 15);
    if r.fract() == 0.0 && r.abs() < 1e15 {
        return format!("{}", r as i64);
    }
    let exp = r.abs().log10().floor() as i32;
    if (-9..15).contains(&exp) {
        let decimals = (14 - exp).max(0) as usize;
        let mut s = format!("{:.*}", decimals, r);
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    } else {
        let s = format!("{:.14e}", r);
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let mut m = m.to_string();
        if m.contains('.') {
            while m.ends_with('0') {
                m.pop();
            }
            if m.ends_with('.') {
                m.pop();
            }
        }
        let e: i32 = e.parse().unwrap_or(0);
        format!("{}E{}{:02}", m, if e < 0 { '-' } else { '+' }, e.abs())
    }
}

/// A plain number in text: sign, `$`, thousands commas, decimals, exponent, `%`, `(5)` for -5.
fn parse_plain_number(s: &str) -> Option<f64> {
    let mut t = s.trim();
    if t.is_empty() {
        return None;
    }
    let mut neg = false;
    if t.starts_with('(') && t.ends_with(')') {
        neg = true;
        t = t[1..t.len() - 1].trim();
    }
    let mut pct = 0;
    while let Some(x) = t.strip_suffix('%') {
        pct += 1;
        t = x.trim_end();
    }
    if let Some(x) = t.strip_prefix('-') {
        neg = !neg;
        t = x.trim_start();
    } else if let Some(x) = t.strip_prefix('+') {
        t = x.trim_start();
    }
    if let Some(x) = t.strip_prefix('$') {
        t = x.trim_start();
        if let Some(x) = t.strip_prefix('-') {
            neg = !neg;
            t = x.trim_start();
        }
    }
    if t.is_empty() {
        return None;
    }
    // Thousands commas only between digit groups of three before the decimal point.
    let (int_part, rest) = match t.find(|c: char| c == '.' || c == 'e' || c == 'E') {
        Some(i) => (&t[..i], &t[i..]),
        None => (t, ""),
    };
    let int_clean: String = if int_part.contains(',') {
        let groups: Vec<&str> = int_part.split(',').collect();
        if groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3) {
            return None;
        }
        groups.concat()
    } else {
        int_part.to_string()
    };
    if !int_clean.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let body = format!("{int_clean}{rest}");
    if body.is_empty() || body == "." {
        return None;
    }
    if !body.chars().all(|c| c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-') {
        return None;
    }
    let mut v: f64 = body.parse().ok()?;
    for _ in 0..pct {
        v /= 100.0;
    }
    Some(if neg { -v } else { v })
}

const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

/// `Nov`, `november`, `Sept` -> the month number.
fn month_name(s: &str) -> Option<u32> {
    let l = s.to_ascii_lowercase();
    if l.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.starts_with(&l)).map(|i| i as u32 + 1)
}

/// Days in a month the spreadsheet's way: February 1900 has 29 days in the 1900 system.
pub fn days_in_month(y: i64, m: u32, date1904: bool) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
            if leap || (y == 1900 && !date1904) {
                29
            } else {
                28
            }
        }
    }
}

/// (year, month, day) to a serial, with the day allowed past the month's end or below 1 (DATE's
/// rule), and the 29 February 1900 that never was at serial 60.
pub fn ymd_serial(y: i64, m: u32, d: i64, date1904: bool) -> i64 {
    ymd_to_serial(y, m, 1, date1904) + d - 1
}

/// A serial's (year, month, day).
pub fn serial_ymd(serial: f64, date1904: bool) -> (i64, u32, u32) {
    let (y, m, d, _) = serial_to_ymd(serial.floor() as i64, date1904);
    (y, m, d)
}

fn parse_time(s: &str) -> Option<f64> {
    let l = s.trim().to_ascii_lowercase();
    let (body, ampm) = if let Some(x) = l.strip_suffix("am").or_else(|| l.strip_suffix('a')) {
        (x.trim(), Some(false))
    } else if let Some(x) = l.strip_suffix("pm").or_else(|| l.strip_suffix('p')) {
        (x.trim(), Some(true))
    } else {
        (l.as_str(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.is_empty() || parts.len() > 3 || (parts.len() == 1 && ampm.is_none()) {
        return None;
    }
    let mut h: f64 = parts[0].trim().parse::<u32>().ok()? as f64;
    let m: f64 = if parts.len() > 1 { parts[1].trim().parse::<u32>().ok()? as f64 } else { 0.0 };
    let sec: f64 = if parts.len() > 2 { parts[2].trim().parse::<f64>().ok()? } else { 0.0 };
    if m >= 60.0 || sec >= 60.0 || sec < 0.0 {
        return None;
    }
    if let Some(pm) = ampm {
        if !(1.0..=12.0).contains(&h) {
            return None;
        }
        if h == 12.0 {
            h = 0.0;
        }
        if pm {
            h += 12.0;
        }
    }
    Some((h * 3600.0 + m * 60.0 + sec) / 86400.0)
}

fn year_of(s: &str) -> Option<i64> {
    if !s.chars().all(|c| c.is_ascii_digit()) || s.is_empty() {
        return None;
    }
    let y: i64 = s.parse().ok()?;
    Some(match s.len() {
        1 | 2 => {
            if y < 30 {
                2000 + y
            } else {
                1900 + y
            }
        }
        4 => y,
        _ => return None,
    })
}

fn parse_date(s: &str, date1904: bool) -> Option<f64> {
    let parts: Vec<&str> = s.split(|c: char| c == '/' || c == '-' || c == ',' || c == ' ' || c == '.').filter(|p| !p.is_empty()).collect();
    let num = |p: &str| -> Option<i64> {
        if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() && p.len() <= 4 {
            p.parse().ok()
        } else {
            None
        }
    };
    let (y, m, d) = match parts.as_slice() {
        [a, b, c] => {
            if a.len() == 4 && num(a).is_some() {
                // yyyy-mm-dd (or yyyy-mmm-dd)
                (num(a)?, num(b).map(|x| x as u32).or_else(|| month_name(b))?, num(c)?)
            } else if let (Some(x), Some(yv)) = (month_name(b), year_of(c)) {
                (yv, x, num(a)?)
            } else if let (Some(x), Some(yv)) = (month_name(a), year_of(c)) {
                (yv, x, num(b)?)
            } else {
                // m/d/y, the en-US order.
                (year_of(c)?, num(a)? as u32, num(b)?)
            }
        }
        [a, b] => {
            if let (Some(x), Some(yv)) = (month_name(a), year_of(b)) {
                if b.len() == 4 {
                    (yv, x, 1)
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        _ => return None,
    };
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m, date1904) as i64 || !(1900..=9999).contains(&y) {
        return None;
    }
    let serial = ymd_serial(y, m, d, date1904);
    if serial < 0 {
        return None;
    }
    Some(serial as f64)
}

/// A date and/or time in text as a serial (`2026-11-26`, `11/26/2026`, `26-Nov-2026`,
/// `Nov 26, 2026`, `10:30`, `10:30 PM`, `2026-11-26 08:30:00`, `2026-11-26T08:30`).
pub fn parse_datetime_text(s: &str, date1904: bool) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(t) = parse_time(s) {
        if s.contains(':') || s.to_ascii_lowercase().ends_with('m') {
            return Some(t);
        }
    }
    // A time part: the last space- or T-separated piece holding ':' (plus an AM/PM word).
    let normal = if s.len() > 10 && s.as_bytes().get(10) == Some(&b'T') { format!("{} {}", &s[..10], &s[11..]) } else { s.to_string() };
    let words: Vec<&str> = normal.split_whitespace().collect();
    let mut cut = words.len();
    for (i, w) in words.iter().enumerate() {
        if w.contains(':') {
            cut = i;
            break;
        }
    }
    let date_part = words[..cut].join(" ");
    let time_part = words[cut..].join(" ");
    let date = if date_part.is_empty() { 0.0 } else { parse_date(&date_part, date1904)? };
    let time = if time_part.is_empty() { 0.0 } else { parse_time(&time_part)? };
    Some(date + time)
}

/// Text that the spreadsheet reads as a number (plain numbers, percents, currency, dates, times).
pub fn parse_number_text(s: &str, date1904: bool) -> Option<f64> {
    parse_plain_number(s).or_else(|| parse_datetime_text(s, date1904))
}

thread_local! {
    static DATE1904: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Sets the date system text coercion reads dates in (set by the evaluator for its workbook).
pub fn set_coercion_date1904(on: bool) {
    DATE1904.with(|d| d.set(on));
}

fn coercion_date1904() -> bool {
    DATE1904.with(|d| d.get())
}

/// A scalar as a number: blank 0, booleans 1/0, text that reads as a number, else `#VALUE!`.
pub fn to_num(v: &Value) -> Result<f64, ErrKind> {
    match v {
        Value::Blank => Ok(0.0),
        Value::Num(n) => Ok(*n),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Text(s) => parse_number_text(s, coercion_date1904()).ok_or(ErrKind::Value),
        Value::Err(e) => Err(*e),
        Value::Array(a) => to_num(a.data.first().unwrap_or(&Value::Blank)),
        Value::Ref(_) | Value::Lambda(_) => Err(ErrKind::Value),
    }
}

/// A scalar as text: numbers to 15 digits, TRUE/FALSE, blank "".
pub fn to_text(v: &Value) -> Result<String, ErrKind> {
    match v {
        Value::Blank => Ok(String::new()),
        Value::Num(n) => Ok(num_to_text(*n)),
        Value::Bool(b) => Ok(if *b { "TRUE".into() } else { "FALSE".into() }),
        Value::Text(s) => Ok(s.clone()),
        Value::Err(e) => Err(*e),
        Value::Array(a) => to_text(a.data.first().unwrap_or(&Value::Blank)),
        Value::Ref(_) | Value::Lambda(_) => Err(ErrKind::Value),
    }
}

/// A scalar as a boolean: numbers non-zero, text "TRUE"/"FALSE", blank FALSE.
pub fn to_bool(v: &Value) -> Result<bool, ErrKind> {
    match v {
        Value::Blank => Ok(false),
        Value::Num(n) => Ok(*n != 0.0),
        Value::Bool(b) => Ok(*b),
        Value::Text(s) => {
            if s.eq_ignore_ascii_case("TRUE") {
                Ok(true)
            } else if s.eq_ignore_ascii_case("FALSE") {
                Ok(false)
            } else {
                Err(ErrKind::Value)
            }
        }
        Value::Err(e) => Err(*e),
        Value::Array(a) => to_bool(a.data.first().unwrap_or(&Value::Blank)),
        Value::Ref(_) | Value::Lambda(_) => Err(ErrKind::Value),
    }
}

/// Two numbers equal at 15 significant digits (so `0.1+0.2=0.3` is TRUE).
pub fn num_eq(a: f64, b: f64) -> bool {
    a == b || round_sig(a, 15) == round_sig(b, 15)
}

fn num_cmp(a: f64, b: f64) -> Ordering {
    if num_eq(a, b) {
        Ordering::Equal
    } else {
        a.partial_cmp(&b).unwrap_or(Ordering::Equal)
    }
}

/// Text compared without case.
pub fn text_cmp(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

fn rank(v: &Value) -> u8 {
    match v {
        Value::Num(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        _ => 3,
    }
}

/// The comparison operators' order: errors propagate (left first), a blank takes the other side's
/// kind (0, "" or FALSE), numbers < text < booleans, text without case.
pub fn compare(a: &Value, b: &Value) -> Result<Ordering, ErrKind> {
    if let Value::Err(e) = a {
        return Err(*e);
    }
    if let Value::Err(e) = b {
        return Err(*e);
    }
    let blank_as = |other: &Value| match other {
        Value::Text(_) => Value::Text(String::new()),
        Value::Bool(_) => Value::Bool(false),
        _ => Value::Num(0.0),
    };
    let a2;
    let b2;
    let (a, b) = match (a, b) {
        (Value::Blank, Value::Blank) => return Ok(Ordering::Equal),
        (Value::Blank, o) => {
            a2 = blank_as(o);
            (&a2, o)
        }
        (o, Value::Blank) => {
            b2 = blank_as(o);
            (o, &b2)
        }
        _ => (a, b),
    };
    Ok(match (a, b) {
        (Value::Num(x), Value::Num(y)) => num_cmp(*x, *y),
        (Value::Text(x), Value::Text(y)) => text_cmp(x, y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    })
}

/// The sort order of SORT and friends: numbers, text, booleans, errors, blanks last.
pub fn sort_cmp(a: &Value, b: &Value) -> Ordering {
    let r = |v: &Value| match v {
        Value::Num(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        Value::Err(_) => 3,
        _ => 4,
    };
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Text(x), Value::Text(y)) => text_cmp(x, y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => r(a).cmp(&r(b)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WildTok {
    Any,
    One,
    Lit(char),
}

fn wild_tokens(pat: &str) -> Vec<WildTok> {
    let mut out = Vec::new();
    let mut it = pat.chars().flat_map(char::to_lowercase).peekable();
    while let Some(c) = it.next() {
        match c {
            '*' => out.push(WildTok::Any),
            '?' => out.push(WildTok::One),
            '~' => match it.peek() {
                Some(&n) if n == '*' || n == '?' || n == '~' => {
                    it.next();
                    out.push(WildTok::Lit(n));
                }
                _ => out.push(WildTok::Lit('~')),
            },
            _ => out.push(WildTok::Lit(c)),
        }
    }
    out
}

/// True when a pattern holds a wildcard (`*`, `?`) not escaped with `~`.
pub fn has_wildcard(pat: &str) -> bool {
    wild_tokens(pat).iter().any(|t| matches!(t, WildTok::Any | WildTok::One)) || pat.contains('~')
}

/// The whole of `text` matches `pat` (`*` any run, `?` one character, `~` escapes), without case.
pub fn wild_match(pat: &str, text: &str) -> bool {
    let p = wild_tokens(pat);
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() {
            match p[pi] {
                WildTok::Any => {
                    star = Some((pi, ti));
                    pi += 1;
                    continue;
                }
                WildTok::One => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                WildTok::Lit(c) if c == t[ti] => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((sp, st)) => {
                pi = sp + 1;
                ti = st + 1;
                star = Some((sp, st + 1));
            }
            None => return false,
        }
    }
    while pi < p.len() && p[pi] == WildTok::Any {
        pi += 1;
    }
    pi == p.len()
}
