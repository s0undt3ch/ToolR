"""End-to-end backwards-compat: existing tools/*.py keep working."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import textwrap
from collections.abc import Callable
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(
    shutil.which("uv") is None,
    reason="uv not on PATH",
)

REPO_ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture
def project_dir(tmp_path: Path) -> Callable[[], Path]:
    """Factory: scaffold a minimal project with a tools/ dir."""

    def _make() -> Path:
        proj = tmp_path / "proj"
        proj.mkdir()
        tools = proj / "tools"
        tools.mkdir()
        (tools / "__init__.py").write_text("")
        (tools / "demo.py").write_text(
            textwrap.dedent(
                '''\
                from __future__ import annotations

                from toolr import command_group

                group = command_group("demo", "Demo commands", docstring=__doc__)


                @group.command
                def hello(ctx, name: str = "world") -> None:
                    """Print a greeting.

                    Args:
                        name: Who to greet.
                    """
                    ctx.print(f"hello, {name}")
                '''
            )
        )
        (tools / "pyproject.toml").write_text(
            textwrap.dedent(
                f"""\
                [project]
                name = "demo-tools"
                version = "0"
                requires-python = ">=3.11"
                dependencies = ["toolr"]

                [tool.uv.sources]
                toolr = {{ path = "{REPO_ROOT.as_posix()}", editable = true }}
                """
            )
        )
        return proj

    return _make


@pytest.mark.xfail(
    reason=(
        "maturin develop --release does not install [[bin]] alongside the "
        "pyo3 lib (see test_editable_install_yields_runnable_toolr_binary). "
        "Compat path will work once that limitation is resolved."
    ),
    strict=False,
)
def test_existing_command_group_authoring_still_runs(
    project_dir: Callable[[], Path],
    tmp_path: Path,
) -> None:
    proj = project_dir()
    uv = shutil.which("uv")
    assert uv is not None
    venv = tmp_path / "venv"
    subprocess.run(  # noqa: S603
        [uv, "venv", "--python", sys.executable, str(venv)],
        check=True,
    )
    venv_bin = venv / ("Scripts" if os.name == "nt" else "bin")
    env = {**os.environ, "VIRTUAL_ENV": str(venv)}
    subprocess.run(  # noqa: S603
        [str(venv_bin / "python"), "-m", "pip", "install", "maturin>=1.7,<2.0"],
        check=True,
        env=env,
    )
    subprocess.run(  # noqa: S603
        [str(venv_bin / "maturin"), "develop", "--release"],
        cwd=REPO_ROOT,
        check=True,
        env=env,
    )
    toolr_bin = venv_bin / ("toolr.exe" if os.name == "nt" else "toolr")
    help_result = subprocess.run(  # noqa: S603
        [str(toolr_bin), "--help"],
        cwd=proj,
        check=True,
        capture_output=True,
        text=True,
    )
    assert "demo" in help_result.stdout, help_result.stdout

    run_result = subprocess.run(  # noqa: S603
        [str(toolr_bin), "demo", "hello", "--name", "plan9"],
        cwd=proj,
        check=True,
        capture_output=True,
        text=True,
    )
    assert "hello, plan9" in run_result.stdout, run_result.stdout
