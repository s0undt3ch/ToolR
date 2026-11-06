from toolr import command_group as real

g = real("real", "Real", "Group.")


@g.command
def one(ctx):
    """One."""


command_group = print


def helper():
    command_group("x", "X", "Not toolr's.")
