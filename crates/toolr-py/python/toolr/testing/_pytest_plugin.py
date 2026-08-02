"""pytest plugin: make ``import tools.*`` resolve under pytest, automatically.

Registered as a ``pytest11`` entry point, so it activates for every project
that depends on ``toolr-py`` — which every ``tools/`` package does — with no
scaffolding step required. Mirrors the append-only ``sys.path`` contract
:func:`toolr._runner._append_repo_root` already uses for real command
dispatch: appended, not prepended, so stdlib and site-packages still win.

Repo root is discovered the same way ``toolr_core::discovery`` finds it for
the CLI: the nearest ancestor of the pytest invocation directory that
contains a ``tools/`` directory.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from toolr._runner import _append_repo_root

_REPO_ROOT_KEY = pytest.StashKey[Path | None]()


def _discover_repo_root(start: Path) -> Path | None:
    """Walk up from `start` for the nearest ancestor containing `tools/`."""
    resolved = start.resolve()
    for candidate in (resolved, *resolved.parents):
        if (candidate / "tools").is_dir():
            return candidate
    return None


def pytest_configure(config: pytest.Config) -> None:
    """Append the discovered repo root to ``sys.path``, once per session."""
    repo_root = _discover_repo_root(Path.cwd())
    config.stash[_REPO_ROOT_KEY] = repo_root
    if repo_root is not None:
        _append_repo_root(str(repo_root))


@pytest.fixture(scope="session")
def repo_root(pytestconfig: pytest.Config) -> Path:
    """The repo root discovered by this plugin (see `_discover_repo_root`)."""
    root = pytestconfig.stash.get(_REPO_ROOT_KEY, None)
    if root is None:
        pytest.fail("toolr: no `tools/` directory found above the pytest run — not a toolr repo?")
    return root
