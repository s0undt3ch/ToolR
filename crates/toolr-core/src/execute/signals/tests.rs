use super::unix::{Handlers, Pending, forward_sigint, poll_child};
use super::*;
use libc::{SIGINT, SIGTERM, c_int, siginfo_t};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Command;

fn info(signo: c_int, code: c_int) -> siginfo_t {
    // SAFETY: `siginfo_t` is plain old data; all-zero is a valid value.
    let mut info: siginfo_t = unsafe { std::mem::zeroed() };
    info.si_signo = signo;
    info.si_code = code;
    info
}

/// The `si_code` a `kill()` from another process carries: `SI_USER` on
/// Linux, and on macOS the 0 it reports for a terminal Ctrl-C as well.
const KILL_CODE: c_int = 0;

fn taken(pending: &Pending) -> Vec<c_int> {
    pending.take().collect()
}

/// `true` exits immediately with status 0.
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
    assert_eq!(taken(&pending) == [SIGINT], forward_sigint(KILL_CODE));
}

#[test]
fn sigterm_is_always_recorded_and_survives_a_sigint() {
    let pending = Pending::default();
    pending.record(&info(SIGTERM, KILL_CODE));
    pending.record(&info(SIGINT, KILL_CODE));
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
fn take_yields_every_pending_signal_once() {
    let pending = Pending::default();
    pending.sigint.store(true, Ordering::SeqCst);
    pending.sigterm.store(true, Ordering::SeqCst);
    let mut signals = taken(&pending);
    signals.sort_unstable();
    assert_eq!(signals, vec![SIGINT, SIGTERM]);
    assert_eq!(taken(&pending), Vec::<c_int>::new());
}

/// 9999 is above `NSIG` everywhere, so `sigaction` rejects it with
/// `EINVAL` after SIGTERM was registered, and the guard unwinds it.
#[test]
fn install_fails_for_an_invalid_signal() {
    let pending = std::sync::Arc::new(Pending::default());
    assert!(Handlers::install(&[SIGTERM, 9999], &pending).is_err());
}

/// Bounded by `sleep 10`: if no signal is sent, the child exits 0 and the
/// caller's assertion fails instead of hanging.
///
/// A shell's `&` job runs the tests with SIGINT ignored, and that survives exec, so the
/// child resets it (SIGTERM too, for symmetry). CI never hits this, so keep the reset.
fn sleeper_status(pending: &Pending) -> ExitStatus {
    let mut command = Command::new("sleep");
    command.arg("10");
    // SAFETY: `signal` is async-signal-safe, so it may run between fork and exec.
    unsafe {
        command.pre_exec(|| {
            libc::signal(SIGINT, libc::SIG_DFL);
            libc::signal(SIGTERM, libc::SIG_DFL);
            Ok(())
        });
    }
    let mut child = command.spawn().expect("spawn sleep");
    poll_child(&mut child, pending).expect("wait")
}

#[test]
fn pending_sigterm_reaches_the_child() {
    let pending = Pending::default();
    pending.sigterm.store(true, Ordering::SeqCst);
    assert_eq!(sleeper_status(&pending).signal(), Some(SIGTERM));
}

#[test]
fn pending_sigint_reaches_the_child() {
    let pending = Pending::default();
    pending.sigint.store(true, Ordering::SeqCst);
    assert_eq!(sleeper_status(&pending).signal(), Some(SIGINT));
}
