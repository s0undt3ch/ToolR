from __future__ import annotations

import pytest

import toolr
from toolr._context import current_context
from toolr._exc import NoCurrentContextError


def test_current_context_raises_when_unset() -> None:
    with pytest.raises(NoCurrentContextError, match=r"current_context\(\) has no context"):
        current_context()


def test_current_context_is_exported_from_top_level_package() -> None:
    assert toolr.current_context is current_context
