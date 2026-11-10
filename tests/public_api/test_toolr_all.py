"""`toolr.__all__` is the promised Python surface; this pins it."""

from __future__ import annotations

import pytest

import toolr

EXPECTED = {
    "ArgSection",
    "Context",
    "__version__",
    "arg",
    "arg_section",
    "command",
    "command_group",
    "current_context",
}


def test_all_is_exactly_the_documented_surface():
    assert set(toolr.__all__) == EXPECTED


@pytest.mark.parametrize(
    "name", ["report_on_import_errors", "MANIFEST_SCHEMA_VERSION", "DispatchCommand"]
)
def test_trimmed_names_are_no_longer_top_level(name):
    assert not hasattr(toolr, name)
