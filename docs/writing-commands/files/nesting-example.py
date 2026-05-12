"""Nested command groups example for the writing-commands chapter."""

from __future__ import annotations

from toolr import Context
from toolr import command_group

docker = command_group("docker", title="Docker", description="Container utilities")
docker_image = docker.command_group("image", description="Image subcommands")
docker_container = docker.command_group("container", description="Container subcommands")


@docker_image.command
def build(ctx: Context, tag: str) -> None:
    """Build a docker image.

    Args:
        tag: Tag to assign to the built image.
    """
    ctx.print(f"would build image: {tag}")


@docker_container.command
def start(ctx: Context, name: str) -> None:
    """Start a stopped container.

    Args:
        name: Name (or ID) of the container.
    """
    ctx.print(f"would start container: {name}")
