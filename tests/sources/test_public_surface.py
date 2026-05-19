"""toolr.sources should re-export exactly the documented public surface."""

from __future__ import annotations

import toolr.sources

EXPECTED = {"ArgSchema", "CommandSchema", "DispatchCommand"}


def test_all_lists_public_surface():
    assert set(toolr.sources.__all__) == EXPECTED


def test_each_name_is_importable():
    for name in EXPECTED:
        assert hasattr(toolr.sources, name), f"missing: {name}"
