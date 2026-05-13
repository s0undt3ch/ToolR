//! Forward SIGINT and SIGTERM from the parent toolr process to the
//! Python runner subprocess.
//!
//! Strategy: register signal handlers that write the received signal
//! number to an `Arc<AtomicI32>`. A polling loop in [`wait_with_signals`]
//! checks the atomic between waits and re-sends the signal to the child
//! pid.
//!
//! On Windows we install Ctrl-C handling only — SIGTERM does not exist
//! and Ctrl-C is propagated to the child by the console subsystem by
//! default, so this is effectively a no-op there.

use std::io;
use std::process::{Child, ExitStatus};
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};
#[cfg(unix)]
use std::time::Duration;

/// Wait for `child` to exit, forwarding SIGINT/SIGTERM received by the
/// current process to the child along the way.
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

    pub fn wait_with_signals(child: &mut Child) -> io::Result<ExitStatus> {
        let pending = Arc::new(AtomicI32::new(0));
        let mut signals = Signals::new([SIGINT, SIGTERM])?;
        let pending_for_thread = Arc::clone(&pending);
        let handle = signals.handle();

        let listener = std::thread::spawn(move || {
            for sig in &mut signals {
                pending_for_thread.store(sig, Ordering::SeqCst);
            }
        });

        let child_pid = child.id() as i32;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            let sig = pending.swap(0, Ordering::SeqCst);
            if sig != 0 {
                // Re-send to the child. Ignore errors (the child may have
                // just exited).
                // SAFETY: `kill` is a libc FFI call with no preconditions
                // beyond the pid being a valid signed int.
                unsafe {
                    libc::kill(child_pid, sig);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        };

        handle.close();
        let _ = listener.join();
        Ok(status)
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
}
