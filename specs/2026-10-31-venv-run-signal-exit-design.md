# One exit-code rule for signal-killed children

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-20 (medium).
- **Plan:** [`2026-10-31-venv-run-signal-exit-plan.md`](./2026-10-31-venv-run-signal-exit-plan.md)

## Problem

toolr turns a child's `std::process::ExitStatus` into a number in three places, and each does it differently:

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
`sys.exit(-1)`, which the OS reports as 255.

The audit's description says a Ctrl-C'd child under `venv run` reports 1. That isn't what happens: `venv run` installs
no SIGINT handler, so toolr itself dies of the Ctrl-C and the shell sees toolr's own signal death. That is a
signal-handling problem, not an exit-code one, and is out of scope here (see Scope).

## Decision

Add one classifier in toolr-core, `crates/toolr-core/src/execute/exit_status.rs`, with two renderers:

- `exit_code(status: ExitStatus) -> i32`: the child's exit code when it has one; `128 + signal` when a Unix signal
  killed it; `1` otherwise. Windows statuses pass through unchanged.
- `process_exit_code(status: ExitStatus) -> u8`: `exit_code` when it fits in `0..=255`; `1` otherwise. A failing child
  can never become 0.

All four call sites use them. Dispatch, `venv run` and `venv shell` call `process_exit_code`; `ctx.run` calls
`exit_code`.

### Why 128 + signal for `ctx.run`, not `-signal`

`subprocess` reports a signal death as `-N`. We pick the shell's `128 + N` for `CommandResult.returncode` instead:

- toolr already uses it everywhere else. Dispatch exits `128 + N`, and the runner maps `KeyboardInterrupt` to 130
  (`crates/toolr-py/python/toolr/_runner.py::run`).
- The documented `ctx.exit(result.returncode, ...)` idiom then passes the child's status straight through: a SIGTERM'd
  child makes the command exit 143, as in a shell. With `-15`, the runner's `sys.exit(-15)` would exit 241.
- `returncode != 0` checks, the common case, behave the same either way.

### Rejected alternatives

- **One `-> u8` helper for every path**, as the audit suggested. `ctx.run` hands the code to Python, where Windows
  statuses outside a byte are real information; clamping them there throws it away.
- **Keep `dispatch.rs`'s `clamp(0, 255)`.** It maps negative Windows statuses to success.
- **Clamp out-of-range codes to 255.** Either value is non-zero; `1` is toolr's existing "generic failure" and needs no
  explanation.

## Scope and compatibility

- **No schema bump.** `returncode` is the return value of the `run_command_impl` extension function, not part of the
  runner JSON spec. `RUNNER_SCHEMA_VERSION` and `SCHEMA_VERSION` stay as they are.
- **Behaviour changes:** `venv run` and `venv shell` exit `128 + N` for a signal-killed child (was 1); out-of-range
  codes become 1 on every path (were truncated or clamped, sometimes to 0); `ctx.run` returns `128 + N` (was -1).
- **Out of scope:** `venv run` dying on Ctrl-C itself because it waits with `Command::status()` and no signal handler.
  Fixing that means routing it through `toolr_core::execute::signals::wait_with_signals`, whose SIGINT handling
  `fix/sigint-forwarding` (`specs/2026-10-31-sigint-forwarding-design.md`, F-29) changes. It is a follow-up once that
  branch lands, so `venv run` doesn't pick up the double SIGINT.
- **Coordination with `fix/sigint-forwarding`:** that branch's SIGTERM test asserts dispatch exits 143, which this
  change keeps. Neither branch edits the other's files.
- **Docs:** `docs/cli.md` (`venv run` section), `docs/writing-commands/context.md` ("Running subprocesses") and
  `skills/toolr-command-authoring/SKILL.md` (the `ctx.run` bullet) each gain one sentence on signal deaths. Skill
  reference tables are generated; run `cargo xtask build-skill-refs --check`.
- **`UNRELEASED.md`:**

```markdown
A child killed by a signal now gives the same exit code everywhere: 128 plus the signal number, as in a shell.
`toolr project venv run` and `toolr project venv shell` used to exit 1, and `ctx.run` returned `-1` as
`returncode`; a SIGTERM'd child now gives 143. An exit code that doesn't fit in a byte (Windows crash statuses, or 256)
now makes toolr exit 1. It used to be truncated, and could turn a failure into exit 0.
```

## Open question

`ctx.run` returning `128 + N` instead of `subprocess`'s `-N` is a public API convention. The reasons above favour
`128 + N`; a maintainer who prefers `subprocess` parity should say so before Task 2 lands, because the docs, the test
and the release note all name the value.
