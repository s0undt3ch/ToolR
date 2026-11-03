//! The only place a child's `ExitStatus` becomes a number: `returncode` for Python (`ctx.run`, as
//! `subprocess` reports it), `process_exit_code` for toolr's own exit (as a shell reports it).

use std::process::ExitStatus;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt as _;

/// The exit code, or `-N` for a child killed by signal `N`. Windows statuses pass through unchanged.
pub fn returncode(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    if let Some(signal) = status.signal() {
        return -signal;
    }
    -1
}

/// The exit code if it fits in a byte, `128 + N` for a child killed by signal `N`, else 1, so a
/// failing child never makes toolr exit 0.
pub fn process_exit_code(status: ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return u8::try_from(code).unwrap_or(1);
    }
    #[cfg(unix)]
    if let Some(signal) = status.signal() {
        return u8::try_from(128 + signal).unwrap_or(1);
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn normal_exit_passes_through_both_renderers() {
        let status = ExitStatus::from_raw(3 << 8);
        assert_eq!(returncode(status), 3);
        assert_eq!(process_exit_code(status), 3);
    }

    #[cfg(unix)]
    #[test]
    fn returncode_is_negative_signal_for_signal_death() {
        assert_eq!(returncode(ExitStatus::from_raw(libc::SIGTERM)), -15);
        assert_eq!(returncode(ExitStatus::from_raw(libc::SIGKILL)), -9);
    }

    #[cfg(unix)]
    #[test]
    fn process_exit_code_is_128_plus_signal_for_signal_death() {
        assert_eq!(process_exit_code(ExitStatus::from_raw(libc::SIGTERM)), 143);
        assert_eq!(process_exit_code(ExitStatus::from_raw(libc::SIGKILL)), 137);
    }

    #[cfg(unix)]
    #[test]
    fn stopped_status_falls_back_to_minus_one_and_one() {
        // A stopped child (WIFSTOPPED, here by SIGSTOP) has neither an exit code nor a terminating signal.
        let status = ExitStatus::from_raw((libc::SIGSTOP << 8) | 0x7f);
        assert_eq!(returncode(status), -1);
        assert_eq!(process_exit_code(status), 1);
    }

    #[cfg(windows)]
    #[test]
    fn windows_crash_status_passes_through_returncode_and_exits_one() {
        use std::os::windows::process::ExitStatusExt as _;
        let status = ExitStatus::from_raw(0xC000_0005);
        assert_eq!(returncode(status), 0xC000_0005_u32 as i32);
        assert_eq!(process_exit_code(status), 1);
    }

    #[cfg(windows)]
    #[test]
    fn process_exit_code_maps_out_of_range_to_one() {
        use std::os::windows::process::ExitStatusExt as _;
        let status = ExitStatus::from_raw(256);
        assert_eq!(returncode(status), 256);
        assert_eq!(process_exit_code(status), 1);
    }
}
