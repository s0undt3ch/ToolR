"""Tests for the `run-full-ci` decision in ``toolr ci generate-build-matrix``."""

from __future__ import annotations

import json
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.ci import FULL_BUILD_LABEL
from tools.ci import _run_full_ci
from tools.ci import generate_build_matrix

from tests._git import git
from tests._git import make_repo

pytestmark = pytest.mark.skip_if_binaries_missing("git")

PrRepoFactory = Callable[..., tuple[Path, dict[str, str]]]
EventFactory = Callable[..., None]


@pytest.fixture
def pr_repo(tmp_path: Path) -> PrRepoFactory:
    """Factory: a git repo with a base commit and a head commit changing `changed`."""

    def factory(changed: list[str]) -> tuple[Path, dict[str, str]]:
        repo = make_repo(tmp_path / "repo", files={"README.md": "base\n"})
        base = git(repo, "rev-parse", "HEAD").strip()
        for name in changed:
            path = repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("head\n")
        if changed:
            git(repo, "add", "--", *changed)
        git(repo, "commit", "-q", "--allow-empty", "-m", "head")
        head = git(repo, "rev-parse", "HEAD").strip()
        return repo, {"base": base, "head": head}

    return factory


@pytest.fixture
def pr_event(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> EventFactory:
    """Factory: point the GitHub env vars at a `pull_request` event payload."""

    def factory(
        shas: dict[str, str] | None = None,
        labels: tuple[str, ...] = (),
        event_name: str = "pull_request",
    ) -> None:
        pr: dict[str, object] = {"labels": [{"name": label} for label in labels]}
        for side, sha in (shas or {}).items():
            pr[side] = {"sha": sha}
        event_path = tmp_path / "event.json"
        event_path.write_text(json.dumps({"pull_request": pr}))
        monkeypatch.setenv("GITHUB_EVENT_NAME", event_name)
        monkeypatch.setenv("GITHUB_EVENT_PATH", str(event_path))

    return factory


@pytest.mark.parametrize(
    "changed",
    [
        pytest.param(["specs/2026-11-06-foo-design.md"], id="specs"),
        pytest.param(
            [
                "specs/archive/2026/2026-11-06-empty-xdg-data-home-plan.md",
                "specs/archive/2026/2026-11-06-group-binding-forms-plan.md",
            ],
            id="specs-archive-pr-697",
        ),
        pytest.param([".claude/skills/monitor-pr/SKILL.md"], id="claude-dir"),
        pytest.param(
            ["CLAUDE.md", "SECURITY.md", "renovate.json5", "typos.toml", ".rumdl.toml"],
            id="inert-files",
        ),
    ],
)
def test_inert_only_pr_skips_full_ci(
    pr_repo: PrRepoFactory, pr_event: EventFactory, changed: list[str]
) -> None:
    repo, shas = pr_repo(changed)
    pr_event(shas)
    run, reason = _run_full_ci(make_context(repo))
    assert run is False
    assert FULL_BUILD_LABEL in reason


@pytest.mark.parametrize(
    "changed",
    [
        pytest.param(["specs/foo.md", "tools/ci.py"], id="specs-plus-code"),
        pytest.param(["README.md"], id="readme-is-wheel-metadata"),
        pytest.param(["docs/CLAUDE.md"], id="inert-name-in-other-dir"),
        pytest.param(["specsheet.md"], id="prefix-without-slash"),
        pytest.param([], id="empty-diff"),
    ],
)
def test_pr_touching_built_paths_runs_full_ci(
    pr_repo: PrRepoFactory, pr_event: EventFactory, changed: list[str]
) -> None:
    repo, shas = pr_repo(changed)
    pr_event(shas)
    run, _ = _run_full_ci(make_context(repo))
    assert run is True


def test_full_build_label_forces_full_ci(pr_repo: PrRepoFactory, pr_event: EventFactory) -> None:
    repo, shas = pr_repo(["specs/foo.md"])
    pr_event(shas, labels=(FULL_BUILD_LABEL,))
    run, reason = _run_full_ci(make_context(repo))
    assert run is True
    assert FULL_BUILD_LABEL in reason


def test_push_runs_full_ci(pr_repo: PrRepoFactory, pr_event: EventFactory) -> None:
    repo, shas = pr_repo(["specs/foo.md"])
    pr_event(shas, event_name="push")
    run, reason = _run_full_ci(make_context(repo))
    assert run is True
    assert "push" in reason


@pytest.mark.parametrize("missing", ["base", "head"])
def test_missing_sha_fails_open(
    pr_repo: PrRepoFactory, pr_event: EventFactory, missing: str
) -> None:
    repo, shas = pr_repo(["specs/foo.md"])
    del shas[missing]
    pr_event(shas)
    run, _ = _run_full_ci(make_context(repo))
    assert run is True


def test_unknown_sha_fails_open(pr_repo: PrRepoFactory, pr_event: EventFactory) -> None:
    repo, shas = pr_repo(["specs/foo.md"])
    pr_event({"base": shas["base"], "head": "0" * 40})
    run, reason = _run_full_ci(make_context(repo))
    assert run is True
    assert "couldn't list" in reason


@pytest.mark.parametrize(
    "payload",
    [
        pytest.param("[]", id="not-an-object"),
        pytest.param("{not json", id="invalid-json"),
        pytest.param(None, id="missing-file"),
    ],
)
def test_unreadable_event_payload_fails_open(
    pr_repo: PrRepoFactory,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    payload: str | None,
) -> None:
    repo, _ = pr_repo(["specs/foo.md"])
    event_path = tmp_path / "event.json"
    if payload is not None:
        event_path.write_text(payload)
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    monkeypatch.setenv("GITHUB_EVENT_PATH", str(event_path))
    run, _ = _run_full_ci(make_context(repo))
    assert run is True


def test_unset_event_path_fails_open(
    pr_repo: PrRepoFactory, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo, _ = pr_repo(["specs/foo.md"])
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    monkeypatch.delenv("GITHUB_EVENT_PATH", raising=False)
    run, _ = _run_full_ci(make_context(repo))
    assert run is True


def test_generate_build_matrix_emits_run_full_ci(
    pr_repo: PrRepoFactory,
    pr_event: EventFactory,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    repo, shas = pr_repo(["specs/foo.md"])
    pr_event(shas)
    output = tmp_path / "github_output"
    summary = tmp_path / "step_summary"
    monkeypatch.setenv("GITHUB_OUTPUT", str(output))
    monkeypatch.setenv("GITHUB_STEP_SUMMARY", str(summary))

    generate_build_matrix(make_context(repo))

    assert "run-full-ci=false\n" in output.read_text()
    assert "Full CI: `false`" in summary.read_text()
