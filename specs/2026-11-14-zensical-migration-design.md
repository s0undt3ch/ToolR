# Zensical migration: design

Replaces MkDocs 1.6 + Material 9 with [Zensical](https://zensical.org/) for the docs build. Material
for MkDocs reaches end of life on 2027-05-05. Evidence: `2026-11-14-zensical-spike-findings.md`
(spike on Zensical 0.1.0 and 0.1.1). This spec was checked against the code at `405fff91`; the
corrections to the spike are in "Corrections to the spike".

## Goal and non-goals

Goal: `zensical build --strict` produces the same page set, API pages, `llms.txt` and
`llms-full.txt` as today, in CI and on Read the Docs, with the llms.txt consistency gate still
failing the build.

Non-goals: converting `mkdocs.yml` to `zensical.toml` (Zensical reads `mkdocs.yml`; revisit at
1.0), a theme redesign, replacing mkdocstrings. Nothing is reported or filed upstream: every gap is worked around, or
accepted, in this repo.

## Corrections to the spike

Checked by grep against the tree.

- The `mkdocs` dependency is in three groups, not one: `dev` (`pyproject.toml:62`, for the gate
  tests), `docs` (`:67`) and `pre-commit` (`:81`). All three need handling.
- `hooks:` also carries `on_post_page`, which injects an absolute `<link rel="alternate"
  type="text/markdown">` into each listed page (`docs/.hooks/llmstxt_gate.py`, `on_post_page`).
  The spike mentions only the gate; the alternate-link injection is lost too, and Zensical's
  built-in link is relative.
- `docs/.hooks/llmstxt_preprocess.py` (strips `details.mkdocstrings-source` from the LLM output) is
  wired through the `llmstxt` plugin's `preprocess:`, not `hooks:`. The spike did not say whether it
  still runs under Zensical; the plan verifies it.
- Orphan pages are caught today by `validation.omitted_files: warn` under `strict: true`
  (`mkdocs.yml`), not by a hook. This is the behaviour gap 2 loses.
- Confirmed as stated: `mkdocs-awesome-nav` is listed (`pyproject.toml:68`) and not configured
  (`plugins:` in `mkdocs.yml` has autorefs, search, mkdocstrings, llmstxt only); the CI step is
  `uv run --no-sync mkdocs build --strict` (`.github/workflows/ci.yml:704`); `.readthedocs.yaml`
  uses the `mkdocs:` key.
- Cooldown: `exclude-newer = "7 days"` (`pyproject.toml:27`). Zensical 0.1.1 was released on
  2026-11-10, so it cannot be locked before 2026-11-17 unless a per-package cutoff is added. That
  escape hatch is for security fixes only (`CLAUDE.md`), so this migration waits for the release to
  age.

## Design

### 1. Toolchain

- `docs` group: add `zensical>=0.1.1,<0.2`; drop `mkdocs`, `mkdocs-material` and
  `mkdocs-awesome-nav`. Keep `mkdocstrings[python]`, `mkdocs-llmstxt`, `markdown-include`
  (Zensical's compatibility layer loads them as MkDocs plugins/extensions; the spike built with
  them). `pymdown-extensions` stays transitive.
- `theme.name: material` stays in `mkdocs.yml`. `theme.icon` is already removed on this branch.
- `dev` and `pre-commit` groups: drop `mkdocs` once the gate no longer imports it (section 2).
- Order of removal: the MkDocs caps from the separate `mkdocs<2` / `mkdocs-material<10` branch
  become moot; this work stacks on that branch and deletes them with the dependencies.

### 2. llms.txt gate becomes a post-build step (gap 1)

Zensical ignores `hooks:`, so nothing in `docs/.hooks/llmstxt_gate.py` runs. Decision: turn the file
into a plain module with a CLI and run it after the build.

- Remove the `mkdocs`, `mkdocs.plugins` and `PluginError` imports and the `on_*` event functions.
  Keep the pure functions (`nav_pages`, `resolve`, `section_patterns`, `md_url`, `find_problems`).
- Add `main(argv)`: loads `mkdocs.yml` with PyYAML plus a constructor for the `!ENV` tag (default
  value, or the environment variable when set), calls the same checks `on_post_build` made today
  (unmatched patterns, `find_problems`), prints one problem per line and exits 1 on any.
- `_LINK_TARGET` (`r"\]\(([^)\s]+)\)"`) captures `<https://...>` for the `(<url>)` form, so the
  `startswith(site_url)` filter skips the link and it goes unchecked, silently. Change it to accept
  an optional angle bracket pair: `r"\]\(<?([^)\s>]+)>?\)"`. Tests cover both forms.
- The gate tests lose their `mkdocs` import and the CI skip guard; `PluginError` expectations
  become exit-code or return-value assertions.
- Run it in the CI docs job right after the build, and in the docs hook set used locally if one
  exists. It is not run on Read the Docs: RTD builds publish, CI gates.

The `--dirty` handling (`on_startup`) is dropped: Zensical has no `--dirty`.

### 3. Orphan pages (gap 2)

Zensical has no orphan warning (zensical#124 is closed as by design). Decision: the
post-build gate checks it. Walk `docs_dir` for `*.md`, subtract `exclude_docs` patterns and nav
pages, and fail if anything is left. About 15 lines in the same module, reuses `nav_pages` and
`fnmatch`. Fallback if a Zensical release adds an equivalent warning: delete the check and rely on
`--strict`.

### 4. llms.txt titles and order (gap 3)

Two output differences from the MkDocs build, both from Zensical's llmstxt handling: link titles
are escaped (`Groups &amp; commands`, ``Using \`ctx\` ``, `arg\_section`) and `reference/*.md`
expands alphabetically instead of in nav order.

Recommended: normalise in the same post-build step, after the gate passes. Unescape HTML entities
and markdown backslash escapes inside link text only, and re-order each section's glob expansion by
nav index. `llms.txt` is a generated file read by agents; the step rewrites it in place.
Link-text-only rewriting keeps URLs and descriptions untouched.

Fallback if the rewrite proves brittle (for example the plugin's output shape changes between
0.1.x releases): generate `llms.txt` ourselves from `llmstxt.sections` and the nav, using the built
Markdown copies for `llms-full.txt`, and stop depending on the plugin's index. Larger, so only on
need.

Acceptance: a diff of `llms.txt` against the MkDocs build differs only in the intended places
(absolute vs relative alternate links excepted, see section 5).

### 5. Alternate links

The absolute `<link rel="alternate">` was injected by `on_post_page`. Recommended: accept
Zensical's built-in relative alternate link if it is present on every listed page and resolves
(verified in the plan). Fallback: inject the absolute tag into the built HTML in the post-build
step, using `md_url` and `site_url`.

### 6. Build entry points

- CI: replace `mkdocs build --strict` with `zensical build --strict` (flag spelling checked against
  `zensical build --help` in the plan), then the gate.
- Read the Docs: drop the `mkdocs:` key; add `build.jobs.build.html` that runs `zensical build` and
  copies the site to `$READTHEDOCS_OUTPUT/html`. The existing `install` job
  (`uv sync --frozen --group docs`) is unchanged.
- `CLAUDE.md` "Scale verification" bullet and any contributor text naming `mkdocs build --strict`
  are updated to the new command.

### 7. Cleanup

- Remove `mkdocs-awesome-nav` (unused).
- Remove `exclude_docs`/`validation` keys only if the build proves them ignored; otherwise keep.
- Delete `watch:` only if ignored by `zensical serve`; low value either way.

## Risks

- Zensical is 0.1.x: the pin `<0.2` bounds behaviour changes; Renovate raises each minor as its
  own PR.
- Silent-ignore is the failure mode (`hooks:`, `theme.icon` crash aside). Every setting that
  matters gets a check that fails, not a visual inspection; see the plan's final comparison task.
- RTD `build.jobs` override syntax is not verified here; the plan checks it against the RTD docs
  and a PR build before merge.

## Out of scope

`zensical.toml`, and any content changes to the docs pages.
