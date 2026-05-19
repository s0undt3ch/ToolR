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


def _make_project(tmp_path: Path, name: str, tools_py: str, pyproject_toml: str, command_files: dict[str, str]) -> Path:
    project = tmp_path / name
    project.mkdir()
    tools = project / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "dispatcher.py").write_text(tools_py)
    (tools / "pyproject.toml").write_text(pyproject_toml)
    (tools / ".venv").symlink_to(Path(sys.prefix))
    for relpath, body in command_files.items():
        target = project / relpath
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(body)
    return project


def test_e2e_same_source_attached_to_two_parents(tmp_path: Path, toolr_bin: Path) -> None:
    """A single argparse block attaching to two parents produces working
    children under each parent independently. The user can reach
    `migrate` via either dispatcher, and the dispatcher invoked records
    which path was taken (different print output)."""
    tools_py = (
        textwrap.dedent(
            """
        from toolr import command_group, Context
        from toolr.sources import DispatchCommand

        django_grp = command_group("django", "Django", description="Local Django dispatcher")
        jenkins_grp = command_group("jenkins", "Jenkins", description="Jenkins dispatcher")

        @django_grp.command
        def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
            print(f"local:{dispatched.command}")
            return 0

        @jenkins_grp.command
        def jenkins(ctx: Context, *, dispatched: DispatchCommand) -> int:
            print(f"jenkins:{dispatched.command}")
            return 0
        """
        ).strip()
        + "\n"
    )
    pyproject = (
        textwrap.dedent(
            """
        [project]
        name = "demo-tools"
        version = "0"

        [tool.toolr]
        venv-location = "in-tree"

        [tool.toolr.argparse.commands]
        scan_paths = ["apps/*/management/commands/*.py"]

        [[tool.toolr.argparse.commands.attach]]
        parent = "django"

        [[tool.toolr.argparse.commands.attach]]
        parent = "jenkins"
        """
        ).strip()
        + "\n"
    )
    project = _make_project(
        tmp_path,
        "two-parents",
        tools_py,
        pyproject,
        {
            "apps/x/management/commands/migrate.py": (
                'def add_arguments(self, parser):\n    parser.add_argument("--check", action="store_true")\n'
            )
        },
    )

    env = {**os.environ, "TOOLR_TEST_PYTHON": sys.executable}
    subprocess.run(  # noqa: S603
        [str(toolr_bin), "project", "manifest", "rebuild"], check=True, cwd=project, env=env
    )

    out_local = subprocess.run(  # noqa: S603
        [str(toolr_bin), "django", "migrate"],
        check=True,
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
    ).stdout.strip()
    out_remote = subprocess.run(  # noqa: S603
        [str(toolr_bin), "jenkins", "migrate"],
        check=True,
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
    ).stdout.strip()
    assert out_local == "local:migrate"
    assert out_remote == "jenkins:migrate"


def test_e2e_collision_across_sources_fails_build(tmp_path: Path, toolr_bin: Path) -> None:
    """Two argparse blocks attaching to the same parent with the same
    discovered command name must fail `toolr project manifest rebuild`
    with a clear collision message."""
    tools_py = (
        textwrap.dedent(
            """
        from toolr import command_group, Context
        from toolr.sources import DispatchCommand

        group = command_group("django", "Django", description="Django dispatcher")

        @group.command
        def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
            return 0
        """
        ).strip()
        + "\n"
    )
    pyproject = (
        textwrap.dedent(
            """
        [project]
        name = "demo-tools"
        version = "0"

        [tool.toolr]
        venv-location = "in-tree"

        [tool.toolr.argparse.first]
        scan_paths = ["apps/a/management/commands/*.py"]
        [[tool.toolr.argparse.first.attach]]
        parent = "django"

        [tool.toolr.argparse.second]
        scan_paths = ["apps/b/management/commands/*.py"]
        [[tool.toolr.argparse.second.attach]]
        parent = "django"
        """
        ).strip()
        + "\n"
    )
    cmd_body = 'def add_arguments(self, parser):\n    parser.add_argument("--flag", action="store_true")\n'
    project = _make_project(
        tmp_path,
        "collision",
        tools_py,
        pyproject,
        {
            "apps/a/management/commands/migrate.py": cmd_body,
            "apps/b/management/commands/migrate.py": cmd_body,
        },
    )

    env = {**os.environ, "TOOLR_TEST_PYTHON": sys.executable}
    result = subprocess.run(  # noqa: S603
        [str(toolr_bin), "project", "manifest", "rebuild"],
        check=False,
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0
    combined = result.stdout + result.stderr
    assert "migrate" in combined
    # Names of both colliding sources surface in the error.
    assert "first" in combined
    assert "second" in combined


def test_e2e_auto_rebuild_runs_argparse(
    project_with_dispatcher_and_command: Path,
    tmp_path: Path,
    toolr_bin: Path,
) -> None:
    """Deleting (or never creating) .toolr-manifest.json forces the
    auto-rebuild path, which must include the argparse scanner."""
    project = project_with_dispatcher_and_command
    sidecar = tmp_path / "auto-captured.json"
    manifest_path = project / "tools" / ".toolr-manifest.json"
    assert not manifest_path.exists()  # never built yet

    env = {**os.environ, "E2E_SIDECAR": str(sidecar)}
    subprocess.run(  # noqa: S603
        [str(toolr_bin), "django", "migrate", "--check"],
        check=True,
        cwd=project,
        env=env,
    )

    assert manifest_path.exists()
    captured = json.loads(sidecar.read_text())
    assert captured["command"] == "migrate"
    assert captured["command_args"]["check"] is True
