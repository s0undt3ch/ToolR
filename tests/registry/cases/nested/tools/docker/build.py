"""Docker build commands."""

from __future__ import annotations

from typing import TYPE_CHECKING
from typing import Annotated

from toolr import arg

from . import docker_group

if TYPE_CHECKING:
    from toolr import Context

# Create a nested build command group
build_group = docker_group.command_group("build", "Build Commands", "Docker image build tools")


@build_group.command("image")
def build_image(
    ctx: Context,
    *,
    verbose: Annotated[bool, arg(group="verbosity")] = False,
    quiet: Annotated[bool, arg(group="verbosity")] = False,
) -> None:
    """Build a Docker image.

    Args:
        verbose: Enable verbose output.
        quiet: Suppress all output.
    """
    if verbose:
        ctx.info("Verbose mode enabled")
    elif quiet:
        ctx.info("Quiet mode enabled")
    else:
        ctx.info("Normal mode")
    ctx.info("docker build image executed")


@build_group.command("context")
def build_context(ctx: Context) -> None:
    """Build with context."""
    ctx.print("docker build context executed")


@build_group.command("multi-stage")
def build_multi_stage(ctx: Context) -> None:
    """Build multi-stage image."""
    ctx.print("docker build multi-stage executed")
