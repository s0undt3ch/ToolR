# Zensical migration: plan

Implements `2026-11-14-zensical-migration-design.md`. Doc-only plus tooling, no Rust.

## Stacking

Stack this work on the branch that caps `mkdocs<2` / `mkdocs-material<10` (create with
`git-spice branch create`; `<base>` for `cargo xtask`/release-notes checks is that branch, not
`main`). Task 1 removes those caps along with the packages. Deadline: Material EOL 2027-05-05.

## Preconditions

- `unset VIRTUAL_ENV` before every `uv` command.
- Do not start Task 1 before 2026-11-17: Zensical 0.1.1 (2026-11-10) is inside the 7-day
  `exclude-newer` window. Tasks 2 and 3 do not need the new package and can go first.
- Python tests on 3.11 and the newest version (`uv run --python 3.11 pytest ...`).

## Tasks

1. **Refactor the gate to a mkdocs-free module (design 2).**
   Remove the `mkdocs` imports and `on_*` functions from `docs/.hooks/llmstxt_gate.py`; add
   `main()` with an `!ENV`-aware YAML loader. Update `tests/docs/test_llmstxt_gate.py` (drop the
   `PluginError` import and CI skip guard). Verify: `uv run pytest tests/docs -p no:randomly -q`
   and `uv run pytest tests/docs -q`.

2. **Accept the `(<url>)` link form (design 2).**
   Change `_LINK_TARGET` to `r"\]\(<?([^)\s>]+)>?\)"`. Add tests: a dangling `](<url>)` link
   fails, a valid one passes, the plain form still behaves. Write the failing test first.
   Verify: `uv run pytest tests/docs -q`.

3. **Orphan-page check (design 3).**
   Add `find_orphans(docs_dir, exclude_docs, pages)` and call it from `main()`. Tests: an extra
   `.md` fails, an `exclude_docs` match passes, nav pages pass. Verify:
   `uv run pytest tests/docs -q`.

4. **Swap dependencies (design 1).**
   In `pyproject.toml`: `docs` group gets `zensical>=0.1.1,<0.2` and loses `mkdocs`,
   `mkdocs-material`, `mkdocs-awesome-nav`; `dev` and `pre-commit` lose `mkdocs`; remove the
   `mkdocs<2`/`mkdocs-material<10` caps from the base branch. Then
   `unset VIRTUAL_ENV; uv lock && uv sync --all-extras --dev --group docs`. Verify:
   `uv run python -c "import zensical"` and `git diff uv.lock | grep -c zensical`; confirm the
   lock resolved without a per-package cutoff.

5. **Local build and gate (design 2, 6).**
   Check flags with `uv run zensical build --help`. Run
   `uv run zensical build --strict` (spelling per help), then
   `uv run python docs/.hooks/llmstxt_gate.py`. Verify: both exit 0; then remove a page from
   `llmstxt.sections` in a scratch edit and confirm the gate exits 1 (revert after).

6. **llms.txt titles and order (design 4).**
   Recommended: add `normalise_llms_txt()` to the gate module, run from `main()` after the checks
   pass: unescape entities and backslash escapes in link text only, reorder glob-expanded entries
   by nav index. Tests with a fixture `llms.txt` containing `Groups &amp; commands`,
   ``Using \`ctx\` `` and alphabetical `reference/*.md`. Verify: `uv run pytest tests/docs -q`, then
   build and diff against a MkDocs build of the same tree: titles and order match. Fallback if the
   rewrite is brittle: generate `llms.txt` from `llmstxt.sections` and the nav in the gate module
   instead; keep the plugin only for the per-page Markdown copies and `llms-full.txt`.

7. **Alternate links and preprocess (design 5, corrections).**
   Check a built listed page: `grep -c 'rel="alternate"' site/quickstart/index.html`, that the
   href resolves, and that `site/llms-full.txt` has no `mkdocstrings-source` content (proves
   `llmstxt_preprocess.py` still runs). If the link is absent or unusable, inject the absolute tag
   in the post-build step with a test. Verify: the same checks on `site/`.

8. **Parity comparison.**
   Build with MkDocs on the base branch and Zensical here; compare `find site -name '*.html'`
   lists, `llms.txt`, and the mkdocstrings cross-reference counts from the spike. Verify: page
   sets equal; llms.txt diff only in the accepted places.

9. **CI (design 6).**
   In `.github/workflows/ci.yml` replace the `Build MkDocs site` step with `Build docs site`
   running `uv run --no-sync zensical build` (strict spelling per Task 5) and a following
   `Check llms.txt` step running `uv run --no-sync python docs/.hooks/llmstxt_gate.py`. Verify:
   `prek run --all-files` (workflow hooks) and the docs job green on the PR.

10. **Read the Docs (design 6).**
    Edit `.readthedocs.yaml`: remove the `mkdocs:` block; add `build.jobs.build.html` with
    `uv run zensical build` and `mkdir -p "$READTHEDOCS_OUTPUT" && cp -r site "$READTHEDOCS_OUTPUT/html"`.
    Check the key shape against the RTD config reference before committing. Verify: RTD PR build
    for the branch succeeds and serves the pages with the canonical `site_url`; this needs the PR
    preview and cannot be checked locally.

11. **Docs and cleanup (design 7).**
    Update `CLAUDE.md` ("Scale verification" bullet) and `.gitignore` comment to the new command;
    grep for leftovers: `git grep -n -i 'mkdocs build\|mkdocs-material\|awesome-nav'`. Decide on
    `exclude_docs`, `validation`, `watch` per design 7 by testing whether each is honoured.
    Verify: `prek run --all-files` and `uv run zensical build --strict`.

12. **Release note and archive.**
    `toolr release-notes add --text "..."` only if contributor-visible (docs tooling change: add
    one line); otherwise `Release-Notes: skip`. Last commit: `git mv` both specs and the spike
    findings to `specs/archive/2026/`. Verify:
    `toolr release-notes check --base <stack base branch>`.

## Final verification

`mise run test` (touched Python), `prek run --all-files`, a clean `zensical build` plus gate, and
the RTD preview build.
