//! A small client for a ComfyUI server the member runs themselves (their own PC or one on their
//! network): is it there, which checkpoints does it hold, queue a workflow, wait for the picture,
//! fetch it, cancel it. Plain HTTP on ComfyUI's own API; no workflow of ours is assumed here, the
//! caller hands in the graph. Nothing here is wired to a screen or to the picture queue yet.
#![allow(dead_code)] // used by the next change, which makes ComfyUI one of the ways to draw

use crate::settings::SettingsStore;
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};

/// Where ComfyUI listens when the member starts it the usual way.
pub const DEFAULT_ADDRESS: &str = "http://127.0.0.1:8188";

/// How long one small question (is it there, what is queued) may take.
const ASK_LIMIT: Duration = Duration::from_secs(15);
/// How long fetching a finished picture may take.
const FETCH_LIMIT: Duration = Duration::from_secs(60);
/// How many answers in a row may fail before a drawing is given up (ComfyUI crashed or was closed).
const LOST_AFTER: u32 = 3;

/// How long Castle waits for ComfyUI to answer after running the member's start command. A
/// ComfyUI with many custom nodes can take a minute or two to come up.
pub const START_LIMIT: Duration = Duration::from_secs(180);

/// What the member told Castle about their ComfyUI. Off until they switch it on. Changed only by
/// its own command (`comfy_save`), never by the Settings page.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComfySettings {
    pub on: bool,
    /// Where ComfyUI listens; empty means the usual local one (`DEFAULT_ADDRESS`).
    pub address: String,
    /// What starts ComfyUI: the path of a start file (.bat, .cmd, .sh) or a command line. Castle
    /// runs it only when nothing answers at the address. Empty means the member starts ComfyUI.
    pub start: String,
    /// The checkpoint to draw with; empty means the first one ComfyUI lists.
    pub model: String,
}

impl ComfySettings {
    pub fn server(&self) -> Result<Server, String> {
        parse_address(&self.address)
    }
}

/// The member's choices, checked and tidied before they are kept.
pub fn clean(on: bool, address: &str, start: &str, model: &str) -> Result<ComfySettings, String> {
    let address = match address.trim() {
        "" => String::new(),
        text => parse_address(text)?.address().to_string(),
    };
    let one_line = |text: &str, what: &str, most: usize| -> Result<String, String> {
        let text = text.trim();
        if text.chars().any(char::is_control) {
            return Err(format!("The {what} must be on one line."));
        }
        if text.chars().count() > most {
            return Err(format!("The {what} is too long."));
        }
        Ok(text.to_string())
    };
    Ok(ComfySettings { on, address, start: one_line(start, "start command", 1000)?, model: one_line(model, "checkpoint name", 300)? })
}

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

/// The ComfyUI Castle itself started, if it did: the one thing Castle may end, when Castle ends.
#[derive(Default)]
pub struct Launcher(Mutex<Option<Child>>);

impl Launcher {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<Child>> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether the start command Castle ran is still going.
    pub fn running(&self) -> bool {
        self.slot().as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    /// The exit code of a start command that ended badly (not 0), once.
    fn failed(&self) -> Option<i32> {
        let mut slot = self.slot();
        let status = slot.as_mut()?.try_wait().ok()??;
        (!status.success()).then(|| status.code().unwrap_or(-1))
    }
}

/// Ends the ComfyUI Castle started, with everything it started. A ComfyUI the member started is
/// never touched.
pub fn stop_started(launcher: &Launcher) {
    if let Some(mut child) = launcher.slot().take() {
        if matches!(child.try_wait(), Ok(None)) {
            crate::providers::end_process_tree(&mut child);
        }
    }
}

/// Castle is closing: ends the ComfyUI it started.
pub fn stop_with_castle(app: &AppHandle) {
    stop_started(&app.state::<Launcher>());
}

/// The member's start command as a process. A start file given by its full path runs from its own
/// folder (a ComfyUI start file reaches its Python by a path relative to itself); anything else is a
/// command line for the computer's own shell.
fn start_process(start: &str) -> Command {
    let plain = start.trim().trim_matches('"');
    let file = Path::new(plain);
    let mut command = if file.is_absolute() && file.is_file() {
        let mut command = if !cfg!(windows) && file.extension().is_some_and(|e| e.eq_ignore_ascii_case("sh")) {
            let mut command = Command::new("sh");
            command.arg(file);
            command
        } else {
            Command::new(file)
        };
        if let Some(folder) = file.parent() {
            command.current_dir(folder);
        }
        command
    } else {
        shell_line(start.trim())
    };
    for name in crate::env::NEVER_INHERITED {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

#[cfg(windows)]
fn shell_line(line: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("cmd");
    command.arg("/C").raw_arg(line);
    command
}

#[cfg(not(windows))]
fn shell_line(line: &str) -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", line]);
    command
}

/// Runs the start command, its words going to `log` so the member can see why ComfyUI did not come.
fn spawn(start: &str, log: &Path) -> Result<Child, String> {
    let file = std::fs::File::create(log).map_err(|_| "Castle could not open its log for the start command.".to_string())?;
    let more = file.try_clone().map_err(|_| "Castle could not open its log for the start command.".to_string())?;
    start_process(start)
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(more)
        .spawn()
        .map_err(|_| "The start command could not run. Check the path in Settings.".to_string())
}

/// Makes sure a ComfyUI answers at `server`: one that already does is left alone; otherwise the
/// member's start command is run (once, however many drawings wait) and waited for up to `limit`.
/// `stop` is asked between looks.
pub fn ensure_up(server: &Server, start: &str, launcher: &Launcher, log: &Path, limit: Duration, poll: Duration, stop: &dyn Fn() -> bool) -> Result<(), String> {
    if server.ping().is_ok() {
        return Ok(());
    }
    let start = start.trim();
    if start.is_empty() {
        return Err(format!("{} Add a start command in Settings and Castle can start it for you.", server.down()));
    }
    {
        let mut slot = launcher.slot();
        let going = slot.as_mut().is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        if !going {
            *slot = Some(spawn(start, log)?);
        }
    }
    let began = Instant::now();
    loop {
        if stop() {
            return Err("Stopped.".into());
        }
        if server.ping().is_ok() {
            return Ok(());
        }
        if let Some(code) = launcher.failed() {
            return Err(format!("The start command ended with code {code} before ComfyUI answered. Its words are in {}.", log.display()));
        }
        if began.elapsed() >= limit {
            return Err(format!("ComfyUI did not answer within {} seconds of the start command. Its words are in {}.", limit.as_secs(), log.display()));
        }
        std::thread::sleep(poll);
    }
}

/// What to draw: the member's words, and the size and quality a STUDIO workflow asked for.
pub struct Brief<'a> {
    pub prompt: &'a str,
    /// "1024x1024", "1024x1536" or "1536x1024"; anything else (or none) is the square one
    pub size: Option<&'a str>,
    /// "low", "medium" or "high"; anything else (or none) is medium
    pub quality: Option<&'a str>,
}

/// Said to a checkpoint that does not know our words: keeps the plainest faults out.
const NEGATIVE: &str = "blurry, low quality, deformed, watermark, text";

/// Width and height in pixels. Castle's three sizes are all comfortable for an SDXL-class checkpoint.
pub fn dimensions(size: Option<&str>) -> (u32, u32) {
    match size {
        Some("1024x1536") => (1024, 1536),
        Some("1536x1024") => (1536, 1024),
        _ => (1024, 1024),
    }
}

/// How many sampling steps a quality stands for: ComfyUI draws for free, so quality is time.
pub fn steps_for(quality: Option<&str>) -> u32 {
    match quality {
        Some("low") => 12,
        Some("high") => 30,
        _ => 20,
    }
}

/// The plain text-to-picture workflow every ComfyUI can run: load the checkpoint, encode the words
/// and the faults to avoid, sample, decode, save. Only core nodes, so no custom node is needed.
pub fn txt2img_graph(checkpoint: &str, prompt: &str, width: u32, height: u32, seed: u64, steps: u32) -> Value {
    json!({
        "1": { "class_type": "CheckpointLoaderSimple", "inputs": { "ckpt_name": checkpoint } },
        "2": { "class_type": "CLIPTextEncode", "inputs": { "text": prompt, "clip": ["1", 1] } },
        "3": { "class_type": "CLIPTextEncode", "inputs": { "text": NEGATIVE, "clip": ["1", 1] } },
        "4": { "class_type": "EmptyLatentImage", "inputs": { "width": width, "height": height, "batch_size": 1 } },
        "5": { "class_type": "KSampler", "inputs": { "model": ["1", 0], "positive": ["2", 0], "negative": ["3", 0], "latent_image": ["4", 0], "seed": seed, "steps": steps, "cfg": 6.0, "sampler_name": "euler", "scheduler": "normal", "denoise": 1.0 } },
        "6": { "class_type": "VAEDecode", "inputs": { "samples": ["5", 0], "vae": ["1", 2] } },
        "7": { "class_type": "SaveImage", "inputs": { "images": ["6", 0], "filename_prefix": "castle" } }
    })
}

/// The checkpoint to draw with: the one the member named, else the first ComfyUI lists. A name
/// ComfyUI does not have is said so, with what it does have.
pub fn choose_checkpoint(wanted: &str, have: &[String]) -> Result<String, String> {
    let wanted = wanted.trim();
    if have.is_empty() {
        return Err("This ComfyUI lists no checkpoint to draw with. Put one in its checkpoints folder.".into());
    }
    if wanted.is_empty() {
        return Ok(have[0].clone());
    }
    if have.iter().any(|name| name == wanted) {
        return Ok(wanted.to_string());
    }
    let shown: Vec<&str> = have.iter().take(5).map(String::as_str).collect();
    Err(format!("ComfyUI does not have the checkpoint {wanted}. It has: {}{}.", shown.join(", "), if have.len() > 5 { ", ..." } else { "" }))
}

/// A number that differs every time, for the sampler's seed and the request's name.
fn fresh_number() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

/// Draws one picture with the member's own ComfyUI and saves it at `output`: makes sure ComfyUI is
/// there (running the start command if it is not), queues the plain workflow, waits up to `limit`
/// and keeps the first saved picture. `stop` is asked all the while; a Stop cancels the workflow.
pub fn draw(settings: &ComfySettings, launcher: &Launcher, log: &Path, brief: &Brief, output: &Path, limit: Duration, stop: &dyn Fn() -> bool) -> Result<(), String> {
    let server = settings.server()?;
    ensure_up(&server, &settings.start, launcher, log, START_LIMIT, Duration::from_secs(1), stop)?;
    let checkpoint = choose_checkpoint(&settings.model, &server.checkpoints()?)?;
    let (width, height) = dimensions(brief.size);
    let graph = txt2img_graph(&checkpoint, brief.prompt, width, height, fresh_number(), steps_for(brief.quality));
    let id = server.submit(&graph, &format!("castle-{}", fresh_number()))?;
    let pictures = server.wait(&id, limit, Duration::from_secs(1), stop)?;
    let bytes = server.fetch(&pictures[0])?;
    if let Some(folder) = output.parent() {
        std::fs::create_dir_all(folder).map_err(|_| "The picture's folder could not be made.".to_string())?;
    }
    // whole or not at all: a half-written picture must never look like a drawing
    let part = output.with_extension("png.part");
    std::fs::write(&part, &bytes).map_err(|_| "The picture could not be saved.".to_string())?;
    std::fs::rename(&part, output).map_err(|_| {
        let _ = std::fs::remove_file(&part);
        "The picture could not be saved.".to_string()
    })
}

fn start_log(store: &SettingsStore) -> std::path::PathBuf {
    store.folder().join("comfy-start.log")
}

#[tauri::command]
pub fn comfy_get(store: State<SettingsStore>) -> ComfySettings {
    store.get().comfy
}

/// Keeps the member's choices (after checking them). The Settings page never overwrites them.
#[tauri::command]
pub fn comfy_save(store: State<SettingsStore>, on: bool, address: String, start: String, model: String) -> Result<ComfySettings, String> {
    let comfy = clean(on, &address, &start, &model)?;
    let mut next = store.get();
    next.comfy = comfy.clone();
    crate::settings::save(&store.path, &next).map_err(|_| "The ComfyUI settings could not be saved.".to_string())?;
    *store.current.lock().unwrap() = next;
    Ok(comfy)
}

/// What the Settings page shows about the connection.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub up: bool,
    pub address: String,
    pub checkpoints: Vec<String>,
    /// ComfyUI is running because Castle ran the start command
    pub started_by_castle: bool,
    /// one plain line for the member
    pub message: String,
}

fn status_of(server: &Server, launcher: &Launcher) -> Status {
    let started_by_castle = launcher.running();
    match server.ping().and_then(|_| server.checkpoints()) {
        Ok(checkpoints) => {
            let message = match checkpoints.len() {
                0 => format!("ComfyUI answers at {}, but it lists no checkpoint.", server.address()),
                1 => format!("ComfyUI answers at {} with 1 checkpoint.", server.address()),
                n => format!("ComfyUI answers at {} with {n} checkpoints.", server.address()),
            };
            Status { up: true, address: server.address().into(), checkpoints, started_by_castle, message }
        }
        Err(message) => Status { up: false, address: server.address().into(), checkpoints: Vec::new(), started_by_castle, message },
    }
}

/// Asks the saved address whether ComfyUI is there and which checkpoints it has. Never starts it.
#[tauri::command]
pub async fn comfy_check(app: AppHandle) -> Result<Status, String> {
    let server = app.state::<SettingsStore>().get().comfy.server()?;
    tauri::async_runtime::spawn_blocking(move || status_of(&server, &app.state::<Launcher>())).await.map_err(|_| "The check stopped before it finished.".to_string())
}

/// Runs the start command if nothing answers (waiting for ComfyUI to come up), then checks.
#[tauri::command]
pub async fn comfy_start(app: AppHandle) -> Result<Status, String> {
    let comfy = app.state::<SettingsStore>().get().comfy;
    let server = comfy.server()?;
    let log = start_log(&app.state::<SettingsStore>());
    tauri::async_runtime::spawn_blocking(move || {
        let launcher = app.state::<Launcher>();
        ensure_up(&server, &comfy.start, &launcher, &log, START_LIMIT, Duration::from_secs(1), &|| false)?;
        Ok(status_of(&server, &launcher))
    })
    .await
    .map_err(|_| "The start stopped before it finished.".to_string())?
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

    fn nothing_listens() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        parse_address(&base).unwrap()
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("castle-comfy-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// A ComfyUI that begins answering once `flag` exists.
    fn comfy_after(flag: std::path::PathBuf) -> Fake {
        Fake::new(move |_| if flag.exists() { ok(json!({ "system": {} })) } else { (404, b"no".to_vec()) })
    }

    #[cfg(windows)]
    fn make_file(flag: &Path) -> String {
        format!("echo ready> \"{}\"", flag.display())
    }
    #[cfg(not(windows))]
    fn make_file(flag: &Path) -> String {
        format!("touch '{}'", flag.display())
    }
    #[cfg(windows)]
    const LONG: &str = "ping -n 30 127.0.0.1 > nul";
    #[cfg(not(windows))]
    const LONG: &str = "sleep 30";

    #[test]
    fn the_members_choices_are_checked_and_tidied() {
        let kept = clean(true, " 192.168.1.20:8188/ ", "  W:\\Comfy\\start.bat ", " model.safetensors ").unwrap();
        assert_eq!(kept, ComfySettings { on: true, address: "http://192.168.1.20:8188".into(), start: "W:\\Comfy\\start.bat".into(), model: "model.safetensors".into() });
        assert_eq!(clean(false, "", "", "").unwrap(), ComfySettings::default());
        assert!(clean(true, "http://127.0.0.1:8188/prompt", "", "").unwrap_err().contains("nothing after it"));
        assert!(clean(true, "", "a\nb", "").unwrap_err().contains("one line"));
        assert!(clean(true, "", "", "a\0b").unwrap_err().contains("one line"));
        assert!(clean(true, "", &"x".repeat(1001), "").unwrap_err().contains("too long"));
        assert!(clean(true, "", "", &"x".repeat(301)).unwrap_err().contains("too long"));
    }

    #[test]
    fn an_older_settings_file_has_comfyui_off_and_the_choices_survive_a_save() {
        let old: crate::settings::Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.comfy, ComfySettings::default());
        assert!(!old.comfy.on);
        let read: ComfySettings = serde_json::from_str(r#"{"on":true,"start":"x.bat"}"#).unwrap();
        assert!(read.on && read.address.is_empty() && read.start == "x.bat");
        let mut settings = crate::settings::Settings::default();
        settings.comfy = clean(true, "localhost:9000", "start.cmd", "m.safetensors").unwrap();
        let again: crate::settings::Settings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(again.comfy, settings.comfy);
        assert_eq!(again.comfy.server().unwrap().address(), "http://localhost:9000");
        assert_eq!(ComfySettings::default().server().unwrap().address(), DEFAULT_ADDRESS);
    }

    #[test]
    fn a_comfyui_that_is_already_there_is_left_alone() {
        let comfy = Fake::new(|_| ok(json!({ "system": {} })));
        let launcher = Launcher::default();
        let log = temp("up").join("start.log");
        // a start command that would fail loudly if it ever ran
        ensure_up(&comfy.server, "exit 9", &launcher, &log, Duration::from_secs(2), Duration::from_millis(5), &no_stop).unwrap();
        assert!(!launcher.running());
        assert!(!log.exists(), "nothing was started, so nothing was logged");
    }

    #[test]
    fn nothing_there_and_no_start_command_says_what_to_add() {
        let said = ensure_up(&nothing_listens(), "  ", &Launcher::default(), &temp("none").join("l.log"), Duration::from_secs(1), Duration::from_millis(5), &no_stop).unwrap_err();
        assert!(said.contains("Is it running?") && said.contains("start command"));
    }

    #[test]
    fn the_start_command_runs_and_castle_waits_until_comfyui_answers() {
        let folder = temp("start");
        let flag = folder.join("ready.txt");
        let comfy = comfy_after(flag.clone());
        let launcher = Launcher::default();
        ensure_up(&comfy.server, &make_file(&flag), &launcher, &folder.join("start.log"), Duration::from_secs(20), Duration::from_millis(20), &no_stop).unwrap();
        assert!(flag.exists());
        stop_started(&launcher);
    }

    #[test]
    fn a_start_file_runs_from_its_own_folder() {
        let folder = temp("folder");
        // ComfyUI's own start file reaches its Python by a path relative to itself
        #[cfg(windows)]
        let (file, body) = (folder.join("start comfy.cmd"), "@echo %CD%> here.txt\r\n");
        #[cfg(not(windows))]
        let (file, body) = (folder.join("start comfy.sh"), "pwd > here.txt\n");
        std::fs::write(&file, body).unwrap();
        let here = folder.join("here.txt");
        let comfy = comfy_after(here.clone());
        let launcher = Launcher::default();
        ensure_up(&comfy.server, &format!("\"{}\"", file.display()), &launcher, &folder.join("start.log"), Duration::from_secs(20), Duration::from_millis(20), &no_stop).unwrap();
        let wrote = std::fs::read_to_string(&here).unwrap();
        let name = folder.file_name().unwrap().to_string_lossy().to_lowercase();
        assert!(wrote.trim().to_lowercase().ends_with(&name), "it ran in {wrote:?}, not in the start file's folder");
        stop_started(&launcher);
    }

    #[test]
    fn a_start_command_that_fails_is_reported_with_its_code_at_once() {
        let folder = temp("fails");
        let began = Instant::now();
        let said = ensure_up(&nothing_listens(), "exit 3", &Launcher::default(), &folder.join("start.log"), Duration::from_secs(30), Duration::from_millis(20), &no_stop).unwrap_err();
        assert!(said.contains("code 3") && said.contains("start.log"), "{said}");
        assert!(began.elapsed() < Duration::from_secs(10), "it did not wait out the limit");
    }

    #[test]
    fn a_comfyui_that_never_comes_is_given_up_on_and_a_stop_is_honoured() {
        let folder = temp("slow");
        let launcher = Launcher::default();
        let said = ensure_up(&nothing_listens(), LONG, &launcher, &folder.join("start.log"), Duration::from_millis(300), Duration::from_millis(20), &no_stop).unwrap_err();
        assert!(said.contains("did not answer within") && said.contains("start.log"), "{said}");
        // the command is still going (a slow ComfyUI may yet come); asking again does not run it twice
        assert!(launcher.running());
        let looks = AtomicUsize::new(0);
        let stop = || looks.fetch_add(1, Ordering::SeqCst) >= 2;
        assert_eq!(ensure_up(&nothing_listens(), LONG, &launcher, &folder.join("start.log"), Duration::from_secs(30), Duration::from_millis(20), &stop).unwrap_err(), "Stopped.");
        assert!(launcher.running());
        // quitting Castle ends what Castle started, and only that
        stop_started(&launcher);
        assert!(!launcher.running());
    }

    /// A ComfyUI that holds `checkpoints` and draws `picture` for whatever it is asked.
    fn comfy_that_draws(checkpoints: &[&str], picture: Vec<u8>) -> Fake {
        let names: Vec<String> = checkpoints.iter().map(|c| c.to_string()).collect();
        Fake::new(move |request| {
            if request.starts_with("GET /system_stats ") {
                ok(json!({ "system": {} }))
            } else if request.starts_with("GET /object_info/CheckpointLoaderSimple ") {
                ok(json!({ "CheckpointLoaderSimple": { "input": { "required": { "ckpt_name": [names, {}] } } } }))
            } else if request.starts_with("POST /prompt ") {
                ok(json!({ "prompt_id": "p1", "node_errors": {} }))
            } else if request.starts_with("GET /history/p1 ") {
                ok(done("p1", json!({ "7": { "images": [{ "filename": "castle_00001_.png", "subfolder": "", "type": "output" }] } })))
            } else if request.starts_with("GET /view?") {
                (200, picture.clone())
            } else {
                ok(json!({}))
            }
        })
    }

    fn settings_for(fake: &Fake, model: &str) -> ComfySettings {
        ComfySettings { on: true, address: fake.server.address().into(), start: String::new(), model: model.into() }
    }

    fn sent_graph(fake: &Fake) -> Value {
        let body = fake.asked().into_iter().find(|r| r.starts_with("POST /prompt ")).expect("nothing was queued");
        serde_json::from_str::<Value>(body.split("\r\n\r\n").nth(1).unwrap()).unwrap()["prompt"].clone()
    }

    #[test]
    fn sizes_and_quality_become_pixels_and_steps() {
        assert_eq!(dimensions(None), (1024, 1024));
        assert_eq!(dimensions(Some("1024x1024")), (1024, 1024));
        assert_eq!(dimensions(Some("1024x1536")), (1024, 1536));
        assert_eq!(dimensions(Some("1536x1024")), (1536, 1024));
        assert_eq!(dimensions(Some("99x99")), (1024, 1024));
        assert_eq!((steps_for(Some("low")), steps_for(Some("medium")), steps_for(Some("high")), steps_for(None), steps_for(Some("odd"))), (12, 20, 30, 20, 20));
    }

    #[test]
    fn the_plain_workflow_holds_the_choices_and_every_link_points_at_a_real_node() {
        let graph = txt2img_graph("m.safetensors", "a lantern", 1024, 1536, 42, 30);
        assert_eq!(graph["1"]["inputs"]["ckpt_name"], "m.safetensors");
        assert_eq!(graph["2"]["inputs"]["text"], "a lantern");
        assert_eq!(graph["4"]["inputs"]["width"], 1024);
        assert_eq!(graph["4"]["inputs"]["height"], 1536);
        assert_eq!(graph["5"]["inputs"]["seed"], 42);
        assert_eq!(graph["5"]["inputs"]["steps"], 30);
        assert_eq!(graph["7"]["class_type"], "SaveImage");
        // every ["node", slot] in the graph names a node that exists
        for (_, node) in graph.as_object().unwrap() {
            for (_, input) in node["inputs"].as_object().unwrap() {
                if let Some([target, _]) = input.as_array().map(Vec::as_slice) {
                    assert!(graph.get(target.as_str().unwrap()).is_some(), "a link to a missing node: {target}");
                }
            }
        }
    }

    #[test]
    fn the_checkpoint_is_the_named_one_else_the_first_and_a_wrong_name_lists_what_there_is() {
        let have: Vec<String> = ["a.safetensors", "b.ckpt"].iter().map(|s| s.to_string()).collect();
        assert_eq!(choose_checkpoint("", &have).unwrap(), "a.safetensors");
        assert_eq!(choose_checkpoint(" b.ckpt ", &have).unwrap(), "b.ckpt");
        let said = choose_checkpoint("c.ckpt", &have).unwrap_err();
        assert!(said.contains("does not have the checkpoint c.ckpt") && said.contains("a.safetensors, b.ckpt"), "{said}");
        assert!(choose_checkpoint("", &[]).unwrap_err().contains("no checkpoint"));
        let many: Vec<String> = (0..8).map(|n| format!("m{n}.safetensors")).collect();
        assert!(choose_checkpoint("x", &many).unwrap_err().ends_with("m4.safetensors, ...."));
    }

    #[test]
    fn a_picture_is_drawn_and_saved_where_it_was_asked() {
        let png = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
        let comfy = comfy_that_draws(&["a.safetensors"], png.clone());
        let out = temp("draw").join("deep").join("castle-1.png");
        let brief = Brief { prompt: "a red lantern", size: Some("1536x1024"), quality: Some("high") };
        draw(&settings_for(&comfy, ""), &Launcher::default(), &out.with_extension("log"), &brief, &out, Duration::from_secs(5), &no_stop).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), png);
        assert!(!out.with_extension("png.part").exists());
        let graph = sent_graph(&comfy);
        assert_eq!(graph["1"]["inputs"]["ckpt_name"], "a.safetensors");
        assert_eq!(graph["2"]["inputs"]["text"], "a red lantern");
        assert_eq!((graph["4"]["inputs"]["width"].clone(), graph["4"]["inputs"]["height"].clone()), (json!(1536), json!(1024)));
        assert_eq!(graph["5"]["inputs"]["steps"], 30);
    }

    #[test]
    fn the_members_checkpoint_is_used_and_a_missing_one_draws_nothing() {
        let comfy = comfy_that_draws(&["a.safetensors", "b.safetensors"], vec![0x89, b'P', b'N', b'G']);
        let out = temp("model").join("p.png");
        let brief = Brief { prompt: "x", size: None, quality: None };
        draw(&settings_for(&comfy, "b.safetensors"), &Launcher::default(), &out.with_extension("log"), &brief, &out, Duration::from_secs(5), &no_stop).unwrap();
        assert_eq!(sent_graph(&comfy)["1"]["inputs"]["ckpt_name"], "b.safetensors");

        let other = comfy_that_draws(&["a.safetensors"], vec![1]);
        let out = temp("model2").join("p.png");
        let said = draw(&settings_for(&other, "zzz"), &Launcher::default(), &out.with_extension("log"), &brief, &out, Duration::from_secs(5), &no_stop).unwrap_err();
        assert!(said.contains("does not have the checkpoint zzz"));
        assert!(!out.exists());
        assert!(!other.asked().iter().any(|r| r.starts_with("POST /prompt ")), "nothing may be queued with a wrong checkpoint");
    }

    #[test]
    fn a_comfyui_that_is_not_there_and_cannot_be_started_draws_nothing() {
        let settings = ComfySettings { on: true, address: nothing_listens().address().into(), start: String::new(), model: String::new() };
        let out = temp("down").join("p.png");
        let brief = Brief { prompt: "x", size: None, quality: None };
        let said = draw(&settings, &Launcher::default(), &out.with_extension("log"), &brief, &out, Duration::from_secs(5), &no_stop).unwrap_err();
        assert!(said.contains("Is it running?") && said.contains("start command"), "{said}");
        assert!(!out.exists());
    }

    #[test]
    fn stop_while_it_draws_cancels_the_job_and_saves_nothing() {
        let comfy = Fake::new(|request| {
            if request.starts_with("GET /system_stats ") {
                ok(json!({ "system": {} }))
            } else if request.starts_with("GET /object_info/") {
                ok(json!({ "CheckpointLoaderSimple": { "input": { "required": { "ckpt_name": [["a.safetensors"], {}] } } } }))
            } else if request.starts_with("POST /prompt ") {
                ok(json!({ "prompt_id": "p1" }))
            } else if request.starts_with("GET /queue ") {
                ok(json!({ "queue_running": [[0, "p1", {}, {}, []]] }))
            } else {
                ok(json!({}))
            }
        });
        let out = temp("stop").join("p.png");
        let brief = Brief { prompt: "x", size: None, quality: None };
        let looks = AtomicUsize::new(0);
        let stop = || looks.fetch_add(1, Ordering::SeqCst) >= 4;
        let said = draw(&settings_for(&comfy, ""), &Launcher::default(), &out.with_extension("log"), &brief, &out, Duration::from_secs(60), &stop).unwrap_err();
        assert_eq!(said, "Stopped.");
        assert!(!out.exists());
        assert!(comfy.asked().iter().any(|r| r.starts_with("POST /interrupt ")));
    }

    /// The whole drawing path against the member's real ComfyUI, as Castle's worker runs it: starts
    /// it when it is not running (CASTLE_COMFY_START), draws a plain picture with the checkpoint
    /// (CASTLE_COMFY_MODEL, default the first) and saves it; ends ComfyUI again only if Castle started it:
    /// `cargo test --manifest-path src-tauri/Cargo.toml comfy::tests::live_draw -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_draw_with_the_members_comfyui_from_a_cold_start() {
        let settings = ComfySettings {
            on: true,
            address: std::env::var("CASTLE_COMFY_ADDRESS").unwrap_or_default(),
            start: std::env::var("CASTLE_COMFY_START").unwrap_or_default(),
            model: std::env::var("CASTLE_COMFY_MODEL").unwrap_or_default(),
        };
        let launcher = Launcher::default();
        let out = std::env::temp_dir().join(format!("castle-comfy-draw-{}.png", std::process::id()));
        let log = std::env::temp_dir().join("castle-comfy-live-start.log");
        let brief = Brief { prompt: "a small red lantern on a wooden table, soft light, photo", size: Some("1024x1024"), quality: Some("medium") };
        let began = Instant::now();
        let result = draw(&settings, &launcher, &log, &brief, &out, Duration::from_secs(900), &no_stop);
        println!("took {:.0} s, started by Castle: {}, result: {result:?}", began.elapsed().as_secs_f32(), launcher.running());
        stop_started(&launcher);
        result.unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "not a PNG");
        println!("saved {} ({} bytes)", out.display(), bytes.len());
    }

    /// Needs the member's own start file: CASTLE_COMFY_START (a path or command) with ComfyUI not
    /// running at CASTLE_COMFY_ADDRESS (default http://127.0.0.1:8188). Starts it, checks it, ends it:
    /// `cargo test --manifest-path src-tauri/Cargo.toml comfy::tests::live_castle_starts -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_castle_starts_the_members_comfyui_and_ends_it_again() {
        let server = parse_address(&std::env::var("CASTLE_COMFY_ADDRESS").unwrap_or_default()).unwrap();
        let start = std::env::var("CASTLE_COMFY_START").expect("set CASTLE_COMFY_START to the start file");
        assert!(server.ping().is_err(), "ComfyUI is already running; close it first, this test starts it");
        let launcher = Launcher::default();
        let log = std::env::temp_dir().join("castle-comfy-live-start.log");
        let began = Instant::now();
        ensure_up(&server, &start, &launcher, &log, START_LIMIT, Duration::from_secs(1), &no_stop).unwrap();
        println!("answered {:.0} s after the start command; running by Castle: {}", began.elapsed().as_secs_f32(), launcher.running());
        let status = status_of(&server, &launcher);
        println!("{status:?}");
        assert!(status.up && status.started_by_castle);
        stop_started(&launcher);
        std::thread::sleep(Duration::from_secs(4));
        println!("after ending it, ComfyUI answers: {}", server.ping().is_ok());
        assert!(server.ping().is_err(), "ComfyUI is still running after Castle ended it");
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
