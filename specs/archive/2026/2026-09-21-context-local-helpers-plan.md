# Context-Local Helpers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `toolr.current_context() -> Context`, a `contextvars`-backed ambient
accessor a command-authoring helper can call instead of taking `ctx` as a
parameter — purely additive alongside the existing explicit-`ctx` convention.

**Architecture:** A module-private `ContextVar[Context]` in `toolr/_context.py`,
set once by `toolr/_runner.py::run()` right after building the real `Context`
and before either dispatch path calls the target function. `current_context()`
reads it and raises `NoCurrentContextError` (a new `ToolrError` subclass) when
unset. `toolr.testing.set_current_context(ctx)` is the test-only context
manager that sets/resets it around a `with` block, so tests keep the
`Context`-construction-is-pure invariant `make_context()` already has.

**Tech Stack:** Python 3.11+ (`contextvars` stdlib), `msgspec`, `pytest`.

**Spec:** `specs/2026-09-21-context-local-helpers-design.md`

## Global Constraints

- `ctx` stays the required first argument for every `@command`-decorated
  function — unchanged, not part of this feature.
- `current_context()` is the only public accessor — no `current_context_or_none()`.
- Raises `NoCurrentContextError` (subclasses `toolr._exc.ToolrError`), never a
  bare `RuntimeError` — matches this codebase's existing exception convention
  (`SignatureParameterError`, `SignatureError`).
- `make_context()` stays side-effect-free: it must never set the contextvar
  itself. Only `toolr.testing.set_current_context()` (test path) and
  `toolr/_runner.py::run()` (production path) may call `.set()`.
- No new `toolr.testing`/`toolr.__all__` generator needed — `cargo xtask
  build-skill-refs` already walks both; run it after adding the two new
  exports.
- Exception classes are not added to `toolr.__all__` — `SignatureParameterError`/
  `SignatureError` aren't either; `NoCurrentContextError` follows the same
  convention (importable from `toolr._exc`, not re-exported).

## Model Assignment

Not every task needs the same model. Assignments below, and why:

- **Sonnet** — Tasks 1, 2, 5, 7: correct but well-specified mechanical work
  (contextvar plumbing, a context manager, doc prose, running/triaging a test
  suite) where the exact shape is already nailed down by this plan or the spec.
- **Opus** — Task 3 only: placing `.set()` at the one line in `_runner.py`
  where getting the order wrong relative to `_import_target()` silently breaks
  the feature's core claim (this exact mistake was caught once already during
  spec review — see the design doc's "Correction" note). The highest
  reasoning-to-mechanical-work ratio in this plan.
- **Haiku** — Tasks 4, 6: pure mechanical additions (two `__all__` entries plus
  a generator command; one `UNRELEASED.md` paragraph) with no design judgment
  left to make.

---

### Task 1: `current_context()` accessor and `NoCurrentContextError`

**Suggested model:** Sonnet

**Files:**

- Modify: `crates/toolr-py/python/toolr/_exc.py`
- Modify: `crates/toolr-py/python/toolr/_context.py`
- Test: `tests/context/test_current_context.py` (new)

**Interfaces:**

- Consumes: nothing new.
- Produces: `toolr._exc.NoCurrentContextError` (subclasses `ToolrError`);
  `toolr._context.current_context() -> Context`; the module-private
  `toolr._context._current_ctx: ContextVar[Context]` that Tasks 2 and 3 both
  call `.set()`/`.reset()` on.
- [ ] **Step 1: Write the failing test**

```python
# tests/context/test_current_context.py
from __future__ import annotations

import pytest

from toolr._context import current_context
from toolr._exc import NoCurrentContextError


def test_current_context_raises_when_unset() -> None:
    with pytest.raises(NoCurrentContextError, match=r"current_context\(\) has no context"):
        current_context()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/context/test_current_context.py -v`
Expected: FAIL — `ImportError: cannot import name 'current_context'`
(`NoCurrentContextError` doesn't exist yet either).

- [ ] **Step 3: Add `NoCurrentContextError` to `_exc.py`**

Add after the existing `SignatureError` class, before `ToolrDeprecationWarning`:

```python
class NoCurrentContextError(ToolrError):
    """Raised by `current_context()` when no `Context` is set for the running task/thread."""
```

- [ ] **Step 4: Add the `ContextVar` and `current_context()` to `_context.py`**

Add these imports near the top of `crates/toolr-py/python/toolr/_context.py`
(alongside the existing `import os` / `import pathlib` block):

```python
from contextvars import ContextVar
```

Add, right after the `from toolr.utils._console import ConsoleVerbosity` line
and before `class Context(Struct, frozen=True):`:

```python
from toolr._exc import NoCurrentContextError
```

Add at the **end of the file**, after the `Context` class's `which` method:

```python
_current_ctx: ContextVar[Context] = ContextVar("toolr_current_context")


def current_context() -> Context:
    """Return the `Context` of the toolr command currently executing.

    Raises `NoCurrentContextError` if called from code that isn't running
    inside a toolr command, or from a thread/task that wasn't given the
    context explicitly — see `specs/2026-09-21-context-local-helpers-design.md`
    for the exact cases this covers.
    """
    try:
        return _current_ctx.get()
    except LookupError:
        msg = (
            "current_context() has no context to return. This happens when:\n"
            "  - called from code that isn't running inside a toolr command (e.g.\n"
            "    interactively, from a script invoked outside toolr's dispatch, or\n"
            "    from CommandsTester's module-discovery import, which imports\n"
            "    tools.* modules without running a command)\n"
            "  - called from a thread or async task that wasn't given the context\n"
            "    explicitly — see toolr.current_context's docs\n"
            "\n"
            "If testing a helper that calls this, wrap the call in\n"
            "toolr.testing.set_current_context(ctx)."
        )
        raise NoCurrentContextError(msg) from None
```

- [ ] **Step 5: Run test to verify it passes**

Run: `uv run pytest tests/context/test_current_context.py -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-py/python/toolr/_exc.py \
        crates/toolr-py/python/toolr/_context.py \
        tests/context/test_current_context.py
git commit -m "feat(toolr-py): add current_context() accessor and NoCurrentContextError"
```

---

### Task 2: `toolr.testing.set_current_context`

**Suggested model:** Sonnet

**Files:**

- Create: `crates/toolr-py/python/toolr/testing/_current_context.py`
- Modify: `crates/toolr-py/python/toolr/testing/__init__.py`
- Test: `tests/testing/test_current_context.py` (new)

**Interfaces:**

- Consumes: `toolr._context.Context`, `toolr._context._current_ctx` (from
  Task 1), `toolr._context.current_context` (to assert against in tests),
  `toolr._exc.NoCurrentContextError` (to assert against in tests),
  `toolr.testing.make_context` (existing, to build test `Context` instances).
- Produces: `toolr.testing.set_current_context(ctx: Context) -> AbstractContextManager[Context]`.
- [ ] **Step 1: Write the failing tests**

```python
# tests/testing/test_current_context.py
from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

from toolr._context import current_context
from toolr._exc import NoCurrentContextError
from toolr.testing import make_context
from toolr.testing import set_current_context


def test_returns_ctx_during_block_and_raises_after(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    with set_current_context(ctx):
        assert current_context() is ctx
    with pytest.raises(NoCurrentContextError):
        current_context()


def test_nesting_restores_outer_value(tmp_path: Path) -> None:
    outer = make_context(tmp_path / "outer")
    inner = make_context(tmp_path / "inner")
    with set_current_context(outer):
        assert current_context() is outer
        with set_current_context(inner):
            assert current_context() is inner
        assert current_context() is outer


def test_reset_happens_even_if_block_raises(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    with pytest.raises(ValueError, match="boom"):
        with set_current_context(ctx):
            msg = "boom"
            raise ValueError(msg)
    with pytest.raises(NoCurrentContextError):
        current_context()


def test_concurrent_asyncio_tasks_see_own_context(tmp_path: Path) -> None:
    ctx_a = make_context(tmp_path / "a")
    ctx_b = make_context(tmp_path / "b")

    async def _read_own_context(ctx: object) -> object:
        with set_current_context(ctx):  # type: ignore[arg-type]
            await asyncio.sleep(0)  # yield control so the two tasks interleave
            return current_context()

    async def _main() -> tuple[object, object]:
        return await asyncio.gather(
            _read_own_context(ctx_a),
            _read_own_context(ctx_b),
        )

    result_a, result_b = asyncio.run(_main())
    assert result_a is ctx_a
    assert result_b is ctx_b


def test_command_and_its_helper_see_the_same_context(tmp_path: Path) -> None:
    """The testing-harness equivalent of Task 3's runner-path test: a command
    invoked directly (not via the subprocess runner) and a helper it calls
    via `current_context()` must resolve to the same `Context` instance."""

    def _helper() -> object:
        return current_context()

    def my_command(ctx: object) -> object:
        return _helper() is ctx

    ctx = make_context(tmp_path)
    with set_current_context(ctx):
        assert my_command(ctx) is True
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `uv run pytest tests/testing/test_current_context.py -v`
Expected: FAIL — `ImportError: cannot import name 'set_current_context'`

- [ ] **Step 3: Create `testing/_current_context.py`**

```python
"""Context manager for making `toolr.current_context()` resolve to a specific
`Context` in tests.
"""

from __future__ import annotations

from collections.abc import Iterator
from contextlib import contextmanager

from toolr._context import Context
from toolr._context import _current_ctx


@contextmanager
def set_current_context(ctx: Context) -> Iterator[Context]:
    """Make `ctx` the value `toolr.current_context()` returns for this block.

    Use when a command (or a helper it calls) reads `toolr.current_context()`
    instead of taking `ctx` as a parameter, and a test needs that call to
    resolve to a specific `Context` under test:

        ctx = make_context(repo_root)
        with toolr.testing.set_current_context(ctx):
            my_command(ctx, ...)
    """
    token = _current_ctx.set(ctx)
    try:
        yield ctx
    finally:
        _current_ctx.reset(token)
```

- [ ] **Step 4: Export it from `testing/__init__.py`**

Add the import alongside the other `toolr.testing._*` imports:

```python
from toolr.testing._current_context import set_current_context
```

Add `"set_current_context"` to the `__all__` list (keep it alphabetically
sorted alongside the existing entries).

- [ ] **Step 5: Run tests to verify they pass**

Run: `uv run pytest tests/testing/test_current_context.py -v`
Expected: PASS (all five tests)

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-py/python/toolr/testing/_current_context.py \
        crates/toolr-py/python/toolr/testing/__init__.py \
        tests/testing/test_current_context.py
git commit -m "feat(toolr-py): add toolr.testing.set_current_context"
```

---

### Task 3: Wire the contextvar into `_runner.py`

**Suggested model:** Opus — the exact placement of `.set()` relative to
`_import_target()` is the one line in this whole feature where getting it
wrong silently breaks the "works at import time in production" guarantee.
Get this line's position right and add the reasoning comment; don't rely on
"it looked right."

**Files:**

- Modify: `crates/toolr-py/python/toolr/_runner.py:554` (inside `run()`)
- Test: `tests/runner/test_current_context.py` (new)

**Interfaces:**

- Consumes: `toolr._context._current_ctx` (from Task 1).
- Produces: nothing new callable — this task's deliverable is behavior,
  verified by the integration test below.
- [ ] **Step 1: Write the failing test**

This spawns the real `python -m toolr._runner` subprocess (same pattern as
`tests/runner/test_dispatch.py`), with a `tools/demo.py` command that calls
`current_context()` and prints whether it's the same object the command
itself received as `ctx` — proving the var is set before the target function
runs, and holds the right value.

```python
# tests/runner/test_current_context.py
from __future__ import annotations

import os
import subprocess
import sys
import textwrap
from collections.abc import Callable
from pathlib import Path

import pytest


@pytest.fixture
def tools_module(tmp_path: Path) -> Callable[[str], Path]:
    """Factory: write a ``tools/demo.py`` with the given body. Returns the repo root."""

    def _make(body: str) -> Path:
        tools_dir = tmp_path / "tools"
        tools_dir.mkdir(parents=True, exist_ok=True)
        (tools_dir / "__init__.py").write_text("")
        (tools_dir / "demo.py").write_text(textwrap.dedent(body))
        return tmp_path

    return _make


@pytest.fixture
def spec_file(tmp_path: Path) -> Callable[..., Path]:
    """Factory: write a runner spec JSON to ``tmp_path/spec.json``. Returns its path."""
    import json

    def _make(
        command: str,
        function: str,
        args: dict[str, object] | None = None,
        repo_root: Path | None = None,
    ) -> Path:
        spec_path = tmp_path / "spec.json"
        spec_path.write_text(
            json.dumps(
                {
                    "module": "tools.demo",
                    "function": function,
                    "command": command,
                    "args": args or {},
                    "context": {
                        "repo_root": str(repo_root or tmp_path),
                        "verbosity": "normal",
                    },
                    "dispatch": None,
                    "enum_modules": {},
                }
            )
        )
        return spec_path

    return _make


@pytest.fixture
def run_runner(tmp_path: Path) -> Callable[[Path], subprocess.CompletedProcess[str]]:
    """Factory: spawn ``python -m toolr._runner`` with ``TOOLR_SPEC_FILE`` set."""

    def _run(spec_path: Path) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env["TOOLR_SPEC_FILE"] = str(spec_path)
        env["PYTHONPATH"] = str(tmp_path) + os.pathsep + env.get("PYTHONPATH", "")
        return subprocess.run(
            [sys.executable, "-m", "toolr._runner"],
            env=env,
            cwd=str(tmp_path),
            capture_output=True,
            text=True,
            check=False,
        )

    return _run


def test_current_context_is_set_before_target_runs(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group
        from toolr._context import current_context

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def hello(ctx, name: str = "world") -> None:
            same = current_context() is ctx
            ctx.print(f"hi {name} same={same}")
        """
    )
    spec_path = spec_file(command="hello", function="hello", args={"name": "Alice"})
    result = run_runner(spec_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "hi Alice same=True" in result.stdout


def test_current_context_works_at_module_import_time(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    """Regression guard for the design doc's "Correction": in production,
    the runner sets the contextvar *before* importing the target module, so
    a module-level call succeeds — it must never regress to failing here.
    """
    tools_module(
        """
        from toolr import command_group
        from toolr._context import current_context

        # Module-level call: this runs during `_import_target()`.
        _repo_root_at_import = current_context().repo_root

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def hello(ctx) -> None:
            ctx.print(f"import-time repo_root matched={_repo_root_at_import == ctx.repo_root}")
        """
    )
    spec_path = spec_file(command="hello", function="hello")
    result = run_runner(spec_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "import-time repo_root matched=True" in result.stdout
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `uv run pytest tests/runner/test_current_context.py -v`
Expected: both FAIL — `hi Alice same=True` not in stdout, and the
import-time test fails with a traceback showing `NoCurrentContextError`
raised during module import (since `.set()` isn't wired in yet).

- [ ] **Step 3: Wire `.set()` into `run()`**

In `crates/toolr-py/python/toolr/_runner.py`, add the import near the top
(alongside the other `toolr.*` imports):

```python
from toolr._context import _current_ctx
```

Then modify `run()` — insert one line right after `ctx = _build_context(spec)`
(currently line 554) and before `_append_repo_root(str(repo_root))`:

```python
        ctx = _build_context(spec)
        # `run()` calls the target function at most once per process — see
        # specs/2026-09-21-context-local-helpers-design.md's "Where the var
        # gets set" section. No `.reset()`: if this function ever grows an
        # in-process retry or multi-dispatch loop, that invariant breaks and
        # this needs revisiting.
        _current_ctx.set(ctx)
        _append_repo_root(str(repo_root))
```

Setting it here, before `_import_target(spec)`, means both call sites further
down (`invoke_dispatcher(ctx=ctx, ...)` and `target(ctx, *var_args, **kw_args)`)
already have it set, and — critically — so does the `_import_target(spec)`
call in between, which is what makes a module-level `current_context()` call
in a `tools/*.py` file succeed rather than raise.

- [ ] **Step 4: Run tests to verify they pass**

Run: `uv run pytest tests/runner/test_current_context.py -v`
Expected: PASS (both tests)

- [ ] **Step 5: Run the full runner test suite to check for regressions**

Run: `cargo test -p toolr --test '*' 2>&1 | tail -50` (Rust integration tests
also exercise `_runner.py` via `assert_cmd`) and
`uv run pytest tests/runner/ -v`
Expected: all PASS, no change to existing runner test outcomes.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-py/python/toolr/_runner.py \
        tests/runner/test_current_context.py
git commit -m "feat(toolr-py): set current_context() before dispatching a command"
```

---

### Task 4: Public API exports and skill-refs regen

**Suggested model:** Haiku

**Files:**

- Modify: `crates/toolr-py/python/toolr/__init__.py`
- Modify: (generated) `skills/toolr-command-authoring/references/*.md` — via
  `cargo xtask build-skill-refs`, don't hand-edit

**Interfaces:**

- Consumes: `toolr._context.current_context` (Task 1),
  `toolr.testing.set_current_context` (Task 2, already exported from
  `toolr.testing.__init__` — this task only touches top-level `toolr`).
- Produces: `toolr.current_context` as public API.
- [ ] **Step 1: Add the export**

In `crates/toolr-py/python/toolr/__init__.py`, add:

```python
from toolr._context import current_context
```

alongside the existing `from toolr._context import Context` line, and add
`"current_context"` to `__all__` (alphabetically — between `"command_group"`
and `"report_on_import_errors"`).

- [ ] **Step 2: Write a smoke test for the public path**

```python
# tests/context/test_current_context.py — append to the file from Task 1
import toolr


def test_current_context_is_exported_from_top_level_package() -> None:
    assert toolr.current_context is current_context
```

(This needs `import toolr` added at the top of the existing test file
alongside the other imports.)

- [ ] **Step 3: Run the test**

Run: `uv run pytest tests/context/test_current_context.py -v`
Expected: PASS

- [ ] **Step 4: Regenerate skill refs**

Run: `cargo xtask build-skill-refs`

This regenerates the reference tables that list `toolr.__all__`. Check
`git diff` afterward — it should show exactly one new `current_context` row
added to a generated reference table, no unrelated changes.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-py/python/toolr/__init__.py \
        tests/context/test_current_context.py \
        skills/toolr-command-authoring/references/
git commit -m "feat(toolr-py): export toolr.current_context from the top-level package"
```

---

### Task 5: Document the pattern in `toolr-command-authoring`

**Suggested model:** Sonnet — needs to match the skill's existing tone and
correctly carry over the thread/asyncio limitation without softening it.

**Files:**

- Modify: `skills/toolr-command-authoring/SKILL.md` (or the file within that
  skill directory that documents helper-function conventions — read the
  skill first to find the right section; don't create a new file for this)

**Interfaces:**

- Consumes: nothing code-level — this is a docs-only task.

- [ ] **Step 1: Read the skill to find where helper-function conventions live**

Run: `find skills/toolr-command-authoring -type f -name "*.md"` and read
whichever file documents how helpers take `ctx` today (likely `SKILL.md`
itself, given the skill's description mentions `ctx.run`).

- [ ] **Step 2: Add a section on the two ways a helper can get its `Context`**

Add prose (exact wording adapted to match the surrounding file's voice, but
must cover all of the following points — do not omit any):

1. `@command`-decorated functions always take `ctx` as their required first
   argument. Unchanged.
2. A helper function *not* on that boundary has two options: take `ctx` as a
   parameter (caller passes it explicitly — the existing, still-supported
   pattern), or call `toolr.current_context()` itself. Both are valid; this
   is a per-helper choice, not a migration.
3. `toolr.current_context()` raises `NoCurrentContextError` if nothing is
   running a toolr command. It does **not** propagate into a manually
   started `threading.Thread` or a `ThreadPoolExecutor` worker — a helper
   that needs `ctx` inside one of those must be passed it explicitly. It
   *does* propagate into `asyncio.create_task` children.
4. Testing a helper that uses `current_context()`: wrap the call in
   `toolr.testing.set_current_context(ctx)`.

- [ ] **Step 3: Verify the doc still builds**

Run: `mkdocs build --strict` (per this repo's doc-only-PR verification
scale — this is a docs change, no Rust/Python touched).

- [ ] **Step 4: Commit**

```bash
git add skills/toolr-command-authoring/
git commit -m "docs(toolr-command-authoring): document current_context() as a helper option"
```

---

### Task 6: `UNRELEASED.md` entry

**Suggested model:** Haiku

**Files:**

- Modify: `UNRELEASED.md`

- [ ] **Step 1: Add the entry**

Append (keeping the file's existing "no scaffolding, just write the note"
convention):

```markdown
### `toolr.current_context()`

Added `toolr.current_context()`, a `contextvars`-backed accessor a
command-authoring helper can call to get the `Context` of the toolr command
currently executing, without it being passed as a parameter. Purely
additive: `@command`-decorated functions keep receiving `ctx` as their
required first argument, and existing helpers that take `ctx` explicitly are
unaffected. See `toolr.testing.set_current_context()` for testing helpers
that use it.
```

Wording note: don't state a version number or "minor"/"major" bump here —
that's decided at release time from this file's full content plus the
commit log (see this repo's version-bump process), not authored in advance.

- [ ] **Step 2: Commit**

```bash
git add UNRELEASED.md
git commit -m "docs(changelog): queue release note for current_context()"
```

---

### Task 7: Full verification

**Suggested model:** Sonnet — mostly running and triaging, escalate only if
a failure needs real debugging.

**Files:** none (verification only)

- [ ] **Step 1: Run the full umbrella test suite**

Run: `mise run test`

This covers the skill-refs drift gate (already run manually in Task 4, this
re-confirms it), `cargo test --workspace`, and `uv run pytest`. Per this
repo's guidance, poll output every 30–60s rather than fire-and-forget —
`cargo test --workspace` can stall.

- [ ] **Step 2: Run pre-commit on everything touched**

Run: `prek run --all-files`

- [ ] **Step 3: If anything fails, fix and re-run from Step 1**

Do not proceed past a red step. If a failure is in code outside this
feature's files, stop and report it rather than fixing it as a drive-by
(per this repo's scope-discipline convention) — it's likely pre-existing.

- [ ] **Step 4: Final review of the diff**

Run: `git log --oneline main..HEAD` and `git diff main...HEAD --stat` to
confirm the change set matches this plan's file list exactly — no
unintended files touched.
