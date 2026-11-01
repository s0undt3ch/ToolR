"""Queued release notes: one markdown file per note under ``release-notes/``."""

from __future__ import annotations

import re
import secrets
from datetime import UTC
from datetime import datetime
from pathlib import Path
from typing import Final

from toolr import Context
from toolr import command_group

NOTES_DIR: Final[str] = "release-notes"
README_NAME: Final[str] = "README.md"
NOTE_NAME_RE: Final[re.Pattern[str]] = re.compile(
    r"^(?P<date>\d{4}-\d{2}-\d{2})\.(?P<time>\d{2}-\d{2}-\d{2})\.(?P<slug>[a-z0-9]+(?:-[a-z0-9]+)*)\.md$"
)
SLUG_RE: Final[re.Pattern[str]] = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
BREAKING_PREFIX: Final[str] = "**Breaking:**"
HEADING_RE: Final[re.Pattern[str]] = re.compile(r"^#{1,6}(\s|$)")
FENCE_RE: Final[re.Pattern[str]] = re.compile(r"^\s*(```|~~~)")

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


def _slug_from_branch(branch: str) -> str | None:
    """Derive a note slug from a branch name, or ``None`` if nothing usable is left."""
    if branch in {"HEAD", "main"}:
        return None
    name = re.sub(r"^[^/]+/", "", branch, count=1).lower()
    return re.sub(r"[^a-z0-9]+", "-", name).strip("-") or None


def _note_filename(now: datetime, slug: str) -> str:
    return f"{now:%Y-%m-%d}.{now:%H-%M-%S}.{slug}.md"


def _find_by_slug(notes_dir: Path, slug: str) -> Path | None:
    for path in _note_paths(notes_dir):
        match = NOTE_NAME_RE.match(path.name)
        if match and match["slug"] == slug:
            return path
    return None


def _github_env_block(name: str, value: str) -> str:
    """Format ``name=value`` as a ``$GITHUB_ENV`` heredoc block."""
    delimiter = f"{name}_{secrets.token_hex(16)}"
    while delimiter in value:
        delimiter = f"{name}_{secrets.token_hex(16)}"
    return f"{name}<<{delimiter}\n{value}\n{delimiter}\n"


def _has_heading(text: str) -> bool:
    in_fence = False
    for line in text.splitlines():
        if FENCE_RE.match(line):
            in_fence = not in_fence
        elif not in_fence and HEADING_RE.match(line):
            return True
    return False


def _file_problems(repo_root: Path) -> list[str]:
    """Return one line per structural problem in the queued notes."""
    problems: list[str] = []
    if (repo_root / "UNRELEASED.md").exists():
        problems.append(
            "UNRELEASED.md is no longer used: move its content into a note with `toolr release-notes add`"
        )
    for path in _note_paths(repo_root / NOTES_DIR):
        label = f"{NOTES_DIR}/{path.name}"
        if path.is_dir():
            problems.append(f"{label}: subdirectories are not allowed")
            continue
        match = NOTE_NAME_RE.match(path.name)
        if match is None:
            problems.append(f"{label}: name must be <YYYY-MM-DD>.<HH-MM-SS>.<slug>.md")
            continue
        try:
            datetime.strptime(f"{match['date']} {match['time']}", "%Y-%m-%d %H-%M-%S")  # noqa: DTZ007
        except ValueError:
            problems.append(f"{label}: timestamp is not a valid date and time")
            continue
        text = _read_note(path)
        if not text:
            problems.append(f"{label}: note is empty")
        elif _has_heading(text):
            problems.append(f"{label}: notes must not contain markdown headings")
    return problems


def _exit_on_problems(ctx: Context, problems: list[str]) -> None:
    if problems:
        for problem in problems:
            ctx.error(problem)
        ctx.exit(1)


@group.command
def check(ctx: Context, base: str | None = None) -> None:
    """Validate the queued notes.

    Args:
        base: Git ref to compare against. Reserved for the pull-request gate.
    """
    _exit_on_problems(ctx, _file_problems(ctx.repo_root))


@group.command
def render(ctx: Context, github_env: str | None = None) -> None:
    """Print the queued notes, breaking ones first.

    Args:
        github_env: Append the notes to this `$GITHUB_ENV` file as `TOOLR_RELEASE_NOTES` instead of printing.
    """
    _exit_on_problems(ctx, _file_problems(ctx.repo_root))
    text = _render(ctx.repo_root / NOTES_DIR)
    if github_env:
        with Path(github_env).open("a", encoding="utf-8") as f:
            f.write(_github_env_block("TOOLR_RELEASE_NOTES", text))
        return
    ctx.print(text)


@group.command
def add(
    ctx: Context, slug: str | None = None, text: str | None = None, append: bool = False
) -> None:
    """Create a release note and stage it.

    Args:
        slug: Note slug. Defaults to one derived from the current branch name.
        text: Note content. Omit to create an empty file to edit.
        append: Append `--text` to the existing note with this slug instead of refusing.
    """
    if append and text is None:
        ctx.error("--append needs --text")
        ctx.exit(1)
    if slug is None:
        ret = ctx.run(
            "git",
            "-C",
            str(ctx.repo_root),
            "branch",
            "--show-current",
            capture_output=True,
            stream_output=False,
        )
        branch: str = ret.stdout.read().strip()  # type: ignore[assignment]
        slug = _slug_from_branch(branch)
        if slug is None:
            ctx.error("Cannot derive a slug from the current branch; pass --slug")
            ctx.exit(1)
    if not SLUG_RE.match(slug):
        ctx.error(f"Invalid slug {slug!r}: use lowercase letters, digits and single hyphens")
        ctx.exit(1)
    notes_dir = ctx.repo_root / NOTES_DIR
    existing = _find_by_slug(notes_dir, slug)
    if append:
        if existing is None:
            ctx.error(f"No note with slug {slug!r} to append to")
            ctx.exit(1)
        path = existing
        previous = _read_note(path)
        path.write_bytes(f"{previous}\n\n{text}\n".encode())
    else:
        if existing is not None:
            ctx.error(f"A note with slug {slug!r} already exists: {existing.name}; use --append")
            ctx.exit(1)
        notes_dir.mkdir(exist_ok=True)
        path = notes_dir / _note_filename(datetime.now(UTC), slug)
        path.write_bytes(f"{text}\n".encode() if text is not None else b"")
    rel = path.relative_to(ctx.repo_root)
    ret = ctx.run(
        "git",
        "-C",
        str(ctx.repo_root),
        "add",
        "--",
        str(rel),
        capture_output=True,
        stream_output=False,
    )
    if ret.returncode:
        ctx.error(f"git add failed with exit code {ret.returncode}")
        ctx.exit(ret.returncode)
    ctx.print(str(rel))


@group.command
def clear(ctx: Context, rendered: str | None = None) -> None:
    """Delete every queued note, keeping the README.

    Args:
        rendered: Rendered changelog. Refuse to delete notes if it has no `### Notes` line.
    """
    paths = _note_paths(ctx.repo_root / NOTES_DIR)
    if rendered and paths:
        lines = Path(rendered).read_text(encoding="utf-8").splitlines()
        if not any(line.strip() == "### Notes" for line in lines):
            ctx.error(
                f"{rendered} has no '### Notes' section; refusing to delete {len(paths)} queued note(s)"
            )
            ctx.exit(1)
    for path in paths:
        path.unlink()
