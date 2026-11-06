from toolr import command_group as cg

cg: object
g = cg("x", "X", "Group x.")


@g.command
def run(ctx):
    """Run."""
