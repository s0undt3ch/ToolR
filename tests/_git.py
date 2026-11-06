"""Helpers for tests that build real git repositories in `tmp_path`."""

from __future__ import annotations

import dataclasses
import os
import subprocess
from collections.abc import Mapping
from collections.abc import Sequence
from pathlib import Path


@dataclasses.dataclass(frozen=True)
class Git:
    """Runs `git` with the developer's config ignored. Tests get one from the `git` fixture."""

    binary: str

    @staticmethod
    def env() -> dict[str, str]:
        """The process environment, minus the developer's global and system git config.

        A staticmethod so subprocesses that may run git, such as workflow steps, can use it
        without depending on the `git` fixture.
        """
        env = dict(os.environ)
        env["GIT_CONFIG_GLOBAL"] = os.devnull
        env["GIT_CONFIG_NOSYSTEM"] = "1"
        return env

    def __call__(self, repo: Path, *args: str) -> str:
        """Run `git` in `repo` and return its stdout."""
        return subprocess.run(  # noqa: S603  # git binary + args are test-controlled
            [self.binary, *args],
            cwd=repo,
            env=self.env(),
            capture_output=True,
            encoding="utf-8",
            check=True,
        ).stdout

    def configure_identity(self, repo: Path) -> None:
        """Give `repo` a committer identity and turn off commit and tag signing."""
        self(repo, "config", "user.name", "Release Bot")
        self(repo, "config", "user.email", "release-bot@example.com")
        self(repo, "config", "commit.gpgsign", "false")
        self(repo, "config", "tag.gpgsign", "false")

    def commit(
        self, repo: Path, message: str, files: Mapping[str, str | None] | None = None
    ) -> None:
        """Write and stage `files`, then commit. A `None` content deletes that file."""
        for name, text in (files or {}).items():
            if text is None:
                self(repo, "rm", "-q", "--", name)
                continue
            path = repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
            self(repo, "add", "--", name)
        self(repo, "commit", "--allow-empty", "-q", "-m", message)

    def make_repo(
        self,
        path: Path,
        commits: Sequence[str] = ("initial",),
        tags: Mapping[str, int] | None = None,
        files: Mapping[str, str] | None = None,
    ) -> Path:
        """Create a git repository at `path` and return it.

        `files` are written and committed by the first commit; the other commits are
        empty. `tags` maps an annotated tag name to the index of the commit it marks.
        """
        path.mkdir()
        self(path, "init", "-q", "-b", "main")
        self.configure_identity(path)
        tags_by_commit: dict[int, list[str]] = {}
        for tag, index in (tags or {}).items():
            tags_by_commit.setdefault(index, []).append(tag)
        for index, message in enumerate(commits):
            self.commit(path, message, files if index == 0 else None)
            for tag in tags_by_commit.get(index, []):
                self(path, "tag", "-a", "-m", f"Release {tag}", tag)
        return path
