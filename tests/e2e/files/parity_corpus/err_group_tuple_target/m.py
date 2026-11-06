from toolr import command_group

a, b = command_group("ga", "Ga", "Group a."), command_group("gb", "Gb", "Group b.")


@a.command
def one(ctx):
    """One."""


@b.command
def two(ctx):
    """Two."""
