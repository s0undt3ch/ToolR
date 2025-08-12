"""Standalone utility commands."""

from __future__ import annotations

from toolr import Context
from toolr import command_group

# Create a simple utilities group
utils_group = command_group("utils", "Utilities", "General utility commands")


@utils_group.command("clean")
def utils_clean(ctx: Context) -> None:
    """Clean temporary files."""
    ctx.print("utils clean executed")


@utils_group.command("backup")
def utils_backup(ctx: Context) -> None:
    """Create backup."""
    ctx.print("utils backup executed")


@utils_group.command("restore")
def utils_restore(ctx: Context) -> None:
    """Restore from backup."""
    ctx.print("utils restore executed")
