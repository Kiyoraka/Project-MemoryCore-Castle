//! The member's own ComfyUI as a way to draw (Settings > ComfyUI). A small client for a ComfyUI
//! server they run themselves, on their own PC or another one on their network: is it there, which
//! checkpoints does it hold, queue a workflow, wait for the picture, fetch it, cancel it, over
//! ComfyUI's own HTTP API. On top of it: the choices the member made (`ComfySettings`), starting
//! ComfyUI with their start file when nothing answers (and closing only what Castle started), Castle's
//! plain text-to-picture workflow, a workflow of the member's own with Castle's values put in, and
//! `draw`, which the picture queue (providers.rs) calls for a plain picture.

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
    /// The checkpoint to draw with; empty means the first one ComfyUI lists. Ignored with a
    /// workflow of the member's own, which names its own models.
    pub model: String,
    /// The member's own workflow: a file saved from ComfyUI as "API format" with `{{prompt}}` (and
    /// optionally `{{width}}`, `{{height}}`, `{{seed}}`, `{{steps}}`) where Castle's values go.
    /// Empty means Castle's plain built-in workflow.
    pub workflow: String,
    /// How long one drawing may take before Castle stops waiting; 0 means the usual
    /// `DEFAULT_LIMIT_MINUTES`. A big model on a modest card can take minutes.
    pub limit_minutes: u32,
}

/// How long a drawing may take when the member has not said.
pub const DEFAULT_LIMIT_MINUTES: u32 = 15;
/// The longest limit a member may set.
const LIMIT_MINUTES_MAX: u32 = 240;

impl ComfySettings {
    pub fn server(&self) -> Result<Server, String> {
        parse_address(&self.address)
    }

    /// How long one drawing may take.
    pub fn limit(&self) -> Duration {
        Duration::from_secs(60 * u64::from(if self.limit_minutes == 0 { DEFAULT_LIMIT_MINUTES } else { self.limit_minutes }))
    }
}

/// The member's choices, checked and tidied before they are kept. (The workflow file itself is
/// read and checked by `comfy_save`, which has the disk.)
pub fn clean(on: bool, address: &str, start: &str, model: &str, workflow: &str, limit_minutes: u32) -> Result<ComfySettings, String> {
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
    if limit_minutes > LIMIT_MINUTES_MAX {
        return Err(format!("The time limit can be at most {LIMIT_MINUTES_MAX} minutes."));
    }
    Ok(ComfySettings {
        on,
        address,
        start: one_line(start, "start command", 1000)?,
        model: one_line(model, "checkpoint name", 300)?,
        workflow: one_line(workflow, "workflow file path", 1000)?,
        limit_minutes,
    })
}

/// The most a workflow file may weigh (a workflow is a few kilobytes; this is only a guard).
const WORKFLOW_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Reads the member's workflow file and checks it is something Castle can fill and ComfyUI can run.
/// Nothing here knows ComfyUI's node names: a custom node of the member's own is welcome.
pub fn read_workflow(path: &str) -> Result<Value, String> {
    let path = path.trim().trim_matches('"');
    if path.is_empty() {
        return Err("No workflow file is chosen.".into());
    }
    let file = Path::new(path);
    let size = std::fs::metadata(file).map_err(|_| format!("The workflow file {path} could not be found."))?.len();
    if size > WORKFLOW_MAX_BYTES {
        return Err("The workflow file is too big to be a ComfyUI workflow.".into());
    }
    let text = std::fs::read_to_string(file).map_err(|_| format!("The workflow file {path} could not be read."))?;
    parse_workflow(&text)
}

/// A workflow's text as the graph ComfyUI takes, or why it is not one.
pub fn parse_workflow(text: &str) -> Result<Value, String> {
    let mut graph: Value = serde_json::from_str(text).map_err(|_| "The workflow file is not valid JSON.".to_string())?;
    if graph.get("nodes").is_some_and(Value::is_array) {
        return Err("This workflow was saved for ComfyUI's editor. In ComfyUI choose Workflow > Export (API) (older versions: Save (API Format)) and use that file.".into());
    }
    // some tools wrap the graph in {"prompt": {...}}
    if let Some(inner) = graph.get("prompt").filter(|inner| inner.is_object() && inner.get("class_type").is_none()).cloned() {
        graph = inner;
    }
    let nodes = graph.as_object().filter(|nodes| !nodes.is_empty()).ok_or("The workflow file is empty or is not a ComfyUI workflow.")?;
    if !nodes.values().all(|node| node.get("class_type").is_some_and(Value::is_string)) {
        return Err("The workflow file is not in ComfyUI's API format (every node needs a class_type). Use Workflow > Export (API).".into());
    }
    if !nodes.values().any(|node| node.get("class_type").and_then(Value::as_str).is_some_and(|class| class.contains("Save"))) {
        return Err("The workflow has no Save Image node, so ComfyUI would not keep a picture.".into());
    }
    if !placeholder_pattern().captures_iter(&graph.to_string()).any(|found| &found[1] == "prompt") {
        return Err("Put {{prompt}} in the workflow where the words to draw go (the text of your prompt node), then save it again.".into());
    }
    Ok(graph)
}

/// What Castle puts into a member's workflow.
pub struct Values<'a> {
    pub prompt: &'a str,
    pub width: u32,
    pub height: u32,
    pub seed: u64,
    pub steps: u32,
}

/// `{{prompt}}`, `{{ width }}` and so on.
fn placeholder_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| regex::Regex::new(r"\{\{\s*(prompt|width|height|seed|steps)\s*\}\}").expect("a fixed pattern"))
}

/// The workflow with Castle's values put in. A value that is only a placeholder becomes the number
/// (or the words) itself; a placeholder inside longer text becomes text. One pass, so a prompt that
/// happens to contain `{{width}}` is never filled again; other `{{...}}` are left alone.
pub fn fill(graph: &Value, values: &Values) -> Value {
    match graph {
        Value::String(text) => fill_text(text, values),
        Value::Array(items) => Value::Array(items.iter().map(|item| fill(item, values)).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(key, item)| (key.clone(), fill(item, values))).collect()),
        other => other.clone(),
    }
}

fn fill_text(text: &str, values: &Values) -> Value {
    let pattern = placeholder_pattern();
    let whole = pattern.captures(text).filter(|c| c.get(0).is_some_and(|m| m.as_str() == text.trim()));
    if let Some(found) = whole {
        return match &found[1] {
            "width" => json!(values.width),
            "height" => json!(values.height),
            "seed" => json!(values.seed),
            "steps" => json!(values.steps),
            _ => json!(values.prompt),
        };
    }
    Value::String(
        pattern
            .replace_all(text, |c: &regex::Captures| match &c[1] {
                "width" => values.width.to_string(),
                "height" => values.height.to_string(),
                "seed" => values.seed.to_string(),
                "steps" => values.steps.to_string(),
                _ => values.prompt.to_string(),
            })
            .into_owned(),
    )
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

    /// Whether Castle ran a start command at all (and so has a log of what it printed).
    fn has_child(&self) -> bool {
        self.slot().is_some()
    }

    /// How the start command Castle ran ended, if it has: its exit code (a crash of ComfyUI shows here).
    fn exit_code(&self) -> Option<i32> {
        self.slot().as_mut()?.try_wait().ok()??.code()
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

/// The log of the run before this one: `comfy-start.log` becomes `comfy-start.previous.log`.
fn previous_log(log: &Path) -> std::path::PathBuf {
    log.with_extension("previous.log")
}

/// Keeps what the last run printed before a new run replaces the log: when ComfyUI dies, its last
/// words (a crash message goes to the error output) are the only clue, and the restart that follows
/// a failed drawing would otherwise wipe them.
fn rotate_log(log: &Path) {
    if log.is_file() {
        let previous = previous_log(log);
        let _ = std::fs::remove_file(&previous);
        let _ = std::fs::rename(log, previous);
    }
}

/// The error for a ComfyUI that stopped answering mid-drawing, with what Castle can add when it
/// started that ComfyUI itself: how the start command ended (a crash of the program shows as an
/// exit code, often 0xC0000005 or another 0xC... code on Windows) and where its last words are.
fn ended_words(said: String, code: Option<i32>, started: bool, log: &Path) -> String {
    if !said.contains("stopped answering") || !started {
        return said;
    }
    let previous = previous_log(log);
    match code {
        Some(code) => {
            let shown = if code < 0 { format!("{code}, 0x{:08X}", code as u32) } else { code.to_string() };
            format!("ComfyUI closed while it was drawing (exit code {shown}). What it printed last is in {}; the run before is in {}.", log.display(), previous.display())
        }
        None => format!("{said} It may have crashed or frozen. What it printed last is in {}.", log.display()),
    }
}

/// The words to draw from a STUDIO brief. STUDIO adds "Size 1024x1024, medium quality." to the end
/// of a workflow's brief for the ways that read it (Codex); a picture model would try to draw those
/// words, and the size and quality are sent on their own anyway.
pub fn words_of<'a>(brief: &'a str, size: Option<&str>, quality: Option<&str>) -> &'a str {
    if let (Some(size), Some(quality)) = (size, quality) {
        if let Some(words) = brief.strip_suffix(&format!("\n\nSize {size}, {quality} quality.")) {
            return words.trim_end();
        }
    }
    brief
}

/// Runs the start command, its words going to `log` so the member can see why ComfyUI did not come.
fn spawn(start: &str, log: &Path) -> Result<Child, String> {
    rotate_log(log);
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
    // the member's own workflow is read first: a wrong file is said so before ComfyUI is started
    let own = if settings.workflow.trim().is_empty() { None } else { Some(read_workflow(&settings.workflow)?) };
    ensure_up(&server, &settings.start, launcher, log, START_LIMIT, Duration::from_secs(1), stop)?;
    let (width, height) = dimensions(brief.size);
    let (seed, steps) = (fresh_number(), steps_for(brief.quality));
    let words = words_of(brief.prompt, brief.size, brief.quality);
    let graph = match &own {
        Some(workflow) => fill(workflow, &Values { prompt: words, width, height, seed, steps }),
        // the built-in workflow needs a checkpoint; a workflow of the member's own names its models itself
        None => txt2img_graph(&choose_checkpoint(&settings.model, &server.checkpoints()?)?, words, width, height, seed, steps),
    };
    let id = server.submit(&graph, &format!("castle-{}", fresh_number()))?;
    let pictures = server.wait(&id, limit, Duration::from_secs(1), stop).map_err(|said| ended_words(said, launcher.exit_code(), launcher.has_child(), log))?;
    let bytes = as_png(server.fetch(&pictures[0])?)?;
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

/// The picture as PNG bytes: a PNG stays as it is; a JPEG or WebP (a member's own saver node may
/// make one) is turned into a PNG, since the file Castle keeps is named .png.
fn as_png(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Ok(bytes);
    }
    let picture = image::load_from_memory(&bytes).map_err(|_| "ComfyUI made a file that is not a picture Castle can read.".to_string())?;
    let mut png = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut png, image::ImageFormat::Png).map_err(|_| "The picture could not be turned into a PNG.".to_string())?;
    Ok(png.into_inner())
}

/// A file picker for the Settings page: the member's workflow ("workflow": a .json) or the file
/// that starts ComfyUI ("start": .bat, .cmd, .sh or .command). None when they close it.
#[tauri::command]
pub async fn comfy_choose_file(app: AppHandle, kind: String) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file();
        let dialog = if kind == "workflow" {
            dialog.set_title("Choose your ComfyUI workflow").add_filter("ComfyUI workflow (API format)", &["json"])
        } else {
            dialog.set_title("Choose the file that starts ComfyUI").add_filter("Start files", &["bat", "cmd", "sh", "command"])
        };
        dialog.blocking_pick_file().and_then(|file| file.into_path().ok()).map(|path| path.display().to_string())
    })
    .await
    .ok()
    .flatten()
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
pub fn comfy_save(store: State<SettingsStore>, on: bool, address: String, start: String, model: String, workflow: String, limit_minutes: u32) -> Result<ComfySettings, String> {
    let comfy = clean(on, &address, &start, &model, &workflow, limit_minutes)?;
    // a workflow file that is not one is told now, not at the first drawing
    if !comfy.workflow.is_empty() {
        read_workflow(&comfy.workflow)?;
    }
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
    /// why the member's own workflow file cannot be used, when one is set and is not right
    pub workflow_problem: Option<String>,
}

fn status_of(server: &Server, launcher: &Launcher, workflow: &str) -> Status {
    let started_by_castle = launcher.running();
    let workflow_problem = if workflow.trim().is_empty() { None } else { read_workflow(workflow).err() };
    match server.ping().and_then(|_| server.checkpoints()) {
        Ok(checkpoints) => {
            let message = match checkpoints.len() {
                0 => format!("ComfyUI answers at {}, but it lists no checkpoint.", server.address()),
                1 => format!("ComfyUI answers at {} with 1 checkpoint.", server.address()),
                n => format!("ComfyUI answers at {} with {n} checkpoints.", server.address()),
            };
            Status { up: true, address: server.address().into(), checkpoints, started_by_castle, message, workflow_problem }
        }
        Err(message) => Status { up: false, address: server.address().into(), checkpoints: Vec::new(), started_by_castle, message, workflow_problem },
    }
}

/// Asks the saved address whether ComfyUI is there and which checkpoints it has. Never starts it.
#[tauri::command]
pub async fn comfy_check(app: AppHandle) -> Result<Status, String> {
    let comfy = app.state::<SettingsStore>().get().comfy;
    let server = comfy.server()?;
    tauri::async_runtime::spawn_blocking(move || status_of(&server, &app.state::<Launcher>(), &comfy.workflow)).await.map_err(|_| "The check stopped before it finished.".to_string())
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
        Ok(status_of(&server, &launcher, &comfy.workflow))
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
        let kept = clean(true, " 192.168.1.20:8188/ ", "  W:\\Comfy\\start.bat ", " model.safetensors ", " W:\\Comfy\\qwen.json ", 30).unwrap();
        assert_eq!(
            kept,
            ComfySettings { on: true, address: "http://192.168.1.20:8188".into(), start: "W:\\Comfy\\start.bat".into(), model: "model.safetensors".into(), workflow: "W:\\Comfy\\qwen.json".into(), limit_minutes: 30 }
        );
        assert_eq!(clean(false, "", "", "", "", 0).unwrap(), ComfySettings::default());
        assert!(clean(true, "http://127.0.0.1:8188/prompt", "", "", "", 0).unwrap_err().contains("nothing after it"));
        assert!(clean(true, "", "a\nb", "", "", 0).unwrap_err().contains("one line"));
        assert!(clean(true, "", "", "a\0b", "", 0).unwrap_err().contains("one line"));
        assert!(clean(true, "", "", "", "a\nb.json", 0).unwrap_err().contains("one line"));
        assert!(clean(true, "", &"x".repeat(1001), "", "", 0).unwrap_err().contains("too long"));
        assert!(clean(true, "", "", &"x".repeat(301), "", 0).unwrap_err().contains("too long"));
        assert!(clean(true, "", "", "", &"x".repeat(1001), 0).unwrap_err().contains("too long"));
        assert!(clean(true, "", "", "", "", 241).unwrap_err().contains("at most 240"));
    }

    #[test]
    fn an_older_settings_file_has_comfyui_off_and_the_choices_survive_a_save() {
        let old: crate::settings::Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.comfy, ComfySettings::default());
        assert!(!old.comfy.on);
        let read: ComfySettings = serde_json::from_str(r#"{"on":true,"start":"x.bat"}"#).unwrap();
        assert!(read.on && read.address.is_empty() && read.start == "x.bat");
        let mut settings = crate::settings::Settings::default();
        settings.comfy = clean(true, "localhost:9000", "start.cmd", "m.safetensors", "qwen.json", 20).unwrap();
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
        ComfySettings { on: true, address: fake.server.address().into(), model: model.into(), ..Default::default() }
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
        let settings = ComfySettings { on: true, address: nothing_listens().address().into(), ..Default::default() };
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

    /// A member's own workflow in API format, with nodes only their ComfyUI has (like a Qwen one).
    fn own_workflow() -> Value {
        json!({
            "unet": { "class_type": "UNETLoader", "inputs": { "unet_name": "qwen_image.safetensors" } },
            "text": { "class_type": "TextEncodeQwenImage21", "inputs": { "prompt": "{{prompt}}", "negative_prompt": "", "resolution": 1024 } },
            "latent": { "class_type": "EmptyLatentImage", "inputs": { "width": "{{width}}", "height": "{{height}}", "batch_size": 1 } },
            "sample": { "class_type": "KSampler", "inputs": { "seed": "{{seed}}", "steps": "{{steps}}", "cfg": 1.0 } },
            "save": { "class_type": "SaveImage", "inputs": { "filename_prefix": "own_{{width}}x{{height}}" } }
        })
    }

    fn own_file(name: &str, graph: &Value) -> String {
        let file = temp(name).join("workflow.json");
        std::fs::write(&file, graph.to_string()).unwrap();
        file.display().to_string()
    }

    #[test]
    fn a_members_own_workflow_is_accepted_with_custom_nodes_and_refused_in_plain_words_when_not_right() {
        assert!(parse_workflow(&own_workflow().to_string()).is_ok(), "a node only the member's ComfyUI has is welcome");
        // spaces inside the braces are fine
        assert!(parse_workflow(&own_workflow().to_string().replace("{{prompt}}", "{{ prompt }}")).is_ok());
        // some tools wrap the graph in {"prompt": ...}
        let wrapped = json!({ "prompt": own_workflow(), "client_id": "x" });
        assert_eq!(parse_workflow(&wrapped.to_string()).unwrap(), own_workflow());

        let editor = json!({ "nodes": [{ "id": 1, "type": "KSampler" }], "links": [] });
        assert!(parse_workflow(&editor.to_string()).unwrap_err().contains("Export (API)"));
        assert!(parse_workflow("not json").unwrap_err().contains("not valid JSON"));
        assert!(parse_workflow("{}").unwrap_err().contains("empty"));
        assert!(parse_workflow("[1,2]").unwrap_err().contains("empty or is not"));
        assert!(parse_workflow(r#"{"1":{"inputs":{}}}"#).unwrap_err().contains("API format"));

        let mut no_save = own_workflow();
        no_save.as_object_mut().unwrap().remove("save");
        assert!(parse_workflow(&no_save.to_string()).unwrap_err().contains("no Save Image"));
        let no_prompt = own_workflow().to_string().replace("{{prompt}}", "a fixed text");
        assert!(parse_workflow(&no_prompt).unwrap_err().contains("{{prompt}}"));
    }

    #[test]
    fn castles_values_go_into_a_members_workflow_with_their_types_in_one_pass() {
        let values = Values { prompt: "a \"quoted\"\nline with {{width}} in it", width: 1024, height: 1536, seed: 99, steps: 28 };
        let filled = fill(&own_workflow(), &values);
        // a value that is only a placeholder keeps its type
        assert_eq!(filled["latent"]["inputs"]["width"], json!(1024));
        assert_eq!(filled["latent"]["inputs"]["height"], json!(1536));
        assert_eq!(filled["sample"]["inputs"]["seed"], json!(99));
        assert_eq!(filled["sample"]["inputs"]["steps"], json!(28));
        // quotes and newlines in the prompt survive, and a placeholder written inside the prompt is not filled again
        assert_eq!(filled["text"]["inputs"]["prompt"], json!("a \"quoted\"\nline with {{width}} in it"));
        // inside longer text a placeholder becomes text
        assert_eq!(filled["save"]["inputs"]["filename_prefix"], json!("own_1024x1536"));
        // what is not ours is left alone: other braces, other values, the keys
        let other = json!({ "n": { "class_type": "X", "inputs": { "a": "{{other}}", "b": 7, "c": ["n", 0], "{{width}}": true, "t": " {{steps}} " } } });
        let kept = fill(&other, &values);
        assert_eq!(kept["n"]["inputs"]["a"], json!("{{other}}"));
        assert_eq!(kept["n"]["inputs"]["b"], json!(7));
        assert_eq!(kept["n"]["inputs"]["c"], json!(["n", 0]));
        assert_eq!(kept["n"]["inputs"]["{{width}}"], json!(true));
        assert_eq!(kept["n"]["inputs"]["t"], json!(28));
    }

    #[test]
    fn the_prompt_may_go_in_as_many_places_as_the_workflow_wants_and_a_negative_keeps_its_own_words() {
        // two positive prompt nodes (say, a main and a refiner) and a negative one the member wrote themselves
        let graph = json!({
            "a": { "class_type": "CLIPTextEncode", "inputs": { "text": "{{prompt}}" } },
            "b": { "class_type": "CLIPTextEncode", "inputs": { "text": "masterpiece, {{prompt}}, sharp" } },
            "c": { "class_type": "CLIPTextEncode", "inputs": { "text": "blurry, watermark" } },
            "save": { "class_type": "SaveImage", "inputs": { "images": ["a", 0] } }
        });
        assert!(parse_workflow(&graph.to_string()).is_ok());
        let filled = fill(&graph, &Values { prompt: "a red car", width: 1024, height: 1024, seed: 1, steps: 20 });
        assert_eq!(filled["a"]["inputs"]["text"], "a red car");
        assert_eq!(filled["b"]["inputs"]["text"], "masterpiece, a red car, sharp");
        assert_eq!(filled["c"]["inputs"]["text"], "blurry, watermark", "a node without the placeholder is the member's own and stays");
    }

    #[test]
    fn a_workflow_file_is_read_with_its_problems_said_before_anything_runs() {
        let good = own_file("wf-good", &own_workflow());
        assert!(read_workflow(&good).is_ok());
        assert!(read_workflow(&format!("  \"{good}\"  ")).is_ok(), "a pasted path in quotes works");
        assert!(read_workflow("").unwrap_err().contains("No workflow file"));
        assert!(read_workflow("W:\\nowhere\\none.json").unwrap_err().contains("could not be found"));
        let big = temp("wf-big").join("big.json");
        std::fs::write(&big, vec![b' '; (WORKFLOW_MAX_BYTES + 1) as usize]).unwrap();
        assert!(read_workflow(&big.display().to_string()).unwrap_err().contains("too big"));
        let bad = temp("wf-bad").join("bad.json");
        std::fs::write(&bad, "{ not json").unwrap();
        assert!(read_workflow(&bad.display().to_string()).unwrap_err().contains("not valid JSON"));
    }

    #[test]
    fn the_time_limit_is_the_members_or_fifteen_minutes() {
        assert_eq!(ComfySettings::default().limit(), Duration::from_secs(15 * 60));
        assert_eq!(ComfySettings { limit_minutes: 45, ..Default::default() }.limit(), Duration::from_secs(45 * 60));
    }

    #[test]
    fn a_picture_is_drawn_with_the_members_workflow_without_asking_for_checkpoints() {
        let png = vec![0x89, b'P', b'N', b'G', 9, 9];
        let comfy = comfy_that_draws(&[], png.clone());
        let settings = ComfySettings { workflow: own_file("wf-draw", &own_workflow()), ..settings_for(&comfy, "ignored.safetensors") };
        let out = temp("wf-out").join("p.png");
        let brief = Brief { prompt: "a myvi in hotwheel style", size: Some("1024x1536"), quality: Some("low") };
        draw(&settings, &Launcher::default(), &out.with_extension("log"), &brief, &out, Duration::from_secs(5), &no_stop).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), png);
        let asked = comfy.asked();
        assert!(!asked.iter().any(|r| r.contains("/object_info")), "a workflow of the member's own needs no checkpoint list");
        let graph = sent_graph(&comfy);
        assert_eq!(graph["text"]["inputs"]["prompt"], "a myvi in hotwheel style");
        assert_eq!(graph["latent"]["inputs"]["width"], json!(1024));
        assert_eq!(graph["latent"]["inputs"]["height"], json!(1536));
        assert_eq!(graph["sample"]["inputs"]["steps"], json!(12));
        assert!(graph["sample"]["inputs"]["seed"].is_u64());
        assert_eq!(graph["unet"]["inputs"]["unet_name"], "qwen_image.safetensors", "the member's own model names are kept");
    }

    #[test]
    fn a_wrong_workflow_file_stops_the_drawing_before_comfyui_is_even_asked() {
        let comfy = comfy_that_draws(&["a.safetensors"], vec![0x89, b'P', b'N', b'G']);
        let editor = json!({ "nodes": [], "links": [] });
        let settings = ComfySettings { workflow: own_file("wf-editor", &editor), ..settings_for(&comfy, "") };
        let out = temp("wf-wrong").join("p.png");
        let brief = Brief { prompt: "x", size: None, quality: None };
        let said = draw(&settings, &Launcher::default(), &out.with_extension("log"), &brief, &out, Duration::from_secs(5), &no_stop).unwrap_err();
        assert!(said.contains("Export (API)"), "{said}");
        assert!(comfy.asked().is_empty(), "ComfyUI must not be contacted, nor started, for a workflow that cannot run");
        assert!(!out.exists());
    }

    #[test]
    fn a_jpeg_from_a_members_saver_is_kept_as_a_png() {
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb([200, 30, 30]))).write_to(&mut jpeg, image::ImageFormat::Jpeg).unwrap();
        let comfy = comfy_that_draws(&["a.safetensors"], jpeg.into_inner());
        let out = temp("jpeg").join("p.png");
        let brief = Brief { prompt: "x", size: None, quality: None };
        draw(&settings_for(&comfy, ""), &Launcher::default(), &out.with_extension("log"), &brief, &out, Duration::from_secs(5), &no_stop).unwrap();
        let kept = std::fs::read(&out).unwrap();
        assert!(kept.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(image::load_from_memory(&kept).is_ok());
        assert!(as_png(b"not a picture".to_vec()).unwrap_err().contains("not a picture"));
    }

    #[test]
    fn studios_size_and_quality_line_is_not_drawn_as_words() {
        let line = "\n\nSize 1024x1024, medium quality.";
        assert_eq!(words_of(&format!("yellow myvi extreme lowered{line}"), Some("1024x1024"), Some("medium")), "yellow myvi extreme lowered");
        assert_eq!(words_of("two cats\n\nReference pictures, in order: 1 is Mochi.\n\nSize 1536x1024, low quality.", Some("1536x1024"), Some("low")), "two cats\n\nReference pictures, in order: 1 is Mochi.");
        // a prompt typed in the Make bar has no such line, and words that only look like it stay
        assert_eq!(words_of("a plain prompt", None, None), "a plain prompt");
        assert_eq!(words_of("a plain prompt", Some("1024x1024"), Some("medium")), "a plain prompt");
        assert_eq!(words_of(&format!("a poster{line}"), Some("1536x1024"), Some("medium")), format!("a poster{line}"), "a different size is not STUDIO's line");
        assert_eq!(words_of("Size 1024x1024, medium quality.", Some("1024x1024"), Some("medium")), "Size 1024x1024, medium quality.", "the line alone is the member's own words");
    }

    #[test]
    fn a_drawing_sends_the_words_without_studios_size_line_and_the_size_on_its_own() {
        let comfy = comfy_that_draws(&["a.safetensors"], vec![0x89, b'P', b'N', b'G']);
        let out = temp("words").join("p.png");
        let brief = Brief { prompt: "yellow myvi extreme lowered\n\nSize 1024x1536, medium quality.", size: Some("1024x1536"), quality: Some("medium") };
        draw(&settings_for(&comfy, ""), &Launcher::default(), &out.with_extension("log"), &brief, &out, Duration::from_secs(5), &no_stop).unwrap();
        let graph = sent_graph(&comfy);
        assert_eq!(graph["2"]["inputs"]["text"], "yellow myvi extreme lowered");
        assert_eq!((graph["4"]["inputs"]["width"].clone(), graph["4"]["inputs"]["height"].clone()), (json!(1024), json!(1536)));
    }

    #[test]
    fn a_comfyui_that_closed_mid_drawing_says_how_when_castle_started_it() {
        let log = Path::new("C:\\cfg\\comfy-start.log");
        let lost = "ComfyUI at http://127.0.0.1:8188 stopped answering while it was drawing.".to_string();
        // not started by Castle: nothing to add, and no log to point at
        assert_eq!(ended_words(lost.clone(), None, false, log), lost);
        // an ordinary error is left as it is
        assert_eq!(ended_words("Stopped.".into(), Some(0), true, log), "Stopped.");
        // started by Castle and the program ended: the exit code, with a crash code also in hex
        let crashed = ended_words(lost.clone(), Some(-1073741819), true, log);
        assert!(crashed.contains("closed while it was drawing") && crashed.contains("-1073741819, 0xC0000005"), "{crashed}");
        assert!(crashed.contains("comfy-start.log") && crashed.contains("comfy-start.previous.log"));
        assert!(ended_words(lost.clone(), Some(1), true, log).contains("exit code 1)"));
        // still running but not answering: frozen or crashed, with where to look
        let frozen = ended_words(lost, None, true, log);
        assert!(frozen.contains("stopped answering") && frozen.contains("crashed or frozen") && frozen.contains("comfy-start.log"), "{frozen}");
    }

    #[test]
    fn the_last_runs_log_is_kept_when_the_next_run_starts() {
        let folder = temp("rotate");
        let log = folder.join("comfy-start.log");
        rotate_log(&log); // nothing there yet: nothing happens
        assert!(!previous_log(&log).exists());
        std::fs::write(&log, "first run, then it died").unwrap();
        rotate_log(&log);
        assert!(!log.exists());
        assert_eq!(std::fs::read_to_string(previous_log(&log)).unwrap(), "first run, then it died");
        // a third run replaces the kept one, so only the last two runs are ever on the disk
        std::fs::write(&log, "second run").unwrap();
        rotate_log(&log);
        assert_eq!(std::fs::read_to_string(previous_log(&log)).unwrap(), "second run");
        assert_eq!(previous_log(&log).file_name().unwrap().to_string_lossy(), "comfy-start.previous.log");
    }

    #[test]
    fn starting_comfyui_twice_keeps_the_first_runs_words() {
        let folder = temp("twice");
        let log = folder.join("start.log");
        #[cfg(windows)]
        let (first, second) = ("echo first run printed this", "echo second run printed that");
        #[cfg(not(windows))]
        let (first, second) = ("echo first run printed this", "echo second run printed that");
        for (n, command) in [first, second].into_iter().enumerate() {
            let flag = folder.join(format!("up{n}.txt"));
            let comfy = comfy_after(flag.clone());
            std::fs::write(&flag, "x").unwrap();
            let launcher = Launcher::default();
            // nothing answers at first for the real address used here: use a dead one so the command runs
            let _ = ensure_up(&nothing_listens(), command, &launcher, &log, Duration::from_millis(400), Duration::from_millis(20), &no_stop);
            drop(comfy);
            stop_started(&launcher);
        }
        assert!(std::fs::read_to_string(previous_log(&log)).unwrap().contains("first run printed this"));
        assert!(std::fs::read_to_string(&log).unwrap().contains("second run printed that"));
    }

    /// The whole drawing path against the member's real ComfyUI, as Castle's worker runs it: starts
    /// it when it is not running (CASTLE_COMFY_START), draws a plain picture with the checkpoint
    /// (CASTLE_COMFY_MODEL, default the first) or with the member's own workflow file
    /// (CASTLE_COMFY_WORKFLOW) and saves it; ends ComfyUI again only if Castle started it:
    /// `cargo test --manifest-path src-tauri/Cargo.toml comfy::tests::live_draw -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_draw_with_the_members_comfyui_from_a_cold_start() {
        let settings = ComfySettings {
            on: true,
            address: std::env::var("CASTLE_COMFY_ADDRESS").unwrap_or_default(),
            start: std::env::var("CASTLE_COMFY_START").unwrap_or_default(),
            model: std::env::var("CASTLE_COMFY_MODEL").unwrap_or_default(),
            workflow: std::env::var("CASTLE_COMFY_WORKFLOW").unwrap_or_default(),
            limit_minutes: 0,
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
        let status = status_of(&server, &launcher, "");
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
