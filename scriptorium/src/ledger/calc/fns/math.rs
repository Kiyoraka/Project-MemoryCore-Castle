//! Math: SUM, PRODUCT, the ROUND family, INT, TRUNC, ABS, MOD, POWER, SQRT, CEILING.MATH,
//! SUMPRODUCT, SUBTOTAL, SUMIF(S), RAND, RANDBETWEEN.

use super::super::eval::{power, Ctx, Frame};
use super::super::parse::Expr;
use super::super::value::*;
use super::{criteria_hits, lifted, need, nums, stats, R};

fn n1(v: &[Value]) -> Result<f64, ErrKind> {
    to_num(&v[0])
}

fn arg(v: &[Value], i: usize, d: f64) -> Result<f64, ErrKind> {
    match v.get(i) {
        Some(x) => to_num(x),
        None => Ok(d),
    }
}

fn num(x: f64) -> R {
    if x.is_finite() {
        Ok(Value::Num(x))
    } else {
        Err(ErrKind::Num)
    }
}

pub fn call(cx: &Ctx, name: &str, a: &[Expr], f: &Frame) -> Option<R> {
    Some(match name {
        "SUM" => nums(cx, a, f).and_then(|v| num(v.iter().sum())),
        "PRODUCT" => nums(cx, a, f).and_then(|v| num(if v.is_empty() { 0.0 } else { v.iter().product() })),
        "ROUND" => lifted(cx, a, f, 2, 2, &|v| num(round_half_away(n1(v)?, to_num(&v[1])?.trunc() as i32))),
        "ROUNDUP" => lifted(cx, a, f, 2, 2, &|v| num(round_with(n1(v)?, to_num(&v[1])?.trunc() as i32, f64::ceil))),
        "ROUNDDOWN" => lifted(cx, a, f, 2, 2, &|v| num(round_with(n1(v)?, to_num(&v[1])?.trunc() as i32, f64::trunc))),
        "TRUNC" => lifted(cx, a, f, 1, 2, &|v| num(round_with(n1(v)?, arg(v, 1, 0.0)?.trunc() as i32, f64::trunc))),
        "INT" => lifted(cx, a, f, 1, 1, &|v| num(n1(v)?.floor())),
        "ABS" => lifted(cx, a, f, 1, 1, &|v| num(n1(v)?.abs())),
        "SQRT" => lifted(cx, a, f, 1, 1, &|v| {
            let x = n1(v)?;
            if x < 0.0 {
                Err(ErrKind::Num)
            } else {
                num(x.sqrt())
            }
        }),
        "POWER" => lifted(cx, a, f, 2, 2, &|v| num(power(n1(v)?, to_num(&v[1])?)?)),
        "MOD" => lifted(cx, a, f, 2, 2, &|v| {
            let (n, d) = (n1(v)?, to_num(&v[1])?);
            if d == 0.0 {
                return Err(ErrKind::Div0);
            }
            let q = round_sig(n / d, 15).floor();
            num(n - d * q)
        }),
        "CEILING.MATH" => lifted(cx, a, f, 1, 3, &|v| {
            let n = n1(v)?;
            let sig = arg(v, 1, 1.0)?.abs();
            let mode = arg(v, 2, 0.0)?;
            if sig == 0.0 {
                return Ok(Value::Num(0.0));
            }
            let q = round_sig(n.abs() / sig, 15);
            let r = if n >= 0.0 {
                q.ceil() * sig
            } else if mode != 0.0 {
                -(q.ceil() * sig)
            } else {
                -(q.floor() * sig)
            };
            num(r)
        }),
        "SUMPRODUCT" => sumproduct(cx, a, f),
        "SUBTOTAL" => subtotal(cx, a, f),
        "SUMIF" => sumif(cx, a, f),
        "SUMIFS" => sumifs(cx, a, f),
        "RAND" => need(a, 0, 0).map(|_| Value::Num(super::date::rand())),
        "RANDBETWEEN" => (|| {
            need(a, 2, 2)?;
            let lo = cx.num(&a[0], f)?.ceil();
            let hi = cx.num(&a[1], f)?.floor();
            if lo > hi {
                return Err(ErrKind::Num);
            }
            num(lo + (super::date::rand() * (hi - lo + 1.0)).floor())
        })(),
        _ => return None,
    })
}

fn sumproduct(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 1, 255)?;
    let arrays: Vec<_> = a.iter().map(|e| cx.array(&cx.ev(e, f))).collect();
    let (rows, cols) = (arrays[0].rows, arrays[0].cols);
    if arrays.iter().any(|x| x.rows != rows || x.cols != cols) {
        return Err(ErrKind::Value);
    }
    let mut total = 0.0;
    for i in 0..rows * cols {
        let mut p = 1.0;
        for arr in &arrays {
            match &arr.data[i] {
                Value::Num(n) => p *= n,
                Value::Err(e) => return Err(*e),
                _ => p = 0.0,
            }
        }
        total += p;
    }
    num(total)
}

fn subtotal(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 255)?;
    let code = cx.num(&a[0], f)?.trunc() as i64;
    let skip_hidden = code > 100;
    let k = if skip_hidden { code - 100 } else { code };
    if !(1..=11).contains(&k) {
        return Err(ErrKind::Value);
    }
    let mut vals: Vec<Value> = Vec::new();
    for e in &a[1..] {
        match cx.ev(e, f) {
            Value::Ref(area) => {
                let r = area.range;
                let Some(sh) = cx.sheets.get(area.sheet) else { continue };
                for (&row, rr) in sh.rows.range(r.r1..=r.r2) {
                    if skip_hidden && rr.hidden {
                        continue;
                    }
                    for (&col, cell) in rr.cells.range(r.c1..=r.c2) {
                        if let Some(fx) = &cell.formula {
                            let u = fx.to_ascii_uppercase();
                            if u.contains("SUBTOTAL(") || u.contains("AGGREGATE(") {
                                continue;
                            }
                        }
                        vals.push(cx.value_at((area.sheet, row, col)));
                    }
                }
            }
            Value::Err(e) => return Err(e),
            _ => return Err(ErrKind::Value),
        }
    }
    let mut ns = Vec::new();
    for v in &vals {
        match v {
            Value::Num(n) => ns.push(*n),
            Value::Err(e) if k != 2 && k != 3 => return Err(*e),
            _ => {}
        }
    }
    match k {
        1 => stats::average_of(&ns),
        2 => Ok(Value::Num(ns.len() as f64)),
        3 => Ok(Value::Num(vals.iter().filter(|v| !matches!(v, Value::Blank)).count() as f64)),
        4 => Ok(Value::Num(if ns.is_empty() { 0.0 } else { ns.iter().copied().fold(f64::NEG_INFINITY, f64::max) })),
        5 => Ok(Value::Num(if ns.is_empty() { 0.0 } else { ns.iter().copied().fold(f64::INFINITY, f64::min) })),
        6 => num(if ns.is_empty() { 0.0 } else { ns.iter().product() }),
        7 => stats::stdev(&ns, true),
        8 => stats::stdev(&ns, false),
        9 => num(ns.iter().sum()),
        10 => stats::var(&ns, true),
        _ => stats::var(&ns, false),
    }
}

/// The range a SUMIF-style sum range covers: the same shape as the criteria range, from its own
/// top-left cell.
pub(crate) fn resized(sum: Value, rows: usize, cols: usize) -> Value {
    use super::super::super::cellref::{CellRange, MAX_COLS, MAX_ROWS};
    match sum {
        Value::Ref(mut area) => {
            let (r1, c1) = (area.range.r1, area.range.c1);
            let r2 = (r1 as usize + rows.max(1) - 1).min(MAX_ROWS as usize - 1) as u32;
            let c2 = (c1 as usize + cols.max(1) - 1).min(MAX_COLS as usize - 1) as u32;
            area.range = CellRange::new(r1, c1, r2, c2);
            Value::Ref(area)
        }
        other => other,
    }
}

fn sumif(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 3)?;
    let range = cx.ev(&a[0], f);
    let crit = cx.ev(&a[1], f);
    let sum = match a.get(2) {
        Some(e) if !matches!(e, Expr::Missing) => cx.ev(e, f),
        _ => range.clone(),
    };
    Ok(cx.lift(vec![crit], &|c| sumif_one(cx, &range, &c[0], &sum).unwrap_or_else(Value::Err)))
}

/// The cells of `sum` (resized to `range`) where `range` meets `crit`, as plain values.
pub(crate) fn if_values(cx: &Ctx, range: &Value, crit: &Value, sum: &Value) -> Result<Vec<Value>, ErrKind> {
    let (mask, rows, cols) = criteria_hits(cx, &[(range.clone(), crit.clone())], None)?;
    let s = cx.array(&resized(sum.clone(), rows, cols));
    let mut out = Vec::new();
    for (i, hit) in mask.iter().enumerate() {
        if *hit {
            let (r, cc) = (i / cols.max(1), i % cols.max(1));
            if r < s.rows && cc < s.cols {
                out.push(s.get(r, cc).clone());
            }
        }
    }
    Ok(out)
}

fn sumif_one(cx: &Ctx, range: &Value, crit: &Value, sum: &Value) -> R {
    let mut total = 0.0;
    for v in if_values(cx, range, crit, sum)? {
        match v {
            Value::Num(n) => total += n,
            Value::Err(e) => return Err(e),
            _ => {}
        }
    }
    num(total)
}

fn sumifs(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    if a.len() < 3 || a.len() % 2 == 0 {
        return Err(ErrKind::Value);
    }
    let sum = cx.array(&cx.ev(&a[0], f));
    let pairs: Vec<(Value, Value)> = a[1..].chunks(2).map(|p| (cx.ev(&p[0], f), cx.ev_s(&p[1], f))).collect();
    let (mask, _, _) = criteria_hits(cx, &pairs, Some((sum.rows, sum.cols)))?;
    let mut total = 0.0;
    for (i, hit) in mask.iter().enumerate() {
        if *hit {
            match &sum.data[i] {
                Value::Num(n) => total += n,
                Value::Err(e) => return Err(*e),
                _ => {}
            }
        }
    }
    num(total)
}
