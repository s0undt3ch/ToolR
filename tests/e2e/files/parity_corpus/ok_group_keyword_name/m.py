from toolr import command_group

g = command_group(name="kw", title="Kw", description="Group.")


@g.command
def one(ctx):
    """One."""
