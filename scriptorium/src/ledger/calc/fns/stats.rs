//! Statistics: AVERAGE, COUNT, COUNTA, COUNTBLANK, MIN, MAX, MEDIAN, LARGE, SMALL, RANK.EQ,
//! STDEV.S/P, VAR.S/P, COUNTIF(S), AVERAGEIF(S), MAXIFS, MINIFS.

use super::super::eval::{Ctx, Frame};
use super::super::parse::Expr;
use super::super::value::*;
use super::math::if_values;
use super::{criteria_hits, need, nums, nums_of, R};

pub(crate) fn average_of(ns: &[f64]) -> R {
    if ns.is_empty() {
        Err(ErrKind::Div0)
    } else {
        Ok(Value::Num(ns.iter().sum::<f64>() / ns.len() as f64))
    }
}

pub(crate) fn var(ns: &[f64], sample: bool) -> R {
    let n = ns.len() as f64;
    if (sample && ns.len() < 2) || ns.is_empty() {
        return Err(ErrKind::Div0);
    }
    let m = ns.iter().sum::<f64>() / n;
    let ss: f64 = ns.iter().map(|x| (x - m) * (x - m)).sum();
    Ok(Value::Num(ss / if sample { n - 1.0 } else { n }))
}

pub(crate) fn stdev(ns: &[f64], sample: bool) -> R {
    match var(ns, sample)? {
        Value::Num(v) => Ok(Value::Num(v.sqrt())),
        other => Ok(other),
    }
}

fn kth(cx: &Ctx, a: &[Expr], f: &Frame, largest: bool) -> R {
    need(a, 2, 2)?;
    let mut ns = Vec::new();
    nums_of(cx, &cx.ev(&a[0], f), &mut ns)?;
    ns.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let kv = cx.ev(&a[1], f);
    Ok(cx.lift(vec![kv], &|k| {
        let k = match to_num(&k[0]) {
            Ok(k) => k.ceil(),
            Err(e) => return Value::Err(e),
        };
        if k < 1.0 || k as usize > ns.len() {
            return Value::Err(ErrKind::Num);
        }
        let i = k as usize - 1;
        Value::Num(if largest { ns[ns.len() - 1 - i] } else { ns[i] })
    }))
}

fn count(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    let mut n = 0usize;
    for e in a {
        let v = cx.ev(e, f);
        cx.each(&v, &mut |x, direct| match x {
            Value::Num(_) => n += 1,
            Value::Bool(_) if direct => n += 1,
            Value::Text(s) if direct && parse_number_text(s, cx.date1904).is_some() => n += 1,
            _ => {}
        });
    }
    Ok(Value::Num(n as f64))
}

fn counta(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    let mut n = 0usize;
    for e in a {
        if matches!(e, Expr::Missing) {
            continue;
        }
        let v = cx.ev(e, f);
        cx.each(&v, &mut |x, direct| {
            if direct || !matches!(x, Value::Blank) {
                n += 1;
            }
        });
    }
    Ok(Value::Num(n as f64))
}

fn countblank(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 1, 1)?;
    match cx.ev(&a[0], f) {
        Value::Ref(area) => {
            let total = area.range.rows() as f64 * area.range.cols() as f64;
            let mut filled = 0usize;
            cx.each(&Value::Ref(area), &mut |x, _| match x {
                Value::Blank => {}
                Value::Text(s) if s.is_empty() => {}
                _ => filled += 1,
            });
            Ok(Value::Num(total - filled as f64))
        }
        Value::Array(arr) => Ok(Value::Num(arr.data.iter().filter(|x| matches!(x, Value::Blank) || matches!(x, Value::Text(s) if s.is_empty())).count() as f64)),
        Value::Err(e) => Err(e),
        _ => Err(ErrKind::Value),
    }
}

fn rank(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 3)?;
    let x = cx.num(&a[0], f)?;
    let mut ns = Vec::new();
    nums_of(cx, &cx.ev(&a[1], f), &mut ns)?;
    let asc = cx.num_or(a, 2, 0.0, f)? != 0.0;
    if !ns.iter().any(|v| num_eq(*v, x)) {
        return Err(ErrKind::NA);
    }
    let better = ns.iter().filter(|v| !num_eq(**v, x) && if asc { **v < x } else { **v > x }).count();
    Ok(Value::Num(better as f64 + 1.0))
}

/// The plain values of `target` at the positions every (range, criterion) pair matches.
fn ifs_values(cx: &Ctx, a: &[Expr], f: &Frame) -> Result<Vec<Value>, ErrKind> {
    if a.len() < 3 || a.len() % 2 == 0 {
        return Err(ErrKind::Value);
    }
    let target = cx.array(&cx.ev(&a[0], f));
    let pairs: Vec<(Value, Value)> = a[1..].chunks(2).map(|p| (cx.ev(&p[0], f), cx.ev_s(&p[1], f))).collect();
    let (mask, _, _) = criteria_hits(cx, &pairs, Some((target.rows, target.cols)))?;
    Ok(mask.iter().enumerate().filter(|(_, h)| **h).map(|(i, _)| target.data[i].clone()).collect())
}

fn numbers(vals: &[Value]) -> Result<Vec<f64>, ErrKind> {
    let mut out = Vec::new();
    for v in vals {
        match v {
            Value::Num(n) => out.push(*n),
            Value::Err(e) => return Err(*e),
            _ => {}
        }
    }
    Ok(out)
}

pub fn call(cx: &Ctx, name: &str, a: &[Expr], f: &Frame) -> Option<R> {
    Some(match name {
        "AVERAGE" => nums(cx, a, f).and_then(|v| average_of(&v)),
        "COUNT" => count(cx, a, f),
        "COUNTA" => counta(cx, a, f),
        "COUNTBLANK" => countblank(cx, a, f),
        "MIN" => nums(cx, a, f).map(|v| Value::Num(if v.is_empty() { 0.0 } else { v.iter().copied().fold(f64::INFINITY, f64::min) })),
        "MAX" => nums(cx, a, f).map(|v| Value::Num(if v.is_empty() { 0.0 } else { v.iter().copied().fold(f64::NEG_INFINITY, f64::max) })),
        "MEDIAN" => nums(cx, a, f).and_then(|mut v| {
            if v.is_empty() {
                return Err(ErrKind::Num);
            }
            v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
            let n = v.len();
            Ok(Value::Num(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }))
        }),
        "LARGE" => kth(cx, a, f, true),
        "SMALL" => kth(cx, a, f, false),
        "RANK.EQ" | "RANK" => rank(cx, a, f),
        "STDEV.S" | "STDEV" => nums(cx, a, f).and_then(|v| stdev(&v, true)),
        "STDEV.P" | "STDEVP" => nums(cx, a, f).and_then(|v| stdev(&v, false)),
        "VAR.S" | "VAR" => nums(cx, a, f).and_then(|v| var(&v, true)),
        "VAR.P" | "VARP" => nums(cx, a, f).and_then(|v| var(&v, false)),
        "COUNTIF" => (|| {
            need(a, 2, 2)?;
            let range = cx.ev(&a[0], f);
            let crit = cx.ev(&a[1], f);
            Ok(cx.lift(vec![crit], &|c| match criteria_hits(cx, &[(range.clone(), c[0].clone())], None) {
                Ok((mask, _, _)) => Value::Num(mask.iter().filter(|h| **h).count() as f64),
                Err(e) => Value::Err(e),
            }))
        })(),
        "COUNTIFS" => (|| {
            if a.len() < 2 || a.len() % 2 == 1 {
                return Err(ErrKind::Value);
            }
            let pairs: Vec<(Value, Value)> = a.chunks(2).map(|p| (cx.ev(&p[0], f), cx.ev_s(&p[1], f))).collect();
            let (mask, _, _) = criteria_hits(cx, &pairs, None)?;
            Ok(Value::Num(mask.iter().filter(|h| **h).count() as f64))
        })(),
        "AVERAGEIF" => (|| {
            need(a, 2, 3)?;
            let range = cx.ev(&a[0], f);
            let crit = cx.ev(&a[1], f);
            let target = match a.get(2) {
                Some(e) if !matches!(e, Expr::Missing) => cx.ev(e, f),
                _ => range.clone(),
            };
            Ok(cx.lift(vec![crit], &|c| match if_values(cx, &range, &c[0], &target).and_then(|v| numbers(&v)).and_then(|v| average_of(&v)) {
                Ok(v) => v,
                Err(e) => Value::Err(e),
            }))
        })(),
        "AVERAGEIFS" => ifs_values(cx, a, f).and_then(|v| numbers(&v)).and_then(|v| average_of(&v)),
        "MAXIFS" => ifs_values(cx, a, f).and_then(|v| numbers(&v)).map(|v| Value::Num(if v.is_empty() { 0.0 } else { v.iter().copied().fold(f64::NEG_INFINITY, f64::max) })),
        "MINIFS" => ifs_values(cx, a, f).and_then(|v| numbers(&v)).map(|v| Value::Num(if v.is_empty() { 0.0 } else { v.iter().copied().fold(f64::INFINITY, f64::min) })),
        _ => return None,
    })
}
