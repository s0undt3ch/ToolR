# Pass `ctx.run` its output files as owned `File`s

- **Status:** design agreed, not implemented (revised 2026-11-04: #645 already took the fd ownership)
- **Origin:** whole-project review finding F-41 (low).
- **Plan:** [`2026-10-31-ctx-run-owned-fds-plan.md`](./2026-10-31-ctx-run-owned-fds-plan.md)
- **Stacks on:** `fix/ctx-run-reader-drain` (F-40); merge that first.

## What #645 already fixed

F-41 reported `from_raw_fd` + `mem::forget` copied into eight write blocks, and detached reader threads writing to
fd numbers Python may have closed and reused. #645 (`cf8edc77`) fixed both. `own_sink` in
`crates/toolr-core/src/command.rs` duplicates each fd (`F_DUPFD_CLOEXEC` on unix, `try_clone_to_owned` on Windows)
before the spawn, each reader owns its `File`s, and the eight blocks became one `spawn_reader`.

## What is left

`CommandConfig` still carries the four destinations as raw numbers until `run_command_internal` duplicates them:

- `RawFd` on unix and `ThreadSafeHandle` on Windows. `ThreadSafeHandle` is a `RawHandle` wrapper whose only job is a
  hand-written `unsafe impl Send + Sync`, re-exported from `toolr_core`.
- Every one of the four fields, and each line of `Default`, needs a `#[cfg(unix)]` and a `#[cfg(windows)]` copy.
- `toolr-py` turns Python's `int` into a `RawHandle` with its own `fd_to_handle` (`libc::get_osfhandle`), so the
  fd-to-handle step lives in one crate and the duplication in another.
- A bad fd is caught inside `run_command_internal`, as `Failed to duplicate an output handle: …`, which doesn't say
  which argument was wrong.

## Decision

Duplicate each fd once, where Python hands it over, and pass owned `File`s from there on.

- `toolr-core` gets `pub fn file_from_python_fd(fd: i32) -> io::Result<File>` in `command.rs`, next to the code it
  replaces:
    - Unix: `fcntl(fd, F_DUPFD_CLOEXEC, 0)` into an `OwnedFd`, as `own_sink` does today.
    - Windows: reject a negative fd, then `libc::get_osfhandle(fd)` (rejecting `-1`), then
      `BorrowedHandle::borrow_raw(..).try_clone_to_owned()`. This moves `fd_to_handle` out of `toolr-py`.
- `CommandConfig`'s four fields become `Option<File>` on every platform. The `#[cfg]` pairs in the struct and
  `Default`, `ThreadSafeHandle`, its re-export and `own_sink` all go.
- `run_command_internal` hands the config's files straight to the readers. In interactive mode they're dropped
  unused, as the raw fds are today.
- `toolr-py`'s `run_command_impl` converts each `Option<i32>` with `file_from_python_fd` after its existing "both fds
  together" checks, and raises `CommandError("invalid stdout_fd 99: Bad file descriptor (os error 9)")` (naming the
  argument) before anything is spawned.
- A duplicate shares the file description with Python's fd, offset included, so the `seek(0)` that
  `toolr.utils.command.run` does after the call still reads everything.

### Rejected alternatives

- **Leave it.** The hazard is fixed, but `ThreadSafeHandle`'s `unsafe impl`s and the `#[cfg]` pairs stay for no
  reason, and the next change to `CommandConfig` has to touch both copies of every field.
- **Duplicate in `toolr-py`.** `crates/toolr-py/src` is excluded from coverage and has no Rust tests, so the only
  `unsafe` in the fd path would be untested. In `toolr-core` it gets unit tests on every platform CI runs.

## Scope and compatibility

- No runner schema change. The pyo3 signature does not change. `ThreadSafeHandle` was public in `toolr_core`, but
  only `toolr-py` used it, and the two crates ship at the same version.
- User-visible change: an invalid fd now raises `CommandError` naming the argument. Python only passes fds it just got
  from `fileno()`, so this is a hardening change, and needs no release note (`Release-Notes: skip`).
