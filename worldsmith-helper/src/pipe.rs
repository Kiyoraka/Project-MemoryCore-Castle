//! The named pipe: one server per user, one JSON line in and one out per connection.

use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY,
    ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

const MAX_LINE: usize = 1 << 20;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Read up to a newline (or the end).
fn read_line(h: HANDLE) -> Option<String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut n = 0u32;
        let ok = unsafe { ReadFile(h, buf.as_mut_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut()) };
        if ok == 0 {
            let e = unsafe { GetLastError() };
            if e == ERROR_BROKEN_PIPE && !out.is_empty() {
                break;
            }
            return None;
        }
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
        if out.contains(&b'\n') || out.len() > MAX_LINE {
            break;
        }
    }
    let s = String::from_utf8_lossy(&out).into_owned();
    Some(s.lines().next().unwrap_or("").to_string())
}

fn write_all(h: HANDLE, data: &[u8]) -> bool {
    let mut off = 0;
    while off < data.len() {
        let mut n = 0u32;
        let ok = unsafe {
            WriteFile(h, data[off..].as_ptr(), (data.len() - off) as u32, &mut n, std::ptr::null_mut())
        };
        if ok == 0 || n == 0 {
            return false;
        }
        off += n as usize;
    }
    true
}

pub struct Server {
    h: HANDLE,
}

impl Server {
    /// Own the pipe, or None when another server already does.
    pub fn bind(name: &str) -> Result<Option<Server>, String> {
        let w = wide(name);
        let h = unsafe {
            CreateNamedPipeW(
                w.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            let e = unsafe { GetLastError() };
            // the first-instance flag fails with access denied when the name is already served
            if e == ERROR_ACCESS_DENIED || e == ERROR_PIPE_BUSY {
                return Ok(None);
            }
            return Err(format!("Couldn't open the Worldsmith pipe (error {e})."));
        }
        Ok(Some(Server { h }))
    }

    /// Serve forever: one connection at a time, one request line each.
    pub fn run(&self, mut handle: impl FnMut(&str) -> String) -> ! {
        loop {
            let ok = unsafe { ConnectNamedPipe(self.h, std::ptr::null_mut()) };
            if ok == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
                unsafe { DisconnectNamedPipe(self.h) };
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            if let Some(line) = read_line(self.h) {
                let mut reply = handle(&line);
                reply.push('\n');
                if write_all(self.h, reply.as_bytes()) {
                    unsafe { FlushFileBuffers(self.h) };
                }
            }
            unsafe { DisconnectNamedPipe(self.h) };
        }
    }
}

pub enum CallError {
    NotRunning,
    Failed(String),
}

/// Send one request line, wait for the reply line.
pub fn call(name: &str, request: &str) -> Result<String, CallError> {
    let w = wide(name);
    let start = Instant::now();
    let h = loop {
        let h = unsafe {
            CreateFileW(
                w.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut::<c_void>(),
            )
        };
        if h != INVALID_HANDLE_VALUE {
            break h;
        }
        match unsafe { GetLastError() } {
            ERROR_FILE_NOT_FOUND => return Err(CallError::NotRunning),
            // the server answers one request at a time (a big scan can take a while)
            ERROR_PIPE_BUSY if start.elapsed() < Duration::from_secs(600) => {
                unsafe { WaitNamedPipeW(w.as_ptr(), 1000) };
            }
            e => return Err(CallError::Failed(format!("Couldn't reach Worldsmith (error {e})."))),
        }
    };
    let mut line = request.to_string();
    line.push('\n');
    let result = if !write_all(h, line.as_bytes()) {
        Err(CallError::Failed("Couldn't send the request to Worldsmith.".into()))
    } else {
        read_line(h)
            .filter(|l| !l.trim().is_empty())
            .ok_or_else(|| CallError::Failed("Worldsmith closed without an answer.".into()))
    };
    unsafe { CloseHandle(h) };
    result
}
