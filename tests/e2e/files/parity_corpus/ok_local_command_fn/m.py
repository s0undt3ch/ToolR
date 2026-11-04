from toolr import command_group

g = command_group("grp", "Grp", "Grp")


def command(f=None, **kw):
    return f if f else (lambda x: x)


@g.command
def ok(ctx):
    """Ok."""


class Helper:
    @command
    def method(self):
        """Not toolr."""
