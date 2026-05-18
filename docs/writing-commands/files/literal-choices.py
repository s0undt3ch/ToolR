from __future__ import annotations

from typing import Literal

from toolr import Context
from toolr import command
from toolr import command_group

command_group("logs", title="Logs", description="Logging utilities")


@command(group="logs")
def set_level(
    ctx: Context,
    level: Literal["debug", "info", "warning", "error"] = "info",
) -> None:
    """Set the active log level.

    Args:
        level: Which level to use.
    """
    ctx.print(f"log level is now: {level}")
