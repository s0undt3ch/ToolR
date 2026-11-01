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

# `pty` needs `termios`, which Windows lacks; every test here is POSIX-only.
pty = pytest.importorskip("pty")

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="POSIX signals")

IS_LINUX = sys.platform.startswith("linux")

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
    except KeyboardInterrupt:
        print("interrupted", flush=True)
        raise
    finally:
        time.sleep(0.5)
        print("cleaned", flush=True)
'''

READY_TIMEOUT = 30
EXIT_TIMEOUT = 15
# A forwarded SIGINT ends the command within one 50 ms poll plus its 0.5 s cleanup.
FORWARDED_SIGINT_WINDOW = 2
# Output the command flushed just before exiting can still be in the terminal after toolr is reaped.
TERMINAL_DRAIN = 0.2


def assert_interrupted_once(returncode: int | None, output: str, context: str) -> None:
    """The command got exactly one `KeyboardInterrupt` and its `finally:` ran to the end."""
    assert returncode == 130, context
    assert output.count("interrupted") == 1, output
    assert "cleaned" in output, context


@pytest.fixture
def start_cleanup(
    make_tools_project: Callable[..., ToolsProject],
) -> Iterator[Callable[[], subprocess.Popen[str]]]:
    """Factory: start `toolr sig cleanup` with no terminal and return once the command prints `ready`.

    toolr leads a new session, so its pid is also its process group id. Whatever
    is still running at teardown has its process group killed.
    """
    procs: list[subprocess.Popen[str]] = []

    def _start() -> subprocess.Popen[str]:
        project = make_tools_project(sig=SIG_MODULE)
        proc = subprocess.Popen(  # noqa: S603
            [str(project.toolr_bin), "sig", "cleanup"],
            cwd=project.root,
            env=project.env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=True,
        )
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


class PtyProcess:
    """`toolr` running on a pseudo-terminal, as an interactive shell would run it."""

    def __init__(self, pid: int, master: int) -> None:
        self.pid = pid
        self.master = master
        self.output = ""
        self.returncode: int | None = None

    def _read(self, timeout: float) -> None:
        with selectors.DefaultSelector() as selector:
            selector.register(self.master, selectors.EVENT_READ)
            if not selector.select(timeout=timeout):
                return
        try:
            chunk = os.read(self.master, 4096)
        except OSError:  # EIO on Linux once the terminal's last user is gone
            return
        self.output += chunk.decode(errors="replace").replace("\r\n", "\n")

    def read_until_ready(self) -> None:
        deadline = time.monotonic() + READY_TIMEOUT
        while time.monotonic() < deadline:
            if "ready" in self.output.splitlines():
                return
            self._read(0.1)
        pytest.fail(f"command never printed 'ready'; output:\n{self.output}")

    def wait(self) -> int:
        deadline = time.monotonic() + EXIT_TIMEOUT
        while time.monotonic() < deadline:
            self._read(0.05)
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self._read(TERMINAL_DRAIN)
                self.returncode = os.waitstatus_to_exitcode(status)
                return self.returncode
        pytest.fail(f"toolr still running after {EXIT_TIMEOUT}s; output:\n{self.output}")


@pytest.fixture
def start_cleanup_on_a_terminal(
    make_tools_project: Callable[..., ToolsProject],
) -> Iterator[Callable[[], PtyProcess]]:
    """Factory: start `toolr sig cleanup` on a new pseudo-terminal and return once it prints `ready`.

    The terminal's foreground process group is toolr's, as under a shell. Whatever is still
    running at teardown has its process group killed.
    """
    procs: list[PtyProcess] = []

    def _start() -> PtyProcess:
        project = make_tools_project(sig=SIG_MODULE)
        pid, master = pty.fork()
        if pid == 0:  # pragma: no cover - replaced by exec
            try:
                os.chdir(project.root)
                argv = [str(project.toolr_bin), "sig", "cleanup"]
                os.execve(project.toolr_bin, argv, project.env)  # noqa: S606
            finally:
                os._exit(127)
        proc = PtyProcess(pid, master)
        procs.append(proc)
        proc.read_until_ready()
        return proc

    yield _start

    for proc in procs:
        if proc.returncode is None:
            os.killpg(proc.pid, signal.SIGKILL)
            os.waitpid(proc.pid, 0)
        os.close(proc.master)


def test_ctrl_c_on_a_terminal_interrupts_once(
    start_cleanup_on_a_terminal: Callable[[], PtyProcess],
) -> None:
    proc = start_cleanup_on_a_terminal()
    os.write(proc.master, b"\x03")
    assert_interrupted_once(proc.wait(), proc.output, proc.output)


@pytest.mark.parametrize(
    "send",
    [
        pytest.param(
            os.killpg,
            id="group",
            marks=pytest.mark.skipif(
                IS_LINUX,
                reason="Linux forwards a process-sent SIGINT, so a killpg one arrives twice",
            ),
        ),
        pytest.param(
            os.kill,
            id="pid-only",
            marks=pytest.mark.skipif(
                not IS_LINUX, reason="only Linux can tell a process-sent SIGINT from a terminal one"
            ),
        ),
    ],
)
def test_sigint_from_a_process_interrupts_once(
    start_cleanup: Callable[[], subprocess.Popen[str]], send: Callable[[int, int], None]
) -> None:
    proc = start_cleanup()
    send(proc.pid, signal.SIGINT)
    stdout, stderr = proc.communicate(timeout=EXIT_TIMEOUT)
    assert_interrupted_once(proc.returncode, stdout, stderr)


@pytest.mark.skipif(IS_LINUX, reason="Linux forwards a process-sent SIGINT")
def test_pid_only_sigint_does_not_reach_the_command(
    start_cleanup: Callable[[], subprocess.Popen[str]],
) -> None:
    proc = start_cleanup()
    os.kill(proc.pid, signal.SIGINT)
    with pytest.raises(subprocess.TimeoutExpired):
        proc.wait(timeout=FORWARDED_SIGINT_WINDOW)
    os.kill(proc.pid, signal.SIGTERM)
    stdout, stderr = proc.communicate(timeout=EXIT_TIMEOUT)
    assert proc.returncode == 143, stderr
    assert "interrupted" not in stdout


def test_sigterm_to_toolr_is_forwarded(start_cleanup: Callable[[], subprocess.Popen[str]]) -> None:
    proc = start_cleanup()
    os.kill(proc.pid, signal.SIGTERM)
    stdout, stderr = proc.communicate(timeout=EXIT_TIMEOUT)
    assert proc.returncode == 143, stderr
    assert "interrupted" not in stdout
    assert "cleaned" not in stdout
