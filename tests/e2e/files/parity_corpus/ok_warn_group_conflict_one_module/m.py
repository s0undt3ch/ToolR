from toolr import command_group

a = command_group("grp", "Grp", "Grp commands.")
b = command_group("grp", "Other", "Other commands.")


@a.command
def one(ctx):
    """One."""


@b.command
def two(ctx):
    """Two."""
