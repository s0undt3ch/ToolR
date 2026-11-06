import sys

from toolr import command_group


def cg(*args):
    return args


if sys.platform:
    helper = cg("not", "a", "group")

g = command_group("real", "Real", "Group.")


@g.command
def one(ctx):
    """One."""
