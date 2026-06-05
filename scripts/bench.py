"""Benchmark task-runner CLI startup time.

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

The output is a column-padded markdown table (copy/paste-able into a
README or PR description). This script depends only on the Python
standard library — no toolr, no rich — so it can run in a fresh CI job
without bootstrapping a project venv.
"""

from __future__ import annotations

import argparse
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from dataclasses import field
from pathlib import Path

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


def _log(msg: str) -> None:
    """Emit a progress line to stderr so it stays out of the captured markdown."""
    print(msg, file=sys.stderr)  # noqa: T201


def _resolve_binary(spec: ToolSpec, *, install: bool) -> str | None:
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
    _log(f"installing {spec.name} via `uv tool install {spec.pip_pkg}` …")
    # `uv` is intentionally resolved via PATH (S607) so this works against any
    # supported uv install location; `spec.pip_pkg` is a static literal (S603).
    result = subprocess.run(  # noqa: S603
        ["uv", "tool", "install", "--quiet", spec.pip_pkg],  # noqa: S607
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        _log(f"warn: `uv tool install {spec.pip_pkg}` failed; skipping {spec.name}")
        return None
    found = shutil.which(spec.binary)
    if found:
        return found
    bin_dir = subprocess.run(
        ["uv", "tool", "dir", "--bin"],  # noqa: S607 — see comment above for `uv`.
        check=False,
        capture_output=True,
        text=True,
    )
    if bin_dir.returncode == 0:
        candidate = Path(bin_dir.stdout.strip()) / spec.binary
        if candidate.exists():
            return str(candidate)
    _log(f"warn: {spec.binary!r} installed but not on PATH; run `uv tool update-shell`")
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


def compare(
    *,
    only: tuple[str, ...],
    runs: int,
    install: bool,
    keep_tmp: bool,
    path: str | None,
    reuse_caches: bool,
) -> int:
    """Drive the benchmark loop and print the resulting markdown table to stdout."""
    min_runs = 10
    if runs < min_runs:
        _log(
            f"error: --runs must be >= {min_runs} (got {runs}); "
            "steady-state mean needs enough samples"
        )
        return 2

    known = {t.name for t in _TOOLS}
    if only:
        unknown = sorted(set(only) - known)
        if unknown:
            _log(f"error: unknown tool(s): {', '.join(unknown)}")
            _log(f"known: {', '.join(sorted(known))}")
            return 2
        selected = [t for t in _TOOLS if t.name in only]
    else:
        selected = list(_TOOLS)

    user_path = path is not None
    if path is not None:
        root = Path(path).expanduser().resolve()
        root.mkdir(parents=True, exist_ok=True)
    else:
        root = Path(tempfile.mkdtemp(prefix="toolr-bench-"))
    _log(f"workdir: {root}")

    env = None if reuse_caches else _isolated_env(root)
    if not reuse_caches:
        _log(f"isolated XDG_CACHE_HOME: {env['XDG_CACHE_HOME']}")  # type: ignore[index]

    rows = [_Row(spec=s) for s in selected]

    for row in rows:
        spec = row.spec
        _log(f"benchmarking {spec.name} …")
        binary = _resolve_binary(spec, install=install)
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

    _render_markdown(rows, runs=runs)

    if keep_tmp or user_path:
        _log(f"workdir kept at {root}")
    else:
        shutil.rmtree(root, ignore_errors=True)
    return 0


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


def _render_markdown(rows: list[_Row], *, runs: int) -> None:
    """Render the timing rows as a column-padded markdown table to stdout.

    Numeric columns are right-aligned (``|---:|``); text columns get the
    default left alignment. Each cell is padded to the column's widest
    value so the raw markdown source is readable on its own — and still
    renders correctly when pasted into a README or PR description.
    """
    cells = [_row_cells(r) for r in _sorted_rows(rows)]
    headers = [h for h, _ in _COLUMNS]
    widths = [
        max(len(h), *(len(row[i]) for row in cells)) if cells else len(h)
        for i, h in enumerate(headers)
    ]

    def pad(cell: str, width: int, *, numeric: bool) -> str:
        return cell.rjust(width) if numeric else cell.ljust(width)

    def line(values: tuple[str, ...] | list[str]) -> str:
        padded = [pad(v, widths[i], numeric=_COLUMNS[i][1]) for i, v in enumerate(values)]
        return f"| {' | '.join(padded)} |"

    # Right-aligned columns use `---:` — the colon counts toward the
    # visible width, so emit one fewer dash so the separator row lines
    # up with the data rows in the raw markdown source.
    sep_cells = [
        ("-" * (widths[i] - 1) + ":") if is_numeric else ("-" * widths[i])
        for i, (_, is_numeric) in enumerate(_COLUMNS)
    ]

    remaining_count = runs - 2
    out_lines: list[str] = [
        f"`<tool> -h` startup — {runs} runs (remaining = mean of last {remaining_count})",
        "",
        line(headers),
        "| " + " | ".join(sep_cells) + " |",
        *(line(row) for row in cells),
    ]
    print("\n".join(out_lines))  # noqa: T201 — markdown table goes to stdout.


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "only",
        nargs="*",
        metavar="TOOL",
        help=(
            "Optional positional list of tool names to benchmark. "
            "Defaults to all known tools when omitted."
        ),
    )
    parser.add_argument(
        "--runs",
        type=int,
        default=20,
        help="Total invocations per tool. Must be >= 10. (default: %(default)s)",
    )
    parser.add_argument(
        "--install",
        action="store_true",
        help=(
            "`uv tool install` missing Python task-runners. "
            "The `toolr` binary itself is never auto-installed."
        ),
    )
    parser.add_argument(
        "--keep-tmp",
        action="store_true",
        help=("Leave the per-tool directories in place after the run. Implied when --path is set."),
    )
    parser.add_argument(
        "--path",
        default=None,
        help=(
            "Root directory to use instead of a random tmp dir. "
            "Created if missing; existing per-tool subdirs are reused."
        ),
    )
    parser.add_argument(
        "--reuse-caches",
        action="store_true",
        help="Inherit the parent XDG_CACHE_HOME instead of isolating it under <root>.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _build_parser().parse_args(argv)
    return compare(
        only=tuple(args.only),
        runs=args.runs,
        install=args.install,
        keep_tmp=args.keep_tmp,
        path=args.path,
        reuse_caches=args.reuse_caches,
    )


if __name__ == "__main__":
    raise SystemExit(main())
