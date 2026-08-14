# Mockable `Context.run` for `toolr.testing`

**Status:** Approved (2026-08-14)
**Topic:** Give `toolr.testing.make_context` a real, first-class way to mock `ctx.run` so caller
repos stop hand-rolling fake `Context`/`FakeRun` doubles.

## Background

`toolr.testing.make_context` already builds a *real* `Context` (real `print`/`info`/`warn`/
`error`/`exit` semantics, output captured via `Console(file=StringIO(...))`), so a test can call an
`@command`-decorated function directly and assert on what it did. `ctx.run`, however, is the real
subprocess runner — `Context.run()` calls the module-level `toolr.utils.command.run` directly, and
its docstring tells callers to "monkeypatch it yourself if a test needs to intercept it."

Three caller repos in `dashtastic` (`tools/db/tests/_fakes.py`, `tools/pw/tests/_fakes.py`,
`tools/tests/env/_fakes.py`) each independently invented near-identical fixes: a `FakeRun` that
records calls and resolves a canned `FakeResult` by matching a leading-argument prefix, plus a
`FakeCtx` that duck-types the entire `Context` surface instead of using `make_context` at all
(reintroducing exactly the drift risk `make_context` exists to avoid). The three copies have
already started to diverge in small ways.

Two API shapes were considered and rejected before landing on this design:

- **A bespoke `FakeRun`/`FakeResult` matcher** (canonicalizing dashtastic's existing code as-is) —
  rejected because it invents a one-off API (`.calls`, prefix-matched `responses` dict) instead of
  the call-assertion API most Python test authors already know (`unittest.mock`).
- **`pytest-subprocess`** — rejected: it works by monkeypatching `subprocess.Popen`/`subprocess.run`
  at the Python level. Toolr's actual process spawn happens inside a compiled pyo3 extension
  (`toolr/utils/_rust_utils.abi3.so`, via `run_command_impl`), never through Python's `subprocess`
  module, so `pytest-subprocess` has nothing to attach to here. Its *registration API shape*
  (`fp.register(cmd, stdout=..., returncode=...)`, strict-by-default — an unregistered command
  raises) is worth keeping as inspiration, just not the library itself.

## Goal

Add an injection seam to the real `Context` so `ctx.run` can be replaced with a test double, and
ship a canonical double in `toolr.testing` with `unittest.mock`-compatible ergonomics
(`assert_called_with`, `call_args_list`, `side_effect`, `return_value`) plus a `pytest-subprocess`-
style `.register(...)` for the common declarative case. Every other `Context` method
(`print`/`info`/`warn`/`error`/`exit`/`prompt`/`chdir`/`which`) is untouched — this design only
touches subprocess execution.

## Non-goals

- No change to `toolr.utils.command.run`'s real implementation or the Rust command runner.
- No change to `Context.prompt`, `chdir`, or `which` — out of scope; `make_context` callers who need
  those mocked continue to monkeypatch them directly, same as today.
- No dashtastic migration in this design. That's a follow-up PR in the dashtastic repo once this
  ships in a released `toolr-py`.
- No new third-party test dependency (`pytest-subprocess`, `pytest-mock`). `unittest.mock` is
  stdlib; `RunMock` is a subclass of it.

## Design

### `Context._run_impl`

New private field on `Context` (`crates/toolr-py/python/toolr/_context.py`), following the same
pattern as the existing private `_console_stderr`/`_console_stdout` fields on this frozen
`msgspec.Struct`:

```python
_run_impl: Callable[..., CommandResult[str] | CommandResult[bytes]] = command.run
```

`Context.run()` changes its one call site from `command.run(...)` to `self._run_impl(...)`. No
other line in `run()` changes — the info-line logging and timeout-default resolution stay exactly
as they are, so a mocked run still produces the same log output a real one would.

### `toolr.testing.RunMock`

A `unittest.mock.MagicMock` subclass in a new `crates/toolr-py/python/toolr/testing/_run_mock.py`:

- **Call recording and configuration** — inherited from `MagicMock` as-is: `assert_called_with`,
  `assert_any_call`, `call_args_list`, `reset_mock`, `side_effect`, `return_value` all work
  unmodified.
- **`capture_output` contract enforcement** — overrides `__call__`: resolves the return value via
  the normal `MagicMock` machinery first, then — if the call's `capture_output` kwarg is not
  truthy — forces the returned `CommandResult`'s `stdout`/`stderr` to `None` regardless of what was
  configured. This matches the real `command.run`, which only opens capture files when
  `capture_output=True` (confirmed in `crates/toolr-py/python/toolr/utils/command.py`), and closes
  the footgun where a test configures a populated `stdout` but the code under test never actually
  asked to capture it.
- **`.register(*cmdline, stdout="", stderr="", returncode=0, occurrences=None)`** — declarative
  sugar mirroring `pytest-subprocess`'s `fp.register(...)`. Internally appends
  `(cmdline_prefix, CommandResult)` to an ordered list consulted by a `side_effect` installed on
  first use. Matching is longest-leading-prefix, same semantics dashtastic's existing `FakeRun`
  already uses. `occurrences` limits how many times a registration may match (default: unlimited)
  — once exhausted, later matching calls fall through to the next registration or to "unregistered."
  A call that matches no registration raises `AssertionError` immediately (strict by default — no
  silent `default=` fallback to mask a typo in the test), unless `side_effect`/`return_value` was
  set directly on the mock instead of using `.register(...)`, in which case that takes precedence
  and `.register(...)` must not be used on the same instance (mixing the two raises `TypeError` at
  configuration time, not at call time).

### `toolr.testing.make_command_result`

```python
def make_command_result(
    *, args: list[str] | None = None, stdout: str | bytes = "", stderr: str | bytes = "",
    returncode: int = 0,
) -> CommandResult[str] | CommandResult[bytes]:
```

Builds a genuine `CommandResult` with real `io.StringIO`/`io.BytesIO` streams (matching whichever
type `stdout`/`stderr` are passed as). `.register(...)` calls this internally; it's also exported
directly for tests that want to build a `side_effect`/`return_value` by hand.

### `make_context(..., run: Callable | None = None)`

New optional keyword on `toolr.testing.make_context`. When provided, passed straight through as the
new `Context._run_impl` field. When omitted, `Context` uses its own default (`command.run`,
today's behavior) — fully backward compatible, no existing caller needs to change.

## Data flow

```python
run_mock = RunMock()
run_mock.register("git", "fetch", stdout="up to date\n")

ctx_for_test = make_context(repo_root, run=run_mock)

my_command(ctx_for_test.ctx)  # calls ctx.run("git", "fetch")

run_mock.assert_called_once_with(("git", "fetch"), stream_output=True, capture_output=False, ...)
```

`ctx.run("git", "fetch")` → `Context.run` logs the info line, resolves timeout defaults, calls
`self._run_impl(("git", "fetch"), stream_output=..., capture_output=..., ...)` → `RunMock.__call__`
matches the registration, builds/returns the `CommandResult`, forcing `stdout`/`stderr` to `None`
if `capture_output` wasn't requested. The test asserts either via `run_mock`'s standard `Mock` API
or on the returned `CommandResult`/`ctx_for_test.output`.

## Error handling

- Unregistered call with no `side_effect`/`return_value` configured → `AssertionError` naming the
  unmatched cmdline, at call time (fails the test immediately, points at the actual gap).
- `capture_output` not requested → `stdout`/`stderr` forced to `None` on the returned result,
  regardless of registration contents — same failure mode a real bug hitting the real runner would
  produce (`AttributeError`/`None`-access on the caller's side), not a false pass.
- Mixing `.register(...)` with a directly-set `side_effect`/`return_value` on the same `RunMock`
  instance → `TypeError` at configuration time (whichever is set second).

## Testing

- New `crates/toolr-py/tests/testing/test_run_mock.py`: `RunMock` call recording, longest-prefix
  registration matching, `occurrences` exhaustion, the `capture_output` guard (both a populated
  registration hit with `capture_output=False` and the unregistered-call error), and the
  register-vs-side_effect conflict.
- New tests for `make_command_result`: fresh streams per call, str vs bytes dispatch.
- Extend the existing `make_context` tests to cover the new `run=` param end-to-end (a fake command
  function calling `ctx.run`, asserting the mock recorded it) and confirm the default path
  (`run` omitted) is unchanged.

## Migration considerations

Purely additive on the `toolr-py` side — no existing `Context` or `make_context` caller changes
behavior. `dashtastic`'s three `_fakes.py` migrations are out of scope for this design (see
Non-goals) and follow once a `toolr-py` release ships this.

## Risks

- **`RunMock.__call__` overriding `MagicMock` semantics is a partial override** — if a future
  `unittest.mock` version changes `MagicMock.__call__`'s internals in an incompatible way, the
  subclass could drift. Low risk in practice (the override only wraps the return value after
  delegating to `super().__call__`, it doesn't reimplement mock resolution).
- **`.register(...)`'s longest-prefix matching can surprise** if two registrations have prefixes
  that are both valid leading subsequences of a call (e.g. `("git",)` and `("git", "fetch")`) —
  mitigated by picking the longest match deterministically and documenting it, same rule
  dashtastic's existing `FakeRun` already established informally.

## Out-of-scope follow-ups (not part of this work)

- Migrating dashtastic's three `_fakes.py` files to import `RunMock`/`make_command_result` from
  `toolr.testing` and delete `FakeCtx` (separate PR, in the dashtastic repo, after release).
- Mocking `Context.prompt`, `chdir`, or `which` — no reported pain point for these today.

## Approval

User approved the design via brainstorming session on 2026-08-14.
