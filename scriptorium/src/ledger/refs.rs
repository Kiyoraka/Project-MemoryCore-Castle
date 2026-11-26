//! References inside formula text, found and rewritten. One tokenizer finds every reference
//! (`B7`, `$B$7`, `A1:C9`, whole columns `A:C`, whole rows `3:5`, each with an optional sheet,
//! `Sheet2!A1` or `'My Sheet'!A1`); strings, structured references (`Table1[Col]`), function
//! names and other names are copied untouched. On top of it:
//!
//! - [`shift_structure`]: rows or columns inserted or deleted (optionally only in a band of
//!   columns or rows, for inserting or deleting cells), the way the reference product rewrites
//!   every formula, name and range: references after the place move, ranges that span it grow or
//!   shrink, references into deleted cells become `#REF!`.
//! - [`move_refs`]: cells moved (cut and paste): references into the moved block follow it.
//! - [`rename_sheet`] and [`drop_sheet`]: a sheet renamed or deleted.
//!
//! Every function is pure text in, text out; what is not a reference stays byte for byte.

use super::cellref::{col_index, col_name, CellRange, MAX_COLS, MAX_ROWS};

/// What a reference points at (0-based; `abs` flags are the `$` marks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Cell { r: u32, c: u32, ra: bool, ca: bool },
    Area { r1: u32, c1: u32, r2: u32, c2: u32, ra1: bool, ca1: bool, ra2: bool, ca2: bool },
    Cols { c1: u32, c2: u32, a1: bool, a2: bool },
    Rows { r1: u32, r2: u32, a1: bool, a2: bool },
}

/// One reference: its sheet (unquoted, as written) and what it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub sheet: Option<String>,
    pub kind: RefKind,
}

/// What a rewrite does with one reference.
pub enum Out {
    Keep,
    Replace(Ref),
    /// The reference is gone: `#REF!` (its sheet prefix goes too, as the reference product writes it).
    Gone,
}

impl Ref {
    /// The cells it covers (whole columns and rows as full ranges).
    pub fn range(&self) -> CellRange {
        match self.kind {
            RefKind::Cell { r, c, .. } => CellRange::cell(r, c),
            RefKind::Area { r1, c1, r2, c2, .. } => CellRange::new(r1, c1, r2, c2),
            RefKind::Cols { c1, c2, .. } => CellRange::new(0, c1, MAX_ROWS - 1, c2),
            RefKind::Rows { r1, r2, .. } => CellRange::new(r1, 0, r2, MAX_COLS - 1),
        }
    }

    /// True when it points into `target` from a formula living on sheet `home`.
    pub fn on_sheet(&self, home: &str, target: &str) -> bool {
        match &self.sheet {
            Some(s) => s.eq_ignore_ascii_case(target),
            None => !home.is_empty() && home.eq_ignore_ascii_case(target),
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        if let Some(s) = &self.sheet {
            out.push_str(&quote_sheet(s));
            out.push('!');
        }
        let d = |a: bool| if a { "$" } else { "" };
        match self.kind {
            RefKind::Cell { r, c, ra, ca } => out.push_str(&format!("{}{}{}{}", d(ca), col_name(c), d(ra), r + 1)),
            RefKind::Area { r1, c1, r2, c2, ra1, ca1, ra2, ca2 } => out.push_str(&format!(
                "{}{}{}{}:{}{}{}{}",
                d(ca1),
                col_name(c1),
                d(ra1),
                r1 + 1,
                d(ca2),
                col_name(c2),
                d(ra2),
                r2 + 1
            )),
            RefKind::Cols { c1, c2, a1, a2 } => out.push_str(&format!("{}{}:{}{}", d(a1), col_name(c1), d(a2), col_name(c2))),
            RefKind::Rows { r1, r2, a1, a2 } => out.push_str(&format!("{}{}:{}{}", d(a1), r1 + 1, d(a2), r2 + 1)),
        }
        out
    }
}

/// A sheet name as a formula writes it: quoted when it is not a plain name.
pub fn quote_sheet(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && super::cellref::parse_cell(name).is_none()
        && !name.eq_ignore_ascii_case("true")
        && !name.eq_ignore_ascii_case("false")
        && !looks_r1c1(name);
    if plain {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

fn looks_r1c1(s: &str) -> bool {
    let u = s.to_ascii_uppercase();
    let b = u.as_bytes();
    if b.first() != Some(&b'R') && b.first() != Some(&b'C') {
        return false;
    }
    u.bytes().all(|c| c == b'R' || c == b'C' || c.is_ascii_digit())
}

fn is_tok(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '$' || c == '_' || c == '.' || c == '\\' || (c as u32) > 127
}

/// A cell token: (row, col, row abs, col abs).
fn cell_token(t: &str) -> Option<(u32, u32, bool, bool)> {
    let b = t.as_bytes();
    let mut i = 0;
    let ca = b.first() == Some(&b'$');
    if ca {
        i += 1;
    }
    let ls = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    let c = col_index(&t[ls..i])?;
    let ra = i < b.len() && b[i] == b'$';
    if ra {
        i += 1;
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i != b.len() || ds == i || i - ds > 7 {
        return None;
    }
    let r: u32 = t[ds..].parse().ok()?;
    if r == 0 || r > MAX_ROWS {
        return None;
    }
    Some((r - 1, c, ra, ca))
}

fn col_token(t: &str) -> Option<(u32, bool)> {
    let (a, rest) = match t.strip_prefix('$') {
        Some(r) => (true, r),
        None => (false, t),
    };
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    Some((col_index(rest)?, a))
}

fn row_token(t: &str) -> Option<(u32, bool)> {
    let (a, rest) = match t.strip_prefix('$') {
        Some(r) => (true, r),
        None => (false, t),
    };
    if rest.is_empty() || rest.len() > 7 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let r: u32 = rest.parse().ok()?;
    if r == 0 || r > MAX_ROWS {
        return None;
    }
    Some((r - 1, a))
}

/// The reference spelled by `a` (and `b` after a colon), if it is one.
fn parse_kind(a: &str, b: Option<&str>) -> Option<RefKind> {
    match b {
        None => cell_token(a).map(|(r, c, ra, ca)| RefKind::Cell { r, c, ra, ca }),
        Some(b) => {
            if let (Some(p), Some(q)) = (cell_token(a), cell_token(b)) {
                return Some(RefKind::Area { r1: p.0, c1: p.1, r2: q.0, c2: q.1, ra1: p.2, ca1: p.3, ra2: q.2, ca2: q.3 });
            }
            if let (Some(p), Some(q)) = (col_token(a), col_token(b)) {
                return Some(RefKind::Cols { c1: p.0, c2: q.0, a1: p.1, a2: q.1 });
            }
            if let (Some(p), Some(q)) = (row_token(a), row_token(b)) {
                return Some(RefKind::Rows { r1: p.0, r2: q.0, a1: p.1, a2: q.1 });
            }
            None
        }
    }
}

/// Rewrites every reference of a formula with `f`. Text that is not a reference, and references
/// `f` keeps, are copied exactly.
pub fn rewrite(formula: &str, f: &mut dyn FnMut(&Ref) -> Out) -> String {
    let ch: Vec<char> = formula.chars().collect();
    let n = ch.len();
    let mut out = String::with_capacity(formula.len() + 8);
    let mut i = 0;
    let mut external = false;
    let read_tok = |from: usize| -> usize {
        let mut j = from;
        while j < n && is_tok(ch[j]) {
            j += 1;
        }
        j
    };
    while i < n {
        let c = ch[i];
        if c == '"' {
            out.push(c);
            i += 1;
            while i < n {
                out.push(ch[i]);
                if ch[i] == '"' {
                    if i + 1 < n && ch[i + 1] == '"' {
                        out.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            external = false;
            continue;
        }
        if c == '[' {
            let mut depth = 0;
            while i < n {
                out.push(ch[i]);
                if ch[i] == '[' {
                    depth += 1;
                } else if ch[i] == ']' {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
            external = true;
            continue;
        }
        // An optional sheet prefix, then the reference.
        let start = i;
        let mut sheet: Option<String> = None;
        let mut j = i;
        if c == '\'' {
            let mut name = String::new();
            j += 1;
            let mut closed = false;
            while j < n {
                if ch[j] == '\'' {
                    if j + 1 < n && ch[j + 1] == '\'' {
                        name.push('\'');
                        j += 2;
                        continue;
                    }
                    j += 1;
                    closed = true;
                    break;
                }
                name.push(ch[j]);
                j += 1;
            }
            if closed && j < n && ch[j] == '!' {
                sheet = Some(name);
                j += 1;
            } else {
                // Not a sheet name: copy the quoted run as it is.
                out.extend(&ch[start..j]);
                i = j;
                external = false;
                continue;
            }
        } else if is_tok(c) {
            let e = read_tok(j);
            if e < n && ch[e] == '!' {
                sheet = Some(ch[j..e].iter().collect());
                j = e + 1;
            }
        } else {
            out.push(c);
            i += 1;
            external = false;
            continue;
        }
        // The reference token(s) after the prefix.
        let a_end = read_tok(j);
        let a: String = ch[j..a_end].iter().collect();
        let next = ch.get(a_end).copied();
        if a.is_empty() || next == Some('(') || next == Some('[') || (sheet.is_none() && next == Some('!')) {
            out.extend(&ch[start..a_end.max(start + 1)]);
            i = a_end.max(start + 1);
            external = false;
            continue;
        }
        let prev_dot = start > 0 && (ch[start - 1] == '.' || ch[start - 1].is_ascii_alphanumeric());
        let mut end = a_end;
        let mut kind = None;
        if next == Some(':') {
            let b_end = read_tok(a_end + 1);
            let b: String = ch[a_end + 1..b_end].iter().collect();
            if ch.get(b_end) != Some(&'(') && ch.get(b_end) != Some(&'!') {
                if let Some(k) = parse_kind(&a, Some(&b)) {
                    kind = Some(k);
                    end = b_end;
                }
            }
        }
        if kind.is_none() {
            kind = parse_kind(&a, None);
        }
        match kind {
            Some(k) if !external && !prev_dot => {
                let r = Ref { sheet: sheet.clone(), kind: k };
                match f(&r) {
                    Out::Keep => out.extend(&ch[start..end]),
                    Out::Replace(nr) => out.push_str(&nr.to_text()),
                    Out::Gone => out.push_str("#REF!"),
                }
                i = end;
            }
            _ => {
                out.extend(&ch[start..a_end]);
                i = a_end;
            }
        }
        external = false;
    }
    out
}

/// Every reference of a formula (for tests and checks).
pub fn refs_of(formula: &str) -> Vec<Ref> {
    let mut list = Vec::new();
    rewrite(formula, &mut |r| {
        list.push(r.clone());
        Out::Keep
    });
    list
}

/// Rows (`Row`) or columns (`Col`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Row,
    Col,
}

/// A structural change: `count` rows or columns inserted before index `at` (`count` < 0: that many
/// deleted from `at`), on sheet `sheet`; with a `band` (a span of the other axis) only those cells
/// shift (Insert cells shifting down or right, Delete cells shifting up or left).
#[derive(Debug, Clone)]
pub struct Shift {
    pub sheet: String,
    pub axis: Axis,
    pub at: u32,
    pub count: i64,
    pub band: Option<(u32, u32)>,
}

/// A span on the shifted axis moved by the change; `None` when it is deleted.
pub fn shift_span(lo: u32, hi: u32, at: u32, count: i64, max: u32) -> Option<(u32, u32)> {
    if count > 0 {
        let n = count as u64;
        if lo >= at {
            let nl = lo as u64 + n;
            if nl >= max as u64 {
                return None;
            }
            Some((nl as u32, (hi as u64 + n).min(max as u64 - 1) as u32))
        } else if hi >= at {
            Some((lo, (hi as u64 + n).min(max as u64 - 1) as u32))
        } else {
            Some((lo, hi))
        }
    } else if count < 0 {
        let n = (-count) as u64;
        let d1 = at as u64;
        let d2 = d1 + n - 1;
        let (l, h) = (lo as u64, hi as u64);
        if h < d1 {
            return Some((lo, hi));
        }
        if l > d2 {
            return Some(((l - n) as u32, (h - n) as u32));
        }
        if l >= d1 && h <= d2 {
            return None;
        }
        let nl = if l < d1 { l } else { d1 };
        let nh = if h > d2 { h - n } else { d1 - 1 };
        Some((nl as u32, nh as u32))
    } else {
        Some((lo, hi))
    }
}

impl Shift {
    fn max(&self) -> u32 {
        match self.axis {
            Axis::Row => MAX_ROWS,
            Axis::Col => MAX_COLS,
        }
    }

    /// True when a range's other-axis span lies inside the band (always without a band).
    fn in_band(&self, r: &CellRange) -> bool {
        match self.band {
            None => true,
            Some((lo, hi)) => {
                let (a, b) = match self.axis {
                    Axis::Row => (r.c1, r.c2),
                    Axis::Col => (r.r1, r.r2),
                };
                a >= lo && b <= hi
            }
        }
    }

    /// A range after the change; `None` when deleted. Ranges outside the band, and whole rows
    /// under a column change (or whole columns under a row change), stay.
    pub fn range(&self, r: &CellRange) -> Option<CellRange> {
        if !self.in_band(r) {
            return Some(*r);
        }
        let max = self.max();
        match self.axis {
            Axis::Row => {
                if r.r1 == 0 && r.r2 == MAX_ROWS - 1 {
                    return Some(*r);
                }
                let (a, b) = shift_span(r.r1, r.r2, self.at, self.count, max)?;
                Some(CellRange { r1: a, r2: b, ..*r })
            }
            Axis::Col => {
                if r.c1 == 0 && r.c2 == MAX_COLS - 1 {
                    return Some(*r);
                }
                let (a, b) = shift_span(r.c1, r.c2, self.at, self.count, max)?;
                Some(CellRange { c1: a, c2: b, ..*r })
            }
        }
    }

    fn apply(&self, r: &Ref) -> Out {
        let range = r.range();
        let Some(nr) = self.range(&range) else { return Out::Gone };
        if nr == range {
            return Out::Keep;
        }
        let kind = match r.kind {
            RefKind::Cell { ra, ca, .. } => RefKind::Cell { r: nr.r1, c: nr.c1, ra, ca },
            RefKind::Area { r1, c1, ra1, ca1, ra2, ca2, .. } => {
                // Keep the corners in the order they were written.
                let (nr1, nr2) = if r1 <= range.r2 && r1 == range.r1 { (nr.r1, nr.r2) } else { (nr.r2, nr.r1) };
                let (nc1, nc2) = if c1 == range.c1 { (nr.c1, nr.c2) } else { (nr.c2, nr.c1) };
                RefKind::Area { r1: nr1, c1: nc1, r2: nr2, c2: nc2, ra1, ca1, ra2, ca2 }
            }
            RefKind::Cols { a1, a2, .. } => RefKind::Cols { c1: nr.c1, c2: nr.c2, a1, a2 },
            RefKind::Rows { a1, a2, .. } => RefKind::Rows { r1: nr.r1, r2: nr.r2, a1, a2 },
        };
        Out::Replace(Ref { sheet: r.sheet.clone(), kind })
    }
}

/// A formula living on sheet `home` (empty: a defined name, whose references always name their
/// sheet) after a structural change.
pub fn shift_structure(formula: &str, home: &str, op: &Shift) -> String {
    rewrite(formula, &mut |r| if r.on_sheet(home, &op.sheet) { op.apply(r) } else { Out::Keep })
}

/// True when a formula on `home` refers to anything on sheet `target`.
pub fn mentions_sheet(formula: &str, home: &str, target: &str) -> bool {
    refs_of(formula).iter().any(|r| r.on_sheet(home, target))
}

/// A formula on `home` after cells `from` on sheet `sheet` moved by (`dr`, `dc`): references
/// wholly inside the block follow it (both `$` and relative parts).
pub fn move_refs(formula: &str, home: &str, sheet: &str, from: &CellRange, dr: i64, dc: i64) -> String {
    rewrite(formula, &mut |r| {
        if !r.on_sheet(home, sheet) {
            return Out::Keep;
        }
        let range = r.range();
        let whole_rows = range.r1 == 0 && range.r2 == MAX_ROWS - 1;
        let whole_cols = range.c1 == 0 && range.c2 == MAX_COLS - 1;
        if whole_rows || whole_cols || !(from.r1 <= range.r1 && range.r2 <= from.r2 && from.c1 <= range.c1 && range.c2 <= from.c2) {
            return Out::Keep;
        }
        let mv = |v: u32, d: i64, max: u32| -> Option<u32> {
            let n = v as i64 + d;
            (n >= 0 && n < max as i64).then_some(n as u32)
        };
        let kind = match r.kind {
            RefKind::Cell { r: rr, c, ra, ca } => match (mv(rr, dr, MAX_ROWS), mv(c, dc, MAX_COLS)) {
                (Some(a), Some(b)) => RefKind::Cell { r: a, c: b, ra, ca },
                _ => return Out::Gone,
            },
            RefKind::Area { r1, c1, r2, c2, ra1, ca1, ra2, ca2 } => {
                match (mv(r1, dr, MAX_ROWS), mv(c1, dc, MAX_COLS), mv(r2, dr, MAX_ROWS), mv(c2, dc, MAX_COLS)) {
                    (Some(a), Some(b), Some(c), Some(d)) => RefKind::Area { r1: a, c1: b, r2: c, c2: d, ra1, ca1, ra2, ca2 },
                    _ => return Out::Gone,
                }
            }
            other => other,
        };
        Out::Replace(Ref { sheet: r.sheet.clone(), kind })
    })
}

/// References to sheet `old` written with the name `new`.
pub fn rename_sheet(formula: &str, old: &str, new: &str) -> String {
    rewrite(formula, &mut |r| match &r.sheet {
        Some(s) if s.eq_ignore_ascii_case(old) => Out::Replace(Ref { sheet: Some(new.to_string()), kind: r.kind }),
        _ => Out::Keep,
    })
}

/// References to a deleted sheet become `#REF!` (the reference product writes `#REF!` alone).
pub fn drop_sheet(formula: &str, gone: &str) -> String {
    rewrite(formula, &mut |r| match &r.sheet {
        Some(s) if s.eq_ignore_ascii_case(gone) => Out::Gone,
        _ => Out::Keep,
    })
}

/// A formula copied (dr, dc) away: relative parts move, `$` parts stay (see [`super::shift_formula`]).
pub fn copy_shift(formula: &str, dr: i64, dc: i64) -> String {
    super::formula::shift_formula(formula, dr, dc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(sheet: &str, at: u32, count: i64) -> Shift {
        Shift { sheet: sheet.into(), axis: Axis::Row, at, count, band: None }
    }

    fn cols(sheet: &str, at: u32, count: i64) -> Shift {
        Shift { sheet: sheet.into(), axis: Axis::Col, at, count, band: None }
    }

    #[test]
    fn finds_every_kind_of_reference() {
        let r = refs_of("SUM(A1:B2)+$C$3+Sheet2!D4+'My Sheet'!E5:F6+A:B+$3:$4+LOG10(G7)+\"H8\"+Table1[I9]+TRUE");
        let texts: Vec<String> = r.iter().map(Ref::to_text).collect();
        assert_eq!(texts, ["A1:B2", "$C$3", "Sheet2!D4", "'My Sheet'!E5:F6", "A:B", "$3:$4", "G7"]);
        assert!(refs_of("1.5E3+name_1+R1C1").is_empty());
        assert!(refs_of("[1]Sheet1!A1").is_empty(), "external references are left alone");
    }

    #[test]
    fn inserting_rows_moves_and_grows() {
        let op = rows("Data", 4, 2); // two rows before row 5
        assert_eq!(shift_structure("A4+A5+$A$9", "Data", &op), "A4+A7+$A$11");
        assert_eq!(shift_structure("SUM(A1:A10)", "Data", &op), "SUM(A1:A12)");
        assert_eq!(shift_structure("SUM(A6:A10)", "Data", &op), "SUM(A8:A12)");
        assert_eq!(shift_structure("SUM(A:A)+SUM(5:6)", "Data", &op), "SUM(A:A)+SUM(7:8)");
        assert_eq!(shift_structure("A5+Data!A5+Other!A5", "Other", &op), "A5+Data!A7+Other!A5");
        assert_eq!(shift_structure("\"A5\"&A5", "Data", &op), "\"A5\"&A7");
        // Formatting and case of what did not move stay as written.
        assert_eq!(shift_structure("sum( a1 ) + b2", "Data", &op), "sum( a1 ) + b2");
    }

    #[test]
    fn deleting_rows_shrinks_and_breaks() {
        let op = rows("Data", 4, -2); // rows 5 and 6 deleted
        assert_eq!(shift_structure("A4+A7+A5", "Data", &op), "A4+A5+#REF!");
        assert_eq!(shift_structure("SUM(A1:A10)", "Data", &op), "SUM(A1:A8)");
        assert_eq!(shift_structure("SUM(A5:A6)", "Data", &op), "SUM(#REF!)");
        assert_eq!(shift_structure("SUM(A6:A9)", "Data", &op), "SUM(A5:A7)");
        assert_eq!(shift_structure("Data!A5", "", &op), "#REF!");
    }

    #[test]
    fn columns_and_bands() {
        let op = cols("S", 1, 1); // a column before B
        assert_eq!(shift_structure("A1+B1+$C$1+SUM(A:C)+SUM(1:1)", "S", &op), "A1+C1+$D$1+SUM(A:D)+SUM(1:1)");
        let del = cols("S", 1, -1);
        assert_eq!(shift_structure("A1+B1+C1+SUM(A1:C1)", "S", &del), "A1+#REF!+B1+SUM(A1:B1)");
        // Insert cells shifting down in columns B:C only.
        let band = Shift { sheet: "S".into(), axis: Axis::Row, at: 2, count: 1, band: Some((1, 2)) };
        assert_eq!(shift_structure("A5+B5+C3+D5+SUM(B1:C9)+SUM(A1:D9)", "S", &band), "A5+B6+C4+D5+SUM(B1:C10)+SUM(A1:D9)");
        assert_eq!(band.range(&CellRange::new(4, 1, 4, 1)), Some(CellRange::new(5, 1, 5, 1)));
        assert_eq!(rows("S", 0, -1).range(&CellRange::new(0, 0, 0, 3)), None);
    }

    #[test]
    fn spans() {
        assert_eq!(shift_span(5, 5, 5, 1, 100), Some((6, 6)));
        assert_eq!(shift_span(4, 4, 5, 1, 100), Some((4, 4)));
        assert_eq!(shift_span(99, 99, 5, 1, 100), None, "pushed off the sheet");
        assert_eq!(shift_span(2, 8, 5, -2, 100), Some((2, 6)));
        assert_eq!(shift_span(5, 8, 5, -2, 100), Some((5, 6)));
        assert_eq!(shift_span(2, 5, 5, -2, 100), Some((2, 4)));
    }

    #[test]
    fn moves_renames_and_deletes() {
        let from = CellRange::new(0, 0, 1, 0); // A1:A2
        assert_eq!(move_refs("A1*2+A2+A3+SUM(A1:A2)+SUM(A1:A3)+$A$1", "S", "S", &from, 0, 2), "C1*2+C2+A3+SUM(C1:C2)+SUM(A1:A3)+$C$1");
        assert_eq!(move_refs("S!A1+A1", "T", "S", &from, 1, 0), "S!A2+A1");
        assert_eq!(rename_sheet("Data!A1+'data'!B2+Other!C3", "Data", "My Data"), "'My Data'!A1+'My Data'!B2+Other!C3");
        assert_eq!(rename_sheet("'Old one'!A1", "Old one", "New"), "New!A1");
        assert_eq!(drop_sheet("Gone!A1+Kept!B2+SUM('Gone'!A1:A3)", "Gone"), "#REF!+Kept!B2+SUM(#REF!)");
        assert!(mentions_sheet("SUM(A1)", "Data", "Data"));
        assert!(!mentions_sheet("Other!A1", "Data", "Data"));
    }

    #[test]
    fn quoting() {
        assert_eq!(quote_sheet("Sheet1"), "Sheet1");
        assert_eq!(quote_sheet("My Sheet"), "'My Sheet'");
        assert_eq!(quote_sheet("A1"), "'A1'");
        assert_eq!(quote_sheet("2024"), "'2024'");
        assert_eq!(quote_sheet("Bob's"), "'Bob''s'");
        assert_eq!(quote_sheet("R1C1"), "'R1C1'");
    }
}
