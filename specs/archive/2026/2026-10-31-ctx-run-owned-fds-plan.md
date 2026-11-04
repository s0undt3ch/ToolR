# ctx.run Owned Output Files Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `CommandConfig` carries its output destinations as owned `File`s, duplicated once where Python hands them
over, so `ThreadSafeHandle` and the per-platform field pairs go (F-41 residue).

**Architecture:** `toolr_core::command::file_from_python_fd` replaces `own_sink` and `toolr-py`'s `fd_to_handle`.
`toolr-py` calls it per fd; `run_command_internal` passes the files straight to the readers.

**Tech Stack:** Rust, `libc`, pyo3.

**Spec:** [`specs/2026-10-31-ctx-run-owned-fds-design.md`](./2026-10-31-ctx-run-owned-fds-design.md)
(revised 2026-11-04).

## Global Constraints

- Branch `refactor/ctx-run-owned-fds`, stacked on `fix/ctx-run-reader-drain` (F-40). Keep F-40's per-sink handling.
- `toolr-core`'s `command` module builds without the `full` feature (toolr-py links it). Add no dependency.
- 99% patch coverage on changed `toolr-core` lines. `crates/toolr-py/src` is excluded from coverage, so the logic
  goes in `toolr-core`.
- Conventional Commits, no `Co-Authored-By`, British English. Stage by path, never `git add -A`, never `audit/`.
  `rustfmt --edition 2024 <file>` on touched files only.

## Review Focus

1. **A closed fd is an error, not a crash**, on unix: `file_from_python_fd_rejects_a_closed_fd`.
2. **The duplicate shares the offset**, so Python's `seek(0)` after the call reads all output: the Python capture
   tests in `tests/utils/command` stay green.
3. **Windows builds.** CI is the only Windows check; read the `#[cfg(windows)]` branch carefully.
4. **A negative fd is rejected before any CRT call on Windows**, and on unix `fcntl` returns `EBADF` for it:
   `file_from_python_fd_rejects_a_negative_fd`.

---

### Task 1: Owned files end to end

**Files:**

- Modify: `crates/toolr-core/src/command.rs`, `crates/toolr-core/src/lib.rs` (drop the `ThreadSafeHandle`
  re-export, export `file_from_python_fd`)
- Modify: `crates/toolr-py/src/lib.rs`
- Test: `crates/toolr-core/src/command/command_test.rs` (replace the `ThreadSafeHandle`-based `get_file_descriptor`
  helper with one that returns a `File` via `try_clone`)

**Interfaces:**

- `pub fn file_from_python_fd(fd: i32) -> std::io::Result<std::fs::File>`
- `CommandConfig { stdout_fd, stderr_fd, sys_stdout_fd, sys_stderr_fd }: Option<File>` on every platform.
- [ ] **Step 1: Write the tests:**
    - `file_from_python_fd_duplicates_an_open_fd`: open a temp file, call it with the file's raw fd, write through the
      result, read back through the original → the bytes are there, and the returned file's raw fd differs.
    - `file_from_python_fd_rejects_a_closed_fd` (`#[cfg(unix)]`): an fd from a `File` that has been dropped → `Err`
      with `raw_os_error() == Some(libc::EBADF)`.
    - `file_from_python_fd_rejects_a_negative_fd`: `-1` → `Err`.
- [ ] **Step 2: Implement** per the spec. Delete `own_sink`, `ThreadSafeHandle`, the `#[cfg]` field pairs and
  `toolr-py`'s `fd_to_handle`. In `toolr-py`, map each conversion error to
  `CommandError::new_err(format!("invalid {name} {fd}: {e}"))`.
- [ ] **Step 3: Run** `cargo test -p toolr-core --lib command::`, `cargo clippy --workspace --all-targets`,
  `uv sync --all-extras --dev --reinstall-package toolr-py` and `uv run pytest tests/utils/command tests/context`
  → PASS.
- [ ] **Step 4: Add** a Python test in `tests/utils/command/test_command.py` that calls the extension's
  `run_command_impl` with a closed fd and asserts `CommandError` whose message starts `invalid stdout_fd`.
- [ ] **Step 5: Commit** `refactor(command): pass ctx.run output destinations as owned files` with a
  `Release-Notes: skip` trailer.

### Task 2: Verification and archive

- [ ] **Step 1:** `mise run test` → green. Coverage, then
  `cargo xtask patch-coverage --base fix/ctx-run-reader-drain` → ≥ 99%.
- [ ] **Step 2:** `git mv` both specs to `specs/archive/2026/` as the last commit.
