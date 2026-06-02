# venv ↔ uv parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
  superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `toolr project venv upgrade <pkg>` with uv-style flags (`-U` / `--upgrade`, `-P` /
`--upgrade-package`) on a new `venv lock` and the existing `venv sync`, and add `venv add` / `venv remove` so the
wrapper covers the full uv project workflow except `pip`.

**Architecture:** Single Rust workspace edit, stacked branch `venv-uv-parity`
on top of `mise-enter-auto-sync` (PR #289). The toolr-core layer grows an
`UpgradeMode` enum and an `edit` module for add/remove. The toolr CLI gains
three new clap subcommands (`lock`, `add`, `remove`), extends `sync` with two
new flags, and deletes `upgrade`. Pre-flight pyproject guards from
`venv_upgrade` are kept and reused.

**Tech Stack:** Rust workspace · `clap` v4 derive-builder · `anyhow` · `assert_cmd` + `tempfile` for integration tests ·
`toml` crate for pyproject inspection · stub `#!/bin/sh` uv binary for argv capture in tests (Unix-only).

**Spec:** `specs/2026-06-02-venv-uv-parity-design.md`.

**Branch:** `venv-uv-parity` (already created, stacked on `mise-enter-auto-sync`).

**Memory notes acknowledged:**

- No `Co-Authored-By` trailer on commits ([feedback_no_coauthor_footer.md]).
- Monitor long cargo runs ([feedback_monitor_cargo_test.md]) — use `cargo test -p <crate>` per task; only run `cargo
  test --workspace` at the end of a logical batch.

---

## File structure overview

**Modified:**

- `crates/toolr-core/src/venv/sync.rs` — add `UpgradeMode` enum, replace `run_uv_lock_upgrade` with `run_uv_lock`,
  extend `run_uv_sync` + `sync_if_needed` to thread the new enum.
- `crates/toolr-core/src/venv/mod.rs` — re-export `UpgradeMode` + new symbols; drop `run_uv_lock_upgrade`.
- `crates/toolr-core/src/project.rs` — extend `EnsureOpts` with `upgrade: UpgradeMode`, thread through
  `ensure_venv_ready`.
- `crates/toolr/src/cli.rs` — add `lock`/`add`/`remove` subcommands; extend `sync` with `-U`/`-P`; delete `upgrade`.
- `crates/toolr/src/project.rs` — add `venv_lock` / `venv_add` / `venv_remove` handlers; extend `venv_sync` with
  upgrade-flag parsing; delete `venv_upgrade`; update `deps_migration_hint`.
- `crates/toolr/src/builtin_completions.rs` — rename and update the project-venv completions test.
- `crates/toolr/tests/project_venv_sync.rs` — add `-U` / `-P` argv-capture cases.

**Created:**

- `crates/toolr-core/src/venv/edit.rs` — new module with `run_uv_add` + `run_uv_remove` (+ unit tests).
- `crates/toolr/tests/project_venv_lock.rs` — integration tests for `venv lock`.
- `crates/toolr/tests/project_venv_add.rs` — integration tests for `venv add`.
- `crates/toolr/tests/project_venv_remove.rs` — integration tests for `venv remove`.

**Deleted:**

- `crates/toolr/tests/project_venv_upgrade.rs` — superseded by extended `project_venv_sync.rs`.

---

## Task 1: Introduce `UpgradeMode` in `toolr-core`

**Files:**

- Modify: `crates/toolr-core/src/venv/sync.rs`
- Modify: `crates/toolr-core/src/venv/mod.rs`

This is a pure-additive type introduction with no behavior change. Existing `run_uv_sync` / `run_uv_lock_upgrade` stay
untouched in this task.

- [ ] **Step 1: Write the failing unit test**

Append inside the existing `#[cfg(test)] mod tests` block in `crates/toolr-core/src/venv/sync.rs`:

```rust
#[test]
fn upgrade_mode_default_is_none() {
    assert!(matches!(UpgradeMode::default(), UpgradeMode::None));
}

#[test]
fn upgrade_mode_packages_preserves_order() {
    let m = UpgradeMode::Packages(vec!["foo".into(), "bar".into()]);
    if let UpgradeMode::Packages(p) = m {
        assert_eq!(p, vec!["foo".to_string(), "bar".to_string()]);
    } else {
        panic!("expected Packages variant");
    }
}
```

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cargo test -p toolr-core venv::sync::tests::upgrade_mode -- --nocapture`
Expected: compile error — `UpgradeMode` is not in scope.

- [ ] **Step 3: Add the enum**

In `crates/toolr-core/src/venv/sync.rs`, between the `Freshness` enum and `check_freshness`:

```rust
/// Argument shape for `uv lock` / `uv sync` upgrade behavior.
///
/// Mirrors uv's two flags exactly:
/// - `--upgrade` / `-U` → re-resolve every package (`All`).
/// - `--upgrade-package <pkg>` / `-P <pkg>` (repeatable) → re-resolve
///   the listed packages (`Packages`).
///
/// `None` is the default and produces no extra args.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum UpgradeMode {
    #[default]
    None,
    All,
    Packages(Vec<String>),
}

impl UpgradeMode {
    /// Append the matching uv argv tokens (`--upgrade` / repeated
    /// `--upgrade-package <pkg>`) onto `cmd`. No-op for `None`.
    pub(crate) fn append_args(&self, cmd: &mut std::process::Command) {
        match self {
            UpgradeMode::None => {}
            UpgradeMode::All => {
                cmd.arg("--upgrade");
            }
            UpgradeMode::Packages(pkgs) => {
                for p in pkgs {
                    cmd.arg("--upgrade-package").arg(p);
                }
            }
        }
    }
}
```

- [ ] **Step 4: Re-export from `venv/mod.rs`**

Edit `crates/toolr-core/src/venv/mod.rs` line 14:

```rust
pub use sync::{Freshness, UpgradeMode, check_freshness, run_uv_lock_upgrade, run_uv_sync, sync_if_needed};
```

- [ ] **Step 5: Run unit tests**

Run: `cargo test -p toolr-core venv::sync::tests::upgrade_mode -- --nocapture`
Expected: 2 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-core/src/venv/sync.rs crates/toolr-core/src/venv/mod.rs
git commit -m "venv-core: introduce UpgradeMode enum"
```

---

## Task 2: Extend `run_uv_sync` to take `UpgradeMode`

**Files:**

- Modify: `crates/toolr-core/src/venv/sync.rs`
- Modify: `crates/toolr-core/src/project.rs` (caller in `ensure_venv_ready`)

Change `run_uv_sync` signature to add `upgrade: &UpgradeMode` and update every caller. Pin the new behavior with a unit
test before flipping the signature.

- [ ] **Step 1: Write the failing unit test**

Add inside the `#[cfg(test)] mod tests` block in `crates/toolr-core/src/venv/sync.rs`, after the existing
`run_uv_sync_passes_python_flag_when_config_pins_version` test:

```rust
#[cfg(unix)]
#[test]
fn run_uv_sync_passes_upgrade_flag_for_all_mode() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let argdump = tmp.path().join("argdump");
    let stub = tmp.path().join("uv");
    let mut f = fs::File::create(&stub).unwrap();
    writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
    drop(f);
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let uv = UvBinary {
        path: stub,
        version: (0, 4, 0),
        source: crate::uv::UvSource::Path,
    };

    let venv = tmp.path().join("venv");
    fs::create_dir_all(&venv).unwrap();
    let resolved = dummy_resolved(venv);

    run_uv_sync(&uv, tmp.path(), &resolved, &UpgradeMode::All, /*quiet=*/ false)
        .expect("stub uv should succeed");

    let dump = fs::read_to_string(&argdump).unwrap();
    assert!(dump.contains("sync"));
    assert!(dump.lines().any(|l| l == "--upgrade"));
}

#[cfg(unix)]
#[test]
fn run_uv_sync_passes_upgrade_package_flag_for_packages_mode() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let argdump = tmp.path().join("argdump");
    let stub = tmp.path().join("uv");
    let mut f = fs::File::create(&stub).unwrap();
    writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
    drop(f);
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let uv = UvBinary {
        path: stub,
        version: (0, 4, 0),
        source: crate::uv::UvSource::Path,
    };

    let venv = tmp.path().join("venv");
    fs::create_dir_all(&venv).unwrap();
    let resolved = dummy_resolved(venv);

    let mode = UpgradeMode::Packages(vec!["foo".into(), "bar".into()]);
    run_uv_sync(&uv, tmp.path(), &resolved, &mode, /*quiet=*/ false)
        .expect("stub uv should succeed");

    let dump = fs::read_to_string(&argdump).unwrap();
    let mut iter = dump.lines();
    // We expect: ... --upgrade-package foo ... --upgrade-package bar ...
    let mut saw_foo = false;
    let mut saw_bar = false;
    while let Some(line) = iter.next() {
        if line == "--upgrade-package" {
            match iter.next() {
                Some("foo") => saw_foo = true,
                Some("bar") => saw_bar = true,
                _ => {}
            }
        }
    }
    assert!(saw_foo && saw_bar, "expected both packages in argv: {dump}");
}
```

- [ ] **Step 2: Run test to verify it fails to compile**

Run: `cargo test -p toolr-core venv::sync::tests::run_uv_sync_passes_upgrade -- --nocapture`
Expected: compile error — `run_uv_sync` signature mismatch.

- [ ] **Step 3: Change `run_uv_sync` to take `upgrade`**

Edit `run_uv_sync` in `crates/toolr-core/src/venv/sync.rs` (currently around L48):

```rust
pub fn run_uv_sync(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    upgrade: &UpgradeMode,
    quiet: bool,
) -> Result<ExitStatus> {
    if let Some(parent) = resolved.venv_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut cmd = Command::new(&uv.path);
    cmd.arg("sync")
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir)
        .env_remove("VIRTUAL_ENV");
    if quiet {
        cmd.arg("--quiet");
    }
    if let Some(version) = resolved.config.python_version.as_ref() {
        cmd.arg("--python").arg(version);
    }
    upgrade.append_args(&mut cmd);
    let status = cmd
        .status()
        .with_context(|| format!("spawning uv at {}", uv.path.display()))?;
    if status.success() {
        touch_marker(&resolved.venv_dir)?;
    }
    Ok(status)
}
```

- [ ] **Step 4: Update `sync_if_needed`'s signature and short-circuit logic**

Edit `sync_if_needed` (currently around L108):

```rust
pub fn sync_if_needed(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    force: bool,
    quiet: bool,
    upgrade: &UpgradeMode,
) -> Result<(), UvError> {
    // -U / -P explicitly ask for movement; never short-circuit on freshness.
    let bypass_stamp = force || !matches!(upgrade, UpgradeMode::None);
    if !bypass_stamp && matches!(check_freshness(resolved, tools_dir), Freshness::Fresh) {
        return Ok(());
    }
    let status = run_uv_sync(uv, tools_dir, resolved, upgrade, quiet)
        .map_err(|e| UvError::Http(e.to_string()))?;
    if !status.success() {
        return Err(UvError::SyncFailed(status.code()));
    }
    Ok(())
}
```

- [ ] **Step 5: Update existing unit-test call sites in `sync.rs`**

Search for `run_uv_sync(&uv,` and `sync_if_needed(&uv,` in the test module and add `&UpgradeMode::None,` in the right
position. Affected tests (current call shapes):

- `sync_if_needed_skips_run_when_fresh_and_force_off`: `sync_if_needed(&uv, tmp.path(), &resolved, false, false)` →
  `sync_if_needed(&uv, tmp.path(), &resolved, false, false, &UpgradeMode::None)`.
- `sync_if_needed_invokes_uv_when_force_set_even_if_fresh`: append `&UpgradeMode::None`.
- `sync_if_needed_propagates_nonzero_exit_as_sync_failed`: append `&UpgradeMode::None`.
- `sync_if_needed_translates_spawn_failure_to_uv_error`: append `&UpgradeMode::None`.
- `run_uv_sync_passes_quiet_when_requested`: change to `run_uv_sync(&uv, tmp.path(), &resolved, &UpgradeMode::None,
  /*quiet=*/ true)`.
- `run_uv_sync_omits_quiet_by_default`: same shape with `&UpgradeMode::None`.
- `run_uv_sync_passes_python_flag_when_config_pins_version`: same shape with `&UpgradeMode::None`.
- [ ] **Step 6: Update `ensure_venv_ready` to pass `&UpgradeMode::None`**

In `crates/toolr-core/src/project.rs` around L50:

```rust
    sync_if_needed(&uv, &tools, &resolved, opts.force_sync, opts.quiet, &crate::venv::UpgradeMode::None)
        .with_context(|| format!("uv sync against {}", tools.display()))?;
```

(We'll wire the real upgrade plumbing through `EnsureOpts` in Task 5.)

- [ ] **Step 7: Run the toolr-core test suite**

Run: `cargo test -p toolr-core`
Expected: all tests pass — including the two new ones from Step 1.

- [ ] **Step 8: Commit**

```bash
git add crates/toolr-core/src/venv/sync.rs crates/toolr-core/src/project.rs
git commit -m "venv-core: thread UpgradeMode through run_uv_sync + sync_if_needed"
```

---

## Task 3: Replace `run_uv_lock_upgrade` with `run_uv_lock`

**Files:**

- Modify: `crates/toolr-core/src/venv/sync.rs`
- Modify: `crates/toolr-core/src/venv/mod.rs`

`run_uv_lock_upgrade` is the old single-package wrapper. Replace it with a general `run_uv_lock` that drives `uv lock`
and accepts `UpgradeMode`. No callers other than `venv_upgrade` (which we'll delete in Task 9) — safe to remove the old
function in the same task.

- [ ] **Step 1: Write the failing unit test**

Append in the `#[cfg(test)] mod tests` block:

```rust
#[cfg(unix)]
#[test]
fn run_uv_lock_with_none_mode_calls_uv_lock_with_no_upgrade_args() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let argdump = tmp.path().join("argdump");
    let stub = tmp.path().join("uv");
    let mut f = fs::File::create(&stub).unwrap();
    writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
    drop(f);
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let uv = UvBinary {
        path: stub,
        version: (0, 4, 0),
        source: crate::uv::UvSource::Path,
    };
    let venv = tmp.path().join("venv");
    let resolved = dummy_resolved(venv);

    run_uv_lock(&uv, tmp.path(), &resolved, &UpgradeMode::None, /*quiet=*/ false)
        .expect("stub uv should succeed");

    let dump = fs::read_to_string(&argdump).unwrap();
    assert!(dump.contains("lock"), "args: {dump}");
    assert!(dump.contains("--project"), "args: {dump}");
    assert!(!dump.contains("--upgrade"), "args should not contain --upgrade: {dump}");
}

#[cfg(unix)]
#[test]
fn run_uv_lock_with_all_mode_passes_upgrade() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let argdump = tmp.path().join("argdump");
    let stub = tmp.path().join("uv");
    let mut f = fs::File::create(&stub).unwrap();
    writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
    drop(f);
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let uv = UvBinary {
        path: stub,
        version: (0, 4, 0),
        source: crate::uv::UvSource::Path,
    };
    let venv = tmp.path().join("venv");
    let resolved = dummy_resolved(venv);

    run_uv_lock(&uv, tmp.path(), &resolved, &UpgradeMode::All, /*quiet=*/ false)
        .expect("stub uv should succeed");

    let dump = fs::read_to_string(&argdump).unwrap();
    assert!(dump.lines().any(|l| l == "--upgrade"), "args: {dump}");
}

#[cfg(unix)]
#[test]
fn run_uv_lock_with_packages_mode_passes_each_upgrade_package() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let argdump = tmp.path().join("argdump");
    let stub = tmp.path().join("uv");
    let mut f = fs::File::create(&stub).unwrap();
    writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}", argdump.display()).unwrap();
    drop(f);
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    let uv = UvBinary {
        path: stub,
        version: (0, 4, 0),
        source: crate::uv::UvSource::Path,
    };
    let venv = tmp.path().join("venv");
    let resolved = dummy_resolved(venv);

    let mode = UpgradeMode::Packages(vec!["foo".into(), "bar".into()]);
    run_uv_lock(&uv, tmp.path(), &resolved, &mode, /*quiet=*/ false)
        .expect("stub uv should succeed");

    let dump = fs::read_to_string(&argdump).unwrap();
    let occurrences = dump.lines().filter(|l| *l == "--upgrade-package").count();
    assert_eq!(occurrences, 2, "expected 2 --upgrade-package tokens: {dump}");
    assert!(dump.contains("foo"));
    assert!(dump.contains("bar"));
}
```

- [ ] **Step 2: Run to verify the new tests fail to compile**

Run: `cargo test -p toolr-core venv::sync::tests::run_uv_lock -- --nocapture`
Expected: compile error — `run_uv_lock` does not exist.

- [ ] **Step 3: Replace `run_uv_lock_upgrade` with `run_uv_lock`**

In `crates/toolr-core/src/venv/sync.rs`, delete the `run_uv_lock_upgrade` function and the
`run_uv_lock_upgrade_passes_package_and_project_args` test, and add:

```rust
/// Run `uv lock --project <tools>` synchronously, inheriting stdio.
/// Used by `toolr project venv lock` to refresh `tools/uv.lock` without
/// applying the new pins to the venv. The `upgrade` arg controls
/// whether `uv` is asked to re-resolve some or all dependencies; see
/// [`UpgradeMode`].
pub fn run_uv_lock(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    upgrade: &UpgradeMode,
    quiet: bool,
) -> Result<ExitStatus> {
    let mut cmd = Command::new(&uv.path);
    cmd.arg("lock")
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir)
        .env_remove("VIRTUAL_ENV");
    if quiet {
        cmd.arg("--quiet");
    }
    if let Some(version) = resolved.config.python_version.as_ref() {
        cmd.arg("--python").arg(version);
    }
    upgrade.append_args(&mut cmd);
    cmd.status()
        .with_context(|| format!("spawning uv at {}", uv.path.display()))
}
```

- [ ] **Step 4: Update `venv/mod.rs` re-exports**

Edit `crates/toolr-core/src/venv/mod.rs` line 14:

```rust
pub use sync::{Freshness, UpgradeMode, check_freshness, run_uv_lock, run_uv_sync, sync_if_needed};
```

(Removed `run_uv_lock_upgrade`; added `run_uv_lock`.)

- [ ] **Step 5: Run the toolr-core test suite**

Run: `cargo test -p toolr-core`
Expected: passes. The previous `run_uv_lock_upgrade_passes_package_and_project_args` test is gone; the three new
`run_uv_lock_*` tests cover its territory.

- [ ] **Step 6: Verify the workspace still builds (`venv_upgrade` will be broken — that's expected)**

Run: `cargo check -p toolr-core`
Expected: pass. (We expect `cargo check -p toolr` to fail because `venv_upgrade` references the removed
`run_uv_lock_upgrade` — leave it broken; Task 9 fixes it.)

- [ ] **Step 7: Commit**

```bash
git add crates/toolr-core/src/venv/sync.rs crates/toolr-core/src/venv/mod.rs
git commit -m "venv-core: replace run_uv_lock_upgrade with general run_uv_lock"
```

---

## Task 4: Add `edit.rs` with `run_uv_add` + `run_uv_remove`

**Files:**

- Create: `crates/toolr-core/src/venv/edit.rs`
- Modify: `crates/toolr-core/src/venv/mod.rs`
- [ ] **Step 1: Write the failing module-level tests**

Create `crates/toolr-core/src/venv/edit.rs` with this content (it'll fail to compile because `run_uv_add` /
`run_uv_remove` don't exist yet — that's the failing test):

```rust
//! Drive `uv add` / `uv remove` against the tools venv. These commands
//! edit `tools/pyproject.toml` and internally run `uv lock` + `uv sync`,
//! so on success the venv reflects the new state.

use std::path::Path;
use std::process::{Command, ExitStatus};

use anyhow::{Context, Result};

use crate::uv::UvBinary;

use super::resolve::ResolvedVenv;
use super::sync::touch_marker_after_success;

/// Run `uv add <specs...> --project <tools>` synchronously. uv mutates
/// `tools/pyproject.toml`, refreshes `tools/uv.lock`, and re-syncs the
/// environment in one call.
pub fn run_uv_add(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    specs: &[String],
    quiet: bool,
) -> Result<ExitStatus> {
    let mut cmd = Command::new(&uv.path);
    cmd.arg("add")
        .args(specs)
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir)
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
        touch_marker_after_success(&resolved.venv_dir)?;
    }
    Ok(status)
}

/// Run `uv remove <packages...> --project <tools>` synchronously. Same
/// shape as [`run_uv_add`]; uv drops the listed entries from pyproject
/// and re-syncs.
pub fn run_uv_remove(
    uv: &UvBinary,
    tools_dir: &Path,
    resolved: &ResolvedVenv,
    packages: &[String],
    quiet: bool,
) -> Result<ExitStatus> {
    let mut cmd = Command::new(&uv.path);
    cmd.arg("remove")
        .args(packages)
        .arg("--project")
        .arg(tools_dir)
        .env("UV_PROJECT_ENVIRONMENT", &resolved.venv_dir)
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
        touch_marker_after_success(&resolved.venv_dir)?;
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uv::UvSource;
    use crate::venv::config::ToolrConfig;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn dummy_resolved(venv_dir: PathBuf) -> ResolvedVenv {
        ResolvedVenv {
            venv_dir: venv_dir.clone(),
            python: venv_dir.join("bin").join("python"),
            repo_key: "x".into(),
            python_version: "3.13".into(),
            config: ToolrConfig::default(),
        }
    }

    #[cfg(unix)]
    fn stub_uv(tmp: &Path, argdump: &Path) -> UvBinary {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let stub = tmp.join("uv");
        let mut f = fs::File::create(&stub).unwrap();
        writeln!(f, "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0", argdump.display()).unwrap();
        drop(f);
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();
        UvBinary { path: stub, version: (0, 4, 0), source: UvSource::Path }
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_add_passes_specs_and_project() {
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let uv = stub_uv(tmp.path(), &argdump);
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        let specs = vec!["httpx".to_string(), "rich@13.7".to_string()];
        run_uv_add(&uv, tmp.path(), &resolved, &specs, /*quiet=*/ false)
            .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.lines().any(|l| l == "add"), "args: {dump}");
        assert!(dump.contains("httpx"), "args: {dump}");
        assert!(dump.contains("rich@13.7"), "args: {dump}");
        assert!(dump.contains("--project"), "args: {dump}");
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_remove_passes_packages_and_project() {
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let uv = stub_uv(tmp.path(), &argdump);
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        let pkgs = vec!["httpx".to_string()];
        run_uv_remove(&uv, tmp.path(), &resolved, &pkgs, /*quiet=*/ false)
            .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.lines().any(|l| l == "remove"), "args: {dump}");
        assert!(dump.contains("httpx"), "args: {dump}");
        assert!(dump.contains("--project"), "args: {dump}");
    }

    #[cfg(unix)]
    #[test]
    fn run_uv_add_propagates_quiet() {
        let tmp = TempDir::new().unwrap();
        let argdump = tmp.path().join("argdump");
        let uv = stub_uv(tmp.path(), &argdump);
        let venv = tmp.path().join("venv");
        fs::create_dir_all(&venv).unwrap();
        let resolved = dummy_resolved(venv);

        run_uv_add(&uv, tmp.path(), &resolved, &["foo".to_string()], /*quiet=*/ true)
            .expect("stub uv should succeed");

        let dump = fs::read_to_string(&argdump).unwrap();
        assert!(dump.lines().any(|l| l == "--quiet"), "args: {dump}");
    }
}
```

- [ ] **Step 2: Expose `touch_marker` for the new module**

In `crates/toolr-core/src/venv/sync.rs`, find `fn touch_marker(venv_dir: &Path) -> Result<()>` and add a sibling
pub(super) re-export at the bottom of the file (so `edit.rs` can use it without making `touch_marker` itself public):

```rust
pub(super) fn touch_marker_after_success(venv_dir: &Path) -> Result<()> {
    touch_marker(venv_dir)
}
```

- [ ] **Step 3: Register the new module in `mod.rs`**

Edit `crates/toolr-core/src/venv/mod.rs`:

```rust
pub mod config;
pub mod editable;
pub mod edit;
pub mod repo_key;
pub mod resolve;
pub mod sync;
pub mod validate;

pub use config::{ToolrConfig, VenvLocation, load_toolr_config};
pub use edit::{run_uv_add, run_uv_remove};
pub use editable::{EditableOutcome, perform_editable_installs, warn_failures};
pub use repo_key::{TOOLR_MAJOR, compute_repo_key};
pub use resolve::{ResolvedVenv, resolve_venv_path};
pub use sync::{Freshness, UpgradeMode, check_freshness, run_uv_lock, run_uv_sync, sync_if_needed};
pub use validate::{ValidationError, locate_toolr_package, validate_venv};
```

- [ ] **Step 4: Run the new unit tests**

Run: `cargo test -p toolr-core venv::edit`
Expected: 3 passed.

- [ ] **Step 5: Run all of toolr-core**

Run: `cargo test -p toolr-core`
Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-core/src/venv/edit.rs crates/toolr-core/src/venv/sync.rs crates/toolr-core/src/venv/mod.rs
git commit -m "venv-core: add edit module with run_uv_add + run_uv_remove"
```

---

## Task 5: Carry `UpgradeMode` through `EnsureOpts`

**Files:**

- Modify: `crates/toolr-core/src/project.rs`

`venv sync -U/-P` needs to flow `UpgradeMode` from the CLI handler down to `sync_if_needed`. Wire it through
`EnsureOpts` so callers compose it the same way they compose `force_sync` / `quiet`.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in `crates/toolr-core/src/project.rs`:

```rust
#[test]
fn ensure_opts_with_upgrade_sets_mode() {
    use crate::venv::UpgradeMode;
    let opts = EnsureOpts::default()
        .with_upgrade(UpgradeMode::Packages(vec!["foo".into()]));
    match opts.upgrade {
        UpgradeMode::Packages(ref p) => assert_eq!(p, &vec!["foo".to_string()]),
        other => panic!("expected Packages, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p toolr-core project::tests::ensure_opts_with_upgrade_sets_mode -- --nocapture`
Expected: compile error — `EnsureOpts` lacks `upgrade` and `with_upgrade`.

- [ ] **Step 3: Extend `EnsureOpts`**

Edit `crates/toolr-core/src/project.rs`:

```rust
use crate::venv::UpgradeMode;

#[derive(Debug, Clone, Default)]
pub struct EnsureOpts {
    /// Run `uv sync` even when the freshness stamp says the venv is fresh.
    pub force_sync: bool,
    /// Forward `--quiet` to the uv subprocess.
    pub quiet: bool,
    /// Whether to pass `-U` / `-P` flags through to uv.
    pub upgrade: UpgradeMode,
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
    pub fn with_upgrade(mut self, mode: UpgradeMode) -> Self {
        self.upgrade = mode;
        self
    }
}
```

(Note: the `Copy` derive in the previous version goes away because `UpgradeMode::Packages` carries a `Vec`. Update the
struct derive line accordingly — `Default` and `Clone` are kept, `Copy` is dropped.)

- [ ] **Step 4: Update `ensure_venv_ready` to pass `opts.upgrade` to `sync_if_needed`**

Edit `ensure_venv_ready` in the same file:

```rust
sync_if_needed(&uv, &tools, &resolved, opts.force_sync, opts.quiet, &opts.upgrade)
    .with_context(|| format!("uv sync against {}", tools.display()))?;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p toolr-core`
Expected: pass — both the new test and all prior tests. If any test holds an `EnsureOpts` by value where `Copy` was
needed, switch the call site to `.clone()`.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-core/src/project.rs
git commit -m "venv-core: thread UpgradeMode through EnsureOpts"
```

---

## Task 6: Extend `venv sync` clap surface with `-U` / `-P`

**Files:**

- Modify: `crates/toolr/src/cli.rs`
- Modify: `crates/toolr/src/project.rs` (handler)

This is the first toolr-crate task. It also fixes the build, which has been red since Task 3 deleted
`run_uv_lock_upgrade`. We address that by simultaneously deleting `venv_upgrade` in Task 9; for now, keep
`venv_upgrade`'s clap subcommand registered but expect the handler to stay broken until Task 9 (or temporarily
comment-out the body — see Step 0 below).

- [ ] **Step 0: Stub out `venv_upgrade` so the crate compiles**

In `crates/toolr/src/project.rs`, replace the body of `venv_upgrade` with a placeholder that compiles but will be
deleted in Task 9. Replace from `let lock_status = toolr_core::venv::run_uv_lock_upgrade(...)` through the rest of the
function with:

```rust
fn venv_upgrade(_matches: &ArgMatches) -> Result<ExitCode> {
    // Placeholder: this subcommand is being removed in Task 9 of the
    // venv-uv-parity plan. See specs/2026-06-02-venv-uv-parity-plan.md.
    anyhow::bail!("`venv upgrade` is being replaced by `venv sync -U|-P`; this stub will be removed shortly")
}
```

Run: `cargo check -p toolr`
Expected: pass.

- [ ] **Step 1: Write the failing integration test**

Append to `crates/toolr/tests/project_venv_sync.rs`:

```rust
/// `venv sync --help` lists the new -U / -P flags.
#[test]
fn sync_help_lists_upgrade_flags() {
    let output = cargo_bin()
        .args(["project", "venv", "sync", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--upgrade"), "help missing --upgrade: {stdout}");
    assert!(stdout.contains("--upgrade-package"), "help missing --upgrade-package: {stdout}");
}

/// `venv sync -P` with an unknown package fails the pyproject pre-flight
/// guard the same way `venv upgrade` used to.
#[test]
fn sync_dash_p_errors_when_package_not_declared() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(
        tmp.path().join("tools/pyproject.toml"),
        r#"[project]
name = "tools"
version = "0.0.0"
requires-python = ">=3.11"
dependencies = [
    "requests",
]

[tool.toolr]
venv-location = "cache"
"#,
    )
    .unwrap();

    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "sync", "-P", "nonexistent-package"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not declared"),
        "expected validation error, stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("nonexistent-package"),
        "stderr should name the package, stderr was:\n{stderr}"
    );
}
```

- [ ] **Step 2: Run the test (expect failure)**

Run: `cargo test -p toolr --test project_venv_sync sync_help_lists_upgrade_flags
sync_dash_p_errors_when_package_not_declared`
Expected: failure — clap doesn't know `-P` / `--upgrade` yet.

- [ ] **Step 3: Add the flags to clap**

In `crates/toolr/src/cli.rs`, replace the existing `Command::new("sync")` block (lines 302–319 in the current file)
with:

```rust
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
    )
    .arg(
        Arg::new("upgrade")
            .long("upgrade")
            .short('U')
            .action(ArgAction::SetTrue)
            .help("Re-resolve every package (passes --upgrade to uv). Combine with -P to also force specific packages."),
    )
    .arg(
        Arg::new("upgrade-package")
            .long("upgrade-package")
            .short('P')
            .value_name("PACKAGE")
            .action(ArgAction::Append)
            .help("Re-resolve a single package; pass repeatedly for multiple. Each <PACKAGE> must be declared in tools/pyproject.toml."),
    ),
```

- [ ] **Step 4: Update the handler to read the new flags**

In `crates/toolr/src/project.rs`, replace `venv_sync` with:

```rust
fn venv_sync(matches: &ArgMatches) -> Result<ExitCode> {
    let force = matches.get_flag("force");
    let quiet = matches.get_flag("quiet");
    let upgrade_all = matches.get_flag("upgrade");
    let upgrade_pkgs: Vec<String> = matches
        .get_many::<String>("upgrade-package")
        .map(|vals| vals.cloned().collect())
        .unwrap_or_default();

    let upgrade = build_upgrade_mode(upgrade_all, upgrade_pkgs.clone());

    // Pre-flight pyproject guard for -P entries (matches the old
    // `venv upgrade` behavior). -U on its own skips the check because
    // uv re-locks everything.
    let cwd = std::env::current_dir()?;
    if !upgrade_pkgs.is_empty() {
        let repo_root = toolr_core::discovery::discover_project_root(&cwd)?;
        let pyproject = repo_root.join("tools/pyproject.toml");
        for pkg in &upgrade_pkgs {
            if !pyproject_declares_dependency(&pyproject, pkg)? {
                anyhow::bail!(
                    "package `{pkg}` is not declared in {} — add it to `[project] dependencies` first",
                    pyproject.display(),
                );
            }
        }
    }

    let mut consent = toolr_core::uv::install::ConsentMode::from_env();
    if quiet {
        consent.silent_refuse = true;
    }

    let opts = toolr_core::project::EnsureOpts::default()
        .with_force_sync(force)
        .with_quiet(quiet)
        .with_upgrade(upgrade);

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

/// Convert (`--upgrade`, `--upgrade-package ...`) flags into the
/// `UpgradeMode` the core layer wants. `All` wins if both are present —
/// uv accepts the combo, but the lock-side semantics are the same as
/// passing `-U` alone.
fn build_upgrade_mode(all: bool, pkgs: Vec<String>) -> toolr_core::venv::UpgradeMode {
    if all {
        return toolr_core::venv::UpgradeMode::All;
    }
    if pkgs.is_empty() {
        toolr_core::venv::UpgradeMode::None
    } else {
        toolr_core::venv::UpgradeMode::Packages(pkgs)
    }
}
```

(Note: if both `-U` and `-P` are passed, the spec said "do not add a guard" and "uv accepts both together." We coalesce
to `UpgradeMode::All` because that's the broader sweep — uv re-locks everything anyway. This keeps the core layer simple
and is observationally identical to passing both flags.)

- [ ] **Step 5: Run the new integration tests**

Run: `cargo test -p toolr --test project_venv_sync`
Expected: all tests pass, including the two new ones.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr/src/cli.rs crates/toolr/src/project.rs crates/toolr/tests/project_venv_sync.rs
git commit -m "venv sync: add -U / -P (uv upgrade flags) + pyproject guard for -P"
```

---

## Task 7: Add `venv lock` subcommand + handler

**Files:**

- Modify: `crates/toolr/src/cli.rs`
- Modify: `crates/toolr/src/project.rs`
- Create: `crates/toolr/tests/project_venv_lock.rs`
- [ ] **Step 1: Write the failing integration tests**

Create `crates/toolr/tests/project_venv_lock.rs`:

```rust
//! Integration tests for `toolr project venv lock`. Like the other
//! `project_venv_*` tests, these don't run real uv; they cover the
//! pre-flight validation and discovery paths.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_bin() -> Command {
    Command::cargo_bin("toolr").unwrap()
}

/// `--help` lists the new -U / -P flags.
#[test]
fn lock_help_lists_upgrade_flags() {
    let output = cargo_bin()
        .args(["project", "venv", "lock", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--upgrade"), "help missing --upgrade: {stdout}");
    assert!(stdout.contains("--upgrade-package"), "help missing --upgrade-package: {stdout}");
}

/// `venv lock` (no flags) reports the missing project root when run
/// outside a toolr-using directory.
#[test]
fn lock_errors_when_not_in_a_toolr_repo() {
    let tmp = TempDir::new().unwrap();
    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "lock"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("locating project root")
            || stderr.contains("resolving the tools venv path"),
        "stderr should explain the missing project, got:\n{stderr}"
    );
}

/// `venv lock -P <pkg>` runs the same pyproject pre-flight as
/// `venv sync -P <pkg>`.
#[test]
fn lock_dash_p_errors_when_package_not_declared() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(
        tmp.path().join("tools/pyproject.toml"),
        r#"[project]
name = "tools"
version = "0.0.0"
requires-python = ">=3.11"
dependencies = [
    "requests",
]

[tool.toolr]
venv-location = "cache"
"#,
    )
    .unwrap();

    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "lock", "-P", "nonexistent-package"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not declared"), "stderr: {stderr}");
    assert!(stderr.contains("nonexistent-package"), "stderr: {stderr}");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p toolr --test project_venv_lock`
Expected: failure — `lock` is not a known subcommand.

- [ ] **Step 3: Register the clap subcommand**

In `crates/toolr/src/cli.rs`, inside the `venv` `Command::new("venv")` chain (right after the `sync` subcommand block,
before the `upgrade` block), add:

```rust
.subcommand(
    Command::new("lock")
        .about("Refresh tools/uv.lock without applying (wraps `uv lock`)")
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .short('q')
                .action(ArgAction::SetTrue)
                .help("Pass --quiet to uv"),
        )
        .arg(
            Arg::new("upgrade")
                .long("upgrade")
                .short('U')
                .action(ArgAction::SetTrue)
                .help("Re-resolve every package (--upgrade)"),
        )
        .arg(
            Arg::new("upgrade-package")
                .long("upgrade-package")
                .short('P')
                .value_name("PACKAGE")
                .action(ArgAction::Append)
                .help("Re-resolve a single package; pass repeatedly for multiple"),
        ),
)
```

- [ ] **Step 4: Add the dispatcher arm**

In `crates/toolr/src/project.rs`, inside `dispatch_project` add `lock` next to `sync`:

```rust
Some(("sync", sync_m)) => venv_sync(sync_m),
Some(("lock", lock_m)) => venv_lock(lock_m),
Some(("upgrade", upgrade_m)) => venv_upgrade(upgrade_m),
```

- [ ] **Step 5: Add the handler**

In `crates/toolr/src/project.rs`, after `venv_sync` (and its helpers) add:

```rust
fn venv_lock(matches: &ArgMatches) -> Result<ExitCode> {
    let quiet = matches.get_flag("quiet");
    let upgrade_all = matches.get_flag("upgrade");
    let upgrade_pkgs: Vec<String> = matches
        .get_many::<String>("upgrade-package")
        .map(|vals| vals.cloned().collect())
        .unwrap_or_default();

    let cwd = std::env::current_dir()?;
    let repo_root = toolr_core::discovery::discover_project_root(&cwd)?;
    let tools_dir = repo_root.join("tools");

    // -P pre-flight guard — same as venv sync.
    if !upgrade_pkgs.is_empty() {
        let pyproject = tools_dir.join("pyproject.toml");
        for pkg in &upgrade_pkgs {
            if !pyproject_declares_dependency(&pyproject, pkg)? {
                anyhow::bail!(
                    "package `{pkg}` is not declared in {} — add it to `[project] dependencies` first",
                    pyproject.display(),
                );
            }
        }
    }

    let upgrade = build_upgrade_mode(upgrade_all, upgrade_pkgs);

    let consent = toolr_core::uv::install::ConsentMode::from_env();
    let (resolved, uv) = toolr_core::project::ensure_venv_ready(
        &cwd,
        consent,
        toolr_core::project::EnsureOpts::default().with_quiet(quiet),
    )?;

    let status = toolr_core::venv::run_uv_lock(&uv, &tools_dir, &resolved, &upgrade, quiet)?;
    if !status.success() {
        anyhow::bail!(
            "`uv lock` failed with exit code {:?}",
            status.code(),
        );
    }

    if !quiet {
        println!("toolr: refreshed {}", tools_dir.join("uv.lock").display());
    }
    Ok(ExitCode::SUCCESS)
}
```

- [ ] **Step 6: Run the new tests**

Run: `cargo test -p toolr --test project_venv_lock`
Expected: all 3 pass.

- [ ] **Step 7: Commit**

```bash
git add crates/toolr/src/cli.rs crates/toolr/src/project.rs crates/toolr/tests/project_venv_lock.rs
git commit -m "venv lock: new subcommand wrapping `uv lock` with -U / -P"
```

---

## Task 8: Add `venv add` + `venv remove` subcommands + handlers

**Files:**

- Modify: `crates/toolr/src/cli.rs`
- Modify: `crates/toolr/src/project.rs`
- Create: `crates/toolr/tests/project_venv_add.rs`
- Create: `crates/toolr/tests/project_venv_remove.rs`

These two parallel each other; doing them in one task keeps the dispatcher edits coherent.

- [ ] **Step 1: Write the failing integration tests for `add`**

Create `crates/toolr/tests/project_venv_add.rs`:

```rust
//! Integration tests for `toolr project venv add`. These don't run real
//! uv — they cover the clap surface and the help/usage paths.

use assert_cmd::Command;

fn cargo_bin() -> Command {
    Command::cargo_bin("toolr").unwrap()
}

#[test]
fn add_help_lists_package_positional() {
    let output = cargo_bin()
        .args(["project", "venv", "add", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PACKAGE"), "help missing PACKAGE: {stdout}");
    assert!(stdout.contains("uv add"), "help should reference `uv add`: {stdout}");
}

#[test]
fn add_requires_at_least_one_package() {
    let output = cargo_bin()
        .args(["project", "venv", "add"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required") || stderr.contains("PACKAGE") || stderr.contains("usage"),
        "expected clap usage error, stderr was:\n{stderr}"
    );
}
```

- [ ] **Step 2: Write the failing integration tests for `remove`**

Create `crates/toolr/tests/project_venv_remove.rs`:

```rust
//! Integration tests for `toolr project venv remove`. Cover clap +
//! pre-flight guard (package must be declared).

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn cargo_bin() -> Command {
    Command::cargo_bin("toolr").unwrap()
}

#[test]
fn remove_help_lists_package_positional() {
    let output = cargo_bin()
        .args(["project", "venv", "remove", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PACKAGE"), "help missing PACKAGE: {stdout}");
    assert!(stdout.contains("uv remove"), "help should reference `uv remove`: {stdout}");
}

#[test]
fn remove_requires_at_least_one_package() {
    let output = cargo_bin()
        .args(["project", "venv", "remove"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required") || stderr.contains("PACKAGE") || stderr.contains("usage"),
        "expected clap usage error, stderr was:\n{stderr}"
    );
}

#[test]
fn remove_errors_when_package_not_declared() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    fs::write(
        tmp.path().join("tools/pyproject.toml"),
        r#"[project]
name = "tools"
version = "0.0.0"
requires-python = ">=3.11"
dependencies = [
    "requests",
]

[tool.toolr]
venv-location = "cache"
"#,
    )
    .unwrap();

    let output = cargo_bin()
        .current_dir(tmp.path())
        .args(["project", "venv", "remove", "nonexistent-package"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not declared"), "stderr: {stderr}");
    assert!(stderr.contains("nonexistent-package"), "stderr: {stderr}");
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p toolr --test project_venv_add --test project_venv_remove`
Expected: failure — neither subcommand exists.

- [ ] **Step 4: Register both clap subcommands**

In `crates/toolr/src/cli.rs`, after the `lock` subcommand block from Task 7, append:

```rust
.subcommand(
    Command::new("add")
        .about("Add one or more packages to tools/pyproject.toml (wraps `uv add`)")
        .arg(
            Arg::new("packages")
                .value_name("PACKAGE")
                .num_args(1..)
                .required(true)
                .help("Package spec (`name`, `name@version`, `name>=1.2`, …) — passed through to uv"),
        )
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .short('q')
                .action(ArgAction::SetTrue)
                .help("Pass --quiet to uv"),
        ),
)
.subcommand(
    Command::new("remove")
        .about("Remove one or more packages from tools/pyproject.toml (wraps `uv remove`)")
        .arg(
            Arg::new("packages")
                .value_name("PACKAGE")
                .num_args(1..)
                .required(true)
                .help("Package name to remove (must already appear in tools/pyproject.toml)"),
        )
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .short('q')
                .action(ArgAction::SetTrue)
                .help("Pass --quiet to uv"),
        ),
)
```

- [ ] **Step 5: Add dispatcher arms**

In `crates/toolr/src/project.rs`, extend `dispatch_project`:

```rust
Some(("sync", sync_m)) => venv_sync(sync_m),
Some(("lock", lock_m)) => venv_lock(lock_m),
Some(("add", add_m)) => venv_add(add_m),
Some(("remove", remove_m)) => venv_remove(remove_m),
Some(("upgrade", upgrade_m)) => venv_upgrade(upgrade_m),
```

- [ ] **Step 6: Add the handlers**

In `crates/toolr/src/project.rs`, after `venv_lock`:

```rust
fn venv_add(matches: &ArgMatches) -> Result<ExitCode> {
    let quiet = matches.get_flag("quiet");
    let specs: Vec<String> = matches
        .get_many::<String>("packages")
        .expect("clap marks this required")
        .cloned()
        .collect();

    let cwd = std::env::current_dir()?;
    let repo_root = toolr_core::discovery::discover_project_root(&cwd)?;
    let tools_dir = repo_root.join("tools");

    let consent = toolr_core::uv::install::ConsentMode::from_env();
    let (resolved, uv) = toolr_core::project::ensure_venv_ready(
        &cwd,
        consent,
        toolr_core::project::EnsureOpts::default().with_quiet(quiet),
    )?;

    let status = toolr_core::venv::run_uv_add(&uv, &tools_dir, &resolved, &specs, quiet)?;
    if !status.success() {
        anyhow::bail!("`uv add` failed with exit code {:?}", status.code());
    }

    if !quiet {
        println!("toolr: added {} to {}", specs.join(", "), tools_dir.join("pyproject.toml").display());
    }
    Ok(ExitCode::SUCCESS)
}

fn venv_remove(matches: &ArgMatches) -> Result<ExitCode> {
    let quiet = matches.get_flag("quiet");
    let packages: Vec<String> = matches
        .get_many::<String>("packages")
        .expect("clap marks this required")
        .cloned()
        .collect();

    let cwd = std::env::current_dir()?;
    let repo_root = toolr_core::discovery::discover_project_root(&cwd)?;
    let tools_dir = repo_root.join("tools");

    // Pre-flight: every named package must already be declared.
    let pyproject = tools_dir.join("pyproject.toml");
    for pkg in &packages {
        if !pyproject_declares_dependency(&pyproject, pkg)? {
            anyhow::bail!(
                "package `{pkg}` is not declared in {} — nothing to remove",
                pyproject.display(),
            );
        }
    }

    let consent = toolr_core::uv::install::ConsentMode::from_env();
    let (resolved, uv) = toolr_core::project::ensure_venv_ready(
        &cwd,
        consent,
        toolr_core::project::EnsureOpts::default().with_quiet(quiet),
    )?;

    let status = toolr_core::venv::run_uv_remove(&uv, &tools_dir, &resolved, &packages, quiet)?;
    if !status.success() {
        anyhow::bail!("`uv remove` failed with exit code {:?}", status.code());
    }

    if !quiet {
        println!("toolr: removed {} from {}", packages.join(", "), pyproject.display());
    }
    Ok(ExitCode::SUCCESS)
}
```

- [ ] **Step 7: Run the new integration tests**

Run: `cargo test -p toolr --test project_venv_add --test project_venv_remove`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/toolr/src/cli.rs crates/toolr/src/project.rs crates/toolr/tests/project_venv_add.rs crates/toolr/tests/project_venv_remove.rs
git commit -m "venv add/remove: new subcommands wrapping `uv add` / `uv remove`"
```

---

## Task 9: Delete `venv upgrade` + update completions + migration hint

**Files:**

- Modify: `crates/toolr/src/cli.rs`
- Modify: `crates/toolr/src/project.rs`
- Modify: `crates/toolr/src/builtin_completions.rs`
- Delete: `crates/toolr/tests/project_venv_upgrade.rs`
- [ ] **Step 1: Update the failing completions test**

In `crates/toolr/src/builtin_completions.rs` around L269, replace:

```rust
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
```

with:

```rust
#[test]
fn project_venv_offers_path_shell_sync_lock_add_remove() {
    let m = merged_empty_manifest();
    let out = serve_completions(&m, &tokens(&["project", "venv", ""]));
    for expected in ["path", "shell", "sync", "lock", "add", "remove"] {
        assert!(
            out.contains(&expected.to_string()),
            "missing {expected} under project venv, got: {out:?}"
        );
    }
    assert!(
        !out.contains(&"upgrade".to_string()),
        "`upgrade` should no longer be a completion candidate, got: {out:?}"
    );
}
```

- [ ] **Step 2: Run the completions test (expect failure)**

Run: `cargo test -p toolr builtin_completions::tests::project_venv_offers_path_shell_sync_lock_add_remove`
Expected: failure — `upgrade` still in the completions, `lock`/`add`/`remove` may or may not be there yet (they should
be by Task 8, but `upgrade` is the failing assertion).

- [ ] **Step 3: Delete `venv upgrade` clap subcommand**

In `crates/toolr/src/cli.rs`, delete the entire `Command::new("upgrade")` subcommand block (currently around L321–L329):

```rust
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
```

- [ ] **Step 4: Delete the dispatcher arm**

In `crates/toolr/src/project.rs`, remove `Some(("upgrade", upgrade_m)) => venv_upgrade(upgrade_m),` from
`dispatch_project`.

- [ ] **Step 5: Delete `venv_upgrade` function**

In `crates/toolr/src/project.rs`, remove the entire `fn venv_upgrade(...)` function (including the stub placeholder body
from Task 6 Step 0). Keep `pyproject_declares_dependency` and `dep_name_matches` — they're used by `venv_sync`,
`venv_lock`, and `venv_remove`.

- [ ] **Step 6: Update the migration hint**

In `crates/toolr/src/project.rs`, replace `deps_migration_hint` body:

```rust
fn deps_migration_hint() -> Result<ExitCode> {
    eprintln!("error: `project deps` was removed in 0.22");
    eprintln!("hint: use `toolr project venv` instead");
    eprintln!("       project deps sync       →  toolr project venv sync");
    eprintln!("       project deps upgrade …  →  toolr project venv sync -U <pkg>");
    eprintln!("see CHANGELOG.md (0.22 BREAKING) for the rename");
    Ok(ExitCode::from(2))
}
```

- [ ] **Step 7: Delete the integration test file**

```bash
git rm crates/toolr/tests/project_venv_upgrade.rs
```

- [ ] **Step 8: Run the full toolr test suite**

Run: `cargo test -p toolr`
Expected: pass. The completions test now sees the new subcommand list; the `project_venv_upgrade` test is gone;
`venv_sync`, `venv_lock`, `venv_add`, `venv_remove` tests all stay green.

- [ ] **Step 9: Commit**

```bash
git add crates/toolr/src/cli.rs crates/toolr/src/project.rs crates/toolr/src/builtin_completions.rs
git commit -m "venv upgrade: remove subcommand (replaced by sync -U / -P)"
```

---

## Task 10: Regenerate captured `--help` snippets

**Files:**

- Modify: `docs/cli-files/*` (auto-generated)

The pre-commit hook `regen-doc-snippets.py --check` will fail because the captured `--help` outputs no longer match.

- [ ] **Step 1: Find the regen entrypoint**

Run: `head -40 .pre-commit-hooks/regen-doc-snippets.py`
Expected: it'll show the script's usage. The script supports a no-arg invocation that rewrites the files in place (the
`--check` flag in pre-commit is the read-only mode).

- [ ] **Step 2: Regenerate**

Run: `.pre-commit-hooks/regen-doc-snippets.py`
Expected: writes updated `--help` captures into `docs/cli-files/`.

- [ ] **Step 3: Diff and sanity-check**

Run: `git diff -- docs/cli-files/`
Expected diff:

- New entries for `venv lock`, `venv add`, `venv remove`.
- `venv sync --help` gains `-U / --upgrade` and `-P / --upgrade-package`.
- `venv upgrade --help` entry removed.
- [ ] **Step 4: Run the pre-commit check directly**

Run: `prek run regen-doc-snippets --files docs/cli-files/`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add docs/cli-files/
git commit -m "docs: regenerate cli-files snippets for venv lock/add/remove + sync -U/-P"
```

---

## Task 11: Update narrative docs

**Files:**

- Modify: `docs/**/*.md` (whichever pages reference the venv subcommands)

The reference page(s) for `toolr project venv` need a paragraph for each new verb and a search-and-replace for `venv
upgrade` → `venv sync -U <pkg>`.

- [ ] **Step 1: Find the venv reference pages**

Run: `grep -rln "venv upgrade\|project venv sync" docs/ | grep -v cli-files`
Expected: a handful of `.md` files (project-venv reference, quickstart, possibly the mise enter-hook recipe).

- [ ] **Step 2: For each file, update references**

For every match, apply these prose edits:

- Replace `toolr project venv upgrade <pkg>` → `toolr project venv sync -U <pkg>` (or `-P <pkg>` for repeatable single
  packages — pick the form that fits the surrounding context).
- Where the page lists the venv subcommands, add `lock`, `add`, `remove` next to `sync` with one-line summaries:
    - `lock` — Refresh `tools/uv.lock` without touching the venv.
    - `add <package>[@<version>]…` — Add packages to `tools/pyproject.toml` (wraps `uv add`).
    - `remove <package>…` — Remove packages from `tools/pyproject.toml` (wraps `uv remove`).
- [ ] **Step 3: Verify docs build**

Run: `mise run docs-build` (or equivalent — check `mise.toml` for the docs task name; falls back to `mkdocs build
--strict` if mise has no alias).
Expected: pass.

- [ ] **Step 4: Run rumdl on touched files**

Run: `prek run rumdl --files <each modified .md>`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add docs/
git commit -m "docs: cover venv lock/add/remove and migrate venv upgrade references"
```

---

## Task 12: CHANGELOG entries

**Files:**

- Modify: `CHANGELOG.md`

- [ ] **Step 1: Find the Unreleased / 0.22 section**

Run: `grep -n "Unreleased\|## 0.22\|## 0.21.1" CHANGELOG.md | head`
Expected: the rename PR (#289) has already established a 0.22 section (or an Unreleased section bound for 0.22). Locate
the BREAKING-changes block within it.

- [ ] **Step 2: Append the four bullet points**

Add to the BREAKING block (or create one if none exists yet — match the structure of the existing 0.21.0 BREAKING block
at lines 22+):

```markdown
### `project venv upgrade` removed in favour of `venv sync -U / -P`

- **What changed:** `toolr project venv upgrade <pkg>` is gone.
  Use `toolr project venv sync -U <pkg>` to upgrade a single package
  (or `-P <pkg>` repeatedly), or `toolr project venv sync -U` to
  re-resolve all packages.
- **Why:** uv expresses upgrades as flags on `lock` and `sync`, not as
  a standalone verb. Aligning toolr with uv's surface removes a
  toolr-specific verb that didn't pull its weight.
- **Migration:** mechanical rename. `venv upgrade foo` → `venv sync -P foo`.
```

And to the features block:

```markdown
- *(project venv)* `lock` — wrap `uv lock` for refreshing
  `tools/uv.lock` without applying ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
- *(project venv)* `add <package>[@<version>]…` — wrap `uv add` against
  `tools/` ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
- *(project venv)* `remove <package>…` — wrap `uv remove` against
  `tools/` ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
- *(project venv sync)* `-U` / `--upgrade` and `-P` / `--upgrade-package`
  flags mirroring uv ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
```

- [ ] **Step 3: Verify rumdl is happy (CHANGELOG.md is excluded from rumdl per `.rumdl.toml`, but other markdown rules
  may still apply)**

Run: `prek run rumdl --files CHANGELOG.md`
Expected: skipped (config excludes it) or pass.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md
git commit -m "changelog: document venv upgrade removal + new lock/add/remove + sync -U/-P"
```

---

## Task 13: Full-workspace verification

- [ ] **Step 1: Full cargo test**

Run: `cargo test --workspace`
Monitor the run (memory: long cargo runs need monitoring). On a clean cache this can take 5–10 minutes — poll the output
every 30–60 s.
Expected: pass.

- [ ] **Step 2: Full prek run**

Run: `prek run --all-files`
Expected: pass (`rumdl`, `cargo check`, `clippy`, regen-doc-snippets, etc.).

- [ ] **Step 3: Smoke-test the new commands locally (optional but recommended)**

```bash
cargo build -p toolr
target/debug/toolr project venv lock --help
target/debug/toolr project venv add --help
target/debug/toolr project venv remove --help
target/debug/toolr project venv sync --help    # confirm -U/-P appear
```

Expected: each `--help` exits 0 with the documented flags.

- [ ] **Step 4: Push the stacked branch**

```bash
git spice branch submit
```

Expected: git-spice opens / refreshes the PR for `venv-uv-parity` with `mise-enter-auto-sync` (PR #289) as the base.

---

## Self-review pass

**Spec coverage check:**

- Command surface (sync / lock / add / remove + remove upgrade) → Tasks 6, 7, 8, 9.
- Flag semantics (-U / -P / --quiet / --force) → Task 6 (sync), Task 7 (lock); add/remove have only --quiet, Task 8.
- Behavior matrix → covered by integration tests in each task.
- Pre-flight guards (sync -P, lock -P, remove pkg) → Tasks 6, 7, 8.
- Core layer (`UpgradeMode`, `run_uv_lock`, `edit.rs` add/remove, `sync_if_needed` bypass) → Tasks 1, 2, 3, 4, 5.
- CLI handler layer → Tasks 6, 7, 8, 9.
- Migration hint → Task 9.
- Tests (delete upgrade, extend sync, new lock/add/remove, core unit tests, completions test) → Tasks 1, 2, 3, 4, 6, 7,
  8, 9.
- Docs (cli-files snippets, prose, CHANGELOG) → Tasks 10, 11, 12.
- Risks → no specific task; covered by the integration tests pinning argv shapes.

**Placeholder scan:** none — every step has runnable code or a concrete command.

**Type consistency:**

- `UpgradeMode` defined in Task 1, used in Tasks 2, 3, 4, 5, 6, 7. Consistent.
- `EnsureOpts::with_upgrade` introduced in Task 5, used in Task 6's `venv_sync`. Consistent.
- `build_upgrade_mode` defined in Task 6, reused in Task 7's `venv_lock`. Consistent.
- `pyproject_declares_dependency` (pre-existing) stays through Task 9 and is used by Tasks 6, 7, 8.

Plan is internally consistent; proceed.
