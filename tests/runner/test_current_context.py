"""The runner sets ``toolr._context._current_ctx`` before dispatching.

The subprocess tests here spawn the real ``python -m toolr._runner`` (same
pattern as ``tests/runner/test_dispatch.py``); the in-process test calls
``run()`` directly so the wiring itself is measured by coverage.
"""

from __future__ import annotations

import contextvars
import json
import os
import subprocess
import sys
import textwrap
from collections.abc import Callable
from pathlib import Path

import msgspec
import pytest

from toolr._context import _current_ctx
from toolr._runner import SCHEMA_VERSION
from toolr._runner import RunnerSpec
from toolr._runner import run


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
def spec_payload(tmp_path: Path) -> Callable[..., dict[str, object]]:
    """Factory: build a runner spec payload for the ``tools.demo`` module."""

    def _make(
        *,
        command: str,
        function: str,
        args: dict[str, object] | None = None,
        repo_root: Path | None = None,
    ) -> dict[str, object]:
        return {
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

    return _make


@pytest.fixture
def spec_file(
    tmp_path: Path, spec_payload: Callable[..., dict[str, object]]
) -> Callable[..., Path]:
    """Factory: write a runner spec JSON to ``tmp_path/spec.json``. Returns its path."""

    def _make(**kwargs: object) -> Path:
        spec_path = tmp_path / "spec.json"
        spec_path.write_text(json.dumps(spec_payload(**kwargs)))
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
        # directory which would otherwise shadow it).
        return subprocess.run(
            [sys.executable, "-m", "toolr._runner"],
            env=env,
            cwd=str(tmp_path),
            capture_output=True,
            text=True,
            check=False,
        )

    return _run


def test_current_context_is_set_before_target_runs(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group
        from toolr._context import current_context

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def hello(ctx, name: str = "world") -> None:
            same = current_context() is ctx
            ctx.print(f"hi {name} same={same}")
        """
    )
    spec_path = spec_file(command="hello", function="hello", args={"name": "Alice"})
    result = run_runner(spec_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "hi Alice same=True" in result.stdout


def test_current_context_works_at_module_import_time(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    """The runner sets the contextvar *before* importing the target module, so a
    module-level call succeeds — it must never regress to failing here.
    """
    tools_module(
        """
        from toolr import command_group
        from toolr._context import current_context

        # Module-level call: this runs during `_import_target()`.
        _repo_root_at_import = current_context().repo_root

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def hello(ctx) -> None:
            ctx.print(f"import-time repo_root matched={_repo_root_at_import == ctx.repo_root}")
        """
    )
    spec_path = spec_file(command="hello", function="hello")
    result = run_runner(spec_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "import-time repo_root matched=True" in result.stdout


def test_run_sets_the_contextvar_to_the_context_it_builds(
    tools_module: Callable[[str], Path],
    spec_payload: Callable[..., dict[str, object]],
) -> None:
    """In-process counterpart: ``run()`` itself sets the var to the ``ctx`` it passes on."""
    tools_module(
        """
        from toolr._context import current_context

        SEEN = {}

        def record(ctx) -> None:
            SEEN["same"] = current_context() is ctx
        """
    )
    spec = msgspec.convert(spec_payload(command="record", function="record"), type=RunnerSpec)
    saved_path = sys.path[:]
    try:
        # copy_context() keeps the runner's `.set()` out of the test session's
        # own context, so later tests still see an unset var.
        rc = contextvars.copy_context().run(run, spec)
        assert rc == 0
        # Deferred import is intentional: the `tools` package is created at
        # runtime and only becomes importable after `run()` appends repo_root.
        import tools.demo  # noqa: PLC0415 — created at runtime

        assert tools.demo.SEEN == {"same": True}
    finally:
        sys.path[:] = saved_path
        sys.modules.pop("tools.demo", None)
        sys.modules.pop("tools", None)
    assert _current_ctx.get(None) is None
