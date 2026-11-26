//! Formula text, for M1 only what the formula bar needs: a shared formula's master text moved to
//! each child cell by shifting its relative references (`$` parts stay). Strings, quoted sheet
//! names and bracketed parts (structured and external references) are copied untouched; a name
//! followed by `(` is a function. No evaluation here: cached values are shown.

use super::cellref::{col_index, col_name, MAX_COLS, MAX_ROWS};

fn is_tok_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '$' || c == '_' || c == '.' || c == '\\'
}

/// `A1`-style token shifted by (dr, dc); `colon` says whether it touches a `:` (then bare columns
/// `A` and bare rows `3` are references too). `None` when the token is not a reference.
fn shift_token(tok: &str, dr: i64, dc: i64, colon: bool) -> Option<String> {
    let b = tok.as_bytes();
    let mut i = 0;
    let col_abs = i < b.len() && b[i] == b'$';
    if col_abs {
        i += 1;
    }
    let ls = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    let letters = &tok[ls..i];
    let row_abs = i < b.len() && b[i] == b'$';
    let after_letters = i;
    if row_abs {
        i += 1;
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let digits = &tok[ds..i];
    if i != b.len() {
        return None;
    }
    let shift_col = |c: u32| -> Option<String> {
        let n = if col_abs { c as i64 } else { c as i64 + dc };
        if n < 0 || n >= MAX_COLS as i64 {
            None
        } else {
            Some(format!("{}{}", if col_abs { "$" } else { "" }, col_name(n as u32)))
        }
    };
    let shift_row = |r: u32, abs: bool| -> Option<String> {
        let n = if abs { r as i64 } else { r as i64 + dr };
        if n < 1 || n > MAX_ROWS as i64 {
            None
        } else {
            Some(format!("{}{}", if abs { "$" } else { "" }, n))
        }
    };
    if !letters.is_empty() && !digits.is_empty() {
        let c = col_index(letters)?;
        let r: u32 = digits.parse().ok().filter(|&r| r >= 1 && r <= MAX_ROWS)?;
        return Some(match (shift_col(c), shift_row(r, row_abs)) {
            (Some(a), Some(b)) => format!("{a}{b}"),
            _ => "#REF!".to_string(),
        });
    }
    if !colon {
        return None;
    }
    if !letters.is_empty() && digits.is_empty() && !row_abs {
        let c = col_index(letters)?;
        return Some(shift_col(c).unwrap_or_else(|| "#REF!".into()));
    }
    if letters.is_empty() && !digits.is_empty() && !col_abs {
        // `3` or `$3` (the `$` was read as the column's anchor when there are no letters).
        let abs = row_abs || tok.starts_with('$');
        let r: u32 = digits.parse().ok().filter(|&r| r >= 1 && r <= MAX_ROWS)?;
        let _ = after_letters;
        return Some(shift_row(r, abs).unwrap_or_else(|| "#REF!".into()));
    }
    if letters.is_empty() && !digits.is_empty() && col_abs {
        let r: u32 = digits.parse().ok().filter(|&r| r >= 1 && r <= MAX_ROWS)?;
        return Some(shift_row(r, true).unwrap_or_else(|| "#REF!".into()));
    }
    None
}

/// Moves a formula written for one cell to a cell `dr` rows and `dc` columns away.
pub fn shift_formula(f: &str, dr: i64, dc: i64) -> String {
    if dr == 0 && dc == 0 {
        return f.to_string();
    }
    let ch: Vec<char> = f.chars().collect();
    let n = ch.len();
    let mut out = String::with_capacity(f.len() + 8);
    let mut i = 0;
    while i < n {
        let c = ch[i];
        match c {
            '"' | '\'' => {
                // A string or a quoted sheet name; a doubled quote is an escaped one.
                out.push(c);
                i += 1;
                while i < n {
                    out.push(ch[i]);
                    if ch[i] == c {
                        if i + 1 < n && ch[i + 1] == c {
                            out.push(c);
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            '[' => {
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
            }
            _ if is_tok_char(c) => {
                let start = i;
                while i < n && is_tok_char(ch[i]) {
                    i += 1;
                }
                let tok: String = ch[start..i].iter().collect();
                let next = ch.get(i).copied();
                let prev = if start > 0 { Some(ch[start - 1]) } else { None };
                if next == Some('(') || next == Some('!') || next == Some('[') {
                    out.push_str(&tok);
                    continue;
                }
                let colon = next == Some(':') || prev == Some(':');
                match shift_token(&tok, dr, dc, colon) {
                    Some(s) => out.push_str(&s),
                    None => out.push_str(&tok),
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_parts_move_absolute_parts_stay() {
        assert_eq!(shift_formula("A1*2", 1, 0), "A2*2");
        assert_eq!(shift_formula("$A1+A$1+$A$1", 2, 3), "$A3+D$1+$A$1");
        assert_eq!(shift_formula("SUM(A1:B2)", 1, 1), "SUM(B2:C3)");
        assert_eq!(shift_formula("LOG10(A1)", 1, 0), "LOG10(A2)");
        assert_eq!(shift_formula("\"A1\"&A1", 1, 0), "\"A1\"&A2");
        assert_eq!(shift_formula("'My A1'!B2+Sheet2!C3", 1, 0), "'My A1'!B3+Sheet2!C4");
        assert_eq!(shift_formula("SUM(A:A)+SUM(3:3)", 1, 1), "SUM(B:B)+SUM(4:4)");
        assert_eq!(shift_formula("A1", -1, 0), "#REF!");
        assert_eq!(shift_formula("Table1[[#This Row],[A1]]+TRUE", 1, 0), "Table1[[#This Row],[A1]]+TRUE");
        assert_eq!(shift_formula("1.5E3+A1", 1, 0), "1.5E3+A2");
    }
}
