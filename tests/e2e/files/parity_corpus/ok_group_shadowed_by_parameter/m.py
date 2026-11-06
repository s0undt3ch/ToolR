from toolr import command_group

g = command_group("real", "Real", "Group.")


@g.command
def one(ctx):
    """One."""


def helper(command_group):
    return command_group("x", "X", "Not toolr's.")


shout = lambda command_group: command_group("y", "Y", "Not toolr's.")  # noqa: E731
