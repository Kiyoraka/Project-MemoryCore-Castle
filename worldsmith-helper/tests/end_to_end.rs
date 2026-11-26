//! End to end on Windows: a real server, the real client, and the target_game example as the
//! "game". Every run uses its own pipe name (WORLDSMITH_PIPE), so a real server is never touched.
#![cfg(windows)]

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const HELPER: &str = env!("CARGO_BIN_EXE_worldsmith-helper");

fn unique_pipe(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("worldsmith-test-{tag}-{}-{nanos}", std::process::id())
}

/// The example is built by `cargo test` next to the test binaries: target/<profile>/examples.
fn target_game_exe() -> PathBuf {
    let me = std::env::current_exe().unwrap();
    let profile_dir = me.parent().unwrap().parent().unwrap();
    let exe = profile_dir.join("examples").join("target_game.exe");
    assert!(
        exe.exists(),
        "{} is missing - run the whole `cargo test` (it builds the examples) or `cargo build --examples` first",
        exe.display()
    );
    exe
}

fn client(pipe: &str, args: &[&str]) -> Value {
    let out = Command::new(HELPER).env("WORLDSMITH_PIPE", pipe).args(args).output().unwrap();
    assert!(out.status.success(), "{args:?} exited with {:?}", out.status);
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "{args:?} must print one line, printed {text:?}");
    serde_json::from_str(lines[0]).unwrap_or_else(|_| panic!("{args:?} printed non-JSON {text:?}"))
}

fn ok(v: &Value, what: &str) {
    assert_eq!(v["ok"], true, "{what}: {v}");
}

struct Game {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    pid: u32,
    gold: String,
    hp: String,
}

impl Game {
    fn start() -> Game {
        let mut child = Command::new(target_game_exe())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let (mut pid, mut gold, mut hp) = (0, String::new(), String::new());
        loop {
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            let line = line.trim();
            match line.split_once(' ') {
                Some(("pid", p)) => pid = p.parse().unwrap(),
                Some(("gold", a)) => gold = a.to_string(),
                Some(("hp", a)) => hp = a.to_string(),
                _ if line == "ready" => break,
                _ if line.is_empty() => panic!("target_game ended early"),
                _ => {}
            }
        }
        Game { child, stdin, stdout, pid, gold, hp }
    }

    fn say(&mut self, cmd: &str) -> String {
        writeln!(self.stdin, "{cmd}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        line.trim().to_string()
    }

    fn gold(&mut self) -> String {
        self.say("read gold")
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Serve(Child);

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_server(pipe: &str, parent: u32) -> Serve {
    let mut child = Command::new(HELPER)
        .env("WORLDSMITH_PIPE", pipe)
        .args(["serve", "--parent", &parent.to_string()])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut first).unwrap();
    assert_eq!(first.trim(), r#"{"ok":true,"serving":true}"#);
    Serve(child)
}

fn has_address(list: &Value, addr: &str) -> bool {
    list.as_array().map(|a| a.iter().any(|i| i["address"] == addr)).unwrap_or(false)
}

#[test]
fn scan_narrow_write_freeze_detach() {
    let pipe = unique_pipe("main");
    let _server = start_server(&pipe, std::process::id());

    // one server per pipe
    let again = Command::new(HELPER)
        .env("WORLDSMITH_PIPE", &pipe)
        .args(["serve", "--parent", &std::process::id().to_string()])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&again.stdout).trim(), r#"{"ok":true,"already":true}"#);

    // no server on another name
    let none = client(&unique_pipe("nobody"), &["state"]);
    assert_eq!(none["ok"], false);
    assert_eq!(none["error"], "Worldsmith isn't running - open the WORLDSMITH room first.");

    // bad args never reach the server
    let bad = client(&pipe, &["dance"]);
    assert_eq!(bad["ok"], false);
    assert!(client(&pipe, &["scan", "--label", "x", "--type", "auto", "--unknown"])["ok"] == false);

    let games = client(&pipe, &["games"]);
    ok(&games, "games");
    assert!(games["games"].is_array());

    // nothing attached yet
    assert_eq!(client(&pipe, &["scan", "--label", "gold", "--type", "i32", "--value", "1"])["ok"], false);
    let st = client(&pipe, &["state"]);
    assert_eq!(st["attached"], Value::Null);

    let mut game = Game::start();
    let attach = client(&pipe, &["attach", &game.pid.to_string()]);
    ok(&attach, "attach");
    assert_eq!(attach["pid"], game.pid);
    assert_eq!(attach["exe"], "target_game.exe");

    // no window: shot says so in one line
    let shot = client(&pipe, &["shot"]);
    assert_eq!(shot["ok"], false, "{shot}");

    // ---- known value: scan, change, next ----
    let s = client(&pipe, &["scan", "--label", "gold", "--type", "i32", "--value", "1234"]);
    ok(&s, "scan");
    assert!(s["matches"].as_u64().unwrap() >= 1, "{s}");
    assert_eq!(game.say("gold 1500"), "gold 1500");
    let n = client(&pipe, &["next", "--label", "gold", "--value", "1500"]);
    ok(&n, "next");
    let matches = n["matches"].as_u64().unwrap();
    assert!((1..=5).contains(&matches), "{n}");
    assert_eq!(n["steps"].as_array().unwrap().len(), 2);
    assert!(has_address(&n["top"], &game.gold), "{} not in {n}", game.gold);

    let l = client(&pipe, &["list", "--label", "gold", "--max", "20"]);
    ok(&l, "list");
    assert!(has_address(&l["items"], &game.gold));

    // ---- write ----
    let gold_addr = game.gold.clone();
    ok(&client(&pipe, &["write", &gold_addr, "i32", "4321"]), "write");
    assert_eq!(game.gold(), "gold 4321");

    // ---- freeze: the game sets 1, Worldsmith puts 7777 back ----
    ok(&client(&pipe, &["freeze", &gold_addr, "i32", "7777"]), "freeze");
    assert_eq!(game.say("gold 1"), "gold 1");
    let start = Instant::now();
    loop {
        if game.gold() == "gold 7777" {
            break;
        }
        assert!(start.elapsed() < Duration::from_millis(300), "the freeze didn't hold");
        std::thread::sleep(Duration::from_millis(10));
    }
    let st = client(&pipe, &["state"]);
    ok(&st, "state");
    assert_eq!(st["attached"]["pid"], game.pid);
    assert_eq!(st["labels"][0]["label"], "gold");
    assert!(has_address(&st["freezes"], &gold_addr));
    assert_eq!(st["freezes"][0]["value"], "7777");

    // ---- detach: freezes stop ----
    ok(&client(&pipe, &["detach"]), "detach");
    assert_eq!(game.say("gold 2"), "gold 2");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(game.gold(), "gold 2");
    let st = client(&pipe, &["state"]);
    assert_eq!(st["attached"], Value::Null);
    assert_eq!(st["labels"], serde_json::json!([]));
    assert_eq!(st["freezes"], serde_json::json!([]));

    // ---- auto scan finds the f32 ----
    ok(&client(&pipe, &["attach", &game.pid.to_string()]), "attach again");
    let s = client(&pipe, &["scan", "--label", "hp", "--type", "auto", "--value", "250"]);
    ok(&s, "auto scan");
    assert_eq!(game.say("hp 251.5"), "hp 251.5");
    let n = client(&pipe, &["next", "--label", "hp", "--value", "251.5"]);
    ok(&n, "auto next");
    let top = n["top"].as_array().expect("few matches have a top list");
    assert!(
        top.iter().any(|i| i["address"] == game.hp.as_str() && i["type"] == "f32" && i["value"] == "251.5"),
        "{n}"
    );

    // ---- unknown value: changed, then increased ----
    let u = client(&pipe, &["scan", "--label", "mystery", "--type", "i32", "--unknown"]);
    ok(&u, "unknown scan");
    assert!(u["matches"].as_u64().unwrap() > 1000, "{u}");
    assert_eq!(game.say("gold 2000"), "gold 2000");
    let c = client(&pipe, &["next", "--label", "mystery", "--changed"]);
    ok(&c, "next --changed");
    assert!(c["matches"].as_u64().unwrap() < u["matches"].as_u64().unwrap());
    let mut v = 2500;
    assert_eq!(game.say(&format!("gold {v}")), format!("gold {v}"));
    let mut last = client(&pipe, &["next", "--label", "mystery", "--increased"]);
    ok(&last, "next --increased");
    // a few more rounds until the survivors fit one list
    for _ in 0..6 {
        if last["matches"].as_u64().unwrap() <= 1000 {
            break;
        }
        v += 100;
        game.say(&format!("gold {v}"));
        last = client(&pipe, &["next", "--label", "mystery", "--increased"]);
    }
    let survivors = client(&pipe, &["list", "--label", "mystery", "--max", "1000"]);
    ok(&survivors, "list mystery");
    assert!(has_address(&survivors["items"], &game.gold), "{} lost: {last}", game.gold);
    // nothing changed: unchanged keeps it, decreased drops it
    let un = client(&pipe, &["next", "--label", "mystery", "--unchanged"]);
    ok(&un, "unchanged");
    let survivors = client(&pipe, &["list", "--label", "mystery", "--max", "1000"]);
    assert!(has_address(&survivors["items"], &game.gold));
    game.say(&format!("gold {}", v + 1));
    client(&pipe, &["next", "--label", "mystery", "--decreased"]);
    let survivors = client(&pipe, &["list", "--label", "mystery", "--max", "1000"]);
    assert!(!has_address(&survivors["items"], &game.gold));

    // the game exits: Worldsmith detaches by itself
    drop(game);
    let start = Instant::now();
    loop {
        let st = client(&pipe, &["state"]);
        if st["attached"] == Value::Null {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(3), "still attached to a dead game: {st}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn server_exits_with_its_parent() {
    let pipe = unique_pipe("parent");
    let mut parent = Game::start();
    let mut server = start_server(&pipe, parent.pid);
    ok(&client(&pipe, &["state"]), "state");
    let _ = parent.child.kill();
    let _ = parent.child.wait();
    let start = Instant::now();
    loop {
        if server.0.try_wait().unwrap().is_some() {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "serve outlived its parent");
        std::thread::sleep(Duration::from_millis(50));
    }
    let after = client(&pipe, &["state"]);
    assert_eq!(after["ok"], false);
}
