"""Dotted command_group + cross-file nesting example."""

from __future__ import annotations

from toolr import Context
from toolr import command
from toolr import command_group

# Parent group; declared once.
command_group("docker", title="Docker", description="Container utilities")

# Child group declared via dotted path. No reference to the parent
# binding — toolr resolves `docker` from the static manifest.
command_group("docker.image", description="Image subcommands")


@command(group="docker.image")
def build(ctx: Context, tag: str) -> None:
    """Build a docker image.

    Args:
        tag: Tag to assign to the built image.
    """
    ctx.print(f"would build image: {tag}")
