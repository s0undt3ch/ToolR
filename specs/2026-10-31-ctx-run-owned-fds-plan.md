# ctx.run Owned Fds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ctx.run`'s output destinations are owned `File` copies made once at the Python boundary, so no thread
writes to an fd number it doesn't own, and a bad fd is an error instead of undefined behaviour (F-41).

**Architecture:** `toolr_core::command::fds::file_from_python_fd` duplicates each Python fd (`F_DUPFD_CLOEXEC` on
unix, `DuplicateHandle` on Windows). `CommandConfig` holds `Option<File>`; F-40's `Sink` writes to its `File`.

**Tech Stack:** Rust (`std::os::fd::OwnedFd`, `std::os::windows::io::OwnedHandle`), `libc`, `winapi`, pyo3, pytest.

**Spec:** [`specs/2026-10-31-ctx-run-owned-fds-design.md`](./2026-10-31-ctx-run-owned-fds-design.md)

## Global Constraints

- Branch stacks on `fix/ctx-run-reader-drain` (F-40) → `fix/ctx-run-timeout-kills-tree` (F-21) →
  `fix/ctx-run-input-deadlock` (F-08). Keep their behaviour: the stdin thread, `kill_tree`, `drain_threads`, `pump`,
  `Sink` kinds and the shared `sink_error`. Every test those branches added must pass unchanged in what it asserts;
  only how it builds fds may change.
- `file_from_python_fd` is the only `unsafe` left in the fd path.
  `git grep -n 'from_raw_fd\|from_raw_handle\|mem::forget' crates/toolr-core/src/command.rs` prints nothing at the end.
- No runner schema change; the pyo3 signature keeps `Option<i32>` per fd.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **The copy is close-on-exec.** Without it the child inherits a capture fd. Task 1,
   `file_from_python_fd_sets_cloexec`.
2. **Shared offset.** Python's `seek(0)` after the call must still see all output. Covered by the existing
   `tests/utils/command` capture tests.
3. **Bad fds fail before spawn.** Task 2, `test_run_command_impl_rejects_closed_fd`.
4. **Windows.** `get_osfhandle` on a bad fd must not hit the CRT invalid-parameter handler path unchecked; check its
   `-1` return. CI's Windows job is the only place this runs.

---

### Task 1: `file_from_python_fd`

**Files:**

- Create: `crates/toolr-core/src/command/fds.rs`
- Modify: `crates/toolr-core/src/command.rs` (`pub mod fds;`), `crates/toolr-core/src/lib.rs` (re-export
  `file_from_python_fd`)
- Modify: `crates/toolr-core/Cargo.toml` only if a `winapi` feature is missing for `DuplicateHandle` (it already
  lists `handleapi` and `processthreadsapi`)

**Interfaces:**

```rust
pub fn file_from_python_fd(fd: i32) -> std::io::Result<std::fs::File>;
```

- [ ] **Step 1: Write the tests** in `fds.rs` `mod tests`:
    - `file_from_python_fd_writes_to_same_file`: open a `NamedTempFile`, copy its fd, write `b"abc"` through the
      copy, drop the copy, read the temp file → `"abc"`.
    - `file_from_python_fd_survives_original_close`: copy the fd, drop the original `File`, write through the copy →
      `Ok`, and the path's content is the written bytes.
    - `file_from_python_fd_rejects_closed_fd` (`#[cfg(unix)]`): take `as_raw_fd()` of a `File`, drop the `File`, call
      → `Err` with `raw_os_error() == Some(libc::EBADF)`.
    - `file_from_python_fd_rejects_negative_fd`: `-1` → `Err` on every platform.
    - `file_from_python_fd_sets_cloexec` (`#[cfg(unix)]`): `libc::fcntl(copy.as_raw_fd(), F_GETFD) & FD_CLOEXEC != 0`.

- [ ] **Step 2: Run** `cargo test -p toolr-core --lib command::fds` → fails to compile (function missing).

- [ ] **Step 3: Implement** per the spec. Unix: `fcntl(fd, F_DUPFD_CLOEXEC, 0)`, `-1` → `last_os_error()`, else
      `File::from(OwnedFd::from_raw_fd(new))`. Windows: `fd < 0` → `ERROR_INVALID_HANDLE` before any CRT call, then
      `libc::get_osfhandle(fd)`, `-1` →
      `io::Error::from_raw_os_error(ERROR_INVALID_HANDLE)`, then `DuplicateHandle(GetCurrentProcess(), h,
      GetCurrentProcess(), &mut out, 0, FALSE, DUPLICATE_SAME_ACCESS)`, `0` → `last_os_error()`, else
      `File::from(OwnedHandle::from_raw_handle(out))`. One `// SAFETY:` comment per `unsafe` block.

- [ ] **Step 4: Run** the Step 2 command → PASS.

- [ ] **Step 5: Commit.** `refactor(command): add file_from_python_fd`

### Task 2: Owned files through `CommandConfig` and `Sink`

**Files:**

- Modify: `crates/toolr-core/src/command.rs` (`CommandConfig`, `Default`, `Sink`, `ThreadSafeHandle` removed, imports)
- Modify: `crates/toolr-core/src/lib.rs` (drop the `ThreadSafeHandle` re-export)
- Modify: `crates/toolr-py/src/lib.rs` (`run_command_impl`; delete `fd_to_handle` and the `RawHandle`/
  `ThreadSafeHandle` imports)
- Modify: `crates/toolr-core/src/command/command_test.rs`
- Test: `tests/utils/command/test_command.py`

**Interfaces:**

```rust
pub struct CommandConfig {
    // .. unchanged fields ..
    pub stdout_fd: Option<File>,
    pub stderr_fd: Option<File>,
    pub sys_stdout_fd: Option<File>,
    pub sys_stderr_fd: Option<File>,
}

struct Sink { kind: SinkKind, stream_name: &'static str, file: Option<File> } // None = closed
impl Sink { fn write(&mut self, buf: &[u8]) -> io::Result<()>; }               // file.write_all(buf)
```

`CommandConfig` loses `Default`'s `#[cfg]` pairs. The Windows path's `stdout_fd.clone()` calls go: each `File` moves
into its reader thread.

- [ ] **Step 1: Write the Python test** `test_run_command_impl_rejects_closed_fd` in
      `tests/utils/command/test_command.py`: open a `tempfile.TemporaryFile()`, keep `fd = f.fileno()`, `f.close()`,
      call `run_command_impl([sys.executable, "-c", "print(1)"], stdout_fd=fd, stderr_fd=fd)` →
      `pytest.raises(CommandError, match="invalid stdout_fd")`. Mark it `pytest.mark.skipif(IS_WINDOWS, ...)`: a closed
      fd can trip the CRT invalid-parameter handler there. Run `uv run pytest tests/utils/command -k closed_fd` → fails
      today (no error raised, or a silent short write).

- [ ] **Step 2: Implement.**
    - `CommandConfig` fields to `Option<File>`; delete `ThreadSafeHandle` and the `FromRawFd`/`FromRawHandle`
      imports.
    - `Sink` takes the `File` by value; `write` is `file.write_all(buf)`; closing a sink is `self.file = None`. Keep
      F-40's `pump` logic and error strings.
    - `run_command_impl`: after the pairing checks, convert each fd with
      `file_from_python_fd(fd).map_err(|e| CommandError::new_err(format!("invalid {name} {fd}: {e}")))?`.
    - `command_test.rs`: `get_file_descriptor(&File) -> File` becomes `file.try_clone().unwrap()`; delete
      `wrap_fd` (use `Some(..)`); unix `create_pipe` returns `(File, File)` via `OwnedFd::from_raw_fd`; drop the
      manual `libc::close` calls on write ends, which the config now owns and closes; F-40's `broken_pipe()` returns a
      `File`. Windows `create_pipe` returns `(File, File)` from `OwnedHandle`.

- [ ] **Step 3: Run** `cargo test -p toolr-core --lib command::` and `cargo build -p toolr-py`, then
      `uv run pytest tests/utils/command tests/context` → PASS. `git grep -n 'from_raw_fd\|from_raw_handle\|mem::forget\|ThreadSafeHandle'
      crates/toolr-core/src crates/toolr-py/src` → only `fds.rs` and test pipe helpers.

- [ ] **Step 4: Commit.** `refactor(command): pass owned files to the ctx.run driver`

### Task 3: Release notes, verification, archive

- [ ] **Step 1:** Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(command): note ctx.run owned output fds`.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [ ] **Step 3:** Move both specs:

    ```sh
    git mv specs/2026-10-31-ctx-run-owned-fds-design.md \
        specs/2026-10-31-ctx-run-owned-fds-plan.md specs/archive/2026/
    ```

  Then fix the plan's `Spec:` link and set the design's status to implemented. Commit
  `docs(specs): archive ctx.run owned fds design and plan`.
