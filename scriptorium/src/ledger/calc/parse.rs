//! Formula text to a tree. The grammar of the file's formulas (no leading `=` needed, `,` between
//! arguments): numbers, strings, TRUE/FALSE, error literals, references (`A1`, `$A$1`, `A$1`,
//! `A1:B2`, `A:C`, `3:5`, `Sheet2!A1`, `'My Sheet'!A1:B2`), names, function calls (the `_xlfn.`
//! and `_xlws.` prefixes dropped, `_xlpm.` on LET/LAMBDA names), array constants `{1,2;3,4}`,
//! prefix `+ - @`, postfix `%`, and the binary operators with the spreadsheet's precedence:
//! `:` and space (intersection) first, then negation, `%`, `^` (left to right), `* /`, `+ -`, `&`,
//! and the comparisons last. Structured (`Table1[Col]`), external (`[1]Sheet1!A1`), 3D
//! (`Sheet1:Sheet3!A1`) references and unions are kept as `Bad` (evaluated as `#REF!`).

use super::super::cellref::{col_index, parse_cell, CellRange, MAX_COLS, MAX_ROWS};
use super::value::ErrKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A parsed formula.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Err(ErrKind),
    /// A cell or rectangle; `sheet` None is the formula's own sheet.
    Ref { sheet: Option<String>, range: CellRange },
    /// A defined name, or a LET/LAMBDA variable (upper case).
    Name { sheet: Option<String>, name: String },
    /// An array constant, row by row.
    Array(Vec<Vec<Expr>>),
    Neg(Box<Expr>),
    Plus(Box<Expr>),
    Percent(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    /// `a:b` between two reference expressions.
    Range(Box<Expr>, Box<Expr>),
    /// `a b`: the intersection of two references.
    Isect(Box<Expr>, Box<Expr>),
    /// `@a`: implicit intersection.
    At(Box<Expr>),
    /// A function by its upper-case name.
    Call(String, Vec<Expr>),
    /// `LAMBDA(x, x+1)(2)`: a call on an expression's value.
    CallExpr(Box<Expr>, Vec<Expr>),
    /// An argument left empty (`IF(A1,,2)`).
    Missing,
    /// Something we read but do not evaluate (structured, external, 3D references, unions).
    Bad(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Bool(bool),
    Err(ErrKind),
    Ref(Option<String>, CellRange),
    Name(Option<String>, String),
    Func(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Semi,
    LBrace,
    RBrace,
    Bad(String),
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '\\' || c == '$'
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || c == '\\' || c == '$' || c == '?'
}

/// A function name as the evaluator knows it: upper case, file prefixes dropped.
pub fn normalize_fn(name: &str) -> String {
    let mut u = name.to_ascii_uppercase();
    loop {
        let before = u.len();
        for p in ["_XLFN.", "_XLWS.", "_XLL."] {
            if let Some(x) = u.strip_prefix(p) {
                u = x.to_string();
            }
        }
        if u.len() == before {
            break;
        }
    }
    u
}

fn normalize_name(name: &str) -> String {
    let u = name.to_ascii_uppercase();
    match u.strip_prefix("_XLPM.") {
        Some(x) => x.to_string(),
        None => u,
    }
}

struct Lexer {
    ch: Vec<char>,
    i: usize,
}

impl Lexer {
    fn peek(&self) -> Option<char> {
        self.ch.get(self.i).copied()
    }

    fn at(&self, j: usize) -> Option<char> {
        self.ch.get(j).copied()
    }

    /// `$`? digits at `j`: a 0-based row and the position after it.
    fn scan_row(&self, mut j: usize) -> Option<(u32, usize)> {
        if self.at(j) == Some('$') {
            j += 1;
        }
        let s = j;
        while self.at(j).is_some_and(|c| c.is_ascii_digit()) {
            j += 1;
        }
        if j == s || j - s > 7 {
            return None;
        }
        let digits: String = self.ch[s..j].iter().collect();
        let r: u32 = digits.parse().ok()?;
        if r == 0 || r > MAX_ROWS {
            return None;
        }
        Some((r - 1, j))
    }

    /// `3:5` / `$3:$5` at the cursor.
    fn row_range(&mut self) -> Option<CellRange> {
        let (r1, j) = self.scan_row(self.i)?;
        if self.at(j) != Some(':') {
            return None;
        }
        let (r2, k) = self.scan_row(j + 1)?;
        if self.at(k).is_some_and(|c| is_ident(c) || c == '(') {
            return None;
        }
        self.i = k;
        Some(CellRange::new(r1, 0, r2, MAX_COLS - 1))
    }

    fn ident(&mut self) -> String {
        let s = self.i;
        while self.peek().is_some_and(is_ident) {
            self.i += 1;
        }
        self.ch[s..self.i].iter().collect()
    }

    fn error_literal(&mut self) -> Option<ErrKind> {
        for (k, t) in ErrKind::all() {
            let n = t.chars().count();
            if self.i + n <= self.ch.len() {
                let s: String = self.ch[self.i..self.i + n].iter().collect();
                if s.eq_ignore_ascii_case(t) {
                    self.i += n;
                    return Some(k);
                }
            }
        }
        None
    }

    fn col_of(w: &str) -> Option<u32> {
        col_index(w.strip_prefix('$').unwrap_or(w))
    }

    /// After an identifier `w`: a cell, a range, a column range, TRUE/FALSE or a name.
    fn ref_or_name(&mut self, sheet: Option<String>, w: String) -> Tok {
        if let Some((r, c)) = parse_cell(&w) {
            if self.peek() == Some(':') {
                let save = self.i;
                self.i += 1;
                if self.peek().is_some_and(is_ident_start) {
                    let w2 = self.ident();
                    if let Some((r2, c2)) = parse_cell(&w2) {
                        if self.peek() != Some('(') && self.peek() != Some('!') {
                            return Tok::Ref(sheet, CellRange::new(r, c, r2, c2));
                        }
                    }
                }
                self.i = save;
            }
            return Tok::Ref(sheet, CellRange::cell(r, c));
        }
        if let Some(c1) = Self::col_of(&w) {
            if self.peek() == Some(':') && !w.ends_with('$') {
                let save = self.i;
                self.i += 1;
                if self.peek().is_some_and(is_ident_start) {
                    let w2 = self.ident();
                    if let Some(c2) = Self::col_of(&w2) {
                        if self.peek() != Some('(') && self.peek() != Some('!') {
                            return Tok::Ref(sheet, CellRange::new(0, c1, MAX_ROWS - 1, c2));
                        }
                    }
                }
                self.i = save;
            }
        }
        if sheet.is_none() {
            if w.eq_ignore_ascii_case("TRUE") {
                return Tok::Bool(true);
            }
            if w.eq_ignore_ascii_case("FALSE") {
                return Tok::Bool(false);
            }
        }
        Tok::Name(sheet, normalize_name(&w))
    }

    /// What follows `Sheet!`.
    fn ref_part(&mut self, sheet: String) -> Result<Tok, String> {
        match self.peek() {
            Some('#') => match self.error_literal() {
                Some(k) => Ok(Tok::Err(k)),
                None => Err("bad error literal".into()),
            },
            Some(c) if c.is_ascii_digit() || (c == '$' && self.at(self.i + 1).is_some_and(|d| d.is_ascii_digit())) => match self.row_range() {
                Some(r) => Ok(Tok::Ref(Some(sheet), r)),
                None => Err("bad row reference".into()),
            },
            Some(c) if is_ident_start(c) => {
                let w = self.ident();
                Ok(self.ref_or_name(Some(sheet), w))
            }
            _ => Err("a reference must follow '!'".into()),
        }
    }

    fn skip_brackets(&mut self) {
        let mut depth = 0;
        while let Some(c) = self.peek() {
            self.i += 1;
            if c == '[' {
                depth += 1;
            } else if c == ']' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
        }
    }

    fn number(&mut self) -> Result<Tok, String> {
        let s = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        if self.peek() == Some('.') {
            self.i += 1;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            let mut j = self.i + 1;
            if matches!(self.at(j), Some('+') | Some('-')) {
                j += 1;
            }
            if self.at(j).is_some_and(|c| c.is_ascii_digit()) {
                self.i = j;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
            }
        }
        let t: String = self.ch[s..self.i].iter().collect();
        t.parse::<f64>().map(Tok::Num).map_err(|_| format!("bad number {t}"))
    }

    fn tokens(mut self) -> Result<Vec<(Tok, bool)>, String> {
        let mut out = Vec::new();
        loop {
            let mut space = false;
            while self.peek().is_some_and(|c| c.is_whitespace()) {
                self.i += 1;
                space = true;
            }
            let Some(c) = self.peek() else { break };
            let tok = match c {
                '"' => {
                    self.i += 1;
                    let mut s = String::new();
                    loop {
                        match self.peek() {
                            None => return Err("unclosed string".into()),
                            Some('"') => {
                                self.i += 1;
                                if self.peek() == Some('"') {
                                    s.push('"');
                                    self.i += 1;
                                } else {
                                    break;
                                }
                            }
                            Some(ch) => {
                                s.push(ch);
                                self.i += 1;
                            }
                        }
                    }
                    Tok::Str(s)
                }
                '#' => match self.error_literal() {
                    Some(k) => Tok::Err(k),
                    None => return Err("bad error literal".into()),
                },
                '\'' => {
                    self.i += 1;
                    let mut s = String::new();
                    loop {
                        match self.peek() {
                            None => return Err("unclosed sheet name".into()),
                            Some('\'') => {
                                self.i += 1;
                                if self.peek() == Some('\'') {
                                    s.push('\'');
                                    self.i += 1;
                                } else {
                                    break;
                                }
                            }
                            Some(ch) => {
                                s.push(ch);
                                self.i += 1;
                            }
                        }
                    }
                    if self.peek() != Some('!') {
                        return Err("a quoted sheet name must be followed by '!'".into());
                    }
                    self.i += 1;
                    let t = self.ref_part(s.clone())?;
                    if s.contains(':') || s.starts_with('[') {
                        Tok::Bad(format!("'{s}'!"))
                    } else {
                        t
                    }
                }
                '[' => {
                    let s = self.i;
                    self.skip_brackets();
                    while self.peek().is_some_and(|c| is_ident(c) || c == '[') {
                        if self.peek() == Some('[') {
                            self.skip_brackets();
                        } else {
                            self.i += 1;
                        }
                    }
                    if self.peek() == Some('!') {
                        let _ = self.ident();
                        self.i += 1;
                        if self.peek().is_some_and(|c| is_ident_start(c) || c.is_ascii_digit()) {
                            let _ = self.ident();
                            if self.peek() == Some(':') {
                                self.i += 1;
                                let _ = self.ident();
                            }
                        }
                    }
                    Tok::Bad(self.ch[s..self.i].iter().collect())
                }
                c if c.is_ascii_digit() || (c == '.' && self.at(self.i + 1).is_some_and(|d| d.is_ascii_digit())) => match self.row_range() {
                    Some(r) => Tok::Ref(None, r),
                    None => self.number()?,
                },
                '$' if self.at(self.i + 1).is_some_and(|d| d.is_ascii_digit()) => match self.row_range() {
                    Some(r) => Tok::Ref(None, r),
                    None => return Err("bad reference".into()),
                },
                c if is_ident_start(c) => {
                    let w = self.ident();
                    match self.peek() {
                        Some('!') => {
                            self.i += 1;
                            self.ref_part(w)?
                        }
                        Some('(') => {
                            self.i += 1;
                            Tok::Func(normalize_fn(&w))
                        }
                        Some('[') => {
                            let s = self.i;
                            self.skip_brackets();
                            Tok::Bad(format!("{w}{}", self.ch[s..self.i].iter().collect::<String>()))
                        }
                        Some(':') if self.at(self.i + 1).is_some_and(is_ident_start) && !w.starts_with('$') && parse_cell(&w).is_none() && Self::col_of(&w).is_none() => {
                            // `Sheet1:Sheet3!A1` (a 3D reference).
                            let s = self.i;
                            self.i += 1;
                            let _ = self.ident();
                            if self.peek() == Some('!') {
                                self.i += 1;
                                let _ = self.ref_part(String::new());
                                Tok::Bad(format!("{w}{}", self.ch[s..self.i].iter().collect::<String>()))
                            } else {
                                self.i = s;
                                self.ref_or_name(None, w)
                            }
                        }
                        _ => self.ref_or_name(None, w),
                    }
                }
                '+' | '-' | '*' | '/' | '^' | '&' | '%' | '@' | '=' | ':' => {
                    self.i += 1;
                    Tok::Op(match c {
                        '+' => "+",
                        '-' => "-",
                        '*' => "*",
                        '/' => "/",
                        '^' => "^",
                        '&' => "&",
                        '%' => "%",
                        '@' => "@",
                        ':' => ":",
                        _ => "=",
                    })
                }
                '<' => {
                    self.i += 1;
                    match self.peek() {
                        Some('=') => {
                            self.i += 1;
                            Tok::Op("<=")
                        }
                        Some('>') => {
                            self.i += 1;
                            Tok::Op("<>")
                        }
                        _ => Tok::Op("<"),
                    }
                }
                '>' => {
                    self.i += 1;
                    if self.peek() == Some('=') {
                        self.i += 1;
                        Tok::Op(">=")
                    } else {
                        Tok::Op(">")
                    }
                }
                '(' | ')' | ',' | ';' | '{' | '}' => {
                    self.i += 1;
                    match c {
                        '(' => Tok::LParen,
                        ')' => Tok::RParen,
                        ',' => Tok::Comma,
                        ';' => Tok::Semi,
                        '{' => Tok::LBrace,
                        _ => Tok::RBrace,
                    }
                }
                other => return Err(format!("unexpected '{other}'")),
            };
            out.push((tok, space));
        }
        Ok(out)
    }
}

struct Parser {
    toks: Vec<(Tok, bool)>,
    p: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.p).map(|t| &t.0)
    }

    fn space(&self) -> bool {
        self.toks.get(self.p).is_some_and(|t| t.1)
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.p).map(|t| t.0.clone());
        self.p += 1;
        t
    }

    fn op(&self) -> Option<&'static str> {
        match self.peek() {
            Some(Tok::Op(o)) => Some(o),
            _ => None,
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut l = self.concat()?;
        while let Some(o) = self.op() {
            let op = match o {
                "=" => BinOp::Eq,
                "<>" => BinOp::Ne,
                "<" => BinOp::Lt,
                "<=" => BinOp::Le,
                ">" => BinOp::Gt,
                ">=" => BinOp::Ge,
                _ => break,
            };
            self.p += 1;
            let r = self.concat()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn concat(&mut self) -> Result<Expr, String> {
        let mut l = self.additive()?;
        while self.op() == Some("&") {
            self.p += 1;
            let r = self.additive()?;
            l = Expr::Bin(BinOp::Concat, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn additive(&mut self) -> Result<Expr, String> {
        let mut l = self.mult()?;
        while let Some(o @ ("+" | "-")) = self.op() {
            self.p += 1;
            let r = self.mult()?;
            l = Expr::Bin(if o == "+" { BinOp::Add } else { BinOp::Sub }, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn mult(&mut self) -> Result<Expr, String> {
        let mut l = self.power()?;
        while let Some(o @ ("*" | "/")) = self.op() {
            self.p += 1;
            let r = self.power()?;
            l = Expr::Bin(if o == "*" { BinOp::Mul } else { BinOp::Div }, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn power(&mut self) -> Result<Expr, String> {
        let mut l = self.percent()?;
        while self.op() == Some("^") {
            self.p += 1;
            let r = self.percent()?;
            l = Expr::Bin(BinOp::Pow, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn percent(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        while self.op() == Some("%") {
            self.p += 1;
            e = Expr::Percent(Box::new(e));
        }
        Ok(e)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.op() {
            Some("-") => {
                self.p += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some("+") => {
                self.p += 1;
                Ok(Expr::Plus(Box::new(self.unary()?)))
            }
            Some("@") => {
                self.p += 1;
                Ok(Expr::At(Box::new(self.refexpr()?)))
            }
            _ => self.refexpr(),
        }
    }

    fn refexpr(&mut self) -> Result<Expr, String> {
        let mut e = self.postfix()?;
        loop {
            if self.op() == Some(":") {
                self.p += 1;
                let r = self.postfix()?;
                e = Expr::Range(Box::new(e), Box::new(r));
            } else if self.space() && matches!(self.peek(), Some(Tok::Ref(..)) | Some(Tok::Name(..))) && matches!(e, Expr::Ref { .. } | Expr::Name { .. } | Expr::Range(..) | Expr::Isect(..)) {
                let r = self.postfix()?;
                e = Expr::Isect(Box::new(e), Box::new(r));
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let (mut e, mut callable) = self.atom()?;
        while callable && self.peek() == Some(&Tok::LParen) && !self.space() {
            self.p += 1;
            let args = self.args()?;
            e = Expr::CallExpr(Box::new(e), args);
            callable = true;
        }
        Ok(e)
    }

    fn args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if self.peek() == Some(&Tok::RParen) {
            self.p += 1;
            return Ok(args);
        }
        loop {
            if matches!(self.peek(), Some(Tok::Comma) | Some(Tok::RParen)) {
                args.push(Expr::Missing);
            } else {
                args.push(self.expr()?);
            }
            match self.bump() {
                Some(Tok::Comma) => continue,
                Some(Tok::RParen) => break,
                _ => return Err("expected ',' or ')'".into()),
            }
        }
        Ok(args)
    }

    fn constant(&mut self) -> Result<Expr, String> {
        let neg = match self.op() {
            Some("-") => {
                self.p += 1;
                true
            }
            Some("+") => {
                self.p += 1;
                false
            }
            _ => false,
        };
        match self.bump() {
            Some(Tok::Num(n)) => Ok(Expr::Num(if neg { -n } else { n })),
            Some(Tok::Str(s)) if !neg => Ok(Expr::Str(s)),
            Some(Tok::Bool(b)) if !neg => Ok(Expr::Bool(b)),
            Some(Tok::Err(e)) if !neg => Ok(Expr::Err(e)),
            _ => Err("an array constant holds only numbers, text, booleans and errors".into()),
        }
    }

    fn atom(&mut self) -> Result<(Expr, bool), String> {
        match self.bump() {
            Some(Tok::Num(n)) => Ok((Expr::Num(n), false)),
            Some(Tok::Str(s)) => Ok((Expr::Str(s), false)),
            Some(Tok::Bool(b)) => Ok((Expr::Bool(b), false)),
            Some(Tok::Err(e)) => Ok((Expr::Err(e), false)),
            Some(Tok::Ref(sheet, range)) => Ok((Expr::Ref { sheet, range }, false)),
            Some(Tok::Name(sheet, name)) => Ok((Expr::Name { sheet, name }, false)),
            Some(Tok::Bad(s)) => Ok((Expr::Bad(s), false)),
            Some(Tok::Func(name)) => {
                let args = self.args()?;
                Ok((Expr::Call(name, args), true))
            }
            Some(Tok::LParen) => {
                let e = self.expr()?;
                match self.bump() {
                    Some(Tok::RParen) => Ok((e, true)),
                    Some(Tok::Comma) => {
                        // A union `(A1,B2)`: read and kept unevaluated.
                        let mut depth = 1;
                        while let Some(t) = self.bump() {
                            match t {
                                Tok::LParen | Tok::Func(_) => depth += 1,
                                Tok::RParen => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        Ok((Expr::Bad("union".into()), false))
                    }
                    _ => Err("expected ')'".into()),
                }
            }
            Some(Tok::LBrace) => {
                let mut rows = vec![Vec::new()];
                loop {
                    let c = self.constant()?;
                    rows.last_mut().expect("a row").push(c);
                    match self.bump() {
                        Some(Tok::Comma) => {}
                        Some(Tok::Semi) => rows.push(Vec::new()),
                        Some(Tok::RBrace) => break,
                        _ => return Err("expected ',', ';' or '}'".into()),
                    }
                }
                let w = rows[0].len();
                if rows.iter().any(|r| r.len() != w) {
                    return Err("array constant rows differ in length".into());
                }
                Ok((Expr::Array(rows), false))
            }
            Some(t) => Err(format!("unexpected {t:?}")),
            None => Err("the formula ends too early".into()),
        }
    }
}

/// Parses formula text (with or without the leading `=`).
pub fn parse_formula(text: &str) -> Result<Expr, String> {
    let t = text.trim_start();
    let t = t.strip_prefix('=').unwrap_or(t);
    let toks = Lexer { ch: t.chars().collect(), i: 0 }.tokens()?;
    if toks.is_empty() {
        return Err("empty formula".into());
    }
    let mut p = Parser { toks, p: 0 };
    let e = p.expr()?;
    if p.p < p.toks.len() {
        return Err(format!("unexpected {:?}", p.toks[p.p].0));
    }
    Ok(e)
}
