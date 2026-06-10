"""SEC-02: runner sys.path / cwd hygiene."""

from __future__ import annotations

import io
import os
import sys
import textwrap
from pathlib import Path

import msgspec
import pytest

from toolr._runner import SCHEMA_VERSION
from toolr._runner import RunnerSpec
from toolr._runner import _append_repo_root
from toolr._runner import _warn_if_paths_relative_to_invocation as _warn
from toolr._runner import run


def test_append_repo_root_adds_when_absent():
    path_list = ["/usr/lib/python3.13", "/site-packages"]
    _append_repo_root("/repo", path_list)
    assert path_list[-1] == "/repo"


def test_append_repo_root_is_idempotent():
    path_list = ["/repo"]
    _append_repo_root("/repo", path_list)
    assert path_list == ["/repo"]


def _run(invocation_cwd, repo_root, values):
    stream = io.StringIO()
    _warn(Path(invocation_cwd), Path(repo_root), values, stream)
    return stream.getvalue()


def test_warns_on_relative_path_arg_from_subdir():
    out = _run("/repo/sub", "/repo", [Path("x.py")])
    assert "repo root" in out
    assert "/repo" in out


def test_no_warn_when_cwd_is_repo_root():
    assert _run("/repo", "/repo", [Path("x.py")]) == ""


def test_no_warn_without_path_args():
    assert _run("/repo/sub", "/repo", ["x.py", 3, True]) == ""


def test_no_warn_for_absolute_path_arg():
    assert _run("/repo/sub", "/repo", [Path("/abs/x.py")]) == ""


def test_warns_for_relative_path_inside_list():
    out = _run("/repo/sub", "/repo", [[Path("a.py"), Path("/abs/b.py")]])
    assert "repo root" in out


def _make_repo(tmp_path: Path) -> Path:
    repo = tmp_path / "repo"
    (repo / "tools").mkdir(parents=True)
    (repo / "tools" / "__init__.py").write_text("")
    (repo / "tools" / "probe.py").write_text(
        textwrap.dedent(
            """
            import os
            CWD_AT_CALL = {}
            def record(ctx):
                CWD_AT_CALL["cwd"] = os.getcwd()
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


def test_run_chdirs_to_repo_root_and_imports_tools_from_subdir(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = _make_repo(tmp_path)
    sub = repo / "tools"  # a subdirectory of the repo
    saved_path = sys.path[:]
    saved_cwd = os.getcwd()
    try:
        monkeypatch.chdir(sub)  # invoke from a subdirectory
        spec = _spec(repo, "tools.probe", "record")
        rc = run(spec)
        assert rc == 0
        # Deferred import is intentional: the `tools` package is created by
        # `_make_repo` at runtime and only becomes importable after `run()`
        # appends repo_root to sys.path — it cannot be a top-level import.
        import tools.probe  # type: ignore[import-not-found]  # noqa: PLC0415 — created at runtime

        assert tools.probe.CWD_AT_CALL["cwd"] == str(repo)
    finally:
        sys.path[:] = saved_path
        os.chdir(saved_cwd)
        sys.modules.pop("tools.probe", None)
        sys.modules.pop("tools", None)
