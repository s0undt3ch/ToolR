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
- Fixed Ctrl-C during a running command printing a raw Python traceback
  and exiting with an arbitrary code instead of the conventional 130.
  The toolr binary already forwards SIGINT to the Python runner
  subprocess; the runner now catches the resulting `KeyboardInterrupt`
  and exits 130 cleanly instead of leaking it as an unhandled exception.
