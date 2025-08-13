"""Tests for the main CLI entry point."""

from __future__ import annotations

import os
import sys
import types
from unittest.mock import patch

import pytest

import toolr.__main__ as main_module


@pytest.fixture
def skip_loading_entry_points() -> bool:
    """Skip loading entry points."""
    return False


@pytest.fixture(autouse=True)
def _parser(commands_tester):
    return commands_tester.parser


def test_main_runs_and_exits(capfd):
    """Test that main function runs and exits properly."""

    # Ensure tools module exists
    sys.modules["tools"] = types.ModuleType("tools")

    # Run main function - should exit
    with patch("importlib.metadata.entry_points", return_value=[]):
        with pytest.raises(SystemExit) as exc_info:
            main_module.main([])

    # Exit code should be 2 because no command was passed
    assert exc_info.value.code == 2

    out, err = capfd.readouterr()
    assert not out
    assert "the following arguments are required" in err


def test_main_handles_missing_tools_gracefully(capfd):
    """Test that main function handles missing tools gracefully."""

    # Remove tools module and patch import to fail
    sys.modules.pop("tools", None)

    original_import = __builtins__["__import__"]

    def fake_import(name, *args, **kwargs):
        if name == "tools":
            raise ImportError("No module named 'tools'")  # noqa: EM101,TRY003
        return original_import(name, *args, **kwargs)

    with patch("builtins.__import__", fake_import):
        # Should not raise ImportError during execution, but should still exit
        with pytest.raises(SystemExit) as exc_info:
            main_module.main(["third-party", "hello"])
        assert exc_info.value.code == 0

    out, err = capfd.readouterr()
    assert not err
    assert "Hello, World from 3rd-party package!" in out


def test_main_raises_import_error_in_debug_mode():
    """Test that main function raises ImportError in debug mode."""
    # Remove tools module and patch import to fail
    sys.modules.pop("tools", None)

    original_import = __builtins__["__import__"]

    def fake_import(name, *args, **kwargs):
        if name == "tools":
            raise ImportError("No module named 'tools'")  # noqa: EM101,TRY003
        return original_import(name, *args, **kwargs)

    with patch("builtins.__import__", fake_import), patch.dict(os.environ, {"TOOLR_DEBUG_IMPORTS": "1"}):
        # Should raise ImportError in debug mode
        with pytest.raises(ImportError, match="No module named 'tools'"):
            main_module.main([])
