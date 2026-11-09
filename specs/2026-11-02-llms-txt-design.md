# llms.txt for the toolr docs

## Intent

Give an agent that has no toolr skills installed a way into the toolr docs that isn't rendered HTML.
The agent arrives cold: a user said "use toolr", or the repo already has a `tools/` directory. It
needs to do two things, in order:

1. Work out what toolr is: a CLI that finds Python commands in a repo's `tools/` directory, plus
   the mental model of groups, commands, `ctx` and the static manifest. That is enough to decide
   toolr fits and to explain it to the user.
2. Write a working `tools/*.py` command first time: decorators, arguments, docstring-driven
   `--help`, `ctx.run`, testing.

The [llms.txt](https://llmstxt.org/) convention fits this. It is a Markdown index at the docs root
that agents fetch before anything else. [herdr's `llms.txt`](https://herdr.dev/llms.txt) is the
prior art: a short curated index with one-line descriptions, plus a full bundle.

## Goals

- Serve `https://toolr.readthedocs.io/llms.txt` and `https://toolr.readthedocs.io/llms-full.txt`.
- `llms.txt` is a curated index. It covers what toolr is, a pointer to the skills, and the pages
  an agent needs to write a command.
- `llms-full.txt` holds the same pages, expanded into one file.
- `llms.txt` suggests the toolr skills to agents whose harness supports them, so the next session
  starts with them installed.
- The Agent skills page (`docs/skills.md`) says the two files exist.
- The build fails if the files stop being produced.

## Non-goals

- An abridged bundle such as herdr's `llms-small.txt`. Nobody has asked for one, and it is another
  file to keep in shape.
- Changing how Read the Docs builds or serves the site.
- Keeping the GitHub Pages copy. See *Read the Docs is the only docs host*.

## Anti-goals

- **Dumping the whole nav into the index.** If internals, migration and the changelog sit next to
  the Quickstart, the agent reads the wrong thing first.
- **Duplicating skill install instructions.** `docs/skills.md` stays the source of truth for
  install commands. `llms.txt` links to it and copies nothing.
- **Files that quietly drift from the docs.** A new page that never reaches `llms.txt`, or a
  removed page that leaves a dead link, counts as a failure even if the build is green.
- **Links that point to a moving target.** The root file must link to the docs for the version it
  was built from, not to `latest`.

## Current state

- Docs are MkDocs (Material) with mkdocstrings, built by Read the Docs from `.readthedocs.yml`.
  The RTD install step is `uv sync --frozen --group docs`.
- `mkdocs.yml` has no `site_url`.
- `https://toolr.readthedocs.io/` redirects to `/stable/`, so `stable` is the RTD default version.
  The URLs have no language segment (`/stable/quickstart/`, not `/en/stable/quickstart/`).
- `https://toolr.readthedocs.io/llms.txt` returns 404 today.
- CI (`.github/workflows/ci.yml`, docs job) also runs `mkdocs build --strict` and deploys `site/`
  to GitHub Pages (`https://s0undt3ch.github.io/ToolR/`) on `main`. That copy predates Read the
  Docs. Older, archived specs still link to it.

## Decided

### Generator: `mkdocs-llmstxt`

Use the [`mkdocs-llmstxt`](https://github.com/pawamoy/mkdocs-llmstxt) plugin. It converts the
built HTML back to Markdown, so the mkdocstrings API pages come through as real API text. It
writes `llms.txt` from a `sections:` map and `llms-full.txt` from `full_output:`.

The plugin also writes a Markdown copy of each listed page next to its HTML, and `llms.txt`
links to those copies (for example `…/stable/quickstart/index.md`), not to the HTML pages. That is
what agents want, so keep the plugin's default. The per-page copies are part of the deliverable,
not a side effect. Read the Docs serves them as ordinary static files. (Builder's call, based on
the plugin's documented link format. Confirm the exact paths against the first build.)

Add it to the `docs` dependency group in `pyproject.toml` and refresh `uv.lock`. It is subject to
the normal 7-day `exclude-newer` cooldown. This is not a security fix.

The plugin's repo says it is in maintenance mode while its author works on Zensical. It is small
and works today. If it breaks against a future mkdocs-material release, the fallback is a short
MkDocs hook in the repo that writes the files. Do not build the fallback now.

### Serving: Read the Docs root

Read the Docs
[serves `llms.txt` and `llms-full.txt` from the domain root](https://about.readthedocs.com/blog/2026/02/llms-txt-support/)
when the default version is public, active, built, and has the files in its build output.
`.readthedocs.yml` does not change.

The root files only appear once a tagged release containing the plugin has been built as
`stable`. Until then they exist only at `/latest/llms.txt` and `/latest/llms-full.txt`. That is
expected. It does not block merging.

### Links point at the version that was built

The plugin requires `site_url`. Set it from Read the Docs' canonical URL, with the stable URL as a
fallback for builds outside RTD:

```yaml
site_url: !ENV [READTHEDOCS_CANONICAL_URL, "https://toolr.readthedocs.io/stable/"]
```

The `stable` build then links to `/stable/…` and `latest` links to `/latest/…`. The root file
comes from `stable`, so agents land on the released API. Local and CI builds fall back to the
stable RTD URL. A side effect is that `mkdocs serve` serves under `/stable/` locally. That is
accepted.

### Read the Docs is the only docs host

Keep only Read the Docs. Remove the "Deploy to GitHub Pages" step from the CI docs job, and keep
the `mkdocs build --strict` step and the gate. Otherwise the Pages copy would carry an RTD
`site_url`, with canonical tags and a sitemap pointing elsewhere, and it would go stale the moment
it stopped being deployed.

Once the PR merges, unpublish the GitHub Pages site in the repo settings, or with
`gh api -X DELETE repos/s0undt3ch/ToolR/pages`. This is an outward-facing change, so confirm it
with the maintainer at the time. Don't touch the old `github.io` links in archived specs. Archived
specs are immutable.

This is the builder's call over a hard-coded `base_url`. It does the same job and stays correct
for each version. Setting `site_url` also gives Material canonical link tags. That is a side
effect, and a good one.

### Audience and contents

The reader is an agent with no toolr skills installed. It needs to understand toolr, then write
`tools/*.py` commands.

`llms.txt` contains, in this order:

1. The title and a one-paragraph description of toolr (`markdown_description`). Write it for an
   agent deciding whether toolr fits.
2. A short note on the skills. If the agent's harness supports skills, it should install them.
   The note links to the Agent skills page for how. It carries no install commands of its own
   (see *Staying current*).
3. Curated sections, each page with a one-line description:
   - **Start here:** Home (`index.md`), Quickstart, Installation (overview and mise), "How toolr
     is laid out" (`concepts.md`).
   - **Writing commands:** every page under `writing-commands/`.
   - **Reference:** Project configuration, CLI reference, and the API reference pages under
     `reference/`.
   - **Optional:** Third-party packages, Agent skills.

Leave out Internals, Migration, Contributing, Changelog and the external Known bugs link.

`llms-full.txt` expands the same pages. It does not cover the whole site.

### Docs

Add a short section to `docs/skills.md`. It says the root `llms.txt` and `llms-full.txt` exist,
what each is for, and that they are the route for agents without the skills installed. Link both
files by absolute RTD URL.

### Staying current

The files must not go stale as the docs change. There are three ways they can:

1. **A new page is never listed.** Someone adds a page under `writing-commands/` or `reference/`,
   and `llms.txt` never mentions it.
2. **A listed page is renamed or removed.** Nobody has checked whether the plugin fails on a
   missing file or skips it without a warning. Do not rely on either.
3. **Copied text drifts.** Install commands, descriptions and the toolr summary go stale as the
   pages they describe change.

Defences, one per cause:

1. **Every nav page is either listed or excluded.** Keep an explicit exclude list (Internals,
   Migration, Contributing, Changelog) next to the `sections:` config. A check fails if any
   Markdown page in the `mkdocs.yml` nav is in neither, or in both. Adding a page then forces a
   decision: list it in `llms.txt` or exclude it. Use directory globs (`reference/*.md`) where a
   section should take every page. A trial build showed that `writing-commands/*.md` also matches
   the excluded snippet includes under `writing-commands/files/`, because `fnmatch` `*` crosses
   `/`. List that section explicitly, in nav order. The gate catches new pages.
2. **Every listed page appears in the output.** The check confirms that each page the `sections:`
   config resolves to has a link in the built `site/llms.txt` and content in
   `site/llms-full.txt`.
3. **Copy nothing a page already says.** `llms.txt` carries no install commands. The skills note
   says the skills exist and links to the Agent skills page. Keep the page descriptions short, a
   few words each, so there is little to drift. The `markdown_description` paragraph is the one
   piece of hand-written prose. Keep it to what toolr is, not features or versions.

### Gate

After `mkdocs build --strict`, fail if any of these is false:

- `site/llms.txt` and `site/llms-full.txt` exist and are not empty.
- Every Markdown page in the `mkdocs.yml` nav is in exactly one of the `sections:` config or the
  exclude list. External nav links (Known bugs) are not pages and are ignored.
- Every page the `sections:` config resolves to is linked in `site/llms.txt` and present in
  `site/llms-full.txt`.
- Every link in `site/llms.txt` that points into the docs resolves to a file that exists under
  `site/`.

Run it where `mkdocs build --strict` already runs in CI (the docs job), and make it runnable
locally with the same command. A docs change, a plugin update or a dependency update could
otherwise stop the files matching the docs without anyone noticing.

### Release note

Queue one with `toolr release-notes add`. Agents and users can see this change.

## Left to the builder

- The exact one-line page descriptions and the `markdown_description` paragraph. Keep them
  factual. Write them for an agent, not as marketing.
- Where the gate lives: a toolr command under `tools/`, an `xtask`, or a MkDocs hook that fails
  the build. A hook would make `mkdocs build --strict` enforce it everywhere, Read the Docs
  included. Pick whichever runs locally and in CI with no extra step.
- Where the exclude list lives, as long as it sits next to the `sections:` config so both are
  edited together.
- The exact `fnmatch` patterns in `sections:`, as long as the result matches the page list above.
- Whether the plugin's `preprocess` hook is needed to strip Material chrome. Look at the generated
  output first, and only add it if the output is noisy.
- Whether the "Optional" section heading is worth keeping. The llmstxt.org convention treats
  `## Optional` as content an agent can skip.
