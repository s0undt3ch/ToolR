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

- Fixed the manifest builder resolving enum CLI choices by bare class
  name instead of by defining module, which meant two unrelated
  `tools/*.py` modules declaring a same-named `Enum` subclass (e.g.
  `Database`) clobbered each other's `--help` choices depending on
  scan order. `allowed_values` and enum-attribute defaults now resolve
  against the class actually in scope for each module ([#449](https://github.com/s0undt3ch/ToolR/issues/449)).
- `toolr.testing.RunMock` now forwards `assert_called`, `assert_called_once`,
  and `assert_has_calls`, closing gaps in its otherwise-explicit
  `unittest.mock` API subset. A new test asserts the forwarded set stays in
  sync with `Mock`'s own public API going forward.
- `toolr.testing.make_context` now accepts a `width=` override for its
  captured consoles, defaulting to a new `DEFAULT_TEST_CONSOLE_WIDTH`
  (1000 columns) so `ctx.stdout`/`ctx.stderr` assertions never have to
  account for rich wrapping a long line.
