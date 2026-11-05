from click import command

from toolr import command_group

g = command_group("grp", "Grp", "Grp")


@g.command
def ok(ctx):
    """Ok."""


def make_click():
    @command()
    def cli():
        """Click."""

    return cli
