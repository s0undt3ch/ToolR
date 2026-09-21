from __future__ import annotations

import pytest

from toolr._context import current_context
from toolr._exc import NoCurrentContextError


def test_current_context_raises_when_unset() -> None:
    with pytest.raises(NoCurrentContextError, match=r"current_context\(\) has no context"):
        current_context()
