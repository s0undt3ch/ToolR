//! Wait for the Python runner subprocess, forwarding SIGTERM, and on Linux
//! a process-sent SIGINT, to it.
//!
//! The runner shares toolr's process group. Ctrl-C makes the terminal
//! deliver SIGINT to the whole group, so the runner already has it; sending
//! it again would raise a second `KeyboardInterrupt` inside the runner's
//! cleanup. toolr catches SIGINT so Ctrl-C doesn't kill it, and reports the
//! runner's exit code.
//!
//! Which SIGINTs get forwarded depends on what the kernel tells us about the
//! sender (`siginfo_t::si_code`):
//!
//! - Linux: a terminal-generated SIGINT carries `SI_KERNEL` and is not
//!   forwarded. One sent by a process with `kill`/`killpg`/`tgkill`/
//!   `sigqueue` is forwarded, so `kill -INT <toolr pid>` still interrupts
//!   the command. A process-sent `killpg` therefore reaches the runner twice.
//!   Unrecognised codes are forwarded too: only the terminal path is
//!   positively identified.
//! - macOS and other Unix: `si_code` is left at 0 for both a terminal Ctrl-C
//!   and a `kill`, so they can't be told apart. SIGINT is never forwarded;
//!   signal the process group or send SIGTERM instead.
//!
//! SIGTERM is always forwarded. The handler only sets atomic flags; a
//! polling loop in [`wait_with_signals`] checks them between waits and
//! signals the child pid, which `try_wait` has not yet reaped.
//!
//! On Windows nothing is installed: the console already delivers Ctrl-C to
//! the child, and toolr just waits.

use std::io;
use std::process::{Child, ExitStatus};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::time::Duration;

/// Wait for `child` to exit, forwarding signals received by the current
/// process to the child as described in the module docs.
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

    use libc::{SIGINT, SIGTERM, c_int, siginfo_t};
    use std::sync::Arc;

    /// Whether a SIGINT with this `si_code` came from another process rather
    /// than the terminal, which already delivered it to the runner.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(super) fn forward_sigint(si_code: c_int) -> bool {
        si_code != libc::SI_KERNEL
    }

    /// `si_code` is 0 for a terminal Ctrl-C and a `kill` alike here.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub(super) fn forward_sigint(_si_code: c_int) -> bool {
        false
    }

    #[derive(Default)]
    pub(super) struct Pending {
        pub(super) sigint: AtomicBool,
        pub(super) sigterm: AtomicBool,
    }

    impl Pending {
        /// Async-signal-safe: only touches atomics.
        pub(super) fn record(&self, info: &siginfo_t) {
            match info.si_signo {
                SIGTERM => self.sigterm.store(true, Ordering::SeqCst),
                SIGINT if forward_sigint(info.si_code) => self.sigint.store(true, Ordering::SeqCst),
                _ => {}
            }
        }

        /// The signals to forward since the last call, SIGINT first.
        pub(super) fn take(&self) -> impl Iterator<Item = c_int> {
            let sigint = self.sigint.swap(false, Ordering::SeqCst).then_some(SIGINT);
            let sigterm = self
                .sigterm
                .swap(false, Ordering::SeqCst)
                .then_some(SIGTERM);
            sigint.into_iter().chain(sigterm)
        }
    }

    pub fn wait_with_signals(child: &mut Child) -> io::Result<ExitStatus> {
        let pending = Arc::new(Pending::default());
        let mut ids = Vec::with_capacity(2);
        for sig in [SIGINT, SIGTERM] {
            let pending = Arc::clone(&pending);
            // SAFETY: the action only stores to atomics, which is
            // async-signal-safe.
            let id = unsafe {
                signal_hook_registry::register_sigaction(sig, move |info| pending.record(info))
            };
            match id {
                Ok(id) => ids.push(id),
                Err(err) => {
                    ids.into_iter().for_each(|id| {
                        signal_hook_registry::unregister(id);
                    });
                    return Err(err);
                }
            }
        }

        let status = poll_child(child, &pending);

        for id in ids {
            signal_hook_registry::unregister(id);
        }
        status
    }

    /// Only signals a child that `try_wait` has not yet reaped, so the pid
    /// can't have been reused.
    pub(super) fn poll_child(child: &mut Child, pending: &Pending) -> io::Result<ExitStatus> {
        let child_pid = child.id() as i32;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            for sig in pending.take() {
                // Ignore errors (the child may have just exited).
                // SAFETY: `kill` is a libc FFI call with no preconditions
                // beyond the pid being a valid signed int.
                unsafe {
                    libc::kill(child_pid, sig);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::unix::{Pending, forward_sigint, poll_child};
    use super::*;
    use libc::{SIGINT, SIGTERM, c_int, siginfo_t};
    use std::os::unix::process::ExitStatusExt;
    use std::process::Command;

    fn info(signo: c_int, code: c_int) -> siginfo_t {
        // SAFETY: `siginfo_t` is plain old data; all-zero is a valid value.
        let mut info: siginfo_t = unsafe { std::mem::zeroed() };
        info.si_signo = signo;
        info.si_code = code;
        info
    }

    /// The `si_code` a `kill()` from another process carries.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const KILL_CODE: c_int = libc::SI_USER;
    /// Measured on macOS: `kill()` and a terminal Ctrl-C both report 0.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const KILL_CODE: c_int = 0;

    fn taken(pending: &Pending) -> Vec<c_int> {
        pending.take().collect()
    }

    /// `true` exits immediately with status 0. Unix-only — the whole
    /// test module is gated so Windows builds don't drag in the
    /// otherwise-unused imports.
    #[test]
    fn wait_returns_for_quick_child() {
        let mut child = Command::new("true").spawn().expect("spawn true");
        let status = wait_with_signals(&mut child).expect("wait");
        assert!(status.success());
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn linux_forwards_only_process_sent_sigint() {
        assert!(!forward_sigint(libc::SI_KERNEL));
        assert!(forward_sigint(libc::SI_USER));
        assert!(forward_sigint(libc::SI_TKILL));
        assert!(forward_sigint(libc::SI_QUEUE));
        // Unrecognised codes are forwarded.
        assert!(forward_sigint(12345));
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    #[test]
    fn sigint_is_never_forwarded_without_a_reliable_sender() {
        assert!(!forward_sigint(KILL_CODE));
    }

    #[test]
    fn record_follows_the_classification() {
        let pending = Pending::default();
        pending.record(&info(SIGINT, KILL_CODE));
        let forwarded = cfg!(any(target_os = "linux", target_os = "android"));
        assert_eq!(
            taken(&pending),
            if forwarded { vec![SIGINT] } else { vec![] }
        );
    }

    #[test]
    fn sigterm_is_always_recorded_and_survives_a_sigint() {
        let pending = Pending::default();
        pending.record(&info(SIGTERM, 0));
        pending.record(&info(SIGINT, 0));
        assert!(taken(&pending).contains(&SIGTERM));
        assert_eq!(taken(&pending), Vec::<c_int>::new());
    }

    #[test]
    fn other_signals_are_ignored() {
        let pending = Pending::default();
        pending.record(&info(libc::SIGHUP, KILL_CODE));
        assert_eq!(taken(&pending), Vec::<c_int>::new());
    }

    #[test]
    fn take_yields_sigint_before_sigterm() {
        let pending = Pending::default();
        pending
            .sigint
            .store(true, std::sync::atomic::Ordering::SeqCst);
        pending
            .sigterm
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(taken(&pending), vec![SIGINT, SIGTERM]);
    }

    /// Bounded by `sleep 10`: if the signal isn't sent, the child exits 0 and
    /// the assertion fails instead of hanging.
    #[test]
    fn pending_signals_reach_the_child() {
        for (sig, flag) in [(SIGTERM, true), (SIGINT, false)] {
            let mut child = Command::new("sleep")
                .arg("10")
                .spawn()
                .expect("spawn sleep");
            let pending = Pending::default();
            let slot = if flag {
                &pending.sigterm
            } else {
                &pending.sigint
            };
            slot.store(true, std::sync::atomic::Ordering::SeqCst);
            let status = poll_child(&mut child, &pending).expect("wait");
            assert_eq!(status.signal(), Some(sig));
        }
    }
}
