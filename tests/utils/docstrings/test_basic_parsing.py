"""Basic docstring parsing tests."""

from __future__ import annotations

from toolr.utils._docstrings import Docstring


def test_single_line_docstring():
    """Test parsing a single-line docstring."""
    docstring = "A simple function that does nothing."
    result = Docstring.parse(docstring)

    assert result.short_description == "A simple function that does nothing."
    assert result.long_description is None
    assert result.params == {}


def test_docstring_with_only_long_description():
    """Test parsing a docstring with only long description."""
    docstring = """
    This is a longer description that spans multiple lines.
    It provides more detailed information about the function.
    """
    result = Docstring.parse(docstring)

    assert result.short_description == "This is a longer description that spans multiple lines."
    assert "It provides more detailed information" in result.long_description
