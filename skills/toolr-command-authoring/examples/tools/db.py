"""Database lifecycle commands.

Exercise: ``arg_section`` for grouping related flags in ``--help``,
mixed positional and keyword-only arguments, and a Google-style
docstring with ``Args:`` and ``Examples:`` sections.
"""

from __future__ import annotations

from typing import Annotated

from toolr import Context
from toolr import arg
from toolr import arg_section
from toolr import command_group

db = command_group("db", "Database lifecycle", docstring=__doc__)

DESTRUCTIVE = arg_section(
    "Destructive options",
    description="These actions modify or drop user data — pair with --yes.",
)


@db.command
def reset(
    ctx: Context,
    *,
    yes: Annotated[bool, arg(help_section=DESTRUCTIVE)] = False,
) -> None:
    """Drop and recreate the database from scratch.

    Args:
        yes: Skip the confirmation prompt.

    Examples:
        Reset interactively:

            toolr db reset

        Reset without prompting (CI):

            toolr db reset --yes
    """
    if not yes:
        ctx.info("Pass --yes to confirm.")
        return
    ctx.info("Dropping and recreating database…")
