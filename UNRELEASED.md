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
- Fixed the built-in argparse scanner (`[tool.toolr.argparse.*]`) silently
  dropping Django management commands that take no CLI arguments at all —
  e.g. a `BaseCommand` subclass with no `add_arguments` method. The
  scanner used "found zero `add_argument()` calls" as its only signal
  for "this isn't a real command," which also matched genuine zero-arg
  commands. A new opt-in `django = true` block setting recognises a
  module-level `class Command(...)` or `Command = <Name>` alias to a
  same-module class — Django's own loader contract — so zero-arg
  commands are kept while stray helper modules are still excluded.
  Plain argparse blocks (the default) are unaffected. Separately,
  underscore-prefixed filenames (`_helpers.py`, `__init__.py`) are now
  skipped outright for every block, Django or not — that convention is
  plain Python, not Django-specific.

A dispatcher's own flag (e.g. `--dry-run` on a `job` dispatcher wrapping
Django management commands) typed *after* the dispatched subcommand name
was silently forwarded to the wrapped command instead of being rejected —
the dispatcher's own parameter kept its default, so a safety flag like
`--dry-run` had no effect at all. `toolr <dispatcher> <leaf> ... --flag`
now errors, pointing at the correct position, instead of silently
executing for real ([#445](https://github.com/s0undt3ch/ToolR/issues/445)).
