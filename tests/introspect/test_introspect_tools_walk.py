"""Tests for the dynamic-manifest helper walking a tools/ fixture."""

from __future__ import annotations

import json
import subprocess
import sys
import textwrap
from collections.abc import Callable
from pathlib import Path

import pytest


@pytest.fixture
def tools_fixture(tmp_path: Path) -> Callable[[], Path]:
    """Factory: scaffold a ``tools/demo.py`` fixture under ``tmp_path``.

    Returns the ``tools/`` directory path.
    """

    def _make() -> Path:
        tools = tmp_path / "tools"
        tools.mkdir()
        (tools / "__init__.py").write_text("")
        (tools / "demo.py").write_text(
            textwrap.dedent(
                '''
                """Demo dynamic-layer module."""
                from toolr import command_group

                group = command_group("demo", "Demo group", description="A demo.")

                @group.command
                def shout(ctx):
                    """Shout loudly."""
                    return 0
                '''
            ).strip()
            + "\n"
        )
        return tools

    return _make


def test_tools_walk_finds_decorated_command(
    tools_fixture: Callable[[], Path],
    tmp_path: Path,
) -> None:
    tools_root = tools_fixture()
    proc = subprocess.run(  # noqa: S603
        [sys.executable, "-m", "toolr._introspect", "--tools-root", str(tools_root)],
        capture_output=True,
        text=True,
        check=True,
        cwd=str(tmp_path),
    )
    payload = json.loads(proc.stdout)
    names = {g["name"] for g in payload["groups"]}
    assert "demo" in names, payload
    cmd_names = {(c["group"], c["name"]) for c in payload["commands"]}
    assert ("demo", "shout") in cmd_names, payload


def test_broken_module_becomes_warning(tmp_path: Path) -> None:
    tools = tmp_path / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "broken.py").write_text("raise RuntimeError('boom at import')\n")
    proc = subprocess.run(  # noqa: S603
        [sys.executable, "-m", "toolr._introspect", "--tools-root", str(tools)],
        capture_output=True,
        text=True,
        check=True,
        cwd=str(tmp_path),
    )
    payload = json.loads(proc.stdout)
    assert any("broken" in w for w in payload["warnings"]), payload
