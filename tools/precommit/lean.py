"""
Pre-commit hook command that guards the lean ``toolr-core`` build ``toolr-py`` links.

``toolr-py`` builds ``toolr-core`` without its ``full`` feature. No workspace build does that, because
of feature unification, so this command checks that build on its own.
"""

from __future__ import annotations

from toolr import Context

from ._common import cargo_path
from ._common import group

# A new toolr-core dependency must be optional and listed in `full`, or added here.
LEAN_DEPS_ALLOWLIST = ("libc", "serde", "winapi")

LEAN_TREE_ARGS = (
    "tree",
    "-p",
    "toolr-core",
    "--no-default-features",
    "-e",
    "normal",
    "--depth",
    "1",
    "--prefix",
    "none",
    "--target",
    "all",
)

CLIPPY_LEAN_ARGS = (
    (
        "clippy",
        "-p",
        "toolr-core",
        "--no-default-features",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ),
    ("clippy", "-p", "toolr-py", "--all-targets", "--", "-D", "warnings"),
)


def direct_deps(tree: str) -> list[str]:
    """Return the sorted, unique crate names in ``cargo tree --depth 1 --prefix none`` output.

    The first line is the root crate itself, so it is skipped.
    """
    return sorted({line.split()[0] for line in tree.splitlines()[1:] if line.strip()})


def _check_deps(ctx: Context, cargo: str) -> None:
    ret = ctx.run(cargo, *LEAN_TREE_ARGS, capture_output=True, stream_output=False)
    if ret.returncode:
        stderr: str = ret.stderr.read()  # type: ignore[assignment]
        ctx.error(f"cargo tree failed: {stderr}", markup=False)
        ctx.exit(ret.returncode)
    tree: str = ret.stdout.read()  # type: ignore[assignment]
    deps = direct_deps(tree)
    expected = sorted(LEAN_DEPS_ALLOWLIST)
    if deps != expected:
        ctx.error(
            f"toolr-core without `full` depends on {' '.join(deps)!r}, expected {' '.join(expected)!r}",
            markup=False,
        )
        ctx.exit(1)


@group.command
def check_lean_build(ctx: Context) -> None:
    """Check the ``toolr-core`` build without ``full``, and ``toolr-py``.

    First, ``toolr-core`` without ``full`` may depend only on allowlisted crates, because ``toolr-py``
    links whatever that build needs. A new ``toolr-core`` dependency must be ``optional = true`` and
    listed in ``full``, or added to ``LEAN_DEPS_ALLOWLIST``. Then clippy lints both crates with
    warnings denied.
    """
    cargo = cargo_path(ctx)
    _check_deps(ctx, cargo)
    for args in CLIPPY_LEAN_ARGS:
        ret = ctx.run(cargo, *args)
        if ret.returncode:
            ctx.exit(ret.returncode)
