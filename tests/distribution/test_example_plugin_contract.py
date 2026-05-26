"""Packaging-contract test for `examples/plugin-package/`.

This proves the third-party plugin contract end-to-end:

1. A hatchling-built wheel of `toolr-plugin-example` actually ships
   `toolr_example_plugin/toolr-manifest.json` inside the wheel.
2. After installing that wheel into a project's tools venv -- alongside
   the matching pre-built `toolr` (CLI) and `toolr_py` (pyo3) wheels
   from `wheelhouse/` -- the commands declared by the example appear in
   `toolr --help`, proving the dispatch-time glob + merge picks them up.
3. Running one of those commands produces the expected output, proving
   the manifest's `module` + `function` fields resolve correctly.

The test is marked `distribution` (opt-in, slow). It piggybacks on the
existing `toolr_wheel` + `toolr_py_wheel` fixtures from `conftest.py`
(parametrize over wheels in `wheelhouse/`) and on the `make_uv_venv`
fixture (creates a uv venv at a caller-supplied path and resolves
OS-correct interpreter / script paths).
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path
from zipfile import ZipFile

import pytest


@pytest.mark.distribution
def test_example_plugin_wheel_ships_manifest_and_commands(
    toolr_wheel: Path,
    toolr_py_wheel: Path,
    example_plugin_wheel: Path,
    make_uv_venv,
    tmp_path: Path,
) -> None:
    uv = shutil.which("uv")
    if uv is None:
        pytest.skip("uv required to install the example wheel")

    # ---- The example wheel is supplied by the session-scoped
    #      `example_plugin_wheel` fixture: CI builds it once (universal
    #      `py3-none-any`) and ships it via `wheelhouse/`; locally the
    #      fixture falls back to an inline `uv build` so a bare
    #      `pytest tests/distribution/` still works.
    example_wheel = example_plugin_wheel

    # ---- The wheel must actually carry the manifest. If hatchling's
    #      `packages = [...]` rule ever stops including non-`.py` files,
    #      this is the test that catches it.
    with ZipFile(example_wheel) as zf:
        names = set(zf.namelist())
    assert "toolr_example_plugin/toolr-manifest.json" in names, (
        f"wheel does not ship the manifest -- packaging-contract violation. Wheel contents:\n{sorted(names)}"
    )

    # ---- Lay out a minimal project with its own tools venv. The venv
    #      MUST live at `<project>/tools/.venv/` because toolr's in-tree
    #      discovery only finds it there.
    project = tmp_path / "project"
    tools = project / "tools"
    tools.mkdir(parents=True)
    (tools / "pyproject.toml").write_text(
        '[project]\nname = "tools"\nversion = "0.0.0"\n[tool.toolr]\nvenv-location = "in-tree"\n',
    )
    venv = make_uv_venv(tools / ".venv")

    # ---- Install the example wheel alongside the pre-built toolr +
    #      toolr-py wheels from `wheelhouse/`. The example's pyproject
    #      declares `toolr-py` as a dep; pip resolves it against the
    #      local wheel we pass explicitly so the test never reaches PyPI
    #      (where toolr-py for this dev version doesn't exist).
    subprocess.run(  # noqa: S603
        [
            uv,
            "pip",
            "install",
            "--python",
            str(venv.python),
            str(example_wheel),
            str(toolr_wheel),
            str(toolr_py_wheel),
        ],
        check=True,
    )

    # ---- Run `toolr --help` in the project using the freshly installed
    #      CLI binary from the venv. The dispatch-time glob should pick
    #      up the example plugin's manifest from site-packages.
    assert venv.toolr.is_file(), f"toolr binary not installed at {venv.toolr}"
    env = os.environ.copy()
    env["TOOLR_NO_CACHE_HINT"] = "1"
    result = subprocess.run(  # noqa: S603
        [str(venv.toolr), "--help"],
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, (
        f"toolr --help failed: returncode={result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    assert "third-party" in result.stdout, (
        f"expected `third-party` group in --help; got:\n{result.stdout}"
    )
    assert "utils" in result.stdout, f"expected `utils` group in --help; got:\n{result.stdout}"

    # ---- Run one of the example commands end-to-end. This proves the
    #      manifest's module + function fields resolve to real callables
    #      inside the installed wheel.
    result = subprocess.run(  # noqa: S603
        [str(venv.toolr), "third-party", "hello", "--name", "ContractTest"],
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, (
        f"toolr third-party hello failed: returncode={result.returncode}\n"
        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    assert "Hello, ContractTest from toolr-plugin-example!" in result.stdout, (
        f"unexpected command output:\n{result.stdout}"
    )
