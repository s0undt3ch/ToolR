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

- Fixed the `tools/` dogfooding venv lagging its own `toolr-py` pin behind
  the `toolr` binary CI builds from `main` HEAD for up to a week after
  every schema-bumping release, breaking self-CI (`toolr ci
  check-run-build`) for that whole window. The root `exclude-newer = "7
  days"` dependency cooldown, inherited by `tools/`, is meant for
  third-party releases; `toolr-py` is exempted from it specifically since
  it ships from the same release as the binary that runs against it.
