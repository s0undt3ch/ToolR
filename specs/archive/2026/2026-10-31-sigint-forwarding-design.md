# Stop forwarding SIGINT to the runner

- **Status:** implemented
- **Origin:** whole-project review finding F-29 (low, plausible; reproduced below).
- **Plan:** [`2026-10-31-sigint-forwarding-plan.md`](./2026-10-31-sigint-forwarding-plan.md)

## Problem

While a command runs, `toolr` waits in `crates/toolr-core/src/execute/signals.rs::wait_with_signals`. It registers
SIGINT and SIGTERM with `signal_hook`, stores the last one in a single `AtomicI32`, and every 50 ms re-sends it to the
runner with `libc::kill(child_pid, sig)`.

The runner stays in toolr's process group, which is the terminal's foreground group. Ctrl-C makes the terminal send
SIGINT to the whole group, so Python gets it once from the terminal and again from toolr up to 50 ms later. The second
one lands in whatever `except KeyboardInterrupt:` or `finally:` block the first one started, and kills it.

Repro on macOS, under a real pseudo-terminal, with this command in `tools/sig.py`:

```python
@group.command
def cleanup(ctx: Context) -> None:
    try:
        time.sleep(10)
    finally:
        time.sleep(0.5)
        print("cleaned", flush=True)
```

Pressing Ctrl-C a second into `toolr sig cleanup` exits 130 after 0.1 s, three runs out of three, and never prints
`cleaned`. The same body run as bare `python3 -c` under the same harness prints `cleaned` after 0.5 s.

Two smaller problems sit in the same loop:

- The single `AtomicI32` slot holds one signal. A SIGINT and a SIGTERM in the same 50 ms poll forward only the second.
- The only test (`wait_returns_for_quick_child`) spawns `true`. No test sends a signal.

## Decision

toolr keeps SIGINT registered, so it doesn't die on Ctrl-C and still reports the runner's exit code, but **it no longer
forwards SIGINT**. It forwards SIGTERM only. This is the POSIX `system()` convention: the foreground group already got
the SIGINT from the terminal, so the parent's job is to wait.

With one forwarded signal, the single-slot drop goes away: the slot becomes an `AtomicBool` "SIGTERM pending" flag.
Process group, polling loop and Windows path stay as they are.

### Rejected alternatives

- **Put the runner in its own process group and forward everything.** The runner would no longer be in the foreground
  group, so reading the terminal raises SIGTTIN. That breaks `ctx.run(interactive=True)` and the TTY passthrough in
  `specs/archive/2026/2026-05-25-preserve-child-tty-design.md`, unless toolr also moves the terminal's foreground group
  with `tcsetpgrp` and restores it on every exit path.
- **Forward SIGINT only when toolr's group is not the terminal's foreground group** (`tcgetpgrp`). With no terminal, as
  under CI or a process manager that signals the whole group, it still double-delivers. That is also the only shape a
  test can produce without a pseudo-terminal.
- **Filter on the signal's origin** (`siginfo`, `signal_hook`'s `WithOrigin`). A `killpg` from a process manager looks
  the same as a `kill` aimed at toolr alone, so it double-delivers in the same cases. It also needs the
  `extended-siginfo` feature, and the kernel's origin reporting for terminal signals differs between Linux and macOS.
- **Forward from the `signal_hook` listener thread instead of polling.** The main thread would block in `child.wait()`,
  which reaps the child; a forward racing the reap could signal a reused pid. The polling loop only signals a child that
  `try_wait` has not yet reaped.

## Scope and compatibility

- **Behaviour change:** `kill -INT <toolr pid>`, aimed at toolr alone, no longer reaches the runner. toolr keeps waiting
  for it. Send SIGINT to the process group (`kill -INT -<pgid>`) or SIGTERM instead. Ctrl-C, and anything that signals
  the whole group, delivers SIGINT exactly once.
- **No schema bump.** Nothing in the runner JSON spec changes.
- **Out of scope:** `toolr project venv run` and `venv shell` wait with `Command::status()` and no handler, so Ctrl-C
  kills toolr itself. Routing them through `wait_with_signals` is a follow-up once this lands.
  `fix/venv-run-signal-exit` (`specs/2026-10-31-venv-run-signal-exit-design.md`, F-20) edits the same lines in
  `crates/toolr/src/project.rs`, so neither branch does it.
- **Coordination with `fix/venv-run-signal-exit`:** that branch moves dispatch's `128 + signal` mapping into a
  toolr-core helper without changing the result. This branch's SIGTERM test asserts 143, which holds before and after
  it.
- **Comments to update:** the module doc in `signals.rs`, and the `except KeyboardInterrupt:` comment in
  `crates/toolr-py/python/toolr/_runner.py::run`, which says SIGINT is "forwarded from the parent toolr process".
- **Docs:** none. No page describes signal forwarding.
- **`UNRELEASED.md`:**

```markdown
Pressing Ctrl-C during a toolr command now interrupts it once. toolr used to forward the terminal's SIGINT to the
command as well, so a second `KeyboardInterrupt` could cut a `finally:` or `except KeyboardInterrupt:` block short.
toolr now forwards SIGTERM only, so `kill -INT` aimed at the `toolr` process alone no longer reaches the command; signal
the process group or send SIGTERM instead.
```
