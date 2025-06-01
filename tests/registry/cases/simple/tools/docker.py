"""Simple docker commands for testing."""

from __future__ import annotations

from toolr import registry

# Create a simple command group
docker_group = registry.command_group("docker", "Docker Commands", "Docker-related tools")


@docker_group.command("build", help="Build a Docker image")
def docker_build(args):
    """Build a Docker image."""
    return "docker build executed"


@docker_group.command("run", help="Run a Docker container")
def docker_run(args):
    """Run a Docker container."""
    return "docker run executed"
