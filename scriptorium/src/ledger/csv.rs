//! Delimited text: RFC 4180 quoting both ways, the separator found from the text (comma,
//! semicolon or tab), UTF-8 with or without a byte order mark (UTF-16 with its mark is read too).

/// The text of CSV bytes: a UTF-8 or UTF-16 byte order mark is honoured; other bytes are read as
/// UTF-8 (Windows-1252 for bytes that are not).
pub fn decode(bytes: &[u8]) -> String {
    if let Some(b) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(b).into_owned();
    }
    let utf16 = |b: &[u8], le: bool| -> String {
        let units: Vec<u16> = b.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(b) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(b, true);
    }
    if let Some(b) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(b, false);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    match b {
        0x80..=0x9F => char::from_u32(HIGH[(b - 0x80) as usize] as u32).unwrap_or('\u{FFFD}'),
        _ => b as char,
    }
}

/// The separator a text most likely uses: the one of comma, semicolon and tab that appears most
/// often (outside quotes) on the first lines, consistently.
pub fn sniff(text: &str) -> char {
    let lines: Vec<&str> = text.lines().take(10).collect();
    let mut best = (',', 0usize);
    for sep in [',', ';', '\t'] {
        let counts: Vec<usize> = lines.iter().map(|l| count_outside_quotes(l, sep)).collect();
        let first = counts.first().copied().unwrap_or(0);
        let steady = counts.iter().all(|&c| c == first);
        let score = if steady { first * 2 } else { first };
        if score > best.1 {
            best = (sep, score);
        }
    }
    best.0
}

fn count_outside_quotes(line: &str, sep: char) -> usize {
    let mut q = false;
    line.chars().filter(|&c| {
        if c == '"' {
            q = !q;
        }
        !q && c == sep
    })
    .count()
}

/// Rows of fields: quoted fields may hold separators, doubled quotes and line breaks. A trailing
/// line break makes no empty last row.
pub fn parse_delimited(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut started = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                quoted = true;
                started = true;
            }
            c if c == sep => {
                row.push(std::mem::take(&mut field));
                started = true;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                started = false;
            }
            _ => {
                field.push(c);
                started = true;
            }
        }
    }
    if started || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// One field written for CSV: quoted when it holds the separator, a quote or a line break.
pub fn quote_field(f: &str, sep: char) -> String {
    if f.contains(sep) || f.contains('"') || f.contains('\n') || f.contains('\r') || f.starts_with(' ') || f.ends_with(' ') {
        format!("\"{}\"", f.replace('"', "\"\""))
    } else {
        f.to_string()
    }
}

/// Rows written as CSV (CRLF line ends), with a UTF-8 byte order mark when asked.
pub fn write(rows: &[Vec<String>], sep: char, bom: bool) -> Vec<u8> {
    let mut out = String::new();
    for row in rows {
        let line: Vec<String> = row.iter().map(|f| quote_field(f, sep)).collect();
        out.push_str(&line.join(&sep.to_string()));
        out.push_str("\r\n");
    }
    let mut bytes = Vec::with_capacity(out.len() + 3);
    if bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    bytes.extend_from_slice(out.as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4180_both_ways() {
        let text = "a,\"b,c\",\"say \"\"hi\"\"\"\r\n1,2,\"two\nlines\"\n";
        let rows = parse_delimited(text, ',');
        assert_eq!(rows, vec![vec!["a", "b,c", "say \"hi\""], vec!["1", "2", "two\nlines"]]);
        let back = String::from_utf8(write(&rows, ',', false)).unwrap();
        assert_eq!(back, "a,\"b,c\",\"say \"\"hi\"\"\"\r\n1,2,\"two\nlines\"\r\n");
        assert_eq!(parse_delimited(&back, ','), rows);
        assert_eq!(parse_delimited("x,,\n", ','), vec![vec!["x", "", ""]]);
        assert_eq!(parse_delimited("", ','), Vec::<Vec<String>>::new());
        assert!(write(&rows, ',', true).starts_with(&[0xEF, 0xBB, 0xBF]));
    }

    #[test]
    fn separators_and_encodings() {
        assert_eq!(sniff("a;b;c\n1;2;3"), ';');
        assert_eq!(sniff("a\tb\n1\t2"), '\t');
        assert_eq!(sniff("a,b\n1,2"), ',');
        assert_eq!(sniff("\"x;y\",b\n1,2"), ',');
        assert_eq!(decode(&[0xEF, 0xBB, 0xBF, b'h', b'i']), "hi");
        assert_eq!(decode(&[0xFF, 0xFE, b'h', 0, b'i', 0]), "hi");
        assert_eq!(decode(&[b'a', 0x80]), "a€");
    }
}
