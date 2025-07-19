"""Simple docker commands for testing."""

from __future__ import annotations

from toolr import Context
from toolr import registry

# Create a simple command group
docker_group = registry.command_group("docker", "Docker Commands", "Docker-related tools")


@docker_group.command("build", help="Build a Docker image")
def docker_build(ctx: Context) -> None:
    """Build a Docker image."""
    ctx.print("docker build executed")


@docker_group.command("run", help="Run a Docker container")
def docker_run(ctx: Context) -> None:
    """Run a Docker container."""
    ctx.print("docker run executed")
