"""Shared fixtures for CLI tests."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path
from unittest.mock import patch

import pytest

from toolr._parser import Parser
from toolr._registry import CommandGroup
from toolr._registry import CommandRegistry


@pytest.fixture
def _registry(tmp_path: Path) -> Iterator[CommandRegistry]:
    """Patch the registry before each test."""
    _registry = CommandRegistry()
    with patch("toolr._registry.registry", _registry):
        yield _registry


@pytest.fixture
def command_group(_registry: CommandRegistry):
    """Create a command group."""
    return _registry.command_group("test", "Test", "Test commands")


@pytest.fixture
def cli_parser(_registry: CommandRegistry, command_group: CommandGroup, tmp_path: Path):
    """Create a parser for CLI testing."""
    parser = Parser(repo_root=tmp_path)
    _registry.discover_and_build(parser)
    return parser
