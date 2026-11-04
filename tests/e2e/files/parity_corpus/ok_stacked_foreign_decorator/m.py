import functools

from toolr import command_group

g = command_group("grp", "Grp", "Grp")


@functools.lru_cache
@g.command
def cached(ctx):
    """Cached."""
