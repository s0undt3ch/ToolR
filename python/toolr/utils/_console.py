from __future__ import annotations

from enum import IntEnum
from typing import Any

import rich
from msgspec import Struct
from rich.console import Console
from rich.theme import Theme


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


class Consoles(Struct, frozen=True):
    stderr: Console
    stdout: Console

    @classmethod
    def setup(cls, verbosity: ConsoleVerbosity) -> Consoles:
        return cls._setup_consoles(verbosity)

    @classmethod
    def setup_no_colors(cls, verbosity: ConsoleVerbosity) -> Consoles:
        return cls._setup_consoles(
            verbosity,
            color_system=None,
            no_color=True,
            force_terminal=True,
            force_interactive=False,
        )

    @classmethod
    def _setup_consoles(cls, verbosity: ConsoleVerbosity, **console_kwargs: Any) -> Consoles:
        # Late import to avoid circular import issues
        from toolr.utils._logs import include_timestamps  # noqa: PLC0415

        console_kwargs["theme"] = Theme(
            {
                "log-debug": "dim blue",
                "log-info": "dim cyan",
                "log-warning": "magenta",
                "log-error": "bold red",
                "exit-ok": "green",
                "exit-failure": "bold red",
                "logging.level.stdout": "dim blue",
                "logging.level.stderr": "dim red",
            }
        )

        log_path = verbosity >= ConsoleVerbosity.VERBOSE
        log_time = include_timestamps()
        stderr = Console(stderr=True, log_path=log_path, log_time=log_time, **console_kwargs)
        stdout = Console(stderr=False, log_path=log_path, log_time=log_time, **console_kwargs)
        rich.reconfigure(stderr=True, **console_kwargs)
        return cls(stdout=stdout, stderr=stderr)
