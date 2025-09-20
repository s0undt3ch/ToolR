from __future__ import annotations

from typing import TYPE_CHECKING

from msgspec import Struct

from toolr.utils._rust_utils import DocstringParser

if TYPE_CHECKING:
    from toolr.utils._rust_utils import ParsedDocstring


class Docstring(Struct, frozen=True):
    short_description: str
    long_description: str
    params: dict[str, str | None]


def parse_docstring(docstring: str) -> Docstring:
    """Parse a docstring into a Docstring object."""
    parser = DocstringParser()
    result: ParsedDocstring = parser.parse(docstring)
    short_description = result.get("short_description") or ""
    long_description = result.get("long_description") or short_description
    params = result.get("params")
    parameters: dict[str, str | None] = {}
    if params:
        for param_name, param_data in params.items():
            parameters[param_name] = param_data["description"] or None

    examples = result.get("examples")
    if examples:
        long_description += "\n\nExamples:\n"
        for example in examples:
            long_description += f"\n{example['description']}\n{example['snippet']}\n"

    notes = result.get("notes")
    if notes:
        long_description += "\n\nNotes:\n"
        for note in notes:
            long_description += f"\n{note}\n"

    raises = result.get("raises")
    if raises:
        long_description += "\n\nRaises:\n"
        for exception_type, description in raises.items():
            long_description += f"\n{exception_type}: {description}\n"

    warnings = result.get("warnings")
    if warnings:
        long_description += "\n\nWarnings:\n"
        for warning in warnings:
            long_description += f"\n{warning}\n"

    see_also = result.get("see_also")
    if see_also:
        long_description += "\n\nSee Also:\n"
        for see_also_description in see_also:
            long_description += f"\n{see_also_description}\n"

    references = result.get("references")
    if references:
        long_description += "\n\nReferences:\n"
        for reference in references:
            long_description += f"\n{reference}\n"

    todo = result.get("todo")
    if todo:
        long_description += f"\n\nTodo:\n{todo}"

    deprecated = result.get("deprecated")
    if deprecated:
        long_description += f"\n\nDeprecated:\n{deprecated}"

    version_added = result.get("version_added")
    if version_added:
        long_description += f"\n\nVersion Added:\n{version_added}"

    version_changed = result.get("version_changed")
    if version_changed:
        long_description += f"\n\nVersion Changed:\n{version_changed}"

    return Docstring(
        short_description=short_description,
        long_description=long_description,
        params=parameters,
    )
