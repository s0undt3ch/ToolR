from toolr import command_group

# Declares the group without a description: the build errors only if `m.py`'s alias isn't read as a
# declaration, which would have supplied one.
command_group("d", "D")
