# ctx.run Input Deadlock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ctx.run(..., input=...)` no longer deadlocks when the child fills its stdout pipe before reading stdin, and
both timeouts cover the stdin write (F-08).

**Architecture:** `run_command_internal` moves the stdin write into a thread spawned after the two reader threads and
joined after the child exits. `BrokenPipe` from that thread is ignored.

**Tech Stack:** Rust (`std::process`, `std::thread`), pytest.

**Spec:** [`specs/archive/2026/2026-10-31-ctx-run-input-deadlock-design.md`](./2026-10-31-ctx-run-input-deadlock-design.md)

## Global Constraints

- Only `crates/toolr-core/src/command.rs` changes in production code. No runner schema change; the pyo3 signature stays
  the same.
- Every new Rust test that spawns a child goes through `bounded_run_command(config, Duration::from_secs(5))` in
  `command_test.rs` (`mod tokio_tests`), so a regression fails instead of hanging the workspace run.
- Regression tests use the reported shape exactly: a Python child writing 200000 bytes to stdout before reading a
  200000-byte stdin.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **Readers start before the writer.** If `stdin_thread` is spawned before the readers, the deadlock is back. Task 1,
   `input_larger_than_pipe_with_output_first_does_not_deadlock`.
2. **Timeouts cover the write.** A child that never reads stdin must hit `timeout_secs`. Task 1,
   `timeout_fires_while_child_ignores_stdin`.
3. **`BrokenPipe` is not an error, other write errors still are.** Task 1, `child_closing_stdin_early_is_not_an_error`.
4. **No thread outlives the call on the success path.** `stdin_thread` is joined after `try_wait` returns `Some`.

---

### Task 1: Feed stdin from a thread

**Files:**

- Modify: `crates/toolr-core/src/command.rs` (`run_command_internal`)
- Test: `crates/toolr-core/src/command/command_test.rs` (`mod tokio_tests`), `tests/utils/command/test_command.py`

**Interfaces:** none produced. `run_command_internal(config: CommandConfig) -> Result<i32, Box<dyn Error + Send +
Sync>>` keeps its signature. Internal: `stdin_thread: Option<JoinHandle<io::Result<()>>>`.

- [ ] **Step 1: Write the tests.**
    - `command_test.rs` / `tokio_tests`:
        - `input_larger_than_pipe_with_output_first_does_not_deadlock`: args
          `[python, "-c", "import sys; sys.stdout.write('x'*200000); sys.stdout.flush(); sys.stdin.buffer.read()"]`,
          `input: Some(vec![b'y'; 200000])`, capture fds from two `NamedTempFile`s (`get_file_descriptor` +
          `wrap_fd`), `timeout_secs: Some(30.0)`. Assert `bounded_run_command(.., 5s)` returns `Ok(Ok(0))` and the
          stdout capture holds exactly 200000 bytes.
        - `child_closing_stdin_early_is_not_an_error`: args
          `[python, "-c", "import sys; sys.stdin.buffer.read(1)"]`, `input: Some(vec![b'y'; 200000])` → `Ok(Ok(0))`.
        - `timeout_fires_while_child_ignores_stdin`: args
          `[python, "-c", "import time; time.sleep(10)"]`, `input: Some(vec![b'y'; 200000])`,
          `timeout_secs: Some(0.5)` → inner `Err` whose message contains `"timed out after"`.
    - `tests/utils/command/test_command.py`:
        - `test_input_larger_than_pipe_buffer_with_output_first`: `run([sys.executable, "-c", <same script>],
          input=b"y" * 200000, capture_output=True, text=False, timeout_secs=30)` → `returncode == 0`, and
          `len(result.stdout.read()) == 200000`. The repo has no `pytest-timeout`; `timeout_secs=30` is not a guard
          before the fix (the write never reaches the poll loop), so run this test only after the Rust tests pass.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p toolr-core --lib command::` → the deadlock test fails
      with "did not return within 5s"; the early-close test fails with "Failed to write to stdin"; the timeout test
      fails the 5 s bound. Dropping the tokio `Runtime` waits for the wedged blocking task, so a failing run may hang
      after reporting; stop it after 30 s.

- [ ] **Step 3: Implement.** In `run_command_internal`, delete the synchronous `write_all` block. Take
      `child.stdin` and `config.input.take()` (make `config` `mut`) before the reader setup, and after both
      `thread::spawn` reader calls spawn
      `stdin_thread = Some(thread::spawn(move || write_stdin(stdin, input)))`, where

      ```rust
      fn write_stdin(mut stdin: ChildStdin, input: Vec<u8>) -> io::Result<()>
      ```

      calls `write_all`, maps `ErrorKind::BrokenPipe` to `Ok(())` and drops `stdin`. After the poll loop breaks
      with a status, join `stdin_thread` first; on `Ok(Err(e))` return
      `CommandExecutionError::new(format!("Failed to write to stdin: {e}"))` (after joining the readers, so they
      are not leaked). On the two timeout returns, leave the thread to end on its own: the kill closes the pipe.

- [ ] **Step 4: Run** `cargo test -p toolr-core --lib command::` and
      `uv run pytest tests/utils/command/test_command.py` → PASS, old tests included.

- [ ] **Step 5: Commit.** `fix(command): write ctx.run stdin from its own thread`

### Task 2: Release notes, verification, archive

- [ ] **Step 1:** Add the spec's `UNRELEASED.md` text verbatim.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [ ] **Step 3:**
      `git mv specs/2026-10-31-ctx-run-input-deadlock-design.md specs/2026-10-31-ctx-run-input-deadlock-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive ctx.run input deadlock design and plan`.
