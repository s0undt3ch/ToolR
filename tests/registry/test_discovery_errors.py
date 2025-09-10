"""Tests for discovery error cases."""

from __future__ import annotations

import os
from unittest.mock import Mock
from unittest.mock import patch

import pytest

from toolr import Context
from toolr import command_group
from toolr._registry import CommandRegistry


def test_discover_local_commands_with_debug_imports(tmp_path):
    """Test local command discovery with TOOLR_DEBUG_IMPORTS enabled."""
    # Create a tools directory with a problematic module
    tools_dir = tmp_path / "tools"
    tools_dir.mkdir()

    # Create a module that will cause an import error
    problematic_module = tools_dir / "problematic.py"
    problematic_module.write_text("import nonexistent_module")

    registry = CommandRegistry()
    mock_parser = Mock()
    mock_parser.repo_root = tmp_path
    registry._set_parser(mock_parser)

    # Should raise an error when TOOLR_DEBUG_IMPORTS is enabled
    with patch.dict(os.environ, {"TOOLR_DEBUG_IMPORTS": "1"}):
        with pytest.raises(ImportError):
            registry._discover_local_commands()


def test_discover_local_commands_no_tools_dir(tmp_path):
    """Test local command discovery when tools directory doesn't exist."""
    registry = CommandRegistry()
    mock_parser = Mock()
    mock_parser.repo_root = tmp_path
    registry._set_parser(mock_parser)

    # Should not raise an error when tools directory doesn't exist
    registry._discover_local_commands()


def test_command_decorator_with_function():
    """Test command decorator when called with a function directly."""
    group = command_group("test", "Test Group", "A test group")

    def test_command(ctx: Context) -> None:
        """Test command."""

    # Test the overload where command is called with a function
    decorated = group.command(test_command)

    # Should return the function and register it
    assert decorated is test_command
    commands = group.get_commands()
    assert len(commands) == 1
    assert "test-command" in commands
    assert commands["test-command"] is test_command


def test_entry_points_discovery_empty():
    """Test entry point discovery when no entry points exist."""
    registry = CommandRegistry()
    mock_parser = Mock()
    registry._set_parser(mock_parser)

    with patch("importlib.metadata.entry_points", return_value=[]):
        # Should not raise an error
        registry._discover_entry_points_commands()


def test_entry_points_discovery_with_error():
    """Test entry point discovery with import errors."""
    registry = CommandRegistry()
    mock_parser = Mock()
    registry._set_parser(mock_parser)

    # Mock entry points to return an entry point that will fail to load
    mock_entry_point = Mock()
    mock_entry_point.module = "nonexistent.module"
    mock_entry_point.load.side_effect = ImportError("Module not found")

    with patch("importlib.metadata.entry_points", return_value=[mock_entry_point]):
        # Should raise an error when entry point loading fails
        with pytest.raises(ImportError, match=r"Module not found"):
            registry._discover_entry_points_commands()
