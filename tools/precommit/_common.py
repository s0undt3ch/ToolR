"""Commands backing this repo's pre-commit hooks."""

from __future__ import annotations

from toolr import command_group

group = command_group(
    "pre-commit",
    "Commands backing this repo's pre-commit hooks",
    docstring=__doc__,
)
