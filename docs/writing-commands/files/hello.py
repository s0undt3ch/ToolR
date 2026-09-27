from __future__ import annotations

from toolr import Context
from toolr import command_group

greeting = command_group("greeting", "Greeting Commands", "Commands for greeting users")


@greeting.command
def hello(ctx: Context, name: str = "World"):
    """Say hello.

    Args:
        name: The name of the person to greet.
    """
    ctx.info("Hello", name, "!")
