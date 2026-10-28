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

`toolr-py`'s dispatch-schema mismatch error now names the side to upgrade: run
`toolr project venv sync -P toolr-py` when the binary is newer, or upgrade the binary when it is
older. It used to always point at `toolr project venv upgrade`, which was removed in 0.22.0. The
error comes from the `toolr-py` in your tools venv, so a venv still on an older `toolr-py` keeps
printing the old text; the binary's version-skew warning above it gives the right command.

Pressing Ctrl-D (or stdin closing) at the uv install prompt now declines the install. It used to
count as consent and download uv. Bare Enter still installs.

In a repo without `tools/pyproject.toml`, toolr now runs commands with `python3` (or `python`) from
`PATH`, as documented. It used to fail every command with "Python interpreter not found at python3"
unless `$TOOLR_PYTHON` held an absolute path. `$TOOLR_PYTHON` now also accepts a bare name such as
`python3.12`, looked up on `PATH`.
