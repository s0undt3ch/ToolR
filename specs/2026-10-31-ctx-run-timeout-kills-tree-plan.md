# ctx.run Timeout Kills Tree Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `ctx.run` timeout kills the child's whole process tree, and both timeouts still hold after the child exits
while something it started keeps the output pipes open (F-21).

**Architecture:** A new `command/tree.rs` walks a `(pid, ppid)` snapshot to find and kill the child's descendants
before the child is reaped. A `drain_threads` loop replaces the unbounded joins and keeps checking both timeouts.

**Tech Stack:** Rust, `libc` (Linux `/proc`, macOS `proc_listallpids`/`proc_pidinfo`), `winapi` `tlhelp32`.

**Spec:** [`specs/2026-10-31-ctx-run-timeout-kills-tree-design.md`](./2026-10-31-ctx-run-timeout-kills-tree-design.md)

## Global Constraints

- Branch stacks on `fix/ctx-run-input-deadlock` (F-08). `run_command_internal` already has the `stdin_thread` from
  that branch; this plan only touches the timeout branches, the joins, and the new module.
- No process groups and no change to signal handling. See the spec's rejected alternatives.
- Every new test that spawns a child goes through `bounded_run_command(config, Duration::from_secs(10))`.
- Tests that spawn a grandchild use Python's `subprocess.Popen`, so they run on every platform. The `sh -c` tests
  that match the report are `#[cfg(unix)]`.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **Enumerate before reaping.** `kill_tree` must snapshot while the child is unreaped, or the descendants are
   already reparented. Task 2, `timeout_kills_grandchild`.
2. **The drain doesn't change the no-timeout case.** A command that leaves a background process holding stdout, with
   no timeout set, still waits for it. Task 2, `no_timeout_still_waits_for_background_writer`.
3. **PID reuse.** The walk only signals pids whose ppid chain leads to the live, unreaped child, and `SIGSTOP` freezes
   them first. Check that the snapshot is never reused across passes for signalling.
4. **Ctrl-C unchanged.** No `process_group`, `setsid` or `pre_exec` is added to `Command`.

---

### Task 1: Reproduce the reported shape (plausible finding)

**Files:**

- Test: `crates/toolr-core/src/command/command_test.rs` (`mod tokio_tests`)

**Interfaces:** test helpers only.

- `fn process_alive(pid: u32) -> bool`: unix `libc::kill(pid as i32, 0) == 0`; Windows `OpenProcess` +
  `GetExitCodeProcess == STILL_ACTIVE`.

- `fn wait_until_dead(pid: u32, within: Duration) -> bool`: polls `process_alive` every 20 ms.

- `fn read_pid(file: &mut NamedTempFile) -> u32`: rewinds and parses the first line.

- [ ] **Step 1: Write the tests.**
    - `reported_shape_sh_sleep_timeout_returns` (`#[cfg(unix)]`): args `["sh", "-c", "sleep 999"]`,
      `timeout_secs: Some(1.0)` → within the 10 s bound, an inner `Err` containing `"timed out after"`. This is the
      audit's exact shape.
    - `timeout_kills_grandchild`: args `[python, "-c", "import subprocess, sys, time;
      p = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)']); print(p.pid, flush=True);
      time.sleep(60)"]`, capture fds, `timeout_secs: Some(1.0)` → timeout error, and
      `wait_until_dead(read_pid(..), 2s)` is true.
    - `timeout_kills_grandchild_sh` (`#[cfg(unix)]`): args `["sh", "-c", "sleep 60 & echo $!; wait"]`, capture fds,
      `timeout_secs: Some(1.0)` → same assertions.
    - `timeout_holds_when_background_child_keeps_pipe_open`: args `[python, "-c", "import subprocess, sys;
      p = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)']); print(p.pid, flush=True)"]`,
      capture fds, `timeout_secs: Some(1.0)` → the call returns in under 5 s with an inner `Err` containing
      `"timed out after"`, not `Ok(0)` after 30 s. Clean up with `kill(pid)` at the end of the test.
    - `timeout_holds_when_background_child_keeps_pipe_open_sh` (`#[cfg(unix)]`): args
      `["sh", "-c", "sleep 30 &"]`, `timeout_secs: Some(1.0)` → same assertion (the repro from the spec).

- [ ] **Step 2: Run** `cargo test -p toolr-core --lib command::` and record the result.
      Expected: `reported_shape_sh_sleep_timeout_returns` passes today (the audit's hang does not reproduce), the
      grandchild tests fail (`process_alive` still true) and the background tests fail the 5 s check.
      **Close criteria:** if every test passes on the stacked base, F-21 is already fixed. Commit only the tests as
      `test(command): cover ctx.run timeouts with child process trees`, set the design's status to "closed: not
      reproducible", archive both specs (Task 3, Step 3) and stop.

- [ ] **Step 3: Commit** the failing tests with the fix in Task 2 (don't commit a red tree on its own).

### Task 2: Kill the tree and bound the drain

**Files:**

- Create: `crates/toolr-core/src/command/tree.rs`
- Modify: `crates/toolr-core/src/command.rs` (`mod tree;`, the two timeout branches, the joins)
- Modify: `crates/toolr-core/Cargo.toml` (Windows `winapi` features: add `"tlhelp32"`)
- Test: `crates/toolr-core/src/command/command_test.rs`, `crates/toolr-core/src/command/tree.rs` `mod tests`

**Interfaces:**

```rust
// tree.rs
pub(crate) fn kill_tree(child: &mut std::process::Child);
fn descendants(root: u32, table: &[(u32, u32)]) -> Vec<u32>; // pure; (pid, ppid) pairs
fn process_table() -> std::io::Result<Vec<(u32, u32)>>;      // per-platform snapshot

// command.rs
enum DrainOutcome { Finished, TimedOut(Box<dyn Error + Send + Sync>) }
fn drain_threads(
    all_finished: &dyn Fn() -> bool,           // polled every 10 ms
    deadline: Option<Instant>,                 // start_time + timeout_secs; None = no bound
    no_output: Option<(Arc<Mutex<Instant>>, Duration)>,
) -> DrainOutcome;
```

`drain_threads` polls a predicate instead of owning the handles, because the readers return `()` and F-08's
`stdin_thread` returns `io::Result<()>`. The caller passes
`&|| readers.iter().all(JoinHandle::is_finished) && stdin_thread.as_ref().is_none_or(JoinHandle::is_finished)`.
On `Finished` it joins each handle (keeping the stdin result for the existing "Failed to write to stdin" error); on
`TimedOut` it drops them, which detaches the threads.

- [ ] **Step 1: Unit-test `descendants`** in `tree.rs`:
      `descendants_finds_nested_children`: table `[(10, 1), (11, 10), (12, 11), (13, 1)]` → `{11, 12}` for root
      `10`; `descendants_ignores_unrelated`: root `13` → empty; `process_table_includes_self`: the snapshot contains
      `std::process::id()` with a nonzero ppid (`#[cfg(any(target_os = "linux", target_os = "macos", windows))]`).

- [ ] **Step 2: Implement `tree.rs`.** `process_table` per spec section 1; other unix targets return an empty table.
      `kill_tree`: on unix, loop at most 5 times: take a snapshot, `SIGSTOP` every descendant not yet stopped, stop
      when a pass adds nothing; then `SIGKILL` all, `child.kill()`, `child.wait()`. On Windows, one snapshot,
      `OpenProcess(PROCESS_TERMINATE)` + `TerminateProcess(h, 1)` each, then `child.kill()`, `child.wait()`. Ignore
      per-pid errors (`ESRCH`, access denied).

- [ ] **Step 3: Wire it in.** In both timeout branches of the poll loop, replace `child.kill()` + `child.wait()` with
      `tree::kill_tree(&mut child)`, then
      `drain_threads(&all_finished, Some(Instant::now() + Duration::from_secs(2)), None)` and return the timeout error
      whatever the outcome. After a normal exit, call `drain_threads` with the real `start_time + timeout_secs` and the
      `no_output_timeout_secs` watch; on `TimedOut(e)` return `e` (threads are detached by dropping their handles). Then
      handle F-08's stdin result as before.

- [ ] **Step 4: Add** `no_timeout_still_waits_for_background_writer` (`#[cfg(unix)]`): args
      `["sh", "-c", "(sleep 1; echo late) &"]`, capture fds, no timeouts → `Ok(Ok(0))` after at least 1 s, and the
      capture contains `late`.

- [ ] **Step 5: Run** `cargo test -p toolr-core --lib command::` → PASS, including Task 1's tests and the existing
      `test_timeout_exception` and `test_no_output_timeout`. On a Windows-capable checkout, `cargo check --target
      x86_64-pc-windows-msvc -p toolr-core` if the target is installed; otherwise CI's Windows job covers it.

- [ ] **Step 6: Commit.** `fix(command): kill the process tree on ctx.run timeouts`

### Task 3: Docs, release notes, verification, archive

- [ ] **Step 1:** Add the paragraph from the spec to `docs/writing-commands/context.md` under "Running subprocesses",
      and the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(command): document ctx.run timeouts killing the process tree`.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
      `uv run mkdocs build --strict` → green.
- [ ] **Step 3:** Move both specs:

    ```sh
    git mv specs/2026-10-31-ctx-run-timeout-kills-tree-design.md \
        specs/2026-10-31-ctx-run-timeout-kills-tree-plan.md specs/archive/2026/
    ```

  Then fix the plan's `Spec:` link and set the design's status to implemented. Commit
  `docs(specs): archive ctx.run timeout kills tree design and plan`.
