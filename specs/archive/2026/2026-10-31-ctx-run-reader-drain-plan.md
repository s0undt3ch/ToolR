# ctx.run Reader Drain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A failed write to one `ctx.run` output destination no longer stops the other, and a failed capture write
raises instead of returning truncated output (F-40).

**Architecture:** `spawn_reader` takes its capture and stream files by role and handles a failure on each separately.
The first recorded error goes into `ReaderState` and `run_command_internal` returns it after the readers are joined.

**Tech Stack:** Rust, `libc` (test pipes), `tempfile`.

**Spec:** [`specs/2026-10-31-ctx-run-reader-drain-design.md`](./2026-10-31-ctx-run-reader-drain-design.md)
(revised 2026-11-04 against #645).

## Global Constraints

- Branch `fix/ctx-run-reader-drain` on `main`; F-41 (`refactor/ctx-run-owned-fds`) stacks on it.
- Keep `own_sink`, `ReaderState`'s existing fields, `DetachOnDrop`, `Timeouts` and the three-phase wait as they are.
  The reader threads still return `()`.
- Tests that spawn a child go through the existing bounded helper in `command_test.rs` (look for `bounded_run_command`
  or the timeout wrapper the stdin deadlock tests use), so a regression fails instead of hanging the suite.
- Broken-pipe tests build stream fds with `libc::pipe` and close the read end, so they are `#[cfg(unix)]`.
- 99% patch coverage on changed lines. No mocks: real children (`python -c`), real pipes and temp files.
- Conventional Commits, no `Co-Authored-By`, British English. Stage by path, never `git add -A`, never `audit/`.
  `rustfmt --edition 2024 <file>` on touched files only.

## Review Focus

1. **The capture survives a broken stream.** `capture_kept_when_stream_pipe_breaks`.
2. **A broken pipe with nothing else to write to still ends the child.** Draining for ever is the regression to
   avoid. `stream_only_broken_pipe_ends_child_like_a_pipeline`.
3. **No sinks at all still drains**, so the no-output timeout keeps seeing output. The existing timeout tests cover
   this; they must stay green.
4. **Error precedence.** A timeout beats a sink error, and a sink error beats the stdin writer's error.
5. **A capture failure lets the child finish.** `capture_write_failure_is_reported` checks the child's side effect
   (it writes a marker file after its output) happened.

---

### Task 1: Reproduce, then fix per-sink failure handling

**Files:**

- Modify: `crates/toolr-core/src/command.rs` (`ReaderState`, `spawn_reader`, `run_command_internal`)
- Test: `crates/toolr-core/src/command/command_test.rs`

**Interfaces:**

- `ReaderState` gains `error: Mutex<Option<String>>` and `fn record(&self, msg: String)` (keeps the first).
- `fn spawn_reader<R: Read + Send + 'static>(pipe: R, name: &'static str, capture: Option<File>, stream:
  Option<File>, state: Arc<ReaderState>) -> JoinHandle<()>`.
- [ ] **Step 1: Write the failing tests** (all `#[cfg(unix)]`; add a helper `fn broken_pipe() -> RawFd` that makes a
  `libc::pipe`, closes the read end and returns the write end):
    - `capture_kept_when_stream_pipe_breaks`: stream fds from `broken_pipe()`, capture fds from two temp files, child
      `import sys; sys.stdout.write('x'*300000); sys.stdout.flush()` → `Ok(0)`, and the stdout capture holds exactly
      300000 bytes.
    - `stream_only_broken_pipe_ends_child_like_a_pipeline`: broken stream fds, no capture, child
      `import sys\nwhile True: sys.stdout.write('y'*8192)`, `timeout_secs: Some(8.0)` → returns within 3 s, not a
      timeout error, exit code non-zero. (Expected to pass before the fix too: it pins today's behaviour.)
    - `capture_write_failure_is_reported`: `stdout_fd` from a temp file opened read-only, `stderr_fd` from a normal
      temp file, child prints `hello` then writes a marker file → `Err` whose message contains
      `Failed to write captured stdout`, and the marker file exists.
    - `stream_write_failure_other_than_broken_pipe_is_reported`: `sys_stdout_fd` from a read-only temp file → `Err`
      containing `Failed to write stdout`.
- [ ] **Step 2: Run** `cargo test -p toolr-core --lib command::` → the first, third and fourth tests fail.
- [ ] **Step 3: Implement** per the spec's Decision. Build each reader's `capture`/`stream` from
  `own(config.stdout_fd)`/`own(config.sys_stdout_fd)` (likewise stderr) and pass `"stdout"`/`"stderr"`. In the
  loop, track whether a capture write failed; stop reading only when the reader started with at least one sink, has
  none left, and no capture write failed. After the readers are joined, return the recorded error as
  `CommandExecutionError` before `stdin_result`.
- [ ] **Step 4: Run** `cargo test -p toolr-core --lib command::` and
  `uv run pytest tests/utils/command tests/context` → PASS.
- [ ] **Step 5: Commit** `fix(command): keep ctx.run capture when the stream target fails`.

### Task 2: Release note, verification, archive

- [ ] **Step 1:** `toolr release-notes add --text "<the spec's note>"`. Commit `docs(release-notes): …`.
- [ ] **Step 2:** `mise run test` → green. `cargo llvm-cov …` then `cargo xtask patch-coverage --base main` → ≥ 99%.
- [ ] **Step 3:** `git mv` both specs to `specs/archive/2026/` as the last commit.
