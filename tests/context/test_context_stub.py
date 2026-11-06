"""mypy prefers `_context.pyi` over `_context.py`, so stub drift is invisible to runtime tests
and stubtest checks it."""

from __future__ import annotations

import os
import subprocess
import sys
from importlib.util import find_spec
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).parents[2]
ALLOWLIST = Path(__file__).parent / "files" / "stubtest_allowlist.txt"

# CI builds the extension, so a missing one there is a failure, not a skip.
pytestmark = pytest.mark.skipif(
    find_spec("toolr.utils._rust_utils") is None and not os.environ.get("CI"),
    reason="the compiled toolr-py extension is not built, so stubtest cannot import toolr._context",
)


def test_context_stub_matches_runtime() -> None:
    # cwd is the repo root so stubtest picks up `[tool.mypy]` (mypy_path) from pyproject.toml.
    result = subprocess.run(  # noqa: S603  # interpreter and allowlist path are test-controlled
        [sys.executable, "-m", "mypy.stubtest", "toolr._context", "--allowlist", str(ALLOWLIST)],
        capture_output=True,
        text=True,
        cwd=REPO_ROOT,
        check=False,
    )
    assert result.returncode == 0, f"stubtest found drift:\n{result.stdout}\n{result.stderr}"
