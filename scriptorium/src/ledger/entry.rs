//! What a typed entry becomes, the way a spreadsheet reads what the member types into a cell:
//! numbers (locale-neutral: `.` is the decimal point, `,` groups thousands), percentages, money,
//! dates, times, booleans, error literals, formulas (a leading `=`) and text (a leading `'` keeps
//! anything as text). Some entries also choose the cell's number format (`10%` -> `0%`,
//! `$1,200.50` -> money, `2026-11-26` -> a date), as long as the cell is still General.
//!
//! Numbers keep 15 significant digits, as the reference product does on entry.

use super::model::CellValue;
use super::numfmt::ymd_to_serial;

/// What one entry is.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub value: CellValue,
    /// The formula text without `=` (the value is left for the calculation).
    pub formula: Option<String>,
    /// The number format code the entry asks for (applied only to a General cell).
    pub format: Option<&'static str>,
}

impl Entry {
    fn value(value: CellValue) -> Entry {
        Entry { value, formula: None, format: None }
    }

    fn number(v: f64, format: Option<&'static str>) -> Entry {
        Entry { value: CellValue::Number(round15(v)), formula: None, format }
    }
}

/// The error literals a cell can hold.
pub const ERRORS: [&str; 8] = ["#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#GETTING_DATA"];

/// A number to 15 significant digits.
pub fn round15(v: f64) -> f64 {
    if !v.is_finite() || v == 0.0 {
        return v;
    }
    format!("{:.14e}", v).parse().unwrap_or(v)
}

/// Reads what was typed into a cell. `text_cell` says the cell's format is Text (`@`): then the
/// entry is kept exactly as typed.
pub fn parse_input(text: &str, date1904: bool, text_cell: bool) -> Entry {
    if text.is_empty() {
        return Entry::value(CellValue::Empty);
    }
    if text_cell {
        return Entry::value(CellValue::Text(text.to_string()));
    }
    if let Some(rest) = text.strip_prefix('\'') {
        return Entry::value(if rest.is_empty() { CellValue::Empty } else { CellValue::Text(rest.to_string()) });
    }
    if let Some(f) = text.strip_prefix('=') {
        if !f.trim().is_empty() {
            return Entry { value: CellValue::Empty, formula: Some(f.to_string()), format: None };
        }
        return Entry::value(CellValue::Text(text.to_string()));
    }
    let t = text.trim();
    if t.eq_ignore_ascii_case("true") {
        return Entry::value(CellValue::Bool(true));
    }
    if t.eq_ignore_ascii_case("false") {
        return Entry::value(CellValue::Bool(false));
    }
    if let Some(e) = ERRORS.iter().find(|e| e.eq_ignore_ascii_case(t)) {
        return Entry::value(CellValue::Error(e.to_string()));
    }
    if let Some((v, fmt)) = parse_number(t) {
        return Entry::number(v, fmt);
    }
    if let Some((v, fmt)) = parse_date_time(t, date1904) {
        return Entry { value: CellValue::Number(v), formula: None, format: Some(fmt) };
    }
    Entry::value(CellValue::Text(text.to_string()))
}

/// A plain number as typed: sign, `$ € £ ¥` money signs, `(123)` for negative, thousands commas
/// in groups of three, a decimal point, an exponent and a trailing `%`. Answers the value and
/// the format the entry implies.
pub fn parse_number(t: &str) -> Option<(f64, Option<&'static str>)> {
    let mut s = t.trim();
    if s.is_empty() {
        return None;
    }
    let mut neg = false;
    if s.starts_with('(') && s.ends_with(')') && s.len() > 2 {
        neg = true;
        s = s[1..s.len() - 1].trim();
    }
    let mut money = false;
    let take_money = |s: &mut &str, money: &mut bool| {
        for sym in ['$', '€', '£', '¥'] {
            if let Some(r) = s.strip_prefix(sym) {
                *s = r.trim_start();
                *money = true;
                return;
            }
        }
    };
    take_money(&mut s, &mut money);
    if let Some(r) = s.strip_prefix('-') {
        neg = !neg;
        s = r.trim_start();
    } else if let Some(r) = s.strip_prefix('+') {
        s = r.trim_start();
    }
    if !money {
        take_money(&mut s, &mut money);
    }
    let mut percent = false;
    if let Some(r) = s.strip_suffix('%') {
        percent = true;
        s = r.trim_end();
    }
    if !money {
        for sym in ['$', '€', '£', '¥'] {
            if let Some(r) = s.strip_suffix(sym) {
                s = r.trim_end();
                money = true;
                break;
            }
        }
    }
    if s.is_empty() || (money && percent) {
        return None;
    }
    // Split off an exponent.
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int, frac) = match mant.find('.') {
        Some(i) => (&mant[..i], Some(&mant[i + 1..])),
        None => (mant, None),
    };
    if int.is_empty() && frac.map_or(true, |f| f.is_empty()) {
        return None;
    }
    let mut grouped = false;
    let digits_int: String = if int.contains(',') {
        let groups: Vec<&str> = int.split(',').collect();
        if groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3) {
            return None;
        }
        grouped = true;
        groups.concat()
    } else {
        int.to_string()
    };
    if !digits_int.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some(f) = frac {
        if !f.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let mut num = format!("{}.{}", if digits_int.is_empty() { "0" } else { &digits_int }, frac.unwrap_or("0"));
    if let Some(e) = exp {
        let ok = {
            let e = e.strip_prefix(['+', '-']).unwrap_or(e);
            !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
        };
        if !ok || grouped {
            return None;
        }
        num.push('e');
        num.push_str(e);
    }
    let mut v: f64 = num.parse().ok()?;
    if !v.is_finite() {
        return None;
    }
    if neg {
        v = -v;
    }
    let decimals = frac.is_some_and(|f| !f.is_empty());
    let fmt = if percent {
        v /= 100.0;
        Some(if decimals { "0.00%" } else { "0%" })
    } else if money {
        Some(if decimals { "$#,##0.00_);($#,##0.00)" } else { "$#,##0_);($#,##0)" })
    } else if exp.is_some() {
        Some("0.00E+00")
    } else if grouped {
        Some(if decimals { "#,##0.00" } else { "#,##0" })
    } else {
        None
    };
    Some((v, fmt))
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn month_of(word: &str) -> Option<u32> {
    let w = word.to_ascii_lowercase();
    if w.len() < 3 {
        return None;
    }
    let full = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
    (0..12).find(|&i| full[i].starts_with(&w) || (w.len() == 4 && w == "sept" && i == 8) || MONTHS[i] == w).map(|i| i as u32 + 1)
}

fn year_of(text: &str) -> Option<i64> {
    if !text.bytes().all(|b| b.is_ascii_digit()) || text.is_empty() || text.len() > 4 || text.len() == 3 {
        return None;
    }
    let y: i64 = text.parse().ok()?;
    Some(match text.len() {
        1 | 2 => {
            if y < 30 {
                2000 + y
            } else {
                1900 + y
            }
        }
        _ => y,
    })
}

fn days_in(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// This year, from the clock (2026 when there is no clock).
pub fn this_year() -> i64 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(1_780_000_000);
    let days = secs.div_euclid(86_400);
    // Days since 1970 to a year (good to a day; enough for "this year").
    let mut y = 1970 + days * 400 / 146_097;
    while ymd_to_serial(y + 1, 1, 1, false) - ymd_to_serial(1970, 1, 1, false) <= days {
        y += 1;
    }
    while ymd_to_serial(y, 1, 1, false) - ymd_to_serial(1970, 1, 1, false) > days {
        y -= 1;
    }
    y
}

/// A date part: (serial day, format) or None.
fn parse_date(t: &str, date1904: bool) -> Option<(f64, &'static str)> {
    let ok = |y: i64, m: u32, d: u32| -> Option<f64> {
        if !(1..=12).contains(&m) || d == 0 || d > days_in(y, m) || !(1900..=9999).contains(&y) {
            return None;
        }
        let s = ymd_to_serial(y, m, d, date1904);
        if s < 0 {
            return None;
        }
        Some(s as f64)
    };
    let num = |s: &str| -> Option<u32> {
        if s.is_empty() || s.len() > 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    // Numeric: y-m-d, y/m/d (four-digit year first), m/d/y, m-d-y, m/d.
    for sep in ['-', '/'] {
        let parts: Vec<&str> = t.split(sep).collect();
        if parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
            if parts[0].len() == 4 {
                return ok(parts[0].parse().ok()?, num(parts[1])?, num(parts[2])?).map(|v| (v, "m/d/yyyy"));
            }
            return ok(year_of(parts[2])?, num(parts[0])?, num(parts[1])?).map(|v| (v, "m/d/yyyy"));
        }
        if parts.len() == 2 && sep == '/' {
            if let (Some(m), Some(d)) = (num(parts[0]), num(parts[1])) {
                return ok(this_year(), m, d).map(|v| (v, "d-mmm"));
            }
        }
    }
    // Words: 26-Nov-2026, 26 Nov 2026, Nov 26 2026, Nov 26, 2026, 26-Nov, Nov-26 (month-year).
    let words: Vec<&str> = t.split(|c: char| c == '-' || c == ' ' || c == ',' || c == '/').filter(|w| !w.is_empty()).collect();
    match words.len() {
        3 => {
            if let (Some(d), Some(m)) = (num(words[0]), month_of(words[1])) {
                return ok(year_of(words[2])?, m, d).map(|v| (v, "d-mmm-yy"));
            }
            if let (Some(m), Some(d)) = (month_of(words[0]), num(words[1])) {
                return ok(year_of(words[2])?, m, d).map(|v| (v, "d-mmm-yy"));
            }
            None
        }
        2 => {
            if let (Some(d), Some(m)) = (num(words[0]), month_of(words[1])) {
                return ok(this_year(), m, d).map(|v| (v, "d-mmm"));
            }
            if let Some(m) = month_of(words[0]) {
                if words[1].len() == 4 || (words[1].len() == 2 && t.contains('-')) {
                    return ok(year_of(words[1])?, m, 1).map(|v| (v, "mmm-yy"));
                }
                if let Some(d) = num(words[1]) {
                    return ok(this_year(), m, d).map(|v| (v, "d-mmm"));
                }
            }
            None
        }
        _ => None,
    }
}

/// A time part: (fraction of a day, format) or None. `13:45`, `13:45:10`, `1:45 pm`, `1 pm`, `25:30`.
fn parse_time(t: &str) -> Option<(f64, &'static str)> {
    let lower = t.trim().to_ascii_lowercase();
    let (body, ampm) = if let Some(b) = lower.strip_suffix("am").or_else(|| lower.strip_suffix('a')) {
        (b.trim_end().to_string(), Some(false))
    } else if let Some(b) = lower.strip_suffix("pm").or_else(|| lower.strip_suffix('p')) {
        (b.trim_end().to_string(), Some(true))
    } else {
        (lower.clone(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.is_empty() || parts.len() > 3 || (parts.len() == 1 && ampm.is_none()) {
        return None;
    }
    let mut vals = [0f64; 3];
    for (i, p) in parts.iter().enumerate() {
        let p = p.trim();
        if p.is_empty() {
            return None;
        }
        let ok = if i == 2 { p.bytes().all(|b| b.is_ascii_digit() || b == b'.') } else { p.bytes().all(|b| b.is_ascii_digit()) };
        if !ok {
            return None;
        }
        vals[i] = p.parse().ok()?;
    }
    let (mut h, m, s) = (vals[0], vals[1], vals[2]);
    if m >= 60.0 || s >= 60.0 {
        return None;
    }
    if let Some(pm) = ampm {
        if !(1.0..=12.0).contains(&h) && h != 0.0 {
            return None;
        }
        if h == 12.0 {
            h = 0.0;
        }
        if pm {
            h += 12.0;
        }
    }
    let frac = (h * 3600.0 + m * 60.0 + s) / 86_400.0;
    let fmt = match (ampm.is_some(), parts.len() == 3, h >= 24.0) {
        (_, _, true) => "[h]:mm:ss",
        (true, true, _) => "h:mm:ss AM/PM",
        (true, false, _) => "h:mm AM/PM",
        (false, true, _) => "h:mm:ss",
        (false, false, _) => "h:mm",
    };
    Some((frac, fmt))
}

/// A date, a time or both (`2026-11-26 08:30`).
pub fn parse_date_time(t: &str, date1904: bool) -> Option<(f64, &'static str)> {
    if !t.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some(r) = parse_time(t) {
        return Some(r);
    }
    if let Some(r) = parse_date(t, date1904) {
        return Some(r);
    }
    // Date and time split at the last space (or a `T`).
    let split = t.rfind(' ').or_else(|| t.find('T').filter(|&i| i == 10));
    if let Some(i) = split {
        let (d, tm) = (t[..i].trim(), t[i + 1..].trim());
        // "1:30 PM" split at its space: the right half alone is no time, so try one space further left.
        let (d, tm) = if parse_time(tm).is_none() {
            match d.rfind(' ') {
                Some(j) => (d[..j].trim(), &t[j + 1..]),
                None => (d, tm),
            }
        } else {
            (d, tm)
        };
        if let (Some((dv, _)), Some((tv, _))) = (parse_date(d, date1904), parse_time(tm)) {
            return Some((dv + tv, "m/d/yyyy h:mm"));
        }
    }
    None
}

/// What a cell shows in the formula bar and what editing starts from: `=` + formula, or the value
/// written back as one would type it.
pub fn input_text(value: &CellValue, formula: Option<&str>) -> String {
    if let Some(f) = formula {
        return format!("={f}");
    }
    match value {
        CellValue::Empty => String::new(),
        CellValue::Number(n) => {
            let a = n.abs();
            if a != 0.0 && !(1e-9..1e15).contains(&a) {
                super::numfmt::general(*n)
            } else {
                format!("{}", round15(*n))
            }
        }
        CellValue::Text(s) => {
            // Text that would read as something else is shown with its apostrophe.
            if matches!(parse_input(s, false, false).value, CellValue::Text(_)) && !s.starts_with('=') && !s.starts_with('\'') {
                s.clone()
            } else {
                format!("'{s}")
            }
        }
        CellValue::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        CellValue::Error(e) => e.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(t: &str) -> CellValue {
        parse_input(t, false, false).value
    }

    fn fmt(t: &str) -> Option<&'static str> {
        parse_input(t, false, false).format
    }

    #[test]
    fn numbers_locale_neutral() {
        assert_eq!(val("42"), CellValue::Number(42.0));
        assert_eq!(val("  -3.5 "), CellValue::Number(-3.5));
        assert_eq!(val("007"), CellValue::Number(7.0));
        assert_eq!(val(".5"), CellValue::Number(0.5));
        assert_eq!(val("1."), CellValue::Number(1.0));
        assert_eq!(val("1,234.5"), CellValue::Number(1234.5));
        assert_eq!(fmt("1,234.5"), Some("#,##0.00"));
        assert_eq!(fmt("1,234"), Some("#,##0"));
        assert_eq!(val("1,23"), CellValue::Text("1,23".into()));
        assert_eq!(val("(12)"), CellValue::Number(-12.0));
        assert_eq!(val("1e3"), CellValue::Number(1000.0));
        assert_eq!(fmt("1e3"), Some("0.00E+00"));
        assert_eq!(val("12345678901234567890"), CellValue::Number(12345678901234600000.0));
        assert_eq!(fmt("42"), None);
        assert_eq!(val("1.2.3"), CellValue::Text("1.2.3".into()));
        assert_eq!(val("-"), CellValue::Text("-".into()));
    }

    #[test]
    fn percent_and_money() {
        assert_eq!(val("10%"), CellValue::Number(0.1));
        assert_eq!(fmt("10%"), Some("0%"));
        assert_eq!(fmt("12.5%"), Some("0.00%"));
        assert_eq!(val("$1,200.50"), CellValue::Number(1200.5));
        assert_eq!(fmt("$1,200.50"), Some("$#,##0.00_);($#,##0.00)"));
        assert_eq!(val("-$5"), CellValue::Number(-5.0));
        assert_eq!(val("$-5"), CellValue::Number(-5.0));
        assert_eq!(fmt("$5"), Some("$#,##0_);($#,##0)"));
        assert_eq!(val("5€"), CellValue::Number(5.0));
    }

    #[test]
    fn dates_and_times() {
        let d = |t: &str| parse_input(t, false, false);
        assert_eq!(d("2026-11-26").value, CellValue::Number(46352.0));
        assert_eq!(d("2026-11-26").format, Some("m/d/yyyy"));
        assert_eq!(d("11/26/2026").value, CellValue::Number(46352.0));
        assert_eq!(d("26-Nov-2026").value, CellValue::Number(46352.0));
        assert_eq!(d("26 November 2026").value, CellValue::Number(46352.0));
        assert_eq!(d("Nov 26, 2026").value, CellValue::Number(46352.0));
        assert_eq!(d("2/30/2026").value, CellValue::Text("2/30/2026".into()));
        assert_eq!(d("13:30").value, CellValue::Number(13.5 / 24.0));
        assert_eq!(d("13:30").format, Some("h:mm"));
        assert_eq!(d("1:30 PM").value, CellValue::Number(13.5 / 24.0));
        assert_eq!(d("1:30 PM").format, Some("h:mm AM/PM"));
        assert_eq!(d("12:00 am").value, CellValue::Number(0.0));
        assert_eq!(d("08:00:30").format, Some("h:mm:ss"));
        assert_eq!(d("2026-11-26 12:00").value, CellValue::Number(46352.5));
        assert_eq!(d("2026-11-26 12:00").format, Some("m/d/yyyy h:mm"));
        assert_eq!(d("11/26/2026 1:30 PM").value, CellValue::Number(46352.0 + 13.5 / 24.0));
        assert_eq!(d("1/2/30").value, CellValue::Number(ymd_to_serial(1930, 1, 2, false) as f64));
        assert_eq!(d("1904-01-02").value, CellValue::Number(ymd_to_serial(1904, 1, 2, false) as f64));
        assert_eq!(parse_input("1904-01-02", true, false).value, CellValue::Number(1.0));
        assert!(matches!(d("Nov 26").value, CellValue::Number(_)));
        assert_eq!(d("hello 12").value, CellValue::Text("hello 12".into()));
        assert!(this_year() >= 2024);
    }

    #[test]
    fn formulas_booleans_errors_and_text() {
        let e = parse_input("=A1*2", false, false);
        assert_eq!((e.formula.as_deref(), e.value), (Some("A1*2"), CellValue::Empty));
        assert_eq!(val("="), CellValue::Text("=".into()));
        assert_eq!(val("true"), CellValue::Bool(true));
        assert_eq!(val("FALSE"), CellValue::Bool(false));
        assert_eq!(val("#n/a"), CellValue::Error("#N/A".into()));
        assert_eq!(val("'123"), CellValue::Text("123".into()));
        assert_eq!(val("'"), CellValue::Empty);
        assert_eq!(val(""), CellValue::Empty);
        assert_eq!(val(" hello "), CellValue::Text(" hello ".into()));
        assert_eq!(parse_input("=A1", false, true).value, CellValue::Text("=A1".into()));
        assert_eq!(parse_input("007", false, true).value, CellValue::Text("007".into()));
    }

    #[test]
    fn input_text_round_trips() {
        assert_eq!(input_text(&CellValue::Number(1.5), None), "1.5");
        assert_eq!(input_text(&CellValue::Empty, Some("SUM(A1:A3)")), "=SUM(A1:A3)");
        assert_eq!(input_text(&CellValue::Text("123".into()), None), "'123");
        assert_eq!(input_text(&CellValue::Text("abc".into()), None), "abc");
        assert_eq!(input_text(&CellValue::Bool(true), None), "TRUE");
    }
}
