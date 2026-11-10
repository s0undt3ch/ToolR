//! Integration tests for `toolr project init`.

mod common;

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

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
    common::toolr()
        .current_dir(tmp.path())
        .args(["--quiet", "project", "init", "--no-sync"])
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
    common::toolr()
        .current_dir(tmp.path())
        .args(["--quiet", "project", "init", "--no-sync", "--no-example"])
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

    common::toolr()
        .current_dir(tmp.path())
        .args(["--quiet", "project", "init", "--no-sync"])
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

    let output = common::toolr()
        .current_dir(tmp.path())
        .args(["--quiet", "project", "init", "--no-sync"])
        .output()
        .unwrap();

    // Exit code 2 distinguishes "conflicts" from generic failure (1).
    assert_eq!(output.status.code(), Some(2), "expected exit code 2");
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

/// --yes auto-approves all conflict files without interactive prompts.
#[test]
fn init_yes_overwrites_conflicts_non_interactive() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(tmp.path().join("tools/pyproject.toml"), "# stale").unwrap();
    fs::write(tmp.path().join("tools/.gitignore"), "# stale").unwrap();

    common::toolr()
        .current_dir(tmp.path())
        .args(["--quiet", "project", "init", "--no-sync", "--yes"])
        .assert()
        .success();

    let pyproject = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
    assert!(pyproject.contains(r#"name = "tools""#));
    assert!(
        !fs::read_to_string(tmp.path().join("tools/.gitignore"))
            .unwrap()
            .contains("# stale")
    );
}

/// The overwrite prompt takes `yes` as well as `y`, and Enter or anything else declines.
#[cfg(unix)]
#[test]
fn init_overwrite_prompt_accepts_yes_and_declines_the_rest() {
    for (answer, overwritten) in [
        ("yes\n", true),
        ("y\n", true),
        ("\n", false),
        ("no\n", false),
    ] {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("tools")).unwrap();
        fs::write(tmp.path().join("tools/pyproject.toml"), "# stale").unwrap();

        let mut cmd = common::toolr_std();
        cmd.current_dir(tmp.path())
            .args(["--quiet", "project", "init", "--no-sync"]);
        let out = common::run_on_pty(cmd, answer);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{answer:?}: {stderr}");
        assert!(
            stderr.contains("overwrite tools/pyproject.toml? [y/N]"),
            "{stderr}"
        );
        let kept = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
        assert_eq!(kept != "# stale", overwritten, "{answer:?}");
    }
}

/// --force overwrites conflict files without prompting.
#[test]
fn init_force_overwrites_existing_tools() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(tmp.path().join("tools/pyproject.toml"), "# stale").unwrap();

    common::toolr()
        .current_dir(tmp.path())
        .args(["--quiet", "project", "init", "--no-sync", "--force"])
        .assert()
        .success();
    let pyproject = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
    assert!(pyproject.contains(r#"name = "tools""#));
}

/// Running init twice is idempotent — identical files are skipped.
#[test]
fn init_idempotent() {
    let tmp = TempDir::new().unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .args(["--quiet", "project", "init", "--no-sync"])
        .assert()
        .success();

    // Second run: no conflicts, nothing written, exit 0.
    common::toolr()
        .current_dir(tmp.path())
        .args(["--quiet", "project", "init", "--no-sync"])
        .assert()
        .success();
}

#[test]
#[cfg(unix)]
fn init_with_only_a_too_old_python3_writes_the_supported_floor() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let bin = TempDir::new().unwrap();
    let python3 = bin.path().join("python3");
    fs::write(&python3, "#!/bin/sh\necho 'Python 3.9.18'\n").unwrap();
    fs::set_permissions(&python3, fs::Permissions::from_mode(0o755)).unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .env("PATH", bin.path())
        .args(["--quiet", "project", "init", "--no-sync"])
        .assert()
        .success();
    let pyproject = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
    let floor = toolr_core::execute::python::MIN_PYTHON_MINOR;
    assert!(
        pyproject.contains(&format!(r#"requires-python = ">=3.{floor}""#)),
        "{pyproject}"
    );
}

/// Runs `project init` in a repo that ships its own `python3`, reporting a
/// minor above the floor and dropping a sentinel when run.
#[cfg(unix)]
fn init_with_repo_python3(path: &str) -> (String, bool) {
    let tmp = TempDir::new().unwrap();
    let sentinel = tmp.path().join("repo-python3-ran");
    fs::create_dir(tmp.path().join("rel-bin")).unwrap();
    let body = format!(
        "#!/bin/sh\n/usr/bin/touch '{}'\necho 'Python 3.99.0'\n",
        sentinel.display()
    );
    common::write_executable(&tmp.path().join("python3"), &body);
    common::write_executable(&tmp.path().join("rel-bin").join("python3"), &body);
    common::toolr()
        .current_dir(tmp.path())
        .env("PATH", path)
        .args(["--quiet", "project", "init", "--no-sync"])
        .assert()
        .success();
    let pyproject = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
    (pyproject, sentinel.exists())
}

#[cfg(unix)]
fn assert_repo_python3_never_ran(path: &str) {
    let floor = toolr_core::execute::python::MIN_PYTHON_MINOR;
    let (pyproject, ran) = init_with_repo_python3(path);
    assert!(!ran, "the repo's python3 ran with PATH={path:?}");
    assert!(
        pyproject.contains(&format!(r#"requires-python = ">=3.{floor}""#)),
        "PATH={path:?}:\n{pyproject}"
    );
}

#[test]
#[cfg(unix)]
fn init_dot_path_entry_does_not_run_repo_python3() {
    assert_repo_python3_never_ran(".");
}

#[test]
#[cfg(unix)]
fn init_empty_path_entry_does_not_run_repo_python3() {
    assert_repo_python3_never_ran(":");
}

#[test]
#[cfg(unix)]
fn init_relative_path_entry_does_not_run_repo_python3() {
    assert_repo_python3_never_ran("rel-bin");
}

#[test]
fn init_in_tree_writes_correct_venv_location() {
    let tmp = TempDir::new().unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .args([
            "--quiet",
            "project",
            "init",
            "--no-sync",
            "--venv-location",
            "in-tree",
        ])
        .assert()
        .success();
    let pyproject = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
    assert!(pyproject.contains(r#"venv-location = "in-tree""#));
}

#[test]
fn init_example_has_all_four_commands() {
    let tmp = TempDir::new().unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .args(["--quiet", "project", "init", "--no-sync"])
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
#[cfg(unix)]
fn init_then_run_example_hello() {
    let Some(python) = detect_test_python() else {
        eprintln!("skipping: no .venv/bin/python with toolr installed");
        return;
    };
    let tmp = TempDir::new().unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .args([
            "--quiet",
            "project",
            "init",
            "--no-sync",
            "--venv-location",
            "in-tree",
        ])
        .assert()
        .success();
    // Link the test python's venv in as the in-tree tools venv, so dispatch
    // runs it without `uv`.
    let venv = python.parent().and_then(std::path::Path::parent).unwrap();
    std::os::unix::fs::symlink(venv, tmp.path().join("tools/.venv")).unwrap();

    let output = common::toolr()
        .current_dir(tmp.path())
        .env("XDG_CACHE_HOME", tmp.path().join(".cache"))
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
