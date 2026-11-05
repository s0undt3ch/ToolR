"""Fixtures for runner tests, which import throwaway ``tools`` packages."""

from __future__ import annotations

import sys

import pytest

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
