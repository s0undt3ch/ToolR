from toolr import command_group

p1 = command_group("p1", "P1", "P1 commands.")
a = command_group("p1.kid", "Kid", "Kid commands.")
b = command_group("kid", "Child", "Kid commands.", parent="p1")


@p1.command
def top(ctx):
    """Top."""


@a.command
def one(ctx):
    """One."""


@b.command
def two(ctx):
    """Two."""
