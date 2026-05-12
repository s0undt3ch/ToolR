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
    """One captured-output snippet.

    `tools_files`, when set, names additional `.py` files that should be
    overlaid into the fixture's `tools/` directory before this snippet is
    captured. Use it when an example needs its own `tools/<name>.py`
    (typical for the writing-commands chapter, where different examples
    register colliding group names).
    """

    path: Path
    argv: tuple[str, ...]
    tools_files: tuple[Path, ...] = ()


# Every captured snippet the docs consume. Keep this list in sync with
# the `--8<--` includes in the doc pages. Adding a new snippet means:
# 1. Add an entry here.
# 2. Run this script.
# 3. Reference the resulting file via `--8<--` in the doc page.
WC_FILES = REPO_ROOT / "docs" / "writing-commands" / "files"
CLI_FILES = REPO_ROOT / "docs" / "cli-files"

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
    # Writing-commands chapter — Arguments (captures only the subset of
    # features that currently render correctly in the rust front-end —
    # see docs/writing-commands/known-bugs.md for the rest).
    Snippet(
        WC_FILES / "calculator-add-help.txt",
        ("math", "add", "--help"),
        tools_files=(WC_FILES / "calculator.py",),
    ),
    Snippet(
        WC_FILES / "hello-help.txt",
        ("greeting", "hello", "--help"),
        tools_files=(WC_FILES / "hello.py",),
    ),
    Snippet(
        WC_FILES / "literal-choices-help.txt",
        ("logs", "set-level", "--help"),
        tools_files=(WC_FILES / "literal-choices.py",),
    ),
    # Writing-commands chapter — Using `ctx`.
    Snippet(
        WC_FILES / "context-hello.txt",
        ("example", "hello", "--name", "Pedro"),
    ),
    # CLI reference page — every public subcommand's --help output.
    Snippet(CLI_FILES / "toolr-help.txt", ("--help",)),
    Snippet(CLI_FILES / "project-init-help.txt", ("project", "init", "--help")),
    Snippet(CLI_FILES / "project-deps-sync-help.txt", ("project", "deps", "sync", "--help")),
    Snippet(CLI_FILES / "project-venv-path-help.txt", ("project", "venv", "path", "--help")),
    Snippet(CLI_FILES / "project-venv-shell-help.txt", ("project", "venv", "shell", "--help")),
    Snippet(
        CLI_FILES / "project-manifest-rebuild-help.txt",
        ("project", "manifest", "rebuild", "--help"),
    ),
    Snippet(
        CLI_FILES / "self-completion-print-help.txt",
        ("self", "completion", "print", "--help"),
    ),
    Snippet(
        CLI_FILES / "self-completion-install-help.txt",
        ("self", "completion", "install", "--help"),
    ),
    Snippet(CLI_FILES / "self-cache-list-help.txt", ("self", "cache", "list", "--help")),
    Snippet(CLI_FILES / "self-cache-prune-help.txt", ("self", "cache", "prune", "--help")),
    Snippet(
        CLI_FILES / "self-build-manifest-help.txt",
        ("self", "build-manifest", "--help"),
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


def _materialise_in_tree_venv(fixture: Path, runner_python: Path) -> None:
    """Build a fake in-tree venv at `fixture/tools/.venv/` that points to
    `runner_python` (the dev venv).

    This lets `toolr project manifest rebuild` resolve a real Python
    interpreter without paying the cost of `uv sync`. The fixture's
    `tools/pyproject.toml` is rewritten to use `venv-location = "in-tree"`
    so resolution lands at exactly this directory.

    Implementation: symlink the dev venv's `bin/`, `lib/pythonX.Y/`, and
    copy its `pyvenv.cfg` so the symlinked Python finds its base
    interpreter (and therefore the stdlib).
    """
    dev_venv = runner_python.parent.parent  # .venv/bin/python -> .venv
    pyvenv_cfg = dev_venv / "pyvenv.cfg"
    if not pyvenv_cfg.is_file():
        msg = f"dev venv at {dev_venv} has no pyvenv.cfg"
        raise SystemExit(msg)
    py_lib_dir = next(dev_venv.glob("lib/python*"), None)
    if py_lib_dir is None:
        msg = f"could not locate lib/pythonX.Y under {dev_venv}"
        raise SystemExit(msg)

    target_venv = fixture / "tools" / ".venv"
    target_venv.mkdir(parents=True, exist_ok=True)
    # Symlink the full bin/ and lib/pythonX.Y/ trees so toolr's
    # site-packages probe and the runner subprocess both work.
    (target_venv / "bin").symlink_to(dev_venv / "bin")
    (target_venv / "lib").mkdir()
    (target_venv / "lib" / py_lib_dir.name).symlink_to(py_lib_dir)
    # Copy pyvenv.cfg so Python finds its base interpreter / stdlib.
    shutil.copy(pyvenv_cfg, target_venv / "pyvenv.cfg")

    # Rewrite pyproject.toml to use in-tree venv-location.
    pyproject = fixture / "tools" / "pyproject.toml"
    if pyproject.is_file():
        body = pyproject.read_text()
        body = body.replace('venv-location = "cache"', 'venv-location = "in-tree"')
        pyproject.write_text(body)


def prepare_fixture(
    toolr: Path,
    dest: Path,
    runner_python: Path,
    extra_tools_files: tuple[Path, ...] = (),
) -> None:
    """Copy the fixture into `dest` and build a full manifest (static + dynamic).

    When `extra_tools_files` is non-empty the fixture's default
    `tools/example.py` is removed and the named files are copied into
    `tools/` instead; this lets a snippet supply its own scenario
    without polluting the shared sample-repo.

    Materialises a fake in-tree venv that symlinks back to the dev
    venv, then runs `toolr project manifest rebuild` so the manifest
    has both static and dynamic layers (nested groups, enum / bool /
    list inference, etc.).
    """
    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(FIXTURE, dest)
    if extra_tools_files:
        default_example = dest / "tools" / "example.py"
        if default_example.is_file():
            default_example.unlink()
        for src in extra_tools_files:
            shutil.copy(src, dest / "tools" / src.name)
    _materialise_in_tree_venv(dest, runner_python)
    subprocess.run(  # noqa: S603
        [str(toolr), "project", "manifest", "rebuild"],
        cwd=dest,
        check=True,
        capture_output=True,
        env={**os.environ, "TOOLR_NO_CACHE_HINT": "1"},
    )


def capture(toolr: Path, fixture: Path, _python: Path, snippet: Snippet) -> str:
    """Run the toolr binary against the prepared fixture; return stdout+stderr."""
    env = {
        **os.environ,
        "TOOLR_NO_CACHE_HINT": "1",
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
    # Strip trailing whitespace per line so the trailing-whitespace
    # pre-commit hook can't fight us. clap occasionally emits a stray
    # trailing space on the line above a continuation; not interesting
    # to capture verbatim.
    return "\n".join(line.rstrip() for line in body.splitlines()) + ("\n" if body.endswith("\n") else "")


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
        tmproot = Path(tmpdir)
        default_fixture = tmproot / "default"
        prepare_fixture(toolr, default_fixture, python)
        scenario_fixtures: dict[tuple[Path, ...], Path] = {}
        for snippet in SNIPPETS:
            if snippet.tools_files:
                key = snippet.tools_files
                fixture = scenario_fixtures.get(key)
                if fixture is None:
                    fixture = tmproot / f"scenario-{len(scenario_fixtures)}"
                    prepare_fixture(toolr, fixture, python, extra_tools_files=key)
                    scenario_fixtures[key] = fixture
            else:
                fixture = default_fixture
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
