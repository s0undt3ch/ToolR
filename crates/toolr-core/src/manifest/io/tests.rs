use std::io::{Error, ErrorKind};

use super::{READ_ATTEMPTS, read_retrying};

fn denied() -> Error {
    Error::from(ErrorKind::PermissionDenied)
}

#[test]
fn transient_errors_are_retried_until_the_read_succeeds() {
    let mut calls = 0;
    let read = || {
        calls += 1;
        if calls < 4 {
            Err(denied())
        } else {
            Ok(b"ok".to_vec())
        }
    };
    assert_eq!(read_retrying(read, |_| true).unwrap(), b"ok");
    assert_eq!(calls, 4);
}

#[test]
fn persistent_transient_errors_give_up_after_the_last_attempt() {
    let mut calls = 0;
    let read = || {
        calls += 1;
        Err(denied())
    };
    assert!(read_retrying(read, |_| true).is_err());
    assert_eq!(calls, READ_ATTEMPTS);
}

#[test]
fn other_errors_are_not_retried() {
    let mut calls = 0;
    let read = || {
        calls += 1;
        Err(denied())
    };
    assert!(read_retrying(read, |_| false).is_err());
    assert_eq!(calls, 1);
}
