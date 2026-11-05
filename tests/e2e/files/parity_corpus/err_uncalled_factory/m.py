from toolr import command_group

g = command_group("grp", "Grp", "Grp")


@g.command
def ok(ctx):
    """Ok."""


def factory():
    @g.command
    def inner(ctx):
        """Inner."""
