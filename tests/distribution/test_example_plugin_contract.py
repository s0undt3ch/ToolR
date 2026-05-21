"""Packaging-contract test for `examples/plugin-package/`.

This proves the third-party plugin contract end-to-end:

1. A hatchling-built wheel of `toolr-plugin-example` actually ships
   `toolr_example_plugin/toolr-manifest.json` inside the wheel.
2. After installing that wheel into a project's tools venv, the
   commands declared by the example appear in `toolr --help` -- proving
   the dispatch-time glob + merge picks them up.
3. Running one of those commands produces the expected output -- proving
   the manifest's `module` + `function` fields resolve correctly.

The test is marked `distribution` (opt-in, slow). It is the canonical
regression guard for the third-party plugin contract documented in
`docs/third-party.md`.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path
from zipfile import ZipFile

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
EXAMPLE_DIR = REPO_ROOT / "examples" / "plugin-package"


def _resolve_toolr_binary() -> Path:
    """Locate the workspace-built toolr binary.

    Prefers `target/debug/toolr` so the test exercises the current
    source tree; falls back to release if debug isn't built yet.
    Skips the test if neither exists -- the maintainer must `cargo
    build -p toolr` first.
    """
    for variant in ("debug", "release"):
        candidate = REPO_ROOT / "target" / variant / "toolr"
        if candidate.is_file():
            return candidate
    pytest.skip(
        "no workspace-built toolr binary at target/{debug,release}/toolr; run `cargo build -p toolr` first",
    )
    msg = "unreachable: pytest.skip should have aborted"
    raise AssertionError(msg)


@pytest.mark.distribution
def test_example_plugin_wheel_ships_manifest_and_commands(tmp_path: Path) -> None:
    uv = shutil.which("uv")
    if uv is None:
        pytest.skip("uv required to build + install the example wheel")

    toolr_bin = _resolve_toolr_binary()

    # ---- Build a wheel from examples/plugin-package/.
    wheel_dir = tmp_path / "dist"
    subprocess.run(  # noqa: S603
        [uv, "build", "--wheel", "--out-dir", str(wheel_dir), str(EXAMPLE_DIR)],
        check=True,
        cwd=tmp_path,
    )
    wheels = sorted(wheel_dir.glob("toolr_plugin_example-*.whl"))
    assert len(wheels) == 1, f"expected exactly one example wheel, got: {wheels}"
    wheel = wheels[0]

    # ---- The wheel must actually carry the manifest. If hatchling's
    #      `packages = [...]` rule ever stops including non-`.py` files,
    #      this is the test that catches it.
    with ZipFile(wheel) as zf:
        names = set(zf.namelist())
    assert "toolr_example_plugin/toolr-manifest.json" in names, (
        f"wheel does not ship the manifest -- packaging-contract violation. Wheel contents:\n{sorted(names)}"
    )

    # ---- Lay out a minimal project with its own tools venv.
    project = tmp_path / "project"
    tools = project / "tools"
    tools.mkdir(parents=True)
    (tools / "pyproject.toml").write_text(
        '[project]\nname = "tools"\nversion = "0.0.0"\n[tool.toolr]\nvenv-location = "in-tree"\n',
    )
    venv_dir = tools / ".venv"
    subprocess.run(  # noqa: S603
        [uv, "venv", "--python", sys.executable, str(venv_dir)],
        check=True,
    )

    # ---- Install the example wheel into that venv. The wheel pulls in
    #      `toolr-py` as a dep, which we satisfy from the same workspace
    #      so the test doesn't depend on PyPI.
    venv_python = venv_dir / "bin" / "python"
    subprocess.run(  # noqa: S603
        [
            uv,
            "pip",
            "install",
            "--python",
            str(venv_python),
            str(wheel),
            "toolr-py",
        ],
        check=True,
    )

    # ---- Run `toolr --help` in the project. The dispatch-time glob
    #      should pick up the plugin's manifest from the freshly
    #      populated site-packages.
    env = os.environ.copy()
    env["TOOLR_NO_CACHE_HINT"] = "1"
    result = subprocess.run(  # noqa: S603
        [str(toolr_bin), "--help"],
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, (
        f"toolr --help failed: returncode={result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )
    assert "third-party" in result.stdout, f"expected `third-party` group in --help; got:\n{result.stdout}"
    assert "utils" in result.stdout, f"expected `utils` group in --help; got:\n{result.stdout}"

    # ---- Run one of the example commands end-to-end. This proves the
    #      manifest's module + function fields resolve to real callables
    #      inside the installed wheel.
    result = subprocess.run(  # noqa: S603
        [str(toolr_bin), "third-party", "hello", "--name", "ContractTest"],
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
