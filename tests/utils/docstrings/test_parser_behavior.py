"""Parser behavior and edge case tests."""

from __future__ import annotations

import pytest

from toolr.utils._docstrings import Docstring


def test_parser_strict_mode():
    """Test parser strict mode (currently same as normal mode)."""
    docstring = """Test function.

    Args:
        param: A parameter
    """
    result = Docstring.parse(docstring)

    # Strict mode should work the same as normal mode for now
    assert result.short_description == "Test function."
    assert "param" in result.params


def test_parser_performance():
    """Test parser performance with a large docstring."""
    # Create a large docstring
    large_docstring = """Generate test data.

    Args:
    """
    for i in range(100):
        large_docstring += f"    param_{i}: Parameter {i}\n"

    large_docstring += """
    Returns:
        dict: Result

    Notes:
    """
    for i in range(50):
        large_docstring += f"    Note {i}\n"

    # Should parse without issues
    result = Docstring.parse(large_docstring)

    assert result.short_description == "Generate test data."
    assert len(result.params) == 100
    assert len(result.notes) == 50


def test_parser_error_handling():
    """Test parser error handling with malformed docstrings."""
    # Test with None input
    with pytest.raises(TypeError, match="'NoneType' object cannot be converted to 'PyString'"):
        Docstring.parse(None)

    # Test with very long input
    very_long = "A" * 100000
    result = Docstring.parse(very_long)
    assert result.short_description == very_long


def test_parser_consistency():
    """Test parser consistency across multiple calls."""
    docstring = """Test function.

    Args:
        param: A parameter

    Returns:
        dict: Result
    """

    # Parse multiple times
    results = []
    for _ in range(10):
        result = Docstring.parse(docstring)
        results.append(result)

    # All results should be identical
    for result in results[1:]:
        assert result == results[0]


def test_parser_with_special_characters():
    """Test parser with special characters in docstring."""
    docstring = """Function with special chars: @#$%^&*().

    Args:
        param_with_underscores: Parameter with underscores
        param-with-dashes: Parameter with dashes
        param.with.dots: Parameter with dots

    Notes:
        Note with special chars: @#$%^&*()
    """
    result = Docstring.parse(docstring)

    assert "special chars" in result.short_description
    assert "param_with_underscores" in result.params
    assert "param-with-dashes" in result.params
    assert "param.with.dots" in result.params
    assert "special chars" in result.notes[0]


def test_parser_with_unicode():
    """Test parser with unicode characters."""
    docstring = """Function with unicode: 你好世界.

    Args:
        parámetro: Parámetro con acentos
        параметр: Параметр на русском

    Notes:
        Результат с unicode
    """  # noqa: RUF001
    result = Docstring.parse(docstring)

    assert "unicode" in result.short_description
    assert "parámetro" in result.params
    assert "параметр" in result.params
    assert "Результат с unicode" in result.notes  # noqa: RUF001


def test_parser_with_empty_sections():
    """Test parser with empty sections."""
    docstring = """Test function.

    Args:

    Returns:

    Notes:

    Examples:
    """
    result = Docstring.parse(docstring)

    assert result.short_description == "Test function."
    assert result.params == {}
    assert result.notes == []
    assert result.examples == []


def test_parser_with_malformed_sections():
    """Test parser with malformed section headers."""
    docstring = """Test function.

    Args: (malformed)
        param: A parameter

    Returns: (malformed)
        dict: Result

    Notes: (malformed)
        A note
    """
    result = Docstring.parse(docstring)

    # Parser should handle malformed sections gracefully
    assert result.short_description == "Test function."
    assert "param" in result.params
    assert result.notes is not None
    assert len(result.notes) > 0
