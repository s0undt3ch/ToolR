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

`--help` output is now rendered as markdown through `termimad`.
Python docstrings flow through unchanged — headings, sections
(`## Examples`, `## Notes`, `## Warnings`), bullet lists, and
fenced code blocks render as styled markdown in the terminal and
as readable plain text when captured (non-TTY or `NO_COLOR`).
`$COLUMNS` is honored for width control. The page matches clap's
default layout: a `**Usage:**` line, `Arguments:` / `Options:` /
`Commands:` blocks reflecting the actual command shape, possible
values on positionals, and `[default: …]` on options where a
literal default exists (unresolvable Python expressions are
suppressed rather than shown as `<expr>`).

`-h` (short) and `--help` (long) remain distinct: short shows the
first-paragraph `about` and truncates per-option help to its first
line; long shows the full docstring body, Examples/Notes sections,
possible-value lists, and `[default: …]` annotations.

Sphinx-style ``code`` (double backticks) in docstrings is now
normalized to single-backtick markdown at parse time, so RST
syntax no longer leaks into rendered help.

Internal: clap's built-in help is disabled and dispatched to
`crate::help`; the `crate::markdown` pre-render layer, the
`wrap_help` feature on `clap`, and the `clap-help` dependency
are all gone.
