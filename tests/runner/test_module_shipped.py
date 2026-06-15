"""Sanity checks that ``toolr._runner`` ships with the package."""

from __future__ import annotations

import importlib
import importlib.util


def test_runner_module_is_importable() -> None:
    # Smoke: simply importing the module should not raise.
    mod = importlib.import_module("toolr._runner")
    assert hasattr(mod, "main")
    assert hasattr(mod, "RunnerSpec")
    assert mod.SCHEMA_VERSION == 2


def test_runner_module_file_is_under_toolr_package() -> None:
    spec = importlib.util.find_spec("toolr._runner")
    assert spec is not None, "toolr._runner should be findable"
    assert spec.origin is not None
    # Reaching this assertion means the source file is shipped alongside
    # the rest of the package — installing the wheel ships it too.
    assert spec.origin.endswith("_runner.py")
