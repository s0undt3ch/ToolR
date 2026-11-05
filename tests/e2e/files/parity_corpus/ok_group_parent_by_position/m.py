from toolr import command_group

ci = command_group("ci", "CI", "CI commands.")
g = command_group("lint", "Lint", "Lint commands.", None, None, "ci")


@g.command
def one(ctx):
    """One."""
