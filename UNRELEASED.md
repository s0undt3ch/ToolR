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

- Fixed the manifest builder resolving enum CLI choices by bare class
  name instead of by defining module, which meant two unrelated
  `tools/*.py` modules declaring a same-named `Enum` subclass (e.g.
  `Database`) clobbered each other's `--help` choices depending on
  scan order. `allowed_values` and enum-attribute defaults now resolve
  against the class actually in scope for each module ([#449](https://github.com/s0undt3ch/ToolR/issues/449)).
