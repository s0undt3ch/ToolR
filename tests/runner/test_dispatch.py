from __future__ import annotations

import json
import os
import subprocess
import sys
import textwrap
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr._runner import SCHEMA_VERSION


@pytest.fixture
def tools_module(tmp_path: Path) -> Callable[[str], Path]:
    """Factory: write a ``tools/demo.py`` with the given body. Returns the repo root."""

    def _make(body: str) -> Path:
        tools_dir = tmp_path / "tools"
        tools_dir.mkdir(parents=True, exist_ok=True)
        (tools_dir / "__init__.py").write_text("")
        (tools_dir / "demo.py").write_text(textwrap.dedent(body))
        return tmp_path

    return _make


@pytest.fixture
def spec_file(tmp_path: Path) -> Callable[..., Path]:
    """Factory: write a runner spec JSON to ``tmp_path/spec.json``. Returns its path."""

    def _make(
        *,
        command: str,
        function: str,
        args: dict[str, object] | None = None,
        repo_root: Path | None = None,
    ) -> Path:
        payload = {
            "schema_version": SCHEMA_VERSION,
            "group": "demo",
            "command": command,
            "module": "tools.demo",
            "function": function,
            "args": args or {},
            "context": {
                "repo_root": str(repo_root or tmp_path),
                "verbosity": "normal",
                "timestamps": False,
                "log_level": "INFO",
            },
        }
        spec_path = tmp_path / "spec.json"
        spec_path.write_text(json.dumps(payload))
        return spec_path

    return _make


@pytest.fixture
def run_runner(tmp_path: Path) -> Callable[[Path], subprocess.CompletedProcess[str]]:
    """Factory: spawn ``python -m toolr._runner`` with ``TOOLR_SPEC_FILE`` set."""

    def _run(spec_path: Path) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env["TOOLR_SPEC_FILE"] = str(spec_path)
        # PYTHONPATH addition lets ``import tools.demo`` find the tmp ``tools/`` package.
        env["PYTHONPATH"] = str(tmp_path) + os.pathsep + env.get("PYTHONPATH", "")
        # cwd=tmp_path makes the tmp ``tools/`` package the unambiguous ``tools``
        # import target (the real toolr project root also contains a ``tools/``
        # directory which would otherwise shadow it via ``python -m`` injecting
        # cwd as ``sys.path[0]``).
        return subprocess.run(
            [sys.executable, "-m", "toolr._runner"],
            env=env,
            cwd=str(tmp_path),
            capture_output=True,
            text=True,
            check=False,
        )

    return _run


def test_runner_invokes_target_function(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def hello(ctx, name: str = "world") -> None:
            ctx.print(f"hi {name}")
        """
    )
    spec_path = spec_file(command="hello", function="hello", args={"name": "Alice"})
    result = run_runner(spec_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "hi Alice" in result.stdout


def test_runner_propagates_nonzero_exit_via_ctx_exit(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def boom(ctx) -> None:
            ctx.exit(7, "failing on purpose")
        """
    )
    spec_path = spec_file(command="boom", function="boom")
    result = run_runner(spec_path)
    assert result.returncode == 7


def test_runner_propagates_exception_as_exit_1(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def crash(ctx) -> None:
            raise RuntimeError("crashed")
        """
    )
    spec_path = spec_file(command="crash", function="crash")
    result = run_runner(spec_path)
    assert result.returncode == 1
    assert "RuntimeError" in result.stderr
    assert "crashed" in result.stderr


def test_runner_fails_clearly_when_spec_env_unset(tmp_path: Path) -> None:
    # This test exercises the env-unset path directly without the factory
    # fixtures because the whole point is the absence of ``TOOLR_SPEC_FILE``.
    env = os.environ.copy()
    env.pop("TOOLR_SPEC_FILE", None)
    result = subprocess.run(
        [sys.executable, "-m", "toolr._runner"],
        env=env,
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode != 0
    assert "TOOLR_SPEC_FILE" in result.stderr
