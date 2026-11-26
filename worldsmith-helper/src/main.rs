//! worldsmith-helper - the user-mode memory helper for WORLDSMITH.
//! For your own offline single-player games only. MIT licence.
//!
//! `worldsmith-helper serve --parent <pid>` runs the per-user server;
//! `worldsmith-helper <verb> <args...>` sends one request to it and prints one JSON line.

mod proto;
mod refuse;
mod scan;
mod thumb;

#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod freeze;
#[cfg(windows)]
mod memory;
#[cfg(windows)]
mod pipe;
#[cfg(windows)]
mod process;
#[cfg(windows)]
mod server;

use std::io::Write;

fn print_line(v: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

#[cfg(not(windows))]
fn main() {
    print_line(&proto::err(proto::WINDOWS_ONLY).to_string());
}

#[cfg(windows)]
fn pipe_name() -> String {
    proto::pipe_name(
        std::env::var("USERNAME").ok().as_deref(),
        std::env::var("WORLDSMITH_PIPE").ok().as_deref(),
    )
}

/// Per-monitor DPI aware (v2): every window rect, monitor rect and capture is in physical
/// pixels, the same pixels the picture holds. Set before any window call; a failure (old Windows,
/// or already set) is ignored.
#[cfg(windows)]
fn dpi_aware() {
    use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(windows)]
fn main() {
    dpi_aware();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a.eq_ignore_ascii_case("serve")).unwrap_or(false) {
        serve(&args[1..]);
        return;
    }
    let req = match proto::parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            print_line(&proto::err(&e).to_string());
            return;
        }
    };
    let line = serde_json::to_string(&req).expect("a request always serialises");
    match pipe::call(&pipe_name(), &line) {
        Ok(reply) => print_line(reply.trim()),
        Err(pipe::CallError::NotRunning) => print_line(&proto::err(proto::NOT_RUNNING).to_string()),
        Err(pipe::CallError::Failed(e)) => print_line(&proto::err(&e).to_string()),
    }
}

#[cfg(windows)]
fn serve(args: &[String]) {
    use std::sync::Arc;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE};

    let parent: Option<u32> = match args {
        [flag, pid] if flag.eq_ignore_ascii_case("--parent") => pid.trim().parse().ok().filter(|p| *p != 0),
        _ => None,
    };
    let Some(parent) = parent else {
        print_line(&proto::err("Usage: worldsmith-helper serve --parent <pid>").to_string());
        return;
    };
    let parent_handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent) };
    if parent_handle.is_null() {
        print_line(&proto::err(&format!("The parent process {parent} isn't running.")).to_string());
        return;
    }
    let server_pipe = match pipe::Server::bind(&pipe_name()) {
        Ok(Some(s)) => s,
        Ok(None) => {
            print_line(r#"{"ok":true,"already":true}"#);
            return;
        }
        Err(e) => {
            print_line(&proto::err(&e).to_string());
            return;
        }
    };

    let shared = Arc::new(freeze::Shared::default());
    // the parent's death ends everything: freezes stop, the handle closes, the pipe goes away
    {
        let shared = shared.clone();
        let h = parent_handle as usize;
        std::thread::spawn(move || {
            unsafe { WaitForSingleObject(h as _, INFINITE) };
            shared.detach();
            std::process::exit(0);
        });
    }
    freeze::start(shared.clone());
    print_line(r#"{"ok":true,"serving":true}"#);

    let mut srv = server::Server::new(shared);
    server_pipe.run(|line| srv.handle_line(line));
}
