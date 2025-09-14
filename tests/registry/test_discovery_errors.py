"""Tests for discovery error cases."""

from __future__ import annotations

import importlib
import os
from unittest.mock import Mock
from unittest.mock import patch

import pytest
from msgspec import Struct

from toolr import Context
from toolr import command_group
from toolr._registry import CommandRegistry


class ImportErrorSideEffect(Struct, frozen=True):
    module_name: str

    def __call__(self, module_name):
        if module_name == self.module_name:
            error_msg = "Simulated import error"
            raise ImportError(error_msg)
        # For other modules, use the real import
        return importlib.import_module(module_name)  # pragma: no cover


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
        with pytest.raises(ModuleNotFoundError):
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
    mock_entry_point.load.side_effect = ModuleNotFoundError("Module not found")

    with patch("importlib.metadata.entry_points", return_value=[mock_entry_point]):
        # Should raise an error when entry point loading fails
        with pytest.raises(ModuleNotFoundError, match=r"Module not found"):
            registry._discover_entry_points_commands()


def test_import_error_always_raised(tmp_path):
    """Test that ImportError is always raised during command discovery."""
    # Create a tools directory
    tools_dir = tmp_path / "tools"
    tools_dir.mkdir()
    tools_dir.joinpath("__init__.py").touch()

    # Create a module that will cause an ImportError (not ModuleNotFoundError)
    import_error_module = tools_dir / "import_error.py"
    import_error_module.write_text(
        "from toolr import command_group\ngroup = command_group('test', 'Test', 'Test commands')"
    )

    registry = CommandRegistry()
    mock_parser = Mock()
    mock_parser.repo_root = tmp_path
    registry._set_parser(mock_parser)

    # Mock importlib.import_module to raise ImportError for this specific module
    with patch("importlib.import_module") as mock_import:
        mock_import.side_effect = ImportErrorSideEffect(module_name="tools.import_error")

        # Should raise an error - ImportError should always be raised
        with pytest.raises(ImportError, match="Simulated import error"):
            registry._discover_local_commands()


def test_mixed_error_scenarios(tmp_path):
    """Test mixed error scenarios to ensure proper differentiation."""
    # Create a tools directory with multiple modules
    tools_dir = tmp_path / "tools"
    tools_dir.mkdir()
    tools_dir.joinpath("__init__.py").touch()

    # Create a module that will cause a ModuleNotFoundError
    missing_dep_module = tools_dir / "missing_dep.py"
    missing_dep_module.write_text("import nonexistent_package")

    # Create a module that will cause an ImportError
    import_error_module = tools_dir / "import_error.py"
    import_error_module.write_text(
        "from toolr import command_group\ngroup = command_group('test', 'Test', 'Test commands')"
    )

    # Create a valid module
    valid_module = tools_dir / "valid.py"
    valid_module.write_text(
        "from toolr import command_group\ngroup = command_group('valid', 'Valid', 'Valid commands')"
    )

    registry = CommandRegistry()
    mock_parser = Mock()
    mock_parser.repo_root = tmp_path
    registry._set_parser(mock_parser)

    # Mock importlib.import_module to raise ImportError for the import_error module
    with patch("importlib.import_module") as mock_import:
        mock_import.side_effect = ImportErrorSideEffect(module_name="tools.import_error")

        # Should raise ImportError (not ModuleNotFoundError) because ImportError takes precedence
        with pytest.raises(ImportError, match="Simulated import error"):
            registry._discover_local_commands()


def test_module_not_found_vs_import_error_differentiation(tmp_path):
    """Test that the registry correctly differentiates between ModuleNotFoundError and ImportError."""
    # Create a tools directory
    tools_dir = tmp_path / "tools"
    tools_dir.mkdir()
    tools_dir.joinpath("__init__.py").touch()

    # Test 1: ModuleNotFoundError should be suppressed
    missing_dep_module = tools_dir / "missing_dep.py"
    missing_dep_module.write_text("import nonexistent_package")

    registry = CommandRegistry()
    mock_parser = Mock()
    mock_parser.repo_root = tmp_path
    registry._set_parser(mock_parser)

    # Should not raise an error
    registry._discover_local_commands()

    # Test 2: ImportError should be raised
    import_error_module = tools_dir / "import_error.py"
    import_error_module.write_text(
        "from toolr import command_group\ngroup = command_group('test', 'Test', 'Test commands')"
    )

    # Mock importlib.import_module to raise ImportError for the import_error module
    with patch("importlib.import_module") as mock_import:
        mock_import.side_effect = ImportErrorSideEffect(module_name="tools.import_error")

        # Should raise ImportError
        with pytest.raises(ImportError, match="Simulated import error"):
            registry._discover_local_commands()
