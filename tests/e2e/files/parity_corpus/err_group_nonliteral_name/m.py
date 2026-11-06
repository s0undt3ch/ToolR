from toolr import command_group

NAME = "n"
g = command_group(NAME, "N", "Group.")


@g.command
def one(ctx):
    """One."""
