from __future__ import annotations

import stat
import sys
from collections.abc import Iterator
from pathlib import Path
from unittest.mock import patch

import pytest

from toolr._context import Context


@pytest.fixture
def foo_binary(tmp_path: Path) -> Iterator[Path]:
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    binary_name = "foo"
    # On Windows, executables need .exe extension
    if sys.platform.startswith("win"):
        binary_name += ".exe"
    foo_binary = bin_dir / binary_name
    with open(foo_binary, "w") as wfh:
        wfh.write("This would never be a binary, but works for the test")
    foo_binary.chmod(stat.S_IRWXU | stat.S_IRGRP | stat.S_IXGRP | stat.S_IROTH | stat.S_IXOTH)
    with patch("os.environ", {"PATH": str(bin_dir)}):
        yield foo_binary


def test_which(ctx: Context, foo_binary: Path):
    """Test that the which method returns the path to an executable in the system PATH."""
    # On Windows, we call which("foo") but it finds "foo.exe"
    cmd = ctx.which(foo_binary.stem)
    assert cmd is not None
    try:
        assert cmd == str(foo_binary)
    except AssertionError:
        if not sys.platform.startswith("win"):  # pragma: no cover
            raise
        # On Windows, the executable is named <executable>.EXE, not <executable>.exe
        assert cmd.lower() == str(foo_binary).lower()


def test_which_not_found_path_environment(ctx: Context, foo_binary: Path):
    """Test that with the wrong PATH, the which method returns None."""
    # Just make sure it's there
    assert ctx.which(foo_binary.stem) is not None
    # Now let's change the PATH to something that doesn't contain the foo binary
    with patch("os.environ", {"PATH": str(foo_binary.parent.parent)}):
        cmd = ctx.which(foo_binary.stem)
    assert cmd is None


def test_which_not_found_path_call_argument(ctx: Context, foo_binary: Path):
    """Test that with the wrong 'path' passed to the call, the which method returns None."""
    # Just make sure it's there
    assert ctx.which(foo_binary.stem) is not None
    # Now let's change the PATH to something that doesn't contain the foo binary
    cmd = ctx.which(foo_binary.stem, path=str(foo_binary.parent.parent))
    assert cmd is None
