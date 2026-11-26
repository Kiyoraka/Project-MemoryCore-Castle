//! Lookup and reference: VLOOKUP, HLOOKUP, LOOKUP, MATCH, XMATCH, XLOOKUP, INDEX, CHOOSE,
//! INDIRECT, OFFSET, ROW, COLUMN, ROWS, COLUMNS.
//!
//! Approximate matches walk the list in order and stop at the first value past the one looked
//! for, skipping values of another kind: on sorted data that is the binary search's answer, and
//! it keeps the `LOOKUP(2, 1/(cond), range)` "last match" idiom working.

use std::cmp::Ordering;
use std::rc::Rc;

use super::super::super::cellref::{CellRange, MAX_COLS, MAX_ROWS};
use super::super::eval::{Ctx, Frame};
use super::super::parse::{parse_formula, Expr};
use super::super::value::*;
use super::{need, R};

/// Same-kind comparison of a list value with the needle (`None` for another kind).
fn kind_cmp(v: &Value, needle: &Value) -> Option<Ordering> {
    match (v, needle) {
        (Value::Num(a), Value::Num(b)) => Some(if num_eq(*a, *b) { Ordering::Equal } else { a.partial_cmp(b)? }),
        (Value::Text(a), Value::Text(b)) => Some(text_cmp(a, b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn exact_eq(v: &Value, needle: &Value, wild: bool) -> bool {
    match (v, needle) {
        (Value::Text(a), Value::Text(p)) if wild && has_wildcard(p) => wild_match(p, a),
        (Value::Blank, Value::Blank) => true,
        _ => kind_cmp(v, needle) == Some(Ordering::Equal),
    }
}

/// The index of the needle: exact (`wild` allows wildcards), or approximate in ascending order
/// (largest <= needle) or descending order (smallest >= needle).
fn find(list: &[&Value], needle: &Value, mode: i32, wild: bool) -> Option<usize> {
    match mode {
        0 => list.iter().position(|v| exact_eq(v, needle, wild)),
        1 => {
            let mut cand = None;
            for (i, v) in list.iter().enumerate() {
                match kind_cmp(v, needle) {
                    Some(Ordering::Greater) => break,
                    Some(_) => cand = Some(i),
                    None => {}
                }
            }
            cand
        }
        _ => {
            let mut cand = None;
            for (i, v) in list.iter().enumerate() {
                match kind_cmp(v, needle) {
                    Some(Ordering::Less) => break,
                    Some(_) => cand = Some(i),
                    None => {}
                }
            }
            cand
        }
    }
}

fn needle_of(v: Value) -> Result<Value, ErrKind> {
    match v {
        Value::Err(e) => Err(e),
        other => Ok(other),
    }
}

fn vhlookup(cx: &Ctx, a: &[Expr], f: &Frame, vertical: bool) -> R {
    need(a, 3, 4)?;
    let table = cx.array(&cx.ev(&a[1], f));
    let idx = cx.num(&a[2], f)?.trunc();
    let approx = cx.bool_or(a, 3, true, f)?;
    let nv = cx.ev(&a[0], f);
    Ok(cx.lift(vec![nv], &|n| {
        let run = || -> R {
            let needle = needle_of(n[0].clone())?;
            if idx < 1.0 {
                return Err(ErrKind::Value);
            }
            let (len, width) = if vertical { (table.rows, table.cols) } else { (table.cols, table.rows) };
            if idx as usize > width {
                return Err(ErrKind::Ref);
            }
            let list: Vec<&Value> = (0..len).map(|i| if vertical { table.get(i, 0) } else { table.get(0, i) }).collect();
            let hit = find(&list, &needle, if approx { 1 } else { 0 }, !approx).ok_or(ErrKind::NA)?;
            let k = idx as usize - 1;
            Ok(if vertical { table.get(hit, k).clone() } else { table.get(k, hit).clone() })
        };
        run().unwrap_or_else(Value::Err)
    }))
}

fn vector(arr: &Array) -> Option<Vec<&Value>> {
    if arr.rows == 1 {
        Some((0..arr.cols).map(|c| arr.get(0, c)).collect())
    } else if arr.cols == 1 {
        Some((0..arr.rows).map(|r| arr.get(r, 0)).collect())
    } else {
        None
    }
}

/// XMATCH / XLOOKUP's search: match mode 0 exact, -1 exact or next smaller, 1 exact or next
/// larger, 2 wildcard; search mode 1 first to last, -1 last to first (2 and -2, the binary
/// searches, answer as 1 and -1 on sorted data).
fn xfind(list: &[&Value], needle: &Value, match_mode: i64, search_mode: i64) -> Result<Option<usize>, ErrKind> {
    if !matches!(match_mode, -1..=2) || !matches!(search_mode, -2 | -1 | 1 | 2) {
        return Err(ErrKind::Value);
    }
    let order: Vec<usize> = if search_mode < 0 { (0..list.len()).rev().collect() } else { (0..list.len()).collect() };
    let wild = match_mode == 2;
    if let Some(i) = order.iter().copied().find(|&i| exact_eq(list[i], needle, wild)) {
        return Ok(Some(i));
    }
    if match_mode == 0 || match_mode == 2 {
        return Ok(None);
    }
    let mut best: Option<usize> = None;
    for &i in &order {
        let Some(o) = kind_cmp(list[i], needle) else { continue };
        let ok = if match_mode == -1 { o == Ordering::Less } else { o == Ordering::Greater };
        if !ok {
            continue;
        }
        best = match best {
            None => Some(i),
            Some(b) => {
                let better = match kind_cmp(list[i], list[b]) {
                    Some(Ordering::Greater) => match_mode == -1,
                    Some(Ordering::Less) => match_mode == 1,
                    _ => false,
                };
                if better {
                    Some(i)
                } else {
                    Some(b)
                }
            }
        };
    }
    Ok(best)
}

fn xlookup(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 3, 6)?;
    let look_v = cx.ev(&a[1], f);
    let ret_v = cx.ev(&a[2], f);
    let look = cx.array(&look_v);
    let list = vector(&look).ok_or(ErrKind::Value)?;
    let vertical = look.cols == 1 && look.rows > 1 || (look.rows == 1 && look.cols == 1);
    let not_found = match a.get(3) {
        Some(e) if !matches!(e, Expr::Missing) => Some(cx.ev(e, f)),
        _ => None,
    };
    let mm = cx.num_or(a, 4, 0.0, f)?.trunc() as i64;
    let sm = match a.get(5) {
        Some(Expr::Missing) | None => 1,
        Some(e) => cx.num(e, f)?.trunc() as i64,
    };
    let ret = cx.array(&ret_v);
    let pick = |i: usize| -> R {
        if vertical {
            if ret.rows != look.rows {
                return Err(ErrKind::Value);
            }
            if let Value::Ref(area) = &ret_v {
                let r = area.range.r1 + i as u32;
                return Ok(Value::Ref(Area { sheet: area.sheet, range: CellRange::new(r, area.range.c1, r, area.range.c2) }));
            }
            Ok(if ret.cols == 1 { ret.get(i, 0).clone() } else { Value::Array(Rc::new(Array::row((0..ret.cols).map(|c| ret.get(i, c).clone()).collect()))) })
        } else {
            if ret.cols != look.cols {
                return Err(ErrKind::Value);
            }
            if let Value::Ref(area) = &ret_v {
                let c = area.range.c1 + i as u32;
                return Ok(Value::Ref(Area { sheet: area.sheet, range: CellRange::new(area.range.r1, c, area.range.r2, c) }));
            }
            Ok(if ret.rows == 1 { ret.get(0, i).clone() } else { Value::Array(Rc::new(Array::column((0..ret.rows).map(|r| ret.get(r, i).clone()).collect()))) })
        }
    };
    let one = |n: &Value| -> R {
        let needle = needle_of(n.clone())?;
        match xfind(&list, &needle, mm, sm)? {
            Some(i) => pick(i),
            None => match &not_found {
                Some(v) => Ok(v.clone()),
                None => Err(ErrKind::NA),
            },
        }
    };
    let nv = cx.ev(&a[0], f);
    if nv.is_multi() {
        return Ok(cx.lift(vec![nv], &|n| match one(&n[0]) {
            Ok(v) => cx.scalar(v, f),
            Err(e) => Value::Err(e),
        }));
    }
    one(&cx.scalar(nv, f))
}

fn index(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 4)?;
    let v = cx.ev(&a[0], f);
    let r = cx.num_or(a, 1, 0.0, f)?.trunc();
    let c = cx.num_or(a, 2, 0.0, f)?.trunc();
    if r < 0.0 || c < 0.0 {
        return Err(ErrKind::Value);
    }
    let (rows, cols) = match &v {
        Value::Ref(area) => (area.range.rows() as f64, area.range.cols() as f64),
        Value::Array(arr) => (arr.rows as f64, arr.cols as f64),
        Value::Err(e) => return Err(*e),
        _ => (1.0, 1.0),
    };
    // One index into a single row picks a column.
    let (r, c) = if a.len() == 2 && rows == 1.0 { (1.0, r) } else { (r, c) };
    if r > rows || c > cols {
        return Err(ErrKind::Ref);
    }
    match v {
        Value::Ref(area) => {
            let g = area.range;
            let (r1, r2) = if r == 0.0 { (g.r1, g.r2) } else { (g.r1 + r as u32 - 1, g.r1 + r as u32 - 1) };
            let (c1, c2) = if c == 0.0 { (g.c1, g.c2) } else { (g.c1 + c as u32 - 1, g.c1 + c as u32 - 1) };
            Ok(Value::Ref(Area { sheet: area.sheet, range: CellRange::new(r1, c1, r2, c2) }))
        }
        Value::Array(arr) => {
            let rs: Vec<usize> = if r == 0.0 { (0..arr.rows).collect() } else { vec![r as usize - 1] };
            let cs: Vec<usize> = if c == 0.0 { (0..arr.cols).collect() } else { vec![c as usize - 1] };
            let data: Vec<Value> = rs.iter().flat_map(|&i| cs.iter().map(move |&j| (i, j))).map(|(i, j)| arr.get(i, j).clone()).collect();
            if data.len() == 1 {
                Ok(data.into_iter().next().unwrap_or(Value::Blank))
            } else {
                Ok(Value::Array(Rc::new(Array::new(rs.len(), cs.len(), data))))
            }
        }
        other => Ok(other),
    }
}

/// `R2C3`, `R[1]C[-1]`, `R1C`, with an optional sheet, relative to the formula's cell.
fn parse_r1c1(text: &str, f: &Frame, cx: &Ctx) -> Option<Area> {
    let (sheet, body) = match text.rfind('!') {
        Some(i) => {
            let s = text[..i].trim_matches('\'').replace("''", "'");
            (cx.sheet_index(&s)?, &text[i + 1..])
        }
        None => (f.sheet, text),
    };
    let part = |s: &str, base: u32, max: u32| -> Option<(u32, usize)> {
        let b = s.as_bytes();
        if b.is_empty() || (b[0] != b'[' && !b[0].is_ascii_digit()) {
            return Some((base, 0));
        }
        if b[0] == b'[' {
            let end = s.find(']')?;
            let off: i64 = s[1..end].parse().ok()?;
            let v = base as i64 + off;
            if v < 0 || v >= max as i64 {
                return None;
            }
            Some((v as u32, end + 1))
        } else {
            let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
            let v: u32 = s[..end].parse().ok()?;
            if v == 0 || v > max {
                return None;
            }
            Some((v - 1, end))
        }
    };
    let one = |s: &str| -> Option<(u32, u32)> {
        let u = s.to_ascii_uppercase();
        let rest = u.strip_prefix('R')?;
        let (r, used) = part(rest, f.row, MAX_ROWS)?;
        let rest = rest[used..].strip_prefix('C')?;
        let (c, used) = part(rest, f.col, MAX_COLS)?;
        if used != rest.len() {
            return None;
        }
        Some((r, c))
    };
    let (a, b) = match body.split_once(':') {
        Some((x, y)) => (one(x)?, one(y)?),
        None => {
            let p = one(body)?;
            (p, p)
        }
    };
    Some(Area { sheet, range: CellRange::new(a.0, a.1, b.0, b.1) })
}

fn indirect(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 1, 2)?;
    let text = cx.text(&a[0], f)?;
    let a1 = cx.bool_or(a, 1, true, f)?;
    if !a1 {
        return parse_r1c1(text.trim(), f, cx).map(Value::Ref).ok_or(ErrKind::Ref);
    }
    let e = parse_formula(text.trim()).map_err(|_| ErrKind::Ref)?;
    match e {
        Expr::Ref { .. } | Expr::Range(..) | Expr::Name { .. } => match cx.ev(&e, f) {
            v @ Value::Ref(_) => Ok(v),
            _ => Err(ErrKind::Ref),
        },
        _ => Err(ErrKind::Ref),
    }
}

fn offset(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 3, 5)?;
    let Value::Ref(base) = cx.ev(&a[0], f) else { return Err(ErrKind::Value) };
    let dr = cx.num(&a[1], f)?.trunc() as i64;
    let dc = cx.num(&a[2], f)?.trunc() as i64;
    let h = match a.get(3) {
        Some(e) if !matches!(e, Expr::Missing) => cx.num(e, f)?.trunc() as i64,
        _ => base.range.rows() as i64,
    };
    let w = match a.get(4) {
        Some(e) if !matches!(e, Expr::Missing) => cx.num(e, f)?.trunc() as i64,
        _ => base.range.cols() as i64,
    };
    if h < 1 || w < 1 {
        return Err(ErrKind::Ref);
    }
    let r1 = base.range.r1 as i64 + dr;
    let c1 = base.range.c1 as i64 + dc;
    let (r2, c2) = (r1 + h - 1, c1 + w - 1);
    if r1 < 0 || c1 < 0 || r2 >= MAX_ROWS as i64 || c2 >= MAX_COLS as i64 {
        return Err(ErrKind::Ref);
    }
    Ok(Value::Ref(Area { sheet: base.sheet, range: CellRange::new(r1 as u32, c1 as u32, r2 as u32, c2 as u32) }))
}

fn row_col(cx: &Ctx, a: &[Expr], f: &Frame, row: bool) -> R {
    need(a, 0, 1)?;
    let Some(e) = a.first().filter(|e| !matches!(e, Expr::Missing)) else {
        return Ok(Value::Num(if row { f.row } else { f.col } as f64 + 1.0));
    };
    match cx.ev(e, f) {
        Value::Ref(area) => {
            let g = area.range;
            if row {
                if g.r1 == g.r2 || g.r2 == MAX_ROWS - 1 && g.r1 == 0 {
                    return Ok(Value::Num(g.r1 as f64 + 1.0));
                }
                Ok(Value::Array(Rc::new(Array::column((g.r1..=g.r2).map(|r| Value::Num(r as f64 + 1.0)).collect()))))
            } else {
                if g.c1 == g.c2 || g.c2 == MAX_COLS - 1 && g.c1 == 0 {
                    return Ok(Value::Num(g.c1 as f64 + 1.0));
                }
                Ok(Value::Array(Rc::new(Array::row((g.c1..=g.c2).map(|c| Value::Num(c as f64 + 1.0)).collect()))))
            }
        }
        Value::Err(e) => Err(e),
        _ => Err(ErrKind::Value),
    }
}

fn dims(cx: &Ctx, a: &[Expr], f: &Frame, rows: bool) -> R {
    need(a, 1, 1)?;
    let (r, c) = match cx.ev(&a[0], f) {
        Value::Ref(area) => (area.range.rows() as f64, area.range.cols() as f64),
        Value::Array(arr) => (arr.rows as f64, arr.cols as f64),
        Value::Err(e) => return Err(e),
        _ => (1.0, 1.0),
    };
    Ok(Value::Num(if rows { r } else { c }))
}

fn choose(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 255)?;
    let iv = cx.ev(&a[0], f);
    if iv.is_multi() {
        let mut vals = vec![iv];
        vals.extend(a[1..].iter().map(|e| cx.ev(e, f)));
        let n = a.len() - 1;
        return Ok(cx.lift(vals, &|v| match to_num(&v[0]) {
            Ok(i) if i >= 1.0 && (i.trunc() as usize) <= n => v[i.trunc() as usize].clone(),
            Ok(_) => Value::Err(ErrKind::Value),
            Err(e) => Value::Err(e),
        }));
    }
    let i = to_num(&cx.scalar(iv, f))?.trunc();
    if i < 1.0 || i as usize >= a.len() {
        return Err(ErrKind::Value);
    }
    Ok(match &a[i as usize] {
        Expr::Missing => Value::Num(0.0),
        e => cx.ev(e, f),
    })
}

fn lookup(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 3)?;
    let needle = needle_of(cx.ev_s(&a[0], f))?;
    let look = cx.array(&cx.ev(&a[1], f));
    let (list, result): (Vec<&Value>, Rc<Array>) = match a.get(2) {
        Some(e) => (vector(&look).ok_or(ErrKind::NA)?, cx.array(&cx.ev(e, f))),
        None => {
            // The array form: search the first row or column, answer from the last.
            if look.cols > look.rows {
                ((0..look.cols).map(|c| look.get(0, c)).collect(), Rc::new(Array::row((0..look.cols).map(|c| look.get(look.rows - 1, c).clone()).collect())))
            } else {
                ((0..look.rows).map(|r| look.get(r, 0)).collect(), Rc::new(Array::column((0..look.rows).map(|r| look.get(r, look.cols - 1).clone()).collect())))
            }
        }
    };
    let i = find(&list, &needle, 1, false).ok_or(ErrKind::NA)?;
    let out = vector(&result).ok_or(ErrKind::NA)?;
    out.get(i).map(|v| (*v).clone()).ok_or(ErrKind::NA)
}

pub fn call(cx: &Ctx, name: &str, a: &[Expr], f: &Frame) -> Option<R> {
    Some(match name {
        "VLOOKUP" => vhlookup(cx, a, f, true),
        "HLOOKUP" => vhlookup(cx, a, f, false),
        "LOOKUP" => lookup(cx, a, f),
        "MATCH" => (|| {
            need(a, 2, 3)?;
            let arr = cx.array(&cx.ev(&a[1], f));
            let list = vector(&arr).ok_or(ErrKind::NA)?;
            let ty = cx.num_or(a, 2, 1.0, f)?;
            let mode = if ty == 0.0 { 0 } else if ty > 0.0 { 1 } else { -1 };
            let nv = cx.ev(&a[0], f);
            Ok(cx.lift(vec![nv], &|n| match needle_of(n[0].clone()) {
                Ok(needle) => match find(&list, &needle, mode, mode == 0) {
                    Some(i) => Value::Num(i as f64 + 1.0),
                    None => Value::Err(ErrKind::NA),
                },
                Err(e) => Value::Err(e),
            }))
        })(),
        "XMATCH" => (|| {
            need(a, 2, 4)?;
            let arr = cx.array(&cx.ev(&a[1], f));
            let list = vector(&arr).ok_or(ErrKind::Value)?;
            let mm = cx.num_or(a, 2, 0.0, f)?.trunc() as i64;
            let sm = match a.get(3) {
                Some(Expr::Missing) | None => 1,
                Some(e) => cx.num(e, f)?.trunc() as i64,
            };
            let nv = cx.ev(&a[0], f);
            Ok(cx.lift(vec![nv], &|n| match needle_of(n[0].clone()).and_then(|needle| xfind(&list, &needle, mm, sm)) {
                Ok(Some(i)) => Value::Num(i as f64 + 1.0),
                Ok(None) => Value::Err(ErrKind::NA),
                Err(e) => Value::Err(e),
            }))
        })(),
        "XLOOKUP" => xlookup(cx, a, f),
        "INDEX" => index(cx, a, f),
        "CHOOSE" => choose(cx, a, f),
        "INDIRECT" => indirect(cx, a, f),
        "OFFSET" => offset(cx, a, f),
        "ROW" => row_col(cx, a, f, true),
        "COLUMN" => row_col(cx, a, f, false),
        "ROWS" => dims(cx, a, f, true),
        "COLUMNS" => dims(cx, a, f, false),
        // The file's spelling of the `@` operator.
        "SINGLE" => need(a, 1, 1).map(|_| match cx.ev(&a[0], f) {
            Value::Ref(area) if !area.range.is_single() => cx.implicit(&area, f),
            Value::Array(arr) => arr.data.first().cloned().unwrap_or(Value::Err(ErrKind::Calc)),
            v => v,
        }),
        _ => return None,
    })
}
