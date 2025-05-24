"""
Platform-specific pytest fixtures for cross-platform command testing.
"""

from __future__ import annotations

import sys

import pytest

# Detect platform
IS_WINDOWS = sys.platform.startswith("win")


@pytest.fixture
def echo_command():
    """Return a cross-platform echo command using Python."""

    def _echo_cmd(stdout: str | None = None, stderr: str | None = None):
        assert stdout is not None or stderr is not None, "Either stdout or stderr must be provided"
        py_script_parts = ["import sys"]
        if stdout is not None:
            py_script_parts.extend(
                [
                    f"sys.stdout.write({stdout!r})",
                    "sys.stdout.flush()",
                ]
            )
        if stderr is not None:
            py_script_parts.extend(
                [
                    f"sys.stderr.write({stderr!r})",
                    "sys.stderr.flush()",
                ]
            )
        # Join the script parts into a single string
        py_script = "; ".join(py_script_parts)
        # Use Python's print() to consistently output text across platforms
        return [sys.executable, "-c", py_script]

    return _echo_cmd


@pytest.fixture
def cat_command():
    """Return a command that reads file contents using Python for better cross-platform support."""

    def _cat_cmd(file_path):
        # Use Python's open() to reliably read files on all platforms
        return [sys.executable, "-c", f"with open(r'{file_path}', 'r') as f: print(f.read(), end='')"]

    return _cat_cmd


@pytest.fixture
def env_var_echo_command():
    """Return platform-specific command to echo an environment variable using Python."""

    def _env_var_echo_cmd(var_name):
        # Use Python's os.environ to reliably access environment variables on all platforms
        return [sys.executable, "-c", f"import os; print(os.environ.get('{var_name}', ''), end='')"]

    return _env_var_echo_cmd


@pytest.fixture
def stdin_cat_command():
    """Return a command that reads from stdin and outputs to stdout without buffering."""
    # Use Python's sys.stdin.read() to reliably read from stdin on all platforms
    # The -u flag ensures unbuffered operation
    return [sys.executable, "-u", "-c", "import sys; sys.stdout.write(sys.stdin.read())"]


@pytest.fixture
def cwd_command():
    """Return a command that prints the current working directory using Python."""
    # Use Python's os.getcwd() to reliably get the current working directory on all platforms
    # The -u flag ensures unbuffered operation
    return [sys.executable, "-u", "-c", "import os, sys; sys.stdout.write(os.getcwd())"]


@pytest.fixture
def sleep_command():
    """Return a Python-based sleep command that should work on all platforms."""

    def _python_sleep_cmd(seconds):
        return [sys.executable, "-c", f"import time; time.sleep({seconds})"]

    return _python_sleep_cmd
