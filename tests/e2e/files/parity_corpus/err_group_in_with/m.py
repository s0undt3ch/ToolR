import contextlib

from toolr import command_group

with contextlib.nullcontext():
    g = command_group("g", "G", "Group.")


@g.command
def one(ctx):
    """One."""
