# Say which `ContextForTesting` stream each `Context` method writes to

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-42 (low).
- **Plan:** [`2026-10-31-tester-stdout-docstring-plan.md`](./2026-10-31-tester-stdout-docstring-plan.md)

## Problem

`ContextForTesting` (`crates/toolr-py/python/toolr/testing/_make_context.py`) documents its two capture properties
wrongly:

- `stdout`: "Everything written to `print`/`info` so far."
- `stderr`: "Everything written to `error`/`warn`/`debug` so far."

`Context.info` (`crates/toolr-py/python/toolr/_context.py`) logs to `_console_stderr`, like `warn`, `error` and
`debug`. The `Running '…'` echo that `Context.run` prints before every command goes through `self.info`, so it lands in
`stderr` too. `docs/writing-commands/testing.md` ("Calling a command directly") lumps `print`/`info`/`error` together.
These docstrings are also embedded in `skills/toolr-command-authoring/references/testing.md`.

Repro: `ctx = make_context(tmp, prompt_input="bob\n", run=lambda *a, **k: make_command_result())`, then
`ctx.print("P")`, `ctx.info("I")`, `ctx.prompt("Name?")`, `ctx.run("echo", "SUB")`, `ctx.exit(1, "bye")`. Result:
`ctx.stdout == "P\nName?: "`; `ctx.stderr` holds `I`, `Running 'echo SUB'` and `bye`. A test asserting
`"I" in ctx.stdout` fails although the docstring says it should pass.

## Decision

Fix the docstrings and the docs paragraph, and pin the split with a test.

- `stdout`: "Everything written through `print` so far, plus `prompt`'s question text."
- `stderr`: "Everything written through `info`, `warn`, `error` and `debug` so far (`debug` only at verbose
  verbosity, `info` not at quiet), the message passed to `exit`, and the `Running '…'` line `run` prints before each
  command." Add: the streamed output of the subprocess itself goes to the real process streams, not here; use
  `capture_output=True` and read the `CommandResult`.
- `docs/writing-commands/testing.md`: replace "`ctx.stdout`/`ctx.stderr` capture everything written through
  `ctx.print`/`ctx.info`/`ctx.error`/etc." with the same split.

### Rejected alternatives

- **Move `info` to stdout.** `info` is a log line by design (rich `Console.log` with timestamp and caller) and the real
  CLI writes it to stderr. The test double must match the CLI, not the other way round.
- **Docstring-only, no test.** The split is part of the public test API. A test keeps the docstring honest if a method
  ever changes console.

## Scope and compatibility

- No behaviour change. No schema change. No new public name, but the property docstrings are embedded in
  `skills/toolr-command-authoring/references/testing.md`: run `cargo xtask build-skill-refs`.
- Docs: `docs/writing-commands/testing.md`, the paragraph after the prompt examples in "Calling a command directly".
- `UNRELEASED.md`: none. It corrects documentation of unchanged behaviour; the fixed docs ship with the release.
