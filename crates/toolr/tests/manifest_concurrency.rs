//! Regression tests for #542: concurrent `toolr` processes sharing one
//! `tools/.toolr-manifest.json`, and an unreadable manifest never being
//! silently replaced by an empty command set.

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

/// Enough commands that the serialised manifest spans many write calls,
/// widening the window a non-atomic rewrite would expose.
const COMMAND_COUNT: usize = 300;

fn fixtures_py() -> String {
    let mut src = String::from(
        "from toolr import Context, command_group\n\n\
         fixtures = command_group(\"fixtures\", \"Fixture commands\")\n",
    );
    for i in 0..COMMAND_COUNT {
        src.push_str(&format!(
            "\n@fixtures.command\ndef cmd_{i}(ctx: Context, name: str = \"world\") -> None:\n    \
             \"\"\"Fixture command number {i}.\"\"\"\n    ctx.print(name)\n"
        ));
    }
    src
}

fn write_project(root: &Path) {
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname = \"tools\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    fs::write(tools.join("fixtures.py"), fixtures_py()).unwrap();
}

/// Seed a parseable but stale manifest so every process takes the
/// rebuild-and-rewrite path at once.
fn write_stale_manifest(root: &Path) {
    fs::write(
        root.join("tools").join(".toolr-manifest.json"),
        r#"{"schema_version": 1, "static_hash": "stale", "third_party_hash": "",
            "groups": [], "commands": []}"#,
    )
    .unwrap();
}

fn toolr(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(cargo_bin("toolr"));
    cmd.args(args)
        .current_dir(root)
        .env("TOOLR_NO_CACHE_HINT", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

#[test]
fn concurrent_processes_against_stale_manifest_all_dispatch() {
    const PROCESSES: usize = 16;
    const ROUNDS: usize = 8;

    let tmp = TempDir::new().unwrap();
    write_project(tmp.path());

    for round in 0..ROUNDS {
        write_stale_manifest(tmp.path());
        let children: Vec<_> = (0..PROCESSES)
            .map(|_| toolr(tmp.path(), &["fixtures", "--help"]).spawn().unwrap())
            .collect();
        let outputs: Vec<Output> = children
            .into_iter()
            .map(|c| c.wait_with_output().unwrap())
            .collect();
        for (i, out) in outputs.iter().enumerate() {
            assert!(
                out.status.success(),
                "round {round}, process {i} failed to dispatch `fixtures --help`:\n\
                 stderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                stdout.contains("cmd-0"),
                "round {round}, process {i} rendered the wrong command set:\n{stdout}"
            );
        }
    }
}

#[test]
fn unreadable_manifest_is_reported_not_treated_as_empty() {
    // The freshness rebuild fails (syntax error) and the on-disk manifest
    // is garbage, so there is no usable command set. That must surface
    // as a manifest error, not clap's misleading `unrecognized subcommand`.
    let tmp = TempDir::new().unwrap();
    write_project(tmp.path());
    fs::write(
        tmp.path().join("tools").join("broken.py"),
        "def not closed(",
    )
    .unwrap();
    fs::write(
        tmp.path().join("tools").join(".toolr-manifest.json"),
        "{\"schema_ver",
    )
    .unwrap();

    let out = toolr(tmp.path(), &["fixtures", "--help"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected failure, got: {out:?}");
    assert!(
        !stderr.contains("unrecognized subcommand"),
        "unreadable manifest was silently treated as empty:\n{stderr}"
    );
    assert!(
        stderr.contains(".toolr-manifest.json"),
        "expected the manifest path in the error:\n{stderr}"
    );
}

#[test]
fn unreadable_manifest_does_not_block_builtins() {
    // `project manifest rebuild` is how a user repairs a corrupt manifest;
    // the corrupt file must not stop built-ins from parsing.
    let tmp = TempDir::new().unwrap();
    write_project(tmp.path());
    fs::write(tmp.path().join("tools").join(".toolr-manifest.json"), "").unwrap();

    let out = toolr(tmp.path(), &["project", "manifest", "--help"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "built-in blocked by corrupt manifest: {out:?}"
    );
}
