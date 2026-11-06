from pathlib import Path as command_group
from toolr import command_group as real

g = real("real", "Real", "Group.")


@g.command
def one(ctx):
    """One."""


def helper():
    return command_group("x", "X", "Not toolr's.")
