from toolr import command_group

DESCRIPTION = "CI commands."

g = command_group("ci", "CI", description=DESCRIPTION)


@g.command
def one(ctx):
    """One."""
