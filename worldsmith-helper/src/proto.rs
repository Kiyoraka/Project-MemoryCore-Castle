//! The contract: argv -> Request, the JSON line on the pipe, reply builders, addresses and the
//! pipe name. Pure code, tested on any OS.

use crate::scan::{self, NextRule, ScanType, Transform, ValueType};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const NOT_RUNNING: &str = "Worldsmith isn't running - open the WORLDSMITH room first.";
#[cfg_attr(windows, allow(dead_code))]
pub const WINDOWS_ONLY: &str = "Worldsmith works on Windows only.";

/// The helper's version, as `caps` reports it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Every client verb `parse_args` knows (`serve` is not one: it starts the server).
pub const VERBS: &[&str] = &[
    "games", "attach", "detach", "shot", "thumb", "scan", "next", "list", "write", "freeze",
    "unfreeze", "state", "caps",
];

/// What this helper can do beyond the first contract, as `caps` reports it.
pub const FEATURES: &[&str] = &["rect", "thumb", "unknown-scan", "stamp"];

/// One request, as sent over the pipe (one JSON line).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "lowercase")]
pub enum Request {
    Games,
    Attach { pid: u32 },
    Detach,
    Shot,
    Scan {
        label: String,
        #[serde(rename = "type")]
        ty: ScanType,
        /// None for an unknown-value scan (`--unknown`).
        value: Option<String>,
        transform: Transform,
    },
    Next { label: String, rule: NextRule },
    List { label: String, max: usize },
    Write {
        address: u64,
        #[serde(rename = "type")]
        ty: ValueType,
        value: String,
    },
    Freeze {
        address: u64,
        #[serde(rename = "type")]
        ty: ValueType,
        value: String,
    },
    Unfreeze { address: u64 },
    State,
    Caps,
    Thumb,
}

/// `0x` + uppercase hex.
pub fn fmt_addr(a: u64) -> String {
    format!("0x{a:X}")
}

/// Accepts `0x1A2F40`, `0X1a2f40` or bare hex `1A2F40`.
pub fn parse_addr(s: &str) -> Result<u64, String> {
    let t = s.trim();
    let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    if hex.is_empty() || hex.len() > 16 {
        return Err(format!("\"{t}\" isn't an address (use 0x and hex, like 0x1A2F40)."));
    }
    let a = u64::from_str_radix(hex, 16)
        .map_err(|_| format!("\"{t}\" isn't an address (use 0x and hex, like 0x1A2F40)."))?;
    if a == 0 {
        return Err("Address 0x0 can't be used.".into());
    }
    Ok(a)
}

/// The per-user pipe: `\\.\pipe\worldsmith-<USERNAME>`, or `WORLDSMITH_PIPE` when set (a full
/// `\\.\pipe\...` name, or a bare name that gets the prefix).
pub fn pipe_name(user: Option<&str>, override_name: Option<&str>) -> String {
    if let Some(o) = override_name.map(str::trim).filter(|o| !o.is_empty()) {
        if o.starts_with(r"\\.\pipe\") {
            return o.to_string();
        }
        return format!(r"\\.\pipe\{}", sanitise(o));
    }
    format!(r"\\.\pipe\worldsmith-{}", sanitise(user.unwrap_or("")))
}

/// Keep [A-Za-z0-9_-]; anything else becomes `_`; empty becomes `user`.
pub fn sanitise(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    if out.is_empty() {
        "user".into()
    } else {
        out
    }
}

/// One plain line: newlines and tabs become spaces.
pub fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn err(msg: &str) -> Value {
    json!({ "ok": false, "error": one_line(msg) })
}

pub fn ok() -> Value {
    json!({ "ok": true })
}

/// `{"ok":true, ...fields}`.
pub fn ok_with(fields: Value) -> Value {
    let mut v = ok();
    if let (Some(m), Value::Object(f)) = (v.as_object_mut(), fields) {
        for (k, val) in f {
            m.insert(k, val);
        }
    }
    v
}

/// One item of a `top` / `items` list.
pub fn item(addr: u64, ty: ValueType, bytes: &[u8]) -> Value {
    json!({ "address": fmt_addr(addr), "type": ty.name(), "value": scan::display(ty, bytes) })
}

fn take_flag(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    let a = &args[*i];
    if let Some((_, v)) = a.split_once('=') {
        return Ok(v.to_string());
    }
    *i += 1;
    args.get(*i).cloned().ok_or_else(|| format!("{flag} needs a value."))
}

struct Flags {
    label: Option<String>,
    ty: Option<String>,
    value: Option<String>,
    transform: Option<String>,
    max: Option<String>,
    unknown: bool,
    rules: Vec<NextRule>,
}

fn parse_flags(args: &[String], allowed: &[&str]) -> Result<Flags, String> {
    let mut f = Flags {
        label: None,
        ty: None,
        value: None,
        transform: None,
        max: None,
        unknown: false,
        rules: vec![],
    };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let name = a.split('=').next().unwrap_or(a).to_ascii_lowercase();
        if !allowed.contains(&name.as_str()) {
            return Err(format!("Unexpected argument \"{a}\"."));
        }
        match name.as_str() {
            "--label" => f.label = Some(take_flag(args, &mut i, "--label")?),
            "--type" => f.ty = Some(take_flag(args, &mut i, "--type")?),
            "--value" => f.value = Some(take_flag(args, &mut i, "--value")?),
            "--transform" => f.transform = Some(take_flag(args, &mut i, "--transform")?),
            "--max" => f.max = Some(take_flag(args, &mut i, "--max")?),
            "--unknown" => f.unknown = true,
            "--changed" => f.rules.push(NextRule::Changed),
            "--unchanged" => f.rules.push(NextRule::Unchanged),
            "--increased" => f.rules.push(NextRule::Increased),
            "--decreased" => f.rules.push(NextRule::Decreased),
            _ => unreachable!(),
        }
        i += 1;
    }
    Ok(f)
}

fn label(f: &Flags) -> Result<String, String> {
    let l = f.label.as_deref().map(one_line).unwrap_or_default();
    if l.is_empty() {
        return Err("--label is required (a name like Gold).".into());
    }
    if l.chars().count() > 64 {
        return Err("That label is too long (64 characters at most).".into());
    }
    Ok(l)
}

fn value_type(s: &str) -> Result<ValueType, String> {
    ValueType::parse(s).ok_or_else(|| format!("\"{s}\" isn't a type (use i32, i64, f32 or f64)."))
}

fn positional(args: &[String], n: usize, usage: &str) -> Result<(), String> {
    if args.len() != n {
        return Err(format!("Usage: worldsmith-helper {usage}"));
    }
    Ok(())
}

/// argv (without the program name) -> Request. `serve` is handled before this.
pub fn parse_args(args: &[String]) -> Result<Request, String> {
    let Some(verb) = args.first() else {
        return Err(format!("No verb given ({}).", VERBS.join(", ")));
    };
    let rest = &args[1..];
    match verb.to_ascii_lowercase().as_str() {
        "games" => positional(rest, 0, "games").map(|_| Request::Games),
        "detach" => positional(rest, 0, "detach").map(|_| Request::Detach),
        "shot" => positional(rest, 0, "shot").map(|_| Request::Shot),
        "state" => positional(rest, 0, "state").map(|_| Request::State),
        "caps" => positional(rest, 0, "caps").map(|_| Request::Caps),
        "thumb" => positional(rest, 0, "thumb").map(|_| Request::Thumb),
        "attach" => {
            positional(rest, 1, "attach <pid>")?;
            let pid: u32 = rest[0]
                .trim()
                .parse()
                .map_err(|_| format!("\"{}\" isn't a process id.", rest[0].trim()))?;
            if pid == 0 {
                return Err("Process id 0 can't be attached.".into());
            }
            Ok(Request::Attach { pid })
        }
        "scan" => {
            let f = parse_flags(rest, &["--label", "--type", "--value", "--transform", "--unknown"])?;
            let label = label(&f)?;
            let ty_s = f.ty.clone().unwrap_or_else(|| "auto".into());
            let ty = ScanType::parse(&ty_s)
                .ok_or_else(|| format!("\"{ty_s}\" isn't a type (use i32, i64, f32, f64 or auto)."))?;
            let tr_s = f.transform.clone().unwrap_or_default();
            let transform = Transform::parse(&tr_s)
                .ok_or_else(|| format!("\"{tr_s}\" isn't a transform (use x10, x100 or neg)."))?;
            let value = match (&f.value, f.unknown) {
                (Some(_), true) => return Err("Use --value V or --unknown, not both.".into()),
                (None, false) => return Err("--value is required (or --unknown for a value you can't see).".into()),
                (None, true) => {
                    if ty == ScanType::Auto {
                        return Err("An unknown-value scan needs one type: --type i32, i64, f32 or f64.".into());
                    }
                    None
                }
                (Some(v), false) => {
                    scan::Target::new(v, transform)?;
                    Some(v.trim().to_string())
                }
            };
            Ok(Request::Scan { label, ty, value, transform })
        }
        "next" => {
            let f = parse_flags(
                rest,
                &["--label", "--value", "--changed", "--unchanged", "--increased", "--decreased"],
            )?;
            let label = label(&f)?;
            let mut rules = f.rules.clone();
            if let Some(v) = &f.value {
                scan::Target::new(v, Transform::None)?;
                rules.push(NextRule::Value(v.trim().to_string()));
            }
            if rules.len() != 1 {
                return Err(
                    "next needs exactly one of --value V, --changed, --unchanged, --increased, --decreased."
                        .into(),
                );
            }
            Ok(Request::Next { label, rule: rules.remove(0) })
        }
        "list" => {
            let f = parse_flags(rest, &["--label", "--max"])?;
            let label = label(&f)?;
            let max = match &f.max {
                None => 20,
                Some(m) => m
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|m| *m >= 1)
                    .ok_or_else(|| format!("\"{}\" isn't a count for --max.", m.trim()))?,
            }
            .min(1000);
            Ok(Request::List { label, max })
        }
        "write" | "freeze" => {
            positional(rest, 3, &format!("{verb} <address> <type> <value>"))?;
            let address = parse_addr(&rest[0])?;
            let ty = value_type(&rest[1])?;
            scan::encode(ty, &rest[2])?;
            let value = rest[2].trim().to_string();
            Ok(if verb.eq_ignore_ascii_case("write") {
                Request::Write { address, ty, value }
            } else {
                Request::Freeze { address, ty, value }
            })
        }
        "unfreeze" => {
            positional(rest, 1, "unfreeze <address>")?;
            Ok(Request::Unfreeze { address: parse_addr(&rest[0])? })
        }
        other => Err(format!("Unknown verb \"{other}\".")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn addresses_round_trip_in_uppercase_hex() {
        assert_eq!(fmt_addr(0x1a2f40), "0x1A2F40");
        assert_eq!(parse_addr("0x1A2F40").unwrap(), 0x1A2F40);
        assert_eq!(parse_addr("0X1a2f40").unwrap(), 0x1A2F40);
        assert_eq!(parse_addr("1A2F40").unwrap(), 0x1A2F40);
        assert!(parse_addr("0xZZ").is_err());
        assert!(parse_addr("0x").is_err());
        assert!(parse_addr("0x0").is_err());
        assert!(parse_addr("0x11112222333344445").is_err());
    }

    #[test]
    fn pipe_names() {
        assert_eq!(pipe_name(Some("Kiyo"), None), r"\\.\pipe\worldsmith-Kiyo");
        assert_eq!(pipe_name(Some("A B.c"), None), r"\\.\pipe\worldsmith-A_B_c");
        assert_eq!(pipe_name(None, None), r"\\.\pipe\worldsmith-user");
        assert_eq!(pipe_name(Some("x"), Some(r"\\.\pipe\t-1")), r"\\.\pipe\t-1");
        assert_eq!(pipe_name(Some("x"), Some("t 2")), r"\\.\pipe\t_2");
        assert_eq!(pipe_name(Some("x"), Some("  ")), r"\\.\pipe\worldsmith-x");
    }

    #[test]
    fn simple_verbs() {
        assert_eq!(parse_args(&argv("games")).unwrap(), Request::Games);
        assert_eq!(parse_args(&argv("attach 123")).unwrap(), Request::Attach { pid: 123 });
        assert!(parse_args(&argv("attach")).is_err());
        assert!(parse_args(&argv("attach abc")).is_err());
        assert!(parse_args(&argv("games extra")).is_err());
        assert!(parse_args(&argv("dance")).unwrap_err().contains("Unknown verb"));
        assert!(parse_args(&[]).is_err());
        assert_eq!(parse_args(&argv("caps")).unwrap(), Request::Caps);
        assert_eq!(parse_args(&argv("thumb")).unwrap(), Request::Thumb);
        assert_eq!(parse_args(&argv("shot")).unwrap(), Request::Shot);
        assert!(parse_args(&argv("caps extra")).is_err());
        assert!(parse_args(&argv("thumb 1")).is_err());
    }

    #[test]
    fn verbs_lists_every_verb() {
        // every verb parse_args knows, spelled out once here: adding a verb means adding it to
        // VERBS too, or this fails
        let known = [
            "games", "attach", "detach", "shot", "thumb", "scan", "next", "list", "write", "freeze",
            "unfreeze", "state", "caps",
        ];
        for v in known {
            assert!(VERBS.contains(&v), "VERBS is missing {v}");
        }
        assert_eq!(VERBS.len(), known.len(), "VERBS has a verb parse_args doesn't know");
        for v in VERBS {
            let e = parse_args(&[v.to_string()]).err().unwrap_or_default();
            assert!(!e.contains("Unknown verb"), "{v} isn't a parse_args verb");
        }
        assert!(parse_args(&[]).unwrap_err().contains("caps"));
        assert_eq!(VERSION, "1.0.0");
        assert!(FEATURES.contains(&"rect") && FEATURES.contains(&"thumb"));
    }

    #[test]
    fn scan_args() {
        let r = parse_args(&argv("scan --label Gold --type i32 --value 1234")).unwrap();
        assert_eq!(
            r,
            Request::Scan {
                label: "Gold".into(),
                ty: ScanType::I32,
                value: Some("1234".into()),
                transform: Transform::None
            }
        );
        let r = parse_args(&argv("scan --label HP --type auto --value 25.5 --transform x10")).unwrap();
        assert!(matches!(r, Request::Scan { ty: ScanType::Auto, transform: Transform::X10, .. }));
        let r = parse_args(&argv("scan --label=Neg --value=-5 --transform neg")).unwrap();
        assert!(matches!(r, Request::Scan { ty: ScanType::Auto, transform: Transform::Neg, .. }));
        assert!(parse_args(&argv("scan --type i32 --value 1")).is_err());
        assert!(parse_args(&argv("scan --label G --value abc")).is_err());
        assert!(parse_args(&argv("scan --label G --type u8 --value 1")).is_err());
        assert!(parse_args(&argv("scan --label G --value 1 --transform x3")).is_err());
        assert!(parse_args(&argv("scan --label G --value")).is_err());
        assert!(parse_args(&argv("scan --label G --value 1 --bogus")).is_err());
    }

    #[test]
    fn unknown_scan_args() {
        let r = parse_args(&argv("scan --label Gold --type i32 --unknown")).unwrap();
        assert_eq!(
            r,
            Request::Scan { label: "Gold".into(), ty: ScanType::I32, value: None, transform: Transform::None }
        );
        assert!(parse_args(&argv("scan --label Gold --type auto --unknown")).unwrap_err().contains("one type"));
        assert!(parse_args(&argv("scan --label Gold --unknown")).is_err(), "auto is the default type");
        assert!(parse_args(&argv("scan --label Gold --type i32 --unknown --value 3")).is_err());
        assert!(parse_args(&argv("scan --label Gold --type i32")).is_err());
    }

    #[test]
    fn next_args() {
        let r = parse_args(&argv("next --label Gold --value 1500")).unwrap();
        assert_eq!(r, Request::Next { label: "Gold".into(), rule: NextRule::Value("1500".into()) });
        let r = parse_args(&argv("next --label Gold --decreased")).unwrap();
        assert_eq!(r, Request::Next { label: "Gold".into(), rule: NextRule::Decreased });
        assert!(parse_args(&argv("next --label Gold")).is_err());
        assert!(parse_args(&argv("next --label Gold --changed --unchanged")).is_err());
    }

    #[test]
    fn write_freeze_list() {
        let r = parse_args(&argv("write 0x1A2F40 i32 9999")).unwrap();
        assert_eq!(r, Request::Write { address: 0x1A2F40, ty: ValueType::I32, value: "9999".into() });
        let r = parse_args(&argv("freeze 0x10 f32 2.5")).unwrap();
        assert_eq!(r, Request::Freeze { address: 0x10, ty: ValueType::F32, value: "2.5".into() });
        assert!(parse_args(&argv("write 0x10 auto 1")).is_err());
        assert!(parse_args(&argv("write 0x10 i32 1.5")).is_err());
        assert!(parse_args(&argv("write 0x10 i32")).is_err());
        assert_eq!(parse_args(&argv("unfreeze 0x10")).unwrap(), Request::Unfreeze { address: 0x10 });
        assert_eq!(
            parse_args(&argv("list --label Gold --max 5")).unwrap(),
            Request::List { label: "Gold".into(), max: 5 }
        );
        assert_eq!(parse_args(&argv("list --label Gold")).unwrap(), Request::List { label: "Gold".into(), max: 20 });
        assert!(parse_args(&argv("list --label Gold --max 0")).is_err());
    }

    #[test]
    fn requests_survive_the_pipe() {
        for line in [
            "games",
            "attach 7",
            "scan --label G --type f64 --value 3 --transform x100",
            "scan --label G --type f32 --unknown",
            "next --label G --value 4",
            "next --label G --changed",
            "list --label G --max 3",
            "write 0xABC i64 -9",
            "freeze 0xABC f32 1",
            "unfreeze 0xABC",
            "state",
            "detach",
            "shot",
            "thumb",
            "caps",
        ] {
            let r = parse_args(&argv(line)).unwrap();
            let j = serde_json::to_string(&r).unwrap();
            assert!(!j.contains('\n'));
            let back: Request = serde_json::from_str(&j).unwrap();
            assert_eq!(back, r, "{line} -> {j}");
        }
    }

    #[test]
    fn replies() {
        assert_eq!(err("bad\nthing").to_string(), r#"{"error":"bad thing","ok":false}"#);
        let v = ok_with(json!({"label": "Gold", "matches": 3}));
        assert_eq!(v["ok"], true);
        assert_eq!(v["matches"], 3);
        let it = item(0xABC, ValueType::I32, &9999i32.to_le_bytes());
        assert_eq!(it, json!({"address": "0xABC", "type": "i32", "value": "9999"}));
    }
}
