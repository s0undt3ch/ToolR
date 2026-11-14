//! A tools file's parse error quotes the offending token from the repo; it must reach the
//! terminal with control characters escaped, on the stale-cache warning and the fatal path.

mod common;

use common::Project;

const HOSTILE: &str = "\x1b]0;pwn\x07 = 1\n";

fn assert_escaped(stderr: &str) {
    assert!(
        !stderr.chars().any(|c| c.is_control() && c != '\n'),
        "stderr:\n{stderr:?}"
    );
    assert!(stderr.contains("\\u{1b}"), "stderr:\n{stderr}");
}

#[test]
fn fatal_parse_error_on_a_fresh_clone_is_escaped() {
    let p = Project::new();
    p.write_tool("bad.py", HOSTILE);
    let out = p.toolr(&["--help"]).output().unwrap();
    assert!(!out.status.success());
    assert_escaped(&String::from_utf8(out.stderr).unwrap());
}

#[test]
fn stale_cache_warning_is_escaped() {
    let p = Project::new();
    p.stderr(&["--help"]);
    p.write_tool("bad.py", HOSTILE);
    let stderr = p.stderr(&["--help"]);
    assert!(
        stderr.contains("falling back to cached manifest"),
        "{stderr}"
    );
    assert_escaped(&stderr);
}
