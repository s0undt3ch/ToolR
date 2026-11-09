"""MkDocs hook: fail the build when llms.txt drifts from the nav."""

from __future__ import annotations

import re
from fnmatch import fnmatch
from pathlib import Path
from typing import TYPE_CHECKING
from typing import Any

import mkdocs.plugins
from mkdocs.exceptions import PluginError

if TYPE_CHECKING:
    from collections.abc import Iterable
    from collections.abc import Mapping
    from collections.abc import Sequence

    from mkdocs.config.defaults import MkDocsConfig

_LINK_TARGET = re.compile(r"\]\(([^)\s]+)\)")


def nav_pages(nav: list[Any]) -> list[str]:
    """Return every `.md` source path in a MkDocs nav, in order."""
    pages: list[str] = []
    for item in nav:
        if isinstance(item, str):
            if not item.startswith(("http://", "https://")):
                pages.append(item)
        elif isinstance(item, dict):
            for value in item.values():
                pages.extend(nav_pages(value if isinstance(value, list) else [value]))
    return pages


def resolve(patterns: Iterable[str], pages: Sequence[str]) -> list[str]:
    """Return the nav pages matching any fnmatch pattern, in nav order."""
    patterns = list(patterns)
    return [page for page in pages if any(fnmatch(page, pattern) for pattern in patterns)]


def section_patterns(sections: Mapping[str, Sequence[str | Mapping[str, str]]]) -> list[str]:
    """Flatten the plugin's `sections` into a list of path patterns."""
    patterns: list[str] = []
    for entries in sections.values():
        for entry in entries:
            patterns.append(entry if isinstance(entry, str) else next(iter(entry)))
    return patterns


def md_url(page: str) -> str:
    """Return the path of the Markdown copy the plugin writes for a page."""
    if page == "index.md" or page.endswith("/index.md"):
        return page
    return page.removesuffix(".md") + "/index.md"


def _first_content_line(path: Path) -> str | None:
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            return stripped
    return None


def _link_exists(site_dir: Path, path: str) -> bool:
    path = path.split("#", 1)[0].split("?", 1)[0]
    if not path or path.endswith("/"):
        path += "index.html"
    return (site_dir / path).is_file()


def stale_patterns(patterns: Iterable[str], pages: Sequence[str]) -> list[str]:
    """Return the patterns that match no nav page."""
    return [pattern for pattern in patterns if not resolve([pattern], pages)]


def find_problems(
    *,
    pages: Sequence[str],
    listed: Sequence[str],
    excluded: Sequence[str],
    site_dir: Path,
    site_url: str,
) -> list[str]:
    """Return one line per way llms.txt disagrees with the nav; empty means it passed."""
    outputs = {name: site_dir / name for name in ("llms.txt", "llms-full.txt")}
    missing = [
        f"{name} was not generated or is empty"
        for name, path in outputs.items()
        if not path.is_file() or path.stat().st_size == 0
    ]
    if missing:
        return missing

    if not site_url:
        return ["site_url is not set in mkdocs.yml, so llms.txt links cannot be checked"]
    site_url = site_url if site_url.endswith("/") else site_url + "/"

    index = outputs["llms.txt"].read_text(encoding="utf-8")
    full = outputs["llms-full.txt"].read_text(encoding="utf-8")
    problems: list[str] = []

    for page in pages:
        in_listed, in_excluded = page in listed, page in excluded
        if in_listed and in_excluded:
            problems.append(f"{page} is in both llmstxt.sections and extra.llmstxt_exclude")
        elif not in_listed and not in_excluded:
            problems.append(
                f"{page} is in the nav but in neither llmstxt.sections nor extra.llmstxt_exclude"
            )

    for page in listed:
        if site_url + md_url(page) not in index:
            problems.append(f"{page} is listed but missing from llms.txt")
        copy = site_dir / md_url(page)
        if not copy.is_file():
            problems.append(f"{page} is listed but its Markdown copy {md_url(page)} was not built")
            continue
        marker = _first_content_line(copy)
        if marker is None or marker not in full:
            problems.append(f"{page} is listed but its content is missing from llms-full.txt")

    for target in _LINK_TARGET.findall(index):
        if target.startswith(site_url) and not _link_exists(
            site_dir, target.removeprefix(site_url)
        ):
            problems.append(f"llms.txt links to {target}, which was not built")

    return problems


@mkdocs.plugins.event_priority(-100)
def on_post_build(config: MkDocsConfig) -> None:
    """Abort the build when the generated llms.txt is stale."""
    pages = nav_pages(config.nav or [])
    sections = config.plugins["llmstxt"].config.sections
    listed_patterns = section_patterns(sections)
    excluded_patterns = config.extra.get("llmstxt_exclude", [])
    problems = [
        f"{pattern} in {where} matches no nav page"
        for where, patterns in (
            ("llmstxt.sections", listed_patterns),
            ("extra.llmstxt_exclude", excluded_patterns),
        )
        for pattern in stale_patterns(patterns, pages)
    ]
    problems += find_problems(
        pages=pages,
        listed=resolve(listed_patterns, pages),
        excluded=resolve(excluded_patterns, pages),
        site_dir=Path(config.site_dir),
        site_url=config.site_url or "",
    )
    if problems:
        raise PluginError("\n".join(problems))
