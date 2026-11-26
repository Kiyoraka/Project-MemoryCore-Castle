//! Small writers for PDF syntax: numbers, names, strings, dates.

use std::fmt::Write as _;

/// A real number as PDF writes it: at most 4 decimals, trailing zeros trimmed, never an exponent,
/// never `-0`, and 0 for a non-finite value.
pub fn num(v: f64) -> String {
    let mut s = String::new();
    push_num(&mut s, v);
    s
}

/// [`num`] appended to `out`.
pub fn push_num(out: &mut String, v: f64) {
    if !v.is_finite() {
        out.push('0');
        return;
    }
    let r = (v * 10000.0).round() / 10000.0;
    if r == 0.0 {
        out.push('0');
        return;
    }
    if r == r.trunc() && r.abs() < 1e15 {
        let _ = write!(out, "{}", r as i64);
        return;
    }
    let t = format!("{r:.4}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    out.push_str(t);
}

/// Several numbers separated by spaces.
pub fn nums(vs: &[f64]) -> String {
    let mut s = String::new();
    for (i, v) in vs.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        push_num(&mut s, *v);
    }
    s
}

/// A name object (`/Name`), escaping delimiters, `#` and anything outside printable ASCII as `#xx`.
pub fn name(s: &str) -> String {
    let mut out = String::from("/");
    for b in s.bytes() {
        let plain = (0x21..=0x7e).contains(&b) && !b"()<>[]{}/%#".contains(&b);
        if plain {
            out.push(b as char);
        } else {
            let _ = write!(out, "#{b:02X}");
        }
    }
    out
}

/// A text string: a literal `( )` string when the text is plain printable ASCII, else UTF-16BE with a
/// byte-order mark, written in hex.
pub fn text_string(s: &str) -> String {
    if s.chars().all(|c| (' '..='~').contains(&c)) {
        let mut out = String::from("(");
        for c in s.chars() {
            match c {
                '(' | ')' | '\\' => {
                    out.push('\\');
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        out.push(')');
        out
    } else {
        let mut out = String::from("<FEFF");
        let mut buf = [0u16; 2];
        for c in s.chars() {
            for u in c.encode_utf16(&mut buf) {
                let _ = write!(out, "{u:04X}");
            }
        }
        out.push('>');
        out
    }
}

/// A byte string (ASCII only, e.g. a URI): a literal string with `( ) \` escaped and anything else
/// outside printable ASCII as an octal escape.
pub fn byte_string(bytes: &[u8]) -> String {
    let mut out = String::from("(");
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out.push(')');
    out
}

/// Bytes as upper-case hex.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02X}");
    }
    out
}

/// A PDF date (`D:YYYYMMDDHHmmSSZ`, UTC) for seconds since the Unix epoch.
pub fn pdf_date(unix_seconds: i64) -> String {
    let days = unix_seconds.div_euclid(86_400);
    let secs = unix_seconds.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("D:{:04}{:02}{:02}{:02}{:02}{:02}Z", y, m, d, secs / 3600, secs / 60 % 60, secs % 60)
}

/// Year, month, day of a day count since 1970-01-01 (proleptic Gregorian; Howard Hinnant's method).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_names_strings_dates() {
        assert_eq!(num(0.0), "0");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(12.0), "12");
        assert_eq!(num(-1.5), "-1.5");
        assert_eq!(num(1.0 / 3.0), "0.3333");
        assert_eq!(num(f64::NAN), "0");
        assert_eq!(num(1e-7), "0");
        assert_eq!(num(123456789.12345), "123456789.1235");
        assert_eq!(nums(&[1.0, 2.25, -3.0]), "1 2.25 -3");
        assert_eq!(name("Arial-BoldMT"), "/Arial-BoldMT");
        assert_eq!(name("A B#(x)"), "/A#20B#23#28x#29");
        assert_eq!(text_string("Hi (there) \\"), "(Hi \\(there\\) \\\\)");
        assert_eq!(text_string("Caf\u{e9}"), "<FEFF00430061006600E9>");
        assert_eq!(text_string("\u{1F600}"), "<FEFFD83DDE00>");
        assert_eq!(byte_string(b"a(b)\n"), "(a\\(b\\)\\012)");
        assert_eq!(hex(&[0, 0xab, 0x10]), "00AB10");
        assert_eq!(pdf_date(0), "D:19700101000000Z");
        assert_eq!(pdf_date(951_782_400), "D:20000229000000Z");
        assert_eq!(pdf_date(1_700_000_000), "D:20231114221320Z");
        assert_eq!(pdf_date(-1), "D:19691231235959Z");
    }
}
