import toolr

cg = toolr.command_group
g = cg("assigned", "Assigned", "Assigned group.")


@g.command
def run(ctx):
    """Run."""
