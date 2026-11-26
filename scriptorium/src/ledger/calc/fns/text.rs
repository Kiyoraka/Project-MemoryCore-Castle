//! Text: LEFT, RIGHT, MID, LEN, TRIM, UPPER, LOWER, PROPER, SUBSTITUTE, REPLACE, FIND, SEARCH,
//! CONCAT, CONCATENATE, TEXTJOIN, TEXT (through the number format engine), VALUE, REPT, EXACT,
//! HYPERLINK, TEXTBEFORE, TEXTAFTER, TEXTSPLIT. Positions count characters.

use std::rc::Rc;

use super::super::super::model::CellValue;
use super::super::super::numfmt::format_value_full;
use super::super::eval::{Ctx, Frame};
use super::super::parse::Expr;
use super::super::value::*;
use super::{lifted, need, R};

fn t(v: &[Value], i: usize) -> Result<String, ErrKind> {
    to_text(&v[i])
}

fn n(v: &[Value], i: usize, d: f64) -> Result<f64, ErrKind> {
    match v.get(i) {
        Some(x) => to_num(x),
        None => Ok(d),
    }
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

const MAX_TEXT: usize = 32_767;

fn text_ok(s: String) -> R {
    if s.chars().count() > MAX_TEXT {
        Err(ErrKind::Value)
    } else {
        Ok(Value::Text(s))
    }
}

/// The 1-based position of `pat` in `s` from `start`, case-sensitive (FIND) or not with wildcards
/// (SEARCH).
fn find_in(pat: &str, s: &str, start: f64, search: bool) -> R {
    let hay = chars(s);
    let start = start.trunc();
    if start < 1.0 || start as usize > hay.len() + 1 {
        return Err(ErrKind::Value);
    }
    let from = start as usize - 1;
    if pat.is_empty() {
        return Ok(Value::Num(start));
    }
    if search {
        let p = format!("{pat}*");
        for i in from..hay.len() {
            let rest: String = hay[i..].iter().collect();
            if wild_match(&p, &rest) {
                return Ok(Value::Num(i as f64 + 1.0));
            }
        }
    } else {
        let needle = chars(pat);
        if needle.len() <= hay.len() {
            for i in from..=hay.len() - needle.len() {
                if hay[i..i + needle.len()] == needle[..] {
                    return Ok(Value::Num(i as f64 + 1.0));
                }
            }
        }
    }
    Err(ErrKind::Value)
}

fn proper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_letter = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            if prev_letter {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            prev_letter = true;
        } else {
            out.push(c);
            prev_letter = false;
        }
    }
    out
}

fn trim(s: &str) -> String {
    s.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

fn substitute(s: &str, old: &str, new: &str, instance: Option<usize>) -> String {
    if old.is_empty() {
        return s.to_string();
    }
    match instance {
        None => s.replace(old, new),
        Some(k) => {
            let mut out = String::new();
            let mut rest = s;
            let mut seen = 0;
            while let Some(i) = rest.find(old) {
                seen += 1;
                if seen == k {
                    out.push_str(&rest[..i]);
                    out.push_str(new);
                    out.push_str(&rest[i + old.len()..]);
                    return out;
                }
                out.push_str(&rest[..i + old.len()]);
                rest = &rest[i + old.len()..];
            }
            out.push_str(rest);
            out
        }
    }
}

/// Every string of a value (a range read densely, so empty cells give "").
fn strings_of(cx: &Ctx, v: &Value) -> Result<Vec<String>, ErrKind> {
    let arr = match v {
        Value::Ref(_) | Value::Array(_) => cx.array(v),
        other => Rc::new(Array::scalar(other.clone())),
    };
    arr.data.iter().map(to_text).collect()
}

/// A delimiter argument: one text or an array of texts.
fn delims(cx: &Ctx, v: &Value) -> Result<Vec<String>, ErrKind> {
    match v {
        Value::Ref(_) | Value::Array(_) => cx.array(v).data.iter().map(to_text).collect(),
        other => Ok(vec![to_text(other)?]),
    }
}

/// Non-overlapping matches of any delimiter, left to right: (start, end) in characters.
fn matches_of(s: &[char], ds: &[Vec<char>], ci: bool) -> Vec<(usize, usize)> {
    let low = |c: &char| if ci { c.to_lowercase().next().unwrap_or(*c) } else { *c };
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let mut hit = None;
        for d in ds {
            if d.is_empty() || i + d.len() > s.len() {
                continue;
            }
            if s[i..i + d.len()].iter().map(low).eq(d.iter().map(low)) && hit.map_or(true, |h: usize| d.len() > h) {
                hit = Some(d.len());
            }
        }
        match hit {
            Some(l) => {
                out.push((i, i + l));
                i += l;
            }
            None => i += 1,
        }
    }
    out
}

fn split_by(s: &str, ds: &[String], ci: bool, ignore_empty: bool) -> Vec<String> {
    let c = chars(s);
    let dv: Vec<Vec<char>> = ds.iter().map(|d| chars(d)).collect();
    let mut out = Vec::new();
    let mut last = 0;
    for (a, b) in matches_of(&c, &dv, ci) {
        out.push(c[last..a].iter().collect::<String>());
        last = b;
    }
    out.push(c[last..].iter().collect());
    if ignore_empty {
        out.retain(|p| !p.is_empty());
    }
    out
}

fn before_after(cx: &Ctx, a: &[Expr], f: &Frame, before: bool) -> R {
    need(a, 2, 6)?;
    let ds = delims(cx, &cx.ev(&a[1], f))?;
    let instance = cx.num_def(a, 2, 1.0, f)?.trunc() as i64;
    let ci = cx.num_def(a, 3, 0.0, f)? != 0.0;
    let match_end = cx.num_def(a, 4, 0.0, f)? != 0.0;
    let not_found = match a.get(5) {
        Some(e) if !matches!(e, Expr::Missing) => Some(cx.ev_s(e, f)),
        _ => None,
    };
    let textv = cx.ev(&a[0], f);
    Ok(cx.lift(vec![textv], &|v| {
        let run = || -> R {
            let s = chars(&to_text(&v[0])?);
            if instance == 0 || instance.unsigned_abs() as usize > s.len() + 1 {
                return Err(ErrKind::Value);
            }
            let dv: Vec<Vec<char>> = ds.iter().map(|d| chars(d)).collect();
            let mut pos = if dv.iter().any(|d| d.is_empty()) {
                // An empty delimiter matches at once (at the start, or at the end counting back).
                if instance > 0 {
                    vec![(0, 0)]
                } else {
                    vec![(s.len(), s.len())]
                }
            } else {
                matches_of(&s, &dv, ci)
            };
            if match_end {
                if instance > 0 {
                    pos.push((s.len(), s.len()));
                } else {
                    pos.insert(0, (0, 0));
                }
            }
            let hit = if instance > 0 { pos.get(instance as usize - 1) } else { pos.len().checked_sub(instance.unsigned_abs() as usize).and_then(|i| pos.get(i)) };
            match hit {
                Some(&(st, en)) => Ok(Value::Text(if before { s[..st].iter().collect() } else { s[en..].iter().collect() })),
                None => match &not_found {
                    Some(x) => Ok(x.clone()),
                    None => Err(ErrKind::NA),
                },
            }
        };
        run().unwrap_or_else(Value::Err)
    }))
}

fn textsplit(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 6)?;
    let s = cx.text(&a[0], f)?;
    let col_d = match &a[1] {
        Expr::Missing => Vec::new(),
        e => delims(cx, &cx.ev(e, f))?,
    };
    let row_d = match a.get(2) {
        Some(e) if !matches!(e, Expr::Missing) => delims(cx, &cx.ev(e, f))?,
        _ => Vec::new(),
    };
    if col_d.is_empty() && row_d.is_empty() {
        return Err(ErrKind::Value);
    }
    let ignore = cx.bool_or(a, 3, false, f)?;
    let ci = cx.num_def(a, 4, 0.0, f)? != 0.0;
    let pad = match a.get(5) {
        Some(e) if !matches!(e, Expr::Missing) => cx.ev_s(e, f),
        _ => Value::Err(ErrKind::NA),
    };
    let rows: Vec<String> = if row_d.is_empty() { vec![s] } else { split_by(&s, &row_d, ci, ignore) };
    let grid: Vec<Vec<String>> = rows.iter().map(|r| if col_d.is_empty() { vec![r.clone()] } else { split_by(r, &col_d, ci, ignore) }).collect();
    let width = grid.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
    let mut data = Vec::new();
    for r in &grid {
        for i in 0..width {
            data.push(match r.get(i) {
                Some(p) => Value::Text(p.clone()),
                None => pad.clone(),
            });
        }
    }
    let rows_n = grid.len().max(1);
    if data.is_empty() {
        return Err(ErrKind::Calc);
    }
    Ok(Value::Array(Rc::new(Array::new(rows_n, width, data))))
}

/// TEXT: the number format engine on the value (number-like text taken as its number).
fn text_fn(v: &Value, code: &str, date1904: bool) -> R {
    let cv = match v {
        Value::Num(x) => CellValue::Number(*x),
        Value::Blank => CellValue::Number(0.0),
        Value::Bool(b) => return Ok(Value::Text(if *b { "TRUE".into() } else { "FALSE".into() })),
        Value::Text(s) => match parse_number_text(s, date1904) {
            Some(x) => CellValue::Number(x),
            None => CellValue::Text(s.clone()),
        },
        Value::Err(e) => return Err(*e),
        _ => return Err(ErrKind::Value),
    };
    let out = format_value_full(&cv, code, date1904);
    if out.hashes {
        return Err(ErrKind::Value);
    }
    Ok(Value::Text(out.text))
}

pub fn call(cx: &Ctx, name: &str, a: &[Expr], f: &Frame) -> Option<R> {
    let d1904 = cx.date1904;
    Some(match name {
        "LEFT" | "RIGHT" => {
            let right = name == "RIGHT";
            lifted(cx, a, f, 1, 2, &move |v| {
                let s = chars(&t(v, 0)?);
                let k = n(v, 1, 1.0)?;
                if k < 0.0 {
                    return Err(ErrKind::Value);
                }
                let k = (k.trunc() as usize).min(s.len());
                Ok(Value::Text(if right { s[s.len() - k..].iter().collect() } else { s[..k].iter().collect() }))
            })
        }
        "MID" => lifted(cx, a, f, 3, 3, &|v| {
            let s = chars(&t(v, 0)?);
            let (st, k) = (to_num(&v[1])?.trunc(), to_num(&v[2])?.trunc());
            if st < 1.0 || k < 0.0 {
                return Err(ErrKind::Value);
            }
            let from = (st as usize - 1).min(s.len());
            let to = (from + k as usize).min(s.len());
            Ok(Value::Text(s[from..to].iter().collect()))
        }),
        "LEN" => lifted(cx, a, f, 1, 1, &|v| Ok(Value::Num(t(v, 0)?.chars().count() as f64))),
        "TRIM" => lifted(cx, a, f, 1, 1, &|v| Ok(Value::Text(trim(&t(v, 0)?)))),
        "UPPER" => lifted(cx, a, f, 1, 1, &|v| Ok(Value::Text(t(v, 0)?.to_uppercase()))),
        "LOWER" => lifted(cx, a, f, 1, 1, &|v| Ok(Value::Text(t(v, 0)?.to_lowercase()))),
        "PROPER" => lifted(cx, a, f, 1, 1, &|v| Ok(Value::Text(proper(&t(v, 0)?)))),
        "SUBSTITUTE" => lifted(cx, a, f, 3, 4, &|v| {
            let inst = match v.get(3) {
                Some(x) => {
                    let k = to_num(x)?.trunc();
                    if k < 1.0 {
                        return Err(ErrKind::Value);
                    }
                    Some(k as usize)
                }
                None => None,
            };
            text_ok(substitute(&t(v, 0)?, &t(v, 1)?, &t(v, 2)?, inst))
        }),
        "REPLACE" => lifted(cx, a, f, 4, 4, &|v| {
            let s = chars(&t(v, 0)?);
            let (st, k) = (to_num(&v[1])?.trunc(), to_num(&v[2])?.trunc());
            if st < 1.0 || k < 0.0 {
                return Err(ErrKind::Value);
            }
            let from = (st as usize - 1).min(s.len());
            let to = (from + k as usize).min(s.len());
            let mut out: String = s[..from].iter().collect();
            out.push_str(&t(v, 3)?);
            out.extend(s[to..].iter());
            text_ok(out)
        }),
        "FIND" => lifted(cx, a, f, 2, 3, &|v| find_in(&t(v, 0)?, &t(v, 1)?, n(v, 2, 1.0)?, false)),
        "SEARCH" => lifted(cx, a, f, 2, 3, &|v| find_in(&t(v, 0)?, &t(v, 1)?, n(v, 2, 1.0)?, true)),
        "CONCAT" => (|| {
            need(a, 1, 253)?;
            let mut out = String::new();
            for e in a {
                for s in strings_of(cx, &cx.ev(e, f))? {
                    out.push_str(&s);
                }
            }
            text_ok(out)
        })(),
        "CONCATENATE" => lifted(cx, a, f, 1, 255, &|v| {
            let mut out = String::new();
            for x in v {
                out.push_str(&to_text(x)?);
            }
            text_ok(out)
        }),
        "TEXTJOIN" => (|| {
            need(a, 3, 255)?;
            let ds = delims(cx, &cx.ev(&a[0], f))?;
            let ignore = cx.boolean(&a[1], f)?;
            let mut parts = Vec::new();
            for e in &a[2..] {
                for s in strings_of(cx, &cx.ev(e, f))? {
                    if !(ignore && s.is_empty()) {
                        parts.push(s);
                    }
                }
            }
            let mut out = String::new();
            for (i, p) in parts.iter().enumerate() {
                if i > 0 && !ds.is_empty() {
                    out.push_str(&ds[(i - 1) % ds.len()]);
                }
                out.push_str(p);
            }
            text_ok(out)
        })(),
        "TEXT" => lifted(cx, a, f, 2, 2, &move |v| text_fn(&v[0], &t(v, 1)?, d1904)),
        "VALUE" => lifted(cx, a, f, 1, 1, &move |v| match &v[0] {
            Value::Num(x) => Ok(Value::Num(*x)),
            Value::Blank => Ok(Value::Num(0.0)),
            Value::Text(s) => parse_number_text(s, d1904).map(Value::Num).ok_or(ErrKind::Value),
            Value::Err(e) => Err(*e),
            _ => Err(ErrKind::Value),
        }),
        "REPT" => lifted(cx, a, f, 2, 2, &|v| {
            let k = to_num(&v[1])?.trunc();
            let s = t(v, 0)?;
            if k < 0.0 || s.chars().count() as f64 * k > MAX_TEXT as f64 {
                return Err(ErrKind::Value);
            }
            Ok(Value::Text(s.repeat(k as usize)))
        }),
        "EXACT" => lifted(cx, a, f, 2, 2, &|v| Ok(Value::Bool(t(v, 0)? == t(v, 1)?))),
        "HYPERLINK" => lifted(cx, a, f, 1, 2, &|v| match v.get(1) {
            Some(x) => Ok(x.clone()),
            None => Ok(Value::Text(t(v, 0)?)),
        }),
        "TEXTBEFORE" => before_after(cx, a, f, true),
        "TEXTAFTER" => before_after(cx, a, f, false),
        "TEXTSPLIT" => textsplit(cx, a, f),
        _ => return None,
    })
}
