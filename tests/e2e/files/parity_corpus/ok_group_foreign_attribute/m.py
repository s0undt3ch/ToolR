from toolr import command_group

g = command_group("real", "Real", "Group.")


@g.command
def one(ctx):
    """One."""


class Registry:
    def command_group(self, name, title, description):
        return name

    def build(self, obj):
        self.command_group("x", "X", "Not toolr's.")
        return obj.command_group("y", "Y", "Not toolr's.")
