# Keep `ctx.run` output flowing when one destination fails

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-40 (low, plausible).
- **Plan:** [`2026-10-31-ctx-run-reader-drain-plan.md`](./2026-10-31-ctx-run-reader-drain-plan.md)
- **Stacks on:** `fix/ctx-run-timeout-kills-tree` (F-21, `specs/2026-10-31-ctx-run-timeout-kills-tree-design.md`);
  merge that first.

## Problem

Each `ctx.run` reader thread in `run_command_internal` (`crates/toolr-core/src/command.rs`) copies the child's
stdout (or stderr) to up to two destinations: the capture file (`stdout_fd`/`stderr_fd`, the temp files behind
`capture_output=True`) and the stream target (`sys_stdout_fd`/`sys_stderr_fd`, toolr's own stdout and stderr). The
eight copy blocks all `break` out of the read loop on the first `write_all` or `flush` error.

The audit expected the child to block on a full pipe until a timeout fires. That does not happen. `break` drops the
reader, which closes the read end of the child's pipe, so the child's next write fails at once. Repro against the
current `toolr-py` extension: stream fds whose read ends are closed, and a child writing 300000 bytes, return `120`
(Python's exit code for a failed flush at shutdown) after 0.1 s.

What does happen is quieter and worse:

- **One failed destination stops the other.** With the default `stream_output=True` plus `capture_output=True`,
  `toolr cmd | head -1` breaks the stream pipe, and the capture stops at the same moment. `ctx.run` returns a truncated
  `result.stdout`, and the child is torn down by `SIGPIPE` or `EPIPE`, even though the caller asked for the full
  output.
- **Capture failures are silent.** A failed write to the capture file (disk full, a bad fd) truncates
  `result.stdout` and kills the child, and `ctx.run` returns the child's exit status as if nothing went wrong. A
  child killed by `SIGPIPE` reports `-1`.

## Decision

Treat each destination separately, and never lose captured output without saying so.

- A small `Sink` type in `command.rs` wraps one destination: its kind (`Capture` or `Stream`), its raw fd or handle,
  and whether it is still open. `Sink::write(&mut self, buf) -> io::Result<()>` holds the existing
  `from_raw_fd` + `mem::forget` code, so the eight blocks become one call per sink. F-41 replaces the inside of
  `Sink`.
- **Capture sink fails:** record the error, close that sink, and keep reading and discarding, so the child runs to
  completion. Once the child has exited, `run_command_internal` returns
  `CommandExecutionError("Failed to write captured stdout: {e}")` (or `stderr`). A truncated capture never reaches the
  caller as a success.
- **Stream sink fails with `BrokenPipe`:** close that sink without an error. Whoever was reading toolr's output went
  away, which is normal in a pipeline. Any other stream error is recorded and returned like a capture error.
- **No open sinks left** (the stream sink broke and there is no capture): stop reading and drop the pipe, as today.
  The child gets `SIGPIPE`, exactly as it would in a shell pipeline. Draining for ever would keep a `yes`-style
  command running after `| head` has gone.
- The recorded error goes into an `Arc<Mutex<Option<String>>>` shared by both reader threads (the first error wins).
  The threads keep returning `()`, so F-21's `drain_threads` predicate and the joins after it do not change.
- The error is checked after the child exits and the threads are drained, before F-08's stdin result. A timeout
  error still wins over a sink error.

### Rejected alternatives

- **Kill the child on the first write error** (one of the audit's options). It turns a broken `| head` into a dead
  capture, which is the bug.
- **Keep draining with no open sinks.** A command that writes for ever would never end once `| head` exits.
- **Return the error from the reader threads.** Cleaner, but it changes the `JoinHandle` type F-21's drain loop takes.
  Shared state keeps the stack's changes apart.

## Scope and compatibility

- No runner schema change. The pyo3 signature does not change.
- Assumes F-08 (stdin thread) and F-21 (`drain_threads`, `tree::kill_tree`) are merged.
- Behaviour change: a failed capture write now raises `CommandError` instead of returning a short result.
- Docs: none. The capture and streaming options are documented, failure handling is not.
- `UNRELEASED.md`:

  > `ctx.run(..., capture_output=True)` keeps capturing when its streamed copy of the output can't be written, for
  > example when toolr's output is piped into `head`. Both copies used to stop together, so the command was cut off
  > and `result.stdout` came back truncated without a warning. A failure to write the captured output itself now
  > raises `CommandError` instead of returning partial output.
