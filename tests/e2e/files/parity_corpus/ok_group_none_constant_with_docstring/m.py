from toolr import command_group

LONG = None
g = command_group("ci", "CI", long_description=LONG, docstring="CI commands.")


@g.command
def one(ctx):
    """One."""
