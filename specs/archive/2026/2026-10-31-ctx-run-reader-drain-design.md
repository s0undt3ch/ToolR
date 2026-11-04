# Keep `ctx.run` output flowing when one destination fails

- **Status:** design agreed, not implemented (revised 2026-11-04 against the reader rewrite in #645)
- **Origin:** whole-project review finding F-40 (low, plausible).
- **Plan:** [`2026-10-31-ctx-run-reader-drain-plan.md`](./2026-10-31-ctx-run-reader-drain-plan.md)

## Problem

`spawn_reader` (`crates/toolr-core/src/command.rs`) copies one child pipe (stdout or stderr) to up to two
destinations: the capture file (`stdout_fd`/`stderr_fd`, the temp files behind `capture_output=True`) and the stream
target (`sys_stdout_fd`/`sys_stderr_fd`, toolr's own stdout and stderr). Since #645 each reader owns duplicates of
both and writes them in one loop:

```rust
let failed = state.detached.load(..) || sinks.iter_mut().any(|sink| sink.write_all(..).and_then(|()| sink.flush()).is_err());
if failed { break; }
```

The first failing write ends the loop for both destinations. `break` drops the reader, which closes the read end of
the child's pipe, so the child's next write fails at once: it doesn't hang. Two things go wrong:

- **One failed destination stops the other.** With the default `stream_output=True` plus `capture_output=True`,
  `toolr cmd | head -1` breaks the stream pipe, and the capture stops at the same moment. `ctx.run` returns a truncated
  `result.stdout`, and the child dies of `SIGPIPE` or `EPIPE`, even though the caller asked for the full output.
- **Capture failures are silent.** A failed write to the capture file (disk full, a bad fd) truncates
  `result.stdout` and kills the child, and `ctx.run` returns the child's exit status as if nothing went wrong.

## Decision

Treat each destination separately, and never lose captured output without saying so.

- `spawn_reader` takes the two destinations by role, `capture: Option<File>` and `stream: Option<File>`, plus the
  stream name (`"stdout"` or `"stderr"`) for error messages. The fixed `[capture, stream]` array order goes away.
- **Capture write fails:** record `Failed to write captured {name}: {e}`, drop the capture file, and keep reading,
  so the child runs to completion instead of dying mid-write.
- **Stream write fails with `BrokenPipe`:** drop the stream file without recording anything. Whoever was reading
  toolr's output went away, which is normal in a pipeline. Rust maps both `EPIPE` and Windows' `ERROR_NO_DATA` to
  `BrokenPipe`.
- **Stream write fails otherwise:** record `Failed to write {name}: {e}` and drop the stream file.
- **Nothing left to write to:** if a capture write failed, keep reading and discarding until EOF, as above.
  Otherwise (the stream broke and there's no capture) stop reading and drop the pipe, as today, so the child gets
  `SIGPIPE` exactly as it would in a shell pipeline. Draining for ever would keep a `yes`-style command running
  after `| head` has gone.
- Detaching (`state.detached`) still stops the loop at once, whatever the sinks' state.
- The first recorded error goes into a new `error: Mutex<Option<String>>` field on `ReaderState`, which both readers
  already share. The threads keep returning `()`, so the drain loop and joins don't change.
- `run_command_internal` checks it after the readers are joined, and returns it as a `CommandExecutionError`
  (Python sees `CommandError`) before the stdin writer's result. A timeout still returns earlier and wins.

### Rejected alternatives

- **Kill the child on the first write error.** It turns a broken `| head` into a dead capture, which is the bug.
- **Keep draining with no destination and no capture error.** A command that writes for ever would never end once
  `| head` exits.
- **Return the error from the reader threads.** It changes the `JoinHandle` type the drain loop and `all_finished`
  use, for no gain over the state the readers already share.

## Scope and compatibility

- No runner schema change. The pyo3 signature does not change.
- Behaviour change: a failed capture write now raises `CommandError` instead of returning a short result.
- Docs: none. The capture and streaming options are documented, failure handling is not.
- Release note (`toolr release-notes add`):

  > `ctx.run(..., capture_output=True)` keeps capturing when its streamed copy of the output can't be written, for
  > example when toolr's output is piped into `head`. Both copies used to stop together, so the command was cut off
  > and `result.stdout` came back truncated without a warning. A failure to write the captured output itself now
  > raises `CommandError` instead of returning partial output.
