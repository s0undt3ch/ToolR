# Release-notes fragments design

## Intent

Release notes are queued as prose paragraphs in a single `UNRELEASED.md`. Every PR appends to the end
of it, so any two open PRs conflict there, and stacked PRs conflict on every restack. 32 commits
touched the file across 24 merges since `v0.35.1`.

Replace the single file with one file per note in `release-notes/`, managed by a project-local toolr
command. Separate files can't conflict. The rest of the release flow stays as it is: git-cliff still
renders the notes from `TOOLR_RELEASE_NOTES` under `### Notes`, and still owns `CHANGELOG.md`.

## Goals

- No merge conflicts on release notes between concurrent or stacked PRs.
- One command that creates a correctly named note, so humans and agents don't invent names.
- A CI gate that fails a PR that ships a user-visible change without a note.
- Breaking-change notes lead the rendered notes.
- Remove the release workflow's inline Python regex and the byte-exact `printf` reset.

## Non-goals

- Changing `cliff.toml`'s body template, the `### Notes` heading, or how git-cliff groups commits.
- Typed or categorised notes. git-cliff already groups commits by type and flags `!` commits.
- Dependency notes, or teaching Renovate to write notes. Renovate stays as it is, apart from the
  wording of its existing `prBodyNotes` text.
- Deriving notes automatically (for example from a `rust-version` change). Dropped by the maintainer.

## Anti-goals

- A third-party fragment tool (towncrier, scriv, changie, knope). Each wants to own the changelog
  write or the version bump, which git-cliff and `toolr version bump` already own.
- A gate that demands a note on every PR. Renovate, CI, and refactor PRs don't need one.
- Ordering that depends on git history. It breaks silently if the merge strategy ever changes.

## Constraints

- PRs land as true merge commits (`git log --first-parent` on `main` is merge commits plus the
  release commit).
- `ci.yml`'s `prepare-release` job calls `_prepare-release.yml` on every PR and every push to `main`,
  so the render and clear steps run constantly in throwaway checkouts. They must touch nothing
  outside the working tree, and must succeed with zero notes.
- `_prepare-release.yml` already runs a project-local command (`toolr version bump`) after "Setup
  ToolR", so `toolr release-notes …` works there.
- The release commit is made with `git commit -am`, carried as a `format-patch --binary` patch, and
  applied with `git am` (`.github/actions/apply-release-patch/action.yml`). Tracked-file deletions
  survive that chain unchanged.
- `tools/` isn't measured by coverage (`.coveragerc` sources `toolr` and `tests`), so there is no
  patch gate, but tests are still required.
- Every CI job must be listed in the `pipeline-gate` needs; the `check-gate-needs` pre-commit hook
  enforces it.

## Decisions

### Layout

- Directory: `release-notes/` at the repo root. mkdocs (`docs_dir: docs`) never sees it.
- `release-notes/README.md` holds the guidance now in `UNRELEASED.md`'s HTML comment. It also keeps
  the directory tracked when no notes are queued. Every subcommand ignores it by name.
- `UNRELEASED.md` is deleted.

### Note files

- Name: `release-notes/<YYYY-MM-DD>.<HH-MM-SS>.<slug>.md`, e.g.
  `release-notes/2026-11-01.14-03-27.repo-trust.md`. (Maintainer's choice.)
- Timestamp: the UTC time the note was created (`datetime.now(UTC)`, as `tools/version.py` does).
  Filename order is therefore creation order, which keeps the notes in the order they were written.
- Slug: `[a-z0-9]+(-[a-z0-9]+)*`. Slugs contain no dots, so the three dot-separated parts parse
  unambiguously.
- Content: bare Markdown prose, one or more paragraphs. No front-matter.
- No Markdown headings in a note. Headings would nest wrongly under the `### Notes` heading git-cliff
  renders.
- Breaking changes start with `**Breaking:**`, as today.

### Command: `tools/release_notes.py`, group `release-notes`

**`toolr release-notes add [SLUG] [--text TEXT] [--append]`**

- Without `SLUG`, derive one from the current branch. Strip a leading `<prefix>/` (`feat/`, `fix/`,
  `docs/`, and so on), then normalise to the slug pattern. Fail with a clear message if nothing
  usable remains, for example on a detached HEAD.
- Write `release-notes/<now>.<slug>.md`, run `git add` on it, and print its path.
- `--text` writes that text as the content. Without it the file is created empty, and `check` rejects
  it until it is filled in.
- If a note with that slug already exists under any timestamp, refuse. With `--append`, append `--text`
  to the existing file as a new paragraph instead. `--append` without `--text` is an error.

**`toolr release-notes check [--base REF]`**

Without `--base`, check the files only. This is the pre-commit mode.

- Every entry in `release-notes/` other than `README.md` is a file matching the name pattern, with a
  real calendar date and time.
- No note is empty or whitespace-only.
- No note contains a Markdown heading outside a fenced code block.
- `UNRELEASED.md` does not exist. This turns the modify/delete conflict on rebased in-flight branches
  into a pointed error.

With `--base REF`, run the file checks, then the gate. This is the CI mode.

- Commits in scope: non-merge commits in `REF...HEAD`.
- A note is required when any in-scope commit's subject is `feat` or `fix` (any scope, with or
  without `!`), or carries `!` on any type.
- Exempt: commits whose scope is `deps`, unless they carry `!`. A breaking commit always needs a
  note. (Builder-check call.)
- Exempt: commits carrying the existing `Changelog: skip` trailer. git-cliff hides those, so they
  aren't user-visible. (Builder-check call.)
- Escape hatch: a `Release-Notes: skip` trailer in the body of any in-scope commit passes the gate
  for the whole PR. It mirrors the existing `Changelog: skip` trailer.
- Satisfied when `git diff --diff-filter=AM --name-only REF...HEAD -- release-notes/` lists at least
  one note (not `README.md`). Modifying an existing note counts, so `--append` satisfies it.
- On failure, list the commits that triggered the gate and print both remedies: `toolr release-notes
  add`, or the trailer.

**`toolr release-notes render [--github-env PATH]`**

- Read every note. Normalise CRLF to LF, strip surrounding whitespace.
- Order: notes whose content starts with `**Breaking:**` first, then everything else. Filename order
  within each group.
- Join with one blank line between notes. Zero notes render as an empty string.
- Without `--github-env`, write to stdout.
- With `--github-env PATH`, append `TOOLR_RELEASE_NOTES` to that file using the GitHub multiline
  syntax (`NAME<<DELIM … DELIM`). Generate a random delimiter, and check that it doesn't appear in
  the content.

**`toolr release-notes clear`**

- Delete every note, keeping `README.md`. Deleting nothing is fine.
- Only unlink the files. The release workflow's `git commit -am` stages the deletions.

### Release workflow: `.github/workflows/_prepare-release.yml`

- Replace "Load UNRELEASED.md into TOOLR_RELEASE_NOTES env var" with `toolr release-notes render
  --github-env "$GITHUB_ENV"`.
- Delete the "Get the Python Binary path from Mise" step if nothing else uses its output.
- Replace the "Reset UNRELEASED.md" `printf` step with `toolr release-notes clear`. It must run after
  both git-cliff steps, as the reset does today.
- Update the comments that describe `UNRELEASED.md`.

### CI gate: `.github/workflows/ci.yml`

- A new job runs `toolr release-notes check --base "origin/${{ github.base_ref }}"` on
  `pull_request` events only.
- It needs `fetch-depth: 0`. A shallow clone makes the range empty, and the gate would pass silently.
- `github.base_ref` is the PR's base branch, which is the parent branch for a git-spice stack. A
  child PR is therefore only judged on its own commits.
- Add the job to the `pipeline-gate` needs.

### Pre-commit hook: `.pre-commit-config.yaml`

- A `language: system` hook, `entry: toolr release-notes check`, `pass_filenames: false`, firing on
  `^(release-notes/|UNRELEASED\.md$)`. This follows the existing `toolr pre-commit …` hooks.

### git-cliff: `cliff.toml`

- Add `{ message = "^docs\\((release-notes|unreleased)\\)", skip = true }` to `commit_parsers`
  before the `^doc` rule. Note-only commits then stop duplicating the note as a Documentation entry.
  `CHANGELOG.md` already has 11 such `*(unreleased)*` duplicates. `unreleased` is included because
  `docs(unreleased)` commits already on `main` since `v0.35.1`, and in open PRs, would otherwise
  duplicate in the first release after the cutover. (Builder-check call.)
- Convention: a commit that only adds or edits a note uses `docs(release-notes): …`.

### Migration

- Split the queued `UNRELEASED.md` content into notes, one per logical note. Multi-paragraph notes
  stay together. The `**Breaking:**` bool-flag paragraph must become its own file, or `render` can't
  move it first.
- Give them timestamps one second apart, in their current order in `UNRELEASED.md`, so the backlog
  renders in today's order. Slugs describe each note.
- Add the `**Breaking:**` marker to the repository-trust note (wording otherwise unchanged), so it
  renders first. (Maintainer's choice.)
- Delete `UNRELEASED.md` in the same PR.

### Sequencing (maintainer's choice)

- #625 `fix/release-prek-failures` lands first. This PR stacks on it with git-spice and rewrites
  `_prepare-release.yml` on top of it.
- Final plan task: after the cutover merges, rebase the open PRs that edit `UNRELEASED.md` (as of 2026-11-01: #626
  `fix/tester-discover-errors`, #624 `fix/action-latest-repo`, #622
  `fix/ancestor-tools-discovery`), moving their entries into notes.
- #625 `fix/release-prek-failures` rewrites `_prepare-release.yml` and makes its `prek run
  --all-files` strict. Once both land, the new `check` hook runs during release prep, so an invalid
  note on `main` blocks a release. That is acceptable: the same hook already blocks the note's PR.

### References to update

- `CLAUDE.md`: the "Where things live" entry, the release-bump rule (read `release-notes/` instead of
  `UNRELEASED.md`), and the "Queue release notes" action. The new action tells agents to add a note
  with `toolr release-notes add` for every user-visible change, use `docs(release-notes):` for
  note-only commits, use the `Release-Notes: skip` trailer when a `feat`/`fix` has nothing for
  users, and never recreate `UNRELEASED.md`.
- `CONTRIBUTING.md`: the "Adding a supported type" step 6, and a short section on release notes.
- `crates/xtask/tests/rust_version_lockstep.rs`: the assert message.
- `renovate.json5`: reword the `prBodyNotes` text to point at `toolr release-notes add`.
- Archived specs under `specs/archive/` stay as they are.

### Tests

- `tests/test_tools_release_notes.py`, modelled on `tests/test_tools_version.py`: factory fixtures
  over `tmp_path`, imports at the top of the file.
- Cover `README.md` exclusion in `render`, `clear` and `check`; breaking-first ordering; empty
  render; CRLF; the `--github-env` format; name and heading validation; the `UNRELEASED.md` guard;
  slug derivation; and the gate against a real temporary git repo with `feat`, `fix`, `!`, `deps`,
  trailer, and appended-note cases.

## Left to the builder

- Internal structure of `tools/release_notes.py` and how it shells out to git (`ctx.run` or
  `subprocess`).
- The CI job's name and its exact placement in `ci.yml`.
- The wording of `release-notes/README.md`, the CLAUDE.md rule, and error messages, within the rules
  above.
- Slugs for the migrated notes.
