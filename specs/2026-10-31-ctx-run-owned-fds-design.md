# Give `ctx.run` its own copies of the output fds

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-41 (low).
- **Plan:** [`2026-10-31-ctx-run-owned-fds-plan.md`](./2026-10-31-ctx-run-owned-fds-plan.md)
- **Stacks on:** `fix/ctx-run-reader-drain` (F-40, `specs/2026-10-31-ctx-run-reader-drain-design.md`); merge that
  first.

## Problem

`CommandConfig` (`crates/toolr-core/src/command.rs`) carries the four output destinations as raw numbers: `RawFd` on
unix, `ThreadSafeHandle` (a `RawHandle` with hand-written `unsafe impl Send + Sync`) on Windows. `run_command_impl`
(`crates/toolr-py/src/lib.rs`) fills them straight from the `i32` values Python passes, which come from
`TemporaryFile.fileno()` and `sys.stdout.fileno()`.

Every write then does `unsafe { File::from_raw_fd(fd) }`, writes, and `std::mem::forget`s the `File` so Python's fd is
not closed. After F-40 that code lives in one place, `Sink::write`, but the problems are the same:

- **Ownership is borrowed, not held.** `from_raw_fd` requires an fd that is open and owned for the life of the
  `File`. Nothing checks that. A bad number from Python is undefined behaviour by contract, not an error.
- **A panic between `from_raw_fd` and `mem::forget` closes Python's fd.** Python closes it again later, or by then
  the number belongs to another file.
- **Detached threads write to fd numbers they don't own.** On a timeout, `toolr.utils.command.run` closes the capture
  files at once. F-21 kills the process tree and drains for 2 seconds, but a process that escaped the walk can keep a
  detached reader thread alive. That thread then writes to an fd number that a later `open()` in the Python process
  may have reused, so child output lands in an unrelated file.
- `ThreadSafeHandle` exists only to move raw handles across threads, and every field of `CommandConfig` and
  `Default` needs a `#[cfg(unix)]` and a `#[cfg(windows)]` variant.

## Decision

Duplicate each fd once, at the Python boundary, and pass owned `File`s from there on.

- New `crates/toolr-core/src/command/fds.rs` with
  `pub fn file_from_python_fd(fd: i32) -> io::Result<File>`. It is the only `unsafe` in the fd path.
    - Unix: `libc::fcntl(fd, F_DUPFD_CLOEXEC, 0)`. On `-1` it returns `io::Error::last_os_error()` (`EBADF` for a
      closed or bogus fd). On success the new fd is ours, so `OwnedFd::from_raw_fd` is sound, and the `File` comes
      from it. `CLOEXEC` keeps the copy out of the child.
    - Windows: reject a negative fd before any CRT call, then `libc::get_osfhandle(fd)`, rejecting `-1`, then
      `DuplicateHandle` with `bInheritHandle = FALSE` into an `OwnedHandle`, then `File`. This moves the existing
      `fd_to_handle` out of `toolr-py`. A closed but non-negative fd is rejected cleanly on unix only: on Windows
      `_get_osfhandle` may still reach the CRT's invalid-parameter handler, as any `_get_osfhandle` call in the process
      would. Python only passes fds it just got from `fileno()`, so this is a hardening limit, not a regression.
- `CommandConfig`'s four fields become `Option<File>` on every platform. The `#[cfg]` pairs in the struct and
  `Default` go away, and so do `ThreadSafeHandle` and its re-export from `toolr_core`.
- `Sink` holds a `File` and writes with `write_all`. No `from_raw_*`, no `mem::forget`, no `flush` (a `File` has no
  buffer). F-40's semantics stay as they are: per-sink kind, the first recorded error, and closing a sink by dropping
  its `File`.
- `run_command_impl` in `toolr-py` converts each `Option<i32>` with `file_from_python_fd` before building the config.
  A failure raises `CommandError("invalid stdout_fd 99: Bad file descriptor (os error 9)")` before anything is
  spawned. The existing "both fds must be provided together" checks stay first.
- A copy shares the file description with Python's fd, including the offset, so the `seek(0)` that
  `toolr.utils.command.run` does after the call still reads everything. A detached thread now writes to its own copy,
  which stays valid after Python closes the original. The output goes to the right temp file, which is already
  unlinked, and is freed when the thread ends.

### Rejected alternatives

- **`BorrowedFd::borrow_raw` + `libc::write`.** It removes the `mem::forget` but keeps borrowing an fd the threads
  can outlive, so the reuse hazard stays.
- **`ManuallyDrop<File>`.** It fixes the panic double close only. It still writes to a number it doesn't own.
- **Duplicate inside `run_command_internal`.** Then `CommandConfig` would still carry raw numbers, and every Rust
  caller and test would pass unchecked fds. The boundary is the one place that knows the fd came from outside.

## Scope and compatibility

- No runner schema change. `run_command_impl`'s Python signature stays `int | None` per fd.
- `toolr_core::CommandConfig` changes type and `ThreadSafeHandle` is removed. The crate has `publish = false`, and
  `toolr-py` and the command tests are the only users.
- Assumes F-08 (`stdin_thread`), F-21 (`tree::kill_tree`, `drain_threads`) and F-40 (`Sink`, `pump`, shared
  `sink_error`) are merged. This change only touches how a `Sink` gets and uses its destination.
- Docs: none.
- `UNRELEASED.md`:

  > `ctx.run` now writes captured and streamed output through its own copies of the file descriptors. When a command
  > timed out and a process it started kept writing, that output could land in an unrelated file opened later by the
  > same toolr command.
