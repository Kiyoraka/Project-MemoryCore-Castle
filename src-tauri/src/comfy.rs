//! A small client for a ComfyUI server the member runs themselves (their own PC or one on their
//! network): is it there, which checkpoints does it hold, queue a workflow, wait for the picture,
//! fetch it, cancel it. Plain HTTP on ComfyUI's own API; no workflow of ours is assumed here, the
//! caller hands in the graph. Nothing here is wired to a screen or to the picture queue yet.
#![allow(dead_code)] // used by the next change, which makes ComfyUI one of the ways to draw

use reqwest::blocking::{Client, Response};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Where ComfyUI listens when the member starts it the usual way.
pub const DEFAULT_ADDRESS: &str = "http://127.0.0.1:8188";

/// How long one small question (is it there, what is queued) may take.
const ASK_LIMIT: Duration = Duration::from_secs(15);
/// How long fetching a finished picture may take.
const FETCH_LIMIT: Duration = Duration::from_secs(60);
/// How many answers in a row may fail before a drawing is given up (ComfyUI crashed or was closed).
const LOST_AFTER: u32 = 3;

/// A ComfyUI server, by its address.
#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    base: String,
}

/// One picture ComfyUI made, as it names it.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub filename: String,
    pub subfolder: String,
    /// "output" for a saved picture ("temp" is a preview that is thrown away)
    pub kind: String,
}

/// The member's address, tidied: `host:port` gets `http://`, a trailing slash goes, empty means
/// the usual local address. Only a scheme, a host and a port are accepted: a path, a query or a
/// login in the address is a mistake, and said so.
pub fn parse_address(text: &str) -> Result<Server, String> {
    let text = text.trim();
    if text.is_empty() {
        return parse_address(DEFAULT_ADDRESS);
    }
    let with_scheme = if text.contains("://") { text.to_string() } else { format!("http://{text}") };
    let url = reqwest::Url::parse(&with_scheme).map_err(|_| "That is not an address. Try something like http://127.0.0.1:8188.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("The address must start with http:// or https://.".into());
    }
    if url.host_str().is_none() {
        return Err("The address has no computer name in it. Try something like http://127.0.0.1:8188.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Leave the user name and password out of the address.".into());
    }
    if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
        return Err("Give only the computer and the port, such as http://127.0.0.1:8188, with nothing after it.".into());
    }
    Ok(Server { base: url.origin().ascii_serialization() })
}

fn client(limit: Duration) -> Result<Client, String> {
    Client::builder()
        .user_agent(concat!("MemoryCoreCastle/", env!("CARGO_PKG_VERSION")))
        .timeout(limit)
        .build()
        .map_err(|_| "Castle could not prepare to talk to ComfyUI.".to_string())
}

impl Server {
    pub fn address(&self) -> &str {
        &self.base
    }

    fn down(&self) -> String {
        format!("ComfyUI did not answer at {}. Is it running?", self.base)
    }

    fn get(&self, path: &str, query: &[(&str, &str)], limit: Duration) -> Result<Response, String> {
        client(limit)?.get(format!("{}{path}", self.base)).query(query).send().map_err(|_| self.down())
    }

    fn post(&self, path: &str, body: &Value, limit: Duration) -> Result<Response, String> {
        client(limit)?.post(format!("{}{path}", self.base)).json(body).send().map_err(|_| self.down())
    }

    fn json(&self, response: Response) -> Result<Value, String> {
        if !response.status().is_success() {
            return Err(format!("ComfyUI at {} answered with an error ({}).", self.base, response.status().as_u16()));
        }
        response.json::<Value>().map_err(|_| format!("ComfyUI at {} sent something Castle could not read.", self.base))
    }

    /// Whether a ComfyUI answers at this address. A different program on the same port is told
    /// apart by the shape of the answer.
    pub fn ping(&self) -> Result<(), String> {
        let stats = self.json(self.get("/system_stats", &[], ASK_LIMIT)?)?;
        if stats.get("system").is_some() {
            Ok(())
        } else {
            Err(format!("Something answers at {}, but it does not look like ComfyUI.", self.base))
        }
    }

    /// The checkpoint files ComfyUI can load (empty when it has none, or no checkpoint loader).
    pub fn checkpoints(&self) -> Result<Vec<String>, String> {
        let info = self.json(self.get("/object_info/CheckpointLoaderSimple", &[], ASK_LIMIT)?)?;
        Ok(info
            .pointer("/CheckpointLoaderSimple/input/required/ckpt_name/0")
            .and_then(Value::as_array)
            .map(|names| names.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
            .unwrap_or_default())
    }

    /// Queues a workflow (ComfyUI's "API format" graph); returns the id to wait on. A graph
    /// ComfyUI refuses comes back as ComfyUI's own reason in plain words.
    pub fn submit(&self, graph: &Value, client_id: &str) -> Result<String, String> {
        let response = self.post("/prompt", &json!({ "prompt": graph, "client_id": client_id }), ASK_LIMIT)?;
        let status = response.status();
        let body: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(rejection_words(&body, status.as_u16()));
        }
        body.get("prompt_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("ComfyUI at {} did not say it took the workflow.", self.base))
    }

    /// Waits until the queued workflow has finished and returns its saved pictures. `stop` is
    /// asked between looks; on Stop or on running past `limit` the workflow is cancelled first.
    pub fn wait(&self, id: &str, limit: Duration, poll: Duration, stop: &dyn Fn() -> bool) -> Result<Vec<Picture>, String> {
        let started = Instant::now();
        let mut lost = 0;
        loop {
            if stop() {
                self.cancel(id);
                return Err("Stopped.".into());
            }
            match self.get(&format!("/history/{id}"), &[], ASK_LIMIT).and_then(|r| self.json(r)) {
                Ok(history) => {
                    lost = 0;
                    if let Some(entry) = history.get(id) {
                        return finished(entry);
                    }
                }
                Err(_) => {
                    lost += 1;
                    if lost >= LOST_AFTER {
                        return Err(format!("ComfyUI at {} stopped answering while it was drawing.", self.base));
                    }
                }
            }
            if started.elapsed() >= limit {
                self.cancel(id);
                return Err("ComfyUI is taking too long. Castle stopped waiting; raise the time limit if your model is slow.".into());
            }
            std::thread::sleep(poll);
        }
    }

    /// The bytes of a picture ComfyUI made.
    pub fn fetch(&self, picture: &Picture) -> Result<Vec<u8>, String> {
        let response = self.get("/view", &[("filename", &picture.filename), ("subfolder", &picture.subfolder), ("type", &picture.kind)], FETCH_LIMIT)?;
        if !response.status().is_success() {
            return Err(format!("ComfyUI would not hand over the picture ({}).", response.status().as_u16()));
        }
        let bytes = response.bytes().map_err(|_| "The picture could not be read from ComfyUI.".to_string())?;
        if bytes.is_empty() {
            return Err("ComfyUI handed over an empty picture.".into());
        }
        Ok(bytes.to_vec())
    }

    /// Takes a workflow off the queue; when it is the one drawing right now, interrupts it. Best
    /// effort and never an error: another job of the member's own is never interrupted.
    pub fn cancel(&self, id: &str) {
        let _ = self.post("/queue", &json!({ "delete": [id] }), ASK_LIMIT);
        let running = self
            .get("/queue", &[], ASK_LIMIT)
            .and_then(|r| self.json(r))
            .map(|queue| queue.get("queue_running").and_then(Value::as_array).is_some_and(|jobs| jobs.iter().any(|job| job.get(1).and_then(Value::as_str) == Some(id))))
            .unwrap_or(false);
        if running {
            let _ = self.post("/interrupt", &json!({}), ASK_LIMIT);
        }
    }
}

/// ComfyUI's own reason for refusing a workflow, in a line.
fn rejection_words(body: &Value, status: u16) -> String {
    let mut words = body.pointer("/error/message").and_then(Value::as_str).unwrap_or("it was not accepted").to_string();
    if let Some((node, report)) = body.get("node_errors").and_then(Value::as_object).and_then(|m| m.iter().next()) {
        let class = report.get("class_type").and_then(Value::as_str).unwrap_or("?");
        let what = report.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("a problem");
        let detail = report.pointer("/errors/0/details").and_then(Value::as_str).filter(|d| !d.is_empty()).map(|d| format!(" ({d})")).unwrap_or_default();
        words = format!("{words}: {class} (node {node}) {what}{detail}");
    }
    format!("ComfyUI refused the workflow ({status}): {words}.")
}

/// What a finished history entry holds: its saved pictures, or the reason it failed.
fn finished(entry: &Value) -> Result<Vec<Picture>, String> {
    if entry.pointer("/status/status_str").and_then(Value::as_str) == Some("error") {
        return Err(failure_words(entry));
    }
    let mut saved = Vec::new();
    for output in entry.get("outputs").and_then(Value::as_object).into_iter().flat_map(|m| m.values()) {
        for image in output.get("images").and_then(Value::as_array).into_iter().flatten() {
            let text = |key: &str| image.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
            let picture = Picture { filename: text("filename"), subfolder: text("subfolder"), kind: text("type") };
            if picture.kind == "output" && !picture.filename.is_empty() {
                saved.push(picture);
            }
        }
    }
    if saved.is_empty() {
        return Err("ComfyUI finished, but the workflow saved no picture. It needs a Save Image node.".into());
    }
    Ok(saved)
}

fn failure_words(entry: &Value) -> String {
    let failure = entry
        .pointer("/status/messages")
        .and_then(Value::as_array)
        .and_then(|messages| messages.iter().find(|m| m.get(0).and_then(Value::as_str) == Some("execution_error")))
        .and_then(|m| m.get(1));
    let why = failure.and_then(|f| f.get("exception_message")).and_then(Value::as_str).map(str::trim).filter(|w| !w.is_empty()).unwrap_or("it gave no reason");
    match failure.and_then(|f| f.get("node_type")).and_then(Value::as_str) {
        Some(node) => format!("ComfyUI could not finish the picture at {node}: {why}."),
        None => format!("ComfyUI could not finish the picture: {why}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    type Answer = (u16, Vec<u8>);

    /// A stand-in ComfyUI: answers every request through `handler` (given the request as text) and
    /// keeps what it was asked. Stops when dropped.
    struct Fake {
        server: Server,
        log: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl Fake {
        fn new(handler: impl Fn(&str) -> Answer + Send + 'static) -> Fake {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let log = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (seen, halt) = (log.clone(), stop.clone());
            std::thread::spawn(move || {
                while !halt.load(Ordering::SeqCst) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                    let mut bytes = Vec::new();
                    loop {
                        let mut buf = [0; 8192];
                        let Ok(n) = stream.read(&mut buf) else { break };
                        if n == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&bytes);
                        if let Some(end) = text.find("\r\n\r\n") {
                            let length = text[..end].lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").and_then(|s| s.trim().parse::<usize>().ok())).unwrap_or(0);
                            if bytes.len() >= end + 4 + length {
                                break;
                            }
                        }
                    }
                    let request = String::from_utf8_lossy(&bytes).to_string();
                    let (status, body) = handler(&request);
                    seen.lock().unwrap().push(request);
                    let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = stream.write_all(&body);
                }
            });
            Fake { server: Server { base }, log, stop }
        }

        fn asked(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    fn ok(value: Value) -> Answer {
        (200, value.to_string().into_bytes())
    }

    fn no_stop() -> bool {
        false
    }

    fn done(id: &str, outputs: Value) -> Value {
        json!({ id: { "outputs": outputs, "status": { "status_str": "success", "completed": true, "messages": [] } } })
    }

    #[test]
    fn an_address_is_tidied_and_a_wrong_one_is_told_why() {
        assert_eq!(parse_address("").unwrap().address(), "http://127.0.0.1:8188");
        assert_eq!(parse_address("  192.168.1.20:8188 ").unwrap().address(), "http://192.168.1.20:8188");
        assert_eq!(parse_address("http://localhost:8188/").unwrap().address(), "http://localhost:8188");
        assert_eq!(parse_address("https://comfy.example.com").unwrap().address(), "https://comfy.example.com");
        assert!(parse_address("ftp://127.0.0.1:8188").unwrap_err().contains("http://"));
        assert!(parse_address("http://user:pw@127.0.0.1:8188").unwrap_err().contains("user name"));
        assert!(parse_address("http://127.0.0.1:8188/prompt").unwrap_err().contains("nothing after it"));
        assert!(parse_address("http://127.0.0.1:8188/?a=1").unwrap_err().contains("nothing after it"));
        assert!(parse_address("http://").is_err());
    }

    #[test]
    fn ping_tells_comfyui_from_nothing_from_something_else() {
        let comfy = Fake::new(|_| ok(json!({ "system": { "os": "nt" }, "devices": [] })));
        assert_eq!(comfy.server.ping(), Ok(()));

        let other = Fake::new(|_| ok(json!({ "hello": "world" })));
        assert!(other.server.ping().unwrap_err().contains("does not look like ComfyUI"));

        let broken = Fake::new(|_| (500, b"oops".to_vec()));
        assert!(broken.server.ping().unwrap_err().contains("(500)"));

        let silent = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            drop(listener);
            parse_address(&base).unwrap()
        };
        assert!(silent.ping().unwrap_err().contains("Is it running?"));
    }

    #[test]
    fn the_checkpoints_come_from_the_loader_and_none_is_an_empty_list() {
        let some = Fake::new(|_| ok(json!({ "CheckpointLoaderSimple": { "input": { "required": { "ckpt_name": [["a.safetensors", "b.ckpt"], {}] } } } })));
        assert_eq!(some.server.checkpoints().unwrap(), vec!["a.safetensors".to_string(), "b.ckpt".to_string()]);
        assert!(some.asked()[0].starts_with("GET /object_info/CheckpointLoaderSimple "));

        let none = Fake::new(|_| ok(json!({})));
        assert!(none.server.checkpoints().unwrap().is_empty());
    }

    #[test]
    fn a_workflow_is_queued_with_its_graph_and_the_id_comes_back() {
        let comfy = Fake::new(|_| ok(json!({ "prompt_id": "abc-1", "number": 3, "node_errors": {} })));
        let id = comfy.server.submit(&json!({ "1": { "class_type": "KSampler", "inputs": {} } }), "castle-7").unwrap();
        assert_eq!(id, "abc-1");
        let sent = &comfy.asked()[0];
        assert!(sent.starts_with("POST /prompt "));
        assert!(sent.contains("\"client_id\":\"castle-7\""));
        assert!(sent.contains("\"class_type\":\"KSampler\""));
    }

    #[test]
    fn a_refused_workflow_comes_back_in_comfyuis_own_words() {
        let comfy = Fake::new(|_| {
            (
                400,
                json!({
                    "error": { "type": "prompt_outputs_failed_validation", "message": "Prompt outputs failed validation" },
                    "node_errors": { "4": { "class_type": "CheckpointLoaderSimple", "errors": [{ "message": "Value not in list", "details": "ckpt_name: 'x.safetensors' not in []" }] } }
                })
                .to_string()
                .into_bytes(),
            )
        });
        let said = comfy.server.submit(&json!({}), "c").unwrap_err();
        assert!(said.contains("refused the workflow (400)"));
        assert!(said.contains("Prompt outputs failed validation"));
        assert!(said.contains("CheckpointLoaderSimple (node 4) Value not in list"));
        assert!(said.contains("x.safetensors"));

        let bare = Fake::new(|_| (400, b"not json".to_vec()));
        assert!(bare.server.submit(&json!({}), "c").unwrap_err().contains("it was not accepted"));
    }

    #[test]
    fn waiting_follows_the_history_until_the_pictures_are_saved_and_skips_previews() {
        let looks = Arc::new(AtomicUsize::new(0));
        let counted = looks.clone();
        let comfy = Fake::new(move |request| {
            assert!(request.starts_with("GET /history/job-9 "));
            if counted.fetch_add(1, Ordering::SeqCst) < 2 {
                ok(json!({}))
            } else {
                ok(done(
                    "job-9",
                    json!({
                        "3": { "images": [{ "filename": "preview.png", "subfolder": "", "type": "temp" }] },
                        "9": { "images": [{ "filename": "castle_00001_.png", "subfolder": "castle", "type": "output" }] }
                    }),
                ))
            }
        });
        let pictures = comfy.server.wait("job-9", Duration::from_secs(5), Duration::from_millis(5), &no_stop).unwrap();
        assert_eq!(pictures, vec![Picture { filename: "castle_00001_.png".into(), subfolder: "castle".into(), kind: "output".into() }]);
        assert_eq!(looks.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_workflow_that_fails_or_saves_nothing_says_so() {
        let failed = Fake::new(|_| {
            ok(json!({ "j": { "outputs": {}, "status": { "status_str": "error", "completed": false, "messages": [["execution_start", {}], ["execution_error", { "node_type": "KSampler", "exception_message": "Allocation on device\n" }]] } } }))
        });
        let said = failed.server.wait("j", Duration::from_secs(5), Duration::from_millis(5), &no_stop).unwrap_err();
        assert_eq!(said, "ComfyUI could not finish the picture at KSampler: Allocation on device.");

        let empty = Fake::new(|_| ok(done("j", json!({}))));
        assert!(empty.server.wait("j", Duration::from_secs(5), Duration::from_millis(5), &no_stop).unwrap_err().contains("Save Image"));
    }

    #[test]
    fn a_comfyui_that_goes_away_while_drawing_is_given_up_on() {
        let gone = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            drop(listener);
            parse_address(&base).unwrap()
        };
        let said = gone.wait("j", Duration::from_secs(30), Duration::from_millis(5), &no_stop).unwrap_err();
        assert!(said.contains("stopped answering"));
    }

    #[test]
    fn stop_and_the_time_limit_cancel_the_workflow() {
        // the workflow is the one drawing right now: cancelling takes it off the queue and interrupts
        let comfy = Fake::new(|request| {
            if request.starts_with("GET /queue ") {
                ok(json!({ "queue_running": [[0, "slow", {}, {}, []]], "queue_pending": [] }))
            } else if request.starts_with("GET /history/") {
                ok(json!({}))
            } else {
                ok(json!({}))
            }
        });
        let asked_to_stop = AtomicBool::new(false);
        let looks = AtomicUsize::new(0);
        let stop = || {
            looks.fetch_add(1, Ordering::SeqCst);
            asked_to_stop.load(Ordering::SeqCst) || looks.load(Ordering::SeqCst) > 3
        };
        assert_eq!(comfy.server.wait("slow", Duration::from_secs(30), Duration::from_millis(5), &stop).unwrap_err(), "Stopped.");
        let asked = comfy.asked();
        assert!(asked.iter().any(|r| r.starts_with("POST /queue ") && r.contains("\"delete\":[\"slow\"]")));
        assert!(asked.iter().any(|r| r.starts_with("POST /interrupt ")));

        // out of time
        let slow = Fake::new(|_| ok(json!({})));
        let said = slow.server.wait("slow", Duration::from_millis(30), Duration::from_millis(5), &no_stop).unwrap_err();
        assert!(said.contains("taking too long"));
        assert!(slow.asked().iter().any(|r| r.starts_with("POST /queue ")));
    }

    #[test]
    fn a_member_s_other_job_is_never_interrupted() {
        // the queue shows a different job running: ours was only waiting, so only the delete is sent
        let comfy = Fake::new(|request| {
            if request.starts_with("GET /queue ") {
                ok(json!({ "queue_running": [[0, "someone-elses", {}, {}, []]], "queue_pending": [[1, "mine", {}, {}, []]] }))
            } else {
                ok(json!({}))
            }
        });
        comfy.server.cancel("mine");
        let asked = comfy.asked();
        assert!(asked.iter().any(|r| r.starts_with("POST /queue ")));
        assert!(!asked.iter().any(|r| r.starts_with("POST /interrupt ")));
    }

    #[test]
    fn a_picture_is_fetched_by_the_names_comfyui_gave() {
        let comfy = Fake::new(|_| (200, vec![0x89, b'P', b'N', b'G']));
        let bytes = comfy.server.fetch(&Picture { filename: "my picture.png".into(), subfolder: "castle".into(), kind: "output".into() }).unwrap();
        assert_eq!(bytes, vec![0x89, b'P', b'N', b'G']);
        let sent = &comfy.asked()[0];
        assert!(sent.starts_with("GET /view?"));
        assert!(sent.contains("filename=my+picture.png") || sent.contains("filename=my%20picture.png"));
        assert!(sent.contains("subfolder=castle"));
        assert!(sent.contains("type=output"));

        let missing = Fake::new(|_| (404, b"no".to_vec()));
        assert!(missing.server.fetch(&Picture { filename: "x.png".into(), subfolder: String::new(), kind: "output".into() }).unwrap_err().contains("(404)"));
        let empty = Fake::new(|_| (200, Vec::new()));
        assert!(empty.server.fetch(&Picture { filename: "x.png".into(), subfolder: String::new(), kind: "output".into() }).unwrap_err().contains("empty"));
    }

    /// Against a real ComfyUI with at least one checkpoint, started by hand:
    /// `cargo test --manifest-path src-tauri/Cargo.toml comfy::tests::live -- --ignored --nocapture`
    /// CASTLE_COMFY_ADDRESS sets the address (default http://127.0.0.1:8188); the picture lands in
    /// the temp folder and its path is printed. Draws one small picture with the first checkpoint.
    #[test]
    #[ignore]
    fn live_a_real_comfyui_draws_one_small_picture() {
        let server = parse_address(&std::env::var("CASTLE_COMFY_ADDRESS").unwrap_or_default()).unwrap();
        server.ping().unwrap();
        let checkpoints = server.checkpoints().unwrap();
        println!("checkpoints: {checkpoints:?}");
        let checkpoint = checkpoints.first().expect("this ComfyUI has no checkpoint to draw with").clone();
        let graph = json!({
            "1": { "class_type": "CheckpointLoaderSimple", "inputs": { "ckpt_name": checkpoint } },
            "2": { "class_type": "CLIPTextEncode", "inputs": { "text": "a small red lantern on a wooden table, soft light, photo", "clip": ["1", 1] } },
            "3": { "class_type": "CLIPTextEncode", "inputs": { "text": "blurry, text, watermark", "clip": ["1", 1] } },
            "4": { "class_type": "EmptyLatentImage", "inputs": { "width": 768, "height": 768, "batch_size": 1 } },
            "5": { "class_type": "KSampler", "inputs": { "model": ["1", 0], "positive": ["2", 0], "negative": ["3", 0], "latent_image": ["4", 0], "seed": 7, "steps": 12, "cfg": 6.0, "sampler_name": "euler", "scheduler": "normal", "denoise": 1.0 } },
            "6": { "class_type": "VAEDecode", "inputs": { "samples": ["5", 0], "vae": ["1", 2] } },
            "7": { "class_type": "SaveImage", "inputs": { "images": ["6", 0], "filename_prefix": "castle-live-test" } }
        });
        let started = Instant::now();
        let id = server.submit(&graph, "castle-live-test").unwrap();
        println!("queued as {id}");
        let pictures = server.wait(&id, Duration::from_secs(600), Duration::from_secs(1), &no_stop).unwrap();
        println!("drawn in {:.0} s: {pictures:?}", started.elapsed().as_secs_f32());
        let bytes = server.fetch(&pictures[0]).unwrap();
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "not a PNG");
        let out = std::env::temp_dir().join(format!("castle-comfy-live-{}.png", std::process::id()));
        std::fs::write(&out, &bytes).unwrap();
        println!("saved {} ({} bytes)", out.display(), bytes.len());
    }
}
