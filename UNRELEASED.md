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

Fixed #454, a regression from the #449 enum-collision fix: importing an `Enum`-typed class from a
different module than the one declaring it could hard-fail `toolr project manifest rebuild` with
"type is not supported" if another, unrelated module also declared a same-named enum class. The
static parser now follows the actual import — relative imports, `__init__.py` re-exports,
`as`-aliasing chains, and `try`/`except ImportError` dual-path imports all resolve to the class
that's genuinely in scope, rather than guessing across same-named classes elsewhere. Two constructs
that can never be resolved unambiguously — `from foo import *` and `import foo.bar` +
`foo.bar.X`-style attribute-chain usage — now get a specific, actionable error instead of the
generic "type is not supported" message.

Also fixed: `Environment | None`-style `Optional`-wrapped `Enum`/`Literal` parameters previously
built successfully but silently populated `allowed_values` as empty — no clap choice validation,
nothing in `--help`.

`if TYPE_CHECKING:`-guarded enum imports (module-level only) now work end-to-end, not just at
build time — the runner lazily imports the real class at coercion time instead of relying on the
target module's own (never-executed) guarded import, so the previous silent "whole command's
argument coercion disabled" failure mode when one annotation was unresolvable no longer applies to
these cases either.
