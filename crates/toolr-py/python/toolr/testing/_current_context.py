"""Context manager for tests.

Makes `toolr.current_context()` resolve to a specific `Context` for a block.
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
