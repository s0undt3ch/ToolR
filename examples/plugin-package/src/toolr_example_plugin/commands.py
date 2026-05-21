"""Command groups exposed by the example plugin.

The decorators here populate the toolr command registry at import time.
For static discovery (the canonical path), the same groups are also
serialised into ``toolr-manifest.json`` shipped alongside this module.
"""

from __future__ import annotations

from toolr import Context
from toolr import command_group

third_party_group = command_group(
    "third-party",
    "Third Party Tools",
    "Tools contributed by a third-party plugin.",
)


@third_party_group.command("hello")
def hello_command(ctx: Context, name: str = "World") -> None:
    """Say hello to someone.

    Args:
        ctx: The execution context.
        name: Name to greet (default: World).
    """
    ctx.print(f"Hello, {name} from toolr-plugin-example!")


@third_party_group.command("version")
def version_command(ctx: Context) -> None:
    """Show the version of the example plugin.

    Args:
        ctx: The execution context.
    """
    ctx.print("toolr-plugin-example version 1.0.0")


utils_group = command_group(
    "utils",
    "Utility Commands",
    "General utility commands shipped by the example plugin.",
)


@utils_group.command("echo")
def echo_command(ctx: Context, message: str, repeat: int = 1) -> None:
    """Echo a message multiple times.

    Args:
        ctx: The execution context.
        message: Message to echo.
        repeat: Number of times to repeat the message (default: 1).
    """
    for i in range(repeat):
        ctx.print(f"[{i + 1}] {message}")


@utils_group.command("info")
def info_command(ctx: Context) -> None:
    """Show information about the example plugin.

    Args:
        ctx: The execution context.
    """
    ctx.print("toolr-plugin-example information:")
    ctx.print("- Name: toolr-plugin-example")
    ctx.print("- Version: 1.0.0")
    ctx.print("- Description: Canonical example of a third-party toolr plugin")
