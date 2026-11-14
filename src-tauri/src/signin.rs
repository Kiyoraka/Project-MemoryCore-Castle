//! Signing a member in to the MemoryCore Foundation room through their own browser - the
//! standard loopback sign-in for desktop apps (RFC 8252) with PKCE (RFC 7636), as the Portfolio's
//! room contract describes. Nothing is pasted: Castle makes a secret and its fingerprint, listens
//! on 127.0.0.1, opens the Portfolio's /members/desktop page, takes the one-time code back, and
//! trades code + secret for the member's sign-in token. The token lives in Credential Manager
//! ("member") and never reaches the page.

use crate::key;
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

/// Where the MemoryCore Foundation room lives (the Portfolio). Built in, so members never type an
/// address. A dev build may point elsewhere with CASTLE_PORTFOLIO (e.g. a local Portfolio).
pub const PORTFOLIO: &str = "https://www.afifmaahi-portfolio.com";
const USER_AGENT: &str = concat!("MemoryCoreCastle/", env!("CARGO_PKG_VERSION"));
const WAIT_FOR_BROWSER: Duration = Duration::from_secs(5 * 60);

pub fn portfolio() -> String {
    if cfg!(debug_assertions) {
        if let Some(url) = std::env::var("CASTLE_PORTFOLIO").ok().filter(|u| u.starts_with("http")) {
            return url.trim_end_matches('/').to_string();
        }
    }
    PORTFOLIO.to_string()
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random(n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).map_err(|_| "The computer could not make a sign-in secret.".to_string())?;
    Ok(buf)
}

/// The PKCE fingerprint of a secret: base64url(SHA-256(verifier)), no padding (43 characters).
pub fn challenge_of(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

/// The one-time code from the browser's callback request line, when its state matches ours.
/// `GET /callback?code=abc&state=xyz HTTP/1.1` -> Ok("abc").
pub fn parse_callback(request_line: &str, state: &str) -> Result<String, String> {
    let target = request_line.split_whitespace().nth(1).ok_or("The browser sent something unreadable.")?;
    let query = target.strip_prefix("/callback?").ok_or("That was not the sign-in coming back.")?;
    let mut code = None;
    let mut got_state = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = decode(v);
        match k {
            "code" => code = Some(v),
            "state" => got_state = Some(v),
            _ => {}
        }
    }
    if got_state.as_deref() != Some(state) {
        return Err("The sign-in that came back was not the one Castle started - please try again.".into());
    }
    code.filter(|c| !c.is_empty()).ok_or_else(|| "The sign-in came back without its code - please try again.".into())
}

/// %-decoding for the few characters a code or state may carry.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn open_in_browser(url: &str) -> Result<(), String> {
    // explorer.exe silently fails to hand a URL to the default browser once it has a query
    // string with "&" in it (falls back to opening a plain Explorer window instead) - rundll32's
    // FileProtocolHandler is the long-standing reliable way to open a URL on Windows.
    #[cfg(windows)]
    {
        return std::process::Command::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|_| "The browser could not be opened.".to_string());
    }
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(not(windows))]
    return command.arg(url).spawn().map(|_| ()).map_err(|_| "The browser could not be opened.".to_string());
}

const PAGE: &str = "<!doctype html><meta charset=utf-8><title>MemoryCore Castle</title><body style=\"font:16px system-ui;background:#0b1026;color:#e8e6ff;display:grid;place-items:center;height:100vh;margin:0\"><div style=\"text-align:center\"><h2>You are signed in</h2><p>You can close this tab and go back to MemoryCore Castle.</p></div>";

/// Waits for the browser to come back to 127.0.0.1 with the code (at most five minutes).
fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|_| "Castle could not listen for the sign-in.".to_string())?;
    let until = Instant::now() + WAIT_FOR_BROWSER;
    while Instant::now() < until {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let first = request.lines().next().unwrap_or_default().to_string();
                if !first.contains("/callback") {
                    // a browser asking for a favicon or similar: answer and keep waiting
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let answer = parse_callback(&first, state);
                let body = if answer.is_ok() { PAGE.to_string() } else { PAGE.replace("You are signed in", "Please try again from Castle") };
                let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(reply.as_bytes());
                return answer;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(150)),
            Err(_) => return Err("Castle stopped listening for the sign-in.".into()),
        }
    }
    Err("The sign-in took too long - press Sign in again when you are ready.".into())
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder().user_agent(USER_AGENT).timeout(timeout).build().map_err(|_| "Castle could not reach the internet.".to_string())
}

fn said(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body).ok()?.get("error")?.as_str().map(str::to_string)
}

/// Trades the one-time code and the secret for the member's sign-in token.
pub fn exchange(base: &str, code: &str, verifier: &str) -> Result<String, String> {
    let answer = client(Duration::from_secs(20))?
        .post(format!("{base}/api/public/members/desktop/token"))
        .json(&serde_json::json!({ "code": code, "verifier": verifier }))
        .send()
        .map_err(|_| "The Portfolio could not be reached. Check the internet connection.".to_string())?;
    let status = answer.status().as_u16();
    let body = answer.text().unwrap_or_default();
    match status {
        200..=299 => serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("token").and_then(Value::as_str).map(str::to_string))
            .filter(|t| !t.is_empty())
            .ok_or_else(|| "The Portfolio's answer had no sign-in in it.".to_string()),
        403 => Err(said(&body).unwrap_or_else(|| "Your Library card is not active.".into())),
        429 => Err("Too many sign-ins from here - please wait a while and try again.".into()),
        _ => Err(said(&body).unwrap_or_else(|| format!("The sign-in did not work ({status}) - please try again."))),
    }
}

/// The whole browser sign-in; the token is kept in Credential Manager.
pub fn sign_in() -> Result<(), String> {
    let base = portfolio();
    let verifier = b64url(&random(32)?);
    let state = b64url(&random(16)?);
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| "Castle could not listen for the sign-in.".to_string())?;
    let port = listener.local_addr().map_err(|_| "Castle could not listen for the sign-in.".to_string())?.port();
    let url = format!("{base}/members/desktop?port={port}&state={state}&challenge={}", challenge_of(&verifier));
    open_in_browser(&url)?;
    let code = wait_for_code(&listener, &state)?;
    let token = exchange(&base, &code, &verifier)?;
    key::save("member", &token)
}

/// The member's door, step by step, from the Portfolio.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemberStatus {
    pub signed_in: bool,
    /// the Library card: "active" | "inactive" | "unknown"
    pub card: String,
    /// NONE | PENDING | APPROVED | REJECTED | REVOKED
    pub foundation: String,
    /// the Foundation display name, else the card name
    pub name: Option<String>,
    /// the room answered /room/me
    pub room_open: bool,
    /// a sign-in that has run out: sign in again
    pub expired: bool,
    /// a plain line when something could not be read
    pub problem: Option<String>,
}

/// What /api/member/me says, as the door's steps (pure, tested).
pub fn status_from_me(status: u16, body: &str) -> MemberStatus {
    let mut s = MemberStatus { signed_in: true, card: "unknown".into(), foundation: "NONE".into(), ..MemberStatus::default() };
    match status {
        200..=299 => {
            let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            s.card = "active".into();
            s.foundation = v.pointer("/foundation/status").and_then(Value::as_str).unwrap_or("NONE").to_uppercase();
            s.name = v
                .pointer("/foundation/displayName")
                .and_then(Value::as_str)
                .filter(|n| !n.trim().is_empty())
                .or_else(|| v.get("name").and_then(Value::as_str))
                .map(str::to_string);
        }
        401 => {
            s.signed_in = false;
            s.expired = true;
        }
        403 => s.card = "inactive".into(),
        _ => s.problem = Some(said(body).unwrap_or_else(|| "The Portfolio could not be read just now.".into())),
    }
    s
}

pub fn member_status_now() -> MemberStatus {
    let Some(token) = key::load("member") else { return MemberStatus { card: "unknown".into(), foundation: "NONE".into(), ..MemberStatus::default() } };
    let base = portfolio();
    let Ok(http) = client(Duration::from_secs(15)) else { return MemberStatus { signed_in: true, problem: Some("Castle could not reach the internet.".into()), ..MemberStatus::default() } };
    let mut s = match http.get(format!("{base}/api/member/me")).bearer_auth(&token).send() {
        Ok(answer) => {
            let status = answer.status().as_u16();
            status_from_me(status, &answer.text().unwrap_or_default())
        }
        Err(_) => MemberStatus { signed_in: true, card: "unknown".into(), foundation: "NONE".into(), problem: Some("The Portfolio could not be reached. Check the internet connection.".into()), ..MemberStatus::default() },
    };
    if s.foundation == "APPROVED" {
        s.room_open = http.get(format!("{base}/api/room/me")).bearer_auth(&token).send().map(|a| a.status().is_success()).unwrap_or(false);
    }
    s
}

#[tauri::command]
pub async fn member_sign_in() -> Result<MemberStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        sign_in()?;
        crate::roomalert::forget_name();
        Ok(member_status_now())
    })
    .await
    .map_err(|_| "The sign-in stopped.".to_string())?
}

#[tauri::command]
pub async fn member_status() -> MemberStatus {
    tauri::async_runtime::spawn_blocking(member_status_now).await.unwrap_or_default()
}

#[tauri::command]
pub fn member_sign_out() -> Result<(), String> {
    crate::roomalert::forget_name();
    key::forget("member")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fingerprint_matches_the_rfc_example() {
        // RFC 7636, appendix B
        assert_eq!(challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let verifier = b64url(&random(32).unwrap());
        assert_eq!(verifier.len(), 43);
        assert_eq!(challenge_of(&verifier).len(), 43);
    }

    #[test]
    fn the_callback_gives_the_code_only_with_our_state() {
        assert_eq!(parse_callback("GET /callback?code=abc123&state=s-1 HTTP/1.1", "s-1").unwrap(), "abc123");
        assert_eq!(parse_callback("GET /callback?state=s-1&code=a%2Db HTTP/1.1", "s-1").unwrap(), "a-b");
        assert!(parse_callback("GET /callback?code=abc&state=other HTTP/1.1", "s-1").is_err());
        assert!(parse_callback("GET /callback?state=s-1 HTTP/1.1", "s-1").is_err());
        assert!(parse_callback("GET /favicon.ico HTTP/1.1", "s-1").is_err());
    }

    #[test]
    fn the_door_steps_come_from_the_card() {
        let s = status_from_me(200, r#"{"name":"Rin Hoshino","email":"x@y","foundation":{"status":"PENDING","displayName":"Rin"}}"#);
        assert_eq!((s.card.as_str(), s.foundation.as_str(), s.name.as_deref()), ("active", "PENDING", Some("Rin")));
        assert_eq!(status_from_me(200, r#"{"name":"Rin"}"#).foundation, "NONE");
        assert_eq!(status_from_me(403, r#"{"error":"inactive"}"#).card, "inactive");
        let gone = status_from_me(401, "");
        assert!(gone.expired && !gone.signed_in);
        // the email never comes through
        assert!(!format!("{:?}", status_from_me(200, r#"{"name":"Rin","email":"secret@mail"}"#)).contains("secret@mail"));
    }

    /// A one-request mock Portfolio.
    fn mock(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(reply.as_bytes()).unwrap();
            request
        });
        (base, handle)
    }

    #[test]
    fn the_code_and_secret_are_traded_for_the_token() {
        let (base, seen) = mock(200, r#"{"token":"member-jwt","member":{"name":"Rin"}}"#);
        assert_eq!(exchange(&base, "code-1", "verifier-1").unwrap(), "member-jwt");
        let request = seen.join().unwrap();
        assert!(request.starts_with("POST /api/public/members/desktop/token "));
        assert!(request.contains("\"code\":\"code-1\"") && request.contains("\"verifier\":\"verifier-1\""));
        assert!(request.contains("MemoryCoreCastle/"));
        let (base, seen) = mock(400, r#"{"error":"This sign-in code has expired"}"#);
        assert_eq!(exchange(&base, "c", "v").unwrap_err(), "This sign-in code has expired");
        let _ = seen.join();
    }
}
