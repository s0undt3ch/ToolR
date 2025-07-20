from __future__ import annotations

from typing import TYPE_CHECKING

from docstring_parser.google import GoogleParser
from msgspec import Struct


class Docstring(Struct, frozen=True):
    short_description: str
    long_description: str
    params: dict[str, str | None]


def parse_docstring(docstring: str) -> Docstring:
    """Parse a docstring into a Docstring object."""
    parser = GoogleParser()
    parse_docstring = parser.parse(docstring)
    short_description = parse_docstring.short_description
    long_description = parse_docstring.long_description or short_description

    if TYPE_CHECKING:
        assert short_description is not None
        assert long_description is not None

    params = {param.arg_name: param.description for param in parse_docstring.params}
    return Docstring(
        short_description=short_description,
        long_description=long_description,
        params=params,
    )
