"""Commands backing this repo's pre-commit hooks."""

from __future__ import annotations

import shutil

from toolr import Context
from toolr import command_group

group = command_group(
    "pre-commit",
    "Commands backing this repo's pre-commit hooks",
    docstring=__doc__,
)


def cargo_path(ctx: Context) -> str:
    """Return the ``cargo`` on ``PATH``, or exit with an error naming the fix."""
    cargo = shutil.which("cargo")
    if cargo is None:
        ctx.error("`cargo` not found on PATH; run `mise install`", markup=False)
        ctx.exit(1)
    return cargo
