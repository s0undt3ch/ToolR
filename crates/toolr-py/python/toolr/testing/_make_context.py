"""Build a real, usable :class:`toolr.Context` outside of the CLI dispatch path.

Lets a test call an ``@command``-decorated function directly and assert on
what it did with ``ctx.run``/``ctx.info``/``ctx.exit``/``ctx.print``, instead
of only being able to test the pure-logic functions those commands wrap.
"""

from __future__ import annotations

import io
from argparse import ArgumentParser
from typing import TYPE_CHECKING

import msgspec
from rich.console import Console

from toolr._context import Context
from toolr.utils._console import TOOLR_THEME
from toolr.utils._console import ConsoleVerbosity

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path
    from typing import TextIO

    from toolr.utils.command import CommandResult


class _EOFOnExhaustionStringIO(io.StringIO):
    """Raises `EOFError` on `readline()` once exhausted, instead of returning `""` forever.

    A plain `io.StringIO` returns `""` past its end, and `rich`'s prompt retry
    loop treats that as "keep asking" rather than "no more input" unless a
    `default` was set — an under-provisioned `prompt_input` in a test would
    otherwise hang the test suite instead of failing it.
    """

    def readline(self, size: int = -1) -> str:  # type: ignore[override]
        line = super().readline(size)
        if line == "":
            msg = "prompt_input exhausted — no more canned answers to read"
            raise EOFError(msg)
        return line


class ContextForTesting(Context, frozen=True):
    """A `Context` returned by `make_context`, with captured-output accessors.

    Every `Context` method works unmodified (`run`, `print`, `chdir`, `prompt`, `exit`, …) —
    this only adds `stdout`/`stderr` properties reading back what was written through the
    captured consoles `make_context` wires up.
    """

    @property
    def stdout(self) -> str:
        """Everything written to `print`/`info` so far."""
        return self._console_stdout.file.getvalue()  # type: ignore[attr-defined]

    @property
    def stderr(self) -> str:
        """Everything written to `error`/`warn`/`debug` so far."""
        return self._console_stderr.file.getvalue()  # type: ignore[attr-defined]

    def replace(self, **changes: object) -> ContextForTesting:
        """Build a copy with the given fields overridden — e.g. `ctx.replace(_run_impl=...)`.

        A thin wrapper around `msgspec.structs.replace`, scoped to this
        testing-only subclass rather than added to the real `Context`.
        """
        return msgspec.structs.replace(self, **changes)


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
    """Build a `Context` usable in a unit test.

    Args:
        repo_root: Value for `ctx.repo_root`.
        verbosity: Value for `ctx.verbosity`.
        default_timeout_secs: Value for `ctx.default_timeout_secs`.
        default_no_output_timeout_secs: Value for `ctx.default_no_output_timeout_secs`.
        run: Override for `ctx.run`'s underlying implementation. Omit to keep the real
            subprocess runner.
        chdir: Override for `ctx.chdir`'s underlying implementation. Omit to keep the real
            `os.chdir`.
        prompt_input: Canned answer(s) fed to `ctx.prompt`. A `str` is wrapped in a stream
            that raises `EOFError` once exhausted (instead of hanging). Pass a `TextIO`
            directly for finer control. Omit to keep `ctx.prompt`'s real stdin-reading
            behavior. **Does not cover `ctx.prompt(..., password=True)`** — `getpass.getpass`
            only uses its `stream` argument to *write* the prompt text, never to *read* the
            answer (it always reads from `/dev/tty` or real `stdin`), so a password prompt
            ignores this parameter entirely. Test password prompts by patching
            `getpass.getpass` directly instead — see `tests/context/test_prompt.py::test_prompt_password`
            for the established pattern.

    Returns:
        A `ContextForTesting` — a `Context` subclass with `stdout`/`stderr`
        properties reading back what was written through its captured consoles.
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
        **context_kwargs,  # type: ignore[arg-type]
    )
