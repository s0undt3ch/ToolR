//! Integration tests for `toolr project init`.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_bin() -> Command {
    Command::cargo_bin("toolr").unwrap()
}

fn detect_test_python() -> Option<PathBuf> {
    let candidate = std::env::var_os("TOOLR_TEST_PYTHON").map(PathBuf::from);
    let candidate = candidate.or_else(|| {
        let p = PathBuf::from(".venv/bin/python");
        if p.exists() { Some(p) } else { None }
    })?;
    let python = if candidate.is_absolute() {
        candidate
    } else {
        std::env::current_dir().ok()?.join(candidate)
    };
    let status = std::process::Command::new(&python)
        .args(["-c", "import toolr._runner"])
        .status()
        .ok()?;
    if status.success() { Some(python) } else { None }
}

#[test]
fn init_no_sync_writes_three_files() {
    let tmp = TempDir::new().unwrap();
    cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync", "--quiet"])
        .assert()
        .success();
    assert!(tmp.path().join("tools/pyproject.toml").is_file());
    assert!(tmp.path().join("tools/.gitignore").is_file());
    assert!(tmp.path().join("tools/example.py").is_file());
    assert!(!tmp.path().join("tools/__init__.py").exists());
}

#[test]
fn init_no_example_skips_example_py() {
    let tmp = TempDir::new().unwrap();
    cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync", "--no-example", "--quiet"])
        .assert()
        .success();
    assert!(tmp.path().join("tools/pyproject.toml").is_file());
    assert!(!tmp.path().join("tools/example.py").exists());
}

/// tools/ has unrelated files only → no conflict, scaffold writes alongside them.
#[test]
fn init_writes_alongside_unrelated_files() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(tmp.path().join("tools/my_tool.py"), "# custom").unwrap();

    cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync", "--quiet"])
        .assert()
        .success();

    assert!(tmp.path().join("tools/pyproject.toml").is_file());
    assert!(tmp.path().join("tools/.gitignore").is_file());
    assert!(tmp.path().join("tools/example.py").is_file());
    // Unrelated file untouched.
    assert_eq!(
        fs::read_to_string(tmp.path().join("tools/my_tool.py")).unwrap(),
        "# custom"
    );
}

/// tools/ has scaffold files with different content; non-interactive → hard error with list.
#[test]
fn init_fails_on_conflict_non_interactive() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(tmp.path().join("tools/pyproject.toml"), "# stale").unwrap();

    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync", "--quiet"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("overwritten") || stderr.contains("already exists"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("pyproject.toml"), "stderr:\n{stderr}");
    // Conflicting file must be preserved.
    assert_eq!(
        fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap(),
        "# stale"
    );
}

/// --force overwrites conflict files without prompting.
#[test]
fn init_force_overwrites_existing_tools() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(tmp.path().join("tools/pyproject.toml"), "# stale").unwrap();

    cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync", "--force", "--quiet"])
        .assert()
        .success();
    let pyproject = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
    assert!(pyproject.contains(r#"name = "tools""#));
}

/// Running init twice is idempotent — identical files are skipped.
#[test]
fn init_idempotent() {
    let tmp = TempDir::new().unwrap();
    cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync", "--quiet"])
        .assert()
        .success();

    // Second run: no conflicts, nothing written, exit 0.
    cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync", "--quiet"])
        .assert()
        .success();
}

#[test]
fn init_in_tree_writes_correct_venv_location() {
    let tmp = TempDir::new().unwrap();
    cargo_bin()
        .current_dir(tmp.path())
        .args([
            "project",
            "init",
            "--no-sync",
            "--venv-location",
            "in-tree",
            "--quiet",
        ])
        .assert()
        .success();
    let pyproject = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
    assert!(pyproject.contains(r#"venv-location = "in-tree""#));
}

#[test]
fn init_example_has_all_four_commands() {
    let tmp = TempDir::new().unwrap();
    cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync", "--quiet"])
        .assert()
        .success();
    let example = fs::read_to_string(tmp.path().join("tools/example.py")).unwrap();
    assert!(example.contains("def hello("));
    assert!(example.contains("def commit("));
    assert!(example.contains("def confirm("));
    assert!(example.contains("def setlog("));
    assert!(example.contains("Literal["));
}

/// End-to-end: scaffold + run `toolr example hello` against the result.
/// Skipped if no usable Python is available.
#[test]
fn init_then_run_example_hello() {
    let Some(python) = detect_test_python() else {
        eprintln!("skipping: no .venv/bin/python with toolr installed");
        return;
    };
    let tmp = TempDir::new().unwrap();
    cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "init", "--no-sync", "--quiet"])
        .assert()
        .success();
    // Remove the scaffolded pyproject so the dispatcher falls back to
    // the legacy TOOLR_PYTHON path (otherwise it would try to resolve a
    // real tools venv via uv, which this test isn't set up for).
    fs::remove_file(tmp.path().join("tools/pyproject.toml")).unwrap();

    // Build the static manifest so dispatch knows about the `example`
    // group/command. Without this clap has no subcommand registered.
    cargo_bin()
        .current_dir(tmp.path())
        .arg("__build-static-manifest")
        .assert()
        .success();

    let output = cargo_bin()
        .current_dir(tmp.path())
        .env("TOOLR_PYTHON", &python)
        .env("PYTHONPATH", tmp.path())
        .args(["example", "hello", "--name", "Plan10"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected success, got {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code()
    );
    assert!(stdout.contains("hello, Plan10"), "stdout was:\n{stdout}");
}
