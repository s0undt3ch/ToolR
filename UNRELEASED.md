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

### `toolr.current_context()`

Added `toolr.current_context()`, a `contextvars`-backed accessor a
command-authoring helper can call to get the `Context` of the toolr command
currently executing, without it being passed as a parameter. Purely
additive: `@command`-decorated functions keep receiving `ctx` as their
required first argument, and existing helpers that take `ctx` explicitly are
unaffected. See `toolr.testing.set_current_context()` for testing helpers
that use it.
