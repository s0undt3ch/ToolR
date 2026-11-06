from toolr import command_group as cg

g = cg("alias", "Alias")


@g.command
def run(ctx):
    """Run."""
