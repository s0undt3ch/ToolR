from toolr import command_group as cg

g = cg("alias", "Alias", "Aliased group.")


@g.command
def run(ctx):
    """Run."""
