from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest

from toolr.testing import CommandsTester


@pytest.fixture
def skip_loading_entry_points() -> bool:
    """Skip loading entry points."""
    return False


@pytest.fixture
def commands_tester(tmp_path: Path, skip_loading_entry_points: bool) -> Iterator[CommandsTester]:
    """Create a commands tester."""
    commands_tester = CommandsTester(search_path=tmp_path, skip_loading_entry_points=skip_loading_entry_points)
    with commands_tester:
        commands_tester.registry.discover_and_build()
        yield commands_tester
