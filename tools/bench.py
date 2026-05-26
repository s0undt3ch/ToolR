"""
Benchmark task-runner CLI startup time.

For each available task-runner (toolr, invoke, python-tools-scripts, duty,
doit, nox), write a minimal task-file in its own temp subdirectory, then
run ``<tool> -h`` repeatedly. We report three columns rather than just
cold/hot so the first-run setup cost stays visible:

- **First** — invocation #1. Truly cold; for toolr this includes the
  static-manifest build that subsequent invocations skip, so pre-warming
  would be unfair.
- **Second** — invocation #2. OS file cache is warm but no in-process
  warmup has happened; this is what a user sees on their second command
  in a session.
- **Remaining (mean / min / max)** — invocations #3 onward, averaged.
  Steady state.

Missing Python task-runners can be installed on demand with ``--install``
(``uv tool install``). The ``toolr`` binary itself must already be on PATH
(install via the standard ``install.sh``).
"""

from __future__ import annotations

import os
import shutil
import statistics
import subprocess
import tempfile
import time
from dataclasses import dataclass
from dataclasses import field
from pathlib import Path

from rich.table import Table

from toolr import Context
from toolr import command_group

group = command_group("bench", "Benchmark task-runner CLIs", docstring=__doc__)


# ---------------------------------------------------------------------------
# Minimal task-files per tool. Each defines a single `hello` command so the
# `--help` output has comparable shape across tools.
# ---------------------------------------------------------------------------

_TOOLR_PYPROJECT = """\
[project]
name = "tools"
version = "0.0.0"
requires-python = ">=3.11"
dependencies = ["toolr-py"]
"""

_TOOLR_EXAMPLE = '''\
"""Example commands."""

from __future__ import annotations

from toolr import Context
from toolr import command_group

example = command_group("example", "Example commands")


@example.command
def hello(ctx: Context, name: str = "world") -> None:
    """Greet someone.

    Args:
        name: The name to greet.
    """
    ctx.print(f"hello, {name}")
'''

_INVOKE_TASKS = '''\
"""Invoke tasks."""

from invoke import task


@task(help={"name": "Name to greet"})
def hello(c, name="world"):
    """Greet someone."""
    print(f"hello, {name}")
'''

_PTSCRIPTS_INIT = '''\
"""Tools package."""

from tools.example import example  # noqa: F401
'''

_PTSCRIPTS_EXAMPLE = '''\
"""Example commands."""

from __future__ import annotations

from ptscripts import Context
from ptscripts import command_group

example = command_group(name="example", help="Example commands")


@example.command(
    arguments={"name": {"help": "The name to greet"}},
)
def hello(ctx: Context, name: str = "world") -> None:
    """Greet someone."""
    ctx.print(f"hello, {name}")
'''

_DUTY_DUTIES = '''\
"""Duties."""

from duty import duty


@duty
def hello(ctx, name="world"):
    """Greet someone.

    Args:
        name: The name to greet.
    """
    ctx.run(f"echo hello, {name}")
'''

_DOIT_DODO = '''\
"""dodo file."""


def task_hello():
    """Greet someone."""
    return {"actions": ["echo hello, world"]}
'''

_NOX_NOXFILE = '''\
"""noxfile."""

import nox


@nox.session
def hello(session):
    """Greet someone."""
    session.log("hello, world")
'''


@dataclass(frozen=True)
class ToolSpec:
    """One benchmarkable task-runner."""

    name: str
    binary: str
    pip_pkg: str | None  # `uv tool install <pkg>` for missing tools (None = skip)
    files: tuple[tuple[str, str], ...]  # (relative path, contents)
    args: tuple[str, ...] = ("-h",)


_TOOLS: tuple[ToolSpec, ...] = (
    ToolSpec(
        name="toolr",
        binary="toolr",
        pip_pkg=None,  # ships as a binary; install via the install.sh path
        files=(
            ("tools/pyproject.toml", _TOOLR_PYPROJECT),
            ("tools/example.py", _TOOLR_EXAMPLE),
        ),
    ),
    ToolSpec(
        name="invoke",
        binary="invoke",
        pip_pkg="invoke",
        files=(("tasks.py", _INVOKE_TASKS),),
    ),
    ToolSpec(
        name="python-tools-scripts",
        binary="tools",
        pip_pkg="python-tools-scripts",
        files=(
            ("tools/__init__.py", _PTSCRIPTS_INIT),
            ("tools/example.py", _PTSCRIPTS_EXAMPLE),
        ),
    ),
    ToolSpec(
        name="duty",
        binary="duty",
        pip_pkg="duty",
        files=(("duties.py", _DUTY_DUTIES),),
    ),
    ToolSpec(
        name="doit",
        binary="doit",
        pip_pkg="doit",
        files=(("dodo.py", _DOIT_DODO),),
        # doit's top-level parser rejects `-h`; the equivalent subcommand
        # is `doit help`. Same shape (prints help, then exits 0).
        args=("help",),
    ),
    ToolSpec(
        name="nox",
        binary="nox",
        pip_pkg="nox",
        files=(("noxfile.py", _NOX_NOXFILE),),
    ),
)


@dataclass
class _Row:
    """Captured timings for one tool. Mutable so the benchmark loop can fill it in.

    ``samples`` collects every ``<tool> -h`` invocation in order. Position 0 is
    "first" (cold + any first-run setup), position 1 is "second" (OS cache
    warm), positions 2+ are the steady-state ``remaining`` runs.
    """

    spec: ToolSpec
    binary: str | None = None
    samples: list[float] = field(default_factory=list)
    note: str = ""


def _resolve_binary(ctx: Context, spec: ToolSpec, *, install: bool) -> str | None:
    """Return the path to ``spec.binary``, optionally installing on demand.

    For the Python task-runners (invoke, duty, doit, nox, ptscripts) we
    install into a uv-managed tool venv; the resulting executable lands
    in ``uv tool dir --bin`` which is *not* automatically on PATH. If
    ``shutil.which`` still returns ``None`` after install, fall back to
    that directory before giving up — keeps the benchmark usable on a
    machine where the user hasn't run ``uv tool update-shell`` yet.
    """
    found = shutil.which(spec.binary)
    if found:
        return found
    if not install or spec.pip_pkg is None:
        return None
    ctx.info(f"installing {spec.name} via `uv tool install {spec.pip_pkg}` …")
    result = ctx.run(
        "uv",
        "tool",
        "install",
        "--quiet",
        spec.pip_pkg,
        capture_output=True,
        stream_output=False,
    )
    if result.returncode != 0:
        ctx.warn(f"`uv tool install {spec.pip_pkg}` failed; skipping {spec.name}")
        return None
    found = shutil.which(spec.binary)
    if found:
        return found
    bin_dir = ctx.run("uv", "tool", "dir", "--bin", capture_output=True, stream_output=False)
    if bin_dir.returncode == 0:
        candidate = Path(bin_dir.stdout.read().strip()) / spec.binary
        if candidate.exists():
            return str(candidate)
    ctx.warn(f"{spec.binary!r} installed but not on PATH; run `uv tool update-shell`")
    return None


def _write_files(root: Path, files: tuple[tuple[str, str], ...]) -> None:
    for rel, contents in files:
        target = root / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(contents)


def _time_once(
    binary: str,
    args: tuple[str, ...],
    cwd: Path,
    env: dict[str, str] | None,
) -> tuple[float, int]:
    """Run ``binary args...`` in ``cwd``; return ``(elapsed_seconds, returncode)``.

    Stdout/stderr are discarded so the terminal stays quiet during the
    measurement loop. Subprocess spawn is what we're measuring, so we
    wrap ``time.perf_counter()`` around the ``subprocess.run`` call.
    """
    start = time.perf_counter()
    result = subprocess.run(  # noqa: S603 — `binary` is a path resolved by `_resolve_binary` and `args` is a static literal per tool spec.
        [binary, *args],
        cwd=cwd,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
        env=env,
    )
    return time.perf_counter() - start, result.returncode


def _isolated_env(root: Path) -> dict[str, str]:
    """Return a subprocess env that isolates per-tool persistent caches under ``root``.

    Concretely: redirect ``XDG_CACHE_HOME`` into the workspace so toolr's
    venv + static-manifest cache live next to the tool tree (and disappear
    with it). ``UV_CACHE_DIR`` is preserved to the *user's* real uv cache
    so each fresh run doesn't redownload ``toolr-py`` from PyPI — that's
    network-bound and not what we want to measure.
    """
    env = dict(os.environ)
    real_uv_cache = env.get("UV_CACHE_DIR")
    if not real_uv_cache:
        xdg = env.get("XDG_CACHE_HOME") or str(Path.home() / ".cache")
        real_uv_cache = str(Path(xdg) / "uv")
    isolated_xdg = root / "_xdg_cache"
    isolated_xdg.mkdir(parents=True, exist_ok=True)
    env["XDG_CACHE_HOME"] = str(isolated_xdg)
    env["UV_CACHE_DIR"] = real_uv_cache
    return env


@group.command
def compare(
    ctx: Context,
    *only: str,
    runs: int = 20,
    install: bool = False,
    keep_tmp: bool = False,
    path: str | None = None,
    reuse_caches: bool = False,
    markdown: bool = False,
) -> None:
    """Compare ``<tool> -h`` startup time across task-runner CLIs.

    For each task-runner whose binary is on PATH (or installable via
    ``--install``), write a minimal task-file in ``<root>/<tool-name>/``
    and run ``<tool> -h`` ``runs`` times. The result is a table sorted
    by steady-state mean (fastest first).

    Methodology
    -----------

    * **Workspace isolation.** Each ``compare`` invocation creates a
      fresh ``<root>`` (a random tmp dir, or ``--path`` if supplied) and
      places each tool's config in its own subdirectory. Per-tool state
      a runner writes into ``cwd`` (e.g. doit's ``.doit.db``, nox's
      ``.nox/``, toolr's ``.toolr-manifest.json``) lives inside the
      per-tool subdir and therefore starts clean every invocation.

    * **Cache isolation.** ``XDG_CACHE_HOME`` is redirected to
      ``<root>/_xdg_cache`` for the measured subprocesses so toolr's
      venv cache is rebuilt rather than reused from a prior bench run.
      ``UV_CACHE_DIR`` is preserved to the user's real uv wheel cache so
      ``toolr-py`` doesn't get re-downloaded from PyPI every run — that
      would be network-bound, not what we want to measure. Pass
      ``--reuse-caches`` to skip the XDG redirect entirely.

    * **Three columns instead of cold/hot.** The first two runs each
      get their own column so first-run setup cost stays separately
      visible from steady state. Runs #3..N are averaged into the
      "Remaining" column with min/max alongside. Pre-warming is
      deliberately avoided — for toolr in particular, the first run
      builds the static manifest cache, and hiding that would be
      unfair.

    * **Sleeps between #1↔#2 and #2↔#3.** A 500ms gap is inserted after
      runs #1 and #2 so the "Second" and first "Remaining" samples
      don't piggy-back on a still-warm CPU / OS page cache from the
      previous invocation. Runs #3 onward fire back-to-back to capture
      the steady-state cadence a user actually experiences.

    * **What still leaks across runs.** ``uv tool install`` venvs (and
      their pre-compiled ``.pyc`` files) under
      ``$XDG_DATA_HOME/uv/tools/`` persist across bench invocations,
      so the Python task-runners are measured "with an already-installed
      tool". The OS page cache also can't be flushed without root, and
      the ``toolr`` binary itself stays hot in kernel cache because the
      parent process running this bench *is* toolr. Both biases favour
      whichever tool was invoked most recently.

    Args:
        runs: Total invocations per tool. Must be >= 10 so the
            "Remaining" mean averages over a reasonable number of
            steady-state samples.
        install: When set, ``uv tool install`` missing Python
            task-runners. The ``toolr`` binary itself is never
            auto-installed — provision it via the standard install.sh
            path.
        keep_tmp: Leave the per-tool directories in place after the run
            so the user can poke around. Implied when ``--path`` is set.
        path: Root directory to use instead of a random tmp dir. Created
            if missing; existing per-tool subdirs are reused as-is.
        reuse_caches: Inherit the parent ``XDG_CACHE_HOME`` instead of
            isolating it under ``<root>``. Lets toolr reuse a previously
            provisioned venv + manifest cache across bench runs.
        markdown: Render the result as a column-padded markdown table
            (copy/paste-able into a README or PR description) instead of
            the default Rich-styled table.
        only: Optional positional list of tool names to benchmark.
            Defaults to all known tools when omitted.
    """
    min_runs = 10
    if runs < min_runs:
        ctx.error(
            f"--runs must be >= {min_runs} (got {runs}); steady-state mean needs enough samples"
        )
        ctx.exit(2)

    known = {t.name for t in _TOOLS}
    if only:
        unknown = sorted(set(only) - known)
        if unknown:
            ctx.error(f"unknown tool(s): {', '.join(unknown)}")
            ctx.error(f"known: {', '.join(sorted(known))}")
            ctx.exit(2)
        selected = [t for t in _TOOLS if t.name in only]
    else:
        selected = list(_TOOLS)

    user_path = path is not None
    if user_path:
        root = Path(path).expanduser().resolve()
        root.mkdir(parents=True, exist_ok=True)
    else:
        root = Path(tempfile.mkdtemp(prefix="toolr-bench-"))
    ctx.info(f"workdir: {root}")

    env = None if reuse_caches else _isolated_env(root)
    if not reuse_caches:
        ctx.info(f"isolated XDG_CACHE_HOME: {env['XDG_CACHE_HOME']}")  # type: ignore[index]

    rows = [_Row(spec=s) for s in selected]

    for row in rows:
        spec = row.spec
        ctx.info(f"benchmarking {spec.name} …")
        binary = _resolve_binary(ctx, spec, install=install)
        if binary is None:
            row.note = "binary not on PATH (pass --install for Python runners)"
            continue
        row.binary = binary

        workdir = root / spec.name
        workdir.mkdir(exist_ok=True)
        _write_files(workdir, spec.files)

        for i in range(runs):
            elapsed, rc = _time_once(binary, spec.args, workdir, env)
            if rc != 0:
                row.note = f"run #{i + 1} exited with code {rc}"
                row.samples = []
                break
            row.samples.append(elapsed)
            # Insert a brief gap between runs #1↔#2 and #2↔#3 so neither
            # the "Second" nor the first "Remaining" sample piggy-backs on
            # a still-warm CPU / OS page cache from the prior invocation.
            # Subsequent runs are back-to-back to capture steady state.
            if i <= 1:
                time.sleep(0.5)

    if markdown:
        _render_markdown(ctx, rows, runs=runs)
    else:
        _render(ctx, rows, runs=runs)

    if keep_tmp or user_path:
        ctx.info(f"workdir kept at {root}")
    else:
        shutil.rmtree(root, ignore_errors=True)


_COLUMNS: tuple[tuple[str, bool], ...] = (
    # (header, is_numeric)
    ("Tool", False),
    ("First (ms)", True),
    ("Second (ms)", True),
    ("Remaining mean (ms)", True),
    ("Remaining min (ms)", True),
    ("Remaining max (ms)", True),
    ("Notes", False),
)


def _fmt_ms(seconds: float) -> str:
    return f"{seconds * 1000:.1f}"


def _remaining(r: _Row) -> list[float]:
    return r.samples[2:]


def _sorted_rows(rows: list[_Row]) -> list[_Row]:
    """Completed rows first (by steady-state mean), failed rows last."""

    def key(r: _Row) -> tuple[int, float]:
        rest = _remaining(r)
        if rest:
            return (0, statistics.mean(rest))
        return (1, 0.0)

    return sorted(rows, key=key)


def _row_cells(r: _Row) -> tuple[str, ...]:
    """Format one ``_Row`` into the seven column strings (in column order)."""
    rest = _remaining(r)
    if not rest:
        return (r.spec.name, "—", "—", "—", "—", "—", r.note or "—")
    return (
        r.spec.name,
        _fmt_ms(r.samples[0]),
        _fmt_ms(r.samples[1]),
        _fmt_ms(statistics.mean(rest)),
        _fmt_ms(min(rest)),
        _fmt_ms(max(rest)),
        r.note,
    )


def _render(ctx: Context, rows: list[_Row], *, runs: int) -> None:
    """Render the timing rows as a Rich table sorted by remaining mean ascending."""
    remaining_count = runs - 2
    table = Table(
        title=f"`<tool> -h` startup — {runs} runs (remaining = mean of last {remaining_count})"
    )
    for header, is_numeric in _COLUMNS:
        table.add_column(header, justify="right" if is_numeric else "left")
    for r in _sorted_rows(rows):
        table.add_row(*_row_cells(r))
    ctx.print(table)


def _render_markdown(ctx: Context, rows: list[_Row], *, runs: int) -> None:
    """Render the timing rows as a column-padded markdown table.

    Numeric columns are right-aligned (``|---:|``); text columns get the
    default left alignment. Each cell is padded to the column's widest
    value so the raw markdown source is readable on its own — and still
    renders correctly when pasted into a README or PR description.
    """
    cells = [_row_cells(r) for r in _sorted_rows(rows)]
    headers = [h for h, _ in _COLUMNS]
    # Column width = max(header, max-cell-in-column).
    widths = [
        max(len(h), *(len(row[i]) for row in cells)) if cells else len(h)
        for i, h in enumerate(headers)
    ]

    def pad(cell: str, width: int, *, numeric: bool) -> str:
        return cell.rjust(width) if numeric else cell.ljust(width)

    def line(values: tuple[str, ...] | list[str]) -> str:
        padded = [pad(v, widths[i], numeric=_COLUMNS[i][1]) for i, v in enumerate(values)]
        return f"| {' | '.join(padded)} |"

    sep_cells = [
        "-" * widths[i] + (":" if is_numeric else "") for i, (_, is_numeric) in enumerate(_COLUMNS)
    ]
    # The leading position of the colon for right-align is on the right;
    # left-align uses a plain dash run.

    remaining_count = runs - 2
    out_lines: list[str] = [
        f"`<tool> -h` startup — {runs} runs (remaining = mean of last {remaining_count})",
        "",
        line(headers),
        "| " + " | ".join(sep_cells) + " |",
        *(line(row) for row in cells),
    ]
    # Use Rich with markup off + soft_wrap so brackets in headers aren't
    # interpreted and long rows don't get terminal-width-wrapped. The
    # source is then byte-identical to what gets pasted into markdown.
    ctx.print("\n".join(out_lines), markup=False, soft_wrap=True, highlight=False)
