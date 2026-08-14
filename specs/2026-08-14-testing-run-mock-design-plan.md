# Mockable `Context.run`/`chdir`/`prompt` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `toolr.testing.make_context` real, first-class ways to mock `ctx.run`, `ctx.chdir`,
and `ctx.prompt`, backed by a canonical `RunMock` double, so caller repos stop hand-rolling fake
`Context` doubles.

**Architecture:** Three new private, defaulted fields on the real `Context`
(`_run_impl`/`_chdir_impl`/`_prompt_stream`), each defaulting to the current real behavior. Three
new `Context.run()`/`chdir()`/`prompt()` call sites read from those fields instead of hard-coded
globals. `make_context` grows three matching optional keywords (`run=`/`chdir=`/`prompt_input=`)
that thread straight through. `toolr.testing` ships a `RunMock` (composition over
`unittest.mock.Mock`, not a subclass) plus a `make_command_result` factory for building genuine
`CommandResult` objects.

**Tech Stack:** Python 3.11+, `msgspec.Struct` (frozen), `unittest.mock`, `pytest`, `rich.prompt`.

**Spec:** `specs/2026-08-14-testing-run-mock-design.md`

## Global Constraints

- New defaulted `Context` fields (`_run_impl`, `_chdir_impl`, `_prompt_stream`) must be added
  *after* `default_no_output_timeout_secs` — `msgspec` raises `TypeError: Required field '…' cannot
  follow optional fields` if a defaulted field precedes a required one.
- `CommandResult` is `frozen=True`. Never assign to an existing instance's fields — build a
  replacement via `msgspec.structs.replace(result, ...)`.
- `Context` is `frozen=True`. Never assign attributes on a `Context` instance (including methods) —
  it raises `AttributeError: immutable type: 'Context'`. All mocking goes through constructor
  fields, never instance-attribute assignment.
- No new third-party test dependency. `unittest.mock` (stdlib) is the only mocking library used;
  `RunMock` wraps a `Mock` instance, it does not subclass `MagicMock`.
- `Context` has a public stub at `crates/toolr-py/python/toolr/_context.pyi` — every new field on
  `Context` must be mirrored there or `mypy` breaks for callers.
- Conventional Commits for every commit message (`feat(testing): …`, `test(context): …`,
  `docs(testing): …`).
- Queue an `UNRELEASED.md` entry for the `toolr.testing.__all__` addition. No `CHANGELOG.md`
  hand-edits.
- Doc examples for the new mocking capabilities must be real, CI-executed pytest test functions
  wrapped in `# --8<-- [start:label]` / `# --8<-- [end:label]` markers and pulled into
  `docs/writing-commands/testing.md` via `pymdownx.snippets` (`--8<-- "tests/…:label"`) — never
  hand-typed markdown code blocks. This repo's own docs already have one silently-broken hand-typed
  example (`result.ctx.prompt = lambda *a, **k: False` — raises `AttributeError` against a real,
  frozen `Context`); this plan fixes that instance as part of Task 6.
- `cargo xtask build-skill-refs` does **not** need to run for this work — confirmed by reading
  `crates/xtask/src/build_skill_refs/authoring.rs`: its generator walks `toolr.__all__`, and
  `toolr.testing` isn't re-exported there.
- Verification scope per `CLAUDE.md`: this plan touches only Python (`crates/toolr-py/python/**`,
  `tests/**`, `docs/**`) — no `.rs` files change. Run `uv run pytest` after each task; run
  `mise run test` once at the end for the full umbrella (it includes the doc-snippet and skill-refs
  drift gates, which should both be no-ops here per the point above, but confirm rather than assume).

---

### Task 1: `Context._run_impl` seam

**Files:**

- Modify: `crates/toolr-py/python/toolr/_context.py` (add field near the other defaulted fields,
  change the one call site in `run()`)
- Modify: `crates/toolr-py/python/toolr/_context.pyi` (mirror the new field)
- Test: `tests/context/test_run.py`

**Interfaces:**

- Produces: `Context._run_impl: Callable[..., CommandResult[str] | CommandResult[bytes]]`, default
  `command.run`. `Context.run()`'s behavior is otherwise byte-for-byte unchanged (same info-line
  logging, same timeout-default resolution).

- [ ] **Step 1: Write the failing test**

Add to `tests/context/test_run.py` (new test, alongside the existing ones — check the existing
fixtures in that file first; this uses whatever `repo_root`/`temp_cwd`-style fixture the file
already has for constructing a real `Context`):

```python
def test_run_uses_run_impl_override(repo_root):
    from unittest.mock import Mock

    from toolr._context import Context
    from toolr.utils.command import CommandResult

    fake_result = CommandResult(args=["echo", "hi"], stdout=None, stderr=None, returncode=0)
    run_impl = Mock(return_value=fake_result)
    ctx = Context(
        repo_root=repo_root,
        parser=make_parser(),  # use this file's existing parser-building helper/fixture
        verbosity=ConsoleVerbosity.NORMAL,  # use this file's existing import for this enum
        _console_stderr=make_console(),  # use this file's existing console-building helper
        _console_stdout=make_console(),
        _run_impl=run_impl,
    )

    result = ctx.run("echo", "hi")

    assert result is fake_result
    run_impl.assert_called_once_with(
        ("echo", "hi"),
        stream_output=True,
        capture_output=False,
        timeout_secs=None,
        no_output_timeout_secs=None,
    )


def test_run_defaults_to_real_command_run(repo_root):
    """Omitting `_run_impl` at construction time keeps today's real-subprocess behavior."""
    from toolr._context import Context
    from toolr.utils import command

    ctx = Context(
        repo_root=repo_root,
        parser=make_parser(),
        verbosity=ConsoleVerbosity.NORMAL,
        _console_stderr=make_console(),
        _console_stdout=make_console(),
    )

    assert ctx._run_impl is command.run
```

Adapt the `make_parser()`/`make_console()`/fixture names to whatever this file's existing tests
already use to build a bare `Context` — read the top of `tests/context/test_run.py` and
`tests/context/conftest.py` first and reuse those helpers verbatim; don't invent new ones.

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/context/test_run.py -k run_impl -v`
Expected: FAIL — `Context.__init__()` (or `TypeError: unexpected keyword argument '_run_impl'`)
since the field doesn't exist yet.

- [ ] **Step 3: Add the field and wire the call site**

In `crates/toolr-py/python/toolr/_context.py`, add the import if not already present:

```python
from typing import Callable
```

Add the field after `default_no_output_timeout_secs: float | None = None`:

```python
    # Injectable subprocess runner. Defaults to the real `command.run`;
    # `toolr.testing.make_context(run=...)` overrides this for tests.
    _run_impl: Callable[..., CommandResult[str] | CommandResult[bytes]] = command.run
```

Change the one call site inside `run()` from:

```python
        return command.run(
```

to:

```python
        return self._run_impl(
```

- [ ] **Step 4: Mirror the field in the `.pyi` stub**

In `crates/toolr-py/python/toolr/_context.pyi`, add the matching field declaration in the same
position relative to the other fields (check the stub's existing field ordering and syntax style
first — it should mirror `_context.py` field-for-field).

- [ ] **Step 5: Run tests to verify they pass**

Run: `uv run pytest tests/context/test_run.py -v`
Expected: PASS, including all pre-existing tests in the file (confirms the real-`command.run`
default path is unchanged).

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-py/python/toolr/_context.py crates/toolr-py/python/toolr/_context.pyi tests/context/test_run.py
git commit -m "feat(context): make Context.run's subprocess runner injectable"
```

---

### Task 2: `Context._chdir_impl` seam

**Files:**

- Modify: `crates/toolr-py/python/toolr/_context.py`
- Modify: `crates/toolr-py/python/toolr/_context.pyi`
- Test: `tests/context/test_chdir.py` (new — no `chdir` tests exist yet)

**Interfaces:**

- Consumes: nothing from Task 1.
- Produces: `Context._chdir_impl: Callable[[str | pathlib.Path], None]`, default `os.chdir`.
- [ ] **Step 1: Write the failing test**

Create `tests/context/test_chdir.py`:

```python
"""Tests for `Context.chdir`, including the `_chdir_impl` injection seam."""

from __future__ import annotations

import pathlib
from unittest.mock import Mock

from toolr._context import Context


def test_chdir_uses_real_os_chdir_by_default(repo_root, tmp_path):
    """The default path still really moves the process cwd (and restores it)."""
    ctx = Context(
        repo_root=repo_root,
        parser=make_parser(),
        verbosity=ConsoleVerbosity.NORMAL,
        _console_stderr=make_console(),
        _console_stdout=make_console(),
    )
    target = tmp_path / "somewhere"
    target.mkdir()
    original_cwd = pathlib.Path.cwd()

    with ctx.chdir(target) as p:
        assert p == target
        assert pathlib.Path.cwd() == target

    assert pathlib.Path.cwd() == original_cwd


def test_chdir_uses_chdir_impl_override_without_touching_real_cwd(repo_root, tmp_path):
    chdir_impl = Mock()
    ctx = Context(
        repo_root=repo_root,
        parser=make_parser(),
        verbosity=ConsoleVerbosity.NORMAL,
        _console_stderr=make_console(),
        _console_stdout=make_console(),
        _chdir_impl=chdir_impl,
    )
    target = tmp_path / "somewhere"
    original_cwd = pathlib.Path.cwd()

    with ctx.chdir(target):
        pass

    # Called twice: once to enter, once to restore.
    chdir_impl.assert_any_call(target)
    chdir_impl.assert_any_call(original_cwd)
    assert chdir_impl.call_count == 2
    # The real process cwd never moved.
    assert pathlib.Path.cwd() == original_cwd
```

Adapt `make_parser()`/`make_console()`/`repo_root` to whatever helpers `tests/context/conftest.py`
already provides (same note as Task 1 — reuse, don't reinvent).

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/context/test_chdir.py -v`
Expected: FAIL — `_chdir_impl` isn't a valid keyword yet.

- [ ] **Step 3: Add the field and wire both call sites**

In `crates/toolr-py/python/toolr/_context.py`, add after `_run_impl`:

```python
    # Injectable cwd-changer. Defaults to the real `os.chdir`;
    # `toolr.testing.make_context(chdir=...)` overrides this for tests
    # that don't want to mutate the real test-process cwd.
    _chdir_impl: Callable[[str | pathlib.Path], None] = os.chdir
```

In `chdir()`, change both `os.chdir(path)` and `os.chdir(cwd)` call sites to
`self._chdir_impl(path)` and `self._chdir_impl(cwd)` respectively.

- [ ] **Step 4: Mirror the field in the `.pyi` stub**

Same as Task 1 Step 4, for `_chdir_impl`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `uv run pytest tests/context/test_chdir.py -v`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-py/python/toolr/_context.py crates/toolr-py/python/toolr/_context.pyi tests/context/test_chdir.py
git commit -m "feat(context): make Context.chdir's os.chdir call injectable"
```

---

### Task 3: `Context._prompt_stream` seam

**Files:**

- Modify: `crates/toolr-py/python/toolr/_context.py`
- Modify: `crates/toolr-py/python/toolr/_context.pyi`
- Test: `tests/context/test_prompt.py`

**Interfaces:**

- Consumes: nothing from Tasks 1–2.
- Produces: `Context._prompt_stream: TextIO | None`, default `None` (real stdin, unchanged).
  `prompt()` forwards it to the already-existing `_prompt(..., stream=...)` parameter.
- [ ] **Step 1: Write the failing test**

Add to `tests/context/test_prompt.py` (reuse whatever fixtures the file already has for building a
`Context` and its consoles — read the top of the file first):

```python
def test_prompt_uses_prompt_stream_override(repo_root):
    import io

    from toolr._context import Context

    ctx = Context(
        repo_root=repo_root,
        parser=make_parser(),
        verbosity=ConsoleVerbosity.NORMAL,
        _console_stderr=make_console(),
        _console_stdout=make_console(),
        _prompt_stream=io.StringIO("yes\n"),
    )

    assert ctx.prompt("Continue?", bool) is True


def test_prompt_stream_defaults_to_none(repo_root):
    """Omitting `_prompt_stream` keeps today's real-stdin behavior."""
    from toolr._context import Context

    ctx = Context(
        repo_root=repo_root,
        parser=make_parser(),
        verbosity=ConsoleVerbosity.NORMAL,
        _console_stderr=make_console(),
        _console_stdout=make_console(),
    )

    assert ctx._prompt_stream is None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/context/test_prompt.py -k prompt_stream -v`
Expected: FAIL — `_prompt_stream` isn't a valid keyword yet, and `prompt()` doesn't forward a
`stream` to `_prompt()`.

- [ ] **Step 3: Add the field and wire `prompt()`**

In `crates/toolr-py/python/toolr/_context.py`, add after `_chdir_impl`:

```python
    # Injectable input stream for `prompt()`. Defaults to `None` (real
    # stdin, via rich's own default); `toolr.testing.make_context(prompt_input=...)`
    # overrides this for tests.
    _prompt_stream: TextIO | None = None
```

`TextIO` is already imported at the top of the file (used in `_prompt`'s own signature) — confirm,
don't re-add if present.

Change `prompt()`'s single call to `self._prompt(...)` to also pass `stream=self._prompt_stream`:

```python
        return self._prompt(
            prompt,
            expected_type,
            password=password,
            case_sensitive=case_sensitive,
            choices=choices,
            default=default,
            show_default=show_default,
            show_choices=show_choices,
            stream=self._prompt_stream,
        )
```

- [ ] **Step 4: Mirror the field in the `.pyi` stub**

Same as Task 1 Step 4, for `_prompt_stream`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `uv run pytest tests/context/test_prompt.py -v`
Expected: PASS, including every pre-existing test in the file.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-py/python/toolr/_context.py crates/toolr-py/python/toolr/_context.pyi tests/context/test_prompt.py
git commit -m "feat(context): make Context.prompt's input stream injectable"
```

---

### Task 4: `RunMock` and `make_command_result`

**Files:**

- Create: `crates/toolr-py/python/toolr/testing/_run_mock.py`
- Modify: `crates/toolr-py/python/toolr/testing/__init__.py` (export the two new names)
- Test: `tests/testing/__init__.py` (new, empty — matches the `tests/context/__init__.py` pattern
  already in this repo)
- Test: `tests/testing/test_run_mock.py` (new)

**Interfaces:**

- Consumes: `toolr.utils.command.CommandResult` (existing).
- Produces:
    - `make_command_result(...) -> CommandResult[str] | CommandResult[bytes]`, keyword-only
      `args: list[str] | None = None`, `stdout: str | bytes = ""`, `stderr: str | bytes = ""`,
      `returncode: int = 0`.
    - `class RunMock:` with `__call__(self, cmdline: tuple[str, ...], **kwargs) -> CommandResult`;
      `.register(...)` (keyword-only `*cmdline: str`, `stdout: str | bytes = ""`,
      `stderr: str | bytes = ""`, `returncode: int = 0`, `occurrences: int | None = None`) `-> None`;
      and forwarded `assert_called_with`, `assert_any_call`, `assert_called_once_with`, `call_args`
      (property), `call_args_list` (property), `call_count` (property), `reset_mock`.
    - Task 5 consumes both by importing `RunMock`/`make_command_result` directly.
- [ ] **Step 1: Write the failing tests**

Create `tests/testing/__init__.py` (empty file — check `tests/context/__init__.py` exists and is
empty first; mirror it).

Create `tests/testing/test_run_mock.py`:

```python
"""Unit tests for `toolr.testing.RunMock` and `make_command_result`."""

from __future__ import annotations

import io

import pytest

from toolr.testing._run_mock import RunMock
from toolr.testing._run_mock import make_command_result


def test_make_command_result_builds_str_streams():
    result = make_command_result(stdout="out", stderr="err", returncode=0)
    assert result.stdout.read() == "out"
    assert result.stderr.read() == "err"
    assert result.returncode == 0


def test_make_command_result_builds_bytes_streams():
    result = make_command_result(stdout=b"out", stderr=b"err")
    assert result.stdout.read() == b"out"
    assert result.stderr.read() == b"err"


def test_make_command_result_mints_fresh_streams_each_call():
    """Two calls with the same args must not share one exhausted stream."""
    first = make_command_result(stdout="out")
    second = make_command_result(stdout="out")
    assert first.stdout.read() == "out"
    assert second.stdout.read() == "out"  # not "" — it's a fresh stream


def test_run_mock_records_calls():
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="up to date\n")

    run_mock(("git", "fetch"), stream_output=True, capture_output=True)

    run_mock.assert_called_once_with(("git", "fetch"), stream_output=True, capture_output=True)
    assert run_mock.call_count == 1


def test_run_mock_resolves_registration_by_longest_prefix():
    run_mock = RunMock()
    run_mock.register("git", stdout="generic git output\n")
    run_mock.register("git", "fetch", stdout="fetch-specific output\n")

    result = run_mock(("git", "fetch"), capture_output=True)

    assert result.stdout.read() == "fetch-specific output\n"


def test_run_mock_forces_stdout_and_stderr_none_without_capture_output():
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="up to date\n")

    result = run_mock(("git", "fetch"), capture_output=False)

    assert result.stdout is None
    assert result.stderr is None


def test_run_mock_raises_on_unregistered_call():
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="up to date\n")

    with pytest.raises(AssertionError, match="git.*push"):
        run_mock(("git", "push"), capture_output=True)


def test_run_mock_occurrences_exhausts_then_raises():
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="first\n", occurrences=1)
    run_mock.register("git", "fetch", stdout="second\n")

    first = run_mock(("git", "fetch"), capture_output=True)
    second = run_mock(("git", "fetch"), capture_output=True)

    assert first.stdout.read() == "first\n"
    assert second.stdout.read() == "second\n"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `uv run pytest tests/testing/test_run_mock.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'toolr.testing._run_mock'`.

- [ ] **Step 3: Write the implementation**

Create `crates/toolr-py/python/toolr/testing/_run_mock.py`:

```python
"""Canonical mock for `Context.run`, for use with `toolr.testing.make_context(run=...)`."""

from __future__ import annotations

import io
from typing import TYPE_CHECKING
from unittest.mock import Mock

import msgspec

from toolr.utils.command import CommandResult

if TYPE_CHECKING:
    from collections.abc import Callable


def make_command_result(
    *,
    args: list[str] | None = None,
    stdout: str | bytes = "",
    stderr: str | bytes = "",
    returncode: int = 0,
) -> CommandResult[str] | CommandResult[bytes]:
    """Build a genuine `CommandResult`, with fresh `stdout`/`stderr` streams."""
    stdout_stream = io.BytesIO(stdout) if isinstance(stdout, bytes) else io.StringIO(stdout)
    stderr_stream = io.BytesIO(stderr) if isinstance(stderr, bytes) else io.StringIO(stderr)
    return CommandResult(
        args=args or [], stdout=stdout_stream, stderr=stderr_stream, returncode=returncode
    )


class _Registration:
    __slots__ = ("prefix", "make_result", "occurrences")

    def __init__(
        self,
        prefix: tuple[str, ...],
        make_result: Callable[[], CommandResult[str] | CommandResult[bytes]],
        occurrences: int | None,
    ) -> None:
        self.prefix = prefix
        self.make_result = make_result
        self.occurrences = occurrences


class RunMock:
    """Drop-in for `Context._run_impl`. Wraps a `Mock`, not a `MagicMock` subclass.

    Standard `Mock` assertions are available via the forwarded methods below.
    `.register(...)` is declarative sugar for the common "these exact args
    return this exact result" case; `side_effect`/`return_value` on the
    underlying `Mock` (accessible as `run_mock.mock`) are the escape hatch
    for anything `.register(...)` can't express.
    """

    def __init__(self) -> None:
        self.mock = Mock(name="RunMock")
        self._registrations: list[_Registration] = []

    def register(
        self,
        *cmdline: str,
        stdout: str | bytes = "",
        stderr: str | bytes = "",
        returncode: int = 0,
        occurrences: int | None = None,
    ) -> None:
        # Only `side_effect` is checked here, not `return_value` — a fresh
        # `Mock()`'s `return_value` is itself a lazily auto-vivified child
        # `Mock`, not a distinguishable "unset" sentinel via the public API,
        # so there's nothing reliable to compare it against. Registrations
        # always take priority when present (see `__call__`); the only real
        # conflict worth guarding against is a `side_effect` that would
        # otherwise silently never run.
        if self.mock.side_effect is not None:
            msg = "cannot mix RunMock.register(...) with a directly-configured side_effect"
            raise TypeError(msg)

        def make_result() -> CommandResult[str] | CommandResult[bytes]:
            return make_command_result(
                args=list(cmdline), stdout=stdout, stderr=stderr, returncode=returncode
            )

        self._registrations.append(_Registration(cmdline, make_result, occurrences))

    def __call__(
        self, cmdline: tuple[str, ...], **kwargs: object
    ) -> CommandResult[str] | CommandResult[bytes]:
        # Records the call once and resolves `side_effect`/`return_value`
        # in the same step — no need to call `self.mock` a second time to
        # get "the" resolved value; this *is* it.
        recorded_result = self.mock(cmdline, **kwargs)

        result = self._resolve(cmdline) if self._registrations else recorded_result

        if not kwargs.get("capture_output"):
            result = msgspec.structs.replace(result, stdout=None, stderr=None)
        return result

    def _resolve(self, cmdline: tuple[str, ...]) -> CommandResult[str] | CommandResult[bytes]:
        best: _Registration | None = None
        for reg in self._registrations:
            if cmdline[: len(reg.prefix)] == reg.prefix and (
                best is None or len(reg.prefix) > len(best.prefix)
            ):
                if reg.occurrences is not None and reg.occurrences <= 0:
                    continue
                best = reg
        if best is None:
            msg = f"RunMock: no registration matches {cmdline!r}"
            raise AssertionError(msg)
        if best.occurrences is not None:
            best.occurrences -= 1
        return best.make_result()

    # Explicit, fixed forwarding to the underlying Mock — not a blanket
    # __getattr__, so a genuine typo raises AttributeError instead of
    # silently returning a fresh child Mock.
    def assert_called_with(self, *args: object, **kwargs: object) -> None:
        self.mock.assert_called_with(*args, **kwargs)

    def assert_any_call(self, *args: object, **kwargs: object) -> None:
        self.mock.assert_any_call(*args, **kwargs)

    def assert_called_once_with(self, *args: object, **kwargs: object) -> None:
        self.mock.assert_called_once_with(*args, **kwargs)

    @property
    def call_args(self):  # noqa: ANN201 - mirrors unittest.mock's own (untyped) property
        return self.mock.call_args

    @property
    def call_args_list(self):  # noqa: ANN201
        return self.mock.call_args_list

    @property
    def call_count(self) -> int:
        return self.mock.call_count

    def reset_mock(self, *args: object, **kwargs: object) -> None:
        self.mock.reset_mock(*args, **kwargs)
        self._registrations.clear()
```

**Design note for the implementer:** the `_registrations` exhaustion check
(`reg.occurrences <= 0`) means an exhausted registration is skipped in favor of the next matching
one, or "unregistered" if none remain — this is what `test_run_mock_occurrences_exhausts_then_raises`
above exercises (first registration handles call 1, then falls through to the second for call 2).

- [ ] **Step 4: Export the two new names**

In `crates/toolr-py/python/toolr/testing/__init__.py`, add:

```python
from toolr.testing._run_mock import RunMock
from toolr.testing._run_mock import make_command_result
```

and add `"RunMock"` and `"make_command_result"` to `__all__` (alphabetical, matching the existing
sort order in that list).

- [ ] **Step 5: Run tests to verify they pass**

Run: `uv run pytest tests/testing/test_run_mock.py -v`
Expected: PASS, all 8 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-py/python/toolr/testing/_run_mock.py crates/toolr-py/python/toolr/testing/__init__.py tests/testing/__init__.py tests/testing/test_run_mock.py
git commit -m "feat(testing): add RunMock and make_command_result"
```

---

### Task 5: `ContextForTesting` + wire `run=`/`chdir=`/`prompt_input=` into `make_context`

**This task's scope grew mid-plan** (see the spec's "Amendment (2026-08-14, mid-implementation)"
section — read it before starting). `make_context` no longer returns a `ContextForTesting` wrapper;
it returns a `ContextForTesting`, a `Context` subclass, directly. `CapturedOutput` and
`ContextForTesting` are deleted, not deprecated. If a Task 1 commit already added a `run=` param to
`make_context` using the old wrapper shape, and migrated `tests/context/test_core.py`/
`tests/context/test_run.py` to call `make_context(..., run=...).ctx`/`.output`, **all of that is
superseded here** — this task replaces it wholesale with the flat `ContextForTesting` shape. Check
`git log -p -- crates/toolr-py/python/toolr/testing/_make_context.py` for what's actually there
before writing new code, rather than assuming the file matches this plan's original Task 5 text.

**Files:**

- Modify: `crates/toolr-py/python/toolr/testing/_make_context.py` (delete `CapturedOutput`/
  `ContextForTesting`, add `ContextForTesting`, rewrite `make_context`)
- Modify: `crates/toolr-py/python/toolr/testing/__init__.py` (replace the `CapturedOutput`/
  `ContextForTesting` exports with `ContextForTesting`)
- Test: `tests/test_make_context.py` (rewrite every test to the flat shape)
- Test: `tests/context/test_core.py` (if Task 1 touched it — rewrite to the flat shape; if Task 1
  didn't touch it, leave it alone, it's out of this task's concern otherwise)
- Test: `tests/context/test_run.py` (same as above)

**Interfaces:**

- Consumes: `Context._run_impl`/`_chdir_impl`/`_prompt_stream` (Tasks 1–3).
- Produces: `class ContextForTesting(Context, frozen=True)` with `stdout`/`stderr` properties;
  `make_context(repo_root, *, verbosity=..., default_timeout_secs=None,
  default_no_output_timeout_secs=None, run=None, chdir=None, prompt_input=None) -> ContextForTesting`.
  Task 6 consumes this shape directly (`ctx.run(...)`, `ctx.stdout`).
- [ ] **Step 1: Write the failing tests**

Rewrite `tests/test_make_context.py` in full to the flat shape (replace every existing test in the
file — they currently use the `result.ctx`/`result.output.stdout` wrapper shape, which no longer
exists):

```python
"""Tests for `toolr.testing.make_context`."""

from __future__ import annotations

from pathlib import Path

import pytest

from toolr.testing import make_context


def test_make_context_sets_repo_root(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    assert ctx.repo_root == tmp_path


def test_make_context_captures_stdout(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    ctx.print("hello, world")
    assert "hello, world" in ctx.stdout


def test_make_context_captures_stderr(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    ctx.error("something broke")
    assert "something broke" in ctx.stderr


def test_make_context_exit_raises_system_exit(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(2, "bad input")
    assert exc_info.value.code == 2


def test_make_context_run_param_wires_into_ctx_run(tmp_path):
    from toolr.testing import RunMock

    run_mock = RunMock()
    run_mock.register("echo", "hi", stdout="hi\n")

    ctx = make_context(tmp_path, run=run_mock)
    output = ctx.run("echo", "hi", capture_output=True)

    assert output.stdout.read() == "hi\n"
    run_mock.assert_called_once_with(
        ("echo", "hi"),
        stream_output=True,
        capture_output=True,
        timeout_secs=None,
        no_output_timeout_secs=None,
    )


def test_make_context_run_param_omitted_uses_real_runner(tmp_path):
    from toolr.utils import command

    ctx = make_context(tmp_path)

    assert ctx._run_impl is command.run


def test_make_context_chdir_param_wires_into_ctx_chdir(tmp_path):
    from unittest.mock import Mock

    chdir_mock = Mock()
    ctx = make_context(tmp_path, chdir=chdir_mock)
    target = tmp_path / "somewhere"
    target.mkdir()

    with ctx.chdir(target):
        pass

    chdir_mock.assert_any_call(target)
    assert chdir_mock.call_count == 2


def test_make_context_prompt_input_param_feeds_a_canned_answer(tmp_path):
    ctx = make_context(tmp_path, prompt_input="yes\n")

    assert ctx.prompt("Continue?", bool) is True


def test_make_context_prompt_input_omitted_defaults_to_none(tmp_path):
    ctx = make_context(tmp_path)

    assert ctx._prompt_stream is None


def test_make_context_prompt_input_exhaustion_raises_instead_of_hanging(tmp_path):
    ctx = make_context(tmp_path, prompt_input="")  # no answers provided

    with pytest.raises(EOFError):
        ctx.prompt("Continue?", bool)  # no `default=` — would hang on a plain StringIO


def test_make_context_returns_a_context_subclass(tmp_path):
    from toolr._context import Context

    ctx = make_context(tmp_path)

    assert isinstance(ctx, Context)
```

Also update `tests/context/test_core.py` and `tests/context/test_run.py` the same way, **only if**
a prior task's commits already call `make_context(...)` in those files — grep first
(`grep -n "make_context" tests/context/test_core.py tests/context/test_run.py`) and rewrite every
`result_ctx = make_context(...)` / `result_ctx.ctx` / `result_ctx.output` occurrence to the flat
`ctx = make_context(...)` / `ctx` shape. If those files don't call `make_context` at all yet
(construct `Context(...)` directly instead), leave them untouched — that's correct, unrelated to
this task.

- [ ] **Step 2: Run tests to verify they fail**

Run: `uv run pytest tests/test_make_context.py -v`
Expected: FAIL — `make_context` doesn't return a `ContextForTesting` yet (or, if Task 1's `run=`
addition is present, the old wrapper shape's `.ctx`/`.output` attributes don't exist on whatever
this rewritten test file now expects).

- [ ] **Step 3: Implement the EOF-on-exhaustion stream wrapper**

In `crates/toolr-py/python/toolr/testing/_make_context.py`, add near the top:

```python
class _EOFOnExhaustionStringIO(io.StringIO):
    """Raises `EOFError` on `readline()` once exhausted, instead of returning `""` forever.

    A plain `io.StringIO` returns `""` past its end, and `rich`'s prompt retry
    loop treats that as "keep asking" rather than "no more input" unless a
    `default` was set — an under-provisioned `prompt_input` in a test would
    otherwise hang the test suite instead of failing it.
    """

    def readline(self, size: int = -1) -> str:
        line = super().readline(size)
        if line == "":
            msg = "prompt_input exhausted — no more canned answers to read"
            raise EOFError(msg)
        return line
```

- [ ] **Step 4: Replace `CapturedOutput`/`ContextForTesting` with `ContextForTesting`**

In `crates/toolr-py/python/toolr/testing/_make_context.py`, delete the `CapturedOutput` and
`ContextForTesting` classes entirely. Add:

```python
class ContextForTesting(Context, frozen=True):
    """A `Context` returned by `make_context`, with captured-output accessors.

    Every `Context` method works unmodified (`run`, `print`, `chdir`, `prompt`, `exit`, …) —
    this only adds `stdout`/`stderr` properties reading back what was written through the
    captured consoles `make_context` wires up.
    """

    @property
    def stdout(self) -> str:
        """Everything written to `print`/`info` so far."""
        return self._console_stdout.file.getvalue()

    @property
    def stderr(self) -> str:
        """Everything written to `error`/`warn`/`debug` so far."""
        return self._console_stderr.file.getvalue()
```

- [ ] **Step 5: Add the three new parameters and rewrite `make_context`'s body**

Change `make_context`'s signature to:

```python
def make_context(
    repo_root: Path,
    *,
    verbosity: ConsoleVerbosity = ConsoleVerbosity.NORMAL,
    default_timeout_secs: float | None = None,
    default_no_output_timeout_secs: float | None = None,
    run: Callable[..., CommandResult[str] | CommandResult[bytes]] | None = None,
    chdir: Callable[[str | Path], None] | None = None,
    prompt_input: str | TextIO | None = None,
) -> ContextForTesting:
```

Add the necessary imports (`Callable`, `TextIO`, `CommandResult` under `TYPE_CHECKING` — check
what's already imported under the existing `if TYPE_CHECKING:` block and extend it rather than
duplicating).

Update the docstring: change the `Returns:` section (a `ContextForTesting` now, not a
`ContextForTesting`/captured-output pair), and add the three new params to `Args:`, following the
existing docstring's one-line-per-param style (Google-style — see
`docs/writing-commands/docstrings.md` if unsure).

Where the return value is built, replace the old two-object construction
(`Context(...)` + `ContextForTesting(ctx=ctx, output=CapturedOutput(...))`) with a single
`ContextForTesting(...)` call, conditionally including the three new fields (so omitting a param keeps
`Context`'s own default rather than explicitly passing `None` and overriding a non-`None` class
default — this matters for `_run_impl`, whose default is `command.run`, not `None`):

```python
    context_kwargs: dict[str, object] = {}
    if run is not None:
        context_kwargs["_run_impl"] = run
    if chdir is not None:
        context_kwargs["_chdir_impl"] = chdir
    if prompt_input is not None:
        if isinstance(prompt_input, str):
            prompt_input = _EOFOnExhaustionStringIO(prompt_input)
        context_kwargs["_prompt_stream"] = prompt_input

    return ContextForTesting(
        repo_root=repo_root,
        parser=parser,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
        default_timeout_secs=default_timeout_secs,
        default_no_output_timeout_secs=default_no_output_timeout_secs,
        **context_kwargs,
    )
```

(Adjust to match however the existing `parser`/`console_stderr`/`console_stdout` construction is
actually laid out in this file — that part is unaffected by this change, only the final return
value's shape changes.)

- [ ] **Step 6: Update the `toolr.testing` package exports**

In `crates/toolr-py/python/toolr/testing/__init__.py`, remove the `CapturedOutput` and
`ContextForTesting` imports/`__all__` entries, add `ContextForTesting` in their place (alphabetical
position in `__all__`, matching the existing sort order).

- [ ] **Step 7: Run tests to verify they pass**

Run: `uv run pytest tests/test_make_context.py tests/context/ -v`
Expected: PASS, including every test in both locations (this also catches any leftover
`.ctx`/`.output` reference Step 1 missed).

- [ ] **Step 8: Commit**

```bash
git add crates/toolr-py/python/toolr/testing/_make_context.py crates/toolr-py/python/toolr/testing/__init__.py tests/test_make_context.py tests/context/test_core.py tests/context/test_run.py
git commit -m "feat(testing): make_context returns ContextForTesting directly, add run/chdir/prompt_input"
```

(Drop `tests/context/test_core.py`/`tests/context/test_run.py` from the `git add` if this task
didn't end up touching them — see Step 1's grep-first note.)

---

### Task 6: Docs — fix the broken example, add real mocking examples via snippet includes

**Files:**

- Modify: `docs/writing-commands/testing.md`
- Modify: `tests/context/test_prompt.py` (add one more snippet-marked example test)
- Modify: `tests/context/test_chdir.py` (add one more snippet-marked example test)
- Modify: `tests/testing/test_run_mock.py` or `tests/test_make_context.py` (add one more
  snippet-marked example test — use whichever already has a `make_context(run=...)` example from
  Task 5 and add the marker there instead of duplicating)

**Interfaces:**

- Consumes: `RunMock` (Task 4), `make_context(run=/chdir=/prompt_input=)` (Task 5).
- Produces: nothing consumed by later tasks — this is the terminal doc-facing task.
- [ ] **Step 1: Mark the existing Task 5 test as a doc snippet**

In `tests/test_make_context.py`, wrap `test_make_context_run_param_wires_into_ctx_run` (written in
Task 5) with section markers, on their own comment lines immediately inside the function:

```python
def test_make_context_run_param_wires_into_ctx_run(tmp_path):
    # --8<-- [start:mock-run-example]
    from toolr.testing import RunMock

    run_mock = RunMock()
    run_mock.register("echo", "hi", stdout="hi\n")

    result = make_context(tmp_path, run=run_mock)
    output = result.ctx.run("echo", "hi", capture_output=True)

    assert output.stdout.read() == "hi\n"
    run_mock.assert_called_once_with(
        ("echo", "hi"),
        stream_output=True,
        capture_output=True,
        timeout_secs=None,
        no_output_timeout_secs=None,
    )
    # --8<-- [end:mock-run-example]
```

- [ ] **Step 2: Mark the Task 5 chdir test as a doc snippet**

In `tests/test_make_context.py`, wrap `test_make_context_chdir_param_wires_into_ctx_chdir` the same
way with `# --8<-- [start:mock-chdir-example]` / `# --8<-- [end:mock-chdir-example]` around its body
(everything after the docstring/blank line, same pattern as Step 1).

- [ ] **Step 3: Mark the Task 5 prompt test as a doc snippet**

In `tests/test_make_context.py`, wrap `test_make_context_prompt_input_param_feeds_a_canned_answer`
the same way with `# --8<-- [start:mock-prompt-example]` / `# --8<-- [end:mock-prompt-example]`.

- [ ] **Step 4: Run the marked tests to confirm they still pass with the markers present**

Run: `uv run pytest tests/test_make_context.py -v`
Expected: PASS — comments don't affect test execution; this just confirms nothing was broken while
editing.

- [ ] **Step 5: Rewrite the "Calling a command directly" section of the docs**

In `docs/writing-commands/testing.md`, replace the existing broken example
(`result.ctx.prompt = lambda *a, **k: False`) and the paragraph after it. Read the current file's
"Calling a command directly" section first (`## Calling a command directly` heading through the
following `## Stability` heading) to see exactly what to replace — the broken snippet is the
`test_confirm_aborts_on_no` function inside the first ` ```python ` block in that section.

Replace that whole function with a note plus the real, working replacement:

````markdown
Prior to this, stubbing `ctx.prompt` by assigning a lambda directly onto `result.ctx.prompt` looked
tempting — don't: `Context` is a frozen `msgspec.Struct`, and that assignment raises
`AttributeError: immutable type: 'Context'`. Feed canned answers through `make_context`'s
`prompt_input` parameter instead:

```python
--8<-- "tests/test_make_context.py:mock-prompt-example"
```

### Mocking `ctx.run`

`make_context`'s `run` parameter replaces the real subprocess runner with a test double. Pass a
`toolr.testing.RunMock`, register the commands your code under test is expected to run, and assert
on it with the standard `Mock`-style API:

```python
--8<-- "tests/test_make_context.py:mock-run-example"
```

### Mocking `ctx.chdir`

`chdir()` calls the real `os.chdir` twice (enter and restore) by default — fine for most tests, but
a test that wants to assert *which* directory a command tried to move into, without touching the
real filesystem, passes a bare `unittest.mock.Mock` as `chdir`:

```python
--8<-- "tests/test_make_context.py:mock-chdir-example"
```
````

- [ ] **Step 6: Update the closing paragraph of that section**

The paragraph immediately after the code examples currently reads (approximately):

> `result.ctx` is a real `Context` — `ctx.repo_root` is set, `ctx.run(...)` calls the real
> subprocess runner (monkeypatch it if a test needs to intercept it), and `ctx.exit(...)` raises
> `SystemExit` via a real `ArgumentParser`, exactly as it does under the CLI. `result.output.stdout`
> / `result.output.stderr` capture everything written through `ctx.print`/`ctx.info`/`ctx.error`/etc.

Update the `ctx.run(...)` clause to describe the new capability instead of telling readers to
monkeypatch it themselves:

> `result.ctx` is a real `Context` — `ctx.repo_root` is set, `ctx.exit(...)` raises `SystemExit` via
> a real `ArgumentParser`, exactly as it does under the CLI, and `ctx.run`/`ctx.chdir`/`ctx.prompt`
> all run for real unless you pass `run=`/`chdir=`/`prompt_input=` to `make_context` (see above).
> `result.output.stdout`/`result.output.stderr` capture everything written through
> `ctx.print`/`ctx.info`/`ctx.error`/etc.

- [ ] **Step 7: Build the docs to confirm the snippet includes resolve**

Run: `uv run mkdocs build --strict`
Expected: builds cleanly with no warnings about missing snippet sources (`pymdownx.snippets` errors
loudly if a `--8<-- "path:label"` target doesn't exist or the label isn't found).

- [ ] **Step 8: Commit**

```bash
git add docs/writing-commands/testing.md tests/test_make_context.py
git commit -m "docs(testing): fix broken ctx.prompt example, document run/chdir/prompt_input mocking"
```

---

### Task 7: Release notes and final full-suite verification

**Files:**

- Modify: `UNRELEASED.md`

**Interfaces:**

- Consumes: nothing new — this is the wrap-up task.

- [ ] **Step 1: Add the release-notes entry**

Append to `UNRELEASED.md`, matching the existing bullet style in that file (short narrative, present
tense, explains the *why* briefly):

```markdown
- `toolr.testing.make_context` gained `run`/`chdir`/`prompt_input` parameters, and
  `toolr.testing.RunMock`/`make_command_result` ship as canonical test doubles for `ctx.run`.
  `Context.run`/`chdir`/`prompt` previously had no supported way to intercept them without
  monkeypatching internals; each now reads from an injectable, defaulted field
  (`_run_impl`/`_chdir_impl`/`_prompt_stream`) that `make_context` can override.
```

- [ ] **Step 2: Run the full umbrella test suite**

Run: `mise run test`
Expected: PASS. This runs the skill-refs drift gate (should be a no-op — confirmed in Global
Constraints that `toolr.testing` isn't part of that generator's input), `cargo test --workspace`
(should be unaffected — no `.rs` files changed), and `uv run pytest` (covers everything from Tasks
1–6). Per this repo's own guidance, poll a long-running `cargo test --workspace` rather than
assuming it completed — check the output rather than fire-and-forget.

- [ ] **Step 3: Run the pre-commit hooks across all files**

Run: `prek run --all-files`
Expected: PASS (this also re-validates `mkdocs build --strict` and `typos`, both already checked in
Task 6, but running the full hook set once at the end catches anything Task 6's narrower check
missed).

- [ ] **Step 4: Commit**

```bash
git add UNRELEASED.md
git commit -m "docs(changelog): queue release notes for mockable run/chdir/prompt"
```

---

## Out of scope (per spec)

- Migrating the separate caller project's three `_fakes.py` files — follow-up PR in that project's
  own repo, after a `toolr-py` release ships this.
- Mocking `Context.which` — already testable via its existing `path=` kwarg.
- A courtesy update to `skills/toolr-command-authoring/SKILL.md`'s hand-written prose mentioning
  `make_context`/`CommandsTester` — optional, not gated by CI, not required for this plan to be
  complete. Do it as a fast-follow if there's time, not as a blocking step here.
