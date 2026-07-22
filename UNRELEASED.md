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

The `Set the … Pipeline Exit Status` gate jobs in the CI, Release, and
install-smoke workflows now actually fail when any upstream job fails or is
cancelled. Previously they only echoed the workflow status and always exited
zero, so the required status check on `main` was green regardless of CI
results. The gates also now depend on every job in their workflow, closing a
gap where a failure early in the job graph surfaced downstream as `skipped`
and went unnoticed. A new `check-pipeline-gate-needs` pre-commit hook keeps
the gates' `needs` lists in sync with the full job set so they cannot
silently drift as jobs are added.
