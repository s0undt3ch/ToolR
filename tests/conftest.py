from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import pytest

from toolr.testing import CommandsTester


@pytest.fixture
def commands_tester(tmp_path: Path) -> Iterator[CommandsTester]:
    """Create a commands tester."""
    commands_tester = CommandsTester(search_path=tmp_path)
    with commands_tester:
        commands_tester.registry.discover_and_build()
        yield commands_tester
