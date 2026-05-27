//! Idempotency contract for `cargo xtask build-skill-refs`.
//!
//! The `--check` gate that protects every generated `references/*.md`
//! against drift only works if successive runs of the generator
//! produce byte-identical output. Without this guarantee, `--check`
//! is testing the wrong thing — drift in the generator itself would
//! be indistinguishable from drift in the documented source.
//!
//! This test runs the generator twice against the live workspace and
//! asserts each file the second pass produces matches the first
//! pass's bytes exactly.
//!
//! The invocation uses `assert_cmd::Command::cargo_bin("xtask")` to
//! point at the test-instrumented binary cargo builds alongside the
//! test runner. That binary carries `-Cinstrument-coverage` and
//! writes a profraw on every run, so `cargo llvm-cov` merges its
//! coverage into the rest of the workspace's profile. (An earlier
//! version of this test shelled out to `cargo run --package xtask
//! --release`; that rebuilds the binary without coverage flags and
//! contributes nothing to codecov — see PR #233 review notes.)

use std::path::PathBuf;
use std::{fs, io};

use assert_cmd::Command;

#[test]
fn build_skill_refs_is_byte_stable_across_runs() {
    let workspace = workspace_root();
    let generated_paths = [
        workspace.join("skills/toolr-command-authoring/references/commands.md"),
        workspace.join("skills/toolr-command-authoring/references/docstrings.md"),
        workspace.join("skills/toolr-command-packaging/references/packaging.md"),
    ];

    // Snapshot the committed bytes so we can restore them at the end
    // — the test should leave the working tree untouched even if it
    // happens to fail mid-flight.
    let originals = snapshot(&generated_paths);

    let run1 = run_generator(&workspace);
    let first = snapshot(&generated_paths);

    let run2 = run_generator(&workspace);
    let second = snapshot(&generated_paths);

    // Restore the committed bytes regardless of outcome.
    restore(&originals);

    assert!(run1, "first `xtask build-skill-refs` invocation failed");
    assert!(run2, "second `xtask build-skill-refs` invocation failed");

    for (path, before) in &first {
        let after = second
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, body)| body)
            .expect("missing path in second pass snapshot");
        assert_eq!(
            before,
            after,
            "two successive `xtask build-skill-refs` runs produced \
             different bytes for {}. The generator is not idempotent, \
             which would make --check unreliable.",
            path.display(),
        );
    }
}

fn run_generator(workspace: &PathBuf) -> bool {
    // `cargo_bin("xtask")` resolves to the test-instrumented binary
    // built alongside this integration test (`target/debug/xtask`
    // under `cargo test`). Invoking it directly — rather than via
    // `cargo run` — keeps coverage instrumentation active in the
    // spawned process so `cargo llvm-cov` picks up the profraws.
    Command::cargo_bin("xtask")
        .expect("xtask binary present in cargo's bin output dir")
        .args(["build-skill-refs"])
        .current_dir(workspace)
        .assert()
        .try_success()
        .is_ok()
}

fn snapshot(paths: &[PathBuf]) -> Vec<(PathBuf, Option<String>)> {
    paths.iter().map(|p| (p.clone(), read_or_none(p))).collect()
}

fn restore(snapshot: &[(PathBuf, Option<String>)]) {
    for (path, body) in snapshot {
        match body {
            Some(bytes) => {
                if let Err(e) = fs::write(path, bytes) {
                    eprintln!("warning: could not restore {}: {e}", path.display());
                }
            }
            None => {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn read_or_none(path: &PathBuf) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => panic!("reading {}: {e}", path.display()),
    }
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at `crates/xtask`; the workspace root
    // is two levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(PathBuf::from)
        .expect("workspace root two levels above CARGO_MANIFEST_DIR")
}
