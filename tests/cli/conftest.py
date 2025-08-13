"""Shared fixtures for CLI tests."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path
from unittest.mock import patch

import pytest

from toolr._parser import Parser
from toolr._registry import CommandGroup
from toolr._registry import CommandRegistry
from toolr._registry import command_group as _command_group


@pytest.fixture
def _patch_get_command_group_storage() -> Iterator[dict[str, CommandGroup]]:
    """Patch the registry before each test."""
    collector: dict[str, CommandGroup] = {}
    with patch("toolr._registry._get_command_group_storage", return_value=collector):
        yield collector


@pytest.fixture
def command_group(_patch_get_command_group_storage: dict[str, CommandGroup]):
    """Create a command group."""
    return _command_group("test", "Test", "Test commands")


@pytest.fixture
def cli_parser(command_group: CommandGroup, tmp_path: Path):
    """Create a parser for CLI testing."""
    parser = Parser(repo_root=tmp_path)
    registry = CommandRegistry()
    registry.discover_and_build(parser)
    return parser
