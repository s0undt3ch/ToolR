from toolr import command_group

cg: object = command_group
g = cg("e", "E", "Group e.")


@g.command
def run(ctx):
    """Run."""
