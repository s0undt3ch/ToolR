from toolr import command_group

p1 = command_group("p1", "P1", "P1 commands.")
p2 = command_group("p2", "P2", "P2 commands.")
a = command_group("kid", "Kid one", "First kid.", parent="p1")
b = command_group("kid", "Kid two", "Second kid.", parent="p2")


@a.command
def one(ctx):
    """One."""


@b.command
def two(ctx):
    """Two."""
