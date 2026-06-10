"""SEC-02: runner sys.path / cwd hygiene."""

from __future__ import annotations

import io
from pathlib import Path

from toolr._runner import _append_repo_root
from toolr._runner import _warn_if_paths_relative_to_invocation as _warn


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
