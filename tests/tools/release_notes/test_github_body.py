"""Tests for ``toolr release-notes github-body``, against the real ``cliff.toml`` and ``git-cliff``."""

from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.release_notes import GITHUB_BODY_MAX_CHARS
from tools.release_notes import github_body

from tests._git import Git

REPO_ROOT = Path(__file__).resolve().parents[3]
CHANGELOG_URL = "https://github.com/o/r/blob/v1.2.0/CHANGELOG.md"


@pytest.fixture(autouse=True)
def _git_cliff_offline(monkeypatch: pytest.MonkeyPatch) -> None:
    if shutil.which("git-cliff") is None:
        # mise.toml pins git-cliff, so CI always has it; only a local run may not.
        if os.environ.get("CI"):
            pytest.fail("git-cliff is not on PATH; it is pinned in mise.toml")
        pytest.skip("git-cliff not available")
    monkeypatch.setenv("GIT_CLIFF_OFFLINE", "true")


@pytest.fixture
def release_repo(tmp_path: Path, git: Git) -> Callable[..., Path]:
    """Factory: a repository with ``cliff.toml``, one queued note and a few commits.

    ``tags`` maps an annotated tag to the index of the commit it marks.
    """

    def make(note: str, tags: dict[str, int] | None = None) -> Path:
        return git.make_repo(
            tmp_path / "repo",
            commits=["feat: add a widget", "docs: explain the widget", "fix: mend the widget"],
            tags=tags,
            files={
                "cliff.toml": (REPO_ROOT / "cliff.toml").read_text(encoding="utf-8"),
                "release-notes/2026-11-01.14-03-27.widget.md": note,
            },
        )

    return make


def test_body_is_notes_and_link(
    release_repo: Callable[..., Path], tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    summary = tmp_path / "step-summary.md"
    monkeypatch.setenv("GITHUB_STEP_SUMMARY", str(summary))
    repo = release_repo("A widget now exists.\n")
    output = tmp_path / "body.md"
    rendered = tmp_path / "rendered.md"
    github_body(make_context(repo), "1.2.0", "o/r", str(output), rendered=str(rendered))

    body = output.read_text(encoding="utf-8")
    assert body.startswith("### Notes\n\nA widget now exists.\n")
    assert f"[the full list of changes]({CHANGELOG_URL})" in body
    assert "Add a widget" not in body
    assert "## Unreleased" not in body
    assert not summary.exists()
    # `clear --rendered` reads this file and needs the heading.
    assert "### Notes" in rendered.read_text(encoding="utf-8").splitlines()


@pytest.mark.parametrize(
    ("tags", "link"),
    [
        (None, "https://github.com/o/r/commits/v1.2.0-rc.1"),
        (
            {"v1.0.0": 0, "v1": 1, "v1.1.0-rc.1": 1},
            "https://github.com/o/r/compare/v1.0.0...v1.2.0-rc.1",
        ),
    ],
    ids=["no-final-release", "compares-last-final-release"],
)
def test_prerelease_link(
    release_repo: Callable[..., Path],
    tmp_path: Path,
    tags: dict[str, int] | None,
    link: str,
) -> None:
    repo = release_repo("A widget now exists.\n", tags=tags)
    output = tmp_path / "body.md"
    github_body(
        make_context(repo),
        "1.2.0-rc.1",
        "o/r",
        str(output),
        rendered=str(tmp_path / "rendered.md"),
        prerelease=True,
    )
    assert f"[the full list of changes]({link})" in output.read_text(encoding="utf-8")


@pytest.mark.parametrize(
    "note",
    [
        "word " * (GITHUB_BODY_MAX_CHARS // 5),
        "\N{GRINNING FACE}" * (GITHUB_BODY_MAX_CHARS // 2 + 1),
    ],
    ids=["ascii", "emoji-counts-two-utf16-units"],
)
def test_too_long_body_is_only_the_link(
    release_repo: Callable[..., Path],
    tmp_path: Path,
    note: str,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    summary = tmp_path / "step-summary.md"
    monkeypatch.setenv("GITHUB_ACTIONS", "true")
    monkeypatch.setenv("GITHUB_STEP_SUMMARY", str(summary))
    repo = release_repo(note + "\n")
    output = tmp_path / "body.md"
    github_body(
        make_context(repo), "1.2.0", "o/r", str(output), rendered=str(tmp_path / "rendered.md")
    )
    assert output.read_text(encoding="utf-8") == (
        f"The release notes are too long for a GitHub release. See [the full list of changes]({CHANGELOG_URL}).\n"
    )
    warning = f"The release notes are over {GITHUB_BODY_MAX_CHARS} characters; the release body is only a link"
    assert f"::warning title=Release body::{warning}\n" in capsys.readouterr().out
    assert summary.read_text(encoding="utf-8") == f"> [!WARNING]\n> {warning}\n"


def test_git_cliff_failure_exits(git: Git, tmp_path: Path) -> None:
    repo = git.make_repo(
        tmp_path / "repo", commits=["feat: add a widget"], files={"cliff.toml": "[changelog\n"}
    )
    output = tmp_path / "body.md"
    with pytest.raises(SystemExit) as exc:
        github_body(
            make_context(repo), "1.2.0", "o/r", str(output), rendered=str(tmp_path / "r.md")
        )
    assert exc.value.code != 0
    assert not output.exists()


def test_changelog_render_still_lists_commits(release_repo: Callable[..., Path]) -> None:
    """Without `TOOLR_FULL_CHANGELOG_URL`, as `Update CHANGELOG.md` runs it, the commits are listed."""
    repo = release_repo("A widget now exists.\n")
    changelog = subprocess.run(  # git-cliff binary + args are test-controlled
        ["git-cliff", "--config", "cliff.toml", "--unreleased", "--tag", "1.2.0"],  # noqa: S607
        cwd=repo,
        capture_output=True,
        encoding="utf-8",
        check=True,
    ).stdout
    assert "- Add a widget" in changelog
    assert "- Mend the widget" in changelog
    assert "full list of changes" not in changelog
    # The footer marker must stay on its own line.
    assert changelog.endswith(")\n<!-- generated by git-cliff -->\n")
