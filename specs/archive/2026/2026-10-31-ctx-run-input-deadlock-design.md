# Write `ctx.run` stdin from its own thread

- **Status:** implemented
- **Origin:** whole-project review finding F-08 (medium).
- **Plan:** [`2026-10-31-ctx-run-input-deadlock-plan.md`](./2026-10-31-ctx-run-input-deadlock-plan.md)

## Problem

`run_command_internal` (`crates/toolr-core/src/command.rs`) spawns the child, then writes the whole `input` payload
with a synchronous `stdin.write_all(input_data)` before it starts the stdout and stderr reader threads. If the child
writes more than a pipe buffer (64 KiB on Linux and macOS) to stdout or stderr before it reads stdin, both sides block:
the child on its full stdout pipe, the parent on its full stdin pipe.

The timeouts don't help. The `timeout_secs` and `no_output_timeout_secs` checks live in the poll loop that runs after
the write, so the call never reaches them. Repro, against the current `toolr-py` extension:

```python
run_command_impl(
    [sys.executable, "-c",
     "import sys; sys.stdout.write('x'*200000); sys.stdout.flush(); sys.stdin.buffer.read()"],
    input=b"y" * 200000, stdout_fd=out.fileno(), stderr_fd=err.fileno(), timeout_secs=3,
)
```

It is still blocked after 8 seconds and has to be killed. `ctx.run(..., input=..., capture_output=True)` takes the same
path.

A second, smaller problem sits in the same code. When the child exits or closes stdin before reading all the input
(`head -c 1`), `write_all` fails with `BrokenPipe` and `ctx.run` raises `CommandError: Failed to write to stdin`.
Whether that happens depends on the payload size against the pipe buffer, not on anything the caller controls.

## Decision

Write stdin from a dedicated thread, the way `std::process::Child::wait_with_output` and Python's
`subprocess.communicate` do.

- Spawn the stdout and stderr reader threads first, then a `stdin_thread` that owns `ChildStdin`, calls
  `write_all(&input)`, and drops the handle to send EOF. The thread takes the payload by value (`config.input.take()`),
  so nothing is copied.
- The thread returns `io::Result<()>`. `BrokenPipe` maps to `Ok(())`: the child stopped reading, which is its choice,
  and its exit status tells the caller what happened. This matches `subprocess.communicate`. Any other error is kept.
- The poll loop runs unchanged, so both timeouts now cover the stdin write. On timeout the child is killed, the write
  fails with `BrokenPipe`, and the thread ends.
- After the child exits, join `stdin_thread` before the readers. A kept error becomes
  `CommandExecutionError("Failed to write to stdin: {e}")`, as today, but only after the child has been reaped.
- In interactive mode `input` is rejected in Python and stdin is inherited, so there is no thread.

### Rejected alternatives

- **Non-blocking stdin with `poll`/`select` in the wait loop.** One thread fewer, but it needs a Windows
  overlapped-I/O path too. A thread is what `std` and CPython already do.
- **Keep the synchronous write but start the readers first.** That removes this deadlock, but a child that never reads
  stdin still blocks the write past both timeouts.
- **Keep raising on `BrokenPipe`.** It stays timing-dependent once the write moves off the main thread. Ignoring it
  is the only rule that gives the same answer every run.

## Scope and compatibility

- No runner schema change. `run_command_impl`'s Python signature does not change.
- Internal to `run_command_internal`. The stacked follow-ups (F-21 process-tree timeout, F-40 sink errors, F-41 owned
  fds) build on the thread layout set here: three I/O threads, joined after the child exits.
- Docs: none. `docs/writing-commands/context.md` doesn't describe the stdin write.
- `UNRELEASED.md`:

  > `ctx.run(..., input=...)` no longer hangs when the command writes a lot of output before it reads its input. toolr
  > now feeds stdin from a separate thread, so the timeouts apply to that phase too. A command that exits without
  > reading all of its input no longer raises "Failed to write to stdin"; its exit code is returned as usual.
