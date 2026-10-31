# Kill the whole process tree when a `ctx.run` timeout fires

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-21 (medium, plausible).
- **Plan:** [`2026-10-31-ctx-run-timeout-kills-tree-plan.md`](./2026-10-31-ctx-run-timeout-kills-tree-plan.md)
- **Stacks on:** `fix/ctx-run-input-deadlock` (F-08, `specs/2026-10-31-ctx-run-input-deadlock-design.md`); merge that
  first.

## Problem

When `timeout_secs` or `no_output_timeout_secs` expires, `run_command_internal`
(`crates/toolr-core/src/command.rs`) calls `child.kill()` and `child.wait()` on the direct child and returns the
timeout error. Nothing else in the child's process tree is touched. After a normal exit, it joins the stdout and
stderr reader threads with no bound at all.

The audit's version ("the join hangs after the timeout fires") does not reproduce: the timeout branches return before
the join, so the reader threads are just dropped. Two other problems do reproduce, against the current `toolr-py`
extension:

1. **The grandchild survives the timeout.** `run_command_impl(["sh", "-c", "sleep 23; true"], timeout_secs=1)` raises
   `CommandTimeoutError` after 1.1 s, but `sleep 23` is still running afterwards (`pgrep` finds it). It keeps the
   stdout and stderr pipes open, so the reader threads also keep running until it exits.
2. **A timeout is not enforced once the child has exited.** `run_command_impl(["sh", "-c", "sleep 24 &"],
   timeout_secs=1)` returns `0` after 24.3 s. `sh` exits at once, the backgrounded `sleep` holds the pipes, and the
   unbounded join waits for it.

The leaked reader threads in case 1 have a sharper edge. They still hold the capture fd numbers that Python passed in.
On the error path, `toolr.utils.command.run` closes those temp files, so a later `open()` can reuse the fd number, and
a leaked thread then writes child output into an unrelated file. F-41 removes that hazard by duplicating the fds; this
change makes leaks rare, F-41 makes them harmless.

`test_timeout_exception` in `crates/toolr-core/src/command/command_test.rs` runs a single Python process, so it can't
see either case.

## Decision

Kill the child's descendants by walking the process table on timeout, and keep enforcing both timeouts while the
reader threads drain.

### 1. Kill the tree, not just the child

New module `crates/toolr-core/src/command/tree.rs` with `pub(crate) fn kill_tree(child: &mut Child)`. It runs while
the child is still alive and unreaped, so its descendants still point at it:

- Unix: list the descendants of `child.id()` from a `(pid, ppid)` snapshot and send each `SIGSTOP`, so none of them
  can fork more. Take snapshots until one finds no new pid (at most 5 passes). Then `SIGKILL` every pid found, kill
  the child, and `wait()` it.
    - Linux: read `/proc/<pid>/stat` for every numeric entry under `/proc`. The ppid is the fourth field after the
      closing `)` of the command name.
    - macOS: `libc::proc_listallpids`, then `libc::proc_pidinfo(pid, PROC_PIDTBSDINFO, ..)` for `pbi_ppid`. Both are
      in `libc` 0.2.191.
    - Other unix targets: no snapshot; kill the child only, as today.
- Windows: `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` gives `th32ParentProcessID`. `TerminateProcess` each
  descendant, then kill the child. Windows has no `SIGSTOP`, so a process forked mid-walk can escape. That is
  acceptable for a timeout path.

Both timeout branches call `kill_tree` instead of `child.kill()` + `child.wait()`.

### 2. Bounded drain

Replace the bare `join()` calls with a `drain_threads` loop that polls `JoinHandle::is_finished()` every 10 ms. It
covers the reader threads and F-08's `stdin_thread`, and checks both timeouts with the same clocks as the main poll
loop (`start_time` and `last_output`):

- After a normal exit: if a timeout expires while a reader is still running, detach the threads and return that
  timeout error. The child's status is dropped: the caller asked for the whole call to finish within the limit, and
  it didn't. With no timeouts set, the drain waits as long as it takes, as today. A command that starts a daemon
  holding stdout still behaves the same.
- After `kill_tree`: drain with a fixed 2-second grace, then detach. Once the tree is dead, the pipes hit EOF at once,
  so the grace only matters when a process escaped the walk.

A process that was already reparented before the timeout (case 2: `sh` exited first) cannot be found by its ppid. The
drain bound still stops the call on time, but that process keeps running. The docs say so.

### Rejected alternatives

- **A new process group (`process_group(0)`) and `killpg`**, as the audit suggests. Non-interactive `ctx.run` inherits
  stdin, and `run_command_impl` holds the GIL for the whole call. A child in a background group stops with `SIGTTIN`
  when it reads the terminal (an ssh passphrase prompt under `git push`, `sudo`). It also no longer gets the
  terminal's `SIGINT`, and Python can't raise `KeyboardInterrupt` until the Rust call returns. Ctrl-C would stop
  working for every `ctx.run`. Fixing that needs signal forwarding and a `SIGTTIN` answer, which is a bigger change
  than this bug.
- **Windows job objects.** They catch descendants that are reparented, but `std::process::Command` can't create the
  child suspended. The child would be assigned to the job after it starts, which has the same race as the snapshot.
  One mechanism on every platform is easier to test.
- **Bound only the post-kill drain and keep the unbounded join after a normal exit.** That leaves case 2 as it is.

## Scope and compatibility

- No runner schema change. The pyo3 signature does not change.
- Assumes F-08 is merged: `run_command_internal` owns an optional `stdin_thread` that is joined with the readers.
  `drain_threads` takes it too.
- Not covered: signal handling for `toolr`'s own Python child (F-29, `execute/signals.rs`). The fd-reuse hazard of
  detached threads is fixed in F-41 (`refactor/ctx-run-owned-fds`).
- `crates/toolr-core/Cargo.toml`: add `"tlhelp32"` to the Windows `winapi` features.
- Docs: `docs/writing-commands/context.md`, "Running subprocesses", gets one paragraph. On timeout, `ctx.run` kills
  the command and everything it started. A process that the command started and left running in the background
  before it exited can't be tracked, but the timeout still returns on time.
- `UNRELEASED.md`:

  > When a `ctx.run` timeout fires, toolr now kills the processes the command started as well as the command itself.
  > It used to kill only the direct child, so `ctx.run("sh", "-c", "sleep 999", timeout_secs=1)` left `sleep`
  > running. The timeouts now also hold when the command exits but something it started in the background keeps its
  > output open; `ctx.run` used to wait for that process with no limit.
