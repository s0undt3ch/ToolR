"""
This module provides the Context class, which is passed to every command group function as the first argument.
"""

from __future__ import annotations

import os
import pathlib
from argparse import ArgumentParser
from collections.abc import Iterator
from contextlib import contextmanager
from typing import TYPE_CHECKING
from typing import Any
from typing import NoReturn
from typing import TextIO

from msgspec import Struct
from rich.prompt import Confirm
from rich.prompt import FloatPrompt
from rich.prompt import IntPrompt
from rich.prompt import Prompt

from toolr.utils import command

if TYPE_CHECKING:
    from rich.console import Console
    from rich.text import TextType

    from toolr.utils.command import CommandResult

from toolr.utils._console import ConsoleVerbosity


class Context(Struct, frozen=True):
    """Context object passed to every command group function as the first argument."""

    repo_root: pathlib.Path
    parser: ArgumentParser
    verbosity: ConsoleVerbosity
    _console_stderr: Console
    _console_stdout: Console

    def prompt(
        self,
        prompt: TextType,
        expected_type: type[str | int | float | bool] | None = None,
        *,
        password: bool = False,
        case_sensitive: bool = True,
        choices: list[str] | None = None,
        default: str | int | float | bool | None = None,
        show_default: bool = True,
        show_choices: bool = True,
    ) -> str | int | float | bool:
        """
        Prompt the user for input.

        This is a wrapper around [rich.prompt.Prompt.ask][rich.prompt].

        See [rich.prompt.Prompt.ask][rich.prompt] for more details.
        """
        return self._prompt(
            prompt,
            expected_type,
            password=password,
            case_sensitive=case_sensitive,
            choices=choices,
            default=default,
            show_default=show_default,
            show_choices=show_choices,
        )

    def _prompt(
        self,
        prompt: TextType,
        expected_type: type[str | int | float | bool] | None = None,
        *,
        password: bool = False,
        case_sensitive: bool = True,
        choices: list[str] | None = None,
        default: str | int | float | bool | None = None,
        show_default: bool = True,
        show_choices: bool = True,
        console: Console | None = None,
        stream: TextIO | None = None,
    ) -> str | int | float | bool:
        """
        This is the actual implementation of the prompt method with two additional arguments to simplify testing.
        """
        klass: type[Prompt | IntPrompt | FloatPrompt | Confirm]
        if expected_type in (str, None):
            klass = Prompt
        elif expected_type is int:
            klass = IntPrompt
        elif expected_type is float:
            klass = FloatPrompt
        elif expected_type is bool:
            klass = Confirm
        else:
            err_msg = f"Unsupported expected_type: {expected_type}"
            raise ValueError(err_msg)

        if choices is not None and not choices:
            err_msg = "choices cannot be an empty list"
            raise ValueError(err_msg)

        return klass.ask(
            prompt,
            console=console or self._console_stdout,
            password=password,
            choices=choices,
            default=default,  # type: ignore[arg-type]
            case_sensitive=case_sensitive,
            show_default=show_default,
            show_choices=show_choices,
            stream=stream,
        )

    def print(self, *args: Any, **kwargs: Any) -> None:
        """
        Print to stdout.

        This is a wrapper around :func:`rich.console.Console.print`.

        See :func:`rich.console.Console.print` for more details.
        """
        self._console_stdout.print(*args, **kwargs)

    def debug(self, *args: Any, **kwargs: Any) -> None:
        """
        Print debug message to stderr.

        This is a wrapper around [rich.console.Console.log][rich.console.Console.log].

        See [rich.console.Console.log][rich.console.Console.log] for more details.
        """
        if self.verbosity >= ConsoleVerbosity.VERBOSE:
            kwargs.update(style="log-debug", _stack_offset=2)
            self._console_stderr.log(*args, **kwargs)

    def info(self, *args: Any, **kwargs: Any) -> None:
        """
        Print info message to stderr.

        This is a wrapper around [rich.console.Console.log][rich.console.Console.log].

        See [rich.console.Console.log][rich.console.Console.log] for more details.
        """
        if self.verbosity >= ConsoleVerbosity.NORMAL:
            kwargs.update(style="log-info", _stack_offset=2)
            self._console_stderr.log(*args, **kwargs)

    def warn(self, *args: Any, **kwargs: Any) -> None:
        """
        Print warning message to stderr.

        This is a wrapper around [rich.console.Console.log][rich.console.Console.log].

        See [rich.console.Console.log][rich.console.Console.log] for more details.
        """
        kwargs.update(style="log-warning", _stack_offset=2)
        self._console_stderr.log(*args, **kwargs)

    def error(self, *args: Any, **kwargs: Any) -> None:
        """
        Print error message to stderr.

        This is a wrapper around [rich.console.Console.log][rich.console.Console.log].

        See [rich.console.Console.log][rich.console.Console.log] for more details.
        """
        kwargs.update(style="log-error", _stack_offset=2)
        self._console_stderr.log(*args, **kwargs)

    def exit(self, status: int = 0, message: str | None = None) -> NoReturn:
        """
        Exit the command execution.
        """
        if message is not None:
            if status == 0:
                style = "exit-ok"
            else:
                style = "exit-failure"
            self._console_stderr.print(message, style=style)
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

        This is a wrapper around [toolr.utils.command.run][] that provides
        a simpler interface for command functions.

        Args:
            cmdline: Command line to run
            stream_output: Whether to stream output to stdout/stderr
            capture_output: Whether to capture output to return
            timeout_secs: Maximum time to wait for command completion
            no_output_timeout_secs: Maximum time to wait without output
            kwargs: Additional keyword arguments to pass to [toolr.utils.command.run][]

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
