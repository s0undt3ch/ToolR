"""Shared fixtures for docstring parser tests."""

from __future__ import annotations

import pytest

from toolr.utils._rust_utils import DocstringParser


@pytest.fixture
def parser():
    """Create a parser instance for testing."""
    return DocstringParser()
