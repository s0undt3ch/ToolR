try:
    from toolr import command_group as cg
except ImportError:
    cg = None

# The `except` binding wins, so the parser doesn't read this as a group.
g = cg("d", "D", "Described where the parser can't see it.")
