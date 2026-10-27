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

Both wheels now carry the `Development Status :: 4 - Beta` PyPI classifier instead of Alpha.

The error shown when the `toolr` binary and the `toolr-py` in your tools venv disagree on the
dispatch schema now names the side to upgrade. A newer binary tells you to run
`toolr project venv sync -P toolr-py`; an older binary tells you to upgrade the binary. It used to
always point at `toolr project venv upgrade`, which was removed in 0.22.0.
