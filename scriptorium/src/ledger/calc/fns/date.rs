//! Dates and times on serial numbers (1900 system with its 29 February 1900, or 1904): TODAY,
//! NOW, DATE, TIME, YEAR, MONTH, DAY, HOUR, MINUTE, SECOND, WEEKDAY, EDATE, EOMONTH,
//! NETWORKDAYS, WORKDAY, DATEDIF, DATEVALUE. Also the clock and the random source (both can be
//! pinned for tests).

use std::cell::Cell;
use std::collections::HashSet;

use super::super::super::numfmt::serial_to_ymd;
use super::super::eval::{Ctx, Frame};
use super::super::parse::Expr;
use super::super::value::*;
use super::{lifted, need, R};

thread_local! {
    static CLOCK: Cell<Option<f64>> = const { Cell::new(None) };
    static SEED: Cell<u64> = const { Cell::new(0) };
}

/// Pins NOW (a 1900-system serial) for this thread; `None` follows the system clock again.
pub fn set_clock(serial: Option<f64>) {
    CLOCK.with(|c| c.set(serial));
}

/// Seeds RAND for this thread (0: seeded from the clock on first use).
pub fn set_seed(seed: u64) {
    SEED.with(|s| s.set(seed));
}

/// The current moment as a serial (UTC: the standard library knows no time zone).
pub fn now_serial(date1904: bool) -> f64 {
    let s = CLOCK.with(|c| c.get()).unwrap_or_else(|| {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        secs / 86400.0 + 25569.0
    });
    if date1904 {
        s - 1462.0
    } else {
        s
    }
}

/// A number in [0, 1) from a xorshift generator.
pub fn rand() -> f64 {
    SEED.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9E37_79B9_7F4A_7C15) | 1;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

const MAX_SERIAL: f64 = 2_958_465.0;

/// A serial date argument: number-like text read as a date, negatives and beyond 9999 `#NUM!`.
fn serial(v: &Value) -> Result<f64, ErrKind> {
    let x = to_num(v)?;
    if !(0.0..MAX_SERIAL + 1.0).contains(&x) {
        return Err(ErrKind::Num);
    }
    Ok(x)
}

fn date_of(y: f64, m: f64, d: f64, date1904: bool) -> R {
    let mut y = y.trunc() as i64;
    if (0..1900).contains(&y) {
        y += 1900;
    }
    if !(0..10000).contains(&y) {
        return Err(ErrKind::Num);
    }
    let months = y * 12 + (m.trunc() as i64 - 1);
    let (yy, mm) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
    if !(1900..=9999).contains(&yy) {
        return Err(ErrKind::Num);
    }
    let s = ymd_serial(yy, mm, d.trunc() as i64, date1904);
    if s < 0 || s as f64 > MAX_SERIAL {
        return Err(ErrKind::Num);
    }
    Ok(Value::Num(s as f64))
}

/// Seconds of the day of a serial (rounded to the nearest second).
fn day_seconds(x: f64) -> i64 {
    ((x - x.floor()) * 86400.0).round() as i64 % 86400
}

fn add_months(s: f64, months: f64, end: bool, date1904: bool) -> R {
    let (y, m, d) = serial_ymd(s, date1904);
    let total = y * 12 + (m as i64 - 1) + months.trunc() as i64;
    let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
    if !(1900..=9999).contains(&ny) {
        return Err(ErrKind::Num);
    }
    let dim = days_in_month(ny, nm, date1904);
    let nd = if end { dim } else { d.min(dim) };
    Ok(Value::Num(ymd_serial(ny, nm, nd as i64, date1904) as f64))
}

/// 0 = Sunday .. 6 = Saturday, the spreadsheet's count.
fn weekday0(s: i64, date1904: bool) -> u32 {
    serial_to_ymd(s, date1904).3
}

fn holidays(cx: &Ctx, a: &[Expr], i: usize, f: &Frame) -> Result<HashSet<i64>, ErrKind> {
    let mut out = HashSet::new();
    if let Some(e) = a.get(i) {
        if matches!(e, Expr::Missing) {
            return Ok(out);
        }
        let v = cx.ev(e, f);
        let mut err = None;
        cx.each(&v, &mut |x, _| match x {
            Value::Err(k) => err = Some(*k),
            Value::Blank => {}
            other => match serial(other) {
                Ok(s) => {
                    out.insert(s.floor() as i64);
                }
                Err(k) => err = Some(k),
            },
        });
        if let Some(k) = err {
            return Err(k);
        }
    }
    Ok(out)
}

fn networkdays(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 3)?;
    let s = serial(&cx.ev_s(&a[0], f))?.floor() as i64;
    let e = serial(&cx.ev_s(&a[1], f))?.floor() as i64;
    let hol = holidays(cx, a, 2, f)?;
    let (lo, hi, sign) = if s <= e { (s, e, 1.0) } else { (e, s, -1.0) };
    let d1904 = cx.date1904;
    let is_work = |d: i64| {
        let w = weekday0(d, d1904);
        w != 0 && w != 6
    };
    let span = hi - lo + 1;
    let weeks = span / 7;
    let mut count = weeks * 5;
    for d in lo + weeks * 7..=hi {
        if is_work(d) {
            count += 1;
        }
    }
    count -= hol.iter().filter(|h| **h >= lo && **h <= hi && is_work(**h)).count() as i64;
    Ok(Value::Num(sign * count as f64))
}

fn workday(cx: &Ctx, a: &[Expr], f: &Frame) -> R {
    need(a, 2, 3)?;
    let mut d = serial(&cx.ev_s(&a[0], f))?.floor() as i64;
    let days = cx.num(&a[1], f)?.trunc() as i64;
    let hol = holidays(cx, a, 2, f)?;
    let step = if days < 0 { -1 } else { 1 };
    let mut left = days.abs();
    while left > 0 {
        d += step;
        if d < 0 || d as f64 > MAX_SERIAL {
            return Err(ErrKind::Num);
        }
        let w = weekday0(d, cx.date1904);
        if w != 0 && w != 6 && !hol.contains(&d) {
            left -= 1;
        }
    }
    Ok(Value::Num(d as f64))
}

fn datedif(s: f64, e: f64, unit: &str, d1904: bool) -> R {
    let (s, e) = (s.floor(), e.floor());
    if s > e {
        return Err(ErrKind::Num);
    }
    let (y1, m1, d1) = serial_ymd(s, d1904);
    let (y2, m2, d2) = serial_ymd(e, d1904);
    let months = (y2 - y1) * 12 + m2 as i64 - m1 as i64 - if d2 < d1 { 1 } else { 0 };
    let v = match unit.to_ascii_uppercase().as_str() {
        "D" => e - s,
        "M" => months as f64,
        "Y" => (months / 12) as f64,
        "YM" => (months % 12) as f64,
        "MD" => {
            if d2 >= d1 {
                (d2 - d1) as f64
            } else {
                let (py, pm) = if m2 == 1 { (y2 - 1, 12) } else { (y2, m2 - 1) };
                (days_in_month(py, pm, d1904) as i64 - d1 as i64 + d2 as i64).max(0) as f64
            }
        }
        "YD" => {
            let mut y = y2;
            let clamp = |y: i64| ymd_serial(y, m1, d1.min(days_in_month(y, m1, d1904)) as i64, d1904) as f64;
            if clamp(y) > e {
                y -= 1;
            }
            e - clamp(y)
        }
        _ => return Err(ErrKind::Num),
    };
    Ok(Value::Num(v))
}

pub fn call(cx: &Ctx, name: &str, a: &[Expr], f: &Frame) -> Option<R> {
    let d1904 = cx.date1904;
    Some(match name {
        "TODAY" => need(a, 0, 0).map(|_| Value::Num(now_serial(d1904).floor())),
        "NOW" => need(a, 0, 0).map(|_| Value::Num(now_serial(d1904))),
        "DATE" => lifted(cx, a, f, 3, 3, &move |v| date_of(to_num(&v[0])?, to_num(&v[1])?, to_num(&v[2])?, d1904)),
        "TIME" => lifted(cx, a, f, 3, 3, &|v| {
            let secs = to_num(&v[0])?.trunc() * 3600.0 + to_num(&v[1])?.trunc() * 60.0 + to_num(&v[2])?.trunc();
            if secs < 0.0 {
                return Err(ErrKind::Num);
            }
            Ok(Value::Num((secs % 86400.0) / 86400.0))
        }),
        "YEAR" => lifted(cx, a, f, 1, 1, &move |v| Ok(Value::Num(serial_ymd(serial(&v[0])?, d1904).0 as f64))),
        "MONTH" => lifted(cx, a, f, 1, 1, &move |v| Ok(Value::Num(serial_ymd(serial(&v[0])?, d1904).1 as f64))),
        "DAY" => lifted(cx, a, f, 1, 1, &move |v| Ok(Value::Num(serial_ymd(serial(&v[0])?, d1904).2 as f64))),
        "HOUR" => lifted(cx, a, f, 1, 1, &|v| Ok(Value::Num((day_seconds(serial(&v[0])?) / 3600) as f64))),
        "MINUTE" => lifted(cx, a, f, 1, 1, &|v| Ok(Value::Num((day_seconds(serial(&v[0])?) % 3600 / 60) as f64))),
        "SECOND" => lifted(cx, a, f, 1, 1, &|v| Ok(Value::Num((day_seconds(serial(&v[0])?) % 60) as f64))),
        "WEEKDAY" => lifted(cx, a, f, 1, 2, &move |v| {
            let s = serial(&v[0])?.floor() as i64;
            let ty = match v.get(1) {
                Some(x) => to_num(x)?.trunc() as i64,
                None => 1,
            };
            let w = weekday0(s, d1904) as i64; // 0 = Sunday
            let r = match ty {
                1 | 17 => w + 1,
                2 | 11 => (w + 6) % 7 + 1,
                3 => (w + 6) % 7,
                12..=16 => (w - (ty - 10) + 7) % 7 + 1,
                _ => return Err(ErrKind::Num),
            };
            Ok(Value::Num(r as f64))
        }),
        "EDATE" => lifted(cx, a, f, 2, 2, &move |v| add_months(serial(&v[0])?, to_num(&v[1])?, false, d1904)),
        "EOMONTH" => lifted(cx, a, f, 2, 2, &move |v| add_months(serial(&v[0])?, to_num(&v[1])?, true, d1904)),
        "NETWORKDAYS" => networkdays(cx, a, f),
        "WORKDAY" => workday(cx, a, f),
        "DATEDIF" => lifted(cx, a, f, 3, 3, &move |v| datedif(serial(&v[0])?, serial(&v[1])?, &to_text(&v[2])?, d1904)),
        "DATEVALUE" => lifted(cx, a, f, 1, 1, &move |v| match &v[0] {
            Value::Text(s) => match parse_datetime_text(s, d1904) {
                Some(x) if x >= 1.0 || s.contains(['/', '-']) => Ok(Value::Num(x.floor())),
                _ => Err(ErrKind::Value),
            },
            Value::Err(e) => Err(*e),
            _ => Err(ErrKind::Value),
        }),
        _ => return None,
    })
}
