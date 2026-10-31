# ctx.run Reader Drain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A failed write to one `ctx.run` output destination no longer stops the other, and a failed capture write
raises instead of returning truncated output (F-40).

**Architecture:** A `Sink` type replaces the eight copy blocks in the reader threads. Each sink fails on its own; the
first recorded error goes into shared state and is returned after the child exits.

**Tech Stack:** Rust, `libc` (test pipes).

**Spec:** [`specs/2026-10-31-ctx-run-reader-drain-design.md`](./2026-10-31-ctx-run-reader-drain-design.md)

## Global Constraints

- Branch stacks on `fix/ctx-run-timeout-kills-tree` (F-21), which stacks on `fix/ctx-run-input-deadlock` (F-08).
  Keep F-08's `stdin_thread`, F-21's `tree::kill_tree` and `drain_threads` as they are. The reader threads still
  return `()`.
- `Sink::write` keeps the existing `from_raw_fd`/`from_raw_handle` + `mem::forget` code. Replacing it is F-41.
- Every new test that spawns a child goes through `bounded_run_command(config, Duration::from_secs(10))`.
- Broken-pipe tests build stream fds with `libc::pipe` and close the read end, so they are `#[cfg(unix)]`.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **The capture survives a broken stream.** Task 2, `capture_kept_when_stream_pipe_breaks`.
2. **A broken pipe with nothing else to write to still ends the child.** Draining for ever is the regression to
   avoid. Task 2, `stream_only_broken_pipe_ends_child_like_a_pipeline`.
3. **Error precedence.** A timeout error beats a sink error, and a sink error beats F-08's stdin error.
4. **One sink type, eight call sites gone.** No copy-pasted `write_all`/`flush`/`break` blocks are left.

---

### Task 1: Reproduce the reported shape (plausible finding)

**Files:**

- Test: `crates/toolr-core/src/command/command_test.rs` (`mod tokio_tests`)

**Interfaces:** test helper `fn broken_pipe() -> i32` (`#[cfg(unix)]`): `libc::pipe`, close the read end, return the
write end.

- [ ] **Step 1: Write the tests.**
    - `reported_shape_sink_write_error_does_not_hang` (`#[cfg(unix)]`): `sys_stdout_fd`/`sys_stderr_fd` from
      `broken_pipe()`, args `[python, "-c", "import sys; sys.stdout.write('x'*300000); sys.stdout.flush()"]`,
      `timeout_secs: Some(4.0)` → returns within 2 s and is not a timeout error. This is the audit's shape.
    - `capture_kept_when_stream_pipe_breaks` (`#[cfg(unix)]`): the same child and broken stream fds, plus capture fds
      from two `NamedTempFile`s → `Ok(Ok(0))`, and the stdout capture holds exactly 300000 bytes.
    - `capture_write_failure_is_reported` (`#[cfg(unix)]`): `stdout_fd` from a file opened read-only
      (`File::open(tmp)`), `stderr_fd` from a normal temp file, args
      `[python, "-c", "print('hello')"]` → inner `Err` whose message contains `"Failed to write captured stdout"`.

- [ ] **Step 2: Run** `cargo test -p toolr-core --lib command::` and record the result.
      Expected: the reported-shape test passes (no hang: dropping the reader closes the child's pipe), the other two
      fail (`Ok(120)` with a short capture; `Ok(0)` with no error).
      **Close criteria:** if the reported-shape test hangs, the audit's mechanism is real; carry on, the fix covers it.
      If all three pass on the stacked base, a failed write already surfaces; commit only the tests as
      `test(command): cover ctx.run sink write failures`, set the design's status to "closed: not reproducible",
      archive both specs (Task 3, Step 3) and stop.

- [ ] **Step 3:** Commit the failing tests with Task 2's fix.

### Task 2: Per-sink failure handling

**Files:**

- Modify: `crates/toolr-core/src/command.rs` (reader threads in `run_command_internal`)
- Test: `crates/toolr-core/src/command/command_test.rs`

**Interfaces:**

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
enum SinkKind { Capture, Stream }

struct Sink {
    kind: SinkKind,
    stream_name: &'static str, // "stdout" | "stderr", for the error text
    #[cfg(unix)] fd: RawFd,
    #[cfg(windows)] handle: ThreadSafeHandle,
    open: bool,
}

impl Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<()>; // write_all + flush, the existing unsafe code
}

type SinkError = Arc<Mutex<Option<String>>>;

fn pump(reader: impl Read, sinks: Vec<Sink>, last_output: Arc<Mutex<Instant>>, sink_error: SinkError);
```

`pump` is the body of one reader thread: read into an 8 KiB buffer; update `last_output`; write to each open sink. On
error, `Capture` → record `"Failed to write captured {stream_name}: {e}"` (if none yet) and close the sink; `Stream`
with `ErrorKind::BrokenPipe` → close the sink silently; `Stream` otherwise → record
`"Failed to stream {stream_name}: {e}"` and close. Before each read, if no sink is open and at least one sink existed,
return (drops the pipe). With no sinks configured at all, keep draining as today, so `last_output` still feeds the
no-output watchdog.

- [ ] **Step 1: Add** `stream_only_broken_pipe_ends_child_like_a_pipeline` (`#[cfg(unix)]`): broken stream fds, no
      capture, args `[python, "-c", "import sys\nwhile True: sys.stdout.write('y'*8192)"]`, `timeout_secs:
      Some(8.0)` → returns within 3 s with `Ok(Ok(code))`, `code != 0`, and not a timeout error.

- [ ] **Step 2: Implement.** Build the stdout `Vec<Sink>` from `stdout_fd` (`Capture`) and `sys_stdout_fd`
      (`Stream`), the same for stderr, and spawn both reader threads as `thread::spawn(move || pump(..))`. Delete the
      eight inline blocks and the `*_fd_clone` locals they needed. After the child exits and `drain_threads` returns
      `Finished`, check `sink_error`: if set, return `CommandExecutionError::new(msg)`. Then F-08's stdin result. On
      the timeout paths, return the timeout error as now.

- [ ] **Step 3: Run** `cargo test -p toolr-core --lib command::` → PASS, including Task 1's tests and the existing
      `test_sys_fd_streaming`, `test_sys_fd_with_capture` and `interactive_mode_ignores_capture_fds`.
      `uv run pytest tests/utils/command tests/context` → PASS.

- [ ] **Step 4: Commit.** `fix(command): keep ctx.run capture when the stream target fails`

### Task 3: Release notes, verification, archive

- [ ] **Step 1:** Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(command): note ctx.run sink failure handling`.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [ ] **Step 3:** Move both specs:

    ```sh
    git mv specs/2026-10-31-ctx-run-reader-drain-design.md \
        specs/2026-10-31-ctx-run-reader-drain-plan.md specs/archive/2026/
    ```

  Then fix the plan's `Spec:` link and set the design's status to implemented. Commit
  `docs(specs): archive ctx.run reader drain design and plan`.
