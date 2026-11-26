//! A1 references: column letters (bijective base 26, A..XFD), cells (`B7`, `$B$7`) and ranges
//! (`A1:C9`, whole columns `A:C`, whole rows `3:5`). Everything inside is 0-based; the text is
//! 1-based as Excel writes it.

use std::fmt;

/// Rows in a sheet (1,048,576).
pub const MAX_ROWS: u32 = 1_048_576;
/// Columns in a sheet (16,384: A..XFD).
pub const MAX_COLS: u32 = 16_384;

/// The letters of a 0-based column (`0` -> `A`, `25` -> `Z`, `26` -> `AA`, `16383` -> `XFD`).
pub fn col_name(col: u32) -> String {
    let mut n = col as u64 + 1;
    let mut out = Vec::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.push(b'A' + rem);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The 0-based column of its letters (case-insensitive); `None` beyond XFD or for anything else.
pub fn col_index(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut n: u32 = 0;
    for b in letters.bytes() {
        if !b.is_ascii_alphabetic() {
            return None;
        }
        n = n * 26 + (b.to_ascii_uppercase() - b'A' + 1) as u32;
    }
    if n == 0 || n > MAX_COLS {
        return None;
    }
    Some(n - 1)
}

/// `B7` (or `$B$7`, any case) -> (row 6, col 1).
pub fn parse_cell(text: &str) -> Option<(u32, u32)> {
    let s = text.trim();
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && b[i] == b'$' {
        i += 1;
    }
    let ls = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    let col = col_index(&s[ls..i])?;
    if i < b.len() && b[i] == b'$' {
        i += 1;
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i != b.len() || ds == i || i - ds > 7 {
        return None;
    }
    let row: u32 = s[ds..i].parse().ok()?;
    if row == 0 || row > MAX_ROWS {
        return None;
    }
    Some((row - 1, col))
}

/// (row, col) 0-based -> `B7`.
pub fn cell_name(row: u32, col: u32) -> String {
    format!("{}{}", col_name(col), row + 1)
}

/// A rectangle of cells, inclusive on both ends, 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRange {
    pub r1: u32,
    pub c1: u32,
    pub r2: u32,
    pub c2: u32,
}

impl CellRange {
    pub fn new(r1: u32, c1: u32, r2: u32, c2: u32) -> CellRange {
        CellRange { r1: r1.min(r2), c1: c1.min(c2), r2: r1.max(r2), c2: c1.max(c2) }
    }

    pub fn cell(r: u32, c: u32) -> CellRange {
        CellRange { r1: r, c1: c, r2: r, c2: c }
    }

    /// `A1`, `A1:C9`, `$A$1:$C$9`, `A:C` (whole columns), `3:5` (whole rows). A sheet prefix
    /// (`Sheet1!A1`) is ignored.
    pub fn parse(text: &str) -> Option<CellRange> {
        let s = text.trim();
        let s = match s.rfind('!') {
            Some(i) => &s[i + 1..],
            None => s,
        };
        let (a, b) = match s.find(':') {
            Some(i) => (&s[..i], &s[i + 1..]),
            None => (s, s),
        };
        if let (Some(p), Some(q)) = (parse_cell(a), parse_cell(b)) {
            return Some(CellRange::new(p.0, p.1, q.0, q.1));
        }
        let strip = |x: &str| x.trim_start_matches('$').to_string();
        let (a, b) = (strip(a), strip(b));
        if let (Some(p), Some(q)) = (col_index(&a), col_index(&b)) {
            return Some(CellRange::new(0, p, MAX_ROWS - 1, q));
        }
        let row = |x: &str| x.parse::<u32>().ok().filter(|&r| r >= 1 && r <= MAX_ROWS).map(|r| r - 1);
        if let (Some(p), Some(q)) = (row(&a), row(&b)) {
            return Some(CellRange::new(p, 0, q, MAX_COLS - 1));
        }
        None
    }

    pub fn contains(&self, r: u32, c: u32) -> bool {
        r >= self.r1 && r <= self.r2 && c >= self.c1 && c <= self.c2
    }

    pub fn intersects(&self, o: &CellRange) -> bool {
        self.r1 <= o.r2 && o.r1 <= self.r2 && self.c1 <= o.c2 && o.c1 <= self.c2
    }

    /// The smallest range holding both.
    pub fn union(&self, o: &CellRange) -> CellRange {
        CellRange { r1: self.r1.min(o.r1), c1: self.c1.min(o.c1), r2: self.r2.max(o.r2), c2: self.c2.max(o.c2) }
    }

    pub fn rows(&self) -> u64 {
        (self.r2 - self.r1) as u64 + 1
    }

    pub fn cols(&self) -> u64 {
        (self.c2 - self.c1) as u64 + 1
    }

    pub fn is_single(&self) -> bool {
        self.r1 == self.r2 && self.c1 == self.c2
    }
}

/// `A1` for one cell, `A1:C9` otherwise.
impl fmt::Display for CellRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_single() {
            write!(f, "{}", cell_name(self.r1, self.c1))
        } else {
            write!(f, "{}:{}", cell_name(self.r1, self.c1), cell_name(self.r2, self.c2))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_letters_both_ways() {
        for (i, s) in [(0, "A"), (25, "Z"), (26, "AA"), (51, "AZ"), (52, "BA"), (701, "ZZ"), (702, "AAA"), (16383, "XFD")] {
            assert_eq!(col_name(i), s);
            assert_eq!(col_index(s), Some(i));
        }
        assert_eq!(col_index("XFE"), None);
        assert_eq!(col_index("a"), Some(0));
        assert_eq!(col_index(""), None);
        assert_eq!(col_index("A1"), None);
    }

    #[test]
    fn cells_and_ranges() {
        assert_eq!(parse_cell("B7"), Some((6, 1)));
        assert_eq!(parse_cell("$b$7"), Some((6, 1)));
        assert_eq!(parse_cell("XFD1048576"), Some((1_048_575, 16_383)));
        assert_eq!(parse_cell("A0"), None);
        assert_eq!(parse_cell("A1048577"), None);
        assert_eq!(parse_cell("7B"), None);
        assert_eq!(cell_name(6, 1), "B7");
        let r = CellRange::parse("C9:A1").unwrap();
        assert_eq!((r.r1, r.c1, r.r2, r.c2), (0, 0, 8, 2));
        assert_eq!(r.to_string(), "A1:C9");
        assert_eq!(CellRange::parse("Sheet1!B2").unwrap().to_string(), "B2");
        assert_eq!(CellRange::parse("B:C").unwrap(), CellRange::new(0, 1, MAX_ROWS - 1, 2));
        assert_eq!(CellRange::parse("3:4").unwrap(), CellRange::new(2, 0, 3, MAX_COLS - 1));
        assert!(CellRange::parse("hello").is_none());
        assert!(r.contains(4, 1) && !r.contains(9, 0));
    }
}
