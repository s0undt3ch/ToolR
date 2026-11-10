mod common;

use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;
#[cfg(unix)]
use toolr_core::venv::VenvLocation;

fn fixture_with_manifest(json: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    common::write_fresh_manifest(tmp.path(), json);
    tmp
}

#[test]
fn version_flag_works_with_no_project() {
    let tmp = TempDir::new().unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .arg("--version")
        .assert()
        .success();
}

#[test]
fn help_lists_groups_from_manifest() {
    let json = r#"{
        "schema_version": 2,
        "static_hash": "h",
        "third_party_hash": "",
        "groups": [
            {"name": "ci", "title": "CI utilities", "description": "", "origin": "static"}
        ],
        "commands": [
            {
                "name": "hello", "group": "ci", "module": "tools.ci",
                "function": "hello", "summary": "Say hello.",
                "description": "", "arguments": [],
                "origin": "static"
            }
        ]
    }"#;
    let tmp = fixture_with_manifest(json);
    let output = common::toolr()
        .current_dir(tmp.path())
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ci"),
        "expected ci group in help, got:\n{stdout}"
    );
}

/// Returns `Some(path-to-python)` if a Python with msgspec + the local
/// `toolr` package installed is available, otherwise `None`. We accept
/// the project's own dev venv (created by `uv sync`) as the runner.
///
/// The fixtures link that venv in as the project's in-tree tools venv, so
/// dispatch runs it without `uv`. The full sync-driven path is covered by
/// the network-dependent `end_to_end_sync` test.
fn detect_test_python() -> Option<PathBuf> {
    let candidate = std::env::var_os("TOOLR_TEST_PYTHON").map(PathBuf::from);
    let candidate = candidate.or_else(|| {
        // Project dev venv from `uv sync`.
        let p = PathBuf::from(".venv/bin/python");
        if p.exists() { Some(p) } else { None }
    })?;
    // Make absolute without canonicalizing (canonicalize resolves the
    // venv symlink to the real interpreter, which loses the venv's
    // site-packages — and with it, the `toolr` editable install).
    // Tests later set `current_dir` to a tmpdir, so a relative path
    // wouldn't survive across the spawn.
    let python = if candidate.is_absolute() {
        candidate
    } else {
        std::env::current_dir().ok()?.join(candidate)
    };
    // Verify it can import `toolr._runner`. If not, skip.
    let status = std::process::Command::new(&python)
        .args(["-c", "import toolr._runner"])
        .status()
        .ok()?;
    if status.success() { Some(python) } else { None }
}

/// Make `repo_root` a project whose `tools/<module>.py` holds `source`, with
/// `python`'s venv linked in as the in-tree tools venv and a fresh manifest.
#[cfg(unix)]
fn write_demo_project(
    repo_root: &std::path::Path,
    python: &std::path::Path,
    source: &str,
    manifest: &str,
) {
    let tools_dir = common::write_project(repo_root);
    // A regular package, so this checkout's own `tools/` (on the dev venv's
    // path ahead of the appended repo root) can't shadow it.
    std::fs::write(tools_dir.join("__init__.py"), "").unwrap();
    std::fs::write(tools_dir.join("demo.py"), source).unwrap();
    let venv = python.parent().and_then(std::path::Path::parent).unwrap();
    std::os::unix::fs::symlink(venv, tools_dir.join(".venv")).unwrap();
    common::write_fresh_manifest(repo_root, manifest);
}

#[cfg(unix)]
fn write_tools_demo(repo_root: &std::path::Path, python: &std::path::Path) {
    write_demo_project(
        repo_root,
        python,
        r#"
from toolr import command_group

group = command_group("demo", "Demo", description="demo group")

@group.command
def hello(ctx, name: str = "world") -> None:
    """Say hi."""
    ctx.print(f"hi {name}")
"#,
        r#"{
        "groups": [{"name": "demo", "title": "Demo", "description": "", "origin": "static"}],
        "commands": [{
            "name": "hello", "group": "demo", "module": "tools.demo",
            "function": "hello", "summary": "", "description": "",
            "arguments": [
                {
                    "name": "name", "kind": "optional", "help": "",
                    "default": "world", "type_annotation": "str",
                    "allowed_values": []
                }
            ],
            "origin": "static"
        }]
    }"#,
    );
}

#[test]
#[cfg(unix)]
fn running_a_user_command_invokes_python_runner() {
    let Some(python) = detect_test_python() else {
        eprintln!(
            "skipping: no .venv/bin/python with toolr installed. \
             Run `uv sync` first, or set TOOLR_TEST_PYTHON to a python \
             that can `import toolr._runner`."
        );
        return;
    };
    let tmp = TempDir::new().unwrap();
    write_tools_demo(tmp.path(), &python);
    let output = common::toolr()
        .current_dir(tmp.path())
        .env("XDG_CACHE_HOME", tmp.path().join(".cache"))
        .args(["demo", "hello", "--name", "Alice"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected success, got code {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code()
    );
    assert!(stdout.contains("hi Alice"), "stdout was:\n{stdout}");
}

#[test]
fn project_manifest_rebuild_help_lists_command() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    let output = common::toolr()
        .current_dir(tmp.path())
        .args(["project", "manifest", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("rebuild"),
        "expected rebuild listed, got:\n{stdout}"
    );
}

#[test]
fn self_build_manifest_help_works() {
    let output = common::toolr()
        .args(["self", "build-manifest", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Generate a third-party manifest fragment"),
        "unexpected help text: {stdout}"
    );
}

/// Build a fixture project with:
/// - `tools/pyproject.toml` opting into the in-tree venv layout so
///   `resolve_venv_path` lands at `tools/.venv/`.
/// - `tools/.venv/lib/python3.13/site-packages/` containing the modules
///   listed in `present_in_venv` (each materialised as a package with an
///   `__init__.py`).
/// - `tools/.venv/bin/python` as a fake interpreter that just exits 1
///   when the runner is spawned (so we exercise stdout/stderr handling).
/// - A `tools/.toolr-manifest.json` with one `ci hello` command.
#[cfg(unix)]
fn preflight_fixture(present_in_venv: &[&str]) -> tempfile::TempDir {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname = \"tools\"\nversion = \"0.0.0\"\n\
         [tool.toolr]\nvenv-location = \"in-tree\"\n",
    )
    .unwrap();
    fs::write(tools.join("__init__.py"), "").unwrap();
    fs::write(tools.join("ci.py"), "def hello(ctx): pass\n").unwrap();

    // Build the venv tree before computing hashes so the third_party_hash
    // accounts for the final site-packages state.
    let sp = tools
        .join(".venv")
        .join("lib")
        .join("python3.13")
        .join("site-packages");
    fs::create_dir_all(&sp).unwrap();
    for name in present_in_venv {
        let pkg = sp.join(name);
        fs::create_dir(&pkg).unwrap();
        fs::write(pkg.join("__init__.py"), "").unwrap();
    }
    // `toolr` itself must appear installed so the venv-validate guard
    // (if any) doesn't trip the dispatcher before pre-flight runs.
    let toolr_pkg = sp.join("toolr");
    fs::create_dir_all(&toolr_pkg).unwrap();
    fs::write(toolr_pkg.join("__init__.py"), "").unwrap();

    // Stamp real hashes so ensure_manifest_fresh treats the manifest as
    // Fresh and doesn't rebuild it.
    let static_hash = toolr_core::hash::compute_static_hash(&tools).unwrap();
    let venv_dir = tools.join(".venv");
    let third_party_hash = toolr_core::manifest_build::compute_third_party_hash(&venv_dir).unwrap();
    let toolr_version = env!("CARGO_PKG_VERSION");

    let manifest = format!(
        r#"{{
            "schema_version": 2,
            "static_hash": "{static_hash}", "third_party_hash": "{third_party_hash}",
            "toolr_version": "{toolr_version}",
            "groups": [{{
                "name": "ci", "title": "CI", "description": "",
                "origin": "static"
            }}],
            "commands": [{{
                "name": "hello", "group": "ci", "module": "tools.ci",
                "function": "hello", "summary": "", "description": "",
                "arguments": [],
                "origin": "static"
            }}]
        }}"#
    );
    fs::write(tools.join(".toolr-manifest.json"), manifest).unwrap();

    let bin_dir = tools.join(".venv").join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let py = bin_dir.join("python");
    let mut f = fs::File::create(&py).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    // The dispatcher invokes `python -m toolr._runner` during command
    // execution; the default branch exits non-zero so a command "runs"
    // without a real Python.
    writeln!(f, "exit 1").unwrap();
    drop(f);
    let mut perms = fs::metadata(&py).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py, perms).unwrap();

    tmp
}

/// `$XDG_CACHE_HOME` value used by the preflight fixtures: a sibling of
/// the repo under the same TempDir.
#[cfg(unix)]
fn preflight_cache_home(repo: &std::path::Path) -> PathBuf {
    repo.join(".xdg-cache")
}

/// Build a `toolr` command for the preflight fixtures with the
/// sandboxed `XDG_CACHE_HOME`, so the run never touches the real cache.
#[cfg(unix)]
fn preflight_cmd(repo: &std::path::Path) -> Command {
    let mut cmd = common::toolr();
    cmd.current_dir(repo)
        .env("XDG_CACHE_HOME", preflight_cache_home(repo));
    cmd
}

/// Regression: deleting the resolved tools venv's `bin/python` between
/// invocations used to surface as the bare error
/// `toolr: No such file or directory (os error 2)` — the user couldn't
/// tell what was missing or how to recover. Dispatch now pre-checks
/// `python.is_file()` and emits a message that names the path plus
/// `toolr project venv sync` as the recovery action.
#[test]
#[cfg(unix)]
fn dispatch_emits_clear_error_when_venv_python_is_missing() {
    use std::fs;

    let tmp = preflight_fixture(&[]);
    let py = tmp
        .path()
        .join("tools")
        .join(".venv")
        .join("bin")
        .join("python");
    assert!(py.exists(), "fixture should start with a stub python");
    fs::remove_file(&py).unwrap();

    let output = preflight_cmd(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected failure; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Python interpreter not found at"),
        "expected a 'Python interpreter not found at <path>' line; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("toolr project venv sync"),
        "expected recovery hint; stderr:\n{stderr}"
    );
    // Anti-regression: the old bare `os error 2` line must not be the
    // entire message.
    assert!(
        !stderr
            .trim()
            .ends_with("No such file or directory (os error 2)")
            || stderr.contains("Python interpreter not found at"),
        "stderr should not be just the bare io::Error display; stderr:\n{stderr}"
    );
}

/// Regression: dispatching against an in-tree venv used to write the
/// `meta.json` sidecar to `venv_dir.parent()`, i.e. an untracked
/// `tools/meta.json` that `toolr self cache list` never saw. It must land in
/// the out-of-repo cache slot `toolr project venv sync` already uses.
#[test]
#[cfg(unix)]
fn dispatch_in_tree_venv_keeps_meta_json_out_of_tools() {
    let tmp = preflight_fixture(&[]);

    // The fake python exits 1; only the sidecar side effect matters here.
    let output = preflight_cmd(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !tmp.path().join("tools/meta.json").exists(),
        "dispatch must not write tools/meta.json; stderr:\n{stderr}"
    );

    let cache_root = preflight_cache_home(tmp.path()).join("toolr");
    let metas: Vec<PathBuf> = std::fs::read_dir(&cache_root)
        .unwrap_or_else(|e| panic!("no cache root {cache_root:?} ({e}); stderr:\n{stderr}"))
        .map(|entry| entry.unwrap().path().join("meta.json"))
        .filter(|p| p.is_file())
        .collect();
    assert_eq!(
        metas.len(),
        1,
        "expected one cache meta.json, got {metas:?}; stderr:\n{stderr}"
    );
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&metas[0]).unwrap()).unwrap();
    let canonical = tmp.path().canonicalize().unwrap();
    assert_eq!(
        meta["repo_path"].as_str().map(PathBuf::from),
        Some(canonical.clone()),
        "meta.json repo_path should be the canonical repo; meta: {meta}"
    );

    let list = preflight_cmd(tmp.path())
        .args(["self", "cache", "list"])
        .output()
        .unwrap();
    let list_stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        list_stdout.contains(&*canonical.to_string_lossy()),
        "`self cache list` should show the in-tree repo; stdout:\n{list_stdout}"
    );
}

/// A repo that moved from a cache venv to an in-tree one leaves
/// `<cache>/toolr/<repo-key>/venv/` behind. In-tree dispatch must not refresh
/// that slot's `last_used_at`, or `self cache prune` could never reclaim it.
#[test]
#[cfg(unix)]
fn dispatch_in_tree_venv_lets_an_abandoned_cache_venv_age_out() {
    let tmp = preflight_fixture(&[]);
    let canonical = tmp.path().canonicalize().unwrap();
    let cache_root = preflight_cache_home(tmp.path()).join("toolr");

    // The slot the repo's cache venv used before the switch, last used 90 days ago.
    let resolved = toolr_core::venv::resolve_venv_path(tmp.path()).unwrap();
    let abandoned = toolr_core::venv::cache_slot_dir(
        &cache_root,
        tmp.path(),
        &resolved.python_version,
        VenvLocation::Cache,
    )
    .unwrap();
    std::fs::create_dir_all(abandoned.join("venv/bin")).unwrap();
    let mut old = toolr_core::cache::Meta::new(
        canonical.clone(),
        env!("CARGO_PKG_VERSION"),
        resolved.python_version.clone(),
    );
    old.last_used_at -= chrono::Duration::days(90);
    old.write(&abandoned).unwrap();

    let output = preflight_cmd(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        toolr_core::cache::Meta::load(&abandoned)
            .unwrap()
            .last_used_at,
        old.last_used_at,
        "in-tree dispatch refreshed the abandoned cache venv's slot; stderr:\n{stderr}"
    );

    let prune = preflight_cmd(tmp.path())
        .args(["self", "cache", "prune", "--stale-after-days", "30"])
        .output()
        .unwrap();
    assert!(
        prune.status.success(),
        "prune failed: {}",
        String::from_utf8_lossy(&prune.stderr)
    );
    assert!(
        !abandoned.exists(),
        "prune should reclaim the abandoned venv; stdout:\n{}",
        String::from_utf8_lossy(&prune.stdout)
    );
}

/// Run `ci hello` against a venv at `location` whose cache slot has a
/// directory where `meta.json` should be, so touching the sidecar fails.
#[cfg(unix)]
fn dispatch_with_unwritable_meta_json(
    location: VenvLocation,
    flags: &[&str],
) -> std::process::Output {
    let tmp = preflight_fixture(&[]);
    let resolved = toolr_core::venv::resolve_venv_path(tmp.path()).unwrap();
    let slot = toolr_core::venv::cache_slot_dir(
        &preflight_cache_home(tmp.path()).join("toolr"),
        tmp.path(),
        &resolved.python_version,
        location,
    )
    .unwrap();
    std::fs::create_dir_all(slot.join("meta.json")).unwrap();
    let mut cmd = preflight_cmd(tmp.path());
    if location == VenvLocation::Cache {
        // Reuse the fixture's fake venv from the cache slot.
        std::os::unix::fs::symlink(tmp.path().join("tools/.venv"), slot.join("venv")).unwrap();
        cmd.env("TOOLR_VENV_LOCATION", "cache");
    }
    cmd.args(flags).args(["ci", "hello"]).output().unwrap()
}

/// A sidecar that can't be written never stops dispatch. A cache venv warns
/// even under `-q`; an in-tree venv, often under a read-only cache home in CI,
/// warns only with `--debug`.
#[test]
#[cfg(unix)]
fn dispatch_touch_failure_warning_follows_location_and_verbosity() {
    let cases: &[(VenvLocation, &[&str], bool)] = &[
        (VenvLocation::InTree, &[], false),
        (VenvLocation::InTree, &["-q"], false),
        (VenvLocation::InTree, &["--debug"], true),
        (VenvLocation::Cache, &[], true),
        (VenvLocation::Cache, &["-q"], true),
        (VenvLocation::Cache, &["--debug"], true),
    ];
    for &(location, flags, warns) in cases {
        let output = dispatch_with_unwritable_meta_json(location, flags);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let case = format!("{location:?} {flags:?}");
        assert_eq!(
            stderr.contains("toolr: warning: failed to touch cache meta.json"),
            warns,
            "{case}: stderr:\n{stderr}"
        );
        if location == VenvLocation::InTree && flags == ["-q"] {
            assert!(!stderr.contains("toolr:"), "{case}: stderr:\n{stderr}");
        }
        // The fake python exits 1: the runner was spawned, so the failure didn't abort dispatch.
        assert_eq!(output.status.code(), Some(1), "{case}: stderr:\n{stderr}");
    }
}

/// Regression: when the pre-flight is disabled and the runner emits a
/// traceback, the Rust side must pass that traceback through to the
/// terminal *unaltered* — no capture, no rewriting. The styled
/// "run `toolr project venv sync`" hint on `ImportError` is the
/// runner's responsibility (see `toolr._runner.run()` in toolr-py),
/// and is covered by Python-side runner tests. Here we just guard
/// against a future regression that re-introduces a capture path.
#[test]
#[cfg(unix)]
fn dispatch_passes_runner_traceback_through_unaltered() {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let tmp = preflight_fixture(&[]);
    // Replace the fake python so it emits a ModuleNotFoundError traceback
    // (simulating the real runner failing at import time on an inline
    // import).
    let py = tmp
        .path()
        .join("tools")
        .join(".venv")
        .join("bin")
        .join("python");
    fs::remove_file(&py).unwrap();
    let mut f = fs::File::create(&py).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    writeln!(
        f,
        r#"printf 'Traceback (most recent call last):\n  File "<tool>", line 2, in hello\n    import yaml\nModuleNotFoundError: No module named '"'"'yaml'"'"'\n' 1>&2"#
    )
    .unwrap();
    writeln!(f, "exit 1").unwrap();
    drop(f);
    let mut perms = fs::metadata(&py).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&py, perms).unwrap();

    let output = preflight_cmd(tmp.path())
        .args(["ci", "hello"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_ne!(output.status.code(), Some(0), "stderr:\n{stderr}");
    assert!(
        stderr.contains("ModuleNotFoundError: No module named 'yaml'"),
        "expected traceback preserved verbatim, got:\n{stderr}"
    );
}

#[test]
#[cfg(unix)]
fn user_command_propagates_nonzero_exit() {
    let Some(python) = detect_test_python() else {
        eprintln!("skipping: no test python (see above)");
        return;
    };
    let tmp = TempDir::new().unwrap();
    write_demo_project(
        tmp.path(),
        &python,
        r#"
from toolr import command_group

group = command_group("demo", "Demo", description="demo group")

@group.command
def boom(ctx) -> None:
    """Fail."""
    ctx.exit(7, "failing on purpose")
"#,
        r#"{
        "groups": [{"name": "demo", "title": "Demo", "description": "", "origin": "static"}],
        "commands": [{
            "name": "boom", "group": "demo", "module": "tools.demo",
            "function": "boom", "summary": "", "description": "",
            "arguments": [], "origin": "static"
        }]
    }"#,
    );
    let output = common::toolr()
        .current_dir(tmp.path())
        .env("XDG_CACHE_HOME", tmp.path().join(".cache"))
        .args(["demo", "boom"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
}

/// `toolr project venv sync --help` exists and mentions `--force` / `--quiet`.
#[test]
fn project_venv_sync_help_lists_force_and_quiet() {
    let tmp = TempDir::new().unwrap();
    let output = common::toolr()
        .current_dir(tmp.path())
        .args(["project", "venv", "sync", "--help"])
        .output()
        .expect("running toolr should succeed");
    assert!(output.status.success(), "exit: {:?}", output.status.code());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--force"),
        "missing --force in help:\n{stdout}"
    );
    assert!(
        stdout.contains("--quiet"),
        "missing --quiet in help:\n{stdout}"
    );
}

/// F-17 repro tree: `tools/linked.py -> ../shared/linked.py` next to a
/// regular `tools/demo.py`. Returns the project root.
#[cfg(unix)]
fn symlinked_module_project(tmp: &TempDir) -> PathBuf {
    let tools = common::write_project(tmp.path());
    std::fs::write(
        tools.join("demo.py"),
        "group = command_group(\"demo\", \"Demo commands\", \"Demo commands.\")\n\n@group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n",
    )
    .unwrap();
    std::fs::create_dir(tmp.path().join("shared")).unwrap();
    std::fs::write(
        tmp.path().join("shared/linked.py"),
        "group = command_group(\"linked\", \"Linked commands\", \"Linked commands.\")\n\n@group.command\ndef hi(ctx):\n    \"\"\"Say hi.\"\"\"\n",
    )
    .unwrap();
    std::os::unix::fs::symlink("../shared/linked.py", tools.join("linked.py")).unwrap();
    tmp.path().to_path_buf()
}

/// What every path prints for [`symlinked_module_project`], after `toolr: `.
#[cfg(unix)]
const SYMLINK_ERROR: &str = "symlinked modules are not supported (1):\n  - tools/linked.py\nPut the file itself under tools/, or share it as a Python package.";

/// `toolr` in `root` with the trust check off.
#[cfg(unix)]
fn toolr_in(root: &std::path::Path) -> Command {
    let mut cmd = common::toolr();
    cmd.current_dir(root).env("TOOLR_NO_CACHE_HINT", "1");
    cmd
}

/// Run `cmd` with `args` and assert it fails printing exactly
/// [`SYMLINK_ERROR`]: no context prefix, no cache-fallback warning, no
/// clap error about an unknown subcommand.
#[cfg(unix)]
fn assert_rejected(mut cmd: Command, args: &[&str]) {
    let output = cmd.args(args).output().unwrap();
    assert!(!output.status.success(), "{args:?}: {output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("toolr: {SYMLINK_ERROR}\n"),
        "{args:?}"
    );
}

/// Build the cache with the link moved aside, then put it back.
#[cfg(unix)]
fn cache_without_the_link(root: &std::path::Path) {
    let link = root.join("tools/linked.py");
    let aside = root.join("linked.py.off");
    std::fs::rename(&link, &aside).unwrap();
    toolr_in(root).arg("--help").assert().success();
    std::fs::rename(&aside, &link).unwrap();
}

/// F-17: a symlinked `tools/*.py` used to vanish from `--help` without a
/// word; `--help` and dispatch now refuse the tree and name the link.
#[cfg(unix)]
#[test]
fn symlinked_tools_module_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let root = symlinked_module_project(&tmp);
    for args in [&["--help"][..], &["demo", "hello"][..]] {
        assert_rejected(toolr_in(&root), args);
    }
}

/// The first run after upgrading toolr sees a cache stamped by the old
/// version, so the rebuild finds the link before any hash does. It must
/// fail with the full list, not warn and dispatch from the stale cache.
#[cfg(unix)]
#[test]
fn symlinked_tools_module_is_rejected_over_a_stale_version_cache() {
    let tmp = TempDir::new().unwrap();
    let root = symlinked_module_project(&tmp);
    cache_without_the_link(&root);
    let manifest_path = root.join("tools/.toolr-manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["toolr_version"] = "0.0.1".into();
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();

    for args in [&["--help"][..], &["demo", "hello"][..]] {
        assert_rejected(toolr_in(&root), args);
    }
}

/// An untrusted repository never writes a cache, so every run rebuilds in
/// memory; the error must surface there too rather than leaving `demo`
/// as an unrecognised subcommand. Walking `tools/` executes nothing.
#[cfg(unix)]
#[test]
fn symlinked_tools_module_is_rejected_in_an_untrusted_repo() {
    let tmp = TempDir::new().unwrap();
    let root = symlinked_module_project(&tmp);
    let env = common::TrustEnv::new();
    for args in [&["--help"][..], &["demo", "hello"][..]] {
        assert_rejected(env.toolr(&root), args);
    }
    assert!(!root.join("tools/.toolr-manifest.json").exists());
}

/// A link added after the cache was built is found by the freshness hash
/// (dispatch) and by the explicit rebuilds; all print the same message as
/// every other path, with no `hashing <dir>:` or `building …:` prefix.
#[cfg(unix)]
#[test]
fn symlinked_tools_module_added_after_the_cache_reads_the_same_everywhere() {
    let tmp = TempDir::new().unwrap();
    let root = symlinked_module_project(&tmp);
    cache_without_the_link(&root);
    for args in [
        &["--help"][..],
        &["demo", "hello"][..],
        &["project", "manifest", "rebuild"][..],
        &["__build-static-manifest"][..],
    ] {
        assert_rejected(toolr_in(&root), args);
    }
}

/// Built-ins skip the manifest build even after a value-taking root
/// option, so they keep working in a repo with a symlinked module.
#[cfg(unix)]
#[test]
fn built_ins_after_a_root_option_value_ignore_symlinked_modules() {
    let tmp = TempDir::new().unwrap();
    let root = symlinked_module_project(&tmp);
    for args in [
        &["--timeout-secs", "5", "self", "--help"][..],
        &[
            "--no-output-timeout-secs",
            "5",
            "self",
            "completion",
            "print",
            "bash",
        ][..],
    ] {
        let output = toolr_in(&root).args(args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("symlinked"),
            "{args:?}: {output:?}"
        );
    }
}

/// FU-2: two modules defining `grp dup` used to panic clap in a debug
/// build on `toolr grp --help`; the build now refuses the tree instead.
#[test]
fn duplicate_command_across_modules_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let tools = common::write_project(tmp.path());
    let module = "from toolr import command_group\n\ng = command_group(\"grp\", \"Grp\", \"Grp\")\n\n\n@g.command\ndef dup(ctx):\n    \"\"\"Dup.\"\"\"\n";
    std::fs::write(tools.join("m1.py"), module).unwrap();
    std::fs::write(tools.join("m2.py"), module).unwrap();
    for args in [&["--help"][..], &["grp", "--help"][..]] {
        let output = common::toolr()
            .current_dir(tmp.path())
            .env("TOOLR_NO_CACHE_HINT", "1")
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}: {output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "toolr: duplicate commands (1):\n  - `grp dup` is defined more than once: \
             tools.m1::dup, tools.m2::dup. Rename all but one, e.g. with `name=\"…\"`.\n",
            "{args:?}"
        );
    }
}

/// FU-22: two modules declaring `grp` with different titles.
fn conflicting_group_project(tmp: &TempDir) {
    let tools = common::write_project(tmp.path());
    for (module, title, command) in [("m1", "Grp", "one"), ("m2", "Other", "two")] {
        std::fs::write(
            tools.join(format!("{module}.py")),
            format!(
                "from toolr import command_group\n\ng = command_group(\"grp\", \"{title}\", \"Grp\")\n\n\n@g.command\ndef {command}(ctx):\n    \"\"\"Run.\"\"\"\n"
            ),
        )
        .unwrap();
    }
}

const GROUP_CONFLICT_WARNING: &str = "toolr: warning: group `grp` is declared more than once; \
     keeping the title from tools.m1 and ignoring the different title in tools.m2\n";

fn stderr_of(root: &std::path::Path, args: &[&str]) -> String {
    let output = common::toolr()
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stderr).unwrap()
}

/// FU-22: a conflicting group re-declaration warns when the manifest is
/// built, not on every run that reads it back.
#[test]
fn group_conflict_warns_once_per_build() {
    let tmp = TempDir::new().unwrap();
    conflicting_group_project(&tmp);
    let root = tmp.path();
    assert_eq!(stderr_of(root, &["--help"]), GROUP_CONFLICT_WARNING);
    assert_eq!(stderr_of(root, &["--help"]), "");
    assert_eq!(stderr_of(root, &["-q", "--help"]), "");

    // A changed module rebuilds the manifest, which warns again, `-q` included.
    std::fs::write(root.join("tools/m3.py"), "x = 1\n").unwrap();
    assert_eq!(stderr_of(root, &["-q", "--help"]), GROUP_CONFLICT_WARNING);
    std::fs::write(root.join("tools/m3.py"), "x = 2\n").unwrap();
    assert_eq!(stderr_of(root, &["--help"]), GROUP_CONFLICT_WARNING);

    for args in [
        &["project", "manifest", "rebuild"][..],
        &["__build-static-manifest"][..],
    ] {
        assert_eq!(stderr_of(root, args), GROUP_CONFLICT_WARNING, "{args:?}");
    }
}

/// An untrusted repository builds in memory on every run, so it warns on every run.
#[test]
fn group_conflict_warns_on_every_untrusted_run() {
    let tmp = TempDir::new().unwrap();
    conflicting_group_project(&tmp);
    let env = common::TrustEnv::new();
    for _ in 0..2 {
        let output = env.toolr(tmp.path()).arg("--help").output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            GROUP_CONFLICT_WARNING
        );
    }
}

/// Identical warnings from separate re-declarations each print, whether the
/// bootstrap or the freshness step built the manifest.
#[test]
fn identical_group_conflicts_each_warn() {
    let tmp = TempDir::new().unwrap();
    let tools = common::write_project(tmp.path());
    std::fs::write(
        tools.join("m1.py"),
        "from toolr import command_group\n\ng = command_group(\"grp\", \"Grp\", \"Grp\")\n\n\n@g.command\ndef one(ctx):\n    \"\"\"Run.\"\"\"\n",
    )
    .unwrap();
    std::fs::write(
        tools.join("m2.py"),
        "from toolr import command_group\n\ncommand_group(\"grp\", \"Other\")\ncommand_group(\"grp\", \"Other\")\n",
    )
    .unwrap();
    let root = tmp.path();
    let twice = "toolr: warning: group `grp` is declared more than once; keeping the title from \
                 tools.m1 and ignoring the different title in tools.m2\n"
        .repeat(2);
    assert_eq!(stderr_of(root, &["--help"]), twice);
    std::fs::write(tools.join("m3.py"), "x = 1\n").unwrap();
    assert_eq!(stderr_of(root, &["--help"]), twice);
}

fn completion_print(shell: &str) -> std::process::Output {
    let tmp = TempDir::new().unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .args(["self", "completion", "print", shell])
        .output()
        .unwrap()
}

#[test]
fn completion_print_pwsh_matches_powershell() {
    let powershell = completion_print("powershell");
    let pwsh = completion_print("pwsh");
    assert!(powershell.status.success(), "{powershell:?}");
    assert!(pwsh.status.success(), "{pwsh:?}");
    assert!(!powershell.stdout.is_empty());
    assert_eq!(pwsh.stdout, powershell.stdout);
}

#[test]
fn completion_print_rejects_unknown_shell() {
    let out = completion_print("nushell");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("powershell"), "{stderr}");
}

/// The root help points agents at `llms.txt`; subcommand help doesn't repeat it.
#[test]
fn root_help_points_at_llms_txt_and_subcommands_do_not() {
    let tmp = tempfile::tempdir().unwrap();
    let help = |args: &[&str]| {
        // Pin the width so the footer can't wrap on a narrow CI terminal.
        let out = common::toolr()
            .current_dir(tmp.path())
            .env("COLUMNS", "100")
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    for flag in ["--help", "-h"] {
        let out = help(&[flag]);
        assert!(
            out.ends_with(
                "Documentation: https://toolr.readthedocs.io/\nFor AI agents: https://toolr.readthedocs.io/llms.txt\n"
            ),
            "root {flag} footer wrong:\n{out}"
        );
    }
    for args in [&["self", "--help"][..], &["project", "venv", "--help"][..]] {
        assert!(
            !help(args).contains("llms.txt"),
            "{args:?} must not carry the footer"
        );
    }
}
