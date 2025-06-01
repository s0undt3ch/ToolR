"""Docker tools package."""

from __future__ import annotations

from toolr import registry

# Create the main docker command group
docker_group = registry.command_group("docker", "Docker Commands", "Docker container and image management tools")
