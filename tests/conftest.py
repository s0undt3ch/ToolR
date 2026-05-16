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
    """Path to the ``toolr`` binary for subprocess tests.

    Prefers `shutil.which("toolr")` so the test exercises whichever
    binary the surrounding environment actually picks up (in CI: the
    one extracted from the `toolr-archive` artifact and put on PATH;
    locally: whatever `mise` / `cargo install` / `pip install toolr`
    placed). Falls back to `target/release/toolr` only when nothing's
    on PATH so a developer with `cargo build --release` and no install
    can still run subprocess tests.
    """
    found = shutil.which("toolr")
    if found is not None:
        return Path(found)
    candidate = Path(__file__).parent.parent / "target" / "release" / "toolr"
    if candidate.exists():
        return candidate
    pytest.skip(
        "no toolr binary on PATH and no `target/release/toolr` — "
        "run `cargo build --release -p toolr` or install toolr first"
    )
