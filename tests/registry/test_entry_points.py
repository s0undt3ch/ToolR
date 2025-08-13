"""Tests for 3rd-party package entry point discovery."""

from __future__ import annotations

from pathlib import Path

from toolr._registry import CommandRegistry
from toolr.testing import CommandsTester


def test_3rd_party_commands(commands_tester: CommandsTester):
    """Test that 3rd-party commands are discovered via entry points."""
    command_groups = commands_tester.collected_command_groups()
    assert "tools.third-party" in command_groups
    assert "tools.utils" in command_groups

    third_party_commands = command_groups["tools.third-party"].get_commands()
    utils_commands = command_groups["tools.utils"].get_commands()
    assert "hello" in third_party_commands
    assert "version" in third_party_commands
    assert "echo" in utils_commands
    assert "info" in utils_commands


def test_entry_point_discovery_mechanism(tmp_path: Path):
    """Test that entry point discovery mechanism works."""

    registry = CommandRegistry()

    # Create a mock parser
    mock_parser = type("MockParser", (), {"repo_root": tmp_path})()
    registry._set_parser(mock_parser)

    # Test that entry point discovery doesn't raise errors
    # This tests the basic mechanism without requiring actual entry points
    registry._discover_entry_points_commands()
