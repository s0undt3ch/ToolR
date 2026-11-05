"""Runner tests import throwaway ``tools`` packages; keep the repo's own out of the way."""

from __future__ import annotations

import sys

import pytest

from toolr._runner import _is_repo_tools_module


@pytest.fixture(autouse=True)
def _isolate_repo_tools(monkeypatch: pytest.MonkeyPatch) -> None:
    """Hide the repo's own ``tools*`` modules and snapshot ``sys.path`` for every test.

    Collection imports the repo's ``tools`` namespace package, which keeps its original search
    path once cached, so a regular ``tools`` package written under ``tmp_path`` would not be found.
    ``monkeypatch`` restores both afterwards.
    """
    monkeypatch.setattr(sys, "path", sys.path[:])
    for name in [m for m in sys.modules if _is_repo_tools_module(m)]:
        monkeypatch.delitem(sys.modules, name)
