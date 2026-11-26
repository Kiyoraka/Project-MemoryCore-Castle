//! The evaluator: a formula tree worked out against the workbook. Results of this pass are kept
//! in an overlay (`results`) and written into the cells by the caller at the end, so a cell read
//! during the pass sees this pass's value; a dirty formula cell read before its turn (through
//! INDIRECT or OFFSET) is worked out on demand, and one already in progress (a circle) answers its
//! cached value.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::super::cellref::{CellRange, MAX_COLS, MAX_ROWS};
use super::super::model::{CellValue, DefinedName, Sheet};
use super::parse::{parse_formula, BinOp, Expr};
use super::value::*;

/// (sheet index, row, col), 0-based.
pub type Key = (usize, u32, u32);

/// A LET or LAMBDA variable binding (a chain; the innermost first).
#[derive(Debug)]
pub struct Scope {
    pub name: String,
    pub value: Value,
    pub parent: Option<Rc<Scope>>,
}

/// A LAMBDA value.
#[derive(Debug)]
pub struct Lambda {
    pub params: Vec<String>,
    pub body: Rc<Expr>,
    pub scope: Option<Rc<Scope>>,
}

/// Where a formula is being evaluated, and its variables.
#[derive(Debug, Clone)]
pub struct Frame {
    pub sheet: usize,
    pub row: u32,
    pub col: u32,
    pub vars: Option<Rc<Scope>>,
}

impl Frame {
    pub fn at(sheet: usize, row: u32, col: u32) -> Frame {
        Frame { sheet, row, col, vars: None }
    }

    fn var(&self, name: &str) -> Option<Value> {
        let mut s = self.vars.as_ref();
        while let Some(sc) = s {
            if sc.name == name {
                return Some(sc.value.clone());
            }
            s = sc.parent.as_ref();
        }
        None
    }
}

/// One calculation pass over a workbook.
pub struct Ctx<'a> {
    pub sheets: &'a [Sheet],
    pub names: &'a [DefinedName],
    pub date1904: bool,
    sheet_ix: HashMap<String, usize>,
    /// Per sheet: the last used row and column (whole-column references stop there).
    extent: Vec<Option<(u32, u32)>>,
    pub results: RefCell<HashMap<Key, CellValue>>,
    pub spills: RefCell<HashMap<Key, Rc<Array>>>,
    /// The formula cells this pass works out (cells in a circle are left out).
    pub pending: HashMap<Key, Rc<Expr>>,
    in_progress: RefCell<HashSet<Key>>,
    unsupported: Cell<bool>,
    depth: Cell<u32>,
    name_cache: RefCell<HashMap<String, Option<Rc<Expr>>>>,
}

const MAX_DEPTH: u32 = 96;

impl<'a> Ctx<'a> {
    pub fn new(sheets: &'a [Sheet], names: &'a [DefinedName], date1904: bool, pending: HashMap<Key, Rc<Expr>>) -> Ctx<'a> {
        set_coercion_date1904(date1904);
        let sheet_ix = sheets.iter().enumerate().map(|(i, s)| (s.name.to_lowercase(), i)).collect();
        let extent = sheets
            .iter()
            .map(|s| {
                let last_row = *s.rows.keys().next_back()?;
                let last_col = s.rows.values().filter_map(|r| r.cells.keys().next_back().copied()).max()?;
                Some((last_row, last_col))
            })
            .collect();
        Ctx {
            sheets,
            names,
            date1904,
            sheet_ix,
            extent,
            results: RefCell::new(HashMap::new()),
            spills: RefCell::new(HashMap::new()),
            pending,
            in_progress: RefCell::new(HashSet::new()),
            unsupported: Cell::new(false),
            depth: Cell::new(0),
            name_cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheet_ix.get(&name.to_lowercase()).copied()
    }

    fn sheet_of(&self, sheet: &Option<String>, f: &Frame) -> Option<usize> {
        match sheet {
            None => Some(f.sheet),
            Some(n) => self.sheet_index(n),
        }
    }

    /// Marks this cell's formula as using something we do not have (an unknown function or name).
    pub fn mark_unsupported(&self) {
        self.unsupported.set(true);
    }

    /// The value of a cell as this pass sees it.
    pub fn value_at(&self, k: Key) -> Value {
        if let Some(v) = self.results.borrow().get(&k) {
            return cell_to_value(v);
        }
        if let Some(e) = self.pending.get(&k).cloned() {
            if !self.in_progress.borrow().contains(&k) {
                self.compute(k, &e);
                if let Some(v) = self.results.borrow().get(&k) {
                    return cell_to_value(v);
                }
            }
        }
        match self.sheets.get(k.0).and_then(|s| s.cell(k.1, k.2)) {
            Some(c) => cell_to_value(&c.value),
            None => Value::Blank,
        }
    }

    /// Works out one formula cell into `results` (and `spills`).
    pub fn compute(&self, k: Key, e: &Expr) {
        self.in_progress.borrow_mut().insert(k);
        let saved = self.unsupported.replace(false);
        let f = Frame::at(k.0, k.1, k.2);
        let v = self.eval_top(e, &f);
        let unsup = self.unsupported.replace(saved);
        let old = self.sheets.get(k.0).and_then(|s| s.cell(k.1, k.2)).map(|c| c.value.clone()).unwrap_or(CellValue::Empty);
        let cv = if unsup && !old.is_empty() {
            old
        } else {
            let (cv, spill) = self.finish(v, &f);
            if let Some(a) = spill {
                self.spills.borrow_mut().insert(k, a);
            }
            cv
        };
        self.results.borrow_mut().insert(k, cv);
        self.in_progress.borrow_mut().remove(&k);
    }

    /// A formula's final value as a cell value (plus the whole array when it spills).
    pub fn finish(&self, v: Value, f: &Frame) -> (CellValue, Option<Rc<Array>>) {
        match v {
            Value::Ref(a) if a.range.is_single() => (value_to_cell(&self.value_at((a.sheet, a.range.r1, a.range.c1))), None),
            Value::Ref(a) => (value_to_cell(&self.implicit(&a, f)), None),
            Value::Array(arr) => {
                let first = value_to_cell(arr.data.first().unwrap_or(&Value::Err(ErrKind::Calc)));
                if arr.rows * arr.cols > 1 {
                    (first, Some(arr))
                } else {
                    (first, None)
                }
            }
            other => (value_to_cell(&other), None),
        }
    }

    /// Evaluates a formula's root; a final `+`/`-` whose result is a rounding crumb of its
    /// operands (`=0.3-0.2-0.1`) answers 0, as the spreadsheet does.
    pub fn eval_top(&self, e: &Expr, f: &Frame) -> Value {
        if let Expr::Bin(op @ (BinOp::Add | BinOp::Sub), l, r) = e {
            let a = self.eval(l, f);
            let b = self.eval(r, f);
            let (sa, sb) = (self.scalar(a.clone(), f), self.scalar(b.clone(), f));
            let v = self.binary(*op, a, b);
            if let (Value::Num(x), Ok(x1), Ok(x2)) = (&v, to_num(&sa), to_num(&sb)) {
                if !matches!(sa, Value::Array(_)) && !matches!(sb, Value::Array(_)) && *x != 0.0 && x.abs() < x1.abs().max(x2.abs()) * 2f64.powi(-49) {
                    return Value::Num(0.0);
                }
            }
            return v;
        }
        self.eval(e, f)
    }

    pub fn eval(&self, e: &Expr, f: &Frame) -> Value {
        match e {
            Expr::Num(n) => Value::Num(*n),
            Expr::Str(s) => Value::Text(s.clone()),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Err(k) => Value::Err(*k),
            Expr::Missing => Value::Blank,
            Expr::Bad(_) => Value::Err(ErrKind::Ref),
            Expr::Ref { sheet, range } => match self.sheet_of(sheet, f) {
                Some(s) => Value::Ref(Area { sheet: s, range: *range }),
                None => Value::Err(ErrKind::Ref),
            },
            Expr::Name { sheet, name } => self.eval_name(sheet.as_deref(), name, f),
            Expr::Array(rows) => {
                let r = rows.len();
                let c = rows.first().map(|x| x.len()).unwrap_or(0);
                let data = rows.iter().flatten().map(|x| self.eval(x, f)).collect();
                Value::Array(Rc::new(Array::new(r, c, data)))
            }
            Expr::Neg(x) => {
                let v = self.eval(x, f);
                self.lift(vec![v], &|a| match to_num(&a[0]) {
                    Ok(n) => Value::Num(-n),
                    Err(e) => Value::Err(e),
                })
            }
            Expr::Plus(x) => self.eval(x, f),
            Expr::Percent(x) => {
                let v = self.eval(x, f);
                self.lift(vec![v], &|a| match to_num(&a[0]) {
                    Ok(n) => Value::Num(n / 100.0),
                    Err(e) => Value::Err(e),
                })
            }
            Expr::Bin(op, l, r) => {
                let a = self.eval(l, f);
                let b = self.eval(r, f);
                self.binary(*op, a, b)
            }
            Expr::Range(l, r) => match (self.eval(l, f), self.eval(r, f)) {
                (Value::Ref(a), Value::Ref(b)) if a.sheet == b.sheet => Value::Ref(Area { sheet: a.sheet, range: a.range.union(&b.range) }),
                (Value::Err(e), _) | (_, Value::Err(e)) => Value::Err(e),
                _ => Value::Err(ErrKind::Value),
            },
            Expr::Isect(l, r) => match (self.eval(l, f), self.eval(r, f)) {
                (Value::Ref(a), Value::Ref(b)) if a.sheet == b.sheet && a.range.intersects(&b.range) => {
                    let (x, y) = (a.range, b.range);
                    Value::Ref(Area { sheet: a.sheet, range: CellRange::new(x.r1.max(y.r1), x.c1.max(y.c1), x.r2.min(y.r2), x.c2.min(y.c2)) })
                }
                (Value::Err(e), _) | (_, Value::Err(e)) => Value::Err(e),
                (Value::Ref(_), Value::Ref(_)) => Value::Err(ErrKind::Null),
                _ => Value::Err(ErrKind::Value),
            },
            Expr::At(x) => match self.eval(x, f) {
                Value::Ref(a) if !a.range.is_single() => self.implicit(&a, f),
                Value::Array(arr) => arr.data.first().cloned().unwrap_or(Value::Err(ErrKind::Calc)),
                v => v,
            },
            Expr::Call(name, args) => self.call(name, args, f),
            Expr::CallExpr(callee, args) => {
                let c = self.eval(callee, f);
                let vals: Vec<Value> = args.iter().map(|a| self.eval(a, f)).collect();
                self.call_value(&c, vals, f)
            }
        }
    }

    fn find_name(&self, sheet: Option<usize>, name: &str, f: &Frame) -> Option<&'a DefinedName> {
        let want = |d: &&DefinedName| d.name.eq_ignore_ascii_case(name);
        match sheet {
            Some(s) => self.names.iter().filter(want).find(|d| d.local_sheet == Some(s)).or_else(|| self.names.iter().filter(want).find(|d| d.local_sheet.is_none())),
            None => self.names.iter().filter(want).find(|d| d.local_sheet == Some(f.sheet)).or_else(|| self.names.iter().filter(want).find(|d| d.local_sheet.is_none())),
        }
    }

    fn name_expr(&self, d: &DefinedName) -> Option<Rc<Expr>> {
        let key = format!("{:?}|{}", d.local_sheet, d.name.to_ascii_uppercase());
        if let Some(e) = self.name_cache.borrow().get(&key) {
            return e.clone();
        }
        let e = parse_formula(&d.value).ok().map(Rc::new);
        self.name_cache.borrow_mut().insert(key, e.clone());
        e
    }

    fn eval_name(&self, sheet: Option<&str>, name: &str, f: &Frame) -> Value {
        if sheet.is_none() {
            if let Some(v) = f.var(name) {
                return v;
            }
        }
        let sheet_ix = match sheet {
            Some(s) => match self.sheet_index(s) {
                Some(i) => Some(i),
                None => return Value::Err(ErrKind::Ref),
            },
            None => None,
        };
        let Some(d) = self.find_name(sheet_ix, name, f) else {
            self.mark_unsupported();
            return Value::Err(ErrKind::Name);
        };
        let Some(e) = self.name_expr(d) else {
            self.mark_unsupported();
            return Value::Err(ErrKind::Name);
        };
        if self.depth.get() > MAX_DEPTH {
            return Value::Err(ErrKind::Name);
        }
        self.depth.set(self.depth.get() + 1);
        let v = self.eval(&e, &Frame { vars: None, ..f.clone() });
        self.depth.set(self.depth.get() - 1);
        v
    }

    fn call(&self, name: &str, args: &[Expr], f: &Frame) -> Value {
        if let Some(v @ Value::Lambda(_)) = f.var(name) {
            let vals: Vec<Value> = args.iter().map(|a| self.eval(a, f)).collect();
            return self.call_value(&v, vals, f);
        }
        if let Some(v) = super::fns::call(self, name, args, f) {
            return v;
        }
        // A defined name holding a LAMBDA.
        if let Some(d) = self.find_name(None, name, f) {
            if let Some(e) = self.name_expr(d) {
                let callee = self.eval(&e, &Frame { vars: None, ..f.clone() });
                if matches!(callee, Value::Lambda(_)) {
                    let vals: Vec<Value> = args.iter().map(|a| self.eval(a, f)).collect();
                    return self.call_value(&callee, vals, f);
                }
            }
        }
        self.mark_unsupported();
        Value::Err(ErrKind::Name)
    }

    /// Calls a LAMBDA value with argument values.
    pub fn call_value(&self, callee: &Value, args: Vec<Value>, f: &Frame) -> Value {
        let Value::Lambda(l) = callee else {
            return match callee {
                Value::Err(e) => Value::Err(*e),
                _ => Value::Err(ErrKind::Value),
            };
        };
        if args.len() > l.params.len() {
            return Value::Err(ErrKind::Value);
        }
        if self.depth.get() > MAX_DEPTH {
            return Value::Err(ErrKind::Num);
        }
        let mut scope = l.scope.clone();
        let mut it = args.into_iter();
        for p in &l.params {
            let v = it.next().unwrap_or(Value::Blank);
            scope = Some(Rc::new(Scope { name: p.clone(), value: v, parent: scope }));
        }
        self.depth.set(self.depth.get() + 1);
        let v = self.eval(&l.body, &Frame { vars: scope, ..f.clone() });
        self.depth.set(self.depth.get() - 1);
        v
    }

    /// Implicit intersection: the cell of a one-column range on the formula's row, or of a one-row
    /// range in its column; `#VALUE!` otherwise.
    pub fn implicit(&self, a: &Area, f: &Frame) -> Value {
        let r = a.range;
        if r.is_single() {
            return self.value_at((a.sheet, r.r1, r.c1));
        }
        if r.c1 == r.c2 && f.row >= r.r1 && f.row <= r.r2 {
            return self.value_at((a.sheet, f.row, r.c1));
        }
        if r.r1 == r.r2 && f.col >= r.c1 && f.col <= r.c2 {
            return self.value_at((a.sheet, r.r1, f.col));
        }
        Value::Err(ErrKind::Value)
    }

    /// A value made scalar: a one-cell reference read, a larger one intersected, an array's first
    /// element.
    pub fn scalar(&self, v: Value, f: &Frame) -> Value {
        match v {
            Value::Ref(a) => self.implicit(&a, f),
            Value::Array(arr) => arr.data.first().cloned().unwrap_or(Value::Err(ErrKind::Calc)),
            other => other,
        }
    }

    /// A reference's rectangle cut back to the sheet's used part when it spans whole columns or
    /// rows (never below one row/column).
    pub fn clip(&self, a: &Area) -> CellRange {
        let mut r = a.range;
        if let Some(Some((mr, mc))) = self.extent.get(a.sheet) {
            if r.r2 == MAX_ROWS - 1 {
                r.r2 = (*mr).max(r.r1);
            }
            if r.c2 == MAX_COLS - 1 {
                r.c2 = (*mc).max(r.c1);
            }
        } else {
            if r.r2 == MAX_ROWS - 1 {
                r.r2 = r.r1;
            }
            if r.c2 == MAX_COLS - 1 {
                r.c2 = r.c1;
            }
        }
        r
    }

    /// Any value as a rectangle of plain values.
    pub fn array(&self, v: &Value) -> Rc<Array> {
        match v {
            Value::Array(a) => a.clone(),
            Value::Ref(a) => {
                let r = self.clip(a);
                let mut data = Vec::with_capacity((r.rows() * r.cols()) as usize);
                for row in r.r1..=r.r2 {
                    for col in r.c1..=r.c2 {
                        data.push(self.value_at((a.sheet, row, col)));
                    }
                }
                Rc::new(Array::new(r.rows() as usize, r.cols() as usize, data))
            }
            Value::Lambda(_) => Rc::new(Array::scalar(Value::Err(ErrKind::Calc))),
            other => Rc::new(Array::scalar(other.clone())),
        }
    }

    /// Every plain value inside: a reference's stored cells (`false`: from a range), an array's
    /// elements (`false`), or the value itself (`true`: given directly).
    pub fn each(&self, v: &Value, g: &mut dyn FnMut(&Value, bool)) {
        match v {
            Value::Ref(a) => {
                let r = a.range;
                let Some(sh) = self.sheets.get(a.sheet) else { return };
                for (&row, rr) in sh.rows.range(r.r1..=r.r2) {
                    for (&col, _) in rr.cells.range(r.c1..=r.c2) {
                        let x = self.value_at((a.sheet, row, col));
                        g(&x, false);
                    }
                }
            }
            Value::Array(arr) => {
                for x in &arr.data {
                    g(x, false);
                }
            }
            Value::Lambda(_) => g(&Value::Err(ErrKind::Calc), true),
            other => g(other, true),
        }
    }

    /// Applies a scalar function over its arguments; when any is an array or a range of more
    /// than one cell the function runs element by element (broadcasting single rows/columns) and
    /// answers an array.
    pub fn lift(&self, vals: Vec<Value>, func: &dyn Fn(&[Value]) -> Value) -> Value {
        let vals: Vec<Value> = vals
            .into_iter()
            .map(|v| match v {
                Value::Ref(a) if a.range.is_single() => self.value_at((a.sheet, a.range.r1, a.range.c1)),
                Value::Ref(a) => Value::Array(self.array(&Value::Ref(a))),
                other => other,
            })
            .collect();
        let (mut rows, mut cols, mut any) = (1usize, 1usize, false);
        for v in &vals {
            if let Value::Array(a) = v {
                any = true;
                rows = rows.max(a.rows);
                cols = cols.max(a.cols);
            }
        }
        if !any {
            return func(&vals);
        }
        let mut data = Vec::with_capacity(rows * cols);
        let mut buf = vec![Value::Blank; vals.len()];
        for i in 0..rows {
            for j in 0..cols {
                for (k, v) in vals.iter().enumerate() {
                    buf[k] = match v {
                        Value::Array(a) => a.broadcast(i, j),
                        s => s.clone(),
                    };
                }
                data.push(match func(&buf) {
                    Value::Array(a) => a.data.first().cloned().unwrap_or(Value::Err(ErrKind::Calc)),
                    Value::Ref(a) => self.value_at((a.sheet, a.range.r1, a.range.c1)),
                    x => x,
                });
            }
        }
        Value::Array(Rc::new(Array::new(rows, cols, data)))
    }

    /// A binary operator over two values (arrays element by element).
    pub fn binary(&self, op: BinOp, a: Value, b: Value) -> Value {
        self.lift(vec![a, b], &|v| scalar_bin(op, &v[0], &v[1]))
    }

    // ----- argument helpers for the functions -----

    pub fn ev(&self, e: &Expr, f: &Frame) -> Value {
        self.eval(e, f)
    }

    pub fn ev_s(&self, e: &Expr, f: &Frame) -> Value {
        let v = self.eval(e, f);
        self.scalar(v, f)
    }

    pub fn num(&self, e: &Expr, f: &Frame) -> Result<f64, ErrKind> {
        to_num(&self.ev_s(e, f))
    }

    pub fn num_or(&self, a: &[Expr], i: usize, d: f64, f: &Frame) -> Result<f64, ErrKind> {
        match a.get(i) {
            None => Ok(d),
            Some(e) => self.num(e, f),
        }
    }

    /// A numeric argument whose absence or emptiness both mean the default (the newer functions).
    pub fn num_def(&self, a: &[Expr], i: usize, d: f64, f: &Frame) -> Result<f64, ErrKind> {
        match a.get(i) {
            None | Some(Expr::Missing) => Ok(d),
            Some(e) => self.num(e, f),
        }
    }

    pub fn text(&self, e: &Expr, f: &Frame) -> Result<String, ErrKind> {
        to_text(&self.ev_s(e, f))
    }

    pub fn boolean(&self, e: &Expr, f: &Frame) -> Result<bool, ErrKind> {
        to_bool(&self.ev_s(e, f))
    }

    pub fn bool_or(&self, a: &[Expr], i: usize, d: bool, f: &Frame) -> Result<bool, ErrKind> {
        match a.get(i) {
            None => Ok(d),
            Some(e) => self.boolean(e, f),
        }
    }
}

/// One binary operator on two plain values.
pub fn scalar_bin(op: BinOp, a: &Value, b: &Value) -> Value {
    let arith = |f: &dyn Fn(f64, f64) -> Result<f64, ErrKind>| -> Value {
        let x = match to_num(a) {
            Ok(x) => x,
            Err(e) => return Value::Err(e),
        };
        let y = match to_num(b) {
            Ok(y) => y,
            Err(e) => return Value::Err(e),
        };
        match f(x, y) {
            Ok(r) if r.is_finite() => Value::Num(r),
            Ok(_) => Value::Err(ErrKind::Num),
            Err(e) => Value::Err(e),
        }
    };
    match op {
        BinOp::Add => arith(&|x, y| Ok(x + y)),
        BinOp::Sub => arith(&|x, y| Ok(x - y)),
        BinOp::Mul => arith(&|x, y| Ok(x * y)),
        BinOp::Div => arith(&|x, y| if y == 0.0 { Err(ErrKind::Div0) } else { Ok(x / y) }),
        BinOp::Pow => arith(&power),
        BinOp::Concat => match (to_text(a), to_text(b)) {
            (Ok(x), Ok(y)) => Value::Text(x + &y),
            (Err(e), _) | (_, Err(e)) => Value::Err(e),
        },
        _ => match compare(a, b) {
            Err(e) => Value::Err(e),
            Ok(o) => Value::Bool(match op {
                BinOp::Eq => o.is_eq(),
                BinOp::Ne => o.is_ne(),
                BinOp::Lt => o.is_lt(),
                BinOp::Le => o.is_le(),
                BinOp::Gt => o.is_gt(),
                _ => o.is_ge(),
            }),
        },
    }
}

/// `x ^ y` with the spreadsheet's errors: 0^0 and roots of negatives are `#NUM!`, 0^-n `#DIV/0!`.
pub fn power(x: f64, y: f64) -> Result<f64, ErrKind> {
    if x == 0.0 && y == 0.0 {
        return Err(ErrKind::Num);
    }
    if x == 0.0 && y < 0.0 {
        return Err(ErrKind::Div0);
    }
    let r = x.powf(y);
    if r.is_nan() {
        Err(ErrKind::Num)
    } else {
        Ok(r)
    }
}
