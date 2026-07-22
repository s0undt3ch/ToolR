"""
Pre-commit hook commands for the captured doc snippets.
"""

from __future__ import annotations

import difflib
import os
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

from toolr import Context

from ._common import group


@dataclass(frozen=True)
class Snippet:
    """One captured-output snippet.

    Paths are repo-root-relative. `tools_files`, when set, names additional
    `.py` files that should be overlaid into the fixture's `tools/`
    directory before this snippet is captured. Use it when an example needs
    its own `tools/<name>.py` (typical for the writing-commands chapter,
    where different examples register colliding group names).
    """

    path: str
    argv: tuple[str, ...]
    tools_files: tuple[str, ...] = ()


# Every captured snippet the docs consume. Keep this list in sync with
# the `--8<--` includes in the doc pages. Adding a new snippet means:
# 1. Add an entry here.
# 2. Run `toolr pre-commit regen-doc-snippets`.
# 3. Reference the resulting file via `--8<--` in the doc page.
WC_FILES = "docs/writing-commands/files"
CLI_FILES = "docs/cli-files"

SNIPPETS: tuple[Snippet, ...] = (
    Snippet("docs/quickstart-files/toolr-help.txt", ("--help",)),
    Snippet("docs/quickstart-files/example-help.txt", ("example", "--help")),
    Snippet(
        "docs/quickstart-files/example-hello.txt",
        ("example", "hello", "--name", "world"),
    ),
    Snippet(
        "docs/quickstart-files/example-setlog-help.txt",
        ("example", "setlog", "--help"),
    ),
    # Writing-commands chapter — Arguments (captures only the subset of
    # features that currently render correctly in the rust front-end —
    # see docs/writing-commands/known-bugs.md for the rest).
    Snippet(
        f"{WC_FILES}/calculator-add-help.txt",
        ("math", "add", "--help"),
        tools_files=(f"{WC_FILES}/calculator.py",),
    ),
    Snippet(
        f"{WC_FILES}/hello-help.txt",
        ("greeting", "hello", "--help"),
        tools_files=(f"{WC_FILES}/hello.py",),
    ),
    Snippet(
        f"{WC_FILES}/literal-choices-help.txt",
        ("logs", "set-level", "--help"),
        tools_files=(f"{WC_FILES}/literal-choices.py",),
    ),
    # Writing-commands chapter — Using `ctx`.
    Snippet(f"{WC_FILES}/context-hello.txt", ("example", "hello", "--name", "Pedro")),
    # CLI reference page — every public subcommand's --help output.
    Snippet(f"{CLI_FILES}/toolr-help.txt", ("--help",)),
    Snippet(f"{CLI_FILES}/project-init-help.txt", ("project", "init", "--help")),
    Snippet(f"{CLI_FILES}/project-venv-sync-help.txt", ("project", "venv", "sync", "--help")),
    Snippet(f"{CLI_FILES}/project-venv-lock-help.txt", ("project", "venv", "lock", "--help")),
    Snippet(f"{CLI_FILES}/project-venv-add-help.txt", ("project", "venv", "add", "--help")),
    Snippet(f"{CLI_FILES}/project-venv-remove-help.txt", ("project", "venv", "remove", "--help")),
    Snippet(f"{CLI_FILES}/project-venv-path-help.txt", ("project", "venv", "path", "--help")),
    Snippet(f"{CLI_FILES}/project-venv-shell-help.txt", ("project", "venv", "shell", "--help")),
    Snippet(f"{CLI_FILES}/project-venv-run-help.txt", ("project", "venv", "run", "--help")),
    Snippet(
        f"{CLI_FILES}/project-manifest-rebuild-help.txt",
        ("project", "manifest", "rebuild", "--help"),
    ),
    Snippet(
        f"{CLI_FILES}/self-completion-print-help.txt",
        ("self", "completion", "print", "--help"),
    ),
    Snippet(
        f"{CLI_FILES}/self-completion-install-help.txt",
        ("self", "completion", "install", "--help"),
    ),
    Snippet(f"{CLI_FILES}/self-cache-list-help.txt", ("self", "cache", "list", "--help")),
    Snippet(f"{CLI_FILES}/self-cache-prune-help.txt", ("self", "cache", "prune", "--help")),
    Snippet(
        f"{CLI_FILES}/self-build-manifest-help.txt",
        ("self", "build-manifest", "--help"),
    ),
)


def _find_capture_toolr(repo_root: Path) -> Path | None:
    """Resolve the toolr binary the captures should run, or `None`.

    Precedence:
    1. `TOOLR_REGEN_BINARY` env var (explicit override; CI's docs job sets
       this to the freshly built standalone archive).
    2. `target/release/toolr` then `target/debug/toolr` (working-tree cargo
       build, so the captures reflect uncommitted Rust changes).
    3. `toolr` on PATH (system install via mise / install.sh).
    4. `None` — caller decides whether to skip or fail.

    There is intentionally no cargo-build fallback: the local pre-commit
    hook stays fast (or no-ops with a warning) when no toolr is around,
    and CI's docs job always provisions a binary via artifact download.
    """
    override = os.environ.get("TOOLR_REGEN_BINARY")
    if override:
        return Path(override)
    for candidate in (
        repo_root / "target" / "release" / "toolr",
        repo_root / "target" / "debug" / "toolr",
    ):
        if candidate.is_file():
            return candidate
    if path_hit := shutil.which("toolr"):
        return Path(path_hit)
    return None


def _find_runner_python(ctx: Context) -> Path:
    """Resolve a Python that has the toolr runner importable.

    Captures of executable commands need a Python interpreter where
    `import toolr._runner` works. The dev `.venv/bin/python` is the
    natural choice. Overridable via `TOOLR_REGEN_PYTHON`.
    """
    override = os.environ.get("TOOLR_REGEN_PYTHON")
    if override:
        return Path(override)
    candidate = ctx.repo_root / ".venv" / "bin" / "python"
    if not candidate.is_file():
        ctx.error(
            "regen-doc-snippets: no .venv/bin/python found. "
            "Run `uv sync --dev` to create one, or set "
            "TOOLR_REGEN_PYTHON to a Python with toolr installed."
        )
        ctx.exit(1)
    return candidate


def _materialise_in_tree_venv(ctx: Context, fixture: Path, runner_python: Path) -> None:
    """Build a fake in-tree venv at `fixture/tools/.venv/` that points to `runner_python` (the dev venv).

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
        ctx.error(f"dev venv at {dev_venv} has no pyvenv.cfg")
        ctx.exit(1)
    py_lib_dir = next(dev_venv.glob("lib/python*"), None)
    if py_lib_dir is None:
        ctx.error(f"could not locate lib/pythonX.Y under {dev_venv}")
        ctx.exit(1)

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


def _prepare_fixture(
    ctx: Context,
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
    has both static and dynamic layers.
    """
    fixture = ctx.repo_root / "docs" / ".fixtures" / "sample-repo"
    shutil.copytree(fixture, dest)
    if extra_tools_files:
        (dest / "tools" / "example.py").unlink()
        for src in extra_tools_files:
            shutil.copy(src, dest / "tools" / src.name)
    _materialise_in_tree_venv(ctx, dest, runner_python)
    ret = ctx.run(
        str(toolr),
        "project",
        "manifest",
        "rebuild",
        cwd=dest,
        env={**os.environ, "TOOLR_NO_CACHE_HINT": "1"},
        capture_output=True,
        stream_output=False,
    )
    if ret.returncode != 0:
        ctx.error(
            f"regen-doc-snippets: `{toolr} project manifest rebuild` exited "
            f"{ret.returncode} in {dest}"
        )
        sys.stderr.write(ret.stdout.read())
        sys.stderr.write(ret.stderr.read())
        ctx.exit(1)


def _capture(ctx: Context, toolr: Path, fixture: Path, snippet: Snippet) -> str:
    """Run the toolr binary against the prepared fixture; return stdout+stderr."""
    env = {
        **os.environ,
        "TOOLR_NO_CACHE_HINT": "1",
        # Pin terminal width so clap's adaptive `--help` layout is
        # deterministic across machines. Without this, local captures
        # (typically ~80 cols) drift from CI captures (CI sets
        # `COLUMNS=190`), and `--check` perpetually fails on the
        # diff. 100 is wide enough to fit most flag/description pairs
        # on a single line while still wrapping the longer ones —
        # what we'd want a reader to see in the docs.
        "COLUMNS": "100",
    }
    result = ctx.run(
        str(toolr),
        *snippet.argv,
        cwd=fixture,
        env=env,
        capture_output=True,
        stream_output=False,
        # Empty stdin pipe (instead of an inherited tty) so clap
        # respects the COLUMNS variable.
        input="",
    )
    stdout = result.stdout.read()
    stderr = result.stderr.read()
    if result.returncode != 0:
        ctx.error(
            f"regen-doc-snippets: `toolr {' '.join(snippet.argv)}` exited "
            f"{result.returncode} while capturing {snippet.path}"
        )
        sys.stderr.write(stdout)
        sys.stderr.write(stderr)
        ctx.exit(1)

    # Concatenate stdout + stderr so error-path captures work too.
    # Most snippets are clean `--help`, but executing commands may
    # interleave logs on stderr.
    body = stdout
    if stderr:
        if body and not body.endswith("\n"):
            body += "\n"
        body += stderr
    # Strip trailing whitespace per line so the trailing-whitespace
    # pre-commit hook can't fight us. clap occasionally emits a stray
    # trailing space on the line above a continuation; not interesting
    # to capture verbatim.
    return "\n".join(line.rstrip() for line in body.splitlines()) + (
        "\n" if body.endswith("\n") else ""
    )


def _regen_one(
    ctx: Context,
    toolr: Path,
    fixture: Path,
    snippet: Snippet,
    check_only: bool,
) -> str | None:
    """Regenerate one snippet; return a diff when `check_only` finds drift."""
    new_contents = _capture(ctx, toolr, fixture, snippet)
    target = ctx.repo_root / snippet.path
    target.parent.mkdir(parents=True, exist_ok=True)
    if check_only:
        if not target.is_file():
            return f"missing: {snippet.path}\n"
        current = target.read_text()
        if current == new_contents:
            return None
        return "".join(
            difflib.unified_diff(
                current.splitlines(keepends=True),
                new_contents.splitlines(keepends=True),
                fromfile=f"a/{snippet.path}",
                tofile=f"b/{snippet.path}",
            )
        )
    target.write_text(new_contents)
    return None


@group.command
def regen_doc_snippets(ctx: Context, *, check: bool = False) -> None:
    """Regenerate (or verify) captured `.txt` snippets used in the docs.

    Each `.txt` snippet lives alongside a `.py` example file or under
    `docs/.../files/`. The captures are produced by running a real `toolr`
    binary against `docs/.fixtures/sample-repo/` — by default the working
    tree's `target/{release,debug}/toolr` build (falling back to `toolr` on
    PATH), overridable via `TOOLR_REGEN_BINARY`. Fixture commands dispatch
    to the dev `.venv`'s Python (`TOOLR_REGEN_PYTHON` overrides), which
    must be able to `import toolr._runner` — when it can't (and wasn't
    explicitly requested), the command no-ops with a warning so the hook
    stays advisory; CI's docs job is the authoritative freshness gate.

    Args:
        check: Verify on-disk snippets match the regenerated output and
            exit non-zero on drift instead of rewriting the files.
    """
    toolr = _find_capture_toolr(ctx.repo_root)
    if toolr is None:
        ctx.warn(
            "regen-doc-snippets: no `toolr` binary found "
            "(checked $TOOLR_REGEN_BINARY, target/release, target/debug, PATH); "
            "skipping. Run `cargo build --release` or install toolr to enable "
            "this hook locally — CI's docs job verifies snippet freshness regardless."
        )
        return
    python = _find_runner_python(ctx)
    # A binary on PATH does not imply a usable runner: fixture commands
    # dispatch to this Python, which must import toolr's runner (CI's
    # pre-commit job, for instance, has the mise-pinned binary but a venv
    # holding only the `pre-commit` dependency group). Mirror the
    # missing-binary no-op unless the Python was explicitly requested.
    probe = ctx.run(
        str(python),
        "-c",
        "import toolr._runner",
        capture_output=True,
        stream_output=False,
    )
    if probe.returncode != 0:
        if os.environ.get("TOOLR_REGEN_PYTHON"):
            ctx.error(
                f"regen-doc-snippets: $TOOLR_REGEN_PYTHON ({python}) cannot `import toolr._runner`"
            )
            sys.stderr.write(probe.stderr.read())
            ctx.exit(1)
        ctx.warn(
            f"regen-doc-snippets: {python} cannot `import toolr._runner`; "
            "skipping. Run `uv sync --all-extras --dev` to enable this hook "
            "locally — CI's docs job verifies snippet freshness regardless."
        )
        return

    diffs: list[str] = []
    with tempfile.TemporaryDirectory(prefix="toolr-doc-fixture-") as tmpdir:
        tmproot = Path(tmpdir)
        default_fixture = tmproot / "default"
        _prepare_fixture(ctx, toolr, default_fixture, python)
        scenario_fixtures: dict[tuple[str, ...], Path] = {}
        for snippet in SNIPPETS:
            if snippet.tools_files:
                key = snippet.tools_files
                fixture = scenario_fixtures.get(key)
                if fixture is None:
                    fixture = tmproot / f"scenario-{len(scenario_fixtures)}"
                    extra = tuple(ctx.repo_root / name for name in key)
                    _prepare_fixture(ctx, toolr, fixture, python, extra_tools_files=extra)
                    scenario_fixtures[key] = fixture
            else:
                fixture = default_fixture
            diff = _regen_one(ctx, toolr, fixture, snippet, check)
            if diff is not None:
                diffs.append(diff)

    if not check or not diffs:
        if check:
            ctx.info("Doc snippets are in sync.")
        return

    # Drift: replay the diffs on stderr (searchable, copyable job log) and
    # write the human-friendly remediation block to the step summary.
    diff_body = "".join(diffs)
    sys.stderr.write(diff_body)
    github_step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_step_summary is not None:
        if not diff_body.endswith("\n"):
            diff_body += "\n"
        with open(github_step_summary, "a") as wfh:
            wfh.write("## ❌ Doc snippets are stale\n\n")
            wfh.write(
                "The captured `--help` snippets under `docs/` no longer match "
                "the current `toolr` binary. The pre-commit hook is advisory "
                "(skipped when no `toolr` is on PATH locally), so CI is the "
                "authoritative check.\n\n"
            )
            wfh.write("**Fix locally:**\n\n")
            wfh.write("```bash\n")
            wfh.write("cargo build --release -p toolr   # ensure target/release/toolr is current\n")
            wfh.write("toolr pre-commit regen-doc-snippets\n")
            wfh.write("git add docs/\n")
            wfh.write('git commit -m "docs: regen snippets"\n')
            wfh.write("```\n\n")
            wfh.write("<details><summary>Diff</summary>\n\n")
            wfh.write("```diff\n")
            wfh.write(diff_body)
            wfh.write("```\n\n")
            wfh.write("</details>\n")
    ctx.exit(1)
