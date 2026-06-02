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
    """Parsed docstring representation.

    Carries each per-section field plus the Rust-rendered
    ``full_description`` (suitable for clap's ``long_about`` slot).
    """

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
    # Pre-rendered multi-section text (short + long + Examples/Notes/…),
    # populated by the Rust ``DocstringParser::parse`` extension. We
    # don't recompute this on the Python side — the Rust struct method
    # ``toolr_core::docstrings::Docstring::full_description`` is the
    # single source of truth so a stale Python implementation can't
    # drift out of sync.
    full_description: str = ""

    @classmethod
    def parse(cls, docstring: str) -> Docstring:
        """Parse a docstring using our rust implementation."""
        parser = DocstringParser()
        raw_data = parser.parse(docstring)
        return msgspec.convert(raw_data, cls)
