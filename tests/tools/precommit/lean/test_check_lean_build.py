"""Tests for ``toolr pre-commit check-lean-build``."""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.precommit.lean import CLIPPY_LEAN_ARGS
from tools.precommit.lean import LEAN_TREE_ARGS
from tools.precommit.lean import check_lean_build
from tools.precommit.lean import direct_deps

from tests._paths import REPO_ROOT
from tests.tools.precommit.recording_run import RecordingRun

ROOT_LINE = "toolr-core v0.37.0 (~/toolr/crates/toolr-core)\n"
ALLOWED_TREE = ROOT_LINE + "libc v0.2.191\nserde v1.0.229\nwinapi v0.3.9\n"


@pytest.mark.skip_if_binaries_missing("cargo")
def test_repo_lean_build_matches_the_allowlist() -> None:
    """Run the real ``cargo tree`` against this workspace; clippy is stubbed, it takes minutes."""
    real = make_context(REPO_ROOT)
    ret = real.run(
        shutil.which("cargo") or "cargo", *LEAN_TREE_ARGS, capture_output=True, stream_output=False
    )
    assert ret.returncode == 0
    run = RecordingRun((ret.stdout.read(), "", 0), ("", "", 0), ("", "", 0))  # type: ignore[arg-type]
    check_lean_build(make_context(REPO_ROOT, run=run))
    assert run.args == [LEAN_TREE_ARGS, *CLIPPY_LEAN_ARGS]


@pytest.mark.skip_if_binaries_missing("cargo")
def test_runs_the_cargo_on_path() -> None:
    run = RecordingRun((ALLOWED_TREE, "", 0), ("", "", 0), ("", "", 0))
    check_lean_build(make_context(REPO_ROOT, run=run))
    assert {cmdline[0] for cmdline in run.calls} == {shutil.which("cargo")}


def test_clippy_lints_lean_core_then_toolr_py() -> None:
    core, py = CLIPPY_LEAN_ARGS
    assert core[:3] == ("clippy", "-p", "toolr-core")
    assert "--no-default-features" in core
    assert py[:3] == ("clippy", "-p", "toolr-py")
    for args in CLIPPY_LEAN_ARGS:
        assert args[-3:] == ("--", "-D", "warnings")


def test_missing_cargo_is_reported(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setenv("PATH", str(tmp_path))
    run = RecordingRun()
    ctx = make_context(REPO_ROOT, run=run)
    with pytest.raises(SystemExit) as excinfo:
        check_lean_build(ctx)
    assert excinfo.value.code == 1
    assert "`cargo` not found on PATH" in ctx.stderr
    assert run.calls == []


def test_direct_deps_skips_the_root_and_dedupes() -> None:
    tree = ROOT_LINE + "serde v1.0.229\nlibc v0.2.191\nserde v1.0.229\n\nwinapi v0.3.9\n"
    assert direct_deps(tree) == ["libc", "serde", "winapi"]


def test_direct_deps_of_a_crate_without_dependencies() -> None:
    assert direct_deps(ROOT_LINE) == []


@pytest.mark.skip_if_binaries_missing("cargo")
@pytest.mark.parametrize(
    ("tree", "found"),
    [
        (
            ROOT_LINE + "libc v0.2.191\nregex v1.11.0\nserde v1.0.229\nwinapi v0.3.9\n",
            "libc regex serde winapi",
        ),
        (ROOT_LINE + "libc v0.2.191\nserde v1.0.229\n", "libc serde"),
    ],
    ids=["unlisted", "missing"],
)
def test_rejects_deps_off_the_allowlist(tree: str, found: str) -> None:
    run = RecordingRun((tree, "", 0))
    ctx = make_context(REPO_ROOT, run=run)
    with pytest.raises(SystemExit) as excinfo:
        check_lean_build(ctx)
    assert excinfo.value.code == 1
    assert f"depends on {found!r}, expected 'libc serde winapi'" in ctx.stderr
    assert run.args == [LEAN_TREE_ARGS]


@pytest.mark.skip_if_binaries_missing("cargo")
def test_cargo_tree_failure_is_reported() -> None:
    run = RecordingRun(("", "error: package ID specification `toolr-core` did not match", 101))
    ctx = make_context(REPO_ROOT, run=run)
    with pytest.raises(SystemExit) as excinfo:
        check_lean_build(ctx)
    assert excinfo.value.code == 101
    assert "cargo tree failed: error: package ID specification" in ctx.stderr


@pytest.mark.skip_if_binaries_missing("cargo")
@pytest.mark.parametrize(("results", "code"), [([101], 101), ([0, 2], 2)], ids=["core", "py"])
def test_clippy_failure_stops_with_its_code(results: list[int], code: int) -> None:
    run = RecordingRun((ALLOWED_TREE, "", 0), *(("", "", rc) for rc in results))
    with pytest.raises(SystemExit) as excinfo:
        check_lean_build(make_context(REPO_ROOT, run=run))
    assert excinfo.value.code == code
    assert run.args == [LEAN_TREE_ARGS, *CLIPPY_LEAN_ARGS[: len(results)]]
