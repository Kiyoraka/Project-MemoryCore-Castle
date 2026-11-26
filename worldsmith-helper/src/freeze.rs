//! The attached game and its frozen values, shared between the request loop and the freeze
//! thread that rewrites every frozen value about every 50 ms.

use crate::memory::Proc;
use crate::scan::ValueType;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub const FREEZE_EVERY: Duration = Duration::from_millis(50);

pub struct Attached {
    pub proc: Proc,
    pub exe: String,
    pub title: String,
}

#[derive(Clone)]
pub struct Frozen {
    pub ty: ValueType,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
pub struct Shared {
    target: Mutex<Option<Arc<Attached>>>,
    freezes: Mutex<BTreeMap<u64, Frozen>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Shared {
    pub fn target(&self) -> Option<Arc<Attached>> {
        lock(&self.target).clone()
    }

    /// Attach (dropping any earlier game and its freezes).
    pub fn set_target(&self, a: Attached) {
        lock(&self.freezes).clear();
        *lock(&self.target) = Some(Arc::new(a));
    }

    /// Stop every freeze and drop the handle.
    pub fn detach(&self) {
        lock(&self.freezes).clear();
        *lock(&self.target) = None;
    }

    pub fn freeze(&self, addr: u64, f: Frozen) {
        lock(&self.freezes).insert(addr, f);
    }

    pub fn unfreeze(&self, addr: u64) {
        lock(&self.freezes).remove(&addr);
    }

    pub fn freezes(&self) -> Vec<(u64, Frozen)> {
        lock(&self.freezes).iter().map(|(a, f)| (*a, f.clone())).collect()
    }

    fn is_current(&self, t: &Arc<Attached>) -> bool {
        lock(&self.target).as_ref().map(|c| Arc::ptr_eq(c, t)).unwrap_or(false)
    }

    /// One freeze pass. A game that has exited is detached.
    pub fn tick(&self) {
        let Some(t) = self.target() else { return };
        // lock order everywhere: freezes, then target
        if !t.proc.alive() {
            let mut freezes = lock(&self.freezes);
            let mut cur = lock(&self.target);
            if cur.as_ref().map(|c| Arc::ptr_eq(c, &t)).unwrap_or(false) {
                *cur = None;
                freezes.clear();
            }
            return;
        }
        // the lock is held while writing, so a detach or unfreeze never races a late write
        let freezes = lock(&self.freezes);
        if !self.is_current(&t) {
            return;
        }
        for (addr, f) in freezes.iter() {
            t.proc.write(*addr, &f.bytes);
        }
    }
}

/// Start the freeze thread.
pub fn start(shared: Arc<Shared>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(FREEZE_EVERY);
        shared.tick();
    });
}
