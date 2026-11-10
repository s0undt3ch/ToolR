"""The runner defers imports that only some commands need."""

from __future__ import annotations

import subprocess
import sys

import pytest


def _run(code: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, "-c", code], capture_output=True, text=True, check=False)  # noqa: S603


@pytest.mark.parametrize("module", ["importlib.metadata", "packaging.version", "rich.prompt"])
def test_runner_import_leaves_module_unloaded(module: str):
    result = _run(f"import sys, toolr._runner; print({module!r} in sys.modules)")
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "False"


def test_version_attribute_resolves_on_first_access():
    result = _run("import toolr; print(toolr.__version__); print('__version__' in vars(toolr))")
    assert result.returncode == 0, result.stderr
    version, cached = result.stdout.split()
    assert version[0].isdigit()
    assert cached == "True"


def test_unknown_attribute_still_raises():
    result = _run("import toolr; toolr.nope")
    assert result.returncode != 0
    assert "has no attribute 'nope'" in result.stderr


def test_types_version_loads_packaging_on_first_access():
    result = _run(
        "import sys, toolr.types as t; "
        "before = 'packaging.version' in sys.modules; "
        "v = t.Version('1.2'); "
        "print(before, type(v).__module__)"
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.split() == ["False", "packaging.version"]


def test_types_unknown_attribute_still_raises():
    result = _run("import toolr.types; toolr.types.Nope")
    assert result.returncode != 0
    assert "has no attribute 'Nope'" in result.stderr
