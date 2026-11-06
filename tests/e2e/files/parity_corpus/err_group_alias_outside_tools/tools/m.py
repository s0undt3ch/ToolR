from lib_cg import cg

# `lib_cg` sits outside `tools/`, which the parser can't read: not a group to it.
g = cg("a", "A", "Described where the parser can't see it.")
