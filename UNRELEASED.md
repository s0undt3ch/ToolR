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

Fixed: `@group.command(name="…")` silently dropped the keyword and the
command took its hyphenated function name instead — the static parser
only read the positional override. The `name=` keyword is now honoured on
both `@group.command` and `@command`, which share an identical
command-name contract: the override may be passed positionally
(`@command("collect", group="…")`) or by keyword
(`@command(name="collect", group="…")`), but not both — passing both
raises `TypeError` at runtime and fails `build-manifest` with a clear
*conflicting command name* error.

The vestigial, never-implemented `aliases=` keyword has been removed from
the `@command` decorator.
