"""Simple docker commands for testing."""

from __future__ import annotations

from toolr import Context
from toolr import command_group

# Create a simple command group
docker_group = command_group("docker", "Docker Commands", "Docker-related tools")


@docker_group.command("build")
def docker_build(ctx: Context) -> None:
    """Build a Docker image."""
    ctx.print("docker build executed")


@docker_group.command("run")
def docker_run(ctx: Context) -> None:
    """Run a Docker container."""
    ctx.print("docker run executed")
