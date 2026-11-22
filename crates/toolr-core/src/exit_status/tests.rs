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
    assert_eq!(
        returncode(ExitStatus::from_raw(libc::SIGTERM)),
        -libc::SIGTERM
    );
    assert_eq!(
        returncode(ExitStatus::from_raw(libc::SIGKILL)),
        -libc::SIGKILL
    );
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
fn out_of_range_windows_code_passes_through_returncode_and_exits_one() {
    use std::os::windows::process::ExitStatusExt as _;
    // A crash NTSTATUS (negative as an `i32`) and a plain code above a byte.
    for raw in [0xC000_0005_u32, 256] {
        let status = ExitStatus::from_raw(raw);
        assert_eq!(returncode(status), raw as i32);
        assert_eq!(process_exit_code(status), 1);
    }
}
