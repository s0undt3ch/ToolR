"""Fixtures for runner tests, which import throwaway ``tools`` packages."""

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
from toolr._runner import _is_repo_tools_module


@pytest.fixture(autouse=True)
def _isolate_repo_tools(monkeypatch: pytest.MonkeyPatch) -> None:
    """Snapshot ``sys.path`` and hide the repo's own cached ``tools*`` modules for every test.

    Runner tests import throwaway ``tools`` packages and edit ``sys.path``. Collection imports the
    repo's ``tools`` namespace package, which keeps its original search path once cached, so a
    regular ``tools`` package written under ``tmp_path`` would not be found. ``monkeypatch``
    restores both afterwards.
    """
    monkeypatch.setattr(sys, "path", sys.path[:])
    for name in [m for m in sys.modules if _is_repo_tools_module(m)]:
        monkeypatch.delitem(sys.modules, name)


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
    """Factory: build a runner spec payload for the ``tools.demo`` module.

    ``command``, ``function`` and ``args`` have the obvious shape; ``repo_root`` defaults to
    ``tmp_path``. Any other keyword replaces the top-level key of that name, and ``drop`` names
    the top-level keys to remove from the payload.
    """

    def _make(
        *,
        command: str = "hello",
        function: str = "hello",
        args: dict[str, object] | None = None,
        repo_root: Path | None = None,
        drop: tuple[str, ...] = (),
        **overrides: object,
    ) -> dict[str, object]:
        payload: dict[str, object] = {
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
        payload.update(overrides)
        for key in drop:
            del payload[key]
        return payload

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
