<!--
UNRELEASED.md — Queued release notes for the next release.

Append narrative entries here as PRs land. On release, the
`_prepare-release.yml` workflow folds the content of this file
into the `### Notes` subsection of both the GitHub release body
and CHANGELOG.md (under the new version's heading), then resets
this file to empty for the next cycle.

Empty between releases is the steady-state — there's no header,
no scaffolding. Just write whatever should appear in the notes.
-->

## Enhancements

### `--help` now renders the full docstring (Examples, Notes, …); `-h` stays short

Building on the title/description split shipped in 0.22.1, both
`command_group(docstring=__doc__)` and `@command`-decorated function
docstrings now feed clap a multi-section render for `long_about`. The
first paragraph still drives the short `about` slot (shown next to the
name in the parent listing and on `-h`), but `--help` now shows the
short summary plus the long body plus any `Examples` / `Notes` /
`Warnings` / `See Also` / `References` / `Todo` / `Deprecated` /
`Version Added` sections from the docstring. `Args:` / `Arguments:`
stay out of the prose — they already appear as per-argument help
blocks.

Side-effects:

- `toolr <group> <cmd> -h` and `toolr <group> <cmd> --help` now differ:
  short form is just the summary, long form is the full render. The
  previous behaviour of always printing the long form on both was a
  paper-over from when `description` carried only the body paragraph.
- The shared rendering logic now lives exclusively in Rust
  (`toolr_core::docstrings::Docstring::full_description`), exposed to
  Python via PyO3. The parallel Python re-implementation in
  `toolr.utils._docstrings.Docstring.full_description` has been removed
  so the two surfaces can't drift.

See [#292](https://github.com/s0undt3ch/ToolR/issues/292).

## Bug fixes

### Tab completion offers `self` / `project` even outside a toolr project

Running `toolr <TAB>` from a directory with no `tools/` ancestor used to
return nothing and exit 1, so the shell fell back to filename
completion. The binary's own `self` / `project` subtree doesn't depend
on a project root, so it should always complete — only user-defined
groups need a discoverable `tools/`. `run_complete` now falls back to
an empty manifest when project discovery fails, then merges in the
built-in completion entries so `self`, `project`, and their children
are offered everywhere.

See [#306](https://github.com/s0undt3ch/ToolR/issues/306).
