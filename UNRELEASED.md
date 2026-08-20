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

Fixed a regression from the #449 enum-collision fix: importing an `Enum`-typed class from a
different module than the one declaring it could hard-fail `toolr project manifest rebuild`
with "type is not supported" if another, unrelated module also declared a same-named enum class.
Definitions that serialise identically (same members, same values, same order) across modules are
now treated as unambiguous, so a shared enum imported into several command modules resolves
correctly instead of being rejected as a collision.
