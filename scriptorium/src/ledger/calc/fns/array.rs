//! Dynamic arrays: TRANSPOSE, FILTER, SORT, SORTBY, UNIQUE, SEQUENCE, VSTACK. Their results are
//! arrays; a formula cell holding one shows the top-left value and the whole array comes back in
//! the calculation report as a spill.

use std::cmp::Ordering;
use std::rc::Rc;

use super::super::eval::{Ctx, Frame};
use super::super::parse::Expr;
use super::super::value::*;
use super::{need, R};

fn arr(v: Vec<Value>, rows: usize, cols: usize) -> R {
    if v.is_empty() {
        return Err(ErrKind::Calc);
    }
    Ok(Value::Array(Rc::new(Array::new(rows, cols, v))))
}

fn row_of(a: &Array, r: usize) -> Vec<Value> {
    (0..a.cols).map(|c| a.get(r, c).clone()).collect()
}

fn filter(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 3)?;
    let src = cx.array(&cx.ev(&a[0], f));
    let inc = cx.array(&cx.ev(&a[1], f));
    let keep = |v: &Value| -> Result<bool, ErrKind> { to_bool(v) };
    let mut out = Vec::new();
    let (rows, cols);
    if inc.cols == 1 && inc.rows == src.rows {
        let mut n = 0;
        for r in 0..src.rows {
            if keep(inc.get(r, 0))? {
                out.extend(row_of(&src, r));
                n += 1;
            }
        }
        rows = n;
        cols = src.cols;
    } else if inc.rows == 1 && inc.cols == src.cols {
        let picked: Vec<usize> = (0..src.cols).filter_map(|c| keep(inc.get(0, c)).map(|k| k.then_some(c)).transpose()).collect::<Result<_, _>>()?;
        for r in 0..src.rows {
            for &c in &picked {
                out.push(src.get(r, c).clone());
            }
        }
        rows = if picked.is_empty() { 0 } else { src.rows };
        cols = picked.len();
    } else {
        return Err(ErrKind::Value);
    }
    if out.is_empty() {
        return match a.get(2) {
            Some(e) if !matches!(e, Expr::Missing) => Ok(cx.ev(e, f)),
            _ => Err(ErrKind::Calc),
        };
    }
    arr(out, rows, cols)
}

/// Rows (or columns, `by_col`) of an array as vectors, and the array rebuilt from them.
fn lines(a: &Array, by_col: bool) -> Vec<Vec<Value>> {
    if by_col {
        (0..a.cols).map(|c| (0..a.rows).map(|r| a.get(r, c).clone()).collect()).collect()
    } else {
        (0..a.rows).map(|r| row_of(a, r)).collect()
    }
}

fn rebuild(ls: Vec<Vec<Value>>, by_col: bool) -> R {
    let n = ls.len();
    let w = ls.first().map(|l| l.len()).unwrap_or(0);
    let flat: Vec<Value> = ls.into_iter().flatten().collect();
    if by_col {
        let a = Array::new(n, w, flat).transpose();
        arr(a.data, a.rows, a.cols)
    } else {
        arr(flat, n, w)
    }
}

fn sort(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 1, 4)?;
    let src = cx.array(&cx.ev(&a[0], f));
    let idx = cx.num_def(a, 1, 1.0, f)?.trunc() as usize;
    let order = cx.num_def(a, 2, 1.0, f)?;
    let by_col = cx.bool_or(a, 3, false, f)?;
    if order != 1.0 && order != -1.0 {
        return Err(ErrKind::Value);
    }
    let mut ls = lines(&src, by_col);
    let width = ls.first().map(|l| l.len()).unwrap_or(0);
    if idx < 1 || idx > width {
        return Err(ErrKind::Value);
    }
    ls.sort_by(|x, y| ordered(&x[idx - 1], &y[idx - 1], order < 0.0));
    rebuild(ls, by_col)
}

/// Sort order with blanks last in both directions.
fn ordered(x: &Value, y: &Value, desc: bool) -> Ordering {
    let blank = |v: &Value| matches!(v, Value::Blank);
    match (blank(x), blank(y)) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => {
            let o = sort_cmp(x, y);
            if desc {
                o.reverse()
            } else {
                o
            }
        }
    }
}

fn sortby(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 253)?;
    let src = cx.array(&cx.ev(&a[0], f));
    let mut keys: Vec<(Rc<Array>, bool)> = Vec::new();
    let mut i = 1;
    while i < a.len() {
        let by = cx.array(&cx.ev(&a[i], f));
        let ord = match a.get(i + 1) {
            Some(e) if !matches!(e, Expr::Missing) => cx.num(e, f)?,
            _ => 1.0,
        };
        if ord != 1.0 && ord != -1.0 {
            return Err(ErrKind::Value);
        }
        keys.push((by, ord < 0.0));
        i += 2;
    }
    let by_col = keys[0].0.rows == 1 && keys[0].0.cols > 1 && src.rows != keys[0].0.rows;
    let n = if by_col { src.cols } else { src.rows };
    for (k, _) in &keys {
        let len = if by_col { k.cols } else { k.rows };
        if len != n || (if by_col { k.rows } else { k.cols }) != 1 {
            return Err(ErrKind::Value);
        }
    }
    let ls = lines(&src, by_col);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&x, &y| {
        for (k, desc) in &keys {
            let o = ordered(&k.data[x], &k.data[y], *desc);
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
    rebuild(order.into_iter().map(|i| ls[i].clone()).collect(), by_col)
}

fn same(x: &[Value], y: &[Value]) -> bool {
    x.len() == y.len()
        && x.iter().zip(y).all(|(a, b)| match (a, b) {
            (Value::Blank, Value::Blank) => true,
            (Value::Err(p), Value::Err(q)) => p == q,
            _ => std::mem::discriminant(a) == std::mem::discriminant(b) && compare(a, b).is_ok_and(|o| o.is_eq()),
        })
}

fn unique(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 1, 3)?;
    let src = cx.array(&cx.ev(&a[0], f));
    let by_col = cx.bool_or(a, 1, false, f)?;
    let once = cx.bool_or(a, 2, false, f)?;
    let ls = lines(&src, by_col);
    let mut groups: Vec<(Vec<Value>, usize)> = Vec::new();
    for l in ls {
        match groups.iter_mut().find(|(g, _)| same(g, &l)) {
            Some(g) => g.1 += 1,
            None => groups.push((l, 1)),
        }
    }
    let kept: Vec<Vec<Value>> = groups.into_iter().filter(|(_, n)| !once || *n == 1).map(|(g, _)| g).collect();
    if kept.is_empty() {
        return Err(ErrKind::Calc);
    }
    rebuild(kept, by_col)
}

fn sequence(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 1, 4)?;
    let rows = cx.num_def(a, 0, 1.0, f)?.trunc();
    let cols = match a.get(1) {
        Some(Expr::Missing) | None => 1.0,
        Some(e) => cx.num(e, f)?.trunc(),
    };
    let start = match a.get(2) {
        Some(Expr::Missing) | None => 1.0,
        Some(e) => cx.num(e, f)?,
    };
    let step = match a.get(3) {
        Some(Expr::Missing) | None => 1.0,
        Some(e) => cx.num(e, f)?,
    };
    if rows < 1.0 || cols < 1.0 {
        return Err(ErrKind::Calc);
    }
    if rows * cols > 1_048_576.0 * 16.0 {
        return Err(ErrKind::Num);
    }
    let (r, c) = (rows as usize, cols as usize);
    arr((0..r * c).map(|i| Value::Num(start + step * i as f64)).collect(), r, c)
}

fn vstack(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 1, 254)?;
    let parts: Vec<Rc<Array>> = a.iter().map(|e| cx.array(&cx.ev(e, f))).collect();
    let w = parts.iter().map(|p| p.cols).max().unwrap_or(1);
    let mut data = Vec::new();
    let mut rows = 0;
    for p in &parts {
        for r in 0..p.rows {
            for c in 0..w {
                data.push(if c < p.cols { p.get(r, c).clone() } else { Value::Err(ErrKind::NA) });
            }
            rows += 1;
        }
    }
    arr(data, rows, w)
}

pub fn call(cx: &Ctx, name: &str, a: &[Expr], f: &Frame) -> Option<R> {
    Some(match name {
        "TRANSPOSE" => need(a, 1, 1).and_then(|_| {
            let t = cx.array(&cx.ev(&a[0], f)).transpose();
            arr(t.data, t.rows, t.cols)
        }),
        "FILTER" => filter(cx, a, f),
        "SORT" => sort(cx, a, f),
        "SORTBY" => sortby(cx, a, f),
        "UNIQUE" => unique(cx, a, f),
        "SEQUENCE" => sequence(cx, a, f),
        "VSTACK" => vstack(cx, a, f),
        _ => return None,
    })
}
