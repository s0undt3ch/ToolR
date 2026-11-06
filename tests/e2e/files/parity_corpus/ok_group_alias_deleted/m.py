from toolr import command_group as cg

g = cg("c", "C", "Group c.")
del cg


@g.command
def run(ctx):
    """Run."""
