from toolr import command_group

g = command_group("ci", "CI", "Continuous integration commands.")


@g.command
def two(ctx):
    """Two."""
