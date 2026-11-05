from toolr import command_group

g = command_group("ci", "CI")


@g.command
def build(ctx):
    """Build."""
