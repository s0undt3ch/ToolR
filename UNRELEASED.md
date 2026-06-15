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

### Changed

- The binary↔runner dispatch schema is bumped to `2`. The 0.25.0 binary changed
  the runtime contract (it starts the runner with `-P` and relies on the runner
  to put the repo root on `sys.path`) without bumping the schema, so pairing it
  with an older `toolr-py` failed with a cryptic `No module named 'tools'`
  instead of the schema guard's clear "venv out of sync" message. The schema now
  covers that contract, so a future binary paired with a stale `toolr-py` fails
  loudly and points you at `toolr project venv upgrade toolr-py`. Re-sync your
  tools venv after upgrading toolr.
