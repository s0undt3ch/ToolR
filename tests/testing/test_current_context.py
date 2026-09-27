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
    with pytest.raises(ValueError, match="boom"):  # noqa: PT012 — reset must run inside the `with` block
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
