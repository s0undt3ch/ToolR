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

Commands with a named positional parameter ahead of `*args` (for example
`def cmd(ctx, foo: Foo, *paths: pathlib.Path)`) now run. They used to crash with
`TypeError: ... got multiple values for argument`. Commands with positional-only
(`/`) parameters now work too: the manifest parser used to drop them, plus the
parameter right after them.
