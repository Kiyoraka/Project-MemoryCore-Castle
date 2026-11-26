//! Logic and information: IF, IFS, SWITCH, AND, OR, XOR, NOT, IFERROR, IFNA, TRUE, FALSE, LET,
//! LAMBDA, ISBLANK, ISNUMBER, ISTEXT, ISERROR, ISERR, ISNA, ISLOGICAL, ISNONTEXT, NA, N, T.

use std::rc::Rc;

use super::super::eval::{Ctx, Frame, Lambda, Scope};
use super::super::parse::Expr;
use super::super::value::*;
use super::{lifted, need, R};

fn branch(cx: &Ctx, e: Option<&Expr>, absent: Value, f: &Frame) -> Value {
    match e {
        None => absent,
        Some(Expr::Missing) => Value::Num(0.0),
        Some(e) => cx.ev(e, f),
    }
}

fn if_fn(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 1, 3)?;
    let c = cx.ev(&a[0], f);
    if c.is_multi() {
        let t = branch(cx, a.get(1), Value::Bool(true), f);
        let e = branch(cx, a.get(2), Value::Bool(false), f);
        return Ok(cx.lift(vec![c, t, e], &|v| match to_bool(&v[0]) {
            Ok(true) => v[1].clone(),
            Ok(false) => v[2].clone(),
            Err(e) => Value::Err(e),
        }));
    }
    let c = cx.scalar(c, f);
    if a.len() < 2 {
        return Err(ErrKind::Value);
    }
    if to_bool(&c)? {
        Ok(branch(cx, a.get(1), Value::Bool(true), f))
    } else {
        Ok(branch(cx, a.get(2), Value::Bool(false), f))
    }
}

/// AND / OR / XOR over booleans, numbers, and booleans or numbers inside ranges (text there is
/// skipped; text given directly must read as TRUE/FALSE).
fn logical(cx: &Ctx, a: &[Expr], f: &Frame) -> Result<Vec<bool>, ErrKind> {
    need(a, 1, 255)?;
    let mut out = Vec::new();
    let mut err = None;
    for e in a {
        let v = cx.ev(e, f);
        cx.each(&v, &mut |x, direct| {
            if err.is_some() {
                return;
            }
            match x {
                Value::Bool(b) => out.push(*b),
                Value::Num(n) => out.push(*n != 0.0),
                Value::Err(k) => err = Some(*k),
                Value::Text(_) if direct => match to_bool(x) {
                    Ok(b) => out.push(b),
                    Err(k) => err = Some(k),
                },
                _ => {}
            }
        });
        if let Some(k) = err {
            return Err(k);
        }
    }
    if out.is_empty() {
        return Err(ErrKind::Value);
    }
    Ok(out)
}

fn on_error(cx: &Ctx, a: &[Expr], f: &Frame, only_na: bool) -> R {
    need(a, 2, 2)?;
    let v = cx.ev(&a[0], f);
    let hit = |x: &Value| match x {
        Value::Err(ErrKind::NA) => true,
        Value::Err(_) => !only_na,
        _ => false,
    };
    if v.is_multi() {
        let alt = cx.ev_s(&a[1], f);
        return Ok(cx.lift(vec![v], &|x| if hit(&x[0]) { alt.clone() } else { x[0].clone() }));
    }
    let s = cx.scalar(v.clone(), f);
    if hit(&s) {
        Ok(match &a[1] {
            Expr::Missing => Value::Num(0.0),
            e => cx.ev(e, f),
        })
    } else {
        Ok(v)
    }
}

fn let_fn(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    if a.len() < 3 || a.len() % 2 == 0 {
        return Err(ErrKind::Value);
    }
    let mut frame = f.clone();
    for pair in a[..a.len() - 1].chunks(2) {
        let Expr::Name { sheet: None, name } = &pair[0] else { return Err(ErrKind::Value) };
        let v = cx.ev(&pair[1], &frame);
        frame.vars = Some(Rc::new(Scope { name: name.clone(), value: v, parent: frame.vars.take() }));
    }
    Ok(cx.ev(&a[a.len() - 1], &frame))
}

fn lambda_fn(a: &[Expr], f: &Frame) -> R {
    need(a, 1, 254)?;
    let mut params = Vec::new();
    for p in &a[..a.len() - 1] {
        match p {
            Expr::Name { sheet: None, name } => params.push(name.clone()),
            _ => return Err(ErrKind::Value),
        }
    }
    Ok(Value::Lambda(Rc::new(Lambda { params, body: Rc::new(a[a.len() - 1].clone()), scope: f.vars.clone() })))
}

/// An IS-function: a reference is read cell by cell (ISBLANK is TRUE only for a truly empty cell).
fn is(cx: &Ctx, a: &[Expr], f: &Frame, test: fn(&Value) -> bool) -> R {
    lifted(cx, a, f, 1, 1, &|v| Ok(Value::Bool(test(&v[0]))))
}

pub fn call(cx: &Ctx, name: &str, a: &[Expr], f: &Frame) -> Option<R> {
    Some(match name {
        "IF" => if_fn(cx, a, f),
        "IFS" => (|| {
            if a.len() < 2 || a.len() % 2 == 1 {
                return Err(ErrKind::Value);
            }
            for p in a.chunks(2) {
                if cx.boolean(&p[0], f)? {
                    return Ok(cx.ev(&p[1], f));
                }
            }
            Err(ErrKind::NA)
        })(),
        "SWITCH" => (|| {
            need(a, 3, 254)?;
            let x = cx.ev_s(&a[0], f);
            if let Value::Err(e) = x {
                return Err(e);
            }
            let rest = &a[1..];
            for p in rest.chunks(2) {
                if p.len() == 1 {
                    return Ok(cx.ev(&p[0], f));
                }
                let v = cx.ev_s(&p[0], f);
                let same = std::mem::discriminant(&x) == std::mem::discriminant(&v) || matches!(x, Value::Blank);
                if same && compare(&x, &v).is_ok_and(|o| o.is_eq()) {
                    return Ok(cx.ev(&p[1], f));
                }
            }
            Err(ErrKind::NA)
        })(),
        "AND" => logical(cx, a, f).map(|v| Value::Bool(v.iter().all(|b| *b))),
        "OR" => logical(cx, a, f).map(|v| Value::Bool(v.iter().any(|b| *b))),
        "XOR" => logical(cx, a, f).map(|v| Value::Bool(v.iter().filter(|b| **b).count() % 2 == 1)),
        "NOT" => lifted(cx, a, f, 1, 1, &|v| Ok(Value::Bool(!to_bool(&v[0])?))),
        "TRUE" => need(a, 0, 0).map(|_| Value::Bool(true)),
        "FALSE" => need(a, 0, 0).map(|_| Value::Bool(false)),
        "IFERROR" => on_error(cx, a, f, false),
        "IFNA" => on_error(cx, a, f, true),
        "LET" => let_fn(cx, a, f),
        "LAMBDA" => lambda_fn(a, f),
        "ISBLANK" => is(cx, a, f, |v| matches!(v, Value::Blank)),
        "ISNUMBER" => is(cx, a, f, |v| matches!(v, Value::Num(_))),
        "ISTEXT" => is(cx, a, f, |v| matches!(v, Value::Text(_))),
        "ISNONTEXT" => is(cx, a, f, |v| !matches!(v, Value::Text(_))),
        "ISLOGICAL" => is(cx, a, f, |v| matches!(v, Value::Bool(_))),
        "ISERROR" => is(cx, a, f, |v| matches!(v, Value::Err(_))),
        "ISERR" => is(cx, a, f, |v| matches!(v, Value::Err(e) if *e != ErrKind::NA)),
        "ISNA" => is(cx, a, f, |v| matches!(v, Value::Err(ErrKind::NA))),
        "NA" => need(a, 0, 0).and(Err(ErrKind::NA)),
        "N" => lifted(cx, a, f, 1, 1, &|v| match &v[0] {
            Value::Num(n) => Ok(Value::Num(*n)),
            Value::Bool(b) => Ok(Value::Num(if *b { 1.0 } else { 0.0 })),
            Value::Err(e) => Err(*e),
            _ => Ok(Value::Num(0.0)),
        }),
        "T" => lifted(cx, a, f, 1, 1, &|v| match &v[0] {
            Value::Text(s) => Ok(Value::Text(s.clone())),
            Value::Err(e) => Err(*e),
            _ => Ok(Value::Text(String::new())),
        }),
        _ => return None,
    })
}
