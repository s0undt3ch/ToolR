from __future__ import annotations

from toolr import Context
from toolr import command_group

group = command_group("example", title="Example", description="Example commands")


@group.command
def echo(ctx: Context, what: str):
    """
    Command title line.

    This is the command description, it can span several lines.

    Args:
        what: What to echo.
    """
    ctx.print(what)
