"""Tests for the main CLI entry point."""

from __future__ import annotations

import os
import sys
import types
from pathlib import Path
from unittest.mock import patch

import pytest

import toolr.__main__ as main_module


@pytest.fixture
def dummy_parser():
    """Create a dummy parser that calls sys.exit()."""

    class DummyParser:
        def __init__(self):
            self.repo_root = Path.cwd()

        def parse_args(self):
            return types.SimpleNamespace()

        def run(self):
            sys.exit(0)

    return DummyParser


@pytest.fixture
def dummy_registry():
    """Create a dummy registry that doesn't have side effects."""

    class DummyRegistry:
        def discover_and_build(self, parser):
            pass

    return DummyRegistry()


@pytest.fixture
def registry(dummy_registry):
    with patch.object(main_module, "registry", dummy_registry):
        yield dummy_registry


@pytest.fixture
def parser(dummy_parser, registry):
    with patch.object(main_module, "Parser", dummy_parser):
        yield parser


@pytest.mark.usefixtures("parser")
def test_main_runs_and_exits():
    """Test that main function runs and exits properly."""

    # Ensure tools module exists
    sys.modules["tools"] = types.ModuleType("tools")

    # Run main function - should exit
    with pytest.raises(SystemExit) as exc_info:
        main_module.main()
    assert exc_info.value.code == 0


@pytest.mark.usefixtures("parser")
def test_main_handles_missing_tools_gracefully():
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
            main_module.main()
        assert exc_info.value.code == 0


@pytest.mark.usefixtures("parser")
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
            main_module.main()


@pytest.fixture
def _restore_sys_path():
    sys_path = sys.path[:]
    repo_root = str(Path.cwd())
    if repo_root in sys.path:
        sys.path.remove(repo_root)
    yield
    sys.path = sys_path[:]


@pytest.mark.usefixtures("_restore_sys_path", "parser")
def test_main_manipulates_sys_path():
    """Test that main function manipulates sys.path correctly."""

    # Ensure tools module exists
    sys.modules["tools"] = types.ModuleType("tools")

    # We removed the repo root from sys.path in the fixture.
    # Running the main function should add it back in order to find the tools module.
    original_path_length = len(sys.path)

    # Run main function
    with pytest.raises(SystemExit) as exc_info:
        main_module.main()
    assert exc_info.value.code == 0

    # Check that sys.path was manipulated
    assert len(sys.path) == original_path_length + 1
