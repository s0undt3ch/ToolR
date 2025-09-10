"""Tests for registry error cases."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import Mock

import pytest

from toolr import command_group
from toolr._registry import CommandRegistry
from toolr.testing import CommandsTester


def test_registry_parser_not_set_error():
    """Test error when accessing parser before it's set."""
    registry = CommandRegistry()

    with pytest.raises(RuntimeError, match="The parser is not set"):
        _ = registry.parser


def test_registry_set_parser_twice_error():
    """Test error when setting parser twice."""
    registry = CommandRegistry()
    mock_parser1 = Mock()
    mock_parser2 = Mock()

    registry._set_parser(mock_parser1)

    with pytest.raises(RuntimeError, match=r"A parser has already been set"):
        registry._set_parser(mock_parser2)


def test_build_parsers_missing_parent_group(tmp_path: Path):
    """Test parser building with missing parent command group."""
    with CommandsTester(search_path=tmp_path) as tester:
        # Create a command group with a non-existent parent
        command_group("child", "Child Group", "A child group", parent="nonexistent.parent")

        # Should raise an error when building parsers
        with pytest.raises(
            ValueError, match=r"Parent command group 'nonexistent.parent' for command 'child' does not exist"
        ):
            tester.registry._build_parsers()
