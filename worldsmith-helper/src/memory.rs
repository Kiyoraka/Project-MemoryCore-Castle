//! The attached process: a user-mode handle, its writable regions, reads and writes.
//! Only OpenProcess / VirtualQueryEx / ReadProcessMemory / WriteProcessMemory, with the user's
//! own rights. Nothing is injected, nothing is debugged.

use crate::scan;
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION};
use windows_sys::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_READ, PROCESS_VM_WRITE, PROCESS_SYNCHRONIZE,
};

pub const PAGE: usize = 4096;
const CHUNK: usize = 1 << 20;

pub struct Proc {
    h: HANDLE,
    pub pid: u32,
}

// The handle is a kernel handle; using it from several threads is fine.
unsafe impl Send for Proc {}
unsafe impl Sync for Proc {}

impl Drop for Proc {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.h) };
    }
}

impl Proc {
    pub fn open(pid: u32) -> Result<Proc, String> {
        let access = PROCESS_VM_READ
            | PROCESS_VM_WRITE
            | PROCESS_VM_OPERATION
            | PROCESS_QUERY_INFORMATION
            | PROCESS_SYNCHRONIZE;
        let h = unsafe { OpenProcess(access, 0, pid) };
        if h.is_null() {
            return Err(format!(
                "Couldn't open process {pid} - it may have closed, or it runs as administrator."
            ));
        }
        Ok(Proc { h, pid })
    }

    /// Still running?
    pub fn alive(&self) -> bool {
        unsafe { WaitForSingleObject(self.h, 0) != WAIT_OBJECT_0 }
    }

    /// Fill `buf` from `addr`; false unless every byte was read.
    pub fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
        if buf.is_empty() {
            return true;
        }
        let mut n = 0usize;
        let ok = unsafe {
            ReadProcessMemory(self.h, addr as *const c_void, buf.as_mut_ptr() as *mut c_void, buf.len(), &mut n)
        };
        ok != 0 && n == buf.len()
    }

    pub fn write(&self, addr: u64, bytes: &[u8]) -> bool {
        let mut n = 0usize;
        let ok = unsafe {
            WriteProcessMemory(self.h, addr as *const c_void, bytes.as_ptr() as *const c_void, bytes.len(), &mut n)
        };
        ok != 0 && n == bytes.len()
    }

    /// The committed, writable private / image regions (see `scan::is_scannable`).
    pub fn regions(&self) -> Vec<(u64, usize)> {
        let mut out = Vec::new();
        let mut addr: usize = 0;
        loop {
            let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
            let got = unsafe {
                VirtualQueryEx(self.h, addr as *const c_void, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>())
            };
            if got == 0 {
                break;
            }
            let base = mbi.BaseAddress as usize;
            let size = mbi.RegionSize;
            if scan::is_scannable(mbi.State, mbi.Protect, mbi.Type) && size > 0 {
                out.push((base as u64, size));
            }
            let next = base.saturating_add(size);
            if next <= addr || size == 0 {
                break;
            }
            addr = next;
        }
        out
    }

    /// Read a whole region: its bytes, and which 4 KB pages could be read (a failed page is zero).
    pub fn read_region(&self, base: u64, len: usize) -> (Vec<u8>, Vec<bool>) {
        let mut bytes = vec![0u8; len];
        let mut ok = vec![true; len.div_ceil(PAGE)];
        let mut off = 0;
        while off < len {
            let n = CHUNK.min(len - off);
            if !self.read(base + off as u64, &mut bytes[off..off + n]) {
                let mut p = off;
                while p < off + n {
                    let pn = PAGE.min(off + n - p);
                    if !self.read(base + p as u64, &mut bytes[p..p + pn]) {
                        bytes[p..p + pn].fill(0);
                        ok[p / PAGE] = false;
                    }
                    p += pn;
                }
            }
            off += n;
        }
        (bytes, ok)
    }

    /// Walk the writable regions in 1 MB chunks (plus 8 bytes of overlap), calling
    /// `f(buf, base, limit)`; a chunk that can't be read whole is walked page by page.
    /// Stops when `f` returns false.
    pub fn for_each_chunk(&self, mut f: impl FnMut(&[u8], u64, usize) -> bool) {
        let mut buf = vec![0u8; CHUNK + 8];
        for (rbase, rlen) in self.regions() {
            let mut off = 0usize;
            while off < rlen {
                let n = CHUNK.min(rlen - off);
                let with_overlap = (n + 8).min(rlen - off);
                let addr = rbase + off as u64;
                if self.read(addr, &mut buf[..with_overlap]) {
                    if !f(&buf[..with_overlap], addr, n) {
                        return;
                    }
                } else {
                    let mut p = 0;
                    while p < n {
                        let pn = PAGE.min(n - p);
                        if self.read(addr + p as u64, &mut buf[..pn]) && !f(&buf[..pn], addr + p as u64, pn) {
                            return;
                        }
                        p += pn;
                    }
                }
                off += n;
            }
        }
    }
}
