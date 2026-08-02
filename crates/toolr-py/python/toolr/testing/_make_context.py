"""Build a real, usable :class:`toolr.Context` outside of the CLI dispatch path.

Lets a test call an ``@command``-decorated function directly and assert on
what it did with ``ctx.run``/``ctx.info``/``ctx.exit``/``ctx.print``, instead
of only being able to test the pure-logic functions those commands wrap.
"""

from __future__ import annotations

import io
from argparse import ArgumentParser
from typing import TYPE_CHECKING

from attrs import define
from attrs import field
from rich.console import Console

from toolr._context import Context
from toolr.utils._console import TOOLR_THEME
from toolr.utils._console import ConsoleVerbosity

if TYPE_CHECKING:
    from pathlib import Path


@define(slots=True, frozen=True)
class CapturedOutput:
    """The two in-memory buffers a `make_context`-built `Context` writes to."""

    _stdout: io.StringIO
    _stderr: io.StringIO

    @property
    def stdout(self) -> str:
        """Everything written to `ctx.print`/`ctx.info` so far."""
        return self._stdout.getvalue()

    @property
    def stderr(self) -> str:
        """Everything written to `ctx.error`/`ctx.warn`/`ctx.debug` so far."""
        return self._stderr.getvalue()


@define(slots=True, frozen=True)
class ContextForTesting:
    """A `Context` plus the buffers its consoles write to.

    `ctx.exit(...)` still raises `SystemExit` (via the real `ArgumentParser`
    it's built with) — assert on that with `pytest.raises(SystemExit)`.
    `ctx.run(...)` runs the real subprocess runner; monkeypatch it yourself
    if a test needs to intercept it.
    """

    ctx: Context
    output: CapturedOutput = field()


def make_context(
    repo_root: Path,
    *,
    verbosity: ConsoleVerbosity = ConsoleVerbosity.NORMAL,
    default_timeout_secs: float | None = None,
    default_no_output_timeout_secs: float | None = None,
) -> ContextForTesting:
    """Build a `Context` usable in a unit test.

    Args:
        repo_root: Value for `ctx.repo_root`.
        verbosity: Value for `ctx.verbosity`.
        default_timeout_secs: Value for `ctx.default_timeout_secs`.
        default_no_output_timeout_secs: Value for `ctx.default_no_output_timeout_secs`.

    Returns:
        A `ContextForTesting` bundling the `Context` and its captured
        stdout/stderr text.
    """
    stdout_buffer = io.StringIO()
    stderr_buffer = io.StringIO()
    console_stdout = Console(
        file=stdout_buffer, stderr=False, force_terminal=False, theme=TOOLR_THEME
    )
    console_stderr = Console(
        file=stderr_buffer, stderr=True, force_terminal=False, theme=TOOLR_THEME
    )
    parser = ArgumentParser(prog="toolr-test", add_help=False)
    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
        default_timeout_secs=default_timeout_secs,
        default_no_output_timeout_secs=default_no_output_timeout_secs,
    )
    return ContextForTesting(ctx=ctx, output=CapturedOutput(stdout_buffer, stderr_buffer))
