"""MkDocs llmstxt preprocess hook: strips mkdocstrings source dumps from the LLM output."""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from bs4 import BeautifulSoup


def preprocess(soup: BeautifulSoup, output: str) -> None:
    """Remove the collapsible source listings mkdocstrings adds to API pages."""
    for element in soup.select("details.mkdocstrings-source"):
        element.decompose()
