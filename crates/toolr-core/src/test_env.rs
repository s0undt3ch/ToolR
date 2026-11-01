//! Env mutation for unit tests. The env is process-global and tests run in parallel,
//! so one lock for the whole crate: a per-module lock only excludes its own module.
//!
//! Tests mutate `XDG_CACHE_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`,
//! `TOOLR_AUTO_INSTALL_UV`, `TOOLR_TRUST_ALL` and `TOOLR_VENV_LOCATION`. A test whose code reads one of
//! these takes [`EnvGuard::read`].

use std::ffi::{OsStr, OsString};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

static LOCK: RwLock<()> = RwLock::new(());

/// Holds the crate-wide env lock exclusively and restores every var it changed on drop.
pub(crate) struct EnvGuard {
    saved: Vec<(String, Option<OsString>)>,
    _lock: RwLockWriteGuard<'static, ()>,
}

impl EnvGuard {
    pub(crate) fn write() -> Self {
        Self {
            saved: Vec::new(),
            _lock: LOCK.write().unwrap_or_else(|e| e.into_inner()),
        }
    }

    /// Points `XDG_CACHE_HOME` at a fresh temp dir, so code under test that writes to toolr's
    /// cache (rebuild locks, provenance records) never touches the real one.
    pub(crate) fn temp_cache_home() -> (Self, tempfile::TempDir) {
        let mut env = Self::write();
        let dir = tempfile::TempDir::new().expect("create temp XDG_CACHE_HOME");
        env.set("XDG_CACHE_HOME", dir.path());
        (env, dir)
    }

    /// For tests whose code under test reads a var another test changes; they still run in
    /// parallel with each other.
    pub(crate) fn read() -> RwLockReadGuard<'static, ()> {
        LOCK.read().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn set(&mut self, key: &str, value: impl AsRef<OsStr>) {
        self.save(key);
        // SAFETY: mutations are serialised by this lock, and std's own env lock covers
        // concurrent reads through `std::env`; no test reads the env through C.
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
