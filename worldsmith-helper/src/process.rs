//! Processes as the user sees them: top-level windows, exe names, loaded modules and TCP links.

use crate::refuse;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_BAD_LENGTH, HWND, INVALID_HANDLE_VALUE, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
};

const MIB_TCP_STATE_ESTAB: u32 = 5;

/// Windows' own shell and system processes never appear in `games`.
const SYSTEM_EXES: &[&str] = &[
    "explorer.exe",
    "applicationframehost.exe",
    "textinputhost.exe",
    "systemsettings.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "searchhost.exe",
    "searchapp.exe",
    "lockapp.exe",
    "dwm.exe",
    "csrss.exe",
    "winlogon.exe",
    "taskmgr.exe",
    "sihost.exe",
    "ctfmon.exe",
    "widgets.exe",
    "gamebar.exe",
    "worldsmith-helper.exe",
];

pub struct WindowInfo {
    pub hwnd: HWND,
    pub pid: u32,
    pub title: String,
    pub area: i64,
}

fn window_text(hwnd: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(hwnd);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam as *mut Vec<WindowInfo>);
    if IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() {
        return 1;
    }
    if (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & WS_EX_TOOLWINDOW != 0 {
        return 1;
    }
    let mut cloaked: u32 = 0;
    if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, &mut cloaked as *mut u32 as *mut c_void, 4) == 0
        && cloaked != 0
    {
        return 1;
    }
    let title = window_text(hwnd);
    if title.trim().is_empty() {
        return 1;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let mut r: RECT = std::mem::zeroed();
    GetWindowRect(hwnd, &mut r);
    let area = ((r.right - r.left).max(0) as i64) * ((r.bottom - r.top).max(0) as i64);
    out.push(WindowInfo { hwnd, pid, title, area });
    1
}

/// Visible, unowned, non-tool top-level windows with a title.
pub fn windows() -> Vec<WindowInfo> {
    let mut out: Vec<WindowInfo> = Vec::new();
    unsafe { EnumWindows(Some(collect), &mut out as *mut Vec<WindowInfo> as LPARAM) };
    out
}

/// The biggest top-level window of a process.
pub fn main_window(pid: u32) -> Option<WindowInfo> {
    windows().into_iter().filter(|w| w.pid == pid).max_by_key(|w| w.area)
}

/// The full exe path of a process.
pub fn exe_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 32768];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut n);
        CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..n as usize]))
    }
}

pub fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

/// `<size>-<mtime>` of the exe file (bytes, unix seconds); empty when it can't be read.
pub fn stamp(path: Option<&str>) -> String {
    let Some(p) = path else { return String::new() };
    let Ok(meta) = std::fs::metadata(p) else { return String::new() };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}-{}", meta.len(), mtime)
}

/// The names of every module loaded in a process (ToolHelp), or None when they can't be read.
pub fn modules(pid: u32) -> Option<Vec<String>> {
    unsafe {
        let mut snap = INVALID_HANDLE_VALUE;
        // ToolHelp can fail with ERROR_BAD_LENGTH while the target loads a module: try again
        for _ in 0..5 {
            snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
            if snap != INVALID_HANDLE_VALUE || GetLastError() != ERROR_BAD_LENGTH {
                break;
            }
        }
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut out = Vec::new();
        let mut me: MODULEENTRY32W = std::mem::zeroed();
        me.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        let mut ok = Module32FirstW(snap, &mut me);
        while ok != 0 {
            let len = me.szModule.iter().position(|&c| c == 0).unwrap_or(me.szModule.len());
            out.push(String::from_utf16_lossy(&me.szModule[..len]));
            ok = Module32NextW(snap, &mut me);
        }
        CloseHandle(snap);
        (!out.is_empty()).then_some(out)
    }
}

fn tcp_table(family: u32) -> Vec<u8> {
    unsafe {
        let mut size: u32 = 0;
        GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, family, TCP_TABLE_OWNER_PID_ALL, 0);
        for _ in 0..4 {
            let mut buf = vec![0u8; size as usize + 4096];
            let mut s = buf.len() as u32;
            if GetExtendedTcpTable(buf.as_mut_ptr() as *mut c_void, &mut s, 0, family, TCP_TABLE_OWNER_PID_ALL, 0) == 0 {
                return buf;
            }
            size = s;
        }
        Vec::new()
    }
}

/// Remote addresses of ESTABLISHED TCP links, by owning pid.
pub fn tcp_remotes() -> BTreeMap<u32, Vec<IpAddr>> {
    let mut map: BTreeMap<u32, Vec<IpAddr>> = BTreeMap::new();
    let v4 = tcp_table(AF_INET as u32);
    if v4.len() >= 4 {
        let n = u32::from_ne_bytes([v4[0], v4[1], v4[2], v4[3]]) as usize;
        let row = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        // rows start after the 4-byte count, aligned to 4
        for i in 0..n {
            let off = 4 + i * row;
            if off + row > v4.len() {
                break;
            }
            let r: MIB_TCPROW_OWNER_PID = unsafe { std::ptr::read_unaligned(v4[off..].as_ptr() as *const _) };
            if r.dwState == MIB_TCP_STATE_ESTAB {
                let ip = IpAddr::V4(Ipv4Addr::from(r.dwRemoteAddr.to_ne_bytes()));
                map.entry(r.dwOwningPid).or_default().push(ip);
            }
        }
    }
    let v6 = tcp_table(AF_INET6 as u32);
    if v6.len() >= 4 {
        let n = u32::from_ne_bytes([v6[0], v6[1], v6[2], v6[3]]) as usize;
        let row = std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
        for i in 0..n {
            let off = 4 + i * row;
            if off + row > v6.len() {
                break;
            }
            let r: MIB_TCP6ROW_OWNER_PID = unsafe { std::ptr::read_unaligned(v6[off..].as_ptr() as *const _) };
            if r.dwState == MIB_TCP_STATE_ESTAB {
                let ip = IpAddr::V6(Ipv6Addr::from(r.ucRemoteAddr));
                map.entry(r.dwOwningPid).or_default().push(ip);
            }
        }
    }
    map
}

/// Why Worldsmith refuses this process, or None.
pub fn refusal(pid: u32, remotes: &BTreeMap<u32, Vec<IpAddr>>) -> Option<String> {
    let Some(mods) = modules(pid) else { return Some(refuse::unreadable()) };
    let empty = Vec::new();
    refuse::check(&mods, remotes.get(&pid).unwrap_or(&empty))
}

pub struct Game {
    pub pid: u32,
    pub exe: String,
    pub title: String,
    pub stamp: String,
    pub refused: Option<String>,
}

/// Processes with a visible top-level window, one entry each, with their refusal pre-computed.
pub fn games() -> Vec<Game> {
    let me = unsafe { GetCurrentProcessId() };
    let mut by_pid: BTreeMap<u32, WindowInfo> = BTreeMap::new();
    for w in windows() {
        if w.pid == 0 || w.pid == me {
            continue;
        }
        match by_pid.get(&w.pid) {
            Some(old) if old.area >= w.area => {}
            _ => {
                by_pid.insert(w.pid, w);
            }
        }
    }
    let remotes = tcp_remotes();
    let mut out = Vec::new();
    for (pid, w) in by_pid {
        let path = exe_path(pid);
        let exe = path.as_deref().map(file_name).unwrap_or_default();
        if exe.is_empty() || SYSTEM_EXES.contains(&exe.to_ascii_lowercase().as_str()) {
            continue;
        }
        let stamp = stamp(path.as_deref());
        let refused = refusal(pid, &remotes);
        out.push(Game { pid, exe, title: w.title, stamp, refused });
    }
    out
}
