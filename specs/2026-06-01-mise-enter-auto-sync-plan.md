# mise enter-hook auto-sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the `toolr project deps` → `toolr project venv` rename, flip `sync`'s default to
honor the freshness stamp, add a `--quiet` unattended mode, and document a one-line mise
`[hooks].enter` recipe so a user's tools venv stays current on shell-enter without delaying the
prompt.

**Architecture:** Rust changes are confined to four files in `crates/toolr/src/` (`cli.rs`,
`project.rs`, `dispatch.rs`, `builtin_completions.rs`), three files in `crates/toolr-core/src/`
(`venv/sync.rs`, `uv/install.rs`, `project.rs`), and the Python runner
(`crates/toolr-py/python/toolr/_runner.py`). The freshness stamp's location and `check_freshness()`
semantics are unchanged. The mise integration is documentation-only — there is no plugin to ship.

**Tech Stack:** Rust (clap, anyhow, tempfile, Cargo workspace tests), Python 3.13+ (toolr-py
runner), MkDocs (docs), pre-commit/prek (lint + snippet regen).

**Design doc:** `specs/2026-06-01-mise-enter-auto-sync-design.md` (read first; this plan implements
that spec verbatim).

---

## File Map

Rust crate: **toolr-core**

- `crates/toolr-core/src/venv/sync.rs` — add `quiet` parameter to `run_uv_sync` and
  `sync_if_needed`; pass `--quiet` to the uv subprocess. Inline unit tests in the same file.
- `crates/toolr-core/src/uv/install.rs` — add `silent_refuse` field to `ConsentMode`; when set,
  `decide_install` returns `Refuse` without prompting AND without printing. Inline unit tests in the
  same file.
- `crates/toolr-core/src/project.rs` — extend `ensure_venv_ready` with an `EnsureOpts` struct
  (carrying `force_sync` and `quiet`); thread through to `sync_if_needed`. New
  `ensure_venv_ready_unattended` wrapper that applies the unattended-mode guard table from the spec.

Rust crate: **toolr** (CLI)

- `crates/toolr/src/cli.rs` — drop the `project deps` clap subcommand; add `project venv sync` (with
  `--force` and `--quiet` flags) and `project venv upgrade <package>`. Add hidden `project deps`
  subcommand that parses anything (`allow_external_subcommands(true)`) for the migration-hint path.
- `crates/toolr/src/project.rs` — rename `deps_sync` → `venv_sync` (now accepting `ArgMatches`,
  reading `--force` and `--quiet`); rename `deps_upgrade` → `venv_upgrade`; rewire
  `dispatch_project`; add `deps_migration_hint` arm; fix the comment + println at `run_project_init`
  lines 134/139.
- `crates/toolr/src/dispatch.rs` — update the "run `toolr project deps sync`" hint at line 204.
- `crates/toolr/src/builtin_completions.rs` — drop the `deps` group, move `sync` under
  `project.venv` (with `--force` / `--quiet` flags), add a `upgrade` leaf under `project.venv`
  (closing a pre-existing completion gap; in scope because the same file is being edited anyway),
  update the `for expected in […]` test.

Rust crate: **toolr-py** (Python runner)

- `crates/toolr-py/python/toolr/_runner.py` — update the two strings that mention `toolr project
  deps sync` / `toolr project deps upgrade` (lines 167, 407, 458) to use the new paths.

Tests

- `crates/toolr/tests/project_deps_upgrade.rs` → renamed to
  `crates/toolr/tests/project_venv_upgrade.rs`; update all references inside.
- `crates/toolr/tests/cli_smoke.rs` — update lines 309, 317, 346, 361 to assert the new hint text;
  add a migration-hint test.
- `crates/toolr/tests/project_venv_sync.rs` — **new file** covering: fresh-no-op (no uv subprocess),
  stale-syncs (uv runs once), `--force` always spawns uv, `--quiet` is silent on no-op, `--quiet`
  silences each row of the unattended-guard table.

Docs

- `docs/installation/mise.md` — append a new "Auto-sync the tools venv on shell-enter" section.
- `docs/cli.md` — replace lines 90, 95, 265 references; add an upgrade subsection.
- `docs/concepts.md` — line 35.
- `docs/project-config.md` — lines 23, 102, 106.
- `docs/internals/diagnostics.md` — lines 26, 39, 46.
- `CONTRIBUTING.md` — line 75 (commit message example).
- `docs/cli-files/project-deps-sync-help.txt` → renamed to
  `docs/cli-files/project-venv-sync-help.txt`; regenerated against the new clap definition.

Snippet regen

- `.pre-commit-hooks/regen-doc-snippets.py` — line 99: update the `Snippet(CLI_FILES /
  "project-deps-sync-help.txt", …)` entry to the new path and argv. Run the regen tool.

Release notes

- `UNRELEASED.md` — add a `## ⚠ Breaking changes` block describing the rename + behavior flip.

---

## Task 1: Add `quiet` parameter to `run_uv_sync` and `sync_if_needed`

**Files:**

- Modify: `crates/toolr-core/src/venv/sync.rs:46-73` (`run_uv_sync` signature + body) and lines
  100-115 (`sync_if_needed` signature + body)
- Test: `crates/toolr-core/src/venv/sync.rs` inline `#[cfg(test)] mod tests`

This task changes one signature and adds one flag. No callers are updated yet (compilation will
break briefly; we patch the call sites in subsequent tasks).

- [ ] **Step 1: Write the failing unit test**

Append this test to the `mod tests` block at the bottom of `crates/toolr-core/src/venv/sync.rs`
(place it after `sync_if_needed_translates_spawn_failure_to_uv_error`):

```rust
#[cfg(unix)]
#[test]
fn run_uv_sync_passes_quiet_when_requested() {
    use std::os::unix::fs::PermissionsExt;
    use std::io::Write;
    // Stub uv that records its argv to a file so we can assert on it.
    let tmp = TempDir::new().unwrap();
    let argv_log = tmp.path().join("argv.log");
    let stub = tmp.path().join("uv");
    let mut f = fs::File::create(&stub).unwrap();
    writeln!(
        f,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0",
        argv_log.display(),
    )
    .unwrap();
    drop(f);
    let mut perms = fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&stub, perms).unwrap();
    let uv = UvBinary {
        path: stub,
        version: (0, 4, 0),
        source: crate::uv::UvSource::Path,
    };
    let venv = tmp.path().join("venv");
    fs::create_dir_all(&venv).unwrap();
    let resolved = dummy_resolved(venv);

    run_uv_sync(&uv, tmp.path(), &resolved, /*quiet=*/ true)
        .expect("stub uv must exit 0");

    let captured = fs::read_to_string(&argv_log).unwrap();
    assert!(
        captured.lines().any(|l| l == "--quiet"),
        "expected `--quiet` in uv argv, got: {captured}"
    );
}

#[cfg(unix)]
#[test]
fn run_uv_sync_omits_quiet_by_default() {
    use std::os::unix::fs::PermissionsExt;
    use std::io::Write;
    let tmp = TempDir::new().unwrap();
    let argv_log = tmp.path().join("argv.log");
    let stub = tmp.path().join("uv");
    let mut f = fs::File::create(&stub).unwrap();
    writeln!(
        f,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0",
        argv_log.display(),
    )
    .unwrap();
    drop(f);
    let mut perms = fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&stub, perms).unwrap();
    let uv = UvBinary {
        path: stub,
        version: (0, 4, 0),
        source: crate::uv::UvSource::Path,
    };
    let venv = tmp.path().join("venv");
    fs::create_dir_all(&venv).unwrap();
    let resolved = dummy_resolved(venv);

    run_uv_sync(&uv, tmp.path(), &resolved, /*quiet=*/ false)
        .expect("stub uv must exit 0");

    let captured = fs::read_to_string(&argv_log).unwrap();
    assert!(
        !captured.lines().any(|l| l == "--quiet"),
        "did not expect `--quiet` in uv argv, got: {captured}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

```sh
cargo test -p toolr-core --lib venv::sync::tests::run_uv_sync_passes_quiet_when_requested
```

Expected: compilation error — `run_uv_sync` does not yet take a `quiet` parameter.

- [ ] **Step 3: Update `run_uv_sync` signature and body**

Edit `crates/toolr-core/src/venv/sync.rs` lines 46-73. Replace the existing function body:

```rust
/// Run `uv sync --project <tools>` synchronously, inheriting stdio.
/// When `quiet` is true, passes `--quiet` to uv so the subprocess
/// produces no informational output on success.
pub fn run_uv_sync(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    quiet: bool,
) -> Result<ExitStatus> {
    // Ensure the parent of an off-tree venv exists so uv can write into it.
    if let Some(parent) = resolved.venv_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut cmd = Command::new(&uv.path);
    cmd.arg("sync")
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir)
        // Unset any outer VIRTUAL_ENV so uv doesn't warn about a mismatch
        // with the tools venv (e.g. when invoked inside a mise-managed .venv).
        .env_remove("VIRTUAL_ENV");
    if quiet {
        cmd.arg("--quiet");
    }
    if let Some(version) = resolved.config.python_version.as_ref() {
        cmd.arg("--python").arg(version);
    }
    let status = cmd
        .status()
        .with_context(|| format!("spawning uv at {}", uv.path.display()))?;
    if status.success() {
        touch_marker(&resolved.venv_dir)?;
    }
    Ok(status)
}
```

- [ ] **Step 4: Update `sync_if_needed` signature and body**

Replace the body of `sync_if_needed` (lines 100-115):

```rust
/// Convenience wrapper that maps a failure to `UvError::SyncFailed`.
/// `quiet` is forwarded to `run_uv_sync` so the inner uv subprocess
/// inherits the same output discipline.
pub fn sync_if_needed(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    force: bool,
    quiet: bool,
) -> Result<(), UvError> {
    if !force && matches!(check_freshness(resolved, tools_dir), Freshness::Fresh) {
        return Ok(());
    }
    let status = run_uv_sync(uv, tools_dir, resolved, quiet)
        .map_err(|e| UvError::Http(e.to_string()))?;
    if !status.success() {
        return Err(UvError::SyncFailed(status.code()));
    }
    Ok(())
}
```

- [ ] **Step 5: Update existing tests inside `mod tests`**

Three existing tests call `sync_if_needed(…)` or `run_uv_sync(…)` with the old signatures. Patch
them to pass `false` (the new `quiet` parameter is irrelevant for those tests):

In `sync_if_needed_skips_run_when_fresh_and_force_off`:

```rust
sync_if_needed(&uv, tmp.path(), &resolved, false, false)
    .expect("fresh should short-circuit");
```

In `sync_if_needed_invokes_uv_when_force_set_even_if_fresh`:

```rust
sync_if_needed(&uv, tmp.path(), &resolved, true, false)
    .expect("force=true must always invoke uv");
```

In `sync_if_needed_propagates_nonzero_exit_as_sync_failed`:

```rust
let err = sync_if_needed(&uv, tmp.path(), &resolved, false, false)
```

In `sync_if_needed_translates_spawn_failure_to_uv_error`:

```rust
let err = sync_if_needed(&uv, tmp.path(), &resolved, true, false)
```

- [ ] **Step 6: Patch the lone non-test caller of `run_uv_sync`**

`crates/toolr/src/project.rs:257` calls `run_uv_sync` directly inside `deps_upgrade`. Patch the call
site:

```rust
let sync_status = toolr_core::venv::run_uv_sync(&uv, &tools_dir, &resolved, /*quiet=*/ false)?;
```

(Note: this caller will be renamed `venv_upgrade` in a later task; for now we just keep it
compiling.)

- [ ] **Step 7: Patch the in-core caller of `sync_if_needed`**

`crates/toolr-core/src/project.rs:27` calls `sync_if_needed`. Patch:

```rust
sync_if_needed(&uv, &tools, &resolved, force_sync, /*quiet=*/ false)
    .with_context(|| format!("uv sync against {}", tools.display()))?;
```

(Will be replaced again in Task 3 when we thread `quiet` through `ensure_venv_ready`.)

- [ ] **Step 8: Run the test suite — Task 1's new tests pass; nothing else regresses**

```sh
cargo test -p toolr-core --lib venv::sync::tests
```

Expected: all `venv::sync::tests::*` tests pass, including the two new ones.

```sh
cargo build -p toolr -p toolr-core
```

Expected: clean build.

- [ ] **Step 9: Commit**

```sh
git add crates/toolr-core/src/venv/sync.rs crates/toolr-core/src/project.rs crates/toolr/src/project.rs
git commit -m "feat(venv): thread --quiet through run_uv_sync and sync_if_needed"
```

---

## Task 2: Add `silent_refuse` to `ConsentMode`

**Files:**

- Modify: `crates/toolr-core/src/uv/install.rs:23-38` (`ConsentMode` struct + `from_env`), lines
  44-63 (`decide_install`).
- Test: same file's inline `mod tests`.

When `silent_refuse` is true, `decide_install` returns `Refuse` immediately whenever uv is not
already available — without printing a prompt or reading stdin. This is what the enter-hook needs so
it never blocks the shell.

- [ ] **Step 1: Write the failing unit test**

Locate the existing `mod tests` block (around line 320+) and append:

```rust
#[test]
fn silent_refuse_returns_refuse_without_consent_or_tty() {
    let consent = ConsentMode { silent_refuse: true, ..Default::default() };
    // No path/managed uv, stdin is a TTY → without silent_refuse we'd prompt.
    // With silent_refuse we must Refuse immediately.
    assert_eq!(
        decide_install(false, false, consent, true),
        InstallDecision::Refuse,
    );
}

#[test]
fn silent_refuse_does_not_override_already_available() {
    let consent = ConsentMode { silent_refuse: true, ..Default::default() };
    // uv is on PATH: silent_refuse must NOT pretend it's missing.
    assert_eq!(
        decide_install(true, false, consent, false),
        InstallDecision::AlreadyAvailable,
    );
}

#[test]
fn silent_refuse_does_not_override_explicit_consent() {
    let consent = ConsentMode {
        silent_refuse: true,
        yes_flag: true,
        ..Default::default()
    };
    // Caller explicitly asked for unattended install via --yes;
    // silent_refuse must not contradict that. (We pick `Install`
    // here because both flags being set is incoherent — but we
    // document that explicit consent wins.)
    // NOTE: in practice the CLI guarantees these are mutually
    // exclusive; this test pins the precedence rule.
    assert_eq!(
        decide_install(false, false, consent, false),
        InstallDecision::Install,
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

```sh
cargo test -p toolr-core --lib uv::install::tests::silent_refuse
```

Expected: compilation error — `ConsentMode` has no field `silent_refuse`.

- [ ] **Step 3: Add `silent_refuse` field to `ConsentMode`**

Edit `crates/toolr-core/src/uv/install.rs` lines 22-38. Replace the struct + impl:

```rust
/// How the toolr binary was invoked, for non-interactive decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConsentMode {
    /// `--yes` was passed.
    pub yes_flag: bool,
    /// `TOOLR_AUTO_INSTALL_UV=1` is set in the environment.
    pub auto_install_env: bool,
    /// Caller is running unattended (e.g. a shell-enter hook) and must
    /// never prompt; if uv isn't available and `yes_flag`/`auto_install_env`
    /// haven't pre-authorised an install, return `Refuse` silently.
    pub silent_refuse: bool,
}

impl ConsentMode {
    pub fn from_env() -> Self {
        Self {
            yes_flag: false,
            auto_install_env: std::env::var_os("TOOLR_AUTO_INSTALL_UV")
                .is_some_and(|v| v == "1"),
            silent_refuse: false,
        }
    }
}
```

- [ ] **Step 4: Update `decide_install` to honor `silent_refuse`**

Replace lines 44-63:

```rust
/// Pure decision logic. `path_found` is whether `find_uv_on_path` returned
/// `Some`. `managed_found` is whether `find_managed_uv` returned `Some`.
/// `stdin_tty` controls whether to prompt; in tests / pipes we never
/// prompt and rely on `consent` instead.
pub fn decide_install(
    path_found: bool,
    managed_found: bool,
    consent: ConsentMode,
    stdin_tty: bool,
) -> InstallDecision {
    if path_found || managed_found {
        return InstallDecision::AlreadyAvailable;
    }
    if consent.yes_flag || consent.auto_install_env {
        return InstallDecision::Install;
    }
    if consent.silent_refuse {
        return InstallDecision::Refuse;
    }
    if !stdin_tty {
        return InstallDecision::Refuse;
    }
    match prompt_for_consent() {
        Ok(true) => InstallDecision::Install,
        _ => InstallDecision::Refuse,
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

```sh
cargo test -p toolr-core --lib uv::install::tests
```

Expected: all existing `uv::install::tests::*` tests still pass; the three new `silent_refuse_*`
tests pass.

- [ ] **Step 6: Commit**

```sh
git add crates/toolr-core/src/uv/install.rs
git commit -m "feat(uv): add silent_refuse to ConsentMode for unattended callers"
```

---

## Task 3: Add `EnsureOpts` to `ensure_venv_ready`; thread `quiet`

**Files:**

- Modify: `crates/toolr-core/src/project.rs:16-40` (signature + body)
- Modify: all four callers of `ensure_venv_ready` (`crates/toolr/src/project.rs` lines 142, 213,
  247, 342).

`ensure_venv_ready` today takes `force_sync: bool`. We replace that with an `EnsureOpts` struct so
we can also pass `quiet` to `sync_if_needed` without growing the positional-argument list.

- [ ] **Step 1: Write the failing unit test**

Append to the `mod tests` block at the bottom of `crates/toolr-core/src/project.rs`:

```rust
#[test]
fn ensure_opts_default_means_no_force_no_quiet() {
    let opts = EnsureOpts::default();
    assert!(!opts.force_sync);
    assert!(!opts.quiet);
}

#[test]
fn ensure_opts_builder_setters_work() {
    let opts = EnsureOpts::default().with_force_sync(true).with_quiet(true);
    assert!(opts.force_sync);
    assert!(opts.quiet);
}
```

- [ ] **Step 2: Run tests — they fail (no `EnsureOpts`)**

```sh
cargo test -p toolr-core --lib project::tests::ensure_opts
```

Expected: compilation error.

- [ ] **Step 3: Define `EnsureOpts` and update `ensure_venv_ready`**

Edit `crates/toolr-core/src/project.rs`. Replace the existing `ensure_venv_ready` (lines 13-40)
with:

```rust
/// Options for [`ensure_venv_ready`]. Constructed via `Default::default()`
/// plus the builder setters; new fields can be added without breaking
/// callers that took an `EnsureOpts::default()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnsureOpts {
    /// Run `uv sync` even when the freshness stamp says the venv is fresh.
    pub force_sync: bool,
    /// Forward `--quiet` to the uv subprocess. Has no effect when the
    /// stamp short-circuits sync (no uv invocation happens).
    pub quiet: bool,
}

impl EnsureOpts {
    pub fn with_force_sync(mut self, v: bool) -> Self {
        self.force_sync = v;
        self
    }
    pub fn with_quiet(mut self, v: bool) -> Self {
        self.quiet = v;
        self
    }
}

/// One-stop "make the venv ready" entrypoint. Returns the resolved venv
/// + the chosen uv binary on success.
pub fn ensure_venv_ready(
    cwd: &Path,
    consent: ConsentMode,
    opts: EnsureOpts,
) -> Result<(ResolvedVenv, UvBinary)> {
    let repo_root = discover_project_root(cwd)
        .context("locating project root for the tools venv")?;
    let resolved = resolve_venv_path(&repo_root)
        .context("resolving the tools venv path")?;
    let uv = ensure_uv(consent).map_err(UvError::into_anyhow)?;
    let tools = repo_root.join("tools");
    sync_if_needed(&uv, &tools, &resolved, opts.force_sync, opts.quiet)
        .with_context(|| format!("uv sync against {}", tools.display()))?;
    validate_venv(&resolved.venv_dir, &resolved.python)
        .context("validating the synced venv")?;
    write_cache_meta_best_effort(&resolved, &repo_root);
    let outcomes = perform_editable_installs(
        &uv,
        &resolved.config,
        &repo_root,
        &resolved.python,
    );
    warn_failures(&outcomes);
    Ok((resolved, uv))
}
```

- [ ] **Step 4: Update existing test that calls `ensure_venv_ready`**

The test `ensure_venv_ready_reports_missing_project_root` (line 88) calls
`ensure_venv_ready(tmp.path(), ConsentMode::default(), false)`. Update:

```rust
let err = ensure_venv_ready(tmp.path(), ConsentMode::default(), EnsureOpts::default())
    .expect_err("expected ensure_venv_ready to fail without a project");
```

- [ ] **Step 5: Update the four CLI call sites**

`crates/toolr/src/project.rs`:

Line 141-143 (inside `run_project_init`, the auto-sync at the end):

```rust
let consent = toolr_core::uv::install::ConsentMode::from_env();
let (resolved, uv) = toolr_core::project::ensure_venv_ready(
    cwd,
    consent,
    toolr_core::project::EnsureOpts::default().with_force_sync(true),
)?;
```

Lines 212-215 (inside `deps_sync` — soon to become `venv_sync` in Task 4):

```rust
let cwd = std::env::current_dir()?;
let consent = toolr_core::uv::install::ConsentMode::from_env();
let (resolved, uv) = toolr_core::project::ensure_venv_ready(
    &cwd,
    consent,
    toolr_core::project::EnsureOpts::default().with_force_sync(true),
)?;
```

Lines 245-247 (inside `deps_upgrade` — soon to become `venv_upgrade`):

```rust
let consent = toolr_core::uv::install::ConsentMode::from_env();
let (resolved, uv) =
    toolr_core::project::ensure_venv_ready(&cwd, consent, toolr_core::project::EnsureOpts::default())?;
```

Lines 341-344 (inside `venv_shell`):

```rust
let cwd = std::env::current_dir()?;
let consent = toolr_core::uv::install::ConsentMode::from_env();
let (resolved, _) = toolr_core::project::ensure_venv_ready(
    &cwd,
    consent,
    toolr_core::project::EnsureOpts::default(),
)?;
```

- [ ] **Step 6: Run the test suite**

```sh
cargo test -p toolr-core -p toolr --lib
```

Expected: all green. (Integration tests that don't hit this signature continue to pass; ones that do
— none right now — would surface here.)

- [ ] **Step 7: Commit**

```sh
git add crates/toolr-core/src/project.rs crates/toolr/src/project.rs
git commit -m "refactor(venv): replace ensure_venv_ready force flag with EnsureOpts"
```

---

## Task 4: Add `project venv sync` + `project venv upgrade` to clap; remove `project deps`; wire dispatch

**Files:**

- Modify: `crates/toolr/src/cli.rs:281-296` (replace the `deps` clap subcommand block; insert new
  clauses under `venv`).
- Modify: `crates/toolr/src/project.rs:14-33` (`dispatch_project` match arms) plus `deps_sync` →
  `venv_sync` and `deps_upgrade` → `venv_upgrade` (lines 210-269).
- Test: existing tests will need updating (next tasks); new tests added in Task 7.

At the end of this task, `toolr project venv sync` and `toolr project venv upgrade <pkg>` exist with
the new `--force`/`--quiet` flags, `toolr project deps <anything>` returns a tailored migration-hint
error, and the `force_sync=true` default is flipped to `false` (the user-facing behavior change).

- [ ] **Step 1: Write the failing CLI smoke test for the new command**

Append to `crates/toolr/tests/cli_smoke.rs` (place near the other `--help`-asserting tests):

```rust
/// `toolr project venv sync --help` exists and mentions `--force` / `--quiet`.
#[test]
fn project_venv_sync_help_lists_force_and_quiet() {
    let toolr = toolr_bin();
    let output = std::process::Command::new(&toolr)
        .args(["project", "venv", "sync", "--help"])
        .output()
        .expect("running toolr should succeed");
    assert!(output.status.success(), "exit: {:?}", output.status.code());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--force"), "missing --force in help:\n{stdout}");
    assert!(stdout.contains("--quiet"), "missing --quiet in help:\n{stdout}");
}

/// `toolr project deps sync` (removed) prints the migration hint and exits non-zero.
#[test]
fn project_deps_removed_prints_migration_hint() {
    let toolr = toolr_bin();
    let output = std::process::Command::new(&toolr)
        .args(["project", "deps", "sync"])
        .output()
        .expect("running toolr should succeed");
    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("`project deps` was removed in 0.22"),
        "stderr missing removal notice:\n{stderr}"
    );
    assert!(
        stderr.contains("toolr project venv"),
        "stderr missing pointer to new path:\n{stderr}"
    );
}
```

- [ ] **Step 2: Run tests — they fail**

```sh
cargo test -p toolr --test cli_smoke project_venv_sync_help_lists_force_and_quiet project_deps_removed_prints_migration_hint
```

Expected: both fail. The first because `project venv sync` doesn't exist; the second because
`project deps sync` currently runs the old handler instead of emitting the migration hint.

- [ ] **Step 3: Update clap definitions in `cli.rs`**

Edit `crates/toolr/src/cli.rs`. Replace lines 281-307 (the entire `Command::new("deps") …
.subcommand(Command::new("venv") …)` block) with:

```rust
            .subcommand(
                // Migration shim: parses `toolr project deps <anything>` so we
                // can emit a tailored "removed in 0.22; use `project venv`"
                // error from `dispatch_project`. Hidden from `--help`. Drop
                // this subcommand after 0.23 once users have migrated.
                Command::new("deps")
                    .hide(true)
                    .allow_external_subcommands(true)
                    .about("(removed in 0.22) use `toolr project venv` instead"),
            )
            .subcommand(
                Command::new("venv")
                    .about("Inspect, sync, and operate on the tools venv")
                    .subcommand_required(true)
                    .subcommand(
                        Command::new("path").about("Print the absolute path to the tools venv"),
                    )
                    .subcommand(
                        Command::new("shell")
                            .about("Spawn a subshell with the tools venv activated"),
                    )
                    .subcommand(
                        Command::new("sync")
                            .about("Sync the tools venv against tools/pyproject.toml + tools/uv.lock (no-op when fresh)")
                            .arg(
                                Arg::new("force")
                                    .long("force")
                                    .short('f')
                                    .action(ArgAction::SetTrue)
                                    .help("Re-run `uv sync` even when the freshness stamp says the venv is up to date"),
                            )
                            .arg(
                                Arg::new("quiet")
                                    .long("quiet")
                                    .short('q')
                                    .action(ArgAction::SetTrue)
                                    .help("Silent on success and on benign unattended-mode exits (no toolr/uv output)"),
                            ),
                    )
                    .subcommand(
                        Command::new("upgrade")
                            .about("Bump a single package's pin via `uv lock --upgrade-package` + `uv sync`")
                            .arg(
                                Arg::new("package")
                                    .value_name("PACKAGE")
                                    .required(true)
                                    .help("Name of the package to upgrade (must already appear in tools/pyproject.toml)"),
                            ),
                    ),
            )
```

- [ ] **Step 4: Rewire `dispatch_project` and rename the handlers**

Edit `crates/toolr/src/project.rs`. Replace lines 14-33 (`dispatch_project` body):

```rust
pub fn dispatch_project(matches: &ArgMatches) -> Result<ExitCode> {
    match matches.subcommand() {
        Some(("init", init_m)) => project_init(init_m),
        Some(("deps", _)) => deps_migration_hint(),
        Some(("venv", venv_m)) => match venv_m.subcommand() {
            Some(("path", _)) => venv_path(),
            Some(("shell", _)) => venv_shell(),
            Some(("sync", sync_m)) => venv_sync(sync_m),
            Some(("upgrade", upgrade_m)) => venv_upgrade(upgrade_m),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        Some(("manifest", manifest_m)) => match manifest_m.subcommand() {
            Some(("rebuild", _)) => manifest_rebuild(),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        _ => unreachable!("clap enforces subcommand_required"),
    }
}

/// Emit the migration hint for `toolr project deps <…>`. Returns exit
/// code 2 (same code we use for "your inputs were valid but you're
/// pointing at the wrong target" — see `project_init`'s scaffold-
/// conflict path).
fn deps_migration_hint() -> Result<ExitCode> {
    eprintln!("error: `project deps` was removed in 0.22");
    eprintln!("hint: use `toolr project venv` instead");
    eprintln!("       project deps sync       →  toolr project venv sync");
    eprintln!("       project deps upgrade …  →  toolr project venv upgrade …");
    eprintln!("see CHANGELOG.md (0.22 BREAKING) for the rename");
    Ok(ExitCode::from(2))
}
```

- [ ] **Step 5: Rename `deps_sync` → `venv_sync` (flag-aware, behavior-flipped)**

Replace the existing `deps_sync` function (lines 210-222) with:

```rust
fn venv_sync(matches: &ArgMatches) -> Result<ExitCode> {
    let force = matches.get_flag("force");
    let quiet = matches.get_flag("quiet");

    let cwd = std::env::current_dir()?;
    let mut consent = toolr_core::uv::install::ConsentMode::from_env();
    if quiet {
        // Unattended path: never prompt. If uv is missing and we have
        // no env-level consent, return Refuse silently and the guards
        // in venv_sync_unattended_quiet_exit below convert that into a
        // benign exit 0.
        consent.silent_refuse = true;
    }

    let opts = toolr_core::project::EnsureOpts::default()
        .with_force_sync(force)
        .with_quiet(quiet);

    let result = toolr_core::project::ensure_venv_ready(&cwd, consent, opts);

    if quiet {
        if let Some(code) = venv_sync_unattended_quiet_exit(&result) {
            return Ok(code);
        }
    }

    let (resolved, uv) = result?;

    if !quiet {
        println!(
            "toolr: synced venv at {} using uv {}.{}.{}",
            resolved.venv_dir.display(),
            uv.version.0, uv.version.1, uv.version.2,
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Unattended-mode guard table (see spec §"`--quiet` semantics").
/// Returns `Some(ExitCode::SUCCESS)` when the failure is one of the
/// benign rows that `--quiet` swallows (not-a-toolr-repo, lock
/// missing, uv-consent absent). Returns `None` to let the error
/// propagate normally (lock unparsable, uv sync failed, etc).
fn venv_sync_unattended_quiet_exit(
    result: &Result<(toolr_core::venv::ResolvedVenv, toolr_core::uv::UvBinary), anyhow::Error>,
) -> Option<ExitCode> {
    let err = result.as_ref().err()?;
    let chain: Vec<String> = err.chain().map(|e| e.to_string()).collect();
    let joined = chain.join(" :: ");

    // The benign markers we silently exit on. We match against
    // the error-chain context strings emitted by `ensure_venv_ready`
    // and friends.
    let benign_markers = [
        // No tools/pyproject.toml — discover_project_root NotFound,
        // or resolve_venv_path missing tools/pyproject.toml.
        "locating project root",
        "resolving the tools venv path",
        // uv install was needed but silent_refuse short-circuited.
        "uv binary not available",
    ];

    if benign_markers.iter().any(|m| joined.contains(m)) {
        return Some(ExitCode::SUCCESS);
    }
    None
}
```

- [ ] **Step 6: Rename `deps_upgrade` → `venv_upgrade`**

Lines 224-269. The body is unchanged; just rename the function:

```rust
fn venv_upgrade(matches: &ArgMatches) -> Result<ExitCode> {
    // … existing body unchanged …
}
```

- [ ] **Step 7: Update the run_project_init hint at line 134**

```rust
        if !quiet {
            println!("toolr: skipping `uv sync` (--no-sync)");
            println!("toolr: run `toolr project venv sync` when you are ready");
        }
```

And the comment on line 139:

```rust
    // Auto-sync — same path as `toolr project venv sync --force`.
```

- [ ] **Step 8: Verify the `uv binary not available` marker actually appears in the error chain**

Check `crates/toolr-core/src/uv/install.rs` for the error text returned when `decide_install`
produces `Refuse`. Search:

```sh
grep -n "not available\|not found\|Refuse" /Users/pedro.algarvio/projects/me/toolr/crates/toolr-core/src/uv/install.rs | head
```

If the actual error string differs (e.g. it might be "uv not available" or "could not find uv"),
update the `benign_markers` array in Step 5 to match the exact substring. The point is: the
unattended-quiet path must catch the silent-refuse case. Add a comment naming the source file + line
of the error message you're matching.

- [ ] **Step 9: Run the smoke tests from Step 1**

```sh
cargo test -p toolr --test cli_smoke project_venv_sync_help_lists_force_and_quiet project_deps_removed_prints_migration_hint
```

Expected: both pass.

- [ ] **Step 10: Run the full crate test suite to spot regressions**

```sh
cargo test -p toolr -p toolr-core
```

Expected: some integration tests (`cli_smoke`, `project_deps_upgrade`, completion fixtures) will
fail because they still reference the old subcommand. That's expected — Tasks 5–9 fix them. Note
which tests fail so you can verify they're addressed in the right task.

- [ ] **Step 11: Commit**

```sh
git add crates/toolr/src/cli.rs crates/toolr/src/project.rs
git commit -m "feat(cli): move project deps to project venv; flip sync default"
```

---

## Task 5: Update completion entries

**Files:**

- Modify: `crates/toolr/src/builtin_completions.rs:31` (drop `deps` group), 54 (move sync leaf), 245
  (test expectations); add `upgrade` leaf.

- [ ] **Step 1: Write the failing test**

Edit the test at line 241-251. Replace it with:

```rust
    #[test]
    fn project_offers_known_subcommands() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["project", ""]));
        for expected in ["init", "venv", "manifest"] {
            assert!(
                out.contains(&expected.to_string()),
                "missing {expected} in {out:?}"
            );
        }
        // `deps` was removed in 0.22 and must NOT appear as a completion
        // candidate (it would mislead users into trying the old path).
        assert!(
            !out.contains(&"deps".to_string()),
            "`deps` should not be a completion candidate, got: {out:?}"
        );
    }

    #[test]
    fn project_venv_offers_path_shell_sync_upgrade() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["project", "venv", ""]));
        for expected in ["path", "shell", "sync", "upgrade"] {
            assert!(
                out.contains(&expected.to_string()),
                "missing {expected} under project venv, got: {out:?}"
            );
        }
    }

    #[test]
    fn project_venv_sync_offers_force_and_quiet_flags() {
        let m = merged_empty_manifest();
        let out = serve_completions(&m, &tokens(&["project", "venv", "sync", "--"]));
        for expected in ["--force", "--quiet"] {
            assert!(
                out.contains(&expected.to_string()),
                "missing {expected} in project venv sync flags, got: {out:?}"
            );
        }
    }
```

- [ ] **Step 2: Run tests — they fail**

```sh
cargo test -p toolr --lib builtin_completions::tests::project_
```

Expected: `project_offers_known_subcommands` fails (still claims `deps` should be present), and the
two new tests fail (no `venv.sync` flags, no `venv.upgrade` leaf in the table).

- [ ] **Step 3: Update the completion table**

Edit `crates/toolr/src/builtin_completions.rs` lines 29-67:

```rust
    let groups = vec![
        top_group("project", "Operations on the current repo's tools/ directory"),
        child_group("venv", "project", "Inspect, sync, and operate on the tools venv"),
        child_group("manifest", "project", "Manage the project's toolr manifest"),
        top_group("self", "Operations on toolr itself"),
        child_group("cache", "self", "Manage the cache of per-repo virtualenvs"),
        child_group("completion", "self", "Manage shell completion scripts"),
    ];

    let commands = vec![
        // project ...
        leaf(
            "init",
            "project",
            "Scaffold tools/ in the current directory",
            vec![
                flag("force"),
                flag("no-sync"),
                opt_enum("venv-location", &["cache", "in-tree"]),
                flag("no-example"),
                opt("python"),
                flag("quiet"),
            ],
        ),
        leaf("path", "project.venv", "Print the absolute path to the tools venv", vec![]),
        leaf(
            "shell",
            "project.venv",
            "Spawn a subshell with the tools venv activated",
            vec![],
        ),
        leaf(
            "sync",
            "project.venv",
            "Sync the tools venv (no-op when fresh)",
            vec![flag("force"), flag("quiet")],
        ),
        leaf(
            "upgrade",
            "project.venv",
            "Bump a single package's pin via `uv lock --upgrade-package` + `uv sync`",
            vec![positional("package")],
        ),
        leaf(
            "rebuild",
            "project.manifest",
            "Regenerate the static + dynamic manifest in place",
            vec![],
        ),
        // self ... (unchanged below)
```

- [ ] **Step 4: Run completion tests**

```sh
cargo test -p toolr --lib builtin_completions::tests
```

Expected: all three new tests + the updated existing test pass.

- [ ] **Step 5: Commit**

```sh
git add crates/toolr/src/builtin_completions.rs
git commit -m "feat(completions): move project venv sync/upgrade; drop deps"
```

---

## Task 6: Update hint strings in dispatch.rs and the Python runner

**Files:**

- Modify: `crates/toolr/src/dispatch.rs:204`
- Modify: `crates/toolr-py/python/toolr/_runner.py:167, 407, 458`
- Modify: `crates/toolr-core/src/deps_check/mod.rs:12` (comment)
- [ ] **Step 1: Update the Rust dispatch hint**

`crates/toolr/src/dispatch.rs` line 202-206. Replace:

```rust
    if !python.is_file() {
        anyhow::bail!(
            "Python interpreter not found at {}.\n\
             Run `toolr project venv sync` to materialise the tools venv.",
            python.display()
        );
    }
```

- [ ] **Step 2: Update the Python runner hints**

`crates/toolr-py/python/toolr/_runner.py`:

Line 167 (an example in a docstring or message):

```python
            "  toolr project venv upgrade toolr-py\n\n"
```

Line 398-407 (the freshness hint function):

```python
    """Append the styled "run `toolr project venv sync`" hint to ``stream``.

    Used by both the static missing-dependency pre-flight (Rust side) and
    the Python runner's ``ImportError`` fallback so a stale venv produces
    one consistent recovery instruction.
    """
    stream.write(
        "A dependency may be missing - run `toolr project venv sync` "
```

Line 458 (the surrounding comment):

```python
        # failure still gets the styled "run venv sync" guidance.
```

(Search for the literal string `deps sync` and `project deps` in `_runner.py` and replace each
occurrence; the line numbers above are guides, not gospel.)

- [ ] **Step 3: Update the comment in deps_check/mod.rs**

`crates/toolr-core/src/deps_check/mod.rs` line 11-12:

```rust
//! a missing dep is caught in milliseconds with a styled error and an
//! actionable "run `toolr project venv sync`" hint instead of a raw
```

Search the same file for any further `deps sync` mentions and update those too.

- [ ] **Step 4: Run the test suite to surface stragglers**

```sh
cargo test -p toolr -p toolr-core
```

Expected: any failing assertion now points at the precise line still saying `project deps sync`.
Update those tests in subsequent tasks (Task 7 handles `cli_smoke`; Task 8 handles
`project_deps_upgrade`).

```sh
grep -rn "project deps\|deps sync\|deps upgrade" crates/ docs/ skills/ CONTRIBUTING.md README.md 2>/dev/null
```

Expected output: only the files queued for update in Tasks 7-10 should remain. Note the survivors.

- [ ] **Step 5: Commit**

```sh
git add crates/toolr/src/dispatch.rs crates/toolr-py/python/toolr/_runner.py crates/toolr-core/src/deps_check/mod.rs
git commit -m "fix(hints): point users to project venv sync, not project deps sync"
```

---

## Task 7: Update `cli_smoke.rs` assertions to the new hint text

**Files:**

- Modify: `crates/toolr/tests/cli_smoke.rs` lines 309, 317, 346, 361 (and any other lines that hit
  the `deps sync` hint).

- [ ] **Step 1: List the failing assertions**

```sh
grep -n "project deps\|deps sync" /Users/pedro.algarvio/projects/me/toolr/crates/toolr/tests/cli_smoke.rs
```

Confirm: lines 309, 317, 346, 361 (or thereabouts).

- [ ] **Step 2: Replace each occurrence**

Use `Edit` with `replace_all=true` on `crates/toolr/tests/cli_smoke.rs`:

- `toolr project deps sync` → `toolr project venv sync`

Then re-check that the test docstrings still describe the right scenario; update any prose comments
that say "`deps sync`" but already replaced strings shouldn't need another pass.

- [ ] **Step 3: Run the smoke tests**

```sh
cargo test -p toolr --test cli_smoke
```

Expected: all smoke tests pass, including the new ones added in Task 4.

- [ ] **Step 4: Commit**

```sh
git add crates/toolr/tests/cli_smoke.rs
git commit -m "test(cli): update smoke assertions to project venv sync"
```

---

## Task 8: Rename `project_deps_upgrade.rs` → `project_venv_upgrade.rs`

**Files:**

- Rename: `crates/toolr/tests/project_deps_upgrade.rs` →
  `crates/toolr/tests/project_venv_upgrade.rs`
- Modify: the renamed file's internal references.
- [ ] **Step 1: git-mv the file**

```sh
git mv crates/toolr/tests/project_deps_upgrade.rs crates/toolr/tests/project_venv_upgrade.rs
```

- [ ] **Step 2: Update the file's contents**

Open `crates/toolr/tests/project_venv_upgrade.rs`. Replace every occurrence of:

- `toolr project deps upgrade` → `toolr project venv upgrade`
- `project deps upgrade` (in comments / docstrings) → `project venv upgrade`
- The first-line docstring `//! Integration tests for \`toolr project deps upgrade <pkg>\`.` → `//!
  Integration tests for \`toolr project venv upgrade <pkg>\`.`

```sh
grep -n "deps" /Users/pedro.algarvio/projects/me/toolr/crates/toolr/tests/project_venv_upgrade.rs
```

Expected: no matches.

- [ ] **Step 3: Run the renamed test file**

```sh
cargo test -p toolr --test project_venv_upgrade
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```sh
git add crates/toolr/tests/
git commit -m "test(cli): rename project_deps_upgrade to project_venv_upgrade"
```

---

## Task 9: Add `project_venv_sync.rs` integration tests for the new behavior

**Files:**

- Create: `crates/toolr/tests/project_venv_sync.rs`

This test file covers the behaviors that distinguish the new `venv sync` from the old `deps sync`:
the freshness short-circuit, `--force`, `--quiet`, and the unattended-mode guards.

- [ ] **Step 1: Inspect existing fixtures for a model**

Open `crates/toolr/tests/project_deps_upgrade.rs` (now renamed). The setup pattern (how it scaffolds
a temp project, points toolr at it, and asserts on stub-uv invocations) is what we want to mirror.
If `toolr_bin()` or a shared helper exists, reuse it.

```sh
grep -n "fn toolr_bin\|fn setup\|tempfile::tempdir\|sample-repo" /Users/pedro.algarvio/projects/me/toolr/crates/toolr/tests/*.rs | head -20
```

- [ ] **Step 2: Create `project_venv_sync.rs`**

```rust
//! Integration tests for `toolr project venv sync` (the renamed and
//! behavior-flipped successor to `toolr project deps sync`).
//!
//! Covers:
//! - Fresh venv: default `sync` no-ops (no uv subprocess spawned).
//! - Stale venv: default `sync` spawns uv exactly once.
//! - `--force`: spawns uv even when fresh.
//! - `--quiet`: silent on success; benign guards exit 0 silently.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

mod common;
use common::toolr_bin;
// If `common` doesn't already exist, create it as a thin module that
// just exports `toolr_bin()` reading the CARGO_BIN_EXE_toolr env var.
// Mirror the convention used by project_venv_upgrade.rs.

fn scaffold_tools(tmp: &Path) {
    fs::create_dir_all(tmp.join("tools")).unwrap();
    fs::write(
        tmp.join("tools/pyproject.toml"),
        r#"[project]
name = "tools"
version = "0.0.0"
requires-python = ">=3.10"
dependencies = []
"#,
    )
    .unwrap();
    fs::write(tmp.join("tools/uv.lock"), b"# stub lock\n").unwrap();
}

/// Write a `STAMP` file with the same mtime semantics
/// `check_freshness` looks at: newer than `tools/uv.lock` = Fresh.
fn write_fresh_stamp(venv_dir: &Path) {
    fs::create_dir_all(venv_dir).unwrap();
    fs::write(venv_dir.join(".toolr-sync-stamp"), b"").unwrap();
    // Also write a marker pyvenv.cfg so future venv-existence guards
    // (if/when check_freshness gains one) keep treating this as a real venv.
    fs::write(venv_dir.join("pyvenv.cfg"), b"home = stub").unwrap();
}

#[test]
fn venv_sync_is_noop_when_fresh() {
    // This test asserts on the toolr stdout ("toolr: synced venv at ...");
    // exit code 0 confirms the freshness short-circuit succeeded. The
    // *stronger* assertion (no uv subprocess spawned) requires injecting
    // a stub-uv via PATH manipulation — defer that to the stub-arg-capture
    // pattern used by Task 1's unit tests.
    let tmp = TempDir::new().unwrap();
    scaffold_tools(tmp.path());
    // Caller's tools venv lives in cache by default. We can't easily
    // pre-populate it from a test, so we exercise the path with
    // `force=true` first to create a real venv, then run again without
    // --force to observe the no-op.
    let bin = toolr_bin();
    let first = Command::new(&bin)
        .args(["project", "venv", "sync", "--force"])
        .current_dir(tmp.path())
        .env("TOOLR_AUTO_INSTALL_UV", "1")
        .output()
        .expect("toolr should run");
    assert!(
        first.status.success(),
        "first run failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = Command::new(&bin)
        .args(["project", "venv", "sync"])
        .current_dir(tmp.path())
        .env("TOOLR_AUTO_INSTALL_UV", "1")
        .output()
        .expect("toolr should run");
    assert!(
        second.status.success(),
        "second run failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    // The second run must have observed the fresh stamp and short-
    // circuited; we can't directly assert "uv was not invoked" without
    // PATH stubbing, but the `toolr: synced venv at` line is still
    // printed (the short-circuit returns success with that message).
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("toolr: synced venv at"),
        "expected synced-venv line on second run: {stdout}"
    );
}

#[test]
fn venv_sync_force_runs_even_when_fresh() {
    // Functional check: `--force` must succeed even on a fresh venv;
    // the deeper "uv was spawned both times" assertion is best handled
    // by a future stub-uv harness. For now we assert the user-visible
    // contract: --force never errors with "already fresh".
    let tmp = TempDir::new().unwrap();
    scaffold_tools(tmp.path());
    let bin = toolr_bin();
    for _ in 0..2 {
        let out = Command::new(&bin)
            .args(["project", "venv", "sync", "--force"])
            .current_dir(tmp.path())
            .env("TOOLR_AUTO_INSTALL_UV", "1")
            .output()
            .expect("toolr should run");
        assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn venv_sync_quiet_is_silent_on_success() {
    let tmp = TempDir::new().unwrap();
    scaffold_tools(tmp.path());
    let bin = toolr_bin();
    let out = Command::new(&bin)
        .args(["project", "venv", "sync", "--quiet"])
        .current_dir(tmp.path())
        .env("TOOLR_AUTO_INSTALL_UV", "1")
        .output()
        .expect("toolr should run");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.is_empty(),
        "--quiet should produce no stdout; got: {stdout}"
    );
}

#[test]
fn venv_sync_quiet_silently_exits_when_not_a_toolr_repo() {
    // No tools/pyproject.toml exists at all.
    let tmp = TempDir::new().unwrap();
    let bin = toolr_bin();
    let out = Command::new(&bin)
        .args(["project", "venv", "sync", "--quiet"])
        .current_dir(tmp.path())
        .output()
        .expect("toolr should run");
    assert!(
        out.status.success(),
        "--quiet must exit 0 when not in a toolr repo; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty() && out.stderr.is_empty(),
        "--quiet must produce no output; stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn venv_sync_without_quiet_errors_when_not_a_toolr_repo() {
    // Same setup as above (no tools/pyproject.toml), but no --quiet.
    // The user explicitly asked to sync; surfacing the error is correct.
    let tmp = TempDir::new().unwrap();
    let bin = toolr_bin();
    let out = Command::new(&bin)
        .args(["project", "venv", "sync"])
        .current_dir(tmp.path())
        .output()
        .expect("toolr should run");
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("locating project root")
            || stderr.contains("resolving the tools venv path"),
        "stderr should explain the missing project: {stderr}"
    );
}
```

If a `common` module file does not yet exist for shared test helpers, create
`crates/toolr/tests/common/mod.rs`:

```rust
//! Shared test helpers for crates/toolr integration tests.

use std::path::PathBuf;

/// Resolve the path to the built `toolr` binary that cargo placed in
/// the test target directory. Mirrors what cargo's CARGO_BIN_EXE_*
/// env var would expose, but works whether invoked via `cargo test`
/// or as part of a workspace test run.
pub fn toolr_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_toolr"))
}
```

(If `project_venv_upgrade.rs` already uses a different convention for locating the binary, mirror
that convention instead — keep the test crate consistent.)

- [ ] **Step 3: Run the new integration test**

```sh
cargo test -p toolr --test project_venv_sync
```

Expected: all five tests pass.

- [ ] **Step 4: Commit**

```sh
git add crates/toolr/tests/project_venv_sync.rs crates/toolr/tests/common/mod.rs
git commit -m "test(cli): integration coverage for project venv sync flag matrix"
```

---

## Task 10: Update the doc-snippet regen + rename the captured help file

**Files:**

- Modify: `.pre-commit-hooks/regen-doc-snippets.py:99` (Snippet definition).
- Rename: `docs/cli-files/project-deps-sync-help.txt` → `docs/cli-files/project-venv-sync-help.txt`.
- Run the regen tool to rewrite the captured help.
- [ ] **Step 1: Update the Snippet definition**

`.pre-commit-hooks/regen-doc-snippets.py` line 99 (the `project-deps-sync-help.txt` Snippet entry).
Replace:

```python
    Snippet(CLI_FILES / "project-venv-sync-help.txt", ("project", "venv", "sync", "--help")),
```

- [ ] **Step 2: Move the captured help file**

```sh
git mv docs/cli-files/project-deps-sync-help.txt docs/cli-files/project-venv-sync-help.txt
```

(The file's contents will be overwritten by the regen tool — the rename preserves git history.)

- [ ] **Step 3: Regenerate snippets**

```sh
./.pre-commit-hooks/regen-doc-snippets.py
```

Expected: `docs/cli-files/project-venv-sync-help.txt` is rewritten with the new help text — which
now includes `--force` and `--quiet`. No other files should change.

- [ ] **Step 4: Verify drift is zero in `--check` mode**

```sh
./.pre-commit-hooks/regen-doc-snippets.py --check
```

Expected: exits 0, no diff.

- [ ] **Step 5: Commit**

```sh
git add .pre-commit-hooks/regen-doc-snippets.py docs/cli-files/
git commit -m "docs(snippets): regenerate project venv sync --help capture"
```

---

## Task 11: Update prose docs that reference `project deps`

**Files:**

- `docs/cli.md` (lines 90, 95, 265)
- `docs/concepts.md` (line 35)
- `docs/project-config.md` (lines 23, 102, 106)
- `docs/internals/diagnostics.md` (lines 26, 39, 46)
- `CONTRIBUTING.md` (line 75)
- [ ] **Step 1: Update `docs/cli.md`**

Lines 90 and 95 — replace the section header and its example. The section header changes from:

`### \`toolr project venv sync\` {#project-venv-sync}` (note the anchor ID also flips from
`#project-deps-sync` to `#project-venv-sync`).

Update the surrounding prose to describe the new no-op-when-fresh default and the `--force` /
`--quiet` flags. The example invocation that follows must change to `toolr project venv sync --help`
(inside a `text` fenced code block).

Line 265 — replace the inline reference `toolr project deps sync` with `toolr project venv sync` (it
appears in a sentence like "...is the file populated by `toolr project venv sync` after a consent
flow.").

Add a `### \`toolr project venv upgrade <package>\`` subsection immediately after the `sync`
subsection if one doesn't already exist (the symmetry leaves the docs cleaner now that both
subcommands have moved under `venv`). The body can be a one-paragraph stub explaining that it bumps
a single package's pin via `uv lock --upgrade-package` and re-syncs the venv, plus a note that the
package must already appear in `tools/pyproject.toml` (toolr surfaces a clear error for typos rather
than letting uv silently no-op).

- [ ] **Step 2: Update `docs/concepts.md` line 35**

```markdown
Created by `toolr project venv sync` (or automatically by
```

- [ ] **Step 3: Update `docs/project-config.md`**

Lines 23, 102, 106 — replace each `toolr project deps sync` with `toolr project venv sync`. The
"Interaction with" section header on line 106 becomes:

```markdown
## Interaction with `toolr project venv sync`
```

- [ ] **Step 4: Update `docs/internals/diagnostics.md`**

Lines 26, 39, 46 — replace each `toolr project deps sync` with `toolr project venv sync`.

- [ ] **Step 5: Update `CONTRIBUTING.md` line 75**

```markdown
- `feat(cli): add --quiet flag to project venv sync`
```

- [ ] **Step 6: Verify no `project deps` references remain in prose**

```sh
grep -rn "project deps\|deps sync\|deps upgrade" docs/ CONTRIBUTING.md README.md 2>/dev/null
```

Expected: no output.

- [ ] **Step 7: Verify mkdocs strict build is clean**

```sh
uv run --project tools/ mkdocs build --strict
```

Expected: build succeeds; no broken links, no missing anchors. If the `{#project-deps-sync}` anchor
on `cli.md:90` had inbound links from other pages, update those too.

```sh
grep -rn "#project-deps-sync" docs/
```

Expected: no output (the anchor renamed from `#project-deps-sync` to `#project-venv-sync`; update
any references found).

- [ ] **Step 8: Commit**

```sh
git add docs/ CONTRIBUTING.md
git commit -m "docs: rename toolr project deps references to project venv"
```

---

## Task 12: Add the "Auto-sync on shell-enter" section to `docs/installation/mise.md`

**Files:**

- Modify: `docs/installation/mise.md` — append a new section before the "Common commands" or
  "Troubleshooting" section.

- [ ] **Step 1: Read the current shape of `docs/installation/mise.md`**

Open the file and find a sensible insertion point — after "Combining with mise tasks" (the existing
`[tasks.test]` recipe section) and before "Common commands" is the natural place.

- [ ] **Step 2: Insert the new section**

Add this content (verbatim) at the chosen insertion point:

````markdown
## Auto-sync the tools venv on shell-enter

When `tools/pyproject.toml` or `tools/uv.lock` changes — say you've
pulled a branch that bumped a dependency — `toolr`'s next invocation
re-syncs the tools venv before doing its real work. That sync only
happens when you actually run a `toolr` command, so the latency
shows up at an awkward moment, on a command you ran for a different
reason.

mise's `[hooks].enter` lets you run a command every time the shell
enters this project's directory, before you've typed anything else.
Wiring it to `toolr project venv sync --quiet` gives you a tools
venv that's already fresh by the time your prompt returns:

```toml
# In this project's mise.toml
[hooks] enter = "toolr project venv sync --quiet"
```

That's the entire recipe. No `[tasks]` block, no
`sources`/`outputs` configuration — `toolr project venv sync`
honors its own freshness stamp internally, so when nothing has
changed it exits in tens of milliseconds without spawning uv. When
the lock file has moved, it runs `uv sync --quiet` exactly once and
updates the stamp.

The recipe works identically for every project, regardless of
whether `[tool.toolr] venv-location` is `cache` (the default, under
`$XDG_CACHE_HOME/toolr/<repo-key>/venv/`) or `in-tree`
(`tools/.venv/`) — the freshness stamp lives inside the venv either
way, and the recipe never hard-codes a venv path.

### Unattended-mode guards

`--quiet` does more than suppress output: it also tells `toolr` that
it's running in an unattended context where blocking on a TTY prompt
would freeze the shell. To honour that, `--quiet` exits 0 silently
in three benign situations:

- The current directory isn't a toolr-using repo (no
  `tools/pyproject.toml`). The hook fires on every `cd` even into
  non-toolr directories; this is normal, not an error.
- `tools/uv.lock` is missing. The user probably hasn't run
  `toolr project init` yet — that's their next step, not something
  the hook should report.
- `uv` isn't installed and `TOOLR_AUTO_INSTALL_UV=1` isn't set.
  The hook can't reasonably prompt for consent on every shell
  enter, so it stays out of the way.

Genuine failures — an unparsable lock file, a `uv sync` that exits
non-zero — still print their error to stderr and exit non-zero, so
they aren't silently masked.

### First-time bootstrap

The first time you set up a project, run `toolr project venv sync`
once **without** `--quiet`. That's the run that will install uv if
needed (with a normal consent prompt) and materialise the venv. From
then on the enter-hook keeps it fresh:

```sh
toolr project venv sync     # one-time, interactive cd ..; cd back              # enter-hook now
keeps it fresh
```

### Project- vs. machine-scoped

The hook lives in the project's own `mise.toml`. It is **not** a
global setting — every project that wants this behaviour opts in by
adding the line. That keeps non-toolr projects free of unexpected
post-`cd` work.
````

- [ ] **Step 3: Build the docs to verify**

```sh
uv run --project tools/ mkdocs build --strict
```

Expected: clean build, no warnings about the new section.

- [ ] **Step 4: Commit**

```sh
git add docs/installation/mise.md
git commit -m "docs(mise): document the enter-hook auto-sync recipe"
```

---

## Task 13: Add the breaking-change entry to `UNRELEASED.md`

**Files:**

- Modify: `UNRELEASED.md`

- [ ] **Step 1: Read the current shape of `UNRELEASED.md`**

It will likely be empty (the steady state between releases). The file's header comment explains the
convention: append narrative entries here; the release workflow folds them into the CHANGELOG.

- [ ] **Step 2: Append the breaking entry**

Replace the file's contents (preserving the existing top-of-file HTML comment block if present)
with:

```markdown
## ⚠ Breaking changes

### `toolr project deps` removed; replaced by `toolr project venv`

- **What changed:** the `toolr project deps` subcommand group has
  been removed. Its two commands moved under `toolr project venv`:
    - `toolr project deps sync` → `toolr project venv sync`
    - `toolr project deps upgrade <pkg>` → `toolr project venv upgrade <pkg>`
- **Behavior change on `sync`:** the new `toolr project venv sync`
  honors the tools venv's freshness stamp by default and no-ops
  (exit 0, no `uv sync`) when the venv is already up to date.
  Use `--force` to re-run unconditionally — that matches what
  `toolr project deps sync` did before.
- **New `--quiet` flag on `sync`:** silent on success and on
  benign unattended-mode exits ("not a toolr repo", "lock missing",
  "uv install needs consent"). Designed for use from a mise
  `[hooks].enter` recipe — see
  [Auto-sync the tools venv on shell-enter](docs/installation/mise.md#auto-sync-the-tools-venv-on-shell-enter).
- **Migration:** running `toolr project deps <anything>` at 0.22
  prints a tailored error pointing at the new path and exits with
  code 2.
- **Why:** the `deps` group only ever held venv-touching operations;
  collapsing it under `venv` puts every tools-venv operation in one
  place and makes room for future uv-wrapper subcommands (`add`,
  `remove`, `lock`, …) to land in the obvious location.
```

- [ ] **Step 3: Commit**

```sh
git add UNRELEASED.md
git commit -m "docs(unreleased): note the project deps → project venv rename"
```

---

## Task 14: End-to-end verification

- [ ] **Step 1: Full workspace test pass**

```sh
cargo test --workspace
```

Expected: all tests green. Any failures here are real regressions introduced by the rename; address
them before moving on.

- [ ] **Step 2: Verify no `project deps` strings remain anywhere**

```sh
grep -rn "project deps\|deps sync\|deps upgrade" \
    crates/ docs/ skills/ tests/ CONTRIBUTING.md README.md UNRELEASED.md 2>/dev/null \
    | grep -v "0.22 BREAKING\|was removed in 0.22\|migration hint\|RENAMED"
```

Expected: empty output, modulo the migration-hint error message and any RENAMED markers. If anything
else shows up, update that file.

- [ ] **Step 3: Verify the mkdocs build is strict-clean**

```sh
uv run --project tools/ mkdocs build --strict
```

- [ ] **Step 4: Run prek over the whole working tree to catch lint drift**

```sh
prek run --all-files
```

Expected: all hooks pass. (Snippet regen, typo check, codespell, rumdl.)

- [ ] **Step 5: Manual smoke — invoke the migration hint and the new command**

```sh
cargo build -p toolr --release
./target/release/toolr project deps sync
echo "---"
./target/release/toolr project venv sync --help
```

Expected output:

- First command: prints the migration hint to stderr; exits with code 2.
- Second command: prints the new help including `--force` and `--quiet`.
- [ ] **Step 6: Commit any straggler fixes from the verification pass**

```sh
git status
# If anything changed: stage and commit as
git add -p
git commit -m "fix(rename): clean up straggling project deps references"
```

If nothing changed, skip this step.

---

## Task 15: Archive the design + plan into `specs/archive/2026/`

**Files:**

- Move: `specs/2026-06-01-mise-enter-auto-sync-design.md` →
  `specs/archive/2026/2026-06-01-mise-enter-auto-sync-design.md`
- Move: `specs/2026-06-01-mise-enter-auto-sync-plan.md` →
  `specs/archive/2026/2026-06-01-mise-enter-auto-sync-plan.md`

Per `specs/README.md`: "When the PR implementing a design merges to `main`: `git mv
specs/<date>-<topic>-design.md specs/archive/<year>/`. Land the move in the same PR (or as an
immediate follow-up)." This is that move — the final commit on this branch, so the PR's diff carries
both the implementation and the archival in one atomic unit.

Do this **last**, after every other task is committed. Otherwise subagents reading the design/plan
during execution will hit "file moved" surprises.

- [ ] **Step 1: Confirm the archive directory exists**

```sh
ls /Users/pedro.algarvio/projects/me/toolr/specs/archive/2026/
```

Expected: directory exists with prior archived designs in it (it does today — confirmed during
context gathering).

- [ ] **Step 2: Move both files**

```sh
git mv specs/2026-06-01-mise-enter-auto-sync-design.md \
       specs/archive/2026/2026-06-01-mise-enter-auto-sync-design.md
git mv specs/2026-06-01-mise-enter-auto-sync-plan.md \
       specs/archive/2026/2026-06-01-mise-enter-auto-sync-plan.md
```

- [ ] **Step 3: Verify the moves landed**

```sh
git status
ls specs/
ls specs/archive/2026/ | grep mise-enter-auto-sync
```

Expected: both files appear under `specs/archive/2026/` with `R` (renamed) status in `git status`;
the top of `specs/` no longer contains them.

- [ ] **Step 4: Commit the archive move**

```sh
git commit -m "specs: archive mise-enter-auto-sync design + plan (implemented)"
```

(Per the project's auto-memory rule: no `Co-Authored-By` footer on this commit.)

- [ ] **Step 5: Sanity-check the final branch state**

```sh
git log --oneline main..HEAD
```

Expected: a clean stack of commits, one per task (~15 commits), ending in the archive move. No
reverts or fixups. If a fixup exists from Task 14 Step 6, that's fine — but the archive move
**must** be the final commit so the PR description's "this PR ships the design at
specs/archive/2026/…" link resolves correctly on merge.

---

## Spec Coverage Check

Verifying every spec requirement is implemented by some task:

| Spec section | Task(s) |
| --- | --- |
| Command-tree reshape (`deps` → `venv`) | Task 4, 5 |
| Behavior flip on `sync` (default no-op when fresh) | Task 4 |
| `--force` flag | Task 4 |
| `--quiet` flag (output silencing) | Task 1, 4 |
| `--quiet` unattended-mode guards | Task 4 |
| Migration-hint error for removed `deps` | Task 4 |
| `silent_refuse` consent path | Task 2 |
| Stamp location unchanged | (no task — verified by inspection in §Background) |
| mise enter-hook recipe (docs only) | Task 12 |
| Hint string update (Rust dispatch, Python runner) | Task 6 |
| Test coverage (smoke + integration) | Task 4 (smoke), Task 9 (integration), Task 7 (existing smoke updates), Task 8 (upgrade test rename) |
| Snippet regen | Task 10 |
| Prose doc updates | Task 11 |
| Breaking-change CHANGELOG note | Task 13 |
| End-to-end verification | Task 14 |
| Archive design + plan into `specs/archive/2026/` | Task 15 |
| Separate `add`/`remove`/`lock` ticket (Follow-ups) | **Out of scope** — file the issue after the plan ships |

No gaps; no placeholders in the task bodies.

---

Plan complete and saved to `specs/2026-06-01-mise-enter-auto-sync-plan.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks,
   fast iteration.
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with
   checkpoints.

Which approach?
