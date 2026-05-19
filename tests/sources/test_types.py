"""Round-trip and field-default tests for toolr.sources schema types."""

from __future__ import annotations

import msgspec

from toolr.sources import ArgSchema


def test_arg_schema_positional_minimal():
    arg = ArgSchema(name="app_label", kind="positional", help="Target app")
    assert arg.name == "app_label"
    assert arg.kind == "positional"
    assert arg.help == "Target app"
    assert arg.default is None
    assert arg.choices is None
    assert arg.metavar is None
    assert arg.type_annotation is None
    assert arg.nargs is None


def test_arg_schema_round_trips_through_msgspec_json():
    arg = ArgSchema(
        name="database",
        kind="optional",
        help="Database to use",
        default="default",
        type_annotation="str",
    )
    payload = msgspec.json.encode(arg)
    decoded = msgspec.json.decode(payload, type=ArgSchema)
    assert decoded == arg
