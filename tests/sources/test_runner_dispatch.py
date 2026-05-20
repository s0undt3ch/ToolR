"""Runner-side: construct DispatchCommand and call the dispatcher function."""

from __future__ import annotations

from typing import Any

import pytest

from toolr._runner import invoke_dispatcher
from toolr.sources import ArgSchema
from toolr.sources import CommandSchema
from toolr.sources import DispatchCommand


def test_invoke_dispatcher_passes_dispatch_command():
    captured: dict[str, Any] = {}

    def parent(ctx, *, cpu: str = "1", dispatched: DispatchCommand) -> int:
        captured["cpu"] = cpu
        captured["dispatched"] = dispatched
        return 0

    schema = CommandSchema(
        name="migrate",
        summary="",
        description="",
        arguments=[ArgSchema(name="check", kind="flag", help="")],
    )
    rc = invoke_dispatcher(
        ctx=None,
        func=parent,
        parent_kwargs={"cpu": "5000m"},
        child_name="migrate",
        child_args={"check": True},
        child_schema=schema,
    )

    assert rc == 0
    assert captured["cpu"] == "5000m"
    assert isinstance(captured["dispatched"], DispatchCommand)
    assert captured["dispatched"].command == "migrate"
    assert captured["dispatched"].command_args == {"check": True}
    assert captured["dispatched"].schema == schema


def test_invoke_dispatcher_with_non_dispatcher_raises():
    def parent(ctx, *, cpu: str = "1") -> int:
        return 0

    schema = CommandSchema(name="x", summary="", description="", arguments=[])
    with pytest.raises(RuntimeError, match="DispatchCommand"):
        invoke_dispatcher(
            ctx=None,
            func=parent,
            parent_kwargs={},
            child_name="x",
            child_args={},
            child_schema=schema,
        )
