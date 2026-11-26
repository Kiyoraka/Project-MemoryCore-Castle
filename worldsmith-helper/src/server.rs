//! What `serve` does with each request: the attached game, the labels and their candidates.

use crate::freeze::{Attached, Frozen, Shared};
use crate::proto::{self, err, fmt_addr, item, ok, ok_with, Request};
use crate::scan::{
    self, CandList, NextRule, SnapRegion, Target, Transform, ValueType, CANDIDATE_CAP,
    SNAPSHOT_CAP_BYTES,
};
use crate::{capture, memory::Proc, process};
use serde_json::{json, Value};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

/// At most this many matches get a `top` list.
const TOP_MAX: usize = 20;

enum Cands {
    List(CandList),
    /// An unknown-value scan that still has too many candidates to list (see scan.rs).
    Snap { ty: ValueType, regions: Vec<SnapRegion> },
}

impl Cands {
    fn count(&self) -> usize {
        match self {
            Cands::List(l) => l.len(),
            Cands::Snap { regions, .. } => regions.iter().map(|r| r.alive).sum(),
        }
    }

    /// Turn a snapshot into a list once a list is the smaller of the two.
    fn settle(self) -> Cands {
        match self {
            Cands::Snap { ty, regions } => {
                let count: usize = regions.iter().map(|r| r.alive).sum();
                let bytes: usize = regions.iter().map(|r| r.bytes.len()).sum();
                if count <= CANDIDATE_CAP && count * 17 <= bytes {
                    let mut l = CandList::default();
                    for r in &regions {
                        r.push_alive(ty, &mut l);
                    }
                    Cands::List(l)
                } else {
                    Cands::Snap { ty, regions: regions.into_iter().filter(|r| r.alive > 0).collect() }
                }
            }
            list => list,
        }
    }

    /// The first `n` candidates (address, type).
    fn first(&self, n: usize) -> Vec<(u64, ValueType)> {
        match self {
            Cands::List(l) => l.addrs.iter().copied().zip(l.types.iter().copied()).take(n).collect(),
            Cands::Snap { ty, regions } => {
                let mut out = Vec::new();
                'outer: for r in regions {
                    for i in 0..scan::slot_count(r.bytes.len(), ty.size()) {
                        if out.len() >= n {
                            break 'outer;
                        }
                        if r.is_alive(i) {
                            out.push((r.base + (i * scan::ALIGN) as u64, *ty));
                        }
                    }
                }
                out
            }
        }
    }
}

struct Label {
    name: String,
    transform: Transform,
    cands: Cands,
    steps: Vec<usize>,
}

pub struct Server {
    shared: Arc<Shared>,
    labels: Vec<Label>,
    labels_pid: Option<u32>,
}

fn not_attached() -> Value {
    err("No game is attached - attach one first.")
}

impl Server {
    pub fn new(shared: Arc<Shared>) -> Server {
        Server { shared, labels: Vec::new(), labels_pid: None }
    }

    /// One request line -> one reply line.
    pub fn handle_line(&mut self, line: &str) -> String {
        let reply = match serde_json::from_str::<Request>(line) {
            Err(_) => err("That request isn't one Worldsmith understands."),
            Ok(req) => match std::panic::catch_unwind(AssertUnwindSafe(|| self.handle(req))) {
                Ok(v) => v,
                Err(_) => err("Worldsmith hit an internal error on that request."),
            },
        };
        reply.to_string()
    }

    fn target(&mut self) -> Option<Arc<Attached>> {
        let t = self.shared.target();
        let pid = t.as_ref().map(|t| t.proc.pid);
        if pid != self.labels_pid {
            // the game closed or changed: its candidates mean nothing now
            self.labels.clear();
            self.labels_pid = pid;
        }
        t
    }

    fn handle(&mut self, req: Request) -> Value {
        let t = self.target();
        match req {
            Request::Games => games(),
            Request::Attach { pid } => self.attach(pid),
            Request::Detach => {
                self.shared.detach();
                self.labels.clear();
                self.labels_pid = None;
                ok()
            }
            Request::State => self.state(t),
            req => {
                let Some(t) = t else { return not_attached() };
                match req {
                    Request::Shot => shot(&t),
                    Request::Scan { label, ty, value, transform } => {
                        self.scan(&t, label, ty, value, transform)
                    }
                    Request::Next { label, rule } => self.next(&t, &label, rule),
                    Request::List { label, max } => self.list(&t, &label, max),
                    Request::Write { address, ty, value } => write(&t, address, ty, &value),
                    Request::Freeze { address, ty, value } => {
                        let bytes = match scan::encode(ty, &value) {
                            Ok(b) => b,
                            Err(e) => return err(&e),
                        };
                        if !t.proc.write(address, &bytes) {
                            return err(&format!("Couldn't write at {} - that address isn't writable now.", fmt_addr(address)));
                        }
                        self.shared.freeze(address, Frozen { ty, bytes });
                        ok()
                    }
                    Request::Unfreeze { address } => {
                        self.shared.unfreeze(address);
                        ok()
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    fn attach(&mut self, pid: u32) -> Value {
        if pid == std::process::id() {
            return err("Worldsmith can't attach to itself.");
        }
        let Some(path) = process::exe_path(pid) else {
            return err(&format!("There's no process {pid} that Worldsmith can open."));
        };
        if let Some(reason) = process::refusal(pid, &process::tcp_remotes()) {
            return err(&reason);
        }
        let proc = match Proc::open(pid) {
            Ok(p) => p,
            Err(e) => return err(&e),
        };
        let exe = process::file_name(&path);
        let title = process::main_window(pid).map(|w| w.title).unwrap_or_default();
        self.shared.set_target(Attached { proc, exe: exe.clone(), title: title.clone() });
        self.labels.clear();
        self.labels_pid = Some(pid);
        ok_with(json!({ "pid": pid, "exe": exe, "title": title }))
    }

    fn label_index(&self, name: &str) -> Option<usize> {
        self.labels.iter().position(|l| l.name.eq_ignore_ascii_case(name))
    }

    fn scan(
        &mut self,
        t: &Attached,
        label: String,
        ty: scan::ScanType,
        value: Option<String>,
        transform: Transform,
    ) -> Value {
        let mut capped = false;
        let cands = match value {
            Some(v) => {
                let target = match Target::new(&v, transform) {
                    Ok(x) => x,
                    Err(e) => return err(&e),
                };
                let types = ty.types();
                let mut list = CandList::default();
                t.proc.for_each_chunk(|buf, base, limit| {
                    let more = scan::scan_buffer(buf, base, limit, &types, &target, &mut list, CANDIDATE_CAP);
                    capped |= !more;
                    more
                });
                Cands::List(list)
            }
            None => {
                let vt = match ty.types().as_slice() {
                    [one] => *one,
                    _ => return err("An unknown-value scan needs one type: i32, i64, f32 or f64."),
                };
                let mut regions = Vec::new();
                let mut total = 0usize;
                for (base, len) in t.proc.regions() {
                    if total + len > SNAPSHOT_CAP_BYTES {
                        capped = true;
                        continue;
                    }
                    let (bytes, ok_pages) = t.proc.read_region(base, len);
                    let r = SnapRegion::new(base, bytes, &ok_pages, vt);
                    if r.alive > 0 {
                        total += len;
                        regions.push(r);
                    }
                }
                Cands::Snap { ty: vt, regions }.settle()
            }
        };
        let count = cands.count();
        let lbl = Label { name: label.clone(), transform, cands, steps: vec![count] };
        match self.label_index(&label) {
            Some(i) => self.labels[i] = lbl,
            None => self.labels.push(lbl),
        }
        let mut reply = ok_with(json!({ "label": label, "matches": count, "steps": [count] }));
        if capped {
            reply["capped"] = json!(true);
        }
        if count <= TOP_MAX {
            let i = self.label_index(&label).unwrap();
            reply["top"] = top(t, &self.labels[i].cands, TOP_MAX);
        }
        reply
    }

    fn next(&mut self, t: &Attached, label: &str, rule: NextRule) -> Value {
        let Some(i) = self.label_index(label) else {
            return err(&format!("There's no scan called {label} yet - start one with scan."));
        };
        let lbl = &mut self.labels[i];
        let target = match &rule {
            NextRule::Value(v) => match Target::new(v, lbl.transform) {
                Ok(x) => Some(x),
                Err(e) => return err(&e),
            },
            _ => None,
        };
        let old = std::mem::replace(&mut lbl.cands, Cands::List(CandList::default()));
        lbl.cands = match old {
            Cands::List(list) => {
                Cands::List(scan::narrow(&list, &rule, target.as_ref(), |a, b| t.proc.read(a, b)))
            }
            Cands::Snap { ty, mut regions } => {
                for r in regions.iter_mut() {
                    let (bytes, ok_pages) = t.proc.read_region(r.base, r.bytes.len());
                    r.narrow(bytes, &ok_pages, ty, &rule, target.as_ref());
                }
                Cands::Snap { ty, regions }.settle()
            }
        };
        let count = lbl.cands.count();
        lbl.steps.push(count);
        let mut reply = ok_with(json!({ "label": lbl.name, "matches": count, "steps": lbl.steps }));
        if count <= TOP_MAX {
            reply["top"] = top(t, &lbl.cands, TOP_MAX);
        }
        reply
    }

    fn list(&mut self, t: &Attached, label: &str, max: usize) -> Value {
        let Some(i) = self.label_index(label) else {
            return err(&format!("There's no scan called {label} yet - start one with scan."));
        };
        let lbl = &self.labels[i];
        ok_with(json!({ "label": lbl.name, "items": top(t, &lbl.cands, max) }))
    }

    fn state(&mut self, t: Option<Arc<Attached>>) -> Value {
        let attached = match &t {
            Some(a) => json!({ "pid": a.proc.pid, "exe": a.exe, "title": a.title }),
            None => Value::Null,
        };
        let labels: Vec<Value> = self
            .labels
            .iter()
            .map(|l| {
                let top = match &t {
                    Some(a) if l.cands.count() <= TOP_MAX => top(a, &l.cands, TOP_MAX),
                    _ => json!([]),
                };
                json!({ "label": l.name, "steps": l.steps, "top": top })
            })
            .collect();
        let freezes: Vec<Value> = self
            .shared
            .freezes()
            .into_iter()
            .map(|(a, f)| item(a, f.ty, &f.bytes))
            .collect();
        ok_with(json!({ "attached": attached, "labels": labels, "freezes": freezes }))
    }
}

/// Live values of the first `n` candidates (unreadable ones are left out).
fn top(t: &Attached, cands: &Cands, n: usize) -> Value {
    let mut buf = [0u8; 8];
    let items: Vec<Value> = cands
        .first(n)
        .into_iter()
        .filter_map(|(a, ty)| t.proc.read(a, &mut buf[..ty.size()]).then(|| item(a, ty, &buf)))
        .collect();
    Value::Array(items)
}

fn games() -> Value {
    let list: Vec<Value> = process::games()
        .into_iter()
        .map(|g| {
            json!({
                "pid": g.pid,
                "exe": g.exe,
                "title": g.title,
                "stamp": g.stamp,
                "refused": g.refused.is_some(),
                "reason": g.refused.map(|r| proto::one_line(&r)).unwrap_or_default(),
            })
        })
        .collect();
    ok_with(json!({ "games": list }))
}

fn shot(t: &Attached) -> Value {
    let Some(w) = process::main_window(t.proc.pid) else {
        return err("The game has no window to capture.");
    };
    match capture::shot(w.hwnd) {
        Ok((path, via)) => ok_with(json!({ "path": path, "via": via })),
        Err(e) => err(&e),
    }
}

fn write(t: &Attached, address: u64, ty: ValueType, value: &str) -> Value {
    let bytes = match scan::encode(ty, value) {
        Ok(b) => b,
        Err(e) => return err(&e),
    };
    if t.proc.write(address, &bytes) {
        ok()
    } else {
        err(&format!("Couldn't write at {} - that address isn't writable now.", fmt_addr(address)))
    }
}
