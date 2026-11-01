"""Signal delivery to a running command, driven through the real `toolr` binary."""

from __future__ import annotations

import os
import selectors
import signal
import subprocess
import sys
import time
from collections.abc import Callable
from collections.abc import Iterator

import pytest

from tests.conftest import ToolsProject

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="POSIX signals")

SIG_MODULE = '''
import time
from toolr import Context, command_group

group = command_group("sig", "Signals", description="Signal tests")


@group.command
def cleanup(ctx: Context) -> None:
    """Sleep, then clean up slowly."""
    print("ready", flush=True)
    try:
        time.sleep(10)
    finally:
        time.sleep(0.5)
        print("cleaned", flush=True)
'''

READY_TIMEOUT = 30
EXIT_TIMEOUT = 15


@pytest.fixture
def start_cleanup(
    make_tools_project: Callable[..., ToolsProject],
) -> Iterator[Callable[[], subprocess.Popen[str]]]:
    """Factory: start `toolr sig cleanup` and return once the command prints `ready`.

    Whatever is still running at teardown has its process group killed.
    """
    procs: list[subprocess.Popen[str]] = []

    def _start() -> subprocess.Popen[str]:
        project = make_tools_project(sig=SIG_MODULE)
        proc = project.popen("sig", "cleanup")
        procs.append(proc)
        assert proc.stdout is not None
        with selectors.DefaultSelector() as selector:
            selector.register(proc.stdout, selectors.EVENT_READ)
            deadline = time.monotonic() + READY_TIMEOUT
            while (remaining := deadline - time.monotonic()) > 0:
                if not selector.select(timeout=remaining):
                    break
                line = proc.stdout.readline()
                if line.strip() == "ready":
                    return proc
                if not line:
                    break
        os.killpg(proc.pid, signal.SIGKILL)
        _, stderr = proc.communicate(timeout=EXIT_TIMEOUT)
        pytest.fail(f"command never printed 'ready'; stderr:\n{stderr}")

    yield _start

    for proc in procs:
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.communicate(timeout=EXIT_TIMEOUT)


def test_group_sigint_lets_finally_finish(
    start_cleanup: Callable[[], subprocess.Popen[str]],
) -> None:
    proc = start_cleanup()
    os.killpg(proc.pid, signal.SIGINT)
    stdout, stderr = proc.communicate(timeout=EXIT_TIMEOUT)
    assert "cleaned" in stdout, stderr
    assert proc.returncode == 130, stderr


def test_sigterm_to_toolr_is_forwarded(start_cleanup: Callable[[], subprocess.Popen[str]]) -> None:
    proc = start_cleanup()
    os.kill(proc.pid, signal.SIGTERM)
    stdout, stderr = proc.communicate(timeout=EXIT_TIMEOUT)
    assert proc.returncode == 143, stderr
    assert "cleaned" not in stdout
