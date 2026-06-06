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

Tab completion now offers flag candidates at every level of the
command tree, not just leaf commands. `toolr --<Tab>` lists the
binary's own root options (`--debug`, `--quiet`, `--timestamps`, …)
alongside `--help`, and `--help` is offered at every group node
(`toolr self --<Tab>`, `toolr self cache --<Tab>`, …) and on every
leaf in addition to its own flags. A bare `--` typed as the
in-progress word is now preserved through the engine — previously
clap consumed it as the end-of-options marker and the shell saw
an empty candidate list.
