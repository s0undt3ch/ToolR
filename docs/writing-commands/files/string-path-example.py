"""String-path command attachment example."""

from __future__ import annotations

from toolr import Context
from toolr import command
from toolr import command_group

# Declare the group at module scope. No assignment needed — toolr
# picks the bare expression-statement up via the static parser.
command_group("greeting", title="Greetings", description="Hello-world commands")


@command(group="greeting")
def hello(ctx: Context, name: str = "world") -> None:
    """Greet someone.

    Args:
        name: Who to greet.
    """
    ctx.print(f"hello, {name}")


@command("shout-hello", group="greeting")
def shout(ctx: Context, name: str = "world") -> None:
    """Yell a greeting (CLI name overrides function name).

    Args:
        name: Who to greet.
    """
    ctx.print(f"HELLO, {name.upper()}!")
