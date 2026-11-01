"""Queued release notes: one markdown file per note under ``release-notes/``."""

from __future__ import annotations

import re
import secrets
from pathlib import Path
from typing import Final

from toolr import Context
from toolr import command_group

NOTES_DIR: Final[str] = "release-notes"
README_NAME: Final[str] = "README.md"
NOTE_NAME_RE: Final[re.Pattern[str]] = re.compile(
    r"^(?P<date>\d{4}-\d{2}-\d{2})\.(?P<time>\d{2}-\d{2}-\d{2})\.(?P<slug>[a-z0-9]+(?:-[a-z0-9]+)*)\.md$"
)
BREAKING_PREFIX: Final[str] = "**Breaking:**"

group = command_group("release-notes", "Manage queued release notes", docstring=__doc__)


def _note_paths(notes_dir: Path) -> list[Path]:
    """Return every entry in ``notes_dir`` except the README, sorted by name."""
    if not notes_dir.is_dir():
        return []
    return sorted((p for p in notes_dir.iterdir() if p.name != README_NAME), key=lambda p: p.name)


def _read_note(path: Path) -> str:
    return path.read_bytes().decode("utf-8").replace("\r\n", "\n").strip()


def _render(notes_dir: Path) -> str:
    """Join notes oldest-first, with breaking notes moved to the top."""
    notes = [_read_note(p) for p in _note_paths(notes_dir)]
    breaking = [n for n in notes if n.startswith(BREAKING_PREFIX)]
    other = [n for n in notes if not n.startswith(BREAKING_PREFIX)]
    return "\n\n".join(breaking + other)


def _github_env_block(name: str, value: str) -> str:
    """Format ``name=value`` as a ``$GITHUB_ENV`` heredoc block."""
    delimiter = f"{name}_{secrets.token_hex(16)}"
    while delimiter in value:
        delimiter = f"{name}_{secrets.token_hex(16)}"
    return f"{name}<<{delimiter}\n{value}\n{delimiter}\n"


@group.command
def render(ctx: Context, github_env: str | None = None) -> None:
    """Print the queued notes, breaking ones first.

    Args:
        github_env: Append the notes to this `$GITHUB_ENV` file as `TOOLR_RELEASE_NOTES` instead of printing.
    """
    text = _render(ctx.repo_root / NOTES_DIR)
    if github_env:
        with Path(github_env).open("a", encoding="utf-8") as f:
            f.write(_github_env_block("TOOLR_RELEASE_NOTES", text))
        return
    ctx.print(text)
