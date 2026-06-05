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

Bumped all Rust, Python, and GitHub Actions dependencies in a single
sweep — including `ruff_*` git deps to `0.15.15`, `ruff`/`coverage`/
`hypothesis` on the Python side, and `actions/checkout` and
`actions-cool/check-user-permission` in CI. Transitive lockfile bumps
in `Cargo.lock` and `uv.lock` rolled forward at the same time.

The task-runner startup benchmark moved from `toolr bench compare` to
`python3 scripts/bench.py`. The script now depends only on the Python
standard library (no toolr, no rich), so it can run in a fresh CI job
without bootstrapping a project venv. Output is a markdown table on
stdout; progress lines go to stderr.
