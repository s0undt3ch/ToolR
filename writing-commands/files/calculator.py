from __future__ import annotations

from toolr import Context
from toolr import command
from toolr import command_group

command_group("math", "Math Commands", "Basic mathematical operations")


@command(group="math")
def add(ctx: Context, a: int, b: int):
    """Add two numbers together.

    Args:
        a: First number.
        b: Second number.
    """
    result = a + b
    ctx.info(f"{a} + {b} = {result}")
    return result
