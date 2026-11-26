//! The worksheet functions, by category. Each category answers `None` for a name it does not
//! hold; the dispatcher tries them in turn.

mod array;
mod date;
mod fin;
mod logic;
mod lookup;
mod math;
mod stats;
mod text;

use super::eval::{Ctx, Frame};
use super::parse::Expr;
use super::value::*;

pub use date::{now_serial, set_clock, set_seed};

/// Every function the engine works out (the first 120 of the catalogue plus a few old names and
/// companions).
pub const FUNCTIONS: &[&str] = &[
    // The catalogue's first 120, in its order.
    "SUM", "IF", "AVERAGE", "COUNT", "COUNTA", "MIN", "MAX", "ROUND", "AND", "OR", "NOT", "IFERROR", "VLOOKUP", "XLOOKUP", "INDEX", "MATCH", "SUMIF", "SUMIFS", "COUNTIF", "COUNTIFS", "AVERAGEIF",
    "AVERAGEIFS", "TODAY", "NOW", "DATE", "YEAR", "MONTH", "DAY", "LEFT", "RIGHT", "MID", "LEN", "TRIM", "CONCAT", "CONCATENATE", "TEXTJOIN", "TEXT", "VALUE", "UPPER", "LOWER", "PROPER",
    "SUBSTITUTE", "FIND", "SEARCH", "ROUNDUP", "ROUNDDOWN", "INT", "ABS", "MOD", "SUMPRODUCT", "SUBTOTAL", "IFS", "SWITCH", "IFNA", "ISBLANK", "ISNUMBER", "ISTEXT", "ISERROR", "NA",
    "COUNTBLANK", "MAXIFS", "MINIFS", "MEDIAN", "LARGE", "SMALL", "RANK.EQ", "EDATE", "EOMONTH", "WEEKDAY", "NETWORKDAYS", "WORKDAY", "DATEDIF", "HOUR", "MINUTE", "SECOND", "TIME",
    "DATEVALUE", "HLOOKUP", "LOOKUP", "XMATCH", "CHOOSE", "INDIRECT", "OFFSET", "ROW", "COLUMN", "ROWS", "COLUMNS", "HYPERLINK", "TRANSPOSE", "FILTER", "SORT", "SORTBY", "UNIQUE",
    "SEQUENCE", "LET", "LAMBDA", "TEXTBEFORE", "TEXTAFTER", "TEXTSPLIT", "REPLACE", "REPT", "EXACT", "PMT", "FV", "PV", "NPV", "IRR", "RATE", "NPER", "STDEV.S", "STDEV.P", "VAR.S",
    "POWER", "SQRT", "RAND", "RANDBETWEEN", "CEILING.MATH", "PRODUCT", "TRUNC", "VSTACK",
    // Old names and close companions that cost nothing more.
    "RANK", "STDEV", "STDEVP", "VAR", "VAR.P", "VARP", "XOR", "TRUE", "FALSE", "ISERR", "ISNA", "ISLOGICAL", "ISNONTEXT", "N", "T", "SINGLE",
];

/// Functions worked out again on every recalculation.
pub const VOLATILE: &[&str] = &["TODAY", "NOW", "RAND", "RANDBETWEEN", "INDIRECT", "OFFSET"];

/// Calls a function by its upper-case name; `None` when no category holds it.
pub fn call(cx: &Ctx, name: &str, a: &[Expr], f: &Frame) -> Option<Value> {
    let r = math::call(cx, name, a, f)
        .or_else(|| stats::call(cx, name, a, f))
        .or_else(|| logic::call(cx, name, a, f))
        .or_else(|| text::call(cx, name, a, f))
        .or_else(|| date::call(cx, name, a, f))
        .or_else(|| lookup::call(cx, name, a, f))
        .or_else(|| array::call(cx, name, a, f))
        .or_else(|| fin::call(cx, name, a, f))?;
    Some(r.unwrap_or_else(Value::Err))
}

pub(crate) type R = Result<Value, ErrKind>;

/// Checks the argument count (a wrong count is `#VALUE!` at run time; the spreadsheet would refuse
/// the formula).
pub(crate) fn need(a: &[Expr], min: usize, max: usize) -> Result<(), ErrKind> {
    if a.len() < min || a.len() > max {
        Err(ErrKind::Value)
    } else {
        Ok(())
    }
}

/// A scalar function lifted over arrays: evaluates the arguments and applies `func` (element by
/// element when one is an array or a range).
pub(crate) fn lifted(cx: &Ctx, a: &[Expr], f: &Frame, min: usize, max: usize, func: &dyn Fn(&[Value]) -> R) -> R {
    need(a, min, max)?;
    let vals: Vec<Value> = a.iter().map(|e| cx.ev(e, f)).collect();
    Ok(cx.lift(vals, &|v| func(v).unwrap_or_else(Value::Err)))
}

/// The numbers of the arguments the way SUM reads them: numbers in ranges and arrays (text,
/// booleans and blanks there skipped), and given directly numbers, booleans and number-like
/// text (other text `#VALUE!`); the first error met is the answer.
pub(crate) fn nums(cx: &Ctx, a: &[Expr], f: &Frame) -> Result<Vec<f64>, ErrKind> {
    let mut out = Vec::new();
    for e in a {
        let v = cx.ev(e, f);
        nums_of(cx, &v, &mut out)?;
    }
    Ok(out)
}

pub(crate) fn nums_of(cx: &Ctx, v: &Value, out: &mut Vec<f64>) -> Result<(), ErrKind> {
    let mut err = None;
    cx.each(v, &mut |x, direct| {
        if err.is_some() {
            return;
        }
        match x {
            Value::Num(n) => out.push(*n),
            Value::Err(k) => err = Some(*k),
            Value::Bool(b) if direct => out.push(if *b { 1.0 } else { 0.0 }),
            Value::Text(s) if direct => match parse_number_text(s, cx.date1904) {
                Some(n) => out.push(n),
                None => err = Some(ErrKind::Value),
            },
            Value::Blank if direct => out.push(0.0),
            _ => {}
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// A criterion of COUNTIF and friends: `">=10"`, `"<>"`, `"app*"`, `5`, `TRUE`.
#[derive(Debug, Clone)]
pub(crate) struct Criterion {
    op: CmpOp,
    val: CritVal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone)]
enum CritVal {
    Num(f64),
    Text(String),
    Bool(bool),
    Err(ErrKind),
    /// `""`: empty cells and empty text.
    Blank,
    /// `"="`: only truly empty cells.
    Empty,
}

impl Criterion {
    pub(crate) fn parse(v: &Value, date1904: bool) -> Criterion {
        match v {
            Value::Num(n) => Criterion { op: CmpOp::Eq, val: CritVal::Num(*n) },
            Value::Bool(b) => Criterion { op: CmpOp::Eq, val: CritVal::Bool(*b) },
            Value::Err(e) => Criterion { op: CmpOp::Eq, val: CritVal::Err(*e) },
            Value::Blank => Criterion { op: CmpOp::Eq, val: CritVal::Num(0.0) },
            Value::Text(s) => {
                let (op, rest) = if let Some(r) = s.strip_prefix("<=") {
                    (CmpOp::Le, r)
                } else if let Some(r) = s.strip_prefix(">=") {
                    (CmpOp::Ge, r)
                } else if let Some(r) = s.strip_prefix("<>") {
                    (CmpOp::Ne, r)
                } else if let Some(r) = s.strip_prefix('<') {
                    (CmpOp::Lt, r)
                } else if let Some(r) = s.strip_prefix('>') {
                    (CmpOp::Gt, r)
                } else if let Some(r) = s.strip_prefix('=') {
                    (CmpOp::Eq, r)
                } else {
                    (CmpOp::Eq, s.as_str())
                };
                let val = if rest.is_empty() {
                    if s.is_empty() {
                        CritVal::Blank
                    } else {
                        CritVal::Empty
                    }
                } else if let Some(n) = parse_number_text(rest, date1904) {
                    CritVal::Num(n)
                } else if rest.eq_ignore_ascii_case("TRUE") {
                    CritVal::Bool(true)
                } else if rest.eq_ignore_ascii_case("FALSE") {
                    CritVal::Bool(false)
                } else if let Some(e) = ErrKind::parse(rest) {
                    CritVal::Err(e)
                } else {
                    CritVal::Text(rest.to_string())
                };
                Criterion { op, val }
            }
            _ => Criterion { op: CmpOp::Eq, val: CritVal::Err(ErrKind::Value) },
        }
    }

    fn equals(&self, cell: &Value) -> bool {
        match (&self.val, cell) {
            (CritVal::Num(n), Value::Num(x)) => num_eq(*n, *x),
            (CritVal::Num(n), Value::Text(t)) => parse_number_text(t, false).is_some_and(|x| num_eq(*n, x)),
            (CritVal::Text(p), Value::Text(t)) => {
                if has_wildcard(p) {
                    wild_match(p, t)
                } else {
                    p.to_lowercase() == t.to_lowercase()
                }
            }
            (CritVal::Bool(b), Value::Bool(x)) => b == x,
            (CritVal::Err(e), Value::Err(x)) => e == x,
            (CritVal::Blank, Value::Blank) => true,
            (CritVal::Blank, Value::Text(t)) => t.is_empty(),
            (CritVal::Empty, Value::Blank) => true,
            _ => false,
        }
    }

    pub(crate) fn matches(&self, cell: &Value) -> bool {
        match self.op {
            CmpOp::Eq => self.equals(cell),
            CmpOp::Ne => match self.val {
                CritVal::Empty | CritVal::Blank => !matches!(cell, Value::Blank),
                _ => !self.equals(cell),
            },
            op => {
                let ord = match (&self.val, cell) {
                    (CritVal::Num(n), Value::Num(x)) => {
                        if num_eq(*x, *n) {
                            std::cmp::Ordering::Equal
                        } else {
                            x.partial_cmp(n).unwrap_or(std::cmp::Ordering::Equal)
                        }
                    }
                    (CritVal::Text(p), Value::Text(t)) => text_cmp(t, p),
                    (CritVal::Bool(b), Value::Bool(x)) => x.cmp(b),
                    _ => return false,
                };
                match op {
                    CmpOp::Lt => ord.is_lt(),
                    CmpOp::Le => ord.is_le(),
                    CmpOp::Gt => ord.is_gt(),
                    _ => ord.is_ge(),
                }
            }
        }
    }
}

/// Positions (row-major indices) of a rectangle matching every (range, criterion) pair; all the
/// ranges must share one shape.
pub(crate) fn criteria_hits(cx: &Ctx, pairs: &[(Value, Value)], shape: Option<(usize, usize)>) -> Result<(Vec<bool>, usize, usize), ErrKind> {
    let mut mask: Option<Vec<bool>> = None;
    let (mut rows, mut cols) = shape.unwrap_or((0, 0));
    for (range, crit) in pairs {
        let arr = cx.array(range);
        if mask.is_none() && shape.is_none() {
            rows = arr.rows;
            cols = arr.cols;
        }
        if arr.rows != rows || arr.cols != cols {
            return Err(ErrKind::Value);
        }
        let c = Criterion::parse(crit, cx.date1904);
        let m = mask.get_or_insert_with(|| vec![true; rows * cols]);
        for (i, v) in arr.data.iter().enumerate() {
            if m[i] && !c.matches(v) {
                m[i] = false;
            }
        }
    }
    Ok((mask.unwrap_or_else(|| vec![true; rows * cols]), rows, cols))
}
