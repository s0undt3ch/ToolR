"""Tests for DispatchCommand basic shape (argv tested separately)."""

from __future__ import annotations

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
