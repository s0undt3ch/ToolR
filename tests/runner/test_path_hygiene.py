"""SEC-02: runner sys.path / cwd hygiene."""

from __future__ import annotations

from toolr._runner import _append_repo_root


def test_append_repo_root_adds_when_absent():
    path_list = ["/usr/lib/python3.13", "/site-packages"]
    _append_repo_root("/repo", path_list)
    assert path_list[-1] == "/repo"


def test_append_repo_root_is_idempotent():
    path_list = ["/repo"]
    _append_repo_root("/repo", path_list)
    assert path_list == ["/repo"]
