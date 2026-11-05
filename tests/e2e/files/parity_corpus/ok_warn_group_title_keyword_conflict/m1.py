from toolr import command_group

g = command_group("ci", title="CI", description="CI commands.")


@g.command
def one(ctx):
    """One."""
