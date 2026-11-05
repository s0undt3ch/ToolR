from toolr import command_group

g = command_group("ci", "CI", "CI commands.", long_description="Runs the CI checks.")


@g.command
def one(ctx):
    """One."""
