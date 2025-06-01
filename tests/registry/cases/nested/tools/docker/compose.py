"""Docker compose commands."""

from __future__ import annotations

from . import docker_group

# Create a nested compose command group
compose_group = docker_group.command_group("compose", "Compose Commands", "Docker Compose orchestration tools")


@compose_group.command("up", help="Start services")
def compose_up(args):
    """Start services."""
    return "docker compose up executed"


@compose_group.command("down", help="Stop services")
def compose_down(args):
    """Stop services."""
    return "docker compose down executed"


@compose_group.command("logs", help="View service logs")
def compose_logs(args):
    """View service logs."""
    return "docker compose logs executed"
