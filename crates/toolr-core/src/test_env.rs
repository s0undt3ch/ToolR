//! Env mutation for unit tests. The env is process-global and tests run in parallel,
//! so one lock for the whole crate: a per-module lock only excludes its own module.

use std::ffi::{OsStr, OsString};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

static LOCK: RwLock<()> = RwLock::new(());

/// Holds the crate-wide env lock exclusively and restores every var it changed on drop.
pub(crate) struct EnvGuard {
    saved: Vec<(String, Option<OsString>)>,
    _lock: RwLockWriteGuard<'static, ()>,
}

impl EnvGuard {
    pub(crate) fn lock() -> Self {
        Self {
            saved: Vec::new(),
            _lock: LOCK.write().unwrap_or_else(|e| e.into_inner()),
        }
    }

    /// For tests whose code under test reads a var another test changes; they still run in
    /// parallel with each other.
    pub(crate) fn read() -> RwLockReadGuard<'static, ()> {
        LOCK.read().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn set(&mut self, key: &str, value: impl AsRef<OsStr>) {
        self.save(key);
        // SAFETY: every env mutation in this crate's tests goes through this lock.
        unsafe { std::env::set_var(key, value) };
    }

    pub(crate) fn remove(&mut self, key: &str) {
        self.save(key);
        // SAFETY: as for `set`.
        unsafe { std::env::remove_var(key) };
    }

    fn save(&mut self, key: &str) {
        if !self.saved.iter().any(|(k, _)| k == key) {
            self.saved.push((key.to_owned(), std::env::var_os(key)));
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..).rev() {
            // SAFETY: the lock is released only after this body returns.
            unsafe {
                match value {
                    Some(v) => std::env::set_var(&key, v),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }
}
