from toolr import command_group

(g := command_group("g", "G", "Group."))


@g.command
def one(ctx):
    """One."""
