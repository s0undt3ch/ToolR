from __future__ import annotations

from typing import TYPE_CHECKING

from docstring_parser.google import GoogleParser

if TYPE_CHECKING:
    from docstring_parser.common import Docstring


def parse_docstring(docstring: str) -> Docstring:
    """Parse a docstring into a Docstring object."""
    parser = GoogleParser()
    return parser.parse(docstring)
