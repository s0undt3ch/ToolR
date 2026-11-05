from toolr import command_group

g = command_group("ci", "CI", "CI commands.", "Runs every check.")


@g.command
def two(ctx):
    """Two."""
