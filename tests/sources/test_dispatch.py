"""Tests for DispatchCommand basic shape (argv tested separately)."""

from __future__ import annotations

import pytest

from toolr.sources import ArgSchema
from toolr.sources import CommandSchema
from toolr.sources import DispatchCommand


def _migrate_schema() -> CommandSchema:
    return CommandSchema(
        name="migrate",
        summary="",
        description="",
        arguments=[
            ArgSchema(name="check", kind="flag", help=""),
            ArgSchema(name="database", kind="optional", help="", default="default"),
        ],
    )


def test_dispatch_command_holds_match():
    dc = DispatchCommand(
        command="migrate",
        command_args={"check": True, "database": "primary"},
        schema=_migrate_schema(),
    )
    assert dc.command == "migrate"
    assert dc.command_args == {"check": True, "database": "primary"}
    assert dc.schema.name == "migrate"


@pytest.mark.parametrize(
    ("args_in", "schema_args", "expected"),
    [
        # Positional value.
        (
            {"app_label": "auth"},
            [ArgSchema(name="app_label", kind="positional", help="")],
            ["auth"],
        ),
        # Flag set True → emit, False → omit.
        (
            {"check": True, "verbose": False},
            [
                ArgSchema(name="check", kind="flag", help=""),
                ArgSchema(name="verbose", kind="flag", help=""),
            ],
            ["--check"],
        ),
        # Optional with default — omit when equal, emit otherwise.
        (
            {"database": "default"},
            [ArgSchema(name="database", kind="optional", help="", default="default")],
            [],
        ),
        (
            {"database": "primary"},
            [ArgSchema(name="database", kind="optional", help="", default="default")],
            ["--database", "primary"],
        ),
        # Repeated → one `--name value` per element.
        (
            {"exclude": ["a", "b"]},
            [ArgSchema(name="exclude", kind="repeated", help="")],
            ["--exclude", "a", "--exclude", "b"],
        ),
        # Underscores in the name become dashes on the wire when no
        # source-literal long_flag was recorded (native commands or
        # legacy manifests).
        (
            {"dry_run": True},
            [ArgSchema(name="dry_run", kind="flag", help="")],
            ["--dry-run"],
        ),
        # When `long_flag` IS recorded (argparse scanner picked the
        # source's literal spelling), emit it verbatim regardless of
        # whether toolr's CLI display normalised underscores away.
        (
            {"user-ids": 7},
            [
                ArgSchema(
                    name="user-ids",
                    kind="optional",
                    help="",
                    long_flag="--user_ids",
                ),
            ],
            ["--user_ids", "7"],
        ),
        (
            {"user-ids": [3, 4]},
            [
                ArgSchema(
                    name="user-ids",
                    kind="repeated",
                    help="",
                    long_flag="--user_ids",
                ),
            ],
            ["--user_ids", "3", "--user_ids", "4"],
        ),
    ],
)
def test_argv_reconstruction(args_in, schema_args, expected):
    schema = CommandSchema(name="x", summary="", description="", arguments=schema_args)
    dc = DispatchCommand(command="x", command_args=args_in, schema=schema)
    assert dc.argv == expected


def test_argv_unknown_arg_name_raises():
    schema = CommandSchema(name="x", summary="", description="", arguments=[])
    dc = DispatchCommand(command="x", command_args={"surprise": True}, schema=schema)
    with pytest.raises(ValueError, match="surprise"):
        _ = dc.argv
