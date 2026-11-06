import sys

from toolr import command_group

if sys.platform:
    command_group("n", "N", "Group.")
