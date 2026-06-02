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

## Bug fixes

### `command_group(docstring=...)` now splits the docstring into title + description

`command_group("name", docstring=__doc__)` previously stuffed the entire
docstring into the group's `description` field and left `title` empty,
so the blurb never appeared next to the group in the parent `--help`
listing (clap shows `about`, which is what `title` populates). The
parser now mirrors how `@command` function docstrings are handled — the
first paragraph becomes the group's `title` (clap's `about`) and the
remainder becomes the `description` (clap's `long_about`). An explicit
positional `title=` / `description=` still wins for either side.

Both the static parser (`parser/groups.rs`) and the runtime decorator
(`toolr._decorators.command_group`) now go through the same shared
`parse_docstring` helper, so static-manifest output and runtime
introspection agree. The runtime's old `title = name` fallback —
which would have produced a redundant `dbt-config  dbt-config` in
the parent listing — has been removed; an unset title now stays
empty, matching the static parser.

See [#292](https://github.com/s0undt3ch/ToolR/issues/292).
