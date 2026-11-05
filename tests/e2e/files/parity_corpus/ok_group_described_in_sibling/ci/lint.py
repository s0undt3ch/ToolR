from toolr import command_group

g = command_group("ci", "CI")


@g.command
def lint(ctx):
    """Lint."""
