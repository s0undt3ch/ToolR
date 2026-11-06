import toolr

g = toolr.command_group("real", "Real", "Group.")


@g.command
def one(ctx):
    """One."""


def command_group(*args):
    return args


def helper():
    return command_group("not", "toolr", "at all")
