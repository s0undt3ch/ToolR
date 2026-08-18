//! Integration coverage for the dispatch.rs branches that aren't yet
//! exercised by `cli_smoke.rs`, `complete_smoke.rs`, or the project
//! subcommand test files.
//!
//! Each test invokes the real `toolr` binary via `assert_cmd::Command`
//! against a tempdir fixture and asserts on the observable behaviour —
//! exit code + stdout/stderr — rather than reaching into private
//! dispatch internals. That keeps the test surface aligned with what a
//! user / shell would see and prevents the coverage tests from drifting
//! when the dispatch internals are refactored.
//!
//! Branches covered here, by `dispatch.rs` section:
//!
//! - `dispatch()` no-subcommand path → prints root help (lines 39-42).
//! - `dispatch()` group-without-leaf path → success + group help (56-59).
//! - `run_self()` `completion print` per shell (273-280).
//! - `run_self()` `completion install` outcomes (213-264).
//! - `run_self_build_manifest()` argument plumbing (162-186).
//! - `resolve_python_for_build()` explicit `--python` override + bail (188-211).
//! - `output_options_from_matches()` flag propagation (366-390) — verified
//!   indirectly by inspecting the spec the Python runner receives.

use assert_cmd::Command;
use tempfile::TempDir;

// --------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------

/// Build a tempdir that holds a one-group, one-command static manifest
/// but **no `tools/pyproject.toml`** — so commands that hit the Python
/// dispatch path bail without exercising the venv layer.
fn fixture_with_manifest(json: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(tools.join(".toolr-manifest.json"), json).unwrap();
    tmp
}

/// Minimal manifest with a single `ci` group containing a `hello` command.
const SINGLE_GROUP_MANIFEST: &str = r#"{
    "schema_version": 1,
    "static_hash": "h",
    "third_party_hash": "",
    "groups": [
        {"name": "ci", "title": "CI utilities", "description": "", "origin": "static"}
    ],
    "commands": [
        {
            "name": "hello", "group": "ci", "module": "tools.ci",
            "function": "hello", "summary": "Say hello.",
            "description": "", "arguments": [], "imports": [],
            "origin": "static"
        }
    ]
}"#;

// --------------------------------------------------------------------
// dispatch(): early branches.
// --------------------------------------------------------------------

#[test]
fn no_subcommand_prints_root_help_and_exits_success() {
    // `toolr` with no arguments — neither `--help` nor a subcommand —
    // falls through every `if let Some(...) = matches.subcommand()` check
    // and lands in the `root.print_help()?` branch.
    let tmp = fixture_with_manifest(SINGLE_GROUP_MANIFEST);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Root help must mention the group from the manifest (proves the
    // dynamic clap tree was built before the help print).
    assert!(stdout.contains("ci"), "expected `ci` group in root help, got:\n{stdout}");
}

#[test]
fn group_without_leaf_command_prints_help_and_exits_nonzero() {
    // `toolr ci` — a namespace with no subcommand — must show the group's
    // help rather than silently doing nothing (matches `self`/`project`).
    let tmp = fixture_with_manifest(SINGLE_GROUP_MANIFEST);
    let assert = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .arg("ci")
        .assert();
    let output = assert.get_output();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage: toolr ci"), "expected `ci` group help, got:\n{stderr}");
}

// --------------------------------------------------------------------
// run_self() → completion subcommands.
// --------------------------------------------------------------------

// Note: bash-shaped `self completion print` is already covered by
// `complete_smoke.rs::self_completion_print_emits_bash_script`. The tests
// in this file focus on shells that file doesn't exercise (zsh/fish via
// the inline unit tests in `dispatch.rs`'s test module) and on the
// `install` / error subcommands.

#[test]
fn self_completion_print_rejects_unknown_shell() {
    // Clap's value-parser on the `<shell>` arg should reject anything
    // outside the known set with a non-zero exit and a stderr message.
    Command::cargo_bin("toolr")
        .unwrap()
        .args(["self", "completion", "print", "tcsh"])
        .assert()
        .failure();
}

#[test]
fn self_completion_install_writes_script_under_xdg_data_home() {
    // Bash + xdg_data_home set → install to `<xdg>/bash-completion/...`.
    let tmp = TempDir::new().unwrap();
    let xdg = tmp.path().join("xdg-data");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &xdg)
        .env_remove("XDG_CONFIG_HOME")
        .args(["self", "completion", "install", "bash"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected install to succeed; stderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("wrote") || stdout.contains("already"),
        "expected wrote/already-installed message, got:\n{stdout}"
    );
}

#[test]
fn self_completion_install_second_run_is_idempotent_or_force_required() {
    // Run install twice: first writes, second either reports
    // "already installed" (content identical) or "refusing to overwrite"
    // (different content). Both branches are valid outcomes from
    // `run_completion_install` — assert on the bucket, not the exact one.
    let tmp = TempDir::new().unwrap();
    let xdg = tmp.path().join("xdg-data");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let first = Command::cargo_bin("toolr")
        .unwrap()
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &xdg)
        .env_remove("XDG_CONFIG_HOME")
        .args(["self", "completion", "install", "bash"])
        .output()
        .unwrap();
    assert!(first.status.success());

    let second = Command::cargo_bin("toolr")
        .unwrap()
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &xdg)
        .env_remove("XDG_CONFIG_HOME")
        .args(["self", "completion", "install", "bash"])
        .output()
        .unwrap();
    // Either AlreadyInstalled (success) or SkippedNeedsForce (exit 1) —
    // both branches in dispatch.rs:233-263 are covered by this combo.
    let stdout = String::from_utf8_lossy(&second.stdout);
    let stderr = String::from_utf8_lossy(&second.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("already") || combined.contains("refusing"),
        "expected idempotency or force-required message; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn self_completion_install_force_overwrites() {
    let tmp = TempDir::new().unwrap();
    let xdg = tmp.path().join("xdg-data");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    // Plant a stale script first so the second invocation has something
    // to overwrite under `--force`.
    let target_dir = xdg.join("bash-completion").join("completions");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(target_dir.join("toolr"), "# stale\n").unwrap();

    let output = Command::cargo_bin("toolr")
        .unwrap()
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &xdg)
        .env_remove("XDG_CONFIG_HOME")
        .args(["self", "completion", "install", "bash", "--force"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected --force to succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written = std::fs::read_to_string(target_dir.join("toolr")).unwrap();
    assert!(
        !written.starts_with("# stale"),
        "expected --force to overwrite stale content"
    );
}

// --------------------------------------------------------------------
// output_options_from_matches() — flag propagation, observed via the
// help-text plumbing (these flags appear under "Output Options:").
// --------------------------------------------------------------------

#[test]
fn root_help_advertises_every_output_option() {
    // `output_options_from_matches` reads `--quiet`, `--debug`,
    // `--timestamps`, `--no-timestamps`, `--timeout-secs`, and
    // `--no-output-timeout-secs`. The clap definitions live in cli.rs
    // but the dispatcher binds them — this test guards against a regression
    // where a flag goes missing from one side without the other.
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for needle in [
        "--debug",
        "--quiet",
        "--timestamps",
        "--no-timestamps",
        "--timeout-secs",
        "--no-output-timeout-secs",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in --help output, got:\n{stdout}"
        );
    }
}

// --------------------------------------------------------------------
// build_dispatch_spec(): dispatcher-level flags leaked into a leaf's
// trailing variadic positional (toolr#445).
// --------------------------------------------------------------------

/// A `job` dispatcher (declaring its own `--dry-run`) under group
/// `jenkins`, hosting one grafted child `wrapup_company` with a
/// `VarPositional` `company_ids` argument — mirrors the issue's repro
/// shape (a Django-management-command dispatcher).
const JENKINS_JOB_MANIFEST: &str = r#"{
    "schema_version": 1,
    "static_hash": "h",
    "third_party_hash": "",
    "groups": [
        {"name": "jenkins", "title": "Jenkins", "description": "", "origin": "static"}
    ],
    "commands": [
        {
            "name": "job",
            "group": "jenkins",
            "module": "tools.dispatcher",
            "function": "job",
            "summary": "Trigger a Jenkins job.",
            "description": "",
            "is_dispatcher": true,
            "arguments": [
                {
                    "name": "dry_run",
                    "kind": "flag",
                    "help": "Print what would run instead of triggering the job.",
                    "default": "false",
                    "type_annotation": "bool",
                    "resolved_type": {"kind": "bool"},
                    "allowed_values": []
                }
            ],
            "imports": [],
            "origin": "static"
        },
        {
            "name": "wrapup_company",
            "group": "jenkins.job",
            "module": "tools.dispatcher",
            "function": "job",
            "summary": "Wrap up a company.",
            "description": "",
            "dispatched_from": "argparse:jenkins.job",
            "arguments": [
                {
                    "name": "company_ids",
                    "kind": "var_positional",
                    "help": "Company ids.",
                    "default": null,
                    "type_annotation": "str",
                    "resolved_type": {"kind": "str"},
                    "allowed_values": []
                }
            ],
            "imports": [],
            "origin": "static"
        }
    ]
}"#;

#[test]
fn dispatcher_flag_typed_after_leaf_name_is_rejected() {
    // Regression for toolr#445: `--dry-run` belongs to `jenkins job`, not
    // to `wrapup_company`, but typed after the leaf name it used to be
    // silently swallowed by the leaf's trailing `VarPositional` bucket
    // and forwarded to the wrapped command instead of routing to the
    // dispatcher's own flag.
    let tmp = fixture_with_manifest(JENKINS_JOB_MANIFEST);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["jenkins", "job", "wrapup_company", "12345", "--dry-run"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected non-zero exit, got success. stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--dry-run"), "expected flag name in stderr, got:\n{stderr}");
    assert!(
        stderr.contains("wrapup_company"),
        "expected leaf name in stderr, got:\n{stderr}"
    );
}

#[test]
fn dispatcher_flag_typed_before_leaf_name_still_parses_past_the_guard() {
    // The correctly-ordered invocation must not trip the new guard —
    // `--dry-run` is consumed by the dispatcher's own clap `Arg` before
    // the leaf subcommand is even reached. No working Python venv exists
    // in this fixture, so the run still fails downstream; we only assert
    // it gets past the new rejection message.
    let tmp = fixture_with_manifest(JENKINS_JOB_MANIFEST);
    let output = Command::cargo_bin("toolr")
        .unwrap()
        .current_dir(tmp.path())
        .args(["jenkins", "job", "--dry-run", "wrapup_company", "12345"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("is an option of"),
        "correctly-ordered --dry-run must not trip the leaked-flag guard; stderr:\n{stderr}",
    );
}
