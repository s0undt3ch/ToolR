from __future__ import annotations

from toolr.utils._docstrings import Docstring


def test_docstring_with_simple_examples():
    """Test parsing a docstring with simple examples."""
    docstring = """Process data with various options.

    Examples:
        Examples can be seen under the `examples/` directory.
    """
    result = Docstring.parse(docstring)

    assert len(result.examples) == 1
    assert result.examples[0].description == "Examples can be seen under the `examples/` directory."
    assert result.examples[0].snippet is None


def test_docstring_with_complex_examples():
    """Test parsing a docstring with complex examples."""
    docstring = """Process data with various options.

    Examples:
        Basic usage:
            result = process_data("input.txt")

        With custom options:
            result = process_data(
                "input.txt",
                format="json",
                validate=True
            )

        Error handling:
            try:
                result = process_data("nonexistent.txt")
            except FileNotFoundError:
                print("File not found")
    """
    result = Docstring.parse(docstring)
    assert len(result.examples) == 3

    assert result.examples[0].description == "Basic usage:"
    assert result.examples[0].snippet is None
    assert result.examples[1].description == "With custom options:"
    assert result.examples[1].snippet is None
    assert result.examples[2].description == "Error handling:"
    assert result.examples[2].snippet is None


def test_docstring_with_complex_examples_with_markdown_code_blocks():
    """Test parsing a docstring with complex examples."""
    docstring = """Process data with various options.

    Examples:
        Basic usage:
        ```
        result = process_data("input.txt")
        ```

        With custom options:
        ```
        result = process_data(
                "input.txt",
                format="json",
                validate=True
            )
        ```

        Error handling:
        ```
            try:
                result = process_data("nonexistent.txt")
            except FileNotFoundError:
                print("File not found")
        ```
    """
    result = Docstring.parse(docstring)
    assert len(result.examples) == 3

    assert result.examples[0].description == "Basic usage:"
    assert result.examples[0].snippet is not None
    assert result.examples[1].description == "With custom options:"
    assert result.examples[1].snippet is not None
    assert result.examples[2].description == "Error handling:"
    assert result.examples[2].snippet is not None


def test_docstring_with_syntax_examples():
    """Test parsing a docstring with syntax examples."""
    docstring = """Process data with various options.

    Examples:
        Basic usage:
        ```python
        result = process_data("input.txt")
        ```

        Python REPL example:
        >>> result = process_data("input.txt")
        >>> print(result)

        JavaScript example:
        ```javascript
        const result = processData("input.txt");
        console.log(result);
        ```
    """
    result = Docstring.parse(docstring)
    assert len(result.examples) == 3

    # First example with explicit python syntax
    assert result.examples[0].description == "Basic usage:"
    assert result.examples[0].snippet is not None
    assert result.examples[0].syntax == "python"

    # Second example with Python REPL (>>>) - should auto-detect as python
    assert result.examples[1].description == "Python REPL example:"
    assert result.examples[1].snippet is not None
    assert result.examples[1].syntax == "python"

    # Third example with explicit javascript syntax
    assert result.examples[2].description == "JavaScript example:"
    assert result.examples[2].snippet is not None
    assert result.examples[2].syntax == "javascript"
