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

- Fixed the argparse scanner dropping every flag spelling except the
  longest `--flag` on a call declaring several (e.g.
  `add_argument("-s", "--run-synchronously", ...)`). Short flags and
  extra long spellings now all register as clap aliases.
