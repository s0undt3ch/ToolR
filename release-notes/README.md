# Release notes

Each `*.md` file here is one queued release note. On release, `toolr release-notes render` joins them into
the `### Notes` section of the GitHub release body and `CHANGELOG.md`, and `toolr release-notes clear`
empties the directory. The per-commit lists go only in `CHANGELOG.md`: `toolr release-notes github-body`
writes the GitHub release body as the notes plus a link to the full list of changes, or the link alone when
the notes are over GitHub's 125,000 character limit. This README, subdirectories and files that don't end
in `.md` are never treated as notes.

This file is the canonical statement of the rules below. `CONTRIBUTING.md` and `CLAUDE.md` summarise it.

## Adding a note

```sh
toolr release-notes add --text "What changed, and what a user needs to do about it."
```

- The slug comes from the current branch name. Pass `--slug my-change` to choose it.
- `--text` writes the note. Without it you get an empty file to edit, which `check` rejects until it has
  content.
- `--append` adds `--text` to the existing note with the same slug instead of refusing.
- The command stages the file.

Notes are named `<YYYY-MM-DD>.<HH-MM-SS>.<slug>.md`, using the UTC creation time. The slug is lowercase
letters and digits separated by single hyphens. The timestamp keeps notes in the order they were written.

## Writing a note

- Write plain prose, lists and code blocks. Do not use headings, ATX (`#`) or setext (underlined): `render`
  places the notes under one.
- Close every code fence.
- Start a note with `**Breaking:**` if it describes a breaking change. Those notes are rendered first.
- One note per change. Several paragraphs and lists are fine inside one note.

`toolr release-notes check` (also a pre-commit hook) enforces the file name, the timestamp, UTF-8, a
non-empty body, no headings and no unclosed fences. It also rejects a leftover `UNRELEASED.md`.

## The PR gate

CI runs `toolr release-notes check --base <base branch>` on every PR. Run it locally with
`--base <parent branch>`. It fails when:

- a `feat`, `fix` or `revert` commit, or any breaking commit (`!` after the type or a `BREAKING CHANGE:`
  footer), comes with no added or modified note. A `revert` may delete the reverted note instead.
- a breaking commit comes with no added or modified note that starts with `**Breaking:**`.

Exemptions:

- Commits with a `deps` scope, such as `fix(deps): ...`, need no note unless they are breaking.
- A commit whose body contains `Changelog: skip` needs no note unless it is breaking. git-cliff leaves it
  out of the changelog too.
- A `Release-Notes: skip` trailer on any one commit exempts the whole PR.

A PR that only adds or edits notes should use a `docs(release-notes): ...` commit, which git-cliff leaves out
of the changelog.

## Pre-releases

To cut a pre-release, run the `Release` workflow (`release.yml`) with a version such as `1.0.0-rc.1`
(`X.Y.Z-rc.N`). It is tagged `v1.0.0-rc.1` and published as a GitHub pre-release, so `latest` lookups skip it,
and the wheels carry the PEP 440 form `1.0.0rc1`. Its release body shows every note queued since
the last final release and links a comparison with it, but it gets no `CHANGELOG.md` section and leaves the
notes queued. The next final release rolls up everything since the previous final release, pre-releases included.

The policy lives in four files: `tools/version.py` (version parsing and the `release-prerelease` output),
`.github/workflows/_prepare-release.yml` (skips the CHANGELOG and notes-clearing steps for a pre-release),
`.github/workflows/release.yml` (`--prerelease` instead of `--latest`) and `cliff.toml` (`tag_pattern` only
matches final release tags).

## After a parent PR merges

In a stack, restack and push the child branch. Do not re-run the old CI run: it checked the branch against
the previous base.
