//! Wait for the Python runner subprocess, forwarding SIGTERM to it.
//!
//! The runner shares toolr's process group. Ctrl-C, or a `killpg` from a
//! process manager, already delivers SIGINT to the whole group, so the
//! runner gets it directly. toolr catches SIGINT only to survive it and
//! report the runner's exit code; re-sending it would raise a second
//! `KeyboardInterrupt` inside the runner's cleanup. SIGTERM aimed at toolr
//! alone is forwarded.
//!
//! Strategy: the signal listener sets an `Arc<AtomicBool>` when SIGTERM
//! arrives. A polling loop in [`wait_with_signals`] checks the flag between
//! waits and sends SIGTERM to the child pid, which `try_wait` has not yet
//! reaped.
//!
//! On Windows we install Ctrl-C handling only — SIGTERM does not exist
//! and Ctrl-C is propagated to the child by the console subsystem by
//! default, so this is effectively a no-op there.

use std::io;
use std::process::{Child, ExitStatus};
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::time::Duration;

/// Wait for `child` to exit, forwarding SIGTERM received by the current
/// process to the child along the way. SIGINT is caught but not forwarded.
pub fn wait_with_signals(child: &mut Child) -> io::Result<ExitStatus> {
    #[cfg(unix)]
    {
        unix::wait_with_signals(child)
    }
    #[cfg(not(unix))]
    {
        // No portable signal forwarding outside Unix. Just wait.
        child.wait()
    }
}

#[cfg(unix)]
mod unix {
    use super::*;

    use signal_hook::consts::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    /// SIGINT already reached the runner through the process group.
    pub(super) fn should_forward(sig: i32) -> bool {
        sig == SIGTERM
    }

    pub(super) fn record(pending: &AtomicBool, sig: i32) {
        if should_forward(sig) {
            pending.store(true, Ordering::SeqCst);
        }
    }

    pub(super) fn take(pending: &AtomicBool) -> bool {
        pending.swap(false, Ordering::SeqCst)
    }

    pub fn wait_with_signals(child: &mut Child) -> io::Result<ExitStatus> {
        let pending = Arc::new(AtomicBool::new(false));
        // SIGINT stays registered so Ctrl-C doesn't kill toolr.
        let mut signals = Signals::new([SIGINT, SIGTERM])?;
        let pending_for_thread = Arc::clone(&pending);
        let handle = signals.handle();

        let listener = std::thread::spawn(move || listen(&mut signals, &pending_for_thread));

        let status = poll_child(child, &pending);

        handle.close();
        let _ = listener.join();
        status
    }

    pub(super) fn listen(signals: impl IntoIterator<Item = i32>, pending: &AtomicBool) {
        for sig in signals {
            record(pending, sig);
        }
    }

    /// Only signals a child that `try_wait` has not yet reaped, so the pid
    /// can't have been reused.
    pub(super) fn poll_child(child: &mut Child, pending: &AtomicBool) -> io::Result<ExitStatus> {
        let child_pid = child.id() as i32;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            if take(pending) {
                // Ignore errors (the child may have just exited).
                // SAFETY: `kill` is a libc FFI call with no preconditions
                // beyond the pid being a valid signed int.
                unsafe {
                    libc::kill(child_pid, SIGTERM);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::process::Command;

    /// `true` exits immediately with status 0. Unix-only — the whole
    /// test module is gated so Windows builds don't drag in the
    /// otherwise-unused imports.
    #[test]
    fn wait_returns_for_quick_child() {
        let mut child = Command::new("true").spawn().expect("spawn true");
        let status = wait_with_signals(&mut child).expect("wait");
        assert!(status.success());
    }

    #[test]
    fn sigint_is_not_forwarded() {
        use signal_hook::consts::{SIGINT, SIGTERM};
        assert!(!unix::should_forward(SIGINT));
        assert!(unix::should_forward(SIGTERM));
    }

    #[test]
    fn sigterm_survives_a_preceding_sigint() {
        use signal_hook::consts::{SIGINT, SIGTERM};
        use std::sync::atomic::AtomicBool;
        let pending = AtomicBool::new(false);
        unix::record(&pending, SIGTERM);
        unix::record(&pending, SIGINT);
        assert!(unix::take(&pending));
        assert!(!unix::take(&pending));
    }

    #[test]
    fn listener_records_only_sigterm() {
        use signal_hook::consts::{SIGINT, SIGTERM};
        use std::sync::atomic::AtomicBool;
        let pending = AtomicBool::new(false);
        unix::listen([SIGINT], &pending);
        assert!(!unix::take(&pending));
        unix::listen([SIGINT, SIGTERM], &pending);
        assert!(unix::take(&pending));
    }

    /// Bounded by `sleep 10`: if SIGTERM isn't sent, the child exits 0 and
    /// the assertion fails instead of hanging.
    #[test]
    fn pending_sigterm_reaches_the_child() {
        use signal_hook::consts::SIGTERM;
        use std::os::unix::process::ExitStatusExt;
        use std::sync::atomic::AtomicBool;
        let mut child = Command::new("sleep")
            .arg("10")
            .spawn()
            .expect("spawn sleep");
        let pending = AtomicBool::new(true);
        let status = unix::poll_child(&mut child, &pending).expect("wait");
        assert_eq!(status.signal(), Some(SIGTERM));
    }
}
