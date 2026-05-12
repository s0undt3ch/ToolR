#!/usr/bin/env python3
"""Regenerate (or verify) captured `.txt` snippets used in the docs.

Each `.txt` snippet lives alongside a `.py` example file or under
`docs/.../files/`. The captures are produced by running the real
`toolr` binary against `docs/.fixtures/sample-repo/`. This script:

- In default mode, regenerates every snippet in place.
- With `--check`, regenerates into memory and compares against the
  on-disk file; exits 1 (with a diff) on any drift.

Invoked from CI and (path-scoped) from a local pre-commit hook.
"""

from __future__ import annotations

import argparse
import difflib
import os
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
FIXTURE = REPO_ROOT / "docs" / ".fixtures" / "sample-repo"


@dataclass(frozen=True)
class Snippet:
    """One captured-output snippet."""

    path: Path
    argv: tuple[str, ...]


# Every captured snippet the docs consume. Keep this list in sync with
# the `--8<--` includes in the doc pages. Adding a new snippet means:
# 1. Add an entry here.
# 2. Run this script.
# 3. Reference the resulting file via `--8<--` in the doc page.
SNIPPETS: tuple[Snippet, ...] = (
    Snippet(
        REPO_ROOT / "docs" / "quickstart-files" / "toolr-help.txt",
        ("--help",),
    ),
    Snippet(
        REPO_ROOT / "docs" / "quickstart-files" / "example-help.txt",
        ("example", "--help"),
    ),
    Snippet(
        REPO_ROOT / "docs" / "quickstart-files" / "example-hello.txt",
        ("example", "hello", "--name", "world"),
    ),
    Snippet(
        REPO_ROOT / "docs" / "quickstart-files" / "example-setlog-help.txt",
        ("example", "setlog", "--help"),
    ),
)


def find_toolr() -> Path:
    """Resolve the toolr binary the regen should use."""
    override = os.environ.get("TOOLR_REGEN_BINARY")
    if override:
        return Path(override)
    release = REPO_ROOT / "target" / "release" / "toolr"
    if release.is_file():
        return release
    debug = REPO_ROOT / "target" / "debug" / "toolr"
    if debug.is_file():
        return debug
    # No binary on disk — build a debug binary. Debug is much faster
    # to compile than release; output diff is identical for `--help`
    # text and example execution.
    subprocess.run(  # noqa: S603
        ["cargo", "build", "--bin", "toolr", "--quiet"],  # noqa: S607
        cwd=REPO_ROOT,
        check=True,
    )
    return debug


def find_runner_python() -> Path:
    """Resolve a Python that has the toolr runner importable.

    Captures of executable commands need a Python interpreter where
    `import toolr._runner` works. The dev `.venv/bin/python` is the
    natural choice. Overridable via `TOOLR_REGEN_PYTHON`.
    """
    override = os.environ.get("TOOLR_REGEN_PYTHON")
    if override:
        return Path(override)
    candidate = REPO_ROOT / ".venv" / "bin" / "python"
    if not candidate.is_file():
        msg = (
            "regen-doc-snippets: no .venv/bin/python found. "
            "Run `uv sync --dev` to create one, or set "
            "TOOLR_REGEN_PYTHON to a Python with toolr installed."
        )
        raise SystemExit(msg)
    return candidate


def prepare_fixture(toolr: Path, dest: Path) -> None:
    """Copy the fixture into `dest` and build its static manifest.

    Removes `tools/pyproject.toml` afterwards so the dispatcher falls
    back to `TOOLR_PYTHON` instead of trying to materialise an actual
    uv-managed venv. Mirrors the approach used by the integration
    tests in `tests/project_init.rs`.
    """
    shutil.copytree(FIXTURE, dest, dirs_exist_ok=True)
    subprocess.run(  # noqa: S603
        [str(toolr), "__build-static-manifest"],
        cwd=dest,
        check=True,
        capture_output=True,
    )
    pyproject = dest / "tools" / "pyproject.toml"
    if pyproject.is_file():
        pyproject.unlink()


def capture(toolr: Path, fixture: Path, python: Path, snippet: Snippet) -> str:
    """Run the toolr binary against the prepared fixture; return stdout+stderr."""
    env = {
        **os.environ,
        "TOOLR_NO_CACHE_HINT": "1",
        "TOOLR_PYTHON": str(python),
        "PYTHONPATH": str(fixture),
    }
    result = subprocess.run(  # noqa: S603
        [str(toolr), *snippet.argv],
        cwd=fixture,
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    # Concatenate stdout + stderr so error-path captures work too.
    # Most snippets are clean `--help`, but executing commands may
    # interleave logs on stderr.
    body = result.stdout
    if result.stderr:
        if body and not body.endswith("\n"):
            body += "\n"
        body += result.stderr
    return body


def regen_one(
    toolr: Path,
    fixture: Path,
    python: Path,
    snippet: Snippet,
    check_only: bool,
) -> bool:
    """Return True if the snippet on disk matches the regenerated value."""
    new_contents = capture(toolr, fixture, python, snippet)
    snippet.path.parent.mkdir(parents=True, exist_ok=True)
    if check_only:
        if not snippet.path.is_file():
            print(f"missing: {snippet.path}", file=sys.stderr)
            return False
        current = snippet.path.read_text()
        if current == new_contents:
            return True
        diff = difflib.unified_diff(
            current.splitlines(keepends=True),
            new_contents.splitlines(keepends=True),
            fromfile=f"a/{snippet.path.relative_to(REPO_ROOT)}",
            tofile=f"b/{snippet.path.relative_to(REPO_ROOT)}",
        )
        sys.stderr.write("".join(diff))
        return False
    snippet.path.write_text(new_contents)
    return True


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="Verify on-disk snippets match the regenerated output; exit 1 on drift.",
    )
    args = parser.parse_args(argv)

    if not shutil.which("cargo") and not (REPO_ROOT / "target/release/toolr").is_file():
        print("regen-doc-snippets: cargo missing and no pre-built toolr binary", file=sys.stderr)
        return 1

    toolr = find_toolr()
    python = find_runner_python()
    clean = True
    with tempfile.TemporaryDirectory(prefix="toolr-doc-fixture-") as tmpdir:
        fixture = Path(tmpdir)
        prepare_fixture(toolr, fixture)
        for snippet in SNIPPETS:
            ok = regen_one(toolr, fixture, python, snippet, args.check)
            if not ok:
                clean = False
    if args.check and not clean:
        print(
            "\nregen-doc-snippets: doc snippets are stale. "
            "Run `.pre-commit-hooks/regen-doc-snippets.py` to update them.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
