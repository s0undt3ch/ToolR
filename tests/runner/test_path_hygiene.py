"""SEC-02: runner sys.path hygiene (the one cwd/path concern that stays in Python).

The runner appends ``repo_root`` to ``sys.path`` so ``import tools.*`` resolves
regardless of where toolr was invoked. Append (not prepend) is required so the
stdlib and site-packages win — ``PYTHONPATH`` can't express that ordering, which
is why this lives in the runner rather than on the Rust side.

The other two SEC-02 concerns moved to Rust:

- the ``-P`` flag and the chdir-to-repo_root (``Command::current_dir``) live in
  ``crates/toolr-core/src/execute/spawn.rs``;
- the relative-path warning lives in ``crates/toolr/src/execute_build.rs``
  (``relative_path_warning``), where the dispatch layer knows the cwd, the arg
  types, the values, and which were typed on the command line.

So ``run()`` no longer chdirs or warns; this module only covers the append.
"""

from __future__ import annotations

import os
import sys
import textwrap
from pathlib import Path

import msgspec
import pytest

from toolr._runner import SCHEMA_VERSION
from toolr._runner import RunnerSpec
from toolr._runner import _append_repo_root
from toolr._runner import run


def test_append_repo_root_adds_when_absent():
    path_list = ["/usr/lib/python3.13", "/site-packages"]
    _append_repo_root("/repo", path_list)
    assert path_list[-1] == "/repo"


def test_append_repo_root_is_idempotent():
    path_list = ["/repo"]
    _append_repo_root("/repo", path_list)
    assert path_list == ["/repo"]


def _make_repo(tmp_path: Path) -> Path:
    repo = tmp_path / "repo"
    (repo / "tools").mkdir(parents=True)
    (repo / "tools" / "__init__.py").write_text("")
    (repo / "tools" / "probe.py").write_text(
        textwrap.dedent(
            """
            RAN = {}
            def record(ctx):
                RAN["ok"] = True
            """
        )
    )
    return repo


def _spec(repo: Path, module: str, function: str) -> RunnerSpec:
    payload = {
        "schema_version": SCHEMA_VERSION,
        "group": "probe",
        "command": "record",
        "module": module,
        "function": function,
        "args": {},
        "dispatch": None,
        "context": {
            "repo_root": str(repo),
            "verbosity": "normal",
            "timestamps": False,
            "log_level": "INFO",
            "default_timeout_secs": None,
            "default_no_output_timeout_secs": None,
        },
    }
    return msgspec.convert(payload, type=RunnerSpec)


def test_run_imports_tools_from_a_subdirectory(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    # The runner appends repo_root to sys.path, so `import tools.*` resolves
    # even when toolr is invoked from a subdirectory (resolution no longer
    # relies on the invocation cwd being the repo root). chdir itself is now
    # done by the Rust spawn (`current_dir`), so this in-process test covers
    # only the append; it does not assert cwd.
    repo = _make_repo(tmp_path)
    sub = repo / "tools"
    saved_path = sys.path[:]
    saved_cwd = os.getcwd()
    try:
        monkeypatch.chdir(sub)
        spec = _spec(repo, "tools.probe", "record")
        rc = run(spec)
        assert rc == 0
        # Deferred import is intentional: the `tools` package is created at
        # runtime and only becomes importable after `run()` appends repo_root.
        import tools.probe  # noqa: PLC0415 — created at runtime

        assert tools.probe.RAN.get("ok") is True
    finally:
        sys.path[:] = saved_path
        os.chdir(saved_cwd)
        sys.modules.pop("tools.probe", None)
        sys.modules.pop("tools", None)
