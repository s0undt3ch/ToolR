# llms.txt Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve a curated `llms.txt` and `llms-full.txt` from the Read the Docs root. A build gate
stops them drifting from the docs.

**Architecture:** The `mkdocs-llmstxt` plugin writes both files plus per-page `.md` copies into
`site/`. A plugin `preprocess` script strips mkdocstrings source dumps. A MkDocs hook runs after
the plugin and fails `mkdocs build --strict` when the output and the nav disagree. Because the gate
is a hook, it runs everywhere the site builds: locally, in CI and on Read the Docs. The GitHub Pages
deploy is removed.

**Tech Stack:** MkDocs 1.6, mkdocs-material, mkdocstrings, `mkdocs-llmstxt` 0.5.x, pytest.

**Spec:** `specs/2026-11-02-llms-txt-design.md`

## Global Constraints

- `site_url: !ENV [READTHEDOCS_CANONICAL_URL, "https://toolr.readthedocs.io/stable/"]`. RTD sets
  this variable ([RTD canonical URL guide](https://dokk.org/documentation/readthedocs/10.11.0/_sources/guides/canonical-urls.rst.txt)).
  No `base_url`.
- Listed in `llms.txt`: Home, Quickstart, Installation (overview and mise), "How toolr is laid
  out", every `writing-commands/` nav page, Project configuration, CLI reference, every
  `reference/` page. Optional: Third-party packages, Agent skills.
- Excluded: Internals (all), Migration, Contributing, Changelog. External nav links are ignored.
- `llms-full.txt` expands exactly the listed pages.
- `llms.txt` carries no install commands. The skills note links to the Agent skills page.
- Page descriptions are a few words each. `markdown_description` says what toolr is, with no
  features and no versions.
- `mkdocs-llmstxt` goes in the `docs` dependency group. The normal 7-day `exclude-newer` cooldown
  applies. 0.5.1 is current and old enough.
- No new files under `tools/`. toolr scans that directory for commands.
- No `paddle` and no absolute home paths in committed files.
- Conventional Commits. No `Co-Authored-By` trailer.

## Review Focus

1. **A glob that reaches `writing-commands/files/`.** `fnmatch` `*` crosses `/`, so
   `writing-commands/*.md` also matches the excluded snippet includes. In a trial build that gave
   `Page URI 'writing-commands/files/supported-types.md' not found`, a warning that fails
   `--strict`. List `writing-commands/` pages explicitly, in nav order (Task 1). The gate must
   resolve globs against nav pages only (Task 2, `test_resolve_ignores_non_nav_matches`).
2. **A page added to the nav and left out of both lists.** The gate must fail and name the page
   (Task 2, `test_unclassified_nav_page_is_reported`).
3. **A page that is both listed and excluded,** for example through overlapping globs. The gate
   must name it (Task 2, `test_page_in_both_lists_is_reported`).
4. **A dead link in `llms.txt`** after a plugin upgrade changes its output layout. The gate must
   catch every `site_url` link that has no file under `site/` (Task 2,
   `test_dead_link_is_reported`).
5. **The Home page entry in `llms-full.txt`.** In the trial build it rendered as an empty `#`
   heading, because `docs/index.md` has no H1 the plugin picks up. Fix it in Task 1 and check it
   in Task 1 Step 5.

---

## File Structure

- `mkdocs.yml`: adds `site_url`, the `llmstxt` plugin config, `extra.llmstxt_exclude` and
  `hooks:`.
- `docs/.hooks/llmstxt_preprocess.py`: the plugin `preprocess` script. MkDocs ignores dot-dirs
  under `docs_dir`, so nothing here is published.
- `docs/.hooks/llmstxt_gate.py`: the gate hook. Pure check functions plus `on_post_build`.
- `tests/test_docs_llmstxt_gate.py`: unit tests for the gate functions.
- `pyproject.toml`, `uv.lock`: the `docs` group gains `mkdocs-llmstxt`.
- `docs/skills.md`: a new section that points at the two files.
- `.github/workflows/ci.yml`: the docs job drops the Pages deploy.
- `release-notes/<generated>.md`: one note.

### Task 1: Generate the files

**Files:**

- Modify: `mkdocs.yml`, `pyproject.toml`, `uv.lock`
- Create: `docs/.hooks/llmstxt_preprocess.py`
- Possibly modify: `docs/index.md` (Review Focus 5)

**Interfaces:**

- Produces: `config.plugins["llmstxt"].config.sections` (a mapping from section title to a list
  of `str | dict[str, str]`) and `config.extra["llmstxt_exclude"]` (`list[str]` of fnmatch
  patterns). Task 2 reads both.
- [ ] **Step 1: Confirm a clean baseline.** Run `uv run mkdocs build --strict`. Expected: exit 0.
  If it fails on `main`, stop and report it. It is not this task's bug.
- [ ] **Step 2: Add the dependency.** Add `"mkdocs-llmstxt>=0.5.1"` to the `docs` group, then run
  `uv lock`. Expected: `uv.lock` gains `mkdocs-llmstxt` and its deps (`beautifulsoup4`,
  `markdownify`, `mdformat`).
- [ ] **Step 3: Configure it.** In `mkdocs.yml`:
    - Add the `site_url` line from Global Constraints under `site_name`.
    - Add `- llmstxt:` after `mkdocstrings` under `plugins:`, with `full_output: llms-full.txt`,
      `preprocess: docs/.hooks/llmstxt_preprocess.py`, `markdown_description`, and `sections:` with
      the keys `Start here`, `Writing commands`, `Reference` and `Optional`. Use explicit paths for
      `writing-commands/`, in nav order. Use `reference/*.md` for the API pages, after
      `project-config.md` and `cli.md`.
    - Add `extra: {llmstxt_exclude: [internals/*.md, migration.md, contributing.md, changelog.md]}`
      directly below the plugin block, with a one-line comment saying both lists are edited
      together.
    - Write `markdown_description` in two or three sentences. Toolr is a CLI that finds Python
      commands in a repo's `tools/` directory. If your harness supports skills, install the toolr
      skills from the Agent skills page.
- [ ] **Step 4: Write `preprocess(soup: BeautifulSoup, output: str) -> None`.** It removes every
  `details.mkdocstrings-source` element. Those elements hold the mkdocstrings source dumps. The
  trial build had 25 of them in `llms-full.txt`.
- [ ] **Step 5: Build and inspect.** Run `uv run mkdocs build --strict`. Expected: exit 0, and:
    - `grep -c "Source code in" site/llms-full.txt` prints `0`;
    - `site/llms.txt` links look like `https://toolr.readthedocs.io/stable/quickstart/index.md`;
    - `site/llms.txt` has no `internals/` link.

  Check the Home entry in `site/llms-full.txt`. If its heading is empty (`# `), give
  `docs/index.md` a `title:` in front matter or an H1, and rebuild.
- [ ] **Step 6: Commit.** `git add mkdocs.yml pyproject.toml uv.lock docs/.hooks/llmstxt_preprocess.py`
  (and `docs/index.md` if you changed it). Message:
  `feat(docs): generate llms.txt and llms-full.txt`.

### Task 2: The staleness gate

**Files:**

- Create: `docs/.hooks/llmstxt_gate.py`
- Create: `tests/test_docs_llmstxt_gate.py`
- Modify: `mkdocs.yml` (`hooks: [docs/.hooks/llmstxt_gate.py]`)

**Interfaces:**

- Consumes: the Task 1 config keys.
- Produces, in `docs/.hooks/llmstxt_gate.py`:
    - `nav_pages(nav: list[Any]) -> list[str]`: every `.md` source path in a MkDocs nav, in order.
      Skips `http(s)://` entries.
    - `resolve(patterns: Iterable[str], pages: Sequence[str]) -> list[str]`: the nav pages matching
      any fnmatch pattern, in nav order. It never returns a path that isn't in `pages`.
    - `section_patterns(sections: Mapping[str, Sequence[str | Mapping[str, str]]]) -> list[str]`:
      flattens the plugin's `sections` into patterns. A dict entry contributes its single key.
    - `md_url(page: str) -> str`: `index.md` → `index.md`, `a/index.md` → `a/index.md`,
      `a/b.md` → `a/b/index.md`.
    - `find_problems(*, pages, listed, excluded, site_dir, site_url) -> list[str]`, where the first
      three are `Sequence[str]`, `site_dir` is `Path` and `site_url` is `str`. It returns one
      human-readable line per problem. An empty list means it passed.
    - `on_post_build(config: MkDocsConfig) -> None`, decorated
      `@mkdocs.plugins.event_priority(-100)` so it runs after the plugin writes its files. It raises
      `mkdocs.exceptions.PluginError` with the problems joined by newlines.

`find_problems` checks, in this order:

1. `llms.txt` and `llms-full.txt` exist under `site_dir` and are not empty. If either is missing,
   return at once.
2. Each page in `pages` is in exactly one of `listed` and `excluded`.
3. Each listed page has `site_url + md_url(page)` in `llms.txt`. The first non-empty, non-heading
   line of `site_dir / md_url(page)` appears in `llms-full.txt`.
4. Each Markdown link target in `llms.txt` that starts with `site_url` names a file that exists
   under `site_dir`.

- [ ] **Step 1: Write the failing tests.** The test module loads the hook once at the top with
  `importlib.util.spec_from_file_location`, from `Path(__file__).parents[1] / "docs/.hooks/llmstxt_gate.py"`.
  Add a one-line comment saying why: `docs/.hooks` isn't a package. Add a factory fixture
  `make_site(tmp_path)` that writes `llms.txt`, `llms-full.txt` and per-page `.md` files, and
  returns the site dir. Tests:
    - `test_nav_pages_flattens_and_skips_urls`: a nested nav with an `https://…` entry gives just
      the `.md` paths, in order.
    - `test_resolve_ignores_non_nav_matches`: `resolve(["writing-commands/*.md"], ["writing-commands/a.md"])`
      gives `["writing-commands/a.md"]`. It does so even though `writing-commands/files/x.md` would
      match the pattern.
    - `test_section_patterns_flattens_str_and_dict_entries`
    - `test_md_url` is parametrised over the three shapes above.
    - `test_clean_site_has_no_problems`
    - `test_missing_output_is_reported`: there is no `llms-full.txt`, so the result names it.
    - `test_unclassified_nav_page_is_reported`: the result contains the page path.
    - `test_page_in_both_lists_is_reported`
    - `test_listed_page_missing_from_index_is_reported`
    - `test_listed_page_missing_from_full_is_reported`
    - `test_dead_link_is_reported`: `llms.txt` links to `…/gone/index.md` with no file behind it.
    - `test_on_post_build_raises_plugin_error`: a minimal config stub whose site fails one check.
- [ ] **Step 2: Run them.** Run `uv run pytest tests/test_docs_llmstxt_gate.py -v`. Expected: the
  tests fail on import, because the module doesn't exist yet.
- [ ] **Step 3: Implement the functions in `docs/.hooks/llmstxt_gate.py`.** Use `fnmatch.fnmatch`
  and a regex for Markdown link targets.
- [ ] **Step 4: Run them.** Same command. Expected: all pass.
- [ ] **Step 5: Wire the hook.** Add `hooks: [docs/.hooks/llmstxt_gate.py]` to `mkdocs.yml`, then
  run `uv run mkdocs build --strict`. Expected: exit 0.
- [ ] **Step 6: Prove the gate bites.** Temporarily remove `migration.md` from
  `extra.llmstxt_exclude`, then rebuild. Expected: the build aborts, and the error names
  `migration.md`. Revert the change.
- [ ] **Step 7: Commit.** `git add docs/.hooks/llmstxt_gate.py tests/test_docs_llmstxt_gate.py mkdocs.yml`.
  Message: `feat(docs): fail the build when llms.txt drifts from the nav`.

### Task 3: Document it, drop GitHub Pages, queue the note

**Files:**

- Modify: `docs/skills.md`, `.github/workflows/ci.yml`
- Create: a release note, via the tool
- [ ] **Step 1: Add a section to `docs/skills.md`.** Title it `## Without the skills: llms.txt`
  and put it after `## Managing installed skills`. In a few sentences: agents without the skills
  can start from [`llms.txt`](https://toolr.readthedocs.io/llms.txt), a curated index, or from
  [`llms-full.txt`](https://toolr.readthedocs.io/llms-full.txt), the same pages in one file.
  Both are served from the docs root and built from the stable docs. Link both by absolute URL.
- [ ] **Step 2: Remove the Pages deploy.** In the `docs` job of `.github/workflows/ci.yml`:
    - delete the `Deploy to GitHub Pages` step;
    - change `permissions: contents: write` to `contents: read`;
    - rename the job from `Publish Docs` to `Build Docs`.

  Then check that nothing else refers to the old name:
  `git grep -n "Publish Docs" -- .github`. Expected: no output.
- [ ] **Step 3: Queue the release note.** Run `toolr release-notes add --text "…"` with this text:

  > The docs now serve llms.txt and llms-full.txt from the Read the Docs root, so agents without
  > the toolr skills get a curated Markdown index of the docs. The GitHub Pages copy of the docs
  > is no longer published; use toolr.readthedocs.io.

  Then run `toolr release-notes check --base main`. Expected: pass.
- [ ] **Step 4: Verify.** Run `uv run mkdocs build --strict` and `prek run --all-files`.
  Expected: both pass. Per `CLAUDE.md`, this PR touches Python, so also run `mise run test`. It
  must pass. Poll its output every 30–60 seconds.
- [ ] **Step 5: Commit.** `git add docs/skills.md .github/workflows/ci.yml release-notes/<new file>`.
  Message: `docs(skills): point agents without skills at llms.txt`. Make the Pages removal its own
  commit: `ci(docs): stop deploying docs to GitHub Pages`.

### Task 4: Archive the spec

- [ ] **Step 1: Run
  `git mv specs/2026-11-02-llms-txt-design.md specs/2026-11-02-llms-txt-plan.md specs/archive/2026/`.**
  Commit `docs(specs): archive llms.txt design and plan`. This is the last commit before the PR
  opens.

### After merge (needs the maintainer's go-ahead)

- [ ] Unpublish GitHub Pages with `gh api -X DELETE repos/s0undt3ch/ToolR/pages`, or from the repo
  settings. Ask first. Outsiders can see this change.
- [ ] After the next tagged release builds as `stable`, check
  `curl -s -o /dev/null -w '%{http_code}' https://toolr.readthedocs.io/llms.txt`. Expected: `200`.
