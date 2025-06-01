"""Docker build commands."""

from __future__ import annotations

from . import docker_group

# Create a nested build command group
build_group = docker_group.command_group("build", "Build Commands", "Docker image build tools")


@build_group.command("image", help="Build a Docker image")
def build_image(args):
    """Build a Docker image."""
    return "docker build image executed"


@build_group.command("context", help="Build with context")
def build_context(args):
    """Build with context."""
    return "docker build context executed"


@build_group.command("multi-stage", help="Build multi-stage image")
def build_multi_stage(args):
    """Build multi-stage image."""
    return "docker build multi-stage executed"
