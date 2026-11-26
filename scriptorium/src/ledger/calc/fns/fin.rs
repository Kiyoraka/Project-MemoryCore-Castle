//! Finance: PMT, FV, PV, NPER, NPV, IRR, RATE (cash paid out negative, received positive; `type`
//! 1 = payments at the start of each period).

use super::super::eval::{Ctx, Frame};
use super::super::parse::Expr;
use super::super::value::*;
use super::{lifted, need, nums, nums_of, R};

fn opt(v: &[Value], i: usize) -> Result<f64, ErrKind> {
    match v.get(i) {
        Some(x) => to_num(x),
        None => Ok(0.0),
    }
}

fn num(x: f64) -> R {
    if x.is_finite() {
        Ok(Value::Num(x))
    } else {
        Err(ErrKind::Num)
    }
}

pub(crate) fn pmt(r: f64, n: f64, pv: f64, fv: f64, t: f64) -> f64 {
    if r == 0.0 {
        return -(pv + fv) / n;
    }
    let g = (1.0 + r).powf(n);
    -(r * (fv + pv * g)) / ((1.0 + r * t) * (g - 1.0))
}

pub(crate) fn fv(r: f64, n: f64, pmt: f64, pv: f64, t: f64) -> f64 {
    if r == 0.0 {
        return -(pv + pmt * n);
    }
    let g = (1.0 + r).powf(n);
    -(pv * g + pmt * (1.0 + r * t) * (g - 1.0) / r)
}

pub(crate) fn pv(r: f64, n: f64, pmt: f64, fv: f64, t: f64) -> f64 {
    if r == 0.0 {
        return -(fv + pmt * n);
    }
    let g = (1.0 + r).powf(n);
    -(fv + pmt * (1.0 + r * t) * (g - 1.0) / r) / g
}

fn nper(r: f64, pmt: f64, pv: f64, fv: f64, t: f64) -> R {
    if r == 0.0 {
        if pmt == 0.0 {
            return Err(ErrKind::Num);
        }
        return num(-(pv + fv) / pmt);
    }
    let a = pmt * (1.0 + r * t) - fv * r;
    let b = pmt * (1.0 + r * t) + pv * r;
    if b == 0.0 || a / b <= 0.0 || r <= -1.0 {
        return Err(ErrKind::Num);
    }
    num((a / b).ln() / (1.0 + r).ln())
}

/// Newton's method from `guess`, with a bisection fallback over a bracket when Newton strays.
fn solve(g: &dyn Fn(f64) -> f64, guess: f64) -> Option<f64> {
    let mut x = guess;
    for _ in 0..100 {
        let y = g(x);
        if !y.is_finite() {
            break;
        }
        if y.abs() < 1e-12 {
            return Some(x);
        }
        let h = 1e-7 * x.abs().max(1e-7);
        let d = (g(x + h) - g(x - h)) / (2.0 * h);
        if d == 0.0 || !d.is_finite() {
            break;
        }
        let nx = x - y / d;
        if !nx.is_finite() || nx <= -1.0 {
            break;
        }
        if (nx - x).abs() < 1e-12 * nx.abs().max(1.0) {
            return Some(nx);
        }
        x = nx;
    }
    // Bisection: look for a sign change on a grid of rates above -1.
    let grid = [-0.99, -0.9, -0.5, -0.2, -0.1, 0.0, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 100.0];
    for w in grid.windows(2) {
        let (mut lo, mut hi) = (w[0], w[1]);
        let (mut flo, fhi) = (g(lo), g(hi));
        if !flo.is_finite() || !fhi.is_finite() || flo.signum() == fhi.signum() {
            continue;
        }
        for _ in 0..200 {
            let mid = (lo + hi) / 2.0;
            let fm = g(mid);
            if fm.abs() < 1e-12 || (hi - lo) < 1e-15 {
                return Some(mid);
            }
            if fm.signum() == flo.signum() {
                lo = mid;
                flo = fm;
            } else {
                hi = mid;
            }
        }
        return Some((lo + hi) / 2.0);
    }
    None
}

fn irr(vals: &[f64], guess: f64) -> R {
    if !vals.iter().any(|v| *v > 0.0) || !vals.iter().any(|v| *v < 0.0) {
        return Err(ErrKind::Num);
    }
    let g = |r: f64| vals.iter().enumerate().map(|(i, v)| v / (1.0 + r).powi(i as i32)).sum::<f64>();
    solve(&g, guess).map(Value::Num).ok_or(ErrKind::Num)
}

fn rate(n: f64, p: f64, pv0: f64, fv0: f64, t: f64, guess: f64) -> R {
    if n <= 0.0 {
        return Err(ErrKind::Num);
    }
    let g = |r: f64| {
        if r.abs() < 1e-12 {
            pv0 + p * n + fv0
        } else {
            let q = (1.0 + r).powf(n);
            pv0 * q + p * (1.0 + r * t) * (q - 1.0) / r + fv0
        }
    };
    solve(&g, guess).map(Value::Num).ok_or(ErrKind::Num)
}

pub fn call(cx: &Ctx, name: &str, a: &[Expr], f: &Frame) -> Option<R> {
    Some(match name {
        "PMT" => lifted(cx, a, f, 3, 5, &|v| {
            let n = to_num(&v[1])?;
            if n == 0.0 {
                return Err(ErrKind::Num);
            }
            num(pmt(to_num(&v[0])?, n, to_num(&v[2])?, opt(v, 3)?, if opt(v, 4)? != 0.0 { 1.0 } else { 0.0 }))
        }),
        "FV" => lifted(cx, a, f, 3, 5, &|v| num(fv(to_num(&v[0])?, to_num(&v[1])?, to_num(&v[2])?, opt(v, 3)?, if opt(v, 4)? != 0.0 { 1.0 } else { 0.0 }))),
        "PV" => lifted(cx, a, f, 3, 5, &|v| num(pv(to_num(&v[0])?, to_num(&v[1])?, to_num(&v[2])?, opt(v, 3)?, if opt(v, 4)? != 0.0 { 1.0 } else { 0.0 }))),
        "NPER" => lifted(cx, a, f, 3, 5, &|v| nper(to_num(&v[0])?, to_num(&v[1])?, to_num(&v[2])?, opt(v, 3)?, if opt(v, 4)? != 0.0 { 1.0 } else { 0.0 })),
        "NPV" => (|| {
            need(a, 2, 255)?;
            let r = cx.num(&a[0], f)?;
            if r == -1.0 {
                return Err(ErrKind::Div0);
            }
            let vals = nums(cx, &a[1..], f)?;
            num(vals.iter().enumerate().map(|(i, v)| v / (1.0 + r).powi(i as i32 + 1)).sum())
        })(),
        "IRR" => (|| {
            need(a, 1, 2)?;
            let mut vals = Vec::new();
            nums_of(cx, &cx.ev(&a[0], f), &mut vals)?;
            let guess = match a.get(1) {
                Some(e) if !matches!(e, Expr::Missing) => cx.num(e, f)?,
                _ => 0.1,
            };
            irr(&vals, guess)
        })(),
        "RATE" => (|| {
            need(a, 3, 6)?;
            let n = cx.num(&a[0], f)?;
            let p = cx.num(&a[1], f)?;
            let pv0 = cx.num(&a[2], f)?;
            let fv0 = cx.num_or(a, 3, 0.0, f)?;
            let t = if cx.num_or(a, 4, 0.0, f)? != 0.0 { 1.0 } else { 0.0 };
            let guess = match a.get(5) {
                Some(e) if !matches!(e, Expr::Missing) => cx.num(e, f)?,
                _ => 0.1,
            };
            rate(n, p, pv0, fv0, t, guess)
        })(),
        _ => return None,
    })
}
