"""Tests for command discovery and parser building."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

from toolr.testing import CommandsTester

CASES_PATH = Path(__file__).parent / "cases"


def test_discover_commands_no_tools_dir(tmp_path: Path):
    """Test discovery when tools directory doesn't exist."""
    with patch("importlib.metadata.entry_points", return_value=[]):
        with CommandsTester(search_path=tmp_path) as commands_tester:
            command_groups = commands_tester.collected_command_groups()

    # Should not raise an error
    assert not command_groups


def test_discover_simple_case():
    """Test discovery with the simple test case."""
    # Point to our test case directory
    test_case_dir = CASES_PATH / "simple"
    with CommandsTester(test_case_dir) as tester:
        registry = tester.registry

        # Run discovery
        registry._discover_commands()

        # Should have discovered command groups
        command_groups = tester.collected_command_groups()
        assert len(command_groups) >= 2  # docker and git groups
        assert "tools.docker" in command_groups
        assert "tools.git" in command_groups

        # Should have pending commands
        assert len(command_groups["tools.git"].get_commands()) == 2
        assert len(command_groups["tools.docker"].get_commands()) == 2

        # Check specific command groups
        docker_group = command_groups["tools.docker"]
        git_group = command_groups["tools.git"]

        assert docker_group.title == "Docker Commands"
        assert git_group.title == "Git Commands"


def test_discover_nested_case():
    """Test discovery with nested command groups."""
    test_case_dir = CASES_PATH / "nested"
    with CommandsTester(test_case_dir) as tester:
        registry = tester.registry

        # Run discovery
        registry._discover_commands()

        # Should have discovered nested command groups
        command_groups = tester.collected_command_groups()
        assert "tools.docker" in command_groups
        assert "tools.docker.build" in command_groups
        assert "tools.docker.compose" in command_groups

        # Check nested group relationships
        docker_group = command_groups["tools.docker"]
        assert docker_group
        build_group = command_groups["tools.docker.build"]
        compose_group = command_groups["tools.docker.compose"]

        assert build_group.parent == "tools.docker"
        assert compose_group.parent == "tools.docker"
        assert build_group.full_name == "tools.docker.build"
        assert compose_group.full_name == "tools.docker.compose"


def test_discover_mixed_case():
    """Test discovery with mixed command structures."""
    test_case_dir = CASES_PATH / "mixed"
    with CommandsTester(test_case_dir) as tester:
        registry = tester.registry

        # Run discovery
        registry._discover_commands()

        # Should have top-level groups
        command_groups = tester.collected_command_groups()
        assert "tools.deployment" in command_groups
        assert "tools.utils" in command_groups

        # Should have nested groups
        assert "tools.deployment.k8s" in command_groups
        assert "tools.deployment.aws" in command_groups

        # Check that we have both top-level and nested commands
        assert len(command_groups["tools.deployment"].get_commands()) == 2  # status, rollback
        assert len(command_groups["tools.deployment.k8s"].get_commands()) >= 2  # deploy, scale
