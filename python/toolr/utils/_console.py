from __future__ import annotations

import os
from typing import TYPE_CHECKING
from typing import Any

import rich
from rich.console import Console
from rich.theme import Theme

from toolr.utils._logs import include_timestamps

if TYPE_CHECKING:
    from toolr._context import ConsoleVerbosity


def setup_consoles(verbosity: ConsoleVerbosity) -> tuple[Console, Console]:
    # Late import to avoid circular import issues
    from toolr._context import ConsoleVerbosity  # noqa: PLC0415

    # Let's setup the consoles
    theme = Theme(
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
    console_kwargs: dict[str, Any] = {
        "theme": theme,
    }
    if os.environ.get("CI"):
        console_kwargs["force_terminal"] = True
        console_kwargs["force_interactive"] = False
    log_path = verbosity >= ConsoleVerbosity.VERBOSE
    log_time = include_timestamps()
    stderr = Console(stderr=True, log_path=log_path, log_time=log_time, **console_kwargs)
    stdout = Console(log_path=log_path, log_time=log_time, **console_kwargs)
    rich.reconfigure(stderr=True, **console_kwargs)
    return stderr, stdout
