from __future__ import annotations

import os
import shutil
from collections.abc import Iterator
from pathlib import Path

import pytest

from toolr.testing import CommandsTester

# --------------------------------------------------------------------
# Subprocess-coverage bootstrap.
# --------------------------------------------------------------------
#
# CI's `_test.yml` exports `PYTHONPATH=tests/support/coverage` +
# `COVERAGE_PROCESS_START=.coveragerc` before invoking pytest so that
# subprocess Pythons spawned by tests (`python -m toolr._runner`,
# the cross-wheel install smoke, …) run
# `sitecustomize.py` → `coverage.process_startup()` and contribute
# data files to the parallel coverage run.
#
# Locally, `pytest` ran without those exports silently loses every
# subprocess's coverage credit. Set them here so the local invocation
# mirrors CI without each contributor needing to remember the wrapper
# env vars. The shim is a no-op when coverage isn't active (the
# sitecustomize swallows `ImportError`).

_TESTS_DIR = Path(__file__).resolve().parent
_REPO_ROOT = _TESTS_DIR.parent
_COVERAGE_SUPPORT_DIR = _TESTS_DIR / "support" / "coverage"
_COVERAGERC = _REPO_ROOT / ".coveragerc"


def pytest_configure(config: pytest.Config) -> None:
    """Match CI's `_test.yml` env for subprocess coverage."""
    if _COVERAGE_SUPPORT_DIR.is_dir():
        existing = os.environ.get("PYTHONPATH", "")
        entries = existing.split(os.pathsep) if existing else []
        if str(_COVERAGE_SUPPORT_DIR) not in entries:
            os.environ["PYTHONPATH"] = (
                str(_COVERAGE_SUPPORT_DIR) + os.pathsep + existing
                if existing
                else str(_COVERAGE_SUPPORT_DIR)
            )
    if _COVERAGERC.is_file():
        os.environ.setdefault("COVERAGE_PROCESS_START", str(_COVERAGERC))


@pytest.fixture
def commands_tester(tmp_path: Path) -> Iterator[CommandsTester]:
    """Create a commands tester."""
    commands_tester = CommandsTester(search_path=tmp_path)
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
