from __future__ import annotations

import json
import os
import subprocess
import sys
import textwrap
from pathlib import Path

from toolr._runner import SCHEMA_VERSION


def _write_tools_module(tools_dir: Path, body: str) -> None:
    tools_dir.mkdir(parents=True, exist_ok=True)
    (tools_dir / "__init__.py").write_text("")
    (tools_dir / "demo.py").write_text(textwrap.dedent(body))


def _write_spec(spec_path: Path, repo_root: Path, *, command: str, function: str, args: dict[str, object]) -> None:
    payload = {
        "schema_version": SCHEMA_VERSION,
        "group": "demo",
        "command": command,
        "module": "tools.demo",
        "function": function,
        "args": args,
        "context": {
            "repo_root": str(repo_root),
            "verbosity": "normal",
            "timestamps": False,
            "log_level": "INFO",
        },
    }
    spec_path.write_text(json.dumps(payload))


def _run_runner(spec_path: Path, repo_root: Path) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["TOOLR_SPEC_FILE"] = str(spec_path)
    env["PYTHONPATH"] = str(repo_root) + os.pathsep + env.get("PYTHONPATH", "")
    # Run from ``repo_root`` so the fake ``tools/`` package wins over any
    # ``tools/`` directory that happens to live in the parent process's CWD
    # (the real toolr project root contains one).
    return subprocess.run(  # noqa: S603
        [sys.executable, "-m", "toolr._runner"],
        env=env,
        cwd=str(repo_root),
        capture_output=True,
        text=True,
        check=False,
    )


def test_runner_invokes_target_function(tmp_path: Path) -> None:
    _write_tools_module(
        tmp_path / "tools",
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def hello(ctx, name: str = "world") -> None:
            ctx.print(f"hi {name}")
        """,
    )
    spec_path = tmp_path / "spec.json"
    _write_spec(spec_path, tmp_path, command="hello", function="hello", args={"name": "Alice"})

    result = _run_runner(spec_path, tmp_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "hi Alice" in result.stdout


def test_runner_propagates_nonzero_exit_via_ctx_exit(tmp_path: Path) -> None:
    _write_tools_module(
        tmp_path / "tools",
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def boom(ctx) -> None:
            ctx.exit(7, "failing on purpose")
        """,
    )
    spec_path = tmp_path / "spec.json"
    _write_spec(spec_path, tmp_path, command="boom", function="boom", args={})

    result = _run_runner(spec_path, tmp_path)
    assert result.returncode == 7


def test_runner_propagates_exception_as_exit_1(tmp_path: Path) -> None:
    _write_tools_module(
        tmp_path / "tools",
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def crash(ctx) -> None:
            raise RuntimeError("crashed")
        """,
    )
    spec_path = tmp_path / "spec.json"
    _write_spec(spec_path, tmp_path, command="crash", function="crash", args={})

    result = _run_runner(spec_path, tmp_path)
    assert result.returncode == 1
    assert "RuntimeError" in result.stderr
    assert "crashed" in result.stderr


def test_runner_fails_clearly_when_spec_env_unset(tmp_path: Path) -> None:
    env = os.environ.copy()
    env.pop("TOOLR_SPEC_FILE", None)
    result = subprocess.run(  # noqa: S603
        [sys.executable, "-m", "toolr._runner"],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode != 0
    assert "TOOLR_SPEC_FILE" in result.stderr
