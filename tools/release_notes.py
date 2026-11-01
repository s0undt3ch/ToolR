"""Queued release notes: one markdown file per note under ``release-notes/``."""

from __future__ import annotations

import re
import secrets
from datetime import UTC
from datetime import datetime
from pathlib import Path
from typing import Final
from typing import NamedTuple

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
FENCE_OPEN_RE: Final[re.Pattern[str]] = re.compile(r"^ {0,3}(?P<fence>`{3,}|~{3,})(?P<info>.*)$")
SUBJECT_RE: Final[re.Pattern[str]] = re.compile(
    r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]*)\))?(?P<bang>!)?:"
)
BREAKING_FOOTER_RE: Final[re.Pattern[str]] = re.compile(r"^BREAKING[ -]CHANGE:", re.MULTILINE)
NOTE_TYPES: Final[frozenset[str]] = frozenset({"feat", "fix", "revert"})

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
    fence: str | None = None
    for line in text.splitlines():
        if fence is None:
            opener = FENCE_OPEN_RE.match(line)
            if opener and not (opener["fence"][0] == "`" and "`" in opener["info"]):
                fence = opener["fence"]
            elif HEADING_RE.match(line):
                return True
        else:
            stripped = line.strip()
            if (
                len(line) - len(line.lstrip(" ")) < 4
                and stripped
                and set(stripped) == {fence[0]}
                and len(stripped) >= len(fence)
            ):
                fence = None
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
        try:
            text = _read_note(path)
        except UnicodeDecodeError:
            problems.append(f"{label}: note is not valid UTF-8")
            continue
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


class Commit(NamedTuple):
    """A commit in the gated range."""

    sha: str
    subject: str
    body: str


class NoteChanges(NamedTuple):
    """Note paths added or modified, and deleted, since the base (README excluded)."""

    changed: list[str]
    deleted: list[str]


def _is_breaking(commit: Commit) -> bool:
    match = SUBJECT_RE.match(commit.subject)
    return bool(match and match["bang"]) or bool(BREAKING_FOOTER_RE.search(commit.body))


def _requires_note(commit: Commit) -> bool:
    """Whether git-cliff would show ``commit``, so it needs a release note."""
    match = SUBJECT_RE.match(commit.subject)
    if match is None:
        return False
    if _is_breaking(commit):
        return True
    if (match["scope"] or "").startswith("deps"):
        return False
    if re.search("Changelog: skip", commit.body):
        return False
    return match["type"] in NOTE_TYPES


def _has_skip_trailer(body: str) -> bool:
    return bool(re.search(r"^Release-Notes:\s*skip\s*$", body, re.MULTILINE))


def _gate_failures(commits: list[Commit], notes: NoteChanges, breaking_note: bool) -> list[str]:
    """Return one line per commit whose release-note requirement isn't met."""
    if any(_has_skip_trailer(c.body) for c in commits):
        return []
    failures: list[str] = []
    for commit in commits:
        label = f"{commit.sha[:12]} {commit.subject!r}"
        if _requires_note(commit):
            match = SUBJECT_RE.match(commit.subject)
            is_revert = match is not None and match["type"] == "revert"
            if not notes.changed and not (is_revert and notes.deleted):
                hint = " (or delete the reverted note)" if is_revert else ""
                failures.append(
                    f"{label} needs a release note: run `toolr release-notes add`{hint}, "
                    "or add a `Release-Notes: skip` trailer to a commit"
                )
        if _is_breaking(commit) and not breaking_note:
            failures.append(
                f"{label} is breaking: an added or modified note must start with `{BREAKING_PREFIX}`"
            )
    return failures


def _git_output(ctx: Context, *args: str) -> str:
    ret = ctx.run(
        "git",
        "-C",
        str(ctx.repo_root),
        *args,
        capture_output=True,
        stream_output=False,
    )
    if ret.returncode:
        ctx.error(f"git {args[0]} failed with exit code {ret.returncode}")
        ctx.exit(ret.returncode)
    output: str = ret.stdout.read()  # type: ignore[assignment]
    return output


def _commits_in_range(ctx: Context, base: str) -> list[Commit]:
    out = _git_output(ctx, "log", "--no-merges", "--format=%H%x1f%s%x1f%b%x1e", f"{base}..HEAD")
    commits: list[Commit] = []
    for raw in out.split("\x1e"):
        record = raw.strip("\n")
        if record:
            sha, subject, body = record.split("\x1f", 2)
            commits.append(Commit(sha, subject, body))
    return commits


def _note_changes(ctx: Context, base: str) -> NoteChanges:
    out = _git_output(
        ctx, "diff", "--no-renames", "--name-status", f"{base}...HEAD", "--", f"{NOTES_DIR}/"
    )
    changes = NoteChanges(changed=[], deleted=[])
    for line in out.splitlines():
        status, _, path = line.partition("\t")
        if not path or path == f"{NOTES_DIR}/{README_NAME}":
            continue
        if status in {"A", "M"}:
            changes.changed.append(path)
        elif status == "D":
            changes.deleted.append(path)
    return changes


def _is_breaking_note(path: Path) -> bool:
    try:
        return path.is_file() and _read_note(path).startswith(BREAKING_PREFIX)
    except UnicodeDecodeError:
        return False


def _gate_problems(ctx: Context, base: str) -> list[str]:
    ret = ctx.run(
        "git",
        "-C",
        str(ctx.repo_root),
        "rev-parse",
        "--verify",
        "--quiet",
        f"{base}^{{commit}}",
        capture_output=True,
        stream_output=False,
    )
    if ret.returncode:
        return [f"--base {base!r} does not resolve to a commit"]
    commits = _commits_in_range(ctx, base)
    if not commits:
        return [f"No commits in {base}..HEAD: check --base and that the history isn't shallow"]
    notes = _note_changes(ctx, base)
    breaking_note = any(_is_breaking_note(ctx.repo_root / p) for p in notes.changed)
    return _gate_failures(commits, notes, breaking_note)


@group.command
def check(ctx: Context, base: str | None = None) -> None:
    """Validate the queued notes.

    Args:
        base: Git ref to compare against. Also require a note when commits since it need one.
    """
    problems = _file_problems(ctx.repo_root)
    if base is not None:
        problems += _gate_problems(ctx, base)
    _exit_on_problems(ctx, problems)


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
