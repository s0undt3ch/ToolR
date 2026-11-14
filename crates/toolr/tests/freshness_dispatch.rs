//! Integration tests for dispatch-time manifest freshness.

mod common;

use std::fs;

use tempfile::TempDir;

const EXAMPLE_PY: &str = r#"
from toolr import Context, command_group

example = command_group("example", "Example commands", "Example commands.")

@example.command
def hello(ctx: Context, name: str = "world") -> None:
    """Greet someone."""
    ctx.print(f"hello, {name}")
"#;

/// Build a minimal toolr project at `tmp`: a `tools/pyproject.toml`
/// plus an intentionally-stale `tools/.toolr-manifest.json`. Tests
/// then drop additional files into `tools/` and verify the freshness
/// step picks them up.
fn write_minimal_project(tmp: &std::path::Path) {
    let tools = tmp.join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        r#"[project]
name = "tools"
version = "0.0.0"
"#,
    )
    .unwrap();
    // Seed an empty, intentionally-stale manifest: these tests exercise a
    // stale manifest, not a missing one.
    fs::write(
        tools.join(".toolr-manifest.json"),
        r#"{
            "schema_version": 1,
            "static_hash": "stale",
            "third_party_hash": "",
            "groups": [],
            "commands": []
        }"#,
    )
    .unwrap();
}

#[test]
fn new_tools_file_appears_in_help_without_explicit_rebuild() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    // Drop a new `example.py` in tools/ after the manifest was seeded.
    fs::write(tmp.path().join("tools").join("example.py"), EXAMPLE_PY).unwrap();

    let output = common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "toolr --help failed: {output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("example"),
        "expected `example` in --help, got:\n{stdout}"
    );

    // Manifest on disk should have been rewritten to include the group.
    let manifest =
        fs::read_to_string(tmp.path().join("tools").join(".toolr-manifest.json")).unwrap();
    assert!(
        manifest.contains(r#""name": "example""#) || manifest.contains(r#""name":"example""#),
        "manifest was not persisted with the example group:\n{manifest}"
    );
}

#[test]
fn syntax_error_in_tools_warns_and_serves_cached() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    // Overwrite the empty manifest seeded by `write_minimal_project`
    // with one that has a pre-existing `good` group — proves the
    // soft-fail path falls back to (not erases) the cached manifest.
    fs::write(
        tmp.path().join("tools").join(".toolr-manifest.json"),
        r#"{
            "schema_version": 1,
            "static_hash": "stale",
            "third_party_hash": "",
            "groups": [
                {"name": "good", "title": "Good", "description": "", "parent": null, "origin": "static"}
            ],
            "commands": []
        }"#,
    )
    .unwrap();

    // Drop a syntactically broken Python file so the static rebuild
    // returns BuildError::Build (unclosed parenthesis = parse error).
    fs::write(
        tmp.path().join("tools").join("broken.py"),
        "def not closed(",
    )
    .unwrap();

    let output = common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // toolr --help itself must succeed — we're soft-failing.
    assert!(
        output.status.success(),
        "toolr --help failed unexpectedly: {output:?}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tools manifest is stale and a fresh build failed"),
        "expected soft-fail warning in stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("broken.py"),
        "expected the offending filename in the warning; got:\n{stderr}"
    );
    assert!(
        stderr.contains("toolr project manifest rebuild"),
        "expected pointer to explicit rebuild command; got:\n{stderr}"
    );

    // Cached `good` group must still be visible — we fell back, didn't erase.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("good"),
        "expected cached group in --help; got:\n{stdout}"
    );
}

#[test]
fn skip_list_argv_does_not_trigger_freshness() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    // Drop a syntax-broken `tools/*.py` that would crash a rebuild —
    // but skip-list argv must never call into freshness, so the
    // command should still succeed without any warning.
    fs::write(
        tmp.path().join("tools").join("broken.py"),
        "def not closed(",
    )
    .unwrap();

    for argv in [
        vec!["--version"],
        vec!["self", "cache", "list"],
        vec!["project", "manifest", "--help"],
    ] {
        let output = common::toolr()
            .args(&argv)
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "argv {argv:?} should bypass freshness, got: {output:?}"
        );
        // The soft-fail warning must NOT appear; freshness was bypassed entirely.
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("tools manifest is stale"),
            "unexpected freshness warning for argv {argv:?}:\n{stderr}"
        );
    }
}

#[test]
fn version_mismatch_without_venv_preserves_third_party_entries() {
    // #420 regression guard: a toolr-version mismatch must not escalate
    // to `ThirdPartyDrift` when the venv can't be resolved — that would
    // discard cached third-party entries with nothing to re-glob them
    // from. Force venv resolution to fail via an invalid
    // `TOOLR_VENV_LOCATION`, seed a manifest with a matching static_hash
    // but a stale `toolr_version` and a third-party group/command, and
    // confirm the rebuild keeps the third-party entries and still
    // stamps the current version.
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(
        tools.join("pyproject.toml"),
        "[project]\nname = \"tools\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();

    let static_hash = toolr_core::hash::compute_static_hash(&tools).unwrap();
    let manifest = format!(
        r#"{{
            "schema_version": 1,
            "static_hash": "{static_hash}",
            "third_party_hash": "",
            "toolr_version": "0.0.0-old",
            "groups": [
                {{"name": "plugin", "title": "Plugin", "description": "", "parent": null, "origin": "third_party"}}
            ],
            "commands": [
                {{
                    "name": "run", "group": "plugin", "module": "plugin_pkg.commands",
                    "function": "run", "summary": "", "description": "",
                    "arguments": [], "origin": "third_party"
                }}
            ]
        }}"#
    );
    fs::write(tools.join(".toolr-manifest.json"), manifest).unwrap();

    let output = common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .env("TOOLR_VENV_LOCATION", "bogus")
        .output()
        .unwrap();
    assert!(output.status.success(), "toolr --help failed: {output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("plugin"),
        "expected cached third-party group in --help; got:\n{stdout}"
    );

    let manifest = fs::read_to_string(tools.join(".toolr-manifest.json")).unwrap();
    assert!(
        manifest.contains(r#""name": "run""#) || manifest.contains(r#""name":"run""#),
        "third-party command dropped from persisted manifest:\n{manifest}"
    );
    assert!(
        !manifest.contains("0.0.0-old"),
        "stale toolr_version was not refreshed:\n{manifest}"
    );
}

#[test]
fn tab_completion_does_not_persist_manifest() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());
    // Force StaticDrift so the freshness machinery would rebuild if not
    // bypassed: the tools/*.py content differs from the seeded
    // static_hash, but tab completion handles drift in-memory only.
    fs::write(tmp.path().join("tools").join("a.py"), "x = 1\n").unwrap();

    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    let before = fs::read_to_string(&manifest_path).unwrap();
    let mtime_before = fs::metadata(&manifest_path).unwrap().modified().unwrap();

    // Trigger a completion call. The exact tokens after `__complete` don't
    // matter — completion's only job here is to fire the freshness path
    // via `resolve_manifest_at_tab`, which is responsible for staying in-memory.
    let output = common::toolr()
        .args(["__complete", tmp.path().to_str().unwrap(), "toolr", ""])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "__complete failed: {output:?}");

    // Manifest file must not have been rewritten.
    let after = fs::read_to_string(&manifest_path).unwrap();
    let mtime_after = fs::metadata(&manifest_path).unwrap().modified().unwrap();
    assert_eq!(
        before, after,
        "tab completion rewrote the manifest contents"
    );
    assert_eq!(mtime_before, mtime_after, "tab completion touched mtime");
}

/// #656: TAB never reports a corrupt manifest (completion scripts drop
/// stderr) and never repairs it; the next non-TAB run warns once and
/// rewrites it, and the run after that is silent.
#[test]
fn corrupt_manifest_is_reported_once_by_the_next_non_tab_run() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());
    fs::write(tmp.path().join("tools").join("example.py"), EXAMPLE_PY).unwrap();
    let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
    fs::write(&manifest_path, "{\"schema_ver").unwrap();

    let tab = common::toolr()
        .args(["__complete", tmp.path().to_str().unwrap(), ""])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(tab.status.success(), "__complete failed: {tab:?}");
    assert!(tab.stderr.is_empty(), "{tab:?}");
    assert!(String::from_utf8_lossy(&tab.stdout).contains("example"));
    assert_eq!(fs::read_to_string(&manifest_path).unwrap(), "{\"schema_ver");

    let help = |expect_warning: bool| {
        let out = common::toolr()
            .arg("--help")
            .current_dir(tmp.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            stderr.matches("ignoring unreadable").count(),
            usize::from(expect_warning),
            "{stderr}"
        );
    };
    help(true);
    let repaired = toolr_core::manifest::load_manifest(&manifest_path).unwrap();
    // Written with the filesystem clock taken before the build, so the next
    // check can reuse its records.
    assert_ne!(repaired.input_records.snapshot_at_ns, 0);
    help(false);
}

/// End-to-end through the real binary: a command imports an
/// `Enum` from a sibling module (relative import) while an unrelated
/// module elsewhere in the tree declares a same-named enum class. The
/// auto-rebuild triggered by `toolr --help` must succeed (not hard-fail
/// with "invalid parameter declarations") and the persisted manifest must
/// carry the resolved enum's allowed values.
#[test]
fn cross_module_enum_import_rebuilds_and_persists_allowed_values() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());

    let tools = tmp.path().join("tools");
    fs::create_dir_all(tools.join("metrics")).unwrap();
    fs::write(tools.join("metrics").join("__init__.py"), "").unwrap();
    fs::write(
        tools.join("metrics").join("_common.py"),
        r#""""Shared metrics types."""
import enum

class Environment(enum.StrEnum):
    PRODUCTION = "production"
    STAGING = "staging"
"#,
    )
    .unwrap();
    fs::write(
        tools.join("metrics").join("analyse.py"),
        r#""""Metrics analysis -- imports Environment from a sibling module."""
from toolr import Context, command_group
from ._common import Environment

group = command_group("metrics", "Metrics", "Metrics.")

@group.command
def analyse(ctx: Context, *, env: Environment = Environment.PRODUCTION) -> None:
    """Analyse."""
    ctx.print(env.value)
"#,
    )
    .unwrap();
    // Unrelated module declaring a same-named enum elsewhere in the
    // tree -- this is exactly the shape that used to hard-fail the build.
    fs::write(
        tools.join("job.py"),
        r#""""Job module -- unrelated same-named enum."""
import enum
from toolr import Context, command_group

group = command_group("job", "Job", "Job.")

class Environment(enum.StrEnum):
    PRODUCTION = "production"
    STAGING = "staging"

@group.command
def run(ctx: Context, *, env: Environment = Environment.PRODUCTION) -> None:
    """Run."""
    ctx.print(env.value)
"#,
    )
    .unwrap();

    let output = common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "toolr --help failed: {output:?}\nstderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("invalid parameter declarations"),
        "manifest build hard-failed on the cross-module enum import:\n{stderr}"
    );

    let manifest = fs::read_to_string(tools.join(".toolr-manifest.json")).unwrap();
    assert!(
        manifest.contains("production") && manifest.contains("staging"),
        "manifest missing the resolved enum's allowed values:\n{manifest}"
    );
    assert!(
        manifest.contains("tools.metrics._common"),
        "manifest missing the enum's declaring module (tools.metrics._common):\n{manifest}"
    );
}

/// Issue #500: an unknown `arg()` keyword used to build fine and only
/// fail with `TypeError` when the command ran.
#[test]
fn unknown_arg_keyword_fails_the_manifest_build() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());
    fs::write(
        tmp.path().join("tools").join("kw.py"),
        r#"from pathlib import Path
from typing import Annotated

from toolr import Context, arg, command_group

group = command_group("kw", "Kwarg test", description="Kwarg test.")


@group.command
def read(ctx: Context, config: Annotated[Path, arg(path_must_exist=True)]) -> None:
    """Read a config."""
    ctx.print(f"got {config}")
"#,
    )
    .unwrap();

    // Dispatch falls back to the cached manifest, so `kw` never appears.
    let help = common::toolr()
        .args(["kw", "read", "--help"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(
        !help.status.success(),
        "`kw read --help` succeeded: {help:?}"
    );
    assert!(
        String::from_utf8_lossy(&help.stderr).contains("a fresh build failed"),
        "{help:?}"
    );

    let output = common::toolr()
        .args(["project", "manifest", "rebuild"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "build unexpectedly succeeded: {output:?}"
    );
    for needle in [
        "invalid parameter declarations (1):",
        "tools.kw::read argument `config`",
        "unknown `arg()` keyword `path_must_exist`; use `toolr.types.ResolvedPath` instead",
    ] {
        assert!(
            stderr.contains(needle),
            "stderr missing {needle:?}:\n{stderr}"
        );
    }
}

/// Issue #501: a command without a docstring used to build with an empty
/// `--help` summary.
#[test]
fn command_without_a_docstring_fails_the_manifest_build() {
    let tmp = TempDir::new().unwrap();
    write_minimal_project(tmp.path());
    fs::write(
        tmp.path().join("tools").join("doc.py"),
        r#"from toolr import Context, command_group

group = command_group("doc", "Doc test", description="Doc test.")


@group.command
def bare(ctx: Context) -> None:
    ctx.print("no docstring")
"#,
    )
    .unwrap();

    let output = common::toolr()
        .args(["project", "manifest", "rebuild"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "build unexpectedly succeeded: {output:?}"
    );
    for needle in [
        "commands without a docstring (1):",
        "tools.doc::bare: add a docstring.",
    ] {
        assert!(
            stderr.contains(needle),
            "stderr missing {needle:?}:\n{stderr}"
        );
    }
}

#[test]
fn concurrent_dispatches_rebuild_a_stale_manifest_under_the_lock() {
    let tmp = TempDir::new().unwrap();
    let xdg_cache = TempDir::new().unwrap();
    write_minimal_project(tmp.path());
    fs::write(tmp.path().join("tools").join("example.py"), EXAMPLE_PY).unwrap();

    let children: Vec<_> = (0..8)
        .map(|_| {
            common::toolr_std()
                .arg("--help")
                .current_dir(tmp.path())
                .env("XDG_CACHE_HOME", xdg_cache.path())
                .env("TOOLR_NO_CACHE_HINT", "1")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "toolr --help failed: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("example"),
            "expected `example` in --help, got:\n{stdout}"
        );
    }
    let locks: Vec<_> = fs::read_dir(xdg_cache.path().join("toolr").join("locks"))
        .expect("the rebuild should have gone through the cache-dir lock")
        .collect();
    assert_eq!(locks.len(), 1, "one lock per manifest: {locks:?}");
    let stray: Vec<_> = fs::read_dir(tmp.path().join("tools"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().ends_with(".lock"))
        .collect();
    assert!(stray.is_empty(), "lock files leaked into tools/: {stray:?}");
}

const DJANGO_ARGPARSE_BLOCK: &str = r#"
[tool.toolr.argparse.django]
scan_paths = ["apps/*/management/commands/*.py"]
django = true

[[tool.toolr.argparse.django.attach]]
parent = "django"
"#;

const DJANGO_DISPATCHER_PY: &str = r#"
from toolr import Context, command_group
from toolr.sources import DispatchCommand

group = command_group("django", "Django", description="Run Django management commands")


@group.command
def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
    """Dispatch a Django management command."""
    return ctx.run("django-admin", dispatched.command, *dispatched.argv).returncode
"#;

const MIGRATE_PY: &str = r#""""Migrate the database."""


def add_arguments(self, parser):
    parser.add_argument("--check", action="store_true", help="Dry run")
"#;

/// The worked example from `docs/writing-commands/external-sources.md`:
/// a `django` dispatcher in `tools/` and a management command it scans
/// from `apps/`. Returns the path of the scanned `migrate.py`.
fn write_django_project(root: &std::path::Path) -> std::path::PathBuf {
    let tools = common::write_project(root);
    fs::write(
        tools.join("pyproject.toml"),
        format!("{}{DJANGO_ARGPARSE_BLOCK}", common::TOOLS_PYPROJECT),
    )
    .unwrap();
    fs::write(tools.join("dispatcher.py"), DJANGO_DISPATCHER_PY).unwrap();
    let commands = root.join("apps/billing/management/commands");
    fs::create_dir_all(&commands).unwrap();
    let migrate = commands.join("migrate.py");
    fs::write(&migrate, MIGRATE_PY).unwrap();
    migrate
}

fn help_stdout(root: &std::path::Path, args: &[&str]) -> String {
    let output = common::toolr()
        .args(args)
        .arg("--help")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "toolr {args:?} --help failed: {output:?}"
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// #655: editing a scanned file must refresh the grafted command's
/// arguments without `toolr project manifest rebuild`.
#[test]
fn grafted_command_help_reflects_scanned_file_edit() {
    let tmp = TempDir::new().unwrap();
    let migrate = write_django_project(tmp.path());

    let before = help_stdout(tmp.path(), &["django", "migrate"]);
    assert!(before.contains("--check"), "{before}");
    assert!(!before.contains("--database"), "{before}");

    let mut text = fs::read_to_string(&migrate).unwrap();
    text.push_str(
        "    parser.add_argument(\"--database\", default=\"default\", help=\"Target DB\")\n",
    );
    fs::write(&migrate, text).unwrap();

    let after = help_stdout(tmp.path(), &["django", "migrate"]);
    assert!(after.contains("--database"), "{after}");
}

/// #655: editing the `[tool.toolr.argparse.*]` table must refresh the
/// grafted commands too; here pointing `scan_paths` nowhere drops them.
#[test]
fn removing_scan_paths_drops_grafted_commands() {
    let tmp = TempDir::new().unwrap();
    write_django_project(tmp.path());

    let before = help_stdout(tmp.path(), &["django"]);
    assert!(before.contains("migrate"), "{before}");

    let pyproject = tmp.path().join("tools/pyproject.toml");
    let text = fs::read_to_string(&pyproject).unwrap().replace(
        r#"scan_paths = ["apps/*/management/commands/*.py"]"#,
        r#"scan_paths = ["nowhere/*.py"]"#,
    );
    fs::write(&pyproject, text).unwrap();

    let after = help_stdout(tmp.path(), &["django"]);
    assert!(!after.contains("migrate"), "{after}");
}

/// An unreadable scanned file becomes a placeholder command with a
/// warning; it must not fail `--help` or TAB through the freshness check.
#[cfg(unix)]
#[test]
fn unreadable_scanned_file_does_not_break_help_or_completion() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let migrate = write_django_project(tmp.path());
    help_stdout(tmp.path(), &["django"]);
    fs::set_permissions(&migrate, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&migrate).is_ok() {
        eprintln!("skipping: running as root");
        return;
    }

    let help = common::toolr()
        .arg("--help")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let complete = common::toolr()
        .args(["__complete", &tmp.path().to_string_lossy(), "django", ""])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    fs::set_permissions(&migrate, fs::Permissions::from_mode(0o644)).unwrap();

    assert!(help.status.success(), "toolr --help failed: {help:?}");
    assert!(
        complete.status.success(),
        "toolr __complete failed: {complete:?}"
    );
    let stdout = String::from_utf8_lossy(&complete.stdout);
    assert!(stdout.contains("migrate"), "{stdout}");
}

/// `toolr __complete <root> <args...> ""`, which must succeed silently.
fn complete_stdout(root: &std::path::Path, args: &[&str]) -> String {
    let output = common::toolr()
        .args(["__complete", &root.to_string_lossy()])
        .args(args)
        .arg("")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "toolr __complete failed: {output:?}"
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// #655 review: a pyproject mid-edit or a typo'd `scan_paths` glob fails
/// the rebuild. TAB keeps serving the cached commands, silently.
#[test]
fn completion_serves_the_cache_when_the_pyproject_is_broken() {
    let tmp = TempDir::new().unwrap();
    write_django_project(tmp.path());
    help_stdout(tmp.path(), &["django"]);
    let pyproject = tmp.path().join("tools/pyproject.toml");
    let valid = fs::read_to_string(&pyproject).unwrap();
    let invalid_toml = valid.replace("[project]\n", "[project]\ndependencies = [\"django\"\n");
    let bad_glob = valid.replace(
        r#"scan_paths = ["apps/*/management/commands/*.py"]"#,
        r#"scan_paths = ["apps/[/x.py"]"#,
    );
    for broken in [invalid_toml, bad_glob] {
        assert_ne!(broken, valid);
        fs::write(&pyproject, &broken).unwrap();
        let groups = complete_stdout(tmp.path(), &[]);
        assert!(groups.contains("django"), "{broken}\n{groups}");
        let commands = complete_stdout(tmp.path(), &["django"]);
        assert!(commands.contains("migrate"), "{broken}\n{commands}");
    }
}

const BROKEN_PY: &str = "def not closed(\n";

/// The toolr-owned head of a `BROKEN_PY` parse error; the parser's own text follows it.
const PARSE_ERROR: &str = "parsing <root>/tools/broken.py: ";

/// `stderr` is `prefix`, a `BROKEN_PY` parse error naming the file once, and a newline.
fn assert_parse_error(stderr: &str, prefix: &str) {
    let rest = stderr.strip_prefix(&format!("{prefix}{PARSE_ERROR}"));
    assert!(
        rest.is_some_and(|r| r.ends_with('\n') && !r.contains("broken.py")),
        "{stderr}"
    );
}

const PARSE_ERROR_CAUSE: &str = "cause: parsing <root>/tools/broken.py: ";

const FALLBACK_WARNING: &str = "toolr: warning: tools manifest is stale and a fresh build failed; \
     falling back to cached manifest\n";

const REBUILD_POINTER: &str =
    "toolr: warning: run `toolr project manifest rebuild` to see the full error\n";

/// Exit code and stderr of `toolr <args>`, with `root` replaced by `<root>`
/// and path separators by `/`.
fn run_in(mut cmd: assert_cmd::Command, root: &std::path::Path, args: &[&str]) -> (i32, String) {
    let output = cmd.current_dir(root).args(args).output().unwrap();
    let mut stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let canonical = root.canonicalize().unwrap().display().to_string();
    let unverbatim = canonical.trim_start_matches(r"\\?\").to_string();
    for form in [canonical, unverbatim, root.display().to_string()] {
        stderr = stderr.replace(&form, "<root>");
    }
    (output.status.code().unwrap(), stderr.replace('\\', "/"))
}

/// A `common::Project` with or without its in-tree venv.
fn project_with_venv(with_venv: bool) -> common::Project {
    let p = common::Project::new();
    if !with_venv {
        fs::remove_dir_all(p.tools().join(".venv")).unwrap();
    }
    p
}

/// A trusted fresh clone has no manifest to fall back to, so a failed
/// first build is fatal and writes nothing, venv or not.
#[test]
fn fresh_clone_build_error_is_fatal() {
    for with_venv in [false, true] {
        let p = project_with_venv(with_venv);
        p.write_tool("broken.py", BROKEN_PY);
        let (code, stderr) = run_in(p.toolr(&[]), &p.root, &["--help"]);
        assert_eq!(code, 2, "venv={with_venv}: {stderr}");
        assert_parse_error(&stderr, "toolr: ");
        assert!(!p.tools().join(".toolr-manifest.json").exists());
    }
}

/// An untrusted fresh clone is not built, so a broken module is not even noticed.
#[test]
fn untrusted_fresh_clone_is_not_built() {
    let p = project_with_venv(true);
    p.write_tool("broken.py", BROKEN_PY);
    let env = common::TrustEnv::new();
    let (code, stderr) = run_in(env.toolr(&p.root), &p.root, &["--help"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
    assert!(!p.tools().join(".toolr-manifest.json").exists());
}

/// A stale manifest whose rebuild fails warns with the first line of the
/// cause and serves the cache, venv or not.
#[test]
fn stale_manifest_build_error_cause_line() {
    for with_venv in [false, true] {
        let p = project_with_venv(with_venv);
        p.toolr(&["--help"]).assert().success();
        p.write_tool("broken.py", BROKEN_PY);
        let (code, stderr) = run_in(p.toolr(&[]), &p.root, &["--help"]);
        assert_eq!(code, 0, "venv={with_venv}: {stderr}");
        // The cause is the parser's own message with a line and column, not a byte range.
        let cause = stderr
            .strip_prefix(FALLBACK_WARNING)
            .and_then(|r| r.strip_suffix(REBUILD_POINTER))
            .and_then(|r| r.strip_prefix(&format!("toolr: warning: {}", PARSE_ERROR_CAUSE)));
        assert!(
            cause.is_some_and(|c| c.ends_with("(line 1, column 5)\n") && !c.contains("byte range")),
            "venv={with_venv}: {stderr}"
        );
    }
}

/// `project manifest rebuild` prints the whole error chain.
#[test]
fn explicit_rebuild_build_error_message() {
    for with_venv in [false, true] {
        let p = project_with_venv(with_venv);
        p.write_tool("broken.py", BROKEN_PY);
        let (code, stderr) = run_in(p.toolr(&[]), &p.root, &["project", "manifest", "rebuild"]);
        assert_eq!(code, 2, "venv={with_venv}: {stderr}");
        assert_parse_error(
            &stderr,
            "toolr: building static manifest (incl. third-party fragments): ",
        );
    }
}

/// A fresh clone's first run writes a manifest stamped with the venv's
/// plugin hash, so the next run finds it fresh and leaves it untouched.
#[test]
fn first_run_manifest_is_fresh_for_the_next_run() {
    let p = common::Project::new();
    p.add_plugin(
        "demo",
        r#"{"toolr_schema_version": 2, "package": "demo", "groups": [], "commands": []}"#,
    );
    let path = p.tools().join(".toolr-manifest.json");
    p.toolr(&["--help"]).assert().success();
    let first = fs::read(&path).unwrap();
    let first_mtime = fs::metadata(&path).unwrap().modified().unwrap();
    p.toolr(&["--help"]).assert().success();
    assert_eq!(fs::read(&path).unwrap(), first);
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        first_mtime
    );
    let manifest: serde_json::Value = serde_json::from_slice(&first).unwrap();
    // The hash covers the venv path as toolr resolves it: canonical, and on
    // Windows without the verbatim prefix `canonicalize` adds.
    let canonical = p.tools().join(".venv").canonicalize().unwrap();
    let venv = std::path::PathBuf::from(canonical.to_string_lossy().trim_start_matches(r"\\?\"));
    let plugin_hash = toolr_core::manifest_build::compute_third_party_hash(&venv).unwrap();
    assert_ne!(
        plugin_hash,
        toolr_core::manifest_build::empty_third_party_hash()
    );
    assert_eq!(manifest["third_party_hash"], plugin_hash);
}

/// Any build error reads the same on a fresh clone with or without a venv.
#[test]
fn fresh_clone_argparse_error_reads_the_same_with_or_without_a_venv() {
    for with_venv in [false, true] {
        let p = project_with_venv(with_venv);
        write_django_project(&p.root);
        let pyproject = p.tools().join("pyproject.toml");
        let valid = fs::read_to_string(&pyproject).unwrap();
        let bad_glob = valid.replace(
            r#"scan_paths = ["apps/*/management/commands/*.py"]"#,
            r#"scan_paths = ["apps/[/x.py"]"#,
        );
        assert_ne!(bad_glob, valid);
        fs::write(&pyproject, bad_glob).unwrap();
        let (code, stderr) = run_in(p.toolr(&[]), &p.root, &["--help"]);
        assert_eq!(code, 2, "venv={with_venv}: {stderr}");
        // The glob error's position depends on the temp dir's length.
        let (head, tail) = stderr.split_once(" near position ").unwrap();
        assert_eq!(
            head,
            "toolr: argparse scanner error: failed to parse apps/[/x.py: Pattern syntax error",
            "venv={with_venv}"
        );
        assert!(
            tail.ends_with(": invalid range pattern\n") && !tail.contains("failed to parse"),
            "venv={with_venv}: {stderr}"
        );
    }
}
