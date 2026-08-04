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

The argparse-introspection source now classifies `nargs="+"`/`"*"` correctly
instead of capping them at a single value. Optional flags (e.g.
`parser.add_argument("--names", nargs="+")`) are treated as repeated, the
same as `action="append"`, and a single occurrence may now take several
space-separated values (`toolr django run greet --names alice bob`) as well
as repeated invocations (`--names alice --names bob`); this widening only
applies to nargs-derived flags; `action="append"` flags keep taking exactly
one value per occurrence. Positional arguments with `nargs="+"`/`"*"` (e.g.
Django's `manage.py test app1 app2`) are now scanned as variadic positionals
instead of a single required value, with `nargs="+"` still requiring at
least one. `nargs` shapes the scanner can't fully represent — an int count,
`argparse.REMAINDER`, or `nargs="?"` on a positional — now emit a build
warning instead of silently degrading.
