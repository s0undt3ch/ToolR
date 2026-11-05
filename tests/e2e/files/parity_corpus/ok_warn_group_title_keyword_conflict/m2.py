from toolr import command_group

g = command_group("ci", "Continuous integration")


@g.command
def two(ctx):
    """Two."""
