"""Tests for the MkDocs llms.txt staleness gate."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING

import pytest
from mkdocs.exceptions import PluginError

if TYPE_CHECKING:
    from collections.abc import Callable

# docs/.hooks isn't a package, so load the module by path.
_HOOK = Path(__file__).parents[1] / "docs/.hooks/llmstxt_gate.py"
_spec = importlib.util.spec_from_file_location("llmstxt_gate", _HOOK)
assert _spec is not None
assert _spec.loader is not None
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)

SITE_URL = "https://example.test/stable/"


@pytest.fixture
def make_site(tmp_path: Path) -> Callable[..., Path]:
    def _make(
        *,
        pages: tuple[str, ...] = ("quickstart.md",),
        links: tuple[str, ...] | None = None,
        full: str | None = None,
        write_full: bool = True,
    ) -> Path:
        site = tmp_path / "site"
        site.mkdir()
        if links is None:
            links = tuple(SITE_URL + gate.md_url(p) for p in pages)
        (site / "llms.txt").write_text(
            "# T\n\n" + "\n".join(f"- [x]({link}): d" for link in links) + "\n"
        )
        if write_full:
            (site / "llms-full.txt").write_text(
                full if full is not None else "".join(f"# {p}\n\nBody of {p}\n" for p in pages)
            )
        for page in pages:
            target = site / gate.md_url(page)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(f"# {page}\n\nBody of {page}\n")
        return site

    return _make


def test_nav_pages_flattens_and_skips_urls() -> None:
    nav = [
        {"Home": "index.md"},
        {"Guide": [{"Groups": "writing-commands/groups.md"}, {"Ext": "https://example.test/x.md"}]},
        "cli.md",
        {"Site": "http://example.test/y"},
    ]
    assert gate.nav_pages(nav) == ["index.md", "writing-commands/groups.md", "cli.md"]


def test_resolve_ignores_non_nav_matches() -> None:
    pages = ["reference/a.md", "reference/sub/x.md", "cli.md"]
    # fnmatch's "*" crosses "/", the same as the llmstxt plugin's own expansion.
    assert gate.resolve(["reference/*.md"], pages) == ["reference/a.md", "reference/sub/x.md"]
    assert gate.resolve(["reference/*.md", "elsewhere/other.md"], pages) == [
        "reference/a.md",
        "reference/sub/x.md",
    ]
    assert gate.resolve(["cli.md"], pages) == ["cli.md"]


def test_stale_patterns_reports_patterns_matching_no_page() -> None:
    assert gate.stale_patterns(["cli.md", "gone.md", "old/*.md"], ["cli.md"]) == [
        "gone.md",
        "old/*.md",
    ]


def test_section_patterns_flattens_str_and_dict_entries() -> None:
    sections = {
        "A": [{"index.md": "Overview"}, "reference/*.md"],
        "B": [{"cli.md": "CLI"}],
    }
    assert gate.section_patterns(sections) == ["index.md", "reference/*.md", "cli.md"]


@pytest.mark.parametrize(
    ("page", "expected"),
    [
        ("index.md", "index.md"),
        ("a/index.md", "a/index.md"),
        ("a/b.md", "a/b/index.md"),
    ],
)
def test_md_url(page: str, expected: str) -> None:
    assert gate.md_url(page) == expected


def test_clean_site_has_no_problems(make_site: Callable[..., Path]) -> None:
    site = make_site(pages=("quickstart.md", "a/b.md"))
    problems = gate.find_problems(
        pages=["quickstart.md", "a/b.md", "migration.md"],
        listed=["quickstart.md", "a/b.md"],
        excluded=["migration.md"],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert problems == []


def test_missing_output_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site(write_full=False)
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert len(problems) == 1
    assert "llms-full.txt" in problems[0]


def test_unclassified_nav_page_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site()
    problems = gate.find_problems(
        pages=["quickstart.md", "orphan.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert any("orphan.md" in p for p in problems)


def test_page_in_both_lists_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site()
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=["quickstart.md"],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert any("quickstart.md" in p and "both" in p for p in problems)


def test_listed_page_missing_from_index_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site(links=())
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert any("quickstart.md" in p and "llms.txt" in p for p in problems)


def test_listed_page_missing_from_full_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site(full="# Something else\n")
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert any("quickstart.md" in p and "llms-full.txt" in p for p in problems)


def test_dead_link_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site(links=(SITE_URL + "quickstart/index.md", SITE_URL + "gone/index.md"))
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert any("gone/index.md" in p for p in problems)


def test_on_post_build_raises_plugin_error(make_site: Callable[..., Path]) -> None:
    site = make_site()
    config = SimpleNamespace(
        site_dir=str(site),
        site_url=SITE_URL,
        nav=[{"Q": "quickstart.md"}, {"M": "migration.md"}],
        plugins={
            "llmstxt": SimpleNamespace(
                config=SimpleNamespace(sections={"S": [{"quickstart.md": "d"}]})
            )
        },
        extra={"llmstxt_exclude": []},
    )
    with pytest.raises(PluginError, match=r"migration\.md"):
        gate.on_post_build(config)


def test_live_rendered_page_link_is_not_reported(make_site: Callable[..., Path]) -> None:
    site = make_site(links=(SITE_URL + "quickstart/index.md", SITE_URL + "skills/"))
    (site / "skills").mkdir()
    (site / "skills/index.html").write_text("<html></html>")
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert problems == []


def test_dead_rendered_page_link_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site(links=(SITE_URL + "quickstart/index.md", SITE_URL + "gone/"))
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert any("gone/" in p for p in problems)


def test_missing_markdown_copy_is_reported_distinctly(make_site: Callable[..., Path]) -> None:
    site = make_site()
    (site / "quickstart/index.md").unlink()
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert any("quickstart.md" in p and "was not built" in p for p in problems)
    assert not any("missing from llms-full.txt" in p for p in problems)


def test_missing_llms_txt_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site()
    (site / "llms.txt").unlink()
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert len(problems) == 1
    assert "llms.txt" in problems[0]


def test_empty_llms_full_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site(full="")
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL,
    )
    assert len(problems) == 1
    assert "llms-full.txt" in problems[0]


def test_unset_site_url_is_reported(make_site: Callable[..., Path]) -> None:
    site = make_site()
    problems = gate.find_problems(
        pages=["quickstart.md"], listed=["quickstart.md"], excluded=[], site_dir=site, site_url=""
    )
    assert len(problems) == 1
    assert "site_url" in problems[0]


def test_site_url_without_trailing_slash_is_normalised(make_site: Callable[..., Path]) -> None:
    site = make_site()
    problems = gate.find_problems(
        pages=["quickstart.md"],
        listed=["quickstart.md"],
        excluded=[],
        site_dir=site,
        site_url=SITE_URL.rstrip("/"),
    )
    assert problems == []


def test_on_post_build_reports_stale_patterns(make_site: Callable[..., Path]) -> None:
    site = make_site()
    config = SimpleNamespace(
        site_dir=str(site),
        site_url=SITE_URL,
        nav=[{"Q": "quickstart.md"}],
        plugins={
            "llmstxt": SimpleNamespace(
                config=SimpleNamespace(sections={"S": [{"quickstart.md": "d"}]})
            )
        },
        extra={"llmstxt_exclude": ["deleted.md"]},
    )
    with pytest.raises(
        PluginError, match=r"deleted\.md in extra\.llmstxt_exclude matches no nav page"
    ):
        gate.on_post_build(config)


def test_alternate_link_for_listed_page() -> None:
    link = gate.alternate_link("quickstart.md", ["quickstart.md"], "https://example.test/stable")
    assert link == (
        '<link rel="alternate" type="text/markdown" '
        'href="https://example.test/stable/quickstart/index.md">'
    )


def test_alternate_link_none_for_unlisted_page() -> None:
    assert gate.alternate_link("internals.md", ["quickstart.md"], SITE_URL) is None


def _post_page_config() -> SimpleNamespace:
    return SimpleNamespace(
        nav=["quickstart.md", "internals.md"],
        site_url=SITE_URL,
        plugins={
            "llmstxt": SimpleNamespace(
                config=SimpleNamespace(sections={"S": [{"quickstart.md": "d"}]})
            )
        },
    )


@pytest.mark.parametrize(
    ("src", "tagged"),
    [("quickstart.md", True), ("internals.md", False)],
    ids=["listed", "excluded"],
)
def test_on_post_page_inserts_link_before_head_close(src: str, tagged: bool) -> None:
    html = "<html><head><title>T</title></head><body></body></html>"
    page = SimpleNamespace(file=SimpleNamespace(src_uri=src))
    out = gate.on_post_page(html, page=page, config=_post_page_config())
    if tagged:
        tag = f'<link rel="alternate" type="text/markdown" href="{SITE_URL}quickstart/index.md">'
        assert out == f"<html><head><title>T</title>{tag}</head><body></body></html>"
    else:
        assert out == html


def _broken_build_config(site: Path) -> SimpleNamespace:
    return SimpleNamespace(
        site_dir=str(site),
        site_url=SITE_URL,
        nav=[{"Q": "quickstart.md"}, {"M": "migration.md"}],
        plugins={
            "llmstxt": SimpleNamespace(
                config=SimpleNamespace(sections={"S": [{"quickstart.md": "d"}]})
            )
        },
        extra={"llmstxt_exclude": []},
    )


def test_dirty_build_skips_the_gate_with_a_warning(
    make_site: Callable[..., Path], caplog: pytest.LogCaptureFixture
) -> None:
    config = _broken_build_config(make_site())
    gate.on_startup(command="build", dirty=True)
    try:
        with caplog.at_level("WARNING"):
            gate.on_post_build(config)
    finally:
        gate.on_startup(command="build", dirty=False)
    assert "dirty" in caplog.text


def test_clean_build_still_raises_after_a_dirty_one(make_site: Callable[..., Path]) -> None:
    config = _broken_build_config(make_site())
    gate.on_startup(command="build", dirty=True)
    gate.on_startup(command="build", dirty=False)
    with pytest.raises(PluginError, match=r"migration\.md"):
        gate.on_post_build(config)


def test_on_post_build_names_a_missing_plugin(make_site: Callable[..., Path]) -> None:
    config = _broken_build_config(make_site())
    config.plugins = {}
    with pytest.raises(PluginError, match="llmstxt plugin is not configured"):
        gate.on_post_build(config)


def test_on_post_page_names_a_missing_plugin() -> None:
    config = _post_page_config()
    config.plugins = {}
    page = SimpleNamespace(file=SimpleNamespace(src_uri="quickstart.md"))
    with pytest.raises(PluginError, match="llmstxt plugin is not configured"):
        gate.on_post_page("<head></head>", page=page, config=config)
