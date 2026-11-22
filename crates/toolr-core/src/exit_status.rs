//! The only place a child's `ExitStatus` becomes a number: `returncode` for Python (`ctx.run`, as
//! `subprocess` reports it), `process_exit_code` for toolr's own exit (as a shell reports it).

use std::process::ExitStatus;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt as _;

/// The exit code, or `-N` for a child killed by signal `N`. Windows statuses pass through unchanged.
pub fn returncode(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = status.signal() {
        return -signal;
    }
    status.code().unwrap_or(-1)
}

/// The exit code if it fits in a byte, `128 + N` for a child killed by signal `N`, else 1, so a
/// failing child never makes toolr exit 0.
pub fn process_exit_code(status: ExitStatus) -> u8 {
    #[cfg(unix)]
    if let Some(signal) = status.signal() {
        return u8::try_from(128 + signal).unwrap_or(1);
    }
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests;
