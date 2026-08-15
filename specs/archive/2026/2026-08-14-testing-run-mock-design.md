# Mockable `Context.run`/`chdir`/`prompt` for `toolr.testing`

**Status:** Approved (2026-08-14)
**Topic:** Give `toolr.testing.make_context` real, first-class ways to mock `ctx.run`, `ctx.chdir`,
and `ctx.prompt` so caller repos stop hand-rolling fake `Context`/`FakeRun` doubles.

## Background

`toolr.testing.make_context` already builds a *real* `Context` (real `print`/`info`/`warn`/
`error`/`exit` semantics, output captured via `Console(file=StringIO(...))`), so a test can call an
`@command`-decorated function directly and assert on what it did. `ctx.run`, however, is the real
subprocess runner — `Context.run()` calls the module-level `toolr.utils.command.run` directly, and
its docstring tells callers to "monkeypatch it yourself if a test needs to intercept it."

A separate caller project has three toolr command groups (`tools/db/tests/_fakes.py`,
`tools/pw/tests/_fakes.py`, `tools/tests/env/_fakes.py`) that each independently invented
near-identical fixes: a `FakeRun` that
records calls and resolves a canned `FakeResult` by matching a leading-argument prefix, plus a
`FakeCtx` that duck-types the entire `Context` surface instead of using `make_context` at all
(reintroducing exactly the drift risk `make_context` exists to avoid). The three copies have
already started to diverge in small ways.

Two API shapes were considered and rejected before landing on this design:

- **A bespoke `FakeRun`/`FakeResult` matcher** (canonicalizing that project's existing code as-is) —
  rejected because it invents a one-off API (`.calls`, prefix-matched `responses` dict) instead of
  the call-assertion API most Python test authors already know (`unittest.mock`).
- **`pytest-subprocess`** — rejected: it works by monkeypatching `subprocess.Popen`/`subprocess.run`
  at the Python level. Toolr's actual process spawn happens inside a compiled pyo3 extension
  (`toolr/utils/_rust_utils.abi3.so`, via `run_command_impl`), never through Python's `subprocess`
  module, so `pytest-subprocess` has nothing to attach to here. Its *registration API shape*
  (`fp.register(cmd, stdout=..., returncode=...)`, strict-by-default — an unregistered command
  raises) is worth keeping as inspiration, just not the library itself.

## Goal

Add injection seams to the real `Context` so `ctx.run`, `ctx.chdir`, and `ctx.prompt` can each be
replaced with a test double, and ship canonical doubles in `toolr.testing` with `unittest.mock`-
compatible ergonomics (`assert_called_with`, `call_args_list`, `side_effect`, `return_value`) — plus
a `pytest-subprocess`-style `.register(...)` for `run`'s common declarative case. `print`/`debug`/
`info`/`warn`/`error`/`exit` are untouched — `make_context` already routes those through captured
consoles, which is sufficient. `which` is also untouched (see Non-goals).

## Non-goals

- No change to `toolr.utils.command.run`'s real implementation or the Rust command runner.
- No change to `Context.which` — it already takes an explicit `path=` kwarg, so a test simulates
  "tool missing" or "tool found at X" by passing `path=str(tmp_path)` without any seam. Adding
  `_which_impl` would solve a problem that doesn't exist.
- No migration of that separate project in this design. That's a follow-up PR in its own repo once
  this ships in a released `toolr-py`.
- No new third-party test dependency (`pytest-subprocess`, `pytest-mock`). `unittest.mock` is
  stdlib; `RunMock` wraps a `Mock` instance, and the `chdir`/`prompt` doubles are plain `Mock`/`io`
  objects.

## Design

### `Context._run_impl`

New private field on `Context` (`crates/toolr-py/python/toolr/_context.py`), following the same
pattern as the existing private `_console_stderr`/`_console_stdout` fields on this frozen
`msgspec.Struct`:

```python
_run_impl: Callable[..., CommandResult[str] | CommandResult[bytes]] = command.run
```

Verified empirically: `msgspec.Struct` accepts a bare function as a field default (no special
handling needed, unlike mutable defaults). New defaulted fields must be added after
`default_no_output_timeout_secs` — `msgspec` raises `TypeError: Required field '…' cannot follow
optional fields` if a defaulted field precedes a required one, same rule dataclasses enforce.

`Context.run()` changes its one call site from `command.run(...)` to `self._run_impl(...)`. No
other line in `run()` changes — the info-line logging and timeout-default resolution stay exactly
as they are, so a mocked run still produces the same log output a real one would.

`Context` is also declared in `crates/toolr-py/python/toolr/_context.pyi` — the stub needs the same
three new fields (`_run_impl`, `_chdir_impl`, `_prompt_stream`, introduced below) or `mypy` sees the
old signature and `make_context(run=...)` fails type-check even though it works at runtime.

### `toolr.testing.RunMock`

**Not** a `MagicMock` subclass — `unittest.mock`'s `__setattr__`/`__getattr__` overrides are heavily
customized (attribute access auto-vivifies child mocks; a misspelled attribute call on the mock
would silently return a fresh `Mock` instead of raising, directly undermining the "strict by default"
goal), and storing a custom registry list alongside `MagicMock`'s own state invites fighting the
base class's internals for no real benefit. Composition instead, in a new
`crates/toolr-py/python/toolr/testing/_run_mock.py`:

```python
class RunMock:
    def __init__(self) -> None:
        self._mock = Mock(name="RunMock")
        self._registrations: list[tuple[tuple[str, ...], CommandResult, int | None]] = []

    def register(self, *cmdline: str, stdout="", stderr="", returncode=0, occurrences=None) -> None:
        ...

    def __call__(self, cmdline, **kwargs):
        result = self._mock(cmdline, **kwargs)  # records the call unconditionally
        result = self._resolve(cmdline, kwargs, result)
        if not kwargs.get("capture_output"):
            result = msgspec.structs.replace(result, stdout=None, stderr=None)
        return result

    # explicit, fixed forwarding — not a blanket __getattr__, so a real typo raises AttributeError
    assert_called_with = ...       # forwards to self._mock
    assert_any_call = ...
    assert_called_once_with = ...
    call_args = ...                # property
    call_args_list = ...           # property
    call_count = ...               # property
    reset_mock = ...
```

- **`capture_output` contract enforcement** — `CommandResult` is `frozen=True`, so `stdout`/`stderr`
  can't be assigned after construction; forcing them to `None` means building a *replacement* via
  `msgspec.structs.replace(result, stdout=None, stderr=None)`, not mutation. This matches the real
  `command.run`, which only opens capture files when `capture_output=True` (confirmed in
  `crates/toolr-py/python/toolr/utils/command.py`, and confirmed empirically that
  `msgspec.structs.replace` works against a frozen struct) — the type annotation on
  `CommandResult.stdout: IO[T]` is non-optional, so this is knowingly matching real behavior rather
  than the declared type, same as the real runner already does.
- **`.register(*cmdline, stdout="", stderr="", returncode=0, occurrences=None)`** — declarative
  sugar mirroring `pytest-subprocess`'s `fp.register(...)`. Internally appends
  `(cmdline_prefix, CommandResult, occurrences)` to an ordered list consulted on each call.
  Matching is longest-leading-prefix, same semantics that existing `FakeRun` already uses.
  `occurrences` limits how many times a registration may match (default: unlimited) — once
  exhausted, later matching calls fall through to the next registration or to "unregistered." A
  call that matches no registration, with no `side_effect`/`return_value` configured on the
  underlying `Mock` either, raises `AssertionError` immediately (strict by default — no silent
  `default=` fallback to mask a typo in the test).
- **Assertions** go through the explicitly forwarded methods above (`run_mock.assert_called_with(...)`,
  `run_mock.call_args_list`, etc.) — a fixed, documented surface, not everything `Mock` happens to
  expose. Calling anything else on `run_mock` raises `AttributeError`, because there is no blanket
  passthrough to silently catch a misspelled call.

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

### `Context._chdir_impl`

Same pattern, for the real-cwd-mutation problem: `chdir()` currently calls module-level `os.chdir`
twice (enter and restore-on-exit) directly against the actual test process. A not-quite-
exception-safe test leaves the pytest process's cwd wrong for whatever runs after it — a real
test-isolation hazard, not a hypothetical one.

```python
_chdir_impl: Callable[[str | pathlib.Path], None] = os.chdir
```

Both call sites in `chdir()` change from `os.chdir(...)` to `self._chdir_impl(...)`. `make_context`
gains `chdir: Callable | None = None`, passed through as `_chdir_impl`. A test passes a bare
`unittest.mock.Mock()` and asserts `assert_called_once_with(expected_path)` — the real filesystem
cwd never moves.

### `Context._prompt_stream`

`prompt()`'s private `_prompt()` implementation already accepts a `stream=` override built for
exactly this — `klass.ask(..., stream=stream)` — but the public `prompt()` never forwards it, so
`make_context` has no way to reach that existing seam.

```python
_prompt_stream: TextIO | None = None
```

`prompt()` changes its one call to `self._prompt(...)` to also pass `stream=self._prompt_stream`.
`make_context` gains `prompt_input: str | TextIO | None = None`: a plain `str` is wrapped in
`io.StringIO` (so a test can write `prompt_input="yes\n"` for the common single-answer case), a
`TextIO` is passed through as-is (for multi-prompt sequences). Feeds real answers through the real
`rich.prompt.Prompt`/`IntPrompt`/`FloatPrompt`/`Confirm` classes — no mocking of `rich` internals.

### `make_context(..., run=None, chdir=None, prompt_input=None)`

Three new optional keywords on `toolr.testing.make_context`, each independently optional and each
passed straight through to the matching `Context` private field. Omitting any of them keeps that
piece's real, current behavior (real subprocess runner / real `os.chdir` / real blocking stdin
read) — fully backward compatible, no existing caller needs to change.

## Data flow

```python
run_mock = RunMock()
run_mock.register("git", "fetch", stdout="up to date\n")

ctx_for_test = make_context(repo_root, run=run_mock)

my_command(ctx_for_test.ctx)  # calls ctx.run("git", "fetch")

run_mock.assert_called_once_with(
    ("git", "fetch"), stream_output=True, capture_output=False,
    timeout_secs=None, no_output_timeout_secs=None,
)
```

(`Context.run` always forwards `timeout_secs=`/`no_output_timeout_secs=` to `_run_impl` — even when
the caller passed neither, they arrive resolved to the `Context`'s configured defaults, `None` if
unset. Any exact-call assertion has to spell out all four kwargs; there's no kwarg-subset shorthand.)

`ctx.run("git", "fetch")` → `Context.run` logs the info line, resolves timeout defaults, calls
`self._run_impl(("git", "fetch"), stream_output=..., capture_output=..., ...)` → `RunMock.__call__`
matches the registration, builds/returns the `CommandResult`, forcing `stdout`/`stderr` to `None`
if `capture_output` wasn't requested. The test asserts either via `run_mock`'s standard `Mock` API
or on the returned `CommandResult`/`ctx_for_test.output`.

`chdir` and `prompt_input` follow the same shape:

```python
chdir_mock = Mock()
ctx_for_test = make_context(repo_root, chdir=chdir_mock, prompt_input="yes\n")

my_command(ctx_for_test.ctx)  # calls `with ctx.chdir(some_path): ...` and `ctx.prompt("Continue?", bool)`

chdir_mock.assert_any_call(some_path)  # not assert_called_once_with — chdir() also restores the cwd
```

`ctx.chdir(some_path)` → `self._chdir_impl(some_path)` (`chdir_mock`, recording the call, doing
nothing to the real filesystem) → on context-manager exit, `self._chdir_impl(cwd)` again to
restore. `ctx.prompt("Continue?", bool)` → `self._prompt(..., stream=self._prompt_stream)` →
`Confirm.ask` reads `"yes\n"` from the wrapped `io.StringIO` exactly as it would read real stdin.

## Error handling

- Unregistered call with no `side_effect`/`return_value` configured → `AssertionError` naming the
  unmatched cmdline, at call time (fails the test immediately, points at the actual gap).
- `capture_output` not requested → `stdout`/`stderr` forced to `None` on the returned result,
  regardless of registration contents — same failure mode a real bug hitting the real runner would
  produce (`AttributeError`/`None`-access on the caller's side), not a false pass.
- Mixing `.register(...)` with a directly-set `side_effect` on the underlying `Mock`
  (`run_mock.mock.side_effect = ...`) → `TypeError` at `.register()`-call time if the `Mock`
  already has a `side_effect` configured. (`return_value` isn't guarded the same way: a fresh
  `Mock`'s `return_value` is itself a lazily auto-vivified child `Mock`, not a distinguishable
  "unset" sentinel via the public API — registrations simply take priority over it whenever any
  are present.)
- `chdir`'s restore-on-exit call still runs even when `_chdir_impl` is mocked, same as today's
  try/finally — a mocked `chdir` that raises on the restore call surfaces the same way a real
  `os.chdir` failure would (`self.error(...)` path taken if the *real* cwd stopped existing; with a
  mock this branch simply won't trigger unless the test configures the mock to simulate it).
- **Exhausting `_prompt_stream` hangs, it does not raise, unless a `default` was passed to
  `ctx.prompt(...)`.** `Console.input(stream=...)` calls `stream.readline()`; a plain
  `io.StringIO`, once exhausted, returns `""` forever rather than raising. `rich`'s
  `PromptBase.__call__` loops on `InvalidResponse` until it gets an answer, so a `Confirm`/`Prompt`
  call with no `default` and an under-provisioned `prompt_input` spins forever — a test that
  under-provides answers hangs the suite instead of failing it. To avoid this footgun,
  `make_context`'s `prompt_input` wraps the given string/stream in a thin `io.StringIO` subclass
  whose `readline()` raises `EOFError` once the underlying buffer is exhausted (mirroring real
  stdin's EOF-on-closed-pipe behavior), so an under-provisioned test fails fast instead of hanging.

## Testing

- New `crates/toolr-py/tests/testing/test_run_mock.py`: `RunMock` call recording, longest-prefix
  registration matching, `occurrences` exhaustion, the `capture_output` guard (both a populated
  registration hit with `capture_output=False` and the unregistered-call error), and the
  register-vs-side_effect conflict.
- New tests for `make_command_result`: fresh streams per call, str vs bytes dispatch.
- Extend the existing `make_context` tests to cover all three new params end-to-end, and confirm
  each default path (param omitted) is unchanged:
    - `run=` — a fake command function calling `ctx.run`, asserting the mock recorded it.
    - `chdir=` — a command using `with ctx.chdir(...)`, asserting the mock recorded both the enter
  and restore calls, and that the real process cwd never moved.
    - `prompt_input=` — a command calling `ctx.prompt(...)` with each `expected_type`
  (`str`/`int`/`float`/`bool`), asserting the fed answer comes back typed correctly.

## Migration considerations

Purely additive on the `toolr-py` side — no existing `Context` or `make_context` caller changes
behavior. The separate project's three `_fakes.py` migrations are out of scope for this design
(see Non-goals) and follow once a `toolr-py` release ships this.

`toolr.testing.__all__` gains `RunMock` and `make_command_result` — queue a `UNRELEASED.md` entry
per `CLAUDE.md`. `cargo xtask build-skill-refs` does **not** need re-running for this: its
`authoring::commands` generator walks `toolr.__all__` (the top-level package) to build
`skills/toolr-command-authoring/references/commands.md`, and `toolr.testing` isn't re-exported
there — confirmed by reading `crates/xtask/src/build_skill_refs/authoring.rs` and
`crates/toolr-py/python/toolr/__init__.py`. `skills/toolr-command-authoring/SKILL.md` does mention
`make_context`/`CommandsTester` in hand-written prose (not generated) — worth a courtesy update to
mention the new `run=`/`chdir=`/`prompt_input=` params, but it's not a CI gate.

## Risks

- **Composition over `MagicMock` subclassing avoids the sharpest footgun, but the fixed forwarding
  list can go stale** — if a test needs a `Mock` method `RunMock` doesn't forward
  (`assert_not_called`, say), it's a small addition to the explicit list, not a design change. Worth
  a short spike (~10 lines) before the plan locks the exact forwarded surface.
- **`.register(...)`'s longest-prefix matching can surprise** if two registrations have prefixes
  that are both valid leading subsequences of a call (e.g. `("git",)` and `("git", "fetch")`) —
  mitigated by picking the longest match deterministically and documenting it, same rule that
  existing `FakeRun` already established informally.

## Out-of-scope follow-ups (not part of this work)

- Migrating that separate project's three `_fakes.py` files to import `RunMock`/
  `make_command_result` from `toolr.testing` and delete `FakeCtx` (separate PR, in its own repo,
  after release).
- Mocking `Context.which` — no reported pain point; already testable via its existing `path=` kwarg.

## Amendment (2026-08-14, mid-implementation): `ContextForTesting` becomes a `Context` subclass

During implementation (after Task 1 landed), the user rejected the pre-existing
`make_context(...) -> ContextForTesting` **wrapper** shape (`result.ctx.run(...)`,
`result.output.stdout`) as awkward, and — offered a non-breaking alternative alongside it —
explicitly chose the breaking redesign instead: `make_context` returns a `Context` **subclass**
directly, so `ctx = make_context(...)` and `ctx.run(...)`/`ctx.stdout` both work on the same
object, no wrapper indirection. The class keeps the name `ContextForTesting` — same concept, new
shape — rather than inventing a new name (see the naming note below on why).

This breaks the stability guarantee this repo's own docs currently state (`docs/writing-commands/testing.md`
§ Stability: "Same for `make_context`'s signature and the `ContextForTesting`/`CapturedOutput` shape
it returns.") — accepted as a deliberate pre-1.0 breaking change, not an oversight.

**Design:**

```python
class ContextForTesting(Context, frozen=True):
    """A `Context` returned by `make_context`, with captured-output accessors."""

    @property
    def stdout(self) -> str:
        """Everything written to `print`/`info` so far."""
        return self._console_stdout.file.getvalue()

    @property
    def stderr(self) -> str:
        """Everything written to `error`/`warn`/`debug` so far."""
        return self._console_stderr.file.getvalue()
```

Verified empirically: a frozen `msgspec.Struct` subclass that adds only properties (no new fields)
works exactly as expected — `isinstance(ContextForTesting(...), Context)` is `True`, all inherited
methods (`run`, `print`, `error`, `chdir`, `prompt`, …) work unmodified, and `rich.console.Console.file`
(the object the constructor was given as `file=`) is the same object back, so `.getvalue()` reads
whatever was written through it, including with `stderr=True` and a custom `theme=`.

**Naming:** the obvious alternative, `TestingContext`, was rejected — it matches pytest's default
`python_classes = "Test*"` collection glob. Verified empirically that this specific case wouldn't
actually trigger pytest's classic `PytestCollectionWarning` (a `msgspec.Struct` subclass's
`__init__`/`__new__` compare equal to `object`'s at the Python level, unlike a plain class with a
real `__init__`, so pytest's constructor-detection check doesn't fire) — but it's still a fragile,
confusing name to carry in a pytest-based codebase, and a future pytest version could change that
detection heuristic. Reusing `ContextForTesting` (the old wrapper's name) sidesteps the glob
entirely (it doesn't start with `Test`) while keeping the established concept name.

`make_context`'s signature changes from returning the old wrapper `ContextForTesting` (holding
`.ctx`/`.output`) to returning the new subclass `ContextForTesting` (which *is* the `Context`,
directly) — same name, replaced shape. Its body constructs the new `ContextForTesting(...)`
directly instead of building a `Context` plus a separate `CapturedOutput` + old-shape
`ContextForTesting` wrapper pair — the old `CapturedOutput` class and the old wrapper shape are
deleted, not deprecated.

**Blast radius** (confirmed via repo-wide grep before this amendment was written): every caller of
`.ctx`/`.output.stdout`/`.output.stderr` in this repo needs updating — `tests/test_make_context.py`
and `docs/writing-commands/testing.md`. `tests/context/test_core.py`/`tests/context/test_run.py`
do not call `make_context` at all after Task 1's fix round (an earlier attempt to call it from
`test_run.py` was reverted as scope creep, see the ledger) — they construct `Context(...)` directly
and are unaffected by this amendment. `docs/reference/testing.md` is mkdocstrings-generated from
docstrings, so it picks up the new class automatically — no manual edit needed there.
`skills/toolr-command-authoring/SKILL.md` has a hand-written mention of `make_context`; a courtesy
update is worthwhile but non-blocking (same as the plan's existing Out-of-scope note about that
file).

**Non-goal, still:** this amendment does not change `RunMock`, `make_command_result`, or the
`_run_impl`/`_chdir_impl`/`_prompt_stream` seams on `Context` itself — those are unaffected by
what `make_context` returns.

## Approval

User approved the design via brainstorming session on 2026-08-14. The `ContextForTesting` amendment
above was approved directly by the user mid-implementation, the same day, in response to a
concrete ergonomics complaint raised against Task 1's tests.
