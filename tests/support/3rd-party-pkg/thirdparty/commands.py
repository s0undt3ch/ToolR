from __future__ import annotations

from toolr import Context
from toolr import command_group

third_party_group = command_group("third-party", "Third Party Tools", "Tools from third-party packages")


@third_party_group.command("hello")
def hello_command(ctx: Context, name: str = "World") -> None:
    """Say hello to someone.

    Args:
        ctx: The execution context
        name: Name to greet (default: World)
    """
    ctx.print(f"Hello, {name} from 3rd-party package!")


@third_party_group.command("version")
def version_command(ctx: Context) -> None:
    """Show the version of the 3rd-party package.

    Args:
        ctx: The execution context
    """
    ctx.print("3rd-party package version 1.0.0")


utils_group = command_group("utils", "Utility Commands", "General utility commands")


@utils_group.command("echo")
def echo_command(ctx: Context, message: str, repeat: int = 1) -> None:
    """Echo a message multiple times.

    Args:
        ctx: The execution context
        message: Message to echo
        repeat: Number of times to repeat the message (default: 1)
    """
    for i in range(repeat):
        ctx.print(f"[{i + 1}] {message}")


@utils_group.command("info")
def info_command(ctx: Context) -> None:
    """Show information about the 3rd-party package.

    Args:
        ctx: The execution context
    """
    ctx.print("3rd-party package information:")
    ctx.print("- Name: 3rd-party-pkg")
    ctx.print("- Version: 1.0.0")
    ctx.print("- Description: Test package for entry point discovery")
    ctx.print("- Author: Test Author")
