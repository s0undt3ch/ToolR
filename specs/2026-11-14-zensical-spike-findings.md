# Zensical spike findings

Spike for WP3 of the 2026-11-13 audit (finding U-17): Material for MkDocs reaches end of life on
2027-05-05. Question: can the docs move from MkDocs to [Zensical](https://zensical.org/)?

Tested Zensical 0.1.0, then 0.1.1 (released 2026-11-10). Baseline: MkDocs 1.6.1 + Material 9.8.0,
`strict: true`.

## Config change needed

`theme.icon: imgs/toolr.png` crashes Zensical with
`ValueError: Failed to cast key 'icon' to <class 'dict'>`. MkDocs ignores the key, so this branch
deletes it. No other `mkdocs.yml` change was needed for a build.

## What works

- Same HTML page set as MkDocs.
- mkdocstrings API pages. External cross-references match: 190 to docs.python.org, 18 to Rich.
- README include (`markdown_include`) and pymdownx snippets.
- `llms-full.txt`: about 10% smaller, no words lost (formatting differences only, such as escaped
  underscores).
- `site_url: !ENV READTHEDOCS_CANONICAL_URL` feeds the canonical link and `llms.txt` links.
- Strict mode fails on a missing page and on a missing anchor (MkDocs only logs a missing anchor at
  INFO).
- Build time about 2 s against about 5 s.

## Gaps

1. `hooks:` is silently ignored, so `docs/.hooks/llmstxt_gate.py` never runs. Removing a page from
   `llmstxt.sections` passes in Zensical and fails in MkDocs. Zensical's migration guide lists
   `hooks` as "not yet supported". The built-in alternate link is relative (`./index.md`), not
   absolute. Workaround: run `find_problems()` from the gate as a post-build step; its regex
   `_LINK_TARGET` must also accept the `(<url>)` link form.
2. A page outside an explicit nav no longer fails the build. Upstream
   [zensical#124](https://github.com/zensical/zensical/issues/124) is closed; the maintainer calls
   the missing warning "by design".
3. `llms.txt` link titles are escaped (`Groups &amp; commands`, ``Using \`ctx\` ``, `arg\_section`,
   `command\_group`, `current\_context`), and glob entries such as `reference/*.md` sort
   alphabetically instead of in nav order. No upstream issue found.
4. `theme.icon` given as a string crashes the build (see above). No upstream issue found.

All four still reproduce on 0.1.1.

## Other migration work

- `.readthedocs.yaml` builds through the `mkdocs:` key; Zensical needs a `build.jobs` override that
  writes to `$READTHEDOCS_OUTPUT/html`.
- `.github/workflows/ci.yml` runs `mkdocs build`.
- The `pre-commit` dependency group pulls `mkdocs` so mypy can check the hooks.
- `mkdocs-awesome-nav` is in the docs group but not configured anywhere.
- Pin Zensical below 0.2 while it is pre-1.0.

## Status

Not decided. Upstream reports for gaps 3 and 4, a change request for gap 1 (strict should fail on
unsupported settings) and a comment on #124 for gap 2 are proposed, not filed.
