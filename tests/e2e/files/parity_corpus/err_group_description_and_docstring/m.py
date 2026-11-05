"""Grp commands."""

from toolr import command_group

g = command_group("grp", "Grp", description="Grp.", docstring=__doc__)


@g.command
def one(ctx):
    """One."""
