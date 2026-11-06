"""Check the hand-written `_context.pyi` against the real `_context` module with mypy's stubtest.

mypy prefers a `.pyi` over its `.py`, so a missing or stale stub entry is invisible to the runtime
and to any test that imports the module. stubtest imports the runtime and compares signatures,
names and defaults, which an AST comparison of the two files cannot do.
"""

from __future__ import annotations

import os
import subprocess
import sys
from importlib.util import find_spec
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).parents[2]
ALLOWLIST = Path(__file__).parent / "files" / "stubtest_allowlist.txt"

# `toolr._context` imports the compiled extension through `toolr.utils`; without it stubtest
# could only report an import error, which says nothing about the stub. CI always builds it, so
# there a missing extension fails the test rather than skipping it silently.
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
