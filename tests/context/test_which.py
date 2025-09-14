from __future__ import annotations

import sys
from pathlib import Path
from unittest.mock import patch

from toolr._context import Context


def test_which(ctx: Context):
    """Test that the which method returns the path to an executable in the system PATH."""
    cmd = ctx.which("python")
    assert cmd is not None
    # Should just match the executable that is running the test suite
    system_python = sys.executable
    if sys.platform.startswith("win"):
        # On Windows, the executable is named python.EXE, not python.exe
        # Let's just normalize the path to python.exe
        system_python = system_python.lower()
        # And now, we messed with the executable path directories case, so let's also normalize
        # the shutil returned command path
        cmd = cmd.lower()
    assert cmd == system_python


def test_which_not_found_path_environment(ctx: Context, tmp_path: Path):
    """Test that with the wrong PATH, the which method returns None."""
    with patch("os.environ", {"PATH": str(tmp_path)}):
        cmd = ctx.which("python")
    assert cmd is None


def test_which_not_found_path_call_argument(ctx: Context, tmp_path: Path):
    """Test that with the wrong 'path' passed to the call, the which method returns None."""
    cmd = ctx.which("python", path=str(tmp_path))
    assert cmd is None
