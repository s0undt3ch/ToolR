from __future__ import annotations

import msgspec

from toolr.utils._rust_utils import DocstringParser


class DocstringExample(msgspec.Struct, frozen=True):
    """Example of a docstring."""

    description: str = ""
    snippet: str | None = None
    syntax: str | None = None


class DocstringVersionChanged(msgspec.Struct, frozen=True):
    """Version changed entry with version as key and description as value."""

    version: str = ""
    description: str = ""


class Docstring(msgspec.Struct, frozen=True):
    """Optimized docstring representation using direct msgspec fields."""

    short_description: str
    long_description: str | None = None
    params: dict[str, str | None] = msgspec.field(default_factory=dict)
    examples: list[DocstringExample] = msgspec.field(default_factory=list)
    notes: list[str] = msgspec.field(default_factory=list)
    warnings: list[str] = msgspec.field(default_factory=list)
    see_also: list[str] = msgspec.field(default_factory=list)
    references: list[str] = msgspec.field(default_factory=list)
    todo: list[str] = msgspec.field(default_factory=list)
    deprecated: str | None = None
    version_added: str | None = None
    version_changed: list[DocstringVersionChanged] = msgspec.field(default_factory=list)

    @classmethod
    def parse(cls, docstring: str) -> Docstring:
        """Parse a docstring using our rust implementation."""
        parser = DocstringParser()
        raw_data = parser.parse(docstring)
        return msgspec.convert(raw_data, cls)

    @property
    def full_description(self) -> str:
        """
        Generate a full description combining all docstring sections using markdown.
        """
        full_description = self.short_description

        if self.long_description:
            full_description += f"\n\n{self.long_description}\n"

        if self.examples:
            full_description += "\nExamples:"
            for example in self.examples:
                description = example.description
                if not description.startswith(("- ", "* ")):
                    description = f"- {description}"
                full_description += f"\n\n{description}"
                if example.snippet:
                    full_description += f"\n\n```\n{example.snippet}\n```"

        if self.notes:
            full_description += "\n\nNotes:\n"
            for note in self.notes:
                if not note.startswith(("- ", "* ")):
                    note = f"- {note}"  # noqa: PLW2901
                full_description += f"\n{note}"

        if self.warnings:
            full_description += "\n\nWarnings:\n"
            for warning in self.warnings:
                if not warning.startswith(("- ", "* ")):
                    warning = f"- {warning}"  # noqa: PLW2901
                full_description += f"\n{warning}"

        if self.see_also:
            full_description += "\n\nSee Also:\n"
            for see_also in self.see_also:
                if not see_also.startswith(("- ", "* ")):
                    see_also = f"- {see_also}"  # noqa: PLW2901
                full_description += f"\n{see_also}"

        if self.references:
            full_description += "\n\nReferences:\n"
            for reference in self.references:
                if not reference.startswith(("- ", "* ")):
                    reference = f"- {reference}"  # noqa: PLW2901
                full_description += f"\n{reference}"

        if self.todo:
            full_description += "\n\nTodo:\n"
            for todo in self.todo:
                if not todo.startswith(("- ", "* ")):
                    todo = f"- {todo}"  # noqa: PLW2901
                full_description += f"\n{todo}"

        if self.deprecated:
            full_description += f"\n\nDeprecated:\n{self.deprecated}"

        if self.version_added:
            full_description += f"\n\nVersion Added: {self.version_added}"

        if self.version_changed:
            full_description += "\n\nVersion Changed:\n"
            for version_changed in self.version_changed:
                full_description += f"- {version_changed.version}: {version_changed.description}\n"
        return full_description
