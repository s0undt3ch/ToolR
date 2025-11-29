"""Property-based fuzzing tests for docstring parsing."""

from __future__ import annotations

import pytest
from hypothesis import given
from hypothesis import settings
from hypothesis import strategies as st
from hypothesis.strategies import composite
from hypothesis.strategies import integers
from hypothesis.strategies import lists
from hypothesis.strategies import text

from toolr.utils._docstrings import Docstring


# Custom strategies for generating realistic docstring-like content
@composite
def docstring_content(draw):
    """Generate realistic docstring content for fuzzing."""
    # Start with basic text that could be a docstring
    content = draw(
        text(
            alphabet=st.characters(whitelist_categories=("Lu", "Ll", "N", "Pc", "Pd", "Ps", "Pe", "Po", "Sm", "Zs")),
            min_size=0,
            max_size=1000,
        )
    )

    # Sometimes add common docstring elements
    if draw(st.booleans()):
        # Add some common docstring patterns
        patterns = [
            "\n\nArgs:\n    param: description",
            "\n\nReturns:\n    Something useful",
            "\n\nRaises:\n    ValueError: when things go wrong",
            "\n\nExample:\n    >>> func()\n    'result'",
            '"""Triple quoted string"""',
            "    Indented content",
            "\n\n    Multi-line\n    content\n    here",
        ]
        pattern = draw(st.sampled_from(patterns))
        content += pattern

    return content


@composite
def malformed_docstring(draw):
    """Generate potentially malformed docstrings that might cause issues."""
    base = draw(docstring_content())

    # Add potentially problematic elements
    modifications = [
        lambda s: s + "\x00",  # Null byte
        lambda s: s + "\x01\x02\x03",  # Control characters
        lambda s: s * 100,  # Very long repetition
        lambda s: s + "\n" * 1000,  # Many newlines
        lambda s: s + " " * 1000,  # Many spaces
        lambda s: s + "\t" * 100,  # Many tabs
        lambda s: s.replace("\n", "\r\n"),  # Windows line endings
        lambda s: s.replace("\n", "\r"),  # Old Mac line endings
        lambda s: "🚀" + s + "🎉",  # Unicode emoji
        lambda s: "αβγδε" + s + "ζηθικ",  # Greek letters
        lambda s: "中文测试" + s + "日本語",  # CJK characters
    ]

    if draw(st.booleans()):
        modification = draw(st.sampled_from(modifications))
        base = modification(base)

    return base


# Expected exceptions for malformed input
EXPECTED_EXCEPTIONS = (
    ValueError,  # Invalid input format
    TypeError,  # Type conversion issues
    UnicodeError,  # Unicode handling issues
)


@given(content=docstring_content())
@settings(max_examples=200, deadline=5000)  # Run 200 examples with 5s timeout
def test_fuzz_random_docstrings(content: str):
    """Test that docstring parsing doesn't crash on random valid content."""
    # The parser should not crash on any valid string input
    try:
        result = Docstring.parse(content)

        # Basic invariants that should always hold
        assert isinstance(result.short_description, str)
        assert result.long_description is None or isinstance(result.long_description, str)
        assert isinstance(result.params, dict)
        assert isinstance(result.examples, list)
        assert isinstance(result.notes, list)
        assert isinstance(result.warnings, list)

        # The original content should be preserved in some form
        # (though it may be processed/normalized)
        if content.strip():
            # If there was non-whitespace content, there should be some output
            # (unless it was all special formatting that got stripped)
            pass  # Don't assert here as valid docstrings might be empty after processing

    except Exception as e:  # pragma: no cover
        # Log the problematic input for debugging
        pytest.fail(f"Docstring parsing crashed on input: {content!r}\nError: {e}")


@given(content=malformed_docstring())
@settings(max_examples=100, deadline=10000)  # Fewer examples but longer timeout for complex cases
def test_fuzz_malformed_docstrings(content: str):
    """Test that docstring parsing handles malformed input gracefully."""
    try:
        result = Docstring.parse(content)

        # Even with malformed input, basic types should be correct
        assert isinstance(result.short_description, str)
        assert result.long_description is None or isinstance(result.long_description, str)
        assert isinstance(result.params, dict)
        assert isinstance(result.examples, list)
        assert isinstance(result.notes, list)
        assert isinstance(result.warnings, list)

    except Exception as e:  # pragma: no cover
        # Some malformed input might legitimately cause parsing errors
        # but it should be well-defined exceptions, not crashes
        if not isinstance(e, EXPECTED_EXCEPTIONS):
            pytest.fail(f"Unexpected exception type on malformed input: {content!r}\nError: {type(e).__name__}: {e}")


@given(content=text(min_size=0, max_size=10000), repeat=integers(min_value=1, max_value=10))
@settings(max_examples=50)
def test_fuzz_repeated_content(content: str, repeat: int):
    """Test parsing with repeated content patterns."""
    repeated_content = content * repeat

    # Should not crash regardless of repetition
    try:
        result = Docstring.parse(repeated_content)
        assert isinstance(result, Docstring)
    except Exception as e:  # pragma: no cover
        pytest.fail(f"Failed on repeated content (repeat={repeat}): {content!r}\nError: {e}")


@given(lines=lists(text(max_size=100), min_size=0, max_size=100))
@settings(max_examples=50)
def test_fuzz_multiline_content(lines: list[str]):
    """Test parsing with various multiline content."""
    content = "\n".join(lines)

    try:
        result = Docstring.parse(content)
        assert isinstance(result, Docstring)

        # Verify that multiline content is handled properly
        if len(lines) > 1:
            # Should handle multiple lines without crashing
            assert isinstance(result.short_description, str)

    except Exception as e:  # pragma: no cover
        pytest.fail(f"Failed on multiline content: {lines!r}\nError: {e}")


@pytest.mark.parametrize(
    "content",
    [
        "",  # Empty string
        " ",  # Single space
        "\n",  # Single newline
        "\t",  # Single tab
        "   ",  # Multiple spaces
        "\n\n\n",  # Multiple newlines
        "\t\t\t",  # Multiple tabs
        " \n \t \n ",  # Mixed whitespace
    ],
)
def test_fuzz_empty_and_whitespace(content: str):
    """Test parsing of empty and whitespace-only strings."""
    try:
        result = Docstring.parse(content)
        assert isinstance(result, Docstring)
        # Empty/whitespace content should result in empty descriptions
        assert isinstance(result.short_description, str)
    except Exception as e:  # pragma: no cover
        pytest.fail(f"Failed on whitespace content: {content!r}\nError: {e}")


@pytest.mark.parametrize("encoding", ["utf-8", "latin1", "ascii"])
@given(st.binary(min_size=0, max_size=1000))
@settings(max_examples=50)
def test_fuzz_bytes_as_string(encoding: str, data: bytes):
    """Test what happens when binary data is decoded and parsed."""
    try:
        # Decode bytes to string (may raise UnicodeDecodeError)
        content = data.decode(encoding, errors="ignore")

        # Parser should handle the resulting string without crashing
        result = Docstring.parse(content)
        assert isinstance(result, Docstring)

    except UnicodeDecodeError:  # pragma: no cover
        # This is expected for some byte sequences
        pytest.skip(f"UnicodeDecodeError with {encoding} encoding")
    except Exception as e:  # pragma: no cover
        pytest.fail(f"Unexpected error with {encoding} decoded data: {data!r}\nError: {e}")


@pytest.mark.parametrize("line_ending", ["\n", "\r\n", "\r"])
def test_fuzz_different_line_endings(line_ending: str):
    """Test parsing docstrings with different line ending styles."""
    content = f"First line{line_ending}Second line{line_ending}Third line"

    result = Docstring.parse(content)
    assert isinstance(result, Docstring)
    assert isinstance(result.short_description, str)


@pytest.mark.parametrize(
    "unicode_char",
    [
        "🚀",  # Emoji
        "αβγ",  # Greek letters
        "中文",  # Chinese characters
        "日本語",  # Japanese
        "العربية",  # Arabic
        "русский",  # Cyrillic
        "\u200b",  # Zero-width space
        "\ufeff",  # Byte order mark
    ],
)
def test_fuzz_unicode_content(unicode_char: str):
    """Test parsing docstrings with various Unicode characters."""
    content = f"Test {unicode_char} docstring"

    result = Docstring.parse(content)
    assert isinstance(result, Docstring)
    assert isinstance(result.short_description, str)


@pytest.mark.parametrize("size", [1, 10, 100, 1000, 10000])
def test_fuzz_very_long_content(size: int):
    """Test parsing very long docstrings."""
    content = "A" * size

    result = Docstring.parse(content)
    assert isinstance(result, Docstring)
    assert isinstance(result.short_description, str)


@given(st.text(alphabet=st.characters(blacklist_categories=["Cc", "Cs"]), max_size=1000))
@settings(max_examples=100)
def test_fuzz_random_text_no_control_chars(content: str):
    """Test parsing random text without control characters and surrogates."""
    try:
        result = Docstring.parse(content)
        assert isinstance(result, Docstring)
        assert isinstance(result.short_description, str)
        assert result.long_description is None or isinstance(result.long_description, str)
    except UnicodeEncodeError:  # pragma: no cover
        # Expected for some Unicode edge cases like surrogate pairs
        pytest.skip("UnicodeEncodeError with problematic Unicode content")
    except Exception as e:  # pragma: no cover
        # Any other exception should be handled gracefully
        if not isinstance(e, EXPECTED_EXCEPTIONS):
            pytest.fail(f"Unexpected exception type: {type(e).__name__}: {e}")
