"""Docker compose commands."""

from __future__ import annotations

from typing import TYPE_CHECKING

from . import docker_group

if TYPE_CHECKING:
    from toolr import Context

# Create a nested compose command group
compose_group = docker_group.command_group("compose", "Compose Commands", "Docker Compose orchestration tools")


@compose_group.command("up")
def compose_up(ctx: Context) -> None:
    """Start services."""
    ctx.print("docker compose up executed")


@compose_group.command("down")
def compose_down(ctx: Context) -> None:
    """Stop services."""
    ctx.print("docker compose down executed")


@compose_group.command("logs")
def compose_logs(ctx: Context) -> None:
    """View service logs."""
    ctx.print("docker compose logs executed")
