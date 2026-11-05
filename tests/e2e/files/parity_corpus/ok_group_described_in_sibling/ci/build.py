from toolr import command_group

g = command_group("ci", "CI", "CI commands.")


@g.command
def build(ctx):
    """Build."""
