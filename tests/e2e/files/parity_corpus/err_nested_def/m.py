from toolr import command_group

g = command_group("grp", "Grp", "Grp")


@g.command
def plain(ctx):
    """Plain."""


def register():
    @g.command
    def nested(ctx):
        """Nested."""


register()
