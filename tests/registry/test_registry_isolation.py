"""The command-group registry is empty at the start of every test.

The two tests rely on running in file order: the first leaves a group
behind unless the autouse fixture in ``tests/conftest.py`` isolates it.
"""

from __future__ import annotations

from toolr import command_group
from toolr._decorators import _get_command_group_storage


def test_registers_a_group():
    command_group("leak", "L", "d")

    assert "tools.leak" in _get_command_group_storage()


def test_registry_starts_empty():
    assert _get_command_group_storage() == {}
