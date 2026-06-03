//! End-to-end coverage for `arg(conflicts_with=[…])`: a manifest that
//! cross-references two flags via `metadata.conflicts_with` must make
//! clap reject the conflicting invocation at parse time (before any
//! Python is spawned).
//!
//! The static AST parser and the `arg()` helper each have their own
//! unit tests; this file fills the gap between them — that the
//! metadata round-trips through the manifest and into clap's
//! `conflicts_with_all`, the dispatch surface our users actually hit.

use assert_cmd::Command;
use tempfile::TempDir;

const MANIFEST: &str = r#"{
    "schema_version": 1,
    "static_hash": "h",
    "third_party_hash": "",
    "groups": [
        {"name": "probe", "title": "Probe", "description": "", "origin": "static"}
    ],
    "commands": [
        {
            "name": "follow",
            "group": "probe",
            "module": "tools.probe",
            "function": "follow",
            "summary": "Mutex probe.",
            "description": "",
            "arguments": [
                {
                    "name": "follow",
                    "kind": "flag",
                    "help": "tail logs.",
                    "default": "false",
                    "type_annotation": "bool",
                    "resolved_type": {"kind": "bool"},
                    "allowed_values": [],
                    "metadata": {"conflicts_with": ["no_follow"]}
                },
                {
                    "name": "no_follow",
                    "kind": "flag",
                    "help": "do not tail logs.",
                    "default": "false",
                    "type_annotation": "bool",
                    "resolved_type": {"kind": "bool"},
                    "allowed_values": [],
                    "metadata": {"conflicts_with": ["follow"]}
                }
            ],
            "imports": [],
            "origin": "static"
        }
    ]
}"#;

fn fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(tools.join(".toolr-manifest.json"), MANIFEST).unwrap();
    tmp
}

#[test]
fn conflicting_flags_are_rejected_at_parse_time() {
    let tmp = fixture();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["probe", "follow", "--follow", "--no-follow"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected non-zero exit, got success. stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected clap usage-error exit code 2; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "expected clap conflict message in stderr; got:\n{stderr}",
    );
    assert!(
        stderr.contains("--follow") && stderr.contains("--no-follow"),
        "expected both flag spellings in the conflict message; got:\n{stderr}",
    );
}

#[test]
fn either_flag_alone_passes_clap_parse() {
    // Without Python, dispatch fails downstream — but it must get
    // *past* clap's conflict gate, proving the mutex only fires for
    // the conflicting combo. We assert clap's "cannot be used with"
    // message is absent rather than asserting success, so the test
    // doesn't depend on a working tools venv.
    for flag in ["--follow", "--no-follow"] {
        let tmp = fixture();
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["probe", "follow", flag])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("cannot be used with"),
            "{flag} alone should not trigger the mutex; stderr:\n{stderr}",
        );
    }
}

// Re-enabled in Task 11 once dispatch intercepts --help for user commands.
#[test]
#[ignore]
fn help_still_renders_with_conflicts_with_metadata() {
    // Regression guard: clap historically panicked when an arg listed
    // a `conflicts_with` target that no other arg matched. The
    // manifest here cross-references the names correctly, but we
    // verify `--help` builds the command tree cleanly all the same.
    let tmp = fixture();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["probe", "follow", "--help"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "--help failed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--follow"), "help missing --follow:\n{stdout}");
    assert!(stdout.contains("--no-follow"), "help missing --no-follow:\n{stdout}");
}
