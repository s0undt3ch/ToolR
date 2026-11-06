from toolr import command_group as cg

cg = cg
g = cg("y", "Y", "Group y.")


@g.command
def run(ctx):
    """Run."""
