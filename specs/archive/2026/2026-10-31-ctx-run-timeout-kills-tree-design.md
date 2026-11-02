# Kill the whole process tree when a `ctx.run` timeout fires

- **Status:** implemented
- **Origin:** whole-project review finding F-21 (medium, plausible).
- **Plan:** [`2026-10-31-ctx-run-timeout-kills-tree-plan.md`](./2026-10-31-ctx-run-timeout-kills-tree-plan.md)
- **Stacks on:** `fix/ctx-run-input-deadlock` (F-08), now merged into `main`.

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
    - Linux: read `/proc/<pid>/stat` for every numeric entry under `/proc`. The ppid is the second field after the
      last `)`, which closes the command name (`pid (comm) state ppid ...`).
    - macOS: `libc::proc_listallpids`, then `libc::proc_pidinfo(pid, PROC_PIDTBSDINFO, ..)` for `pbi_ppid`. Both are
      in `libc` 0.2.191.
    - Other unix targets: no snapshot; kill the child only, as today.
- Windows: `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` gives `th32ParentProcessID`. `TerminateProcess` each
  descendant, then kill the child. Windows has no `SIGSTOP`, so a process forked mid-walk can escape. That is
  acceptable for a timeout path. Windows keeps a dead parent's pid as the ppid, and the child may have reused it, so
  only a process created after the child is terminated (`GetProcessTimes`).

Both timeout branches call `kill_tree` instead of `child.kill()` + `child.wait()`.

### 2. Bounded drain

Replace the bare `join()` calls with wait loops that poll `JoinHandle::is_finished()` every 10 ms. They cover the
reader threads and F-08's `stdin_thread`. After a normal exit they check both timeouts against the same start time
and output clock as the main poll loop, but only while no reader is writing to a sink (review round 2, below):

- After a normal exit: if a timeout expires while a reader is still running, detach the threads and return that
  timeout error. The child's status is dropped: the caller asked for the whole call to finish within the limit, and
  it didn't. With no timeouts set, the drain waits as long as it takes, as today. A command that starts a daemon
  holding stdout still behaves the same.
- After `kill_tree`: drain with a fixed 0.5-second grace (2 seconds before review round 2), then detach. Once the tree is dead, the pipes hit EOF at once,
  so the grace only matters when a process escaped the walk.

A process that was already reparented before the timeout (case 2: `sh` exited first) cannot be found by its ppid. The
drain bound still stops the call on time, but that process keeps running. The docs say so.

### Decided: a background holder after exit 0 is a timeout

Resolved 2026-11-02 by the maintainer. When the child exits 0 but a process it started keeps the output pipe open,
and a timeout runs out while `ctx.run` waits for that pipe, `ctx.run` raises the timeout error
(`CommandTimeoutError` or `CommandTimeoutNoOutputError`). It does not report success. With no timeout set it still
waits for the pipe to close, as before.

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
  The wait loops cover it too.
- Not covered: signal handling for `toolr`'s own Python child (F-29, `execute/signals.rs`).
- Owned output fds, pulled forward from F-41. The bounded drain detaches reader threads after a normal exit, which
  `main` never did, and `toolr.utils.command.run` then closes its capture files. A detached thread writing through the
  caller's raw fd number put its output in whatever file reused it next. Each reader thread now writes to its own
  duplicate (`fcntl(F_DUPFD_CLOEXEC)` on Unix, `DuplicateHandle` via `try_clone_to_owned` on Windows), made before
  the spawn. F-41 (`refactor/ctx-run-owned-fds`) must rebase onto this and drop what it duplicates.
- Review round 2:
    - A reader blocked writing to a slow sink isn't a holder: the timeouts after a normal exit, and the no-output
      timeout throughout, only count while no reader is writing, and the output clock is stamped after each write
      too. A command that finished in time therefore returns its status with all output delivered.
    - Giving up on the readers sets a shared `detached` flag. A detached reader stops at its next read and drops the
      pipe and its sink copies, so an escaped writer gets EPIPE, as it did on `main` once the call returned.
    - With that, the kill grace only flushes output the dead tree already wrote, so it is 0.5 s rather than 2 s. It
      still extends the GIL hold by that much when a process escapes; the docs say so.
- Review round 3 (no behaviour change): the timeout rules are documented by phase on `Timeouts`, one `expired`
  check gates both timeouts on idle readers, a drop guard sets `detached` on every return path, and the
  `drain_threads` helper became two plain wait loops.
- Windows checks each edge of the walk, not just the root: a process must be no older than the tree member it names
  as its parent. Unix reads each ppid again just before the `SIGSTOP`.
- `crates/toolr-core/Cargo.toml`: `command` builds without `full`, because toolr-py links it alone, so its new
  dependencies are not optional. `libc` becomes a plain dependency, `winapi` a plain Windows dependency with the
  features the code uses (`handleapi`, `processthreadsapi`, `tlhelp32`, `winnt`; the tests add `namedpipeapi`,
  `fileapi` and `minwinbase` as dev-dependency features), and both join the `toolr-core-lean-deps` allowlist in `.pre-commit-config.yaml`, as `CONTRIBUTING.md`
  describes.
- Docs: `docs/writing-commands/context.md`, "Running subprocesses", gets one paragraph. On timeout, `ctx.run` kills
  the command and everything it started. A process that the command started and left running in the background
  before it exited can't be tracked, but the timeout still returns on time.
- Release note (`release-notes/`), marked breaking in review: a timed call that used to return the exit code after
  waiting out a background holder now raises. The headline example uses `sleep 999; true`, because `sh -c "sleep 999"`
  execs `sleep` and has no grandchild. Original draft:

  > When a `ctx.run` timeout fires, toolr now kills the processes the command started as well as the command itself.
  > It used to kill only the direct child, so `ctx.run("sh", "-c", "sleep 999", timeout_secs=1)` left `sleep`
  > running. The timeouts now also hold when the command exits but something it started in the background keeps its
  > output open; `ctx.run` used to wait for that process with no limit.
