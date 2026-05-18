"""Nested command groups example for the writing-commands chapter."""

from __future__ import annotations

from toolr import Context
from toolr import command
from toolr import command_group

command_group("docker", title="Docker", description="Container utilities")
command_group("docker.image", description="Image subcommands")
command_group("docker.container", description="Container subcommands")


@command(group="docker.image")
def build(ctx: Context, tag: str) -> None:
    """Build a docker image.

    Args:
        tag: Tag to assign to the built image.
    """
    ctx.print(f"would build image: {tag}")


@command(group="docker.container")
def start(ctx: Context, name: str) -> None:
    """Start a stopped container.

    Args:
        name: Name (or ID) of the container.
    """
    ctx.print(f"would start container: {name}")
