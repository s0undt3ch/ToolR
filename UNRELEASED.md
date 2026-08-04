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

- Fixed the argparse scanner dropping every flag spelling except the
  longest `--flag` on a call declaring several (e.g.
  `add_argument("-s", "--run-synchronously", ...)`). Short flags and
  extra long spellings now all register as clap aliases.
- Fixed `tests/distribution/test_example_plugin_contract.py` leaking real
  venv provenance stubs into the developer/CI's live cache dir instead of
  a sandboxed one, which inflated `toolr self cache`'s orphan count over
  time.
- Fixed the cache orphan hint (`toolr: cache has N orphan entries (~X)`)
  reporting the whole cache's size instead of just the orphaned entries',
  which made a couple of unrelated live venvs look like the cause of a
  large orphan count.
- Corrected `docs/internals/cache.md`, which falsely claimed multiple git
  worktrees of the same repo share a single venv cache entry — they never
  have. Each worktree gets (and needs) its own.
- Fixed `DispatchCommand.argv` emitting the repeat-the-flag form
  (`--customer-ids a --customer-ids b`) for `nargs="+"`/`"*"` args.
  Standard argparse only keeps the last occurrence for those — every
  value but the last was silently dropped. Now emits one occurrence
  with all values (`--customer-ids a b`), reserving the repeat-the-flag
  form for genuine `action="append"` args.
