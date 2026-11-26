//! Ledger's calculation engine: formulas worked out by our own evaluator. The calc lane owns this
//! folder; the rest of Ledger calls [`recalc`] / [`recalc_from`] after a change.
//!
//! - `parse`: formula text to a tree (references, ranges, whole rows and columns, sheet
//!   references, names, functions, array constants, the operators with their precedence).
//! - `value`: values and the coercion rules (text numbers and dates, booleans, blanks), the
//!   comparison order, 15-digit equality, the 1900 date system with its 29 February 1900.
//! - `eval`: the evaluator (arrays broadcast through operators and scalar functions, implicit
//!   intersection, LET/LAMBDA scopes).
//! - `fns`: the worksheet functions (the catalogue's first 120 and a few old names).
//! - here: the dependency graph from every formula cell, the order of work (Tarjan's strongly
//!   connected components, precedents first), circles found and left with their cached values,
//!   incremental recalculation of only the dependants of the changed cells.
//!
//! Shared formulas: the reader already writes the master's text, shifted, into every child cell
//! (`Cell::formula`), so the engine sees one formula per cell; [`expand_shared`] does the same
//! for a sheet built another way.
//!
//! The cached values of a file are kept on open: nothing here runs by itself. [`recalc_open`]
//! works out only formula cells without a cached value; [`recalc`] / [`recalc_all`] everything.
//! A formula using a function or name the engine does not have keeps its cached value (when it
//! has one), so a file is never made worse by being recalculated.

mod eval;
mod fns;
mod parse;
mod value;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::rc::Rc;

use super::cellref::CellRange;
use super::formula::shift_formula;
use super::model::{CellValue, DefinedName, Sheet};

pub use eval::Key as CellKey;
pub use fns::{now_serial, set_clock, set_seed, FUNCTIONS, VOLATILE};
pub use parse::{parse_formula, BinOp, Expr};
pub use value::{num_to_text, parse_datetime_text, parse_number_text, ErrKind};

/// What the engine needs from the workbook beyond its sheets.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options<'a> {
    /// The workbook's defined names (`TaxRate`, a named LAMBDA, ...).
    pub names: &'a [DefinedName],
    /// The 1904 date system.
    pub date1904: bool,
}

/// An array result: the formula cell at (`sheet`, `row`, `col`) shows `values[0]`, and the
/// whole `rows` x `cols` block (row by row) would spill from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Spill {
    pub sheet: usize,
    pub row: u32,
    pub col: u32,
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<CellValue>,
}

/// What a recalculation did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CalcReport {
    /// Formula cells whose value changed, in (sheet, row, col) order.
    pub changed: Vec<CellKey>,
    /// Formula cells worked out.
    pub computed: usize,
    /// Circular chains (each a list of cells); their cells keep the values they had.
    pub cycles: Vec<Vec<CellKey>>,
    /// Array results of the cells worked out.
    pub spills: Vec<Spill>,
}

struct Node {
    key: CellKey,
    expr: Option<Rc<Expr>>,
    deps: Vec<(usize, CellRange)>,
    volatile: bool,
}

struct Graph {
    nodes: Vec<Node>,
    index: HashMap<CellKey, usize>,
    /// Node -> the formula nodes it reads.
    edges: Vec<Vec<usize>>,
}

/// Collects the references of a formula (names followed into their own references).
fn collect(e: &Expr, cur: usize, sheet_ix: &HashMap<String, usize>, names: &[DefinedName], deps: &mut Vec<(usize, CellRange)>, volatile: &mut bool, depth: u32) {
    let sheet_of = |s: &Option<String>| -> Option<usize> {
        match s {
            None => Some(cur),
            Some(n) => sheet_ix.get(&n.to_lowercase()).copied(),
        }
    };
    match e {
        Expr::Ref { sheet, range } => {
            if let Some(s) = sheet_of(sheet) {
                deps.push((s, *range));
            }
        }
        Expr::Name { sheet, name } => {
            if depth > 16 {
                return;
            }
            let scope = match sheet {
                Some(n) => sheet_ix.get(&n.to_lowercase()).copied(),
                None => Some(cur),
            };
            let d = names.iter().filter(|d| d.name.eq_ignore_ascii_case(name)).find(|d| d.local_sheet == scope).or_else(|| names.iter().filter(|d| d.name.eq_ignore_ascii_case(name)).find(|d| d.local_sheet.is_none()));
            if let Some(d) = d {
                if let Ok(x) = parse_formula(&d.value) {
                    collect(&x, cur, sheet_ix, names, deps, volatile, depth + 1);
                }
            }
        }
        Expr::Array(rows) => rows.iter().flatten().for_each(|x| collect(x, cur, sheet_ix, names, deps, volatile, depth)),
        Expr::Neg(x) | Expr::Plus(x) | Expr::Percent(x) | Expr::At(x) => collect(x, cur, sheet_ix, names, deps, volatile, depth),
        Expr::Bin(_, l, r) | Expr::Range(l, r) | Expr::Isect(l, r) => {
            collect(l, cur, sheet_ix, names, deps, volatile, depth);
            collect(r, cur, sheet_ix, names, deps, volatile, depth);
        }
        Expr::Call(name, args) => {
            if VOLATILE.contains(&name.as_str()) {
                *volatile = true;
            }
            args.iter().for_each(|x| collect(x, cur, sheet_ix, names, deps, volatile, depth));
        }
        Expr::CallExpr(c, args) => {
            collect(c, cur, sheet_ix, names, deps, volatile, depth);
            args.iter().for_each(|x| collect(x, cur, sheet_ix, names, deps, volatile, depth));
        }
        _ => {}
    }
}

fn build(sheets: &[Sheet], opts: &Options) -> Graph {
    let sheet_ix: HashMap<String, usize> = sheets.iter().enumerate().map(|(i, s)| (s.name.to_lowercase(), i)).collect();
    let mut nodes = Vec::new();
    let mut index = HashMap::new();
    for (si, sh) in sheets.iter().enumerate() {
        for (&r, row) in &sh.rows {
            for (&c, cell) in &row.cells {
                let Some(text) = &cell.formula else { continue };
                let expr = parse_formula(text).ok().map(Rc::new);
                let mut deps = Vec::new();
                let mut volatile = false;
                if let Some(e) = &expr {
                    collect(e, si, &sheet_ix, opts.names, &mut deps, &mut volatile, 0);
                }
                deps.sort_by_key(|(s, g)| (*s, g.r1, g.c1, g.r2, g.c2));
                deps.dedup();
                index.insert((si, r, c), nodes.len());
                nodes.push(Node { key: (si, r, c), expr, deps, volatile });
            }
        }
    }
    // Formula cells by sheet and row, to find those inside a referenced rectangle.
    let mut by_sheet: Vec<BTreeMap<u32, Vec<(u32, usize)>>> = vec![BTreeMap::new(); sheets.len()];
    for (i, n) in nodes.iter().enumerate() {
        by_sheet[n.key.0].entry(n.key.1).or_default().push((n.key.2, i));
    }
    let mut cache: HashMap<(usize, CellRange), Vec<usize>> = HashMap::new();
    let mut edges = Vec::with_capacity(nodes.len());
    for n in &nodes {
        let mut out = Vec::new();
        for dep in &n.deps {
            let hits = cache.entry(*dep).or_insert_with(|| {
                let (s, g) = *dep;
                let mut v = Vec::new();
                if let Some(rows) = by_sheet.get(s) {
                    for (_, cols) in rows.range(g.r1..=g.r2) {
                        v.extend(cols.iter().filter(|(c, _)| *c >= g.c1 && *c <= g.c2).map(|(_, i)| *i));
                    }
                }
                v
            });
            out.extend_from_slice(hits);
        }
        out.sort_unstable();
        out.dedup();
        edges.push(out);
    }
    Graph { nodes, index, edges }
}

/// Strongly connected components, each emitted after every component it reads (precedents first).
fn tarjan(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = edges.len();
    const UNSEEN: usize = usize::MAX;
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut out = Vec::new();
    let mut counter = 0;
    for s in 0..n {
        if index[s] != UNSEEN {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(s, 0)];
        index[s] = counter;
        low[s] = counter;
        counter += 1;
        stack.push(s);
        on_stack[s] = true;
        while let Some(&(v, i)) = call.last() {
            if i < edges[v].len() {
                if let Some(top) = call.last_mut() {
                    top.1 += 1;
                }
                let w = edges[v][i];
                if index[w] == UNSEEN {
                    index[w] = counter;
                    low[w] = counter;
                    counter += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                call.pop();
                if let Some(&(p, _)) = call.last() {
                    low[p] = low[p].min(low[v]);
                }
                if low[v] == index[v] {
                    let mut comp = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    comp.reverse();
                    out.push(comp);
                }
            }
        }
    }
    out
}

fn is_cycle(g: &Graph, comp: &[usize]) -> bool {
    comp.len() > 1 || g.edges[comp[0]].contains(&comp[0])
}

/// Works out the `dirty` nodes (all when `None`) in dependency order and writes their values.
fn run(sheets: &mut [Sheet], opts: &Options, g: &Graph, dirty: Option<&HashSet<usize>>) -> CalcReport {
    let comps = tarjan(&g.edges);
    let mut report = CalcReport::default();
    let mut in_cycle = HashSet::new();
    for comp in &comps {
        if is_cycle(g, comp) {
            let mut keys: Vec<CellKey> = comp.iter().map(|&i| g.nodes[i].key).collect();
            keys.sort_unstable();
            report.cycles.push(keys);
            in_cycle.extend(comp.iter().copied());
        }
    }
    let wanted = |i: usize| !in_cycle.contains(&i) && dirty.map_or(true, |d| d.contains(&i));
    let pending: HashMap<CellKey, Rc<Expr>> = (0..g.nodes.len()).filter(|&i| wanted(i)).filter_map(|i| g.nodes[i].expr.clone().map(|e| (g.nodes[i].key, e))).collect();
    let (results, spills) = {
        let ctx = eval::Ctx::new(&*sheets, opts.names, opts.date1904, pending);
        for comp in &comps {
            for &i in comp {
                if !wanted(i) {
                    continue;
                }
                let node = &g.nodes[i];
                if ctx.results.borrow().contains_key(&node.key) {
                    continue;
                }
                match &node.expr {
                    Some(e) => ctx.compute(node.key, e),
                    None => {
                        // Text we cannot read: the cached value stays when there is one.
                        let old = sheets[node.key.0].cell(node.key.1, node.key.2).map(|c| c.value.clone()).unwrap_or(CellValue::Empty);
                        let v = if old.is_empty() { CellValue::Error(ErrKind::Name.text().into()) } else { old };
                        ctx.results.borrow_mut().insert(node.key, v);
                    }
                }
            }
        }
        (ctx.results.into_inner(), ctx.spills.into_inner())
    };
    report.computed = results.len();
    let mut results: Vec<(CellKey, CellValue)> = results.into_iter().collect();
    results.sort_by_key(|(k, _)| *k);
    for (k, v) in results {
        let Some(cell) = sheets.get_mut(k.0).and_then(|s| s.rows.get_mut(&k.1)).and_then(|r| r.cells.get_mut(&k.2)) else { continue };
        if cell.formula.is_none() {
            continue;
        }
        if cell.value != v {
            cell.value = v;
            report.changed.push(k);
        }
    }
    let mut spills: Vec<Spill> = spills
        .into_iter()
        .map(|(k, a)| Spill { sheet: k.0, row: k.1, col: k.2, rows: a.rows, cols: a.cols, values: a.data.iter().map(value::value_to_cell).collect() })
        .collect();
    spills.sort_by_key(|s| (s.sheet, s.row, s.col));
    report.spills = spills;
    report
}

/// The dirty set: the seeds, every volatile formula, and everything that reads them (transitively).
fn dependants(g: &Graph, seeds: impl IntoIterator<Item = usize>) -> HashSet<usize> {
    let mut rev: Vec<Vec<usize>> = vec![Vec::new(); g.nodes.len()];
    for (i, es) in g.edges.iter().enumerate() {
        for &p in es {
            rev[p].push(i);
        }
    }
    let mut dirty = HashSet::new();
    let mut queue: VecDeque<usize> = seeds.into_iter().collect();
    queue.extend(g.nodes.iter().enumerate().filter(|(_, n)| n.volatile).map(|(i, _)| i));
    while let Some(i) = queue.pop_front() {
        if dirty.insert(i) {
            queue.extend(rev[i].iter().copied());
        }
    }
    dirty
}

/// Works out every formula in `sheets` again and writes the results into the cells (no defined
/// names, 1900 dates). Answers how many cells changed value.
pub fn recalc(sheets: &mut [Sheet]) -> usize {
    recalc_all(sheets, &Options::default()).changed.len()
}

/// Works out every formula, with the workbook's names and date system.
pub fn recalc_all(sheets: &mut [Sheet], opts: &Options) -> CalcReport {
    let g = build(sheets, opts);
    run(sheets, opts, &g, None)
}

/// Works out only what depends on the changed cells (and volatile formulas): call it after an
/// edit with every cell whose value or formula changed. Answers how many cells changed value.
pub fn recalc_from(sheets: &mut [Sheet], changed: &[CellKey]) -> usize {
    recalc_from_with(sheets, changed, &Options::default()).changed.len()
}

/// [`recalc_from`] with the workbook's names and date system, and the full report.
pub fn recalc_from_with(sheets: &mut [Sheet], changed: &[CellKey], opts: &Options) -> CalcReport {
    let g = build(sheets, opts);
    let mut seeds = Vec::new();
    for &k in changed {
        if let Some(&i) = g.index.get(&k) {
            seeds.push(i);
        }
    }
    for (i, n) in g.nodes.iter().enumerate() {
        if n.deps.iter().any(|(s, r)| changed.iter().any(|k| k.0 == *s && r.contains(k.1, k.2))) {
            seeds.push(i);
        }
    }
    let dirty = dependants(&g, seeds);
    run(sheets, opts, &g, Some(&dirty))
}

/// On open: keeps every cached value and works out only formula cells that have none (and what
/// reads them).
pub fn recalc_open(sheets: &mut [Sheet], opts: &Options) -> CalcReport {
    let g = build(sheets, opts);
    let seeds: Vec<usize> = g
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| sheets[n.key.0].cell(n.key.1, n.key.2).is_some_and(|c| c.value.is_empty()))
        .map(|(i, _)| i)
        .collect();
    if seeds.is_empty() {
        return CalcReport::default();
    }
    let mut rev: Vec<Vec<usize>> = vec![Vec::new(); g.nodes.len()];
    for (i, es) in g.edges.iter().enumerate() {
        for &p in es {
            rev[p].push(i);
        }
    }
    let mut dirty = HashSet::new();
    let mut queue: VecDeque<usize> = seeds.into_iter().collect();
    while let Some(i) = queue.pop_front() {
        if dirty.insert(i) {
            queue.extend(rev[i].iter().copied());
        }
    }
    run(sheets, opts, &g, Some(&dirty))
}

/// Evaluates formula text as if it stood in (`sheet`, `row`, `col`), against the current cell
/// values (for a formula bar preview or the companion's "what would this give"). Nothing is
/// written.
pub fn evaluate(sheets: &[Sheet], opts: &Options, sheet: usize, row: u32, col: u32, formula: &str) -> CellValue {
    let Ok(e) = parse_formula(formula) else { return CellValue::Error(ErrKind::Name.text().into()) };
    let ctx = eval::Ctx::new(sheets, opts.names, opts.date1904, HashMap::new());
    let f = eval::Frame::at(sheet, row, col);
    let v = ctx.eval_top(&e, &f);
    ctx.finish(v, &f).0
}

/// The circular chains in the workbook (each a sorted list of cells).
pub fn cycles(sheets: &[Sheet], opts: &Options) -> Vec<Vec<CellKey>> {
    let g = build(sheets, opts);
    tarjan(&g.edges)
        .into_iter()
        .filter(|c| is_cycle(&g, c))
        .map(|c| {
            let mut k: Vec<CellKey> = c.iter().map(|&i| g.nodes[i].key).collect();
            k.sort_unstable();
            k
        })
        .collect()
}

/// The rectangles a formula cell reads (names followed); empty for a plain cell.
pub fn precedents(sheets: &[Sheet], opts: &Options, key: CellKey) -> Vec<(usize, CellRange)> {
    let Some(text) = sheets.get(key.0).and_then(|s| s.cell(key.1, key.2)).and_then(|c| c.formula.as_deref()) else { return Vec::new() };
    let Ok(e) = parse_formula(text) else { return Vec::new() };
    let sheet_ix: HashMap<String, usize> = sheets.iter().enumerate().map(|(i, s)| (s.name.to_lowercase(), i)).collect();
    let mut deps = Vec::new();
    let mut vol = false;
    collect(&e, key.0, &sheet_ix, opts.names, &mut deps, &mut vol, 0);
    deps.sort_by_key(|(s, g)| (*s, g.r1, g.c1, g.r2, g.c2));
    deps.dedup();
    deps
}

/// The formula cells that read a cell, directly or through others (sorted).
pub fn dependents(sheets: &[Sheet], opts: &Options, key: CellKey) -> Vec<CellKey> {
    let g = build(sheets, opts);
    let mut rev: Vec<Vec<usize>> = vec![Vec::new(); g.nodes.len()];
    for (i, es) in g.edges.iter().enumerate() {
        for &p in es {
            rev[p].push(i);
        }
    }
    let mut seen = HashSet::new();
    let mut queue: VecDeque<usize> = g.nodes.iter().enumerate().filter(|(_, n)| n.deps.iter().any(|(s, r)| *s == key.0 && r.contains(key.1, key.2))).map(|(i, _)| i).collect();
    while let Some(i) = queue.pop_front() {
        if seen.insert(i) {
            queue.extend(rev[i].iter().copied());
        }
    }
    let mut out: Vec<CellKey> = seen.into_iter().map(|i| g.nodes[i].key).collect();
    out.sort_unstable();
    out
}

/// True when formula text uses a volatile function (TODAY, NOW, RAND, RANDBETWEEN, INDIRECT,
/// OFFSET).
pub fn is_volatile(formula: &str) -> bool {
    let Ok(e) = parse_formula(formula) else { return false };
    let mut deps = Vec::new();
    let mut vol = false;
    collect(&e, 0, &HashMap::new(), &[], &mut deps, &mut vol, 0);
    vol
}

/// Writes a shared formula's master text into the cells of `range` that hold no formula of their
/// own, shifted to each cell (the reader already does this; for sheets built another way).
/// Answers how many cells received a formula.
pub fn expand_shared(sheet: &mut Sheet, master: (u32, u32), text: &str, range: CellRange) -> usize {
    let mut n = 0;
    for (&r, row) in sheet.rows.range_mut(range.r1..=range.r2) {
        for (&c, cell) in row.cells.range_mut(range.c1..=range.c2) {
            if cell.formula.is_none() {
                cell.formula = Some(shift_formula(text, r as i64 - master.0 as i64, c as i64 - master.1 as i64));
                n += 1;
            }
        }
    }
    n
}
