from __future__ import annotations

import shutil
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
        commands_tester.discover()
        yield commands_tester


@pytest.fixture(scope="session")
def toolr_bin() -> Path:
    """Path to the freshly-built ``toolr`` binary for subprocess tests."""
    candidate = Path(__file__).parent.parent / "target" / "release" / "toolr"
    if candidate.exists():
        return candidate
    found = shutil.which("toolr")
    if found is None:
        pytest.skip("toolr binary not built; run `cargo build --release -p toolr` first")
    return Path(found)
