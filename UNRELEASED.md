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
results. The gates now run a new in-repo `pipeline-gate` composite action
that always executes, scans the `needs` context handed to it via
`jobs: toJSON(needs)`, and sets the exit status — and they depend on every
job in their workflow, closing a gap where a failure early in the job graph
surfaced downstream as `skipped` and went unnoticed. A new dogfooded
`toolr ci check-gate-needs` command enforces the whole gate wiring (full
sorted `needs` list, job-level `if: always()`, the `jobs` input, no
step-level `if`), rewriting workflows in place via comment-preserving
`ruamel.yaml` round-trips, and runs as a pre-commit hook in auto-fix style
so the wiring cannot silently drift as jobs are added. The released `toolr`
binary is now pinned in `mise.toml` so `mise install` provides it locally;
CI's pre-commit job installs it via the Setup ToolR action.
