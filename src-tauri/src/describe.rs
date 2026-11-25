//! STUDIO's "Write prompt from picture": one picture in, one image prompt out, the same way a picture
//! would be drawn (`env::picture_way`) - the member's OpenAI key, or Codex signed in with ChatGPT.
//! Nothing is drawn and nothing is kept; the words go back to the page to be changed.

use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The model that reads the picture on the key way (OpenAI's vision guide, Responses API, Nov 2026).
pub const VISION_MODEL: &str = "gpt-6.2-astra";
pub const OPENAI: &str = "https://api.openai.com";
const MAX_PICTURE_BYTES: u64 = 25 * 1024 * 1024;
const TIME_LIMIT: Duration = Duration::from_secs(120);
/// What the AI is asked; the same words on both ways.
pub const INSTRUCTION: &str = "Write one image-generation prompt that would recreate this picture: the subject, the art style, the composition and framing, the light, and the colours. Describe people by how they look, never by a real person's name. Answer with the prompt alone, in one paragraph, no title and no quotes.";
pub const NO_WAY: &str = "Add the OpenAI key in Settings > Keys, or sign in to Codex with ChatGPT, to read a picture.";

/// A picture the reader may send: PNG, JPEG or WebP, at most 25 MB, as a data URL.
pub fn data_url(path: &Path) -> Result<String, String> {
    let lower = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let kind = match lower.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => return Err("Choose a PNG, JPEG or WebP picture.".into()),
    };
    let size = std::fs::metadata(path).map_err(|_| "That picture was not found.".to_string())?.len();
    if size > MAX_PICTURE_BYTES {
        return Err("That picture is larger than 25 MB.".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "That picture could not be read.".to_string())?;
    Ok(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// The Responses API request for one picture.
pub fn request_body(data_url: &str) -> Value {
    request_body_with(data_url, INSTRUCTION)
}

/// The request for one picture with its own instruction (the browser's manga bubbles).
pub fn request_body_with(data_url: &str, instruction: &str) -> Value {
    json!({
        "model": VISION_MODEL,
        "input": [{
            "role": "user",
            "content": [
                { "type": "input_text", "text": instruction },
                { "type": "input_image", "image_url": data_url, "detail": "auto" }
            ]
        }]
    })
}

/// The answer's words: every `output_text` part of the message items, joined.
pub fn text_of(answer: &Value) -> Option<String> {
    let parts: Vec<String> = answer
        .get("output")?
        .as_array()?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .flat_map(|item| item.get("content").and_then(Value::as_array).cloned().unwrap_or_default())
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str).map(str::to_string))
        .collect();
    let text = parts.join("\n").trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// The key way: one Responses API call. `base` is OpenAI's address (a test passes its own).
pub fn ask_openai(base: &str, key: &str, data_url: &str) -> Result<String, String> {
    ask_openai_with(base, key, data_url, INSTRUCTION)
}

/// `ask_openai` with its own instruction: an API call with no tools at all.
pub fn ask_openai_with(base: &str, key: &str, data_url: &str, instruction: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(TIME_LIMIT)
        .user_agent(concat!("MemoryCoreCastle/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "The picture could not be read just now.".to_string())?;
    let answer = client
        .post(format!("{base}/v1/responses"))
        .bearer_auth(key)
        .json(&request_body_with(data_url, instruction))
        .send()
        .map_err(|_| "OpenAI could not be reached.".to_string())?;
    let status = answer.status().as_u16();
    let body: Value = answer.json().map_err(|_| "OpenAI sent something unreadable.".to_string())?;
    if status == 401 {
        return Err("The OpenAI key was not accepted. Check it in Settings > Keys.".into());
    }
    if !(200..300).contains(&status) {
        // the provider's own words (a refusal, a spent balance), never the key
        let said = body.pointer("/error/message").and_then(Value::as_str).unwrap_or("the request was refused");
        return Err(format!("OpenAI said: {}", said.chars().take(200).collect::<String>()));
    }
    text_of(&body).ok_or_else(|| "OpenAI read the picture but wrote nothing back.".to_string())
}

/// The Codex way's arguments: read-only, the picture attached, the last message written to `out`,
/// the instruction on stdin.
pub fn codex_args(picture: &Path, out: &Path) -> Vec<String> {
    vec![
        "exec".into(),
        "--skip-git-repo-check".into(),
        "--sandbox".into(),
        "read-only".into(),
        "-c".into(),
        "forced_login_method=chatgpt".into(),
        "-c".into(),
        "model_provider=openai".into(),
        "--image".into(),
        picture.display().to_string(),
        "--output-last-message".into(),
        out.display().to_string(),
        "-".into(),
    ]
}

/// The Codex way: one `codex exec` in a folder of its own, like a room turn; the chat is untouched.
fn ask_codex(program: &Path, picture: &Path, work: &Path) -> Result<String, String> {
    use std::io::Write;
    std::fs::create_dir_all(work).map_err(|_| "The picture could not be read just now.".to_string())?;
    let out = work.join("prompt.txt");
    let _ = std::fs::remove_file(&out);
    let through_cmd = program.extension().is_some_and(|x| x.eq_ignore_ascii_case("cmd"));
    // platform: windows-only - a .cmd / .bat launcher exists only on Windows
    let mut command = if through_cmd {
        let mut c = std::process::Command::new("cmd.exe");
        c.arg("/c").arg(program);
        c
    } else {
        std::process::Command::new(program)
    };
    command
        .args(codex_args(picture, &out))
        .current_dir(work)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for name in crate::env::NEVER_INHERITED {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|_| "Codex could not start.".to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(INSTRUCTION.as_bytes());
    }
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| "Codex stopped unexpectedly.".to_string())? {
            break status;
        }
        if started.elapsed() > TIME_LIMIT {
            let _ = child.kill();
            return Err("Codex took too long to read the picture.".into());
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let text = std::fs::read_to_string(&out).unwrap_or_default().trim().to_string();
    if !status.success() || text.is_empty() {
        return Err("Codex could not read the picture. Try again, or add an OpenAI key.".into());
    }
    Ok(text)
}

/// Reads one picture the way open now. `way` is `env::picture_way`'s answer.
pub fn describe(way: &str, picture: &Path, key: Option<String>, work: PathBuf) -> Result<String, String> {
    let url = data_url(picture)?;
    match way {
        "key" => ask_openai(OPENAI, key.as_deref().ok_or(NO_WAY)?, &url),
        "codex" => {
            let program = crate::env::find_on_path("codex").ok_or(NO_WAY)?;
            ask_codex(&program, picture, &work)
        }
        _ => Err(NO_WAY.into()),
    }
}

/// "Write prompt from picture": the picture's prompt, or a plain line saying why not.
#[tauri::command]
pub async fn picture_describe(path: String, store: tauri::State<'_, crate::settings::SettingsStore>) -> Result<String, String> {
    crate::rooms::need(&store, "studio")?;
    let work = store.folder().join("describe");
    tauri::async_runtime::spawn_blocking(move || {
        let key = crate::key::load("openai");
        let way = crate::env::picture_way(crate::providers::codex_chatgpt_cached, key.is_some());
        describe(way, Path::new(&path), key, work)
    })
    .await
    .map_err(|_| "The picture could not be read just now.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn temp(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("castle-describe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// One request answered with `status` and `body`; returns the request as text.
    fn serve_once(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buf = [0; 8192];
                let n = stream.read(&mut buf).unwrap();
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
            write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            String::from_utf8_lossy(&bytes).to_string()
        });
        (base, handle)
    }

    #[test]
    fn only_a_png_jpeg_or_webp_within_the_limit_is_read() {
        let root = temp("types");
        let png = root.join("cat.PNG");
        std::fs::write(&png, b"png").unwrap();
        assert_eq!(data_url(&png).unwrap(), "data:image/png;base64,cG5n");
        let jpg = root.join("cat.jpeg");
        std::fs::write(&jpg, b"jpg").unwrap();
        assert!(data_url(&jpg).unwrap().starts_with("data:image/jpeg;base64,"));
        let gif = root.join("cat.gif");
        std::fs::write(&gif, b"gif").unwrap();
        assert!(data_url(&gif).unwrap_err().contains("PNG, JPEG or WebP"));
        assert!(data_url(&root.join("gone.webp")).unwrap_err().contains("not found"));
    }

    #[test]
    fn the_request_carries_the_instruction_and_the_picture() {
        let body = request_body("data:image/png;base64,AAAA");
        assert_eq!(body["model"], VISION_MODEL);
        let content = &body["input"][0]["content"];
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[0]["text"], INSTRUCTION);
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], "data:image/png;base64,AAAA");
    }

    #[test]
    fn the_answers_words_are_read_from_its_message_parts() {
        let answer = json!({ "output": [
            { "type": "reasoning", "summary": [] },
            { "type": "message", "content": [ { "type": "output_text", "text": "  A lantern-lit forest path at dusk, painterly. " } ] }
        ] });
        assert_eq!(text_of(&answer).as_deref(), Some("A lantern-lit forest path at dusk, painterly."));
        assert_eq!(text_of(&json!({ "output": [] })), None);
        assert_eq!(text_of(&json!({ "nothing": true })), None);
    }

    #[test]
    fn the_key_way_posts_to_responses_and_returns_the_prompt() {
        let (base, handle) = serve_once(200, r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"Two cats asleep on a windowsill, soft morning light."}]}]}"#);
        let said = ask_openai(&base, "sk-test-key", "data:image/png;base64,AAAA").unwrap();
        let request = handle.join().unwrap();
        assert_eq!(said, "Two cats asleep on a windowsill, soft morning light.");
        assert!(request.starts_with("POST /v1/responses "));
        assert!(request.to_lowercase().contains("authorization: bearer sk-test-key"));
        assert!(request.contains("\"input_image\"") && request.contains(VISION_MODEL));
    }

    #[test]
    fn the_key_way_says_why_it_was_refused_without_the_key() {
        let (base, handle) = serve_once(401, r#"{"error":{"message":"Incorrect API key"}}"#);
        let said = ask_openai(&base, "sk-test-key", "data:image/png;base64,AAAA").unwrap_err();
        handle.join().unwrap();
        assert!(said.contains("Settings > Keys") && !said.contains("sk-test-key"));

        let (base, handle) = serve_once(400, r#"{"error":{"message":"This picture cannot be read."}}"#);
        let said = ask_openai(&base, "sk-test-key", "data:image/png;base64,AAAA").unwrap_err();
        handle.join().unwrap();
        assert_eq!(said, "OpenAI said: This picture cannot be read.");

        let (base, handle) = serve_once(200, r#"{"output":[]}"#);
        assert!(ask_openai(&base, "sk-test-key", "data:image/png;base64,AAAA").unwrap_err().contains("wrote nothing"));
        handle.join().unwrap();
    }

    #[test]
    fn the_codex_way_is_read_only_with_the_picture_attached() {
        let args = codex_args(Path::new("C:/memory/photo.png"), Path::new("C:/settings/describe/prompt.txt"));
        let at = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].clone());
        assert_eq!(args[0], "exec");
        assert_eq!(at("--sandbox").as_deref(), Some("read-only"));
        assert_eq!(at("--image").as_deref(), Some("C:/memory/photo.png"));
        assert_eq!(at("--output-last-message").as_deref(), Some("C:/settings/describe/prompt.txt"));
        assert!(args.contains(&"forced_login_method=chatgpt".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("-"), "the instruction goes on stdin");
    }

    #[test]
    fn with_no_way_open_it_says_how_to_open_one() {
        let root = temp("no-way");
        let png = root.join("cat.png");
        std::fs::write(&png, b"png").unwrap();
        assert_eq!(describe("none", &png, None, root.join("work")).unwrap_err(), NO_WAY);
        assert_eq!(describe("key", &png, None, root.join("work")).unwrap_err(), NO_WAY);
    }
}
