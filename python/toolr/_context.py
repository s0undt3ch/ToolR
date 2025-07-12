"""
This module provides the Context class, which is passed to every command group function as the first argument.
"""

from __future__ import annotations

import os
import pathlib
from argparse import ArgumentParser
from collections.abc import Iterator
from contextlib import contextmanager
from enum import IntEnum
from typing import TYPE_CHECKING
from typing import Any
from typing import NoReturn

from msgspec import Struct

from toolr.utils import command

if TYPE_CHECKING:
    from rich.console import Console

    from toolr.utils.command import CommandResult


class ConsoleVerbosity(IntEnum):
    """Console verbosity levels."""

    QUIET = 0
    NORMAL = 1
    VERBOSE = 2

    def __repr__(self) -> str:
        """
        Return a string representation of the console verbosity.
        """
        return self.name.lower()


class Context(Struct, frozen=True):
    """Context object passed to every command group function as the first argument."""

    repo_root: pathlib.Path
    parser: ArgumentParser
    verbosity: ConsoleVerbosity
    console: Console
    console_stdout: Console

    def print(self, *args, **kwargs) -> None:
        """
        Print to stdout.
        """
        self.console_stdout.print(*args, **kwargs)

    def debug(self, *args) -> None:
        """
        Print debug message to stderr.
        """
        if self.verbosity >= ConsoleVerbosity.VERBOSE:
            self.console.log(*args, style="log-debug", _stack_offset=2)

    def info(self, *args) -> None:
        """
        Print info message to stderr.
        """
        if self.verbosity >= ConsoleVerbosity.NORMAL:
            self.console.log(*args, style="log-info", _stack_offset=2)

    def warn(self, *args) -> None:
        """
        Print warning message to stderr.
        """
        self.console.log(*args, style="log-warning", _stack_offset=2)

    def error(self, *args) -> None:
        """
        Print error message to stderr.
        """
        self.console.log(*args, style="log-error", _stack_offset=2)

    def exit(self, status: int = 0, message: str | None = None) -> NoReturn:  # type: ignore[misc]
        """
        Exit the command execution.
        """
        if message is not None:
            if status == 0:
                style = "exit-ok"
            else:
                style = "exit-failure"
            self.console.print(message, style=style)
        self.parser.exit(status)

    def run(
        self,
        *cmdline: str,
        stream_output: bool = True,
        capture_output: bool = False,
        timeout_secs: float | None = None,
        no_output_timeout_secs: float | None = None,
        **kwargs: Any,
    ) -> CommandResult[str] | CommandResult[bytes]:
        """Run a command with the given arguments.

        This is a wrapper around :func:`toolr.utils.command.run_command` that provides
        a simpler interface for command functions.

        Args:
            cmdline: Command line to run
            stream_output: Whether to stream output to stdout/stderr
            capture_output: Whether to capture output to return
            timeout_secs: Maximum time to wait for command completion
            no_output_timeout_secs: Maximum time to wait without output
            kwargs: Additional keyword arguments to pass to :func:`toolr.utils.command.run`

        Returns:
            CommandResult instance.
        """
        self.debug(f"""Running '{" ".join(cmdline)}'""")
        return command.run(
            cmdline,
            stream_output=stream_output,
            capture_output=capture_output,
            timeout_secs=timeout_secs,
            no_output_timeout_secs=no_output_timeout_secs,
            **kwargs,
        )

    @contextmanager
    def chdir(self, path: str | pathlib.Path) -> Iterator[pathlib.Path]:
        """Change the working directory for this context.

        Args:
            path: The new working directory path

        Returns:
            Iterator yielding the new working directory as a Path object

        This is a context manager, so it should be used with 'with':

        .. code-block:: python

            with ctx.chdir("/some/path") as p:
                # Do something in /some/path
                # p is the Path object for /some/path
        """
        cwd = pathlib.Path.cwd()
        if isinstance(path, str):
            path = pathlib.Path(path)
        try:
            os.chdir(path)
            yield path
        finally:
            if not cwd.exists():
                self.error(f"Unable to change back to path {cwd}")
            else:
                os.chdir(cwd)
