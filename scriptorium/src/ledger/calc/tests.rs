//! The engine's tests: the parser, coercion, errors, every function with hand-worked values, the
//! graph (a chain, a circle, incremental recalculation, shared formulas, sheet references, names).

use super::*;
use crate::ledger::cellref::{parse_cell, MAX_COLS, MAX_ROWS};
use crate::ledger::model::{Cell, CellValue, DefinedName, Sheet};

fn lit(v: &str) -> CellValue {
    if let Some(t) = v.strip_prefix('\'') {
        return CellValue::Text(t.into());
    }
    if let Ok(n) = v.parse::<f64>() {
        return CellValue::Number(n);
    }
    match v {
        "TRUE" => CellValue::Bool(true),
        "FALSE" => CellValue::Bool(false),
        _ if v.starts_with('#') => CellValue::Error(v.into()),
        _ => CellValue::Text(v.into()),
    }
}

fn put(sh: &mut Sheet, a1: &str, v: &str) {
    let (r, c) = parse_cell(a1).expect("a cell");
    let cell = match v.strip_prefix('=') {
        Some(f) => Cell { value: CellValue::Empty, formula: Some(f.into()), style: 0 },
        None => Cell { value: lit(v), formula: None, style: 0 },
    };
    sh.rows.entry(r).or_default().cells.insert(c, cell);
}

fn sheet(name: &str, cells: &[(&str, &str)]) -> Sheet {
    let mut s = Sheet::new(name);
    for (a, v) in cells {
        put(&mut s, a, v);
    }
    s
}

fn get(sheets: &[Sheet], s: usize, a1: &str) -> CellValue {
    let (r, c) = parse_cell(a1).expect("a cell");
    sheets[s].cell(r, c).map(|c| c.value.clone()).unwrap_or(CellValue::Empty)
}

fn key(s: usize, a1: &str) -> CellKey {
    let (r, c) = parse_cell(a1).expect("a cell");
    (s, r, c)
}

/// The workbook most function tests read.
fn data() -> Vec<Sheet> {
    let d = sheet(
        "Data",
        &[
            ("A1", "10"), ("A2", "20"), ("A3", "30"), ("A4", "40"), ("A5", "50"),
            ("B1", "apple"), ("B2", "banana"), ("B3", "cherry"), ("B4", "apple"), ("B5", "date"),
            ("C1", "1"), ("C2", "2"), ("C3", "3"), ("C4", "4"), ("C5", "5"),
            ("D1", "TRUE"), ("D2", "'5"), ("D4", "hello"),
            ("E1", "3"), ("E2", "1"), ("E3", "2"),
            ("F1", "1"), ("F2", "5"), ("F3", "10"), ("F4", "20"),
            ("G1", "low"), ("G2", "mid"), ("G3", "high"), ("G4", "top"),
            ("H1", "x"), ("H2", "y"), ("H3", "x"),
            ("I1", "100"), ("I2", "200"), ("I3", "300"),
        ],
    );
    let m = sheet("My Sheet", &[("A1", "7"), ("B2", "8")]);
    vec![d, m]
}

/// Evaluates a formula standing in Z100 of the data sheet.
fn ev(formula: &str) -> CellValue {
    let book = data();
    evaluate(&book, &Options::default(), 0, 99, 25, formula)
}

fn n(formula: &str) -> f64 {
    match ev(formula) {
        CellValue::Number(x) => x,
        other => panic!("{formula} gave {other:?}, not a number"),
    }
}

fn s(formula: &str) -> String {
    match ev(formula) {
        CellValue::Text(x) => x,
        other => panic!("{formula} gave {other:?}, not text"),
    }
}

fn b(formula: &str) -> bool {
    match ev(formula) {
        CellValue::Bool(x) => x,
        other => panic!("{formula} gave {other:?}, not a boolean"),
    }
}

fn e(formula: &str) -> String {
    match ev(formula) {
        CellValue::Error(x) => x,
        other => panic!("{formula} gave {other:?}, not an error"),
    }
}

fn near(formula: &str, want: f64, tol: f64) {
    let got = n(formula);
    assert!((got - want).abs() <= tol, "{formula}: got {got}, want {want}");
}

fn eq(formula: &str, want: f64) {
    near(formula, want, 1e-9);
}

// ----- parser -----

#[test]
fn parser_reads_references_of_every_shape() {
    let r = |t: &str| match parse_formula(t).expect("parses") {
        Expr::Ref { sheet, range } => (sheet, range),
        other => panic!("{t}: {other:?}"),
    };
    assert_eq!(r("A1"), (None, CellRange::cell(0, 0)));
    assert_eq!(r("$B$2"), (None, CellRange::cell(1, 1)));
    assert_eq!(r("C$3"), (None, CellRange::cell(2, 2)));
    assert_eq!(r("$A1:B$2"), (None, CellRange::new(0, 0, 1, 1)));
    assert_eq!(r("A:C"), (None, CellRange::new(0, 0, MAX_ROWS - 1, 2)));
    assert_eq!(r("$3:$5"), (None, CellRange::new(2, 0, 4, MAX_COLS - 1)));
    assert_eq!(r("2:2"), (None, CellRange::new(1, 0, 1, MAX_COLS - 1)));
    assert_eq!(r("Sheet2!A1"), (Some("Sheet2".into()), CellRange::cell(0, 0)));
    assert_eq!(r("'My Sheet'!A1:B2"), (Some("My Sheet".into()), CellRange::new(0, 0, 1, 1)));
    assert_eq!(r("'It''s'!B:B"), (Some("It's".into()), CellRange::new(0, 1, MAX_ROWS - 1, 1)));
    assert_eq!(r("=a1"), (None, CellRange::cell(0, 0)));
}

#[test]
fn parser_reads_names_functions_arrays_and_what_it_keeps_aside() {
    assert_eq!(parse_formula("TaxRate*2").unwrap(), Expr::Bin(BinOp::Mul, Box::new(Expr::Name { sheet: None, name: "TAXRATE".into() }), Box::new(Expr::Num(2.0))));
    match parse_formula("_xlfn._xlws.SORT(A1:A3)").unwrap() {
        Expr::Call(name, args) => {
            assert_eq!(name, "SORT");
            assert_eq!(args.len(), 1);
        }
        other => panic!("{other:?}"),
    }
    match parse_formula("_xlfn.LET(_xlpm.x,1,_xlpm.x)").unwrap() {
        Expr::Call(name, args) => {
            assert_eq!(name, "LET");
            assert_eq!(args[0], Expr::Name { sheet: None, name: "X".into() });
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(parse_formula("{1,-2;\"a\",TRUE}").unwrap(), Expr::Array(vec![vec![Expr::Num(1.0), Expr::Num(-2.0)], vec![Expr::Str("a".into()), Expr::Bool(true)]]));
    assert_eq!(parse_formula("IF(A1,,2)").unwrap(), Expr::Call("IF".into(), vec![Expr::Ref { sheet: None, range: CellRange::cell(0, 0) }, Expr::Missing, Expr::Num(2.0)]));
    assert!(matches!(parse_formula("SUM(A1:A3 A2:C2)").unwrap(), Expr::Call(_, a) if matches!(a[0], Expr::Isect(..))));
    assert!(matches!(parse_formula("Table1[Amount]").unwrap(), Expr::Bad(_)));
    assert!(matches!(parse_formula("[1]Sheet1!A1").unwrap(), Expr::Bad(_)));
    assert!(matches!(parse_formula("SUM(Sheet1:Sheet3!B5)").unwrap(), Expr::Call(_, a) if matches!(a[0], Expr::Bad(_))));
    assert_eq!(parse_formula("#N/A").unwrap(), Expr::Err(ErrKind::NA));
    assert_eq!(parse_formula("\"say \"\"hi\"\"\"").unwrap(), Expr::Str("say \"hi\"".into()));
    assert!(matches!(parse_formula("LAMBDA(x,x+1)(2)").unwrap(), Expr::CallExpr(..)));
    assert!(parse_formula("1+").is_err());
    assert!(parse_formula("SUM(1,2").is_err());
    assert!(parse_formula("\"open").is_err());
    assert!(parse_formula("").is_err());
}

#[test]
fn operator_precedence_and_associativity() {
    eq("1+2*3", 7.0);
    eq("(1+2)*3", 9.0);
    eq("-2^2", 4.0); // negation before ^
    eq("2^3^2", 64.0); // ^ left to right
    eq("2^-1", 0.5);
    eq("2*3%", 0.06);
    eq("50%%", 0.005);
    eq("10-4-3", 3.0);
    eq("12/3/2", 2.0);
    assert_eq!(s("1&2+3"), "15"); // + before &
    assert!(b("1+2=3"));
    assert!(b("\"a\"&\"b\"=\"AB\"")); // & before =, text without case
    assert!(b("2>1=TRUE"));
    eq("+5", 5.0);
    eq("--\"3\"", 3.0);
}

// ----- coercion and errors -----

#[test]
fn coercion_follows_the_spreadsheet() {
    eq("\"1\"+1", 2.0);
    eq("TRUE+1", 2.0);
    eq("D3+1", 1.0); // blank is 0
    eq("\"10%\"*2", 0.2);
    eq("\"$1,000\"+0", 1000.0);
    eq("\"(5)\"*1", -5.0);
    eq("\"1e3\"+0", 1000.0);
    eq("\"2026-11-26\"+0", 46352.0);
    eq("\"12:00\"+0", 0.5);
    eq("\"6:00 PM\"+0", 0.75);
    assert_eq!(e("\"a\"+1"), "#VALUE!");
    assert_eq!(e("\"\"+1"), "#VALUE!");
    assert_eq!(e("\"1,00\"+0"), "#VALUE!");
    assert_eq!(s("\"x\"&1.5"), "x1.5");
    assert_eq!(s("\"x\"&TRUE"), "xTRUE");
    assert_eq!(s("1/3&\"\""), "0.333333333333333");
    assert_eq!(s("D3&\"!\""), "!");
    assert!(b("\"abc\"=\"ABC\""));
    assert!(b("1<\"a\"")); // numbers < text < booleans
    assert!(b("\"z\"<TRUE"));
    assert!(b("0.1+0.2=0.3"));
    assert!(b("D3=0") && b("D3=\"\"") && b("D3=FALSE"));
    assert!(!b("D3=\"x\""));
    eq("0.3-0.2-0.1", 0.0); // the final subtraction's rounding crumb is 0
    assert!(n("1*(0.3-0.2-0.1)") != 0.0); // but not inside
    assert_eq!(num_to_text(1e20), "1E+20");
    assert_eq!(num_to_text(-1.5), "-1.5");
    assert_eq!(num_to_text(123456789012.0), "123456789012");
    assert_eq!(num_to_text(0.1 + 0.2), "0.3");
}

#[test]
fn errors_propagate_and_are_literals() {
    assert_eq!(e("1/0"), "#DIV/0!");
    assert_eq!(e("#N/A+1"), "#N/A");
    assert_eq!(e("1+#REF!"), "#REF!");
    assert_eq!(e("#DIV/0!&#N/A"), "#DIV/0!"); // left first
    assert_eq!(e("NA()=1"), "#N/A");
    assert_eq!(e("0^0"), "#NUM!");
    assert_eq!(e("(-8)^(1/3)"), "#NUM!");
    assert_eq!(e("0^-1"), "#DIV/0!");
    assert_eq!(e("NoSuchFunction(1)"), "#NAME?");
    assert_eq!(e("NoSuchName+1"), "#NAME?");
    assert_eq!(e("Nope!A1"), "#REF!");
    assert_eq!(e("Table1[Amount]"), "#REF!");
    assert_eq!(e("A1:A2 C1:C2"), "#NULL!");
    for k in ["#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#SPILL!", "#CALC!", "#GETTING_DATA"] {
        assert_eq!(ErrKind::parse(k).map(|x| x.text()), Some(k));
    }
}

#[test]
fn references_ranges_intersection_and_sheets() {
    eq("A3", 30.0);
    eq("'My Sheet'!A1+'my sheet'!B2", 15.0);
    eq("SUM(A1:A3 A2:C2)", 20.0); // intersection
    eq("SUM(A1:INDEX(A1:A5,3))", 60.0); // range operator over a function's reference
    eq("SUM(A:A)", 150.0);
    eq("SUM(1:1)", 10.0 + 1.0 + 3.0 + 1.0 + 100.0); // A1 C1 E1 F1 I1 (D1 TRUE skipped)
    // Implicit intersection: a column range in row 3 answers that row's cell.
    let book = data();
    assert_eq!(evaluate(&book, &Options::default(), 0, 2, 25, "A1:A5"), CellValue::Number(30.0));
    assert_eq!(evaluate(&book, &Options::default(), 0, 2, 25, "@A1:A5*2"), CellValue::Number(60.0));
    assert_eq!(evaluate(&book, &Options::default(), 0, 2, 25, "_xlfn.SINGLE(A1:A5)"), CellValue::Number(30.0));
    assert_eq!(evaluate(&book, &Options::default(), 0, 9, 25, "A1:A5"), CellValue::Error("#VALUE!".into()));
    // Arrays broadcast through operators.
    eq("SUM(A1:A3*{1;2;3})", 10.0 + 40.0 + 90.0);
    eq("SUM({1,2,3}+{10;20})", (11.0 + 12.0 + 13.0) + (21.0 + 22.0 + 23.0));
}

#[test]
fn the_1900_date_system_and_time_fractions() {
    eq("DATE(1900,1,1)", 1.0);
    eq("DATE(1900,2,28)", 59.0);
    eq("DATE(1900,2,29)", 60.0); // the day that never was
    eq("DATE(1900,3,1)", 61.0);
    eq("DAY(60)", 29.0);
    eq("MONTH(60)", 2.0);
    eq("WEEKDAY(1)", 1.0); // the spreadsheet's 1 January 1900 is a Sunday
    assert_eq!(s("TEXT(60,\"yyyy-mm-dd\")"), "1900-02-29");
    eq("TIME(18,0,0)", 0.75);
    eq("HOUR(46352.75)", 18.0);
    eq("EOMONTH(DATE(1900,2,1),0)", 60.0);
}

// ----- functions -----

#[test]
fn every_listed_function_is_known() {
    assert_eq!(FUNCTIONS[119], "VSTACK");
    assert!(FUNCTIONS.len() >= 120);
    for name in FUNCTIONS {
        let v = ev(&format!("{name}()"));
        assert_ne!(v, CellValue::Error("#NAME?".into()), "{name}");
    }
}

#[test]
fn math_functions() {
    eq("SUM(A1:A5)", 150.0);
    eq("SUM(A1:A5,5,\"5\",TRUE)", 161.0);
    eq("SUM(B1:B5)", 0.0);
    eq("SUM(D1:D4)", 0.0); // TRUE and "5" in a range are skipped
    assert_eq!(e("SUM(\"abc\")"), "#VALUE!");
    assert_eq!(e("SUM(A1:A2,NA())"), "#N/A");
    eq("PRODUCT(5,15,30)", 2250.0);
    eq("ROUND(2.15,1)", 2.2);
    eq("ROUND(2.149,1)", 2.1);
    eq("ROUND(-1.475,2)", -1.48);
    eq("ROUND(2.675,2)", 2.68);
    eq("ROUND(1.005,2)", 1.01);
    eq("ROUND(21.5,-1)", 20.0);
    eq("ROUND(626.3,-3)", 1000.0);
    eq("ROUND(1.98,-1)", 0.0);
    eq("ROUND(-50.55,-2)", -100.0);
    eq("ROUNDUP(3.2,0)", 4.0);
    eq("ROUNDUP(76.9,0)", 77.0);
    eq("ROUNDUP(3.14159,3)", 3.142);
    eq("ROUNDUP(-3.14159,1)", -3.2);
    eq("ROUNDUP(31415.92654,-2)", 31500.0);
    eq("ROUNDDOWN(3.2,0)", 3.0);
    eq("ROUNDDOWN(-3.14159,1)", -3.1);
    eq("ROUNDDOWN(31415.92654,-2)", 31400.0);
    eq("INT(8.9)", 8.0);
    eq("INT(-8.9)", -9.0);
    eq("TRUNC(-8.9)", -8.0);
    eq("TRUNC(3.14159,2)", 3.14);
    eq("ABS(-4)", 4.0);
    eq("MOD(3,2)", 1.0);
    eq("MOD(-3,2)", 1.0);
    eq("MOD(3,-2)", -1.0);
    assert_eq!(e("MOD(3,0)"), "#DIV/0!");
    eq("POWER(5,2)", 25.0);
    eq("POWER(98.6,3.2)", 98.6f64.powf(3.2));
    eq("SQRT(16)", 4.0);
    assert_eq!(e("SQRT(-16)"), "#NUM!");
    eq("CEILING.MATH(24.3,5)", 25.0);
    eq("CEILING.MATH(6.7)", 7.0);
    eq("CEILING.MATH(-8.1,2)", -8.0);
    eq("CEILING.MATH(-5.5,2,-1)", -6.0);
    eq("SUMPRODUCT({3,4;8,6;1,9},{2,7;6,7;5,3})", 156.0);
    eq("SUMPRODUCT((B1:B5=\"apple\")*A1:A5)", 50.0);
    assert_eq!(e("SUMPRODUCT(A1:A2,A1:A3)"), "#VALUE!");
    eq("SUBTOTAL(9,A1:A5)", 150.0);
    eq("SUBTOTAL(1,A1:A5)", 30.0);
    eq("SUBTOTAL(2,A1:B5)", 5.0);
    eq("SUBTOTAL(3,A1:B5)", 10.0);
    eq("SUBTOTAL(4,A1:A5)", 50.0);
    eq("SUBTOTAL(5,A1:A5)", 10.0);
    eq("SUMIF(A1:A5,\">25\")", 120.0);
    eq("SUMIF(B1:B5,\"apple\",A1:A5)", 50.0);
    eq("SUMIF(B1:B5,\"*an*\",C1:C5)", 2.0);
    eq("SUMIF(B1:B5,\"apple\",A1)", 50.0); // the sum range grows to the criteria range's shape
    eq("SUM(SUMIF(B1:B5,{\"apple\",\"date\"},A1:A5))", 100.0);
    eq("SUMIFS(A1:A5,B1:B5,\"apple\",C1:C5,\">1\")", 40.0);
    set_seed(42);
    let r = n("RAND()");
    assert!((0.0..1.0).contains(&r));
    for _ in 0..20 {
        let d = n("RANDBETWEEN(1,6)");
        assert!((1.0..=6.0).contains(&d) && d.fract() == 0.0);
    }
    assert_eq!(e("RANDBETWEEN(5,1)"), "#NUM!");
}

#[test]
fn subtotal_skips_hidden_rows_and_other_subtotals() {
    let mut sh = sheet("S", &[("A1", "1"), ("A2", "2"), ("A3", "4"), ("A4", "=SUBTOTAL(9,A1:A3)"), ("A5", "=SUBTOTAL(109,A1:A4)"), ("A6", "=SUBTOTAL(9,A1:A4)")]);
    sh.rows.get_mut(&1).unwrap().hidden = true;
    let mut book = vec![sh];
    recalc(&mut book);
    assert_eq!(get(&book, 0, "A4"), CellValue::Number(7.0));
    assert_eq!(get(&book, 0, "A5"), CellValue::Number(5.0)); // row 2 hidden, A4 a subtotal
    assert_eq!(get(&book, 0, "A6"), CellValue::Number(7.0));
}

#[test]
fn statistical_functions() {
    eq("AVERAGE(A1:A5)", 30.0);
    eq("AVERAGE(1,)", 0.5);
    assert_eq!(e("AVERAGE(B1:B5)"), "#DIV/0!");
    eq("COUNT(A1:D5)", 10.0);
    eq("COUNT(1,\"2\",\"x\",TRUE)", 3.0);
    eq("COUNTA(A1:D5)", 18.0);
    eq("COUNTA(1,\"\",FALSE)", 3.0);
    eq("COUNTBLANK(D1:D5)", 2.0);
    eq("COUNTBLANK(Z1:Z10)", 10.0);
    eq("MIN(A1:A5)", 10.0);
    eq("MAX(A1:A5)", 50.0);
    eq("MIN(B1:B5)", 0.0);
    eq("MAX(A1:A5,99)", 99.0);
    eq("MEDIAN(1,2,3,4,5,6)", 3.5);
    eq("MEDIAN(A1:A5)", 30.0);
    eq("LARGE({3,5,3,5,4},3)", 4.0);
    eq("SMALL({3,4,5,2,3,4,6,4,7},4)", 4.0);
    assert_eq!(e("LARGE(A1:A5,6)"), "#NUM!");
    eq("RANK.EQ(7,{7,3.5,3.5,1,2})", 1.0);
    eq("RANK.EQ(3.5,{7,3.5,3.5,1,2},1)", 3.0);
    eq("RANK.EQ(2,{7,3.5,3.5,1,2})", 4.0);
    assert_eq!(e("RANK.EQ(9,{7,3.5})"), "#N/A");
    let data = "{1345,1301,1368,1322,1310,1370,1318,1350,1303,1299}";
    near(&format!("STDEV.S({data})"), 27.46391572, 1e-7);
    near(&format!("STDEV.P({data})"), 26.05455814, 1e-7);
    near(&format!("VAR.S({data})"), 754.2666667, 1e-6);
    near(&format!("STDEV({data})"), 27.46391572, 1e-7);
    assert_eq!(e("STDEV.S(1)"), "#DIV/0!");
    eq("COUNTIF(A1:A5,\">=30\")", 3.0);
    eq("COUNTIF(B1:B5,\"apple\")", 2.0);
    eq("COUNTIF(B1:B5,\"APPLE\")", 2.0);
    eq("COUNTIF(B1:B5,\"<>apple\")", 3.0);
    eq("COUNTIF(B1:B5,\"?a*\")", 2.0); // banana, date
    eq("COUNTIF(D1:D5,\"\")", 2.0);
    eq("COUNTIF(D1:D5,\"<>\")", 3.0);
    eq("COUNTIF(C1:C5,3)", 1.0);
    eq("COUNTIF(D1:D5,\"5\")", 1.0); // the text "5" matches the number criterion
    eq("COUNTIF(D1:D5,TRUE)", 1.0);
    eq("COUNTIF(B1:B5,\">b\")", 3.0); // banana, cherry, date
    eq("COUNTIFS(B1:B5,\"apple\",A1:A5,\">15\")", 1.0);
    assert_eq!(e("COUNTIFS(A1:A5,1,A1:A2,1)"), "#VALUE!");
    eq("AVERAGEIF(B1:B5,\"apple\",A1:A5)", 25.0);
    eq("AVERAGEIF(A1:A5,\"<35\")", 20.0);
    assert_eq!(e("AVERAGEIF(A1:A5,\">99\")"), "#DIV/0!");
    eq("AVERAGEIFS(A1:A5,C1:C5,\">=2\",C1:C5,\"<=4\")", 30.0);
    eq("MAXIFS(A1:A5,B1:B5,\"apple\")", 40.0);
    eq("MINIFS(A1:A5,B1:B5,\"apple\")", 10.0);
    eq("MAXIFS(A1:A5,B1:B5,\"none\")", 0.0);
}

#[test]
fn logical_and_information_functions() {
    assert_eq!(s("IF(A1>5,\"big\",\"small\")"), "big");
    assert!(!b("IF(FALSE,1)"));
    eq("IF(TRUE,)", 0.0);
    assert_eq!(e("IF(\"x\",1,2)"), "#VALUE!");
    eq("IF(\"true\",1,2)", 1.0);
    eq("SUM(IF(A1:A5>25,A1:A5,0))", 120.0);
    eq("SUM(IF(A1>5,A1:A2,A3))", 30.0); // IF hands back a reference
    assert_eq!(s("IFS(A1>100,\"a\",A1>5,\"b\")"), "b");
    assert_eq!(e("IFS(FALSE,1)"), "#N/A");
    assert_eq!(s("SWITCH(2,1,\"one\",2,\"two\",\"other\")"), "two");
    assert_eq!(s("SWITCH(9,1,\"one\",\"other\")"), "other");
    assert_eq!(e("SWITCH(9,1,\"one\")"), "#N/A");
    assert!(b("AND(TRUE,1,A1:A5)"));
    assert!(!b("AND(TRUE,FALSE)"));
    assert!(!b("OR(FALSE,0)"));
    assert!(b("OR(A1:A5>45)"));
    assert_eq!(e("OR(B1:B5)"), "#VALUE!");
    assert_eq!(e("AND(\"x\")"), "#VALUE!");
    assert!(b("NOT(0)"));
    assert!(b("XOR(TRUE,TRUE,TRUE)"));
    assert_eq!(s("IFERROR(1/0,\"x\")"), "x");
    eq("IFERROR(5,\"x\")", 5.0);
    eq("SUM(IFERROR(1/(A1:A3-20),0))", 0.1 - 0.1);
    eq("IFNA(NA(),0)", 0.0);
    assert_eq!(e("IFNA(1/0,0)"), "#DIV/0!");
    assert!(b("ISBLANK(D3)"));
    assert!(!b("ISBLANK(D4)"));
    assert!(b("ISNUMBER(A1)"));
    assert!(!b("ISNUMBER(D2)"));
    assert!(b("ISTEXT(B1)"));
    assert!(b("ISERROR(1/0)"));
    assert!(!b("ISERROR(1)"));
    assert!(b("ISNA(NA())"));
    assert!(!b("ISERR(NA())"));
    assert!(b("ISLOGICAL(D1)"));
    assert_eq!(e("NA()"), "#N/A");
    eq("SUM(--ISNUMBER(A1:D1))", 2.0); // A1 and C1
    assert!(b("TRUE()") && !b("FALSE()"));
}

#[test]
fn let_and_lambda() {
    eq("LET(x,2,y,x*3,x+y)", 8.0);
    eq("LET(x,A1:A5,SUM(x)/COUNT(x))", 30.0);
    eq("LAMBDA(a,b,a*b)(3,4)", 12.0);
    eq("LET(f,LAMBDA(n,n+1),f(41))", 42.0);
    eq("LET(k,10,f,LAMBDA(n,n+k),f(5))", 15.0); // the lambda keeps its scope
    assert_eq!(e("LAMBDA(x,x)"), "#CALC!");
    assert_eq!(e("LAMBDA(x,x)(1,2)"), "#VALUE!");
    assert_eq!(e("LET(1,2,3)"), "#VALUE!");
}

#[test]
fn text_functions() {
    assert_eq!(s("LEFT(\"Sale Price\",4)"), "Sale");
    assert_eq!(s("LEFT(\"Sweden\")"), "S");
    assert_eq!(s("RIGHT(\"Stock Number\",6)"), "Number");
    assert_eq!(s("MID(\"Fluid Flow\",7,20)"), "Flow");
    assert_eq!(s("MID(\"Fluid Flow\",20,5)"), "");
    assert_eq!(e("MID(\"Fluid\",0,1)"), "#VALUE!");
    assert_eq!(e("LEFT(\"a\",-1)"), "#VALUE!");
    eq("LEN(\"Phoenix, AZ\")", 11.0);
    eq("LEN(A1)", 2.0);
    eq("LEN(\"héllo\")", 5.0);
    assert_eq!(s("TRIM(\" First Quarter   Earnings \")"), "First Quarter Earnings");
    assert_eq!(s("UPPER(\"total\")"), "TOTAL");
    assert_eq!(s("LOWER(\"E. E. Cummings\")"), "e. e. cummings");
    assert_eq!(s("PROPER(\"this is a TITLE\")"), "This Is A Title");
    assert_eq!(s("PROPER(\"2-way street\")"), "2-Way Street");
    assert_eq!(s("PROPER(\"76BudGet\")"), "76Budget");
    assert_eq!(s("SUBSTITUTE(\"Sales Data\",\"Sales\",\"Cost\")"), "Cost Data");
    assert_eq!(s("SUBSTITUTE(\"Quarter 1, 2008\",\"1\",\"2\",1)"), "Quarter 2, 2008");
    assert_eq!(s("SUBSTITUTE(\"Quarter 1, 2011\",\"1\",\"2\",3)"), "Quarter 1, 2012");
    assert_eq!(s("SUBSTITUTE(\"aaa\",\"a\",\"b\")"), "bbb");
    assert_eq!(s("REPLACE(\"abcdefghijk\",6,5,\"*\")"), "abcde*k");
    assert_eq!(s("REPLACE(\"2009\",3,2,\"10\")"), "2010");
    eq("FIND(\"M\",\"Miriam McGovern\")", 1.0);
    eq("FIND(\"m\",\"Miriam McGovern\")", 6.0);
    eq("FIND(\"M\",\"Miriam McGovern\",3)", 8.0);
    assert_eq!(e("FIND(\"z\",\"abc\")"), "#VALUE!");
    eq("SEARCH(\"e\",\"Statements\",6)", 7.0);
    eq("SEARCH(\"MARGIN\",\"Profit Margin\")", 8.0);
    eq("SEARCH(\"p*n\",\"Profit Margin\")", 1.0);
    eq("SEARCH(\"?a\",\"xxBa\")", 3.0);
    assert_eq!(s("CONCAT(\"a\",1,TRUE)"), "a1TRUE");
    assert_eq!(s("CONCAT(B1:B2)"), "applebanana");
    assert_eq!(s("CONCATENATE(\"a\",\"b\",1)"), "ab1");
    assert_eq!(s("TEXTJOIN(\", \",TRUE,\"a\",\"\",\"b\")"), "a, b");
    assert_eq!(s("TEXTJOIN(\"-\",FALSE,A1:A2,D3)"), "10-20-");
    assert_eq!(s("TEXTJOIN({\"+\",\"=\"},TRUE,1,2,3)"), "1+2=3");
    assert_eq!(s("TEXT(1234.567,\"$#,##0.00\")"), "$1,234.57");
    assert_eq!(s("TEXT(0.25,\"0%\")"), "25%");
    assert_eq!(s("TEXT(46352,\"yyyy-mm-dd\")"), "2026-11-26");
    assert_eq!(s("TEXT(\"12\",\"0.0\")"), "12.0");
    eq("VALUE(\"$1,000\")", 1000.0);
    near("VALUE(\"16:48:00\")-VALUE(\"12:00:00\")", 0.2, 1e-12);
    assert_eq!(e("VALUE(\"abc\")"), "#VALUE!");
    assert_eq!(s("REPT(\"-\",3)"), "---");
    assert!(!b("EXACT(\"word\",\"Word\")"));
    assert!(b("EXACT(\"word\",\"word\")"));
    assert_eq!(s("HYPERLINK(\"http://example.com\",\"site\")"), "site");
    assert_eq!(s("HYPERLINK(\"http://example.com\")"), "http://example.com");
    assert_eq!(s("TEXTBEFORE(\"Red riding hood's, red hood\",\"hood\")"), "Red riding ");
    assert_eq!(s("TEXTAFTER(\"Red riding hood's, red hood\",\"hood\")"), "'s, red hood");
    assert_eq!(s("TEXTAFTER(\"Red riding hood's, red hood\",\"HOOD\",2,1)"), "");
    assert_eq!(s("TEXTBEFORE(\"a-b-c\",\"-\",-1)"), "a-b");
    assert_eq!(s("TEXTAFTER(\"a-b-c\",\"-\",2)"), "c");
    assert_eq!(e("TEXTAFTER(\"a-b-c\",\"x\")"), "#N/A");
    assert_eq!(s("TEXTBEFORE(\"a-b-c\",\"x\",,,,\"none\")"), "none");
    assert_eq!(s("TEXTBEFORE(\"abc\",\"x\",1,0,1)"), "abc"); // the end counts as a delimiter
    assert_eq!(s("INDEX(TEXTSPLIT(\"a,b;c,d\",\",\",\";\"),2,1)"), "c");
    eq("COLUMNS(TEXTSPLIT(\"a b c\",\" \"))", 3.0);
    eq("COLUMNS(TEXTSPLIT(\"a  b\",\" \",,TRUE))", 2.0);
    assert_eq!(e("INDEX(TEXTSPLIT(\"a,b;c\",\",\",\";\"),2,2)"), "#N/A");
    assert_eq!(s("INDEX(TEXTSPLIT(\"a,b;c\",\",\",\";\",,,\"-\"),2,2)"), "-");
}

#[test]
fn date_functions() {
    set_clock(Some(46352.75));
    eq("TODAY()", 46352.0);
    eq("NOW()", 46352.75);
    set_clock(None);
    eq("DATE(2008,1,1)", 39448.0);
    eq("DATE(2026,11,26)", 46352.0);
    eq("DATE(2026,13,1)", 46388.0);
    eq("DATE(2026,1,0)", 46022.0);
    eq("DATE(26,1,1)", 9498.0); // 1926
    assert_eq!(e("DATE(10000,1,1)"), "#NUM!");
    eq("YEAR(46352)", 2026.0);
    eq("MONTH(46352)", 11.0);
    eq("DAY(46352)", 26.0);
    eq("YEAR(\"2026-11-26\")", 2026.0);
    assert_eq!(e("YEAR(-1)"), "#NUM!");
    eq("WEEKDAY(46352)", 5.0); // a Thursday
    eq("WEEKDAY(46352,2)", 4.0);
    eq("WEEKDAY(46352,3)", 3.0);
    eq("WEEKDAY(46352,11)", 4.0);
    eq("WEEKDAY(46352,16)", 6.0); // Saturday = 1
    eq("TIME(12,0,0)", 0.5);
    eq("TIME(25,0,0)", 1.0 / 24.0);
    eq("HOUR(0.75)", 18.0);
    eq("HOUR(\"6:45 PM\")", 18.0);
    eq("MINUTE(TIME(10,45,30))", 45.0);
    eq("SECOND(TIME(10,45,30))", 30.0);
    eq("EDATE(DATE(2026,1,31),1)", 46081.0);
    eq("EDATE(DATE(2026,3,15),-2)", 46037.0);
    eq("EOMONTH(DATE(2026,1,15),1)", 46081.0);
    eq("EOMONTH(DATE(2026,1,15),-1)", 46022.0);
    eq("NETWORKDAYS(DATE(2026,11,2),DATE(2026,11,30))", 21.0);
    eq("NETWORKDAYS(DATE(2026,11,2),DATE(2026,11,30),DATE(2026,11,26))", 20.0);
    eq("NETWORKDAYS(DATE(2026,11,30),DATE(2026,11,2))", -21.0);
    eq("WORKDAY(DATE(2026,11,26),2)", 46356.0);
    eq("WORKDAY(DATE(2026,11,26),2,DATE(2026,11,27))", 46357.0);
    eq("WORKDAY(DATE(2026,11,30),-1)", 46353.0);
    eq("DATEDIF(DATE(2001,1,1),DATE(2003,1,1),\"Y\")", 2.0);
    eq("DATEDIF(DATE(2001,6,1),DATE(2002,8,15),\"D\")", 440.0);
    eq("DATEDIF(DATE(2001,6,1),DATE(2002,8,15),\"YD\")", 75.0);
    eq("DATEDIF(DATE(2001,6,1),DATE(2002,8,15),\"MD\")", 14.0);
    eq("DATEDIF(DATE(2001,6,1),DATE(2002,8,15),\"M\")", 14.0);
    eq("DATEDIF(DATE(2001,6,1),DATE(2002,8,15),\"YM\")", 2.0);
    assert_eq!(e("DATEDIF(DATE(2003,1,1),DATE(2001,1,1),\"Y\")"), "#NUM!");
    eq("DATEVALUE(\"2026-11-26\")", 46352.0);
    eq("DATEVALUE(\"11/26/2026\")", 46352.0);
    eq("DATEVALUE(\"26-Nov-2026\")", 46352.0);
    eq("DATEVALUE(\"Nov 26, 2026\")", 46352.0);
    eq("DATEVALUE(\"2026-11-26 18:00\")", 46352.0);
    assert_eq!(e("DATEVALUE(\"not a date\")"), "#VALUE!");
    assert_eq!(e("DATEVALUE(\"2026-02-30\")"), "#VALUE!");
}

#[test]
fn lookup_and_reference_functions() {
    assert_eq!(s("VLOOKUP(7,F1:G4,2)"), "mid");
    assert_eq!(s("VLOOKUP(10,F1:G4,2,FALSE)"), "high");
    assert_eq!(s("VLOOKUP(10,F1:G4,2,)"), "high"); // an empty fourth argument is FALSE
    assert_eq!(e("VLOOKUP(11,F1:G4,2,FALSE)"), "#N/A");
    assert_eq!(e("VLOOKUP(0,F1:G4,2)"), "#N/A");
    assert_eq!(e("VLOOKUP(5,F1:G4,3,FALSE)"), "#REF!");
    eq("VLOOKUP(\"ch*\",B1:C5,2,FALSE)", 3.0);
    eq("VLOOKUP(\"APPLE\",B1:C5,2,FALSE)", 1.0);
    eq("HLOOKUP(\"b\",{\"a\",\"b\",\"c\";1,2,3},2,FALSE)", 2.0);
    eq("HLOOKUP(2.5,{1,2,3;10,20,30},2)", 20.0);
    eq("XLOOKUP(\"cherry\",B1:B5,A1:A5)", 30.0);
    assert_eq!(s("XLOOKUP(\"zzz\",B1:B5,A1:A5,\"none\")"), "none");
    assert_eq!(e("XLOOKUP(\"zzz\",B1:B5,A1:A5)"), "#N/A");
    eq("XLOOKUP(\"apple\",B1:B5,A1:A5,,0,-1)", 40.0);
    assert_eq!(s("XLOOKUP(7,F1:F4,G1:G4,,-1)"), "mid");
    assert_eq!(s("XLOOKUP(7,F1:F4,G1:G4,,1)"), "high");
    eq("XLOOKUP(\"b*\",B1:B5,C1:C5,,2)", 2.0);
    eq("SUM(XLOOKUP(\"banana\",B1:B5,A1:C5))", 20.0 + 2.0);
    eq("SUM(XLOOKUP({\"apple\",\"date\"},B1:B5,A1:A5))", 60.0);
    eq("INDEX(A1:A5,3)", 30.0);
    assert_eq!(s("INDEX(F1:G4,2,2)"), "mid");
    eq("SUM(INDEX(A1:C5,0,3))", 15.0);
    eq("SUM(INDEX(A1:C5,2,0))", 22.0); // A2 + C2 (B2 text)
    eq("INDEX({1,2;3,4},2,1)", 3.0);
    eq("INDEX(A1:E1,5)", 3.0); // one index into a row picks a column
    assert_eq!(e("INDEX(A1:A5,9)"), "#REF!");
    eq("MATCH(30,A1:A5,0)", 3.0);
    eq("MATCH(35,A1:A5)", 3.0);
    eq("MATCH(35,{50,40,30,20},-1)", 2.0);
    eq("MATCH(\"b?nana\",B1:B5,0)", 2.0);
    assert_eq!(e("MATCH(5,A1:A5)"), "#N/A");
    eq("XMATCH(\"date\",B1:B5)", 5.0);
    eq("XMATCH(25,A1:A5,1)", 3.0);
    eq("XMATCH(25,A1:A5,-1)", 2.0);
    eq("XMATCH(\"apple\",B1:B5,0,-1)", 4.0);
    assert_eq!(s("LOOKUP(4.5,C1:C5,B1:B5)"), "apple");
    eq("LOOKUP(2,1/(H1:H3=\"x\"),I1:I3)", 300.0); // the last-match idiom
    eq("LOOKUP(\"c\",{\"a\",\"b\",\"c\";1,2,3})", 3.0);
    assert_eq!(s("CHOOSE(2,\"a\",\"b\",\"c\")"), "b");
    assert_eq!(e("CHOOSE(4,\"a\")"), "#VALUE!");
    eq("SUM(CHOOSE(2,A1:A2,A3:A4))", 70.0);
    eq("INDIRECT(\"A2\")", 20.0);
    eq("INDIRECT(\"'My Sheet'!B2\")", 8.0);
    eq("INDIRECT(\"R3C1\",FALSE)", 30.0);
    eq("SUM(INDIRECT(\"A1:A3\"))", 60.0);
    assert_eq!(e("INDIRECT(\"not a ref\")"), "#REF!");
    eq("OFFSET(A1,2,0)", 30.0);
    eq("SUM(OFFSET(A1,1,0,3,1))", 90.0);
    eq("ROWS(OFFSET(A1,0,0,4,2))", 4.0);
    assert_eq!(e("OFFSET(A1,-1,0)"), "#REF!");
    eq("ROW(A7)", 7.0);
    eq("ROW()", 100.0);
    eq("COLUMN(C1)", 3.0);
    eq("COLUMN()", 26.0);
    eq("SUM(ROW(A1:A3))", 6.0);
    eq("ROWS(A1:A5)", 5.0);
    eq("COLUMNS(A1:C5)", 3.0);
    eq("ROWS(A:A)", 1_048_576.0);
    eq("COLUMNS({1,2,3})", 3.0);
}

#[test]
fn dynamic_array_functions() {
    eq("INDEX(TRANSPOSE(A1:A5),1,3)", 30.0);
    eq("COLUMNS(TRANSPOSE(A1:A5))", 5.0);
    eq("SUM(FILTER(A1:A5,A1:A5>25))", 120.0);
    eq("ROWS(FILTER(A1:A5,B1:B5=\"apple\"))", 2.0);
    assert_eq!(s("FILTER(A1:A5,A1:A5>100,\"none\")"), "none");
    assert_eq!(e("FILTER(A1:A5,A1:A5>100)"), "#CALC!");
    eq("COLUMNS(FILTER(A1:C1,{TRUE,FALSE,TRUE}))", 2.0);
    eq("INDEX(SORT(E1:E3),1)", 1.0);
    eq("INDEX(SORT(E1:E3,1,-1),1)", 3.0);
    eq("INDEX(SORT(E1:E3,,-1),3)", 1.0); // an empty sort index is the first
    assert_eq!(s("INDEX(SORT(B1:B5),5)"), "date");
    assert_eq!(s("INDEX(SORT(A1:B5,2,1),3,2)"), "banana");
    eq("INDEX(SORT({3,1,2},1,1,TRUE),1,1)", 1.0);
    assert_eq!(s("INDEX(SORTBY(B1:B3,E1:E3),1)"), "banana");
    assert_eq!(s("INDEX(SORTBY(B1:B3,E1:E3,-1),1)"), "apple");
    eq("ROWS(UNIQUE(B1:B5))", 4.0);
    assert_eq!(s("INDEX(UNIQUE(B1:B5,,TRUE),1)"), "banana");
    eq("ROWS(UNIQUE(B1:B5,,TRUE))", 3.0);
    eq("SUM(SEQUENCE(4))", 10.0);
    eq("INDEX(SEQUENCE(2,3,10,5),2,3)", 35.0);
    eq("ROWS(VSTACK(A1:A2,C1:C3))", 5.0);
    assert_eq!(e("INDEX(VSTACK({1,2},{3}),2,2)"), "#N/A");
    eq("SUM(VSTACK({1,2},{3,4}))", 10.0);
}

#[test]
fn finance_functions() {
    near("PMT(0.08/12,10,10000)", -1037.032089, 1e-5);
    near("PMT(0.06/12,18*12,0,50000)", -129.0811609, 1e-6);
    eq("PMT(0,10,1000)", -100.0);
    near("FV(0.06/12,10,-200,-500,1)", 2581.403374, 1e-5);
    near("FV(0.12/12,12,-1000)", 12682.50301, 1e-4);
    near("PV(0.08/12,12*20,500,,0)", -59777.14585, 1e-4);
    near("NPV(0.1,-10000,3000,4200,6800)", 1188.443412, 1e-5);
    near("IRR({-70000,12000,15000,18000,21000,26000})", 0.086630948, 1e-8);
    near("IRR({-70000,12000,15000,18000,21000})", -0.021244848, 1e-8);
    near("IRR({-70000,12000,15000},-0.1)", -0.443506941, 1e-8);
    assert_eq!(e("IRR({1,2,3})"), "#NUM!");
    near("RATE(4*12,-200,8000)", 0.007701472, 1e-9);
    near("NPER(0.12/12,-100,-1000,10000,1)", 59.6738657, 1e-6);
    near("NPER(0.12/12,-100,-1000)", -9.57859404, 1e-7);
    eq("NPER(0,-100,1000)", 10.0);
}

// ----- the graph -----

#[test]
fn a_dependency_chain_recalculates_in_order() {
    // Written bottom-up so the stored order is not the order of work.
    let mut book = vec![sheet("S", &[("A4", "=A3+A2"), ("A3", "=A2*2"), ("A2", "=A1+1"), ("A1", "1")])];
    let changed = recalc(&mut book);
    assert_eq!(changed, 3);
    assert_eq!(get(&book, 0, "A2"), CellValue::Number(2.0));
    assert_eq!(get(&book, 0, "A3"), CellValue::Number(4.0));
    assert_eq!(get(&book, 0, "A4"), CellValue::Number(6.0));
    // Nothing changed: nothing to report.
    assert_eq!(recalc(&mut book), 0);
}

#[test]
fn incremental_recalc_touches_only_dependants() {
    let mut book = vec![sheet("S", &[("A1", "1"), ("A2", "=A1+1"), ("A3", "=A2*2"), ("C1", "3"), ("B1", "=C1*2")])];
    recalc(&mut book);
    // A stale value on a cell the change does not reach shows it is left alone.
    book[0].rows.get_mut(&0).unwrap().cells.get_mut(&1).unwrap().value = CellValue::Number(999.0);
    book[0].rows.get_mut(&0).unwrap().cells.get_mut(&0).unwrap().value = CellValue::Number(5.0);
    let rep = recalc_from_with(&mut book, &[key(0, "A1")], &Options::default());
    assert_eq!(rep.changed, vec![key(0, "A2"), key(0, "A3")]);
    assert_eq!(rep.computed, 2);
    assert_eq!(get(&book, 0, "A3"), CellValue::Number(12.0));
    assert_eq!(get(&book, 0, "B1"), CellValue::Number(999.0));
    // A new formula in a cell: name the cell itself.
    put(&mut book[0], "D1", "=A3+B1");
    assert_eq!(recalc_from(&mut book, &[key(0, "D1")]), 1);
    assert_eq!(get(&book, 0, "D1"), CellValue::Number(1011.0));
    assert_eq!(dependents(&book, &Options::default(), key(0, "A1")), vec![key(0, "D1"), key(0, "A2"), key(0, "A3")]);
    assert_eq!(precedents(&book, &Options::default(), key(0, "D1")), vec![(0, CellRange::cell(0, 1)), (0, CellRange::cell(2, 0))]);
}

#[test]
fn a_circle_is_found_and_keeps_its_values() {
    let mut book = vec![sheet("S", &[("A1", "=B1+1"), ("B1", "=A1+1"), ("C1", "=A1*10"), ("D1", "=SUM(D1:D3)"), ("E1", "2")])];
    book[0].rows.get_mut(&0).unwrap().cells.get_mut(&0).unwrap().value = CellValue::Number(5.0);
    book[0].rows.get_mut(&0).unwrap().cells.get_mut(&1).unwrap().value = CellValue::Number(6.0);
    let rep = recalc_all(&mut book, &Options::default());
    assert_eq!(rep.cycles, vec![vec![key(0, "A1"), key(0, "B1")], vec![key(0, "D1")]]);
    assert_eq!(get(&book, 0, "A1"), CellValue::Number(5.0));
    assert_eq!(get(&book, 0, "B1"), CellValue::Number(6.0));
    assert_eq!(get(&book, 0, "C1"), CellValue::Number(50.0));
    assert_eq!(cycles(&book, &Options::default()).len(), 2);
}

#[test]
fn shared_formulas_expand_and_calculate() {
    let mut sh = sheet("S", &[("A1", "1"), ("A2", "2"), ("A3", "3"), ("B1", "=A1*10+$A$1")]);
    // The children as a file leaves them before expansion: cells with no formula of their own.
    for r in 1..3u32 {
        sh.rows.entry(r).or_default().cells.insert(1, Cell { value: CellValue::Empty, formula: None, style: 0 });
    }
    let text = sh.cell(0, 1).unwrap().formula.clone().unwrap();
    assert_eq!(expand_shared(&mut sh, (0, 1), &text, CellRange::new(0, 1, 2, 1)), 2);
    assert_eq!(sh.cell(2, 1).unwrap().formula.as_deref(), Some("A3*10+$A$1"));
    let mut book = vec![sh];
    recalc(&mut book);
    assert_eq!(get(&book, 0, "B2"), CellValue::Number(21.0));
    assert_eq!(get(&book, 0, "B3"), CellValue::Number(31.0));
}

#[test]
fn sheet_references_and_names() {
    let mut book = vec![
        sheet("Summary", &[("A1", "=SUM(Sales!B1:B3)*TaxRate"), ("A2", "=Double(Sales!B1)"), ("A3", "=Local"), ("A4", "='Q1 Data'!A1+1")]),
        sheet("Sales", &[("B1", "100"), ("B2", "200"), ("B3", "=B1+B2"), ("C1", "=Local")]),
        sheet("Q1 Data", &[("A1", "41")]),
    ];
    let names = vec![
        DefinedName { name: "TaxRate".into(), value: "0.5".into(), local_sheet: None, hidden: false },
        DefinedName { name: "Double".into(), value: "LAMBDA(x,x*2)".into(), local_sheet: None, hidden: false },
        DefinedName { name: "Local".into(), value: "Sales!$B$1".into(), local_sheet: None, hidden: false },
        DefinedName { name: "Local".into(), value: "7".into(), local_sheet: Some(1), hidden: false },
    ];
    let opts = Options { names: &names, date1904: false };
    recalc_all(&mut book, &opts);
    assert_eq!(get(&book, 0, "A1"), CellValue::Number(300.0));
    assert_eq!(get(&book, 0, "A2"), CellValue::Number(200.0));
    assert_eq!(get(&book, 0, "A3"), CellValue::Number(100.0)); // the workbook's Local
    assert_eq!(get(&book, 1, "C1"), CellValue::Number(7.0)); // Sales' own Local
    assert_eq!(get(&book, 0, "A4"), CellValue::Number(42.0));
    // A change on one sheet reaches the other through the name too.
    book[1].rows.get_mut(&0).unwrap().cells.get_mut(&1).unwrap().value = CellValue::Number(1.0);
    let rep = recalc_from_with(&mut book, &[(1, 0, 1)], &opts);
    assert!(rep.changed.contains(&key(0, "A1")) && rep.changed.contains(&key(0, "A3")) && rep.changed.contains(&(1, 2, 1)));
    assert_eq!(get(&book, 0, "A1"), CellValue::Number(201.0));
    assert_eq!(get(&book, 0, "A3"), CellValue::Number(1.0));
}

#[test]
fn unknown_functions_keep_the_files_value() {
    let mut book = vec![sheet("S", &[("A1", "=FUTUREFN(1)"), ("A2", "=FUTUREFN(2)"), ("A3", "=A1+1"), ("A4", "=SUM(")])];
    book[0].rows.get_mut(&0).unwrap().cells.get_mut(&0).unwrap().value = CellValue::Number(42.0);
    book[0].rows.get_mut(&3).unwrap().cells.get_mut(&0).unwrap().value = CellValue::Number(9.0);
    recalc(&mut book);
    assert_eq!(get(&book, 0, "A1"), CellValue::Number(42.0));
    assert_eq!(get(&book, 0, "A2"), CellValue::Error("#NAME?".into()));
    assert_eq!(get(&book, 0, "A3"), CellValue::Number(43.0));
    assert_eq!(get(&book, 0, "A4"), CellValue::Number(9.0)); // unreadable text keeps its value
}

#[test]
fn open_keeps_cached_values() {
    let mut book = vec![sheet("S", &[("A1", "1"), ("A2", "=A1+1"), ("A3", "=A1*5"), ("A4", "=A3+1")])];
    book[0].rows.get_mut(&1).unwrap().cells.get_mut(&0).unwrap().value = CellValue::Number(77.0); // stale but cached
    let rep = recalc_open(&mut book, &Options::default());
    assert_eq!(get(&book, 0, "A2"), CellValue::Number(77.0));
    assert_eq!(get(&book, 0, "A3"), CellValue::Number(5.0));
    assert_eq!(get(&book, 0, "A4"), CellValue::Number(6.0));
    assert_eq!(rep.computed, 2);
    assert_eq!(recalc_open(&mut book, &Options::default()), CalcReport::default());
}

#[test]
fn arrays_spill_into_the_report() {
    let mut book = vec![sheet("S", &[("A1", "=SEQUENCE(3)*2"), ("B1", "=SUM(A1:A3)"), ("C1", "=UNIQUE({1,1,2},TRUE)")])];
    let rep = recalc_all(&mut book, &Options::default());
    assert_eq!(get(&book, 0, "A1"), CellValue::Number(2.0));
    assert_eq!(rep.spills.len(), 2);
    assert_eq!(rep.spills[0], Spill { sheet: 0, row: 0, col: 0, rows: 3, cols: 1, values: vec![CellValue::Number(2.0), CellValue::Number(4.0), CellValue::Number(6.0)] });
    assert_eq!(rep.spills[1].cols, 2);
}

#[test]
fn volatile_and_dynamic_references() {
    assert!(is_volatile("NOW()+1"));
    assert!(is_volatile("SUM(OFFSET(A1,1,0))"));
    assert!(!is_volatile("SUM(A1:A3)"));
    set_clock(Some(46352.5));
    let mut book = vec![sheet("S", &[("A1", "=NOW()"), ("A2", "=A1+1"), ("B1", "=INDIRECT(\"C1\")*2"), ("C1", "=D1+1"), ("D1", "4")])];
    recalc(&mut book);
    assert_eq!(get(&book, 0, "B1"), CellValue::Number(10.0)); // C1 worked out on demand
    set_clock(Some(46353.5));
    // No cell changed, yet NOW and what reads it move.
    let rep = recalc_from_with(&mut book, &[], &Options::default());
    set_clock(None);
    assert!(rep.changed.contains(&key(0, "A1")) && rep.changed.contains(&key(0, "A2")));
    assert_eq!(get(&book, 0, "A2"), CellValue::Number(46354.5));
}

#[test]
fn the_1904_system_moves_dates() {
    let book = data();
    let opts = Options { names: &[], date1904: true };
    assert_eq!(evaluate(&book, &opts, 0, 0, 25, "DATE(1904,1,2)"), CellValue::Number(1.0));
    assert_eq!(evaluate(&book, &opts, 0, 0, 25, "YEAR(0)"), CellValue::Number(1904.0));
    assert_eq!(evaluate(&book, &opts, 0, 0, 25, "DATE(2026,11,26)"), CellValue::Number(46352.0 - 1462.0));
}

#[test]
fn a_long_chain_and_whole_column_sums_stay_cheap() {
    let mut sh = sheet("S", &[("A1", "1"), ("B1", "=SUM(A:A)"), ("C1", "=COUNTIF(A:A,\">2500\")")]);
    for r in 2..=5000u32 {
        put(&mut sh, &format!("A{r}"), &format!("=A{}+1", r - 1));
    }
    let mut book = vec![sh];
    recalc(&mut book);
    assert_eq!(get(&book, 0, "A5000"), CellValue::Number(5000.0));
    assert_eq!(get(&book, 0, "B1"), CellValue::Number(5000.0 * 5001.0 / 2.0));
    assert_eq!(get(&book, 0, "C1"), CellValue::Number(2500.0));
    book[0].rows.get_mut(&0).unwrap().cells.get_mut(&0).unwrap().value = CellValue::Number(2.0);
    let rep = recalc_from_with(&mut book, &[key(0, "A1")], &Options::default());
    assert_eq!(rep.computed, 5001); // A2..A5000, B1 and C1
    assert_eq!(get(&book, 0, "C1"), CellValue::Number(2501.0));
    assert_eq!(get(&book, 0, "A5000"), CellValue::Number(5001.0));
}
