"""End-to-end: argparse scanner → rebuild → dispatch → assert payload."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest


@pytest.fixture
def project_with_dispatcher_and_command(tmp_path: Path) -> Path:
    """Tiny tools project plus an argparse-scannable management command."""
    project = tmp_path / "demo"
    project.mkdir()
    tools = project / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "dispatcher.py").write_text(
        textwrap.dedent(
            """
            import json
            import os
            from toolr import command_group, Context
            from toolr.sources import DispatchCommand

            group = command_group("django", "Django", description="Django dispatcher")

            @group.command
            def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
                payload = {
                    "command": dispatched.command,
                    "command_args": dispatched.command_args,
                    "argv": dispatched.argv,
                }
                with open(os.environ["E2E_SIDECAR"], "w") as fh:
                    json.dump(payload, fh)
                return 0
            """
        ).strip()
        + "\n"
    )

    cmds = project / "apps" / "billing" / "management" / "commands"
    cmds.mkdir(parents=True)
    (cmds / "migrate.py").write_text(
        textwrap.dedent(
            """
            \"\"\"Migrate the database.\"\"\"
            def add_arguments(self, parser):
                parser.add_argument('--check', action='store_true', help='Dry run')
                parser.add_argument('--database', default='default', help='Target DB')
            """
        ).strip()
        + "\n"
    )

    (tools / "pyproject.toml").write_text(
        textwrap.dedent(
            """
            [project]
            name = "demo-tools"
            version = "0"

            [tool.toolr]
            venv-location = "in-tree"

            [tool.toolr.argparse.django]
            scan_paths = ["apps/*/management/commands/*.py"]

            [[tool.toolr.argparse.django.attach]]
            parent = "django"
            """
        ).strip()
        + "\n"
    )

    # Pre-populate tools/.venv by symlinking the workspace venv. That
    # interpreter already has toolr-py installed, so toolr's dynamic-
    # layer introspect helper can `import tools.dispatcher` and
    # `import toolr.sources` without an expensive `uv sync` per test.
    # Use sys.prefix (the venv root) directly, not Path(sys.executable).resolve()
    # — the latter resolves through the venv's python symlink to the base
    # interpreter, which doesn't have toolr-py installed.
    workspace_venv = Path(sys.prefix)
    (tools / ".venv").symlink_to(workspace_venv)

    return project


def test_e2e_dispatch_through_argparse_scanner(
    project_with_dispatcher_and_command: Path,
    tmp_path: Path,
    toolr_bin: Path,
) -> None:
    project = project_with_dispatcher_and_command
    sidecar = tmp_path / "captured.json"

    # `TOOLR_TEST_PYTHON` short-circuits toolr's project-venv resolution
    # so the test doesn't need to run `uv sync` in the tmp project. The
    # interpreter must have `toolr-py` importable so the dynamic-layer
    # introspect helper can `import tools.dispatcher`.
    base_env = {**os.environ, "TOOLR_TEST_PYTHON": sys.executable}

    # 1. Explicit rebuild.
    subprocess.run(  # noqa: S603
        [str(toolr_bin), "project", "manifest", "rebuild"],
        check=True,
        cwd=project,
        env=base_env,
    )

    # 2. Invoke through the dispatcher.
    env = {**base_env, "E2E_SIDECAR": str(sidecar)}
    result = subprocess.run(  # noqa: S603
        [str(toolr_bin), "django", "migrate", "--check", "--database", "primary"],
        check=False,
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        msg = f"dispatch failed (exit {result.returncode})\nSTDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
        raise AssertionError(msg)

    # 3. Assert payload.
    captured = json.loads(sidecar.read_text())
    assert captured["command"] == "migrate"
    assert captured["command_args"]["check"] is True
    assert captured["command_args"]["database"] == "primary"
    assert "--check" in captured["argv"]
    assert "--database" in captured["argv"]
    assert "primary" in captured["argv"]
