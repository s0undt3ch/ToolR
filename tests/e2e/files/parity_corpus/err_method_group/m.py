from toolr import command_group

ci = command_group("ci", "CI", "CI group.")


class Registry:
    def command_group(self, name, title, description):
        return name


registry = Registry()
registry.command_group("x", "X", "A method on a non-toolr object.")


@ci.command
def one(ctx):
    """One."""
