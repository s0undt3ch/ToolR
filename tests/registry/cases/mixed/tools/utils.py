"""Standalone utility commands."""

from __future__ import annotations

from toolr import registry

# Create a simple utilities group
utils_group = registry.command_group("utils", "Utilities", "General utility commands")


@utils_group.command("clean", help="Clean temporary files")
def utils_clean(args):
    """Clean temporary files."""
    return "utils clean executed"


@utils_group.command("backup", help="Create backup")
def utils_backup(args):
    """Create backup."""
    return "utils backup executed"


@utils_group.command("restore", help="Restore from backup")
def utils_restore(args):
    """Restore from backup."""
    return "utils restore executed"
