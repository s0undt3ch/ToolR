from __future__ import annotations

from toolr import Context
from toolr import command_group

group = command_group("greeting", "Greeting Commands", "Commands for greeting users")


@group.command
def hello(ctx: Context, name: str = "World"):
    """Say hello.

    Args:
        name: The name of the person to greet.
    """
    ctx.info("Hello", name, "!")
