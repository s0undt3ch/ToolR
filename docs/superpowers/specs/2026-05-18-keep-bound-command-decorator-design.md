# Keep the bound `@group.command` decorator

**Status:** Approved (2026-05-18)
**Topic:** Roll back the `@group.command` deprecation while keeping the bound subgroup form (`parent.command_group("child", ...)`) on track for removal in toolr 1.0.

## Background

Toolr currently deprecates two legacy decorator patterns:

1. `@group.command` — attach a command to a captured `CommandGroup` binding.
2. `parent.command_group("child", ...)` — declare a subgroup via a bound method on the parent group.

Both emit `ToolrDeprecationWarning` and are slated for removal in 1.0. The replacements are the standalone `@command(group="…")` decorator and the dotted `command_group("parent.child", ...)` form respectively.

The decision was made together in the 2026-05-18 brainstorming session: the bound *subgroup method* really is messy — method-chained hierarchical declaration that mutates global state — and dropping it is correct. The bound *command decorator*, however, is the canonical CLI-framework idiom (Click, Typer, Flask, FastAPI, argparse subparsers). For single-file projects it reads more naturally than the string-key form. The string-key form earns its complexity precisely when projects scale across multiple files, where the captured-binding pattern stops working without shared imports.

## Goal

Stop emitting deprecation warnings for `@group.command` while keeping `@command(group="…")` as the recommended path for cross-file or nested usage. Keep the bound-subgroup deprecation as-is.

## Non-goals

- No change to the Rust static parser. It already accepts both decorator forms; this design touches only the Python registration layer and the documentation that surrounds it.
- No change to the manifest fragment schema.
- No automated code-mod or rewriter for existing legacy usage. Both forms continue to work; no migration is required for the bound command decorator.

## Code change

Single file: `crates/toolr-py/python/toolr/_decorators.py`.

1. Delete the `_emit_legacy_command_warning(group_full_name)` call inside `CommandGroup.command`. The method becomes a thin alias for the existing private `_command` helper that already does the work without warning.
2. Delete the `_emit_legacy_command_warning` function itself — no remaining call sites.
3. Update the `CommandGroup.command` docstring: remove the `.. deprecated::` admonition and describe it as the canonical single-file form. Keep the cross-reference to `@command(group="…")` for the multi-file case.
4. Leave `_emit_legacy_command_group_method_warning` and the `CommandGroup.command_group` call site intact. The subgroup-method form remains deprecated.

No behavioural change beyond "the warning vanishes from `@group.command`". The static parser already accepts both decorator forms; manifest-build is unaffected.

## Documentation changes

| File | Change |
|---|---|
| `docs/quickstart.md` | Rewrite the headline example to use `group = command_group(...)` + `@group.command`. Drop the standalone `command_group(...)` call (which reads as a side-effect-only statement). |
| `docs/quickstart-files/*.py` | Update the doc-snippet fixture files referenced by the `--8<--` includes to match. |
| `docs/writing-commands/groups.md` | Lead with the bound-decorator form as canonical. Add a short forward-pointer near the end: "If your tools span multiple files, see *Scaling command groups across files*." |
| `docs/writing-commands/files/groups-example.py` | Update to bound-decorator form. |
| `docs/writing-commands/across-files.md` *(new)* | Introduce `@command(group="…")`. Explains *why* the string-key form exists: files become decoupled (no shared `CommandGroup` import), scan-order independence, typo safety at manifest-build time with "did you mean…" suggestions, and forward compatibility once `parent.command_group("child", ...)` is removed in 1.0. The four "Why migrate" bullets currently in `docs/migration.md` get rewritten and moved here — they always described scaling benefits rather than migration tax. |
| `docs/writing-commands/nesting.md` | Already uses dotted strings. Add a one-line note that the legacy `parent.command_group("child", ...)` method form is being removed in 1.0 and link to the migration doc. |
| `docs/migration.md` | Shrink. Covers only `parent.command_group("child", ...)` → `command_group("parent.child", ...)`. The `@group.command` rows in the conversion table are removed. The "Why migrate" section is reduced to the forward-compatibility argument (because the other three reasons now live in `across-files.md`). |
| `mkdocs.yml` | Add the new `across-files.md` page under the "Writing commands" navigation section, between `groups.md` and `nesting.md`. |

The new page's title — H1, `mkdocs.yml` nav label, and cross-references from `groups.md` and `migration.md` — is **"Scaling command groups across files"**.

## Test changes

- `tests/decorators/test_decorators_unit.py::test_legacy_group_command_decorator_returns_callable_unchanged` and `test_legacy_group_command_with_explicit_name_returns_decorator` currently allow the deprecation warning to fire silently. Convert them to *forbid* the warning: use `warnings.catch_warnings()` with `simplefilter("error", ToolrDeprecationWarning)` so any future regression re-introducing the warning fails the test.
- Add a sibling test for the still-deprecated path: `parent.command_group("child", ...)` continues to fire `ToolrDeprecationWarning`. (May already exist; verify and add if missing.)
- Existing introspect / runner tests that use `@group.command` in tempdir fixtures continue to work as-is — the warning simply no longer fires, and the surrounding code never relied on it.

## Migration considerations

This change *reduces* deprecation noise. No user code becomes broken. Anyone who migrated from `@group.command` to `@command(group="…")` keeps working. Anyone who did not migrate stops seeing the warning. No coordination required.

The remaining deprecation — `parent.command_group("child", ...)` — continues to fire its warning, and its 1.0 removal stays on the roadmap.

## Risks

- **Doc surface inconsistency.** Some readers will discover the bound decorator via the rewritten quickstart, others via the existing `@command(group="…")` examples in the wild. Mitigated by `across-files.md` explaining when each is appropriate and `groups.md` cross-linking to it.
- **Drift over time.** If future framework changes make the bound decorator structurally awkward (e.g., a new kwarg that only makes sense on `@command(group=…)`), the two forms will diverge. We accept this risk; the bound decorator is intentionally the simple-case form and missing-an-advanced-kwarg is the expected failure mode.

## Out-of-scope follow-ups (not part of this work)

- Removing `_emit_legacy_command_group_method_warning` and the bound-subgroup code path entirely. This happens in 1.0, not here.
- A code-mod that bulk-rewrites `parent.command_group("child", ...)` callers to dotted strings. Single-call refactor; doesn't justify a tool.

## Approval

User approved the design via brainstorming session on 2026-05-18.
