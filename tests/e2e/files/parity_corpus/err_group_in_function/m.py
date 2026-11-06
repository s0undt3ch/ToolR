from toolr import command_group


def make():
    return command_group("g", "G", "Group.")


g = make()


@g.command
def one(ctx):
    """One."""
