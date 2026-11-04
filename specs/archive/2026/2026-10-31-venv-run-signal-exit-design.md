# One exit-code rule for signal-killed children

- **Status:** design agreed (returncode convention decided 2026-11-02), not implemented. Implementation waits for
  #645 (`fix/ctx-run-timeout-kills-tree`, F-21) to merge, because that PR rewrites `crates/toolr-core/src/command.rs`.
- **Origin:** whole-project review finding F-20 (medium).
- **Plan:** [`2026-10-31-venv-run-signal-exit-plan.md`](./2026-10-31-venv-run-signal-exit-plan.md)

## Problem

toolr turns a child's `std::process::ExitStatus` into a number in four places, with three different rules:

| Path | Code | Child killed by SIGTERM | Exit code 256 or a Windows crash status |
|---|---|---|---|
| command dispatch | `crates/toolr/src/dispatch.rs`, end of `run_dispatch` | 143 (128 + signal) | `code.clamp(0, 255)`: 256 becomes 255; a negative Windows status such as `0xC0000005` becomes **0** |
| `toolr project venv run` | `crates/toolr/src/project.rs::run_command_in_venv` | **1** | `as u8` truncates: 256 becomes **0** |
| `toolr project venv shell` | `crates/toolr/src/project.rs::venv_shell` | **1** | same truncation |
| `ctx.run` | `crates/toolr-core/src/command.rs::run_command_internal` | **-1** for every signal | passed through as `i32` |

Repro on macOS, against a fresh in-tree venv whose `bin/python` is `#!/bin/sh` + `kill -TERM $$`:

```text
$ toolr project venv run --no-sync -- python; echo $?
1
$ sh -c 'kill -TERM $$'; echo $?
143
```

`kill -KILL` and `kill -SEGV` also give 1, so a crashed or OOM-killed test run looks like an ordinary failure. On
Windows, the truncation and the dispatch clamp can turn a failing child into exit 0, so CI reports success.

`ctx.run` returns `-1` for any signal, which loses the signal number. The documented idiom
`ctx.exit(result.returncode, ...)` (`docs/writing-commands/files/example.py`) then exits the runner with
`sys.exit(-1)`, which the OS reports as 255. Passing `-15` through unchanged would not help: `sys.exit(-15)` exits 241.

The audit's description says a Ctrl-C'd child under `venv run` reports 1. That isn't what happens: `venv run` installs
no SIGINT handler, so toolr itself dies of the Ctrl-C and the shell sees toolr's own signal death. That is a
signal-handling problem, not an exit-code one, and is out of scope here (see Scope).

## Decision

Two conventions, one per audience:

- **Python API (`ctx.run`):** `subprocess`'s. `CommandResult.returncode` is the exit code, or `-N` for a child killed
  by signal `N`.
- **Process exit codes (dispatch, `venv run`, `venv shell`, the runner):** the shell's. A signal death exits `128 + N`;
  an exit code outside `0..=255` exits 1. A failing child never makes toolr exit 0, and no code is truncated.

`ctx.exit` and the runner translate between the two, so `ctx.exit(result.returncode, ...)` after a SIGTERM'd child
exits 143, as in a shell.

### Rust: one classifier

Add `crates/toolr-core/src/exit_status.rs`, the only place that maps an `ExitStatus` to a number. It is a top-level
`pub mod exit_status;` in `lib.rs`, **not** gated on the `full` feature and not under `execute` (which is): toolr-py
builds toolr-core without `full` and links only `command` and `docstrings`, and `command.rs` must be able to call it. It
uses `std` only (`std::os::unix::process::ExitStatusExt`), no new dependency.

- `returncode(status: ExitStatus) -> i32`: the exit code when there is one; `-signal` when a Unix signal killed the
  child; `-1` otherwise (unreachable from `wait`, kept as the old value). Windows statuses pass through unchanged,
  including negative crash statuses, as `subprocess` does.
- `process_exit_code(status: ExitStatus) -> u8`: the exit code when it fits in `0..=255`; `128 + signal` for a Unix
  signal death; `1` otherwise.

`ctx.run` calls `returncode`. Dispatch, `venv run` and `venv shell` call `process_exit_code`. The "out of range becomes
1" rule applies to process exit codes only; `ctx.run` hands Python the raw `i32`, because a Windows crash status is real
information there.

### Python: `ctx.exit` and the runner

A `status_to_exit_code(status: int) -> int` helper in `crates/toolr-py/python/toolr/_context.py`:

- `status < 0` becomes `128 + |status|` (so `-15` becomes 143);
- anything still outside `0..=255` becomes 1 (so `256` becomes 1, not 0, and a Windows crash status, negative as an
  `i32`, becomes 1);
- everything else passes through.

Two call sites, both needed:

- `Context.exit` maps `status` before `self.parser.exit(status)`, so the raised `SystemExit.code` is already 143 for
  anyone who catches it (for example `toolr.testing` users). The message style (`exit-ok` / `exit-failure`) keeps
  testing the caller's original value.
- `_runner.py::run`, in the `except SystemExit` branch, maps an `int` code before returning it. That covers a bare
  `sys.exit(result.returncode)` or `raise SystemExit(-15)` that bypasses `ctx.exit`. It must happen in Python: the OS
  truncates the runner's exit status before the Rust side sees it, so `sys.exit(256)` would reach dispatch as 0.

This matches the runner's existing `KeyboardInterrupt` branch, which already returns 130 (128 + SIGINT).

### Why `-N` for `ctx.run`

- `CommandResult` mirrors `subprocess.CompletedProcess`'s fields (`crates/toolr-py/python/toolr/utils/command.py`), and
  Python users read `returncode` with `subprocess`'s meaning. `subprocess.run(["sh", "-c", "kill -TERM $$"]).returncode`
  is `-15`, and `os.waitstatus_to_exitcode` gives `-15` for the same wait status.
- `-N` is unambiguous. `128 + N` can't be told apart from a child that ran `exit 143`.
- `returncode != 0` checks, the common case, behave the same as today. So do `returncode < 0` checks, which already
  meant "killed by a signal".
- The shell's convention still reaches the process exit code, through `ctx.exit` and the runner.

### Rejected alternatives

- **`128 + N` for `ctx.run`** (this spec's earlier choice). It matched dispatch's exit code and made
  `ctx.exit(result.returncode)` pass through without help, but it breaks parity with `subprocess` and is ambiguous with
  a real exit 143. Reversed by the maintainer on 2026-11-02.
- **One `-> u8` helper for every path**, as the audit suggested. `ctx.run` hands the code to Python, where Windows
  statuses outside a byte are real information; clamping them there throws it away.
- **Keep `dispatch.rs`'s `clamp(0, 255)`.** It maps negative Windows statuses to success.
- **Clamp out-of-range codes to 255.** Either value is non-zero; `1` is toolr's existing "generic failure" and needs no
  explanation.
- **Map only in the runner, not in `ctx.exit`.** The process exit code would be right, but `SystemExit.code` caught in a
  test would still be `-15`.

## Decided (2026-11-02)

The earlier open question, whether `ctx.run` should report `128 + N` or `subprocess`'s `-N`, is closed: `ctx.run`
returns `-N`; `ctx.exit`, the runner and the CLI paths exit `128 + N`. Evidence the decision rests on:
`subprocess.run(["sh", "-c", "kill -TERM $$"]).returncode == -15`, `os.waitstatus_to_exitcode(15) == -15`, the shell's
`$?` is 143, and `sys.exit(-15)` exits 241.

## Scope and compatibility

- **Depends on #645.** `fix/ctx-run-timeout-kills-tree` (F-21) rewrites `crates/toolr-core/src/command.rs` and moves
  its tests to `crates/toolr-core/src/command/command_test.rs` (plus a new `command/tree.rs`). The `ctx.run` call site,
  `let returncode = status.code().unwrap_or(-1);` at the end of `run_command_internal`, survives that rewrite unchanged.
  Implementation starts after #645 merges, on a branch rebased onto it, so the `ctx.run` change doesn't conflict.
- **No schema bump.** `returncode` is the return value of the `run_command_impl` extension function, and the
  `ctx.exit` / runner mapping is Python-side exit handling. Neither touches a struct serialised into the runner JSON
  spec, or the meaning of a field in it, so `RUNNER_SCHEMA_VERSION` (`crates/toolr-core/src/execute/spec.rs`) and
  `SCHEMA_VERSION` (`crates/toolr-py/python/toolr/_runner.py`) stay as they are.
- **Behaviour changes:**
    - `ctx.run(...).returncode` is `-N` for a child killed by signal `N` (was `-1` for every signal). Unix only;
      Windows has no signal deaths.
    - `ctx.exit(status)` and a command's `SystemExit` map a negative status to `128 + |status|` and anything else
      outside `0..=255` to 1 (were handed to `sys.exit`, so `-1` exited 255, `-15` exited 241 and `256` exited 0).
    - `venv run` and `venv shell` exit `128 + N` for a signal-killed child (was 1).
    - Out-of-range child codes make dispatch, `venv run` and `venv shell` exit 1 (were truncated or clamped, sometimes
      to 0).
- **Breaking?** No. The change is a bug fix towards documented behaviour, and nothing promised the old values:
  `docs/writing-commands/context.md` says only "inspect `result.returncode`", `docs/cli.md` says `venv run`'s exit code
  passes straight through, and `-1` was never documented. `returncode != 0` and `returncode < 0` checks keep working.
  Only code comparing `returncode == -1` breaks. The commits are `fix(...)` and the note doesn't start with
  `**Breaking:**`. If the maintainer judges the `-1` → `-N` change user-visible enough to be breaking, prefix the note
  with `**Breaking:**` and mark the `ctx.run` commit `fix(run)!:`.
- **Out of scope:** `venv run` dying on Ctrl-C itself because it waits with `Command::status()` and no signal handler.
  Fixing that means routing it through `toolr_core::execute::signals::wait_with_signals`. `fix/sigint-forwarding`
  (F-29) has landed, so that follow-up is unblocked, but it is a separate change.
- **Docs:** `docs/cli.md` (`venv run` section), `docs/writing-commands/context.md` ("Running subprocesses"),
  `skills/toolr-command-authoring/SKILL.md` (the "Calling subprocesses" bullet), and the `run` and `Context.exit`
  docstrings each gain one sentence on signal deaths. Skill reference tables are generated from the docstrings; run
  `cargo xtask build-skill-refs --check` and regenerate if it drifts.
- **Release note,** queued with `toolr release-notes add --text "…"` (slug `venv-run-signal-exit`), not a hand-made
  file:

```markdown
A child killed by a signal is now reported consistently. `ctx.run(...).returncode` is `-N` for a child killed by
signal `N`, as with `subprocess` (it used to be `-1` for every signal). `ctx.exit()` turns a negative status into
`128 + N`, as a shell does, so `ctx.exit(result.returncode)` after a SIGTERM'd child exits 143 rather than 241.
`toolr project venv run` and `toolr project venv shell` now exit `128 + N` for a signal-killed child instead of 1. An
exit code that doesn't fit in a byte (Windows crash statuses, or 256) now makes toolr exit 1; it used to be truncated,
and could turn a failure into exit 0.
```
