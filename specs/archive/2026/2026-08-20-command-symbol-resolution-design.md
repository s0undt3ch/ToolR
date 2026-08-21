# Static symbol resolution for command-signature types

**Status:** Draft (2026-08-20)
**Topic:** Replace the `EnumTable` collision heuristics from #449/#454 with an explicit,
import-following resolver, and fix the runtime coercion gap that lets an unresolvable
annotation silently disable type coercion for a whole command.

## Background

[GH #454](https://github.com/s0undt3ch/ToolR/issues/454): `toolr project manifest rebuild`
hard-fails when a `@command`-decorated function uses an `Enum`-typed parameter imported from a
different module than the one declaring it, if any *unrelated* module also declares a same-named
enum class. The #449 fix (`cd84b8a1`) keyed `EnumTable` by defining module to stop
same-named-class collisions from clobbering each other, but its fallback for the "imported from
elsewhere" case was a guess — "if there's exactly one cross-module definition, use it" — which
breaks the moment a second, unrelated module declares the same class name, even though the
importing module's own import statement unambiguously names the real source.

A narrower dedupe-by-identical-members patch (committed `3fd7e35f`) fixes the exact shape
reported in #454 (three modules with byte-identical `Environment(StrEnum)` definitions) but does
nothing for the general case: two modules with a same-named-but-*different* enum, where the
importing module's own `from X import Y` statement makes the answer unambiguous. This design
supersedes that patch with real import resolution.

Two more findings surfaced during design review, both confirmed empirically (see Verification
below), that this design also fixes:

1. **`allowed_values` silently empty for `Optional`-wrapped types.** `Environment | None`
   annotations (the actual shape in the #454 report) built successfully but populated
   `allowed_values: []` — no clap choice validation, nothing in `--help`. Root cause:
   `collect_allowed_values` (signatures.rs) and the `commands.rs` backfill loop only matched a
   bare `Expr::Name` / `SupportedType::Literal`, never peeling an `Optional` wrapper or matching
   `SupportedType::Enum`. **Already fixed** on branch `fix/enum-import-cross-module-454` — see
   commit fixing `commands.rs`'s backfill loop. Not part of this plan's task list; noted here for
   the record.
2. **One unresolvable annotation zeroes out type coercion for an entire command.**
   `_runner.py::_coerce_args` calls `get_type_hints(target)` once per command invocation, with a
   blanket `try/except Exception: hints = {}`. If *any* parameter's annotation can't be resolved
   (e.g. a `TYPE_CHECKING`-only import), the whole call raises `NameError`, the except clause
   swallows it, and `hints` becomes `{}` — silently disabling coercion for *every* parameter on
   that command, not just the offending one. Confirmed by direct reproduction (see Verification).

## Goal

After this work lands:

- Every name used in a `@command` function's parameter annotation or default **must** resolve via
  one of exactly two paths: declared in the same module, or reachable through that module's own
  explicit imports. Nothing else — no cross-module guessing, no reliance on some other module
  having imported the name into a shared namespace.
- `from foo import EnumA as EnumB` resolves (aliased import).
- Import chains resolve, including through `__init__.py` re-exports: `from tools.metrics import X`
  where `X` is itself imported into `metrics/__init__.py` from `metrics/_common.py`.
- Relative imports resolve: `from . import X`, `from .sibling import X`, `from ..pkg.mod import X`.
- `try:`/`except ImportError:` dual-path imports resolve; if the two branches name different
  defining modules, the existing identical-members dedupe (from `3fd7e35f`) is the tie-break —
  same members ⇒ resolves, different members ⇒ ambiguous, hard error.
- `if TYPE_CHECKING:` imports (module-top-level only, not nested in a function/class) resolve
  **and actually work at runtime** — not just at manifest-build time. The runtime never depends
  on the guarded block having executed; it lazily imports the real defining module at coercion
  time and injects the resolved class into `get_type_hints`'s `localns`, so `TYPE_CHECKING`'s
  own circular-import-avoidance purpose is preserved (nothing forces the guarded block to
  execute at the target module's own import time).
- `_coerce_args` failing to resolve type hints for one parameter no longer disables coercion for
  the rest of that command's parameters.
- Two constructs remain **hard build errors**, always, when the unresolved name is needed for a
  command's parameter type or default:
    - `from foo import *` (star imports) — can't know the source of any given name.
    - `import foo.bar.baz` + `foo.bar.baz.X` attribute-chain usage — must be
  `from foo.bar.baz import X` instead. Message points at the fix.
    - Any name that simply isn't found via same-module declaration or a traceable import (typo,
  forgotten import, etc.)

## Non-goals

- No support for imports inside function/class bodies (annotations are evaluated against
  module/enclosing scope, not call-time locals) — same documented gap as `SourcesImports`.
- No support for `import foo.bar.baz` + attribute-chain annotations for arbitrary user classes.
  `toolr.types`/`toolr.sources`'s own existing one-level dotted-attribute support (`TypeImports`,
  `SourcesImports`) is untouched — this restriction is for the *new*, general resolver only.
- No resolution across the local-`tools/`-tree / third-party-plugin-package boundary — an import
  naming a module outside the currently-scanned tree (external package, or a plugin package while
  building the local tree, or vice versa) is "can't resolve, out of scope," not a silent
  fallthrough.
- No attempt to make arbitrary non-`Enum`/`Literal` `TYPE_CHECKING`-guarded types work at runtime
  beyond what this design's `localns` injection naturally covers. `toolr.types` aliases and
  primitives don't need runtime class resolution at all (they're never wrapped by a
  `TYPE_CHECKING` guard in practice — they're always real, cheap, unconditional imports).

## Architecture

```text
┌──────────────────────────────────────────────────────────────────────┐
│ Pass 1 (build.rs): per module                                        │
│                                                                        │
│   ImportTable::from_module(module, module_path)                      │
│     walks module.body top-level statements:                          │
│       - Stmt::ImportFrom  → local_name -> (module_ref, orig_name)     │
│       - Stmt::Import      → (rejected as annotation source; only     │
│                              tracked to detect + reject attribute-    │
│                              chain usage with a clear message)        │
│       - Stmt::If where test is `TYPE_CHECKING`/`typing.TYPE_CHECKING`│
│         and the If is a direct child of module.body → walk its body  │
│         the same way, tagging entries `via_type_checking: true`      │
│       - Stmt::Try → walk `try.body` AND each `handler.body` the same │
│         way, collecting *all* candidate (module_ref, orig_name)      │
│         pairs per local name (branch-disagreement candidates)        │
│       - Stmt::ImportFrom with a `*` alias → record local wildcard    │
│         marker (no name entries; used only for error messages)       │
│                                                                        │
│   module_ref carries level (relative dots) + raw module string;      │
│   resolved to an absolute dotted path once module_path_for_prefix    │
│   for the *importing* module is known (relative import math).        │
│                                                                        │
│ EnumTable::from_module unchanged (still records ClassDef -> members  │
│ per declaring module) — this pass only adds import bookkeeping.      │
├──────────────────────────────────────────────────────────────────────┤
│ Pass 2 (build.rs / commands.rs / resolve.rs): per command             │
│                                                                        │
│   resolve_name(name, enums, imports: &ImportTable, module) now:      │
│     1. same-module ClassDef?              -> use it                  │
│     2. ImportTable has an entry for name?                            │
│          -> follow chain (cycle+depth guarded, mirrors               │
│             TypeAliasTable::lookup) to the module that actually      │
│             declares the class (has the ClassDef)                    │
│          -> if branch-disagreement candidates: identical-members     │
│             dedupe tie-break, else ambiguous -> UnsupportedType      │
│     3. name found only via a `*` marker or an `import X` + attribute │
│        chain -> UnsupportedType with a specific, actionable message  │
│     4. not found anywhere -> UnsupportedType (unchanged, generic)    │
│                                                                        │
│   SupportedType::Enum gains `module: String` (the resolved,          │
│   absolute defining module) alongside existing `name`/`values`.      │
│   This is the field the Python runtime needs for lazy import.        │
├──────────────────────────────────────────────────────────────────────┤
│ Manifest schema (RUNNER_SCHEMA_VERSION / SCHEMA_VERSION bump)         │
│                                                                        │
│   Argument's serialised enum type gains `"module"` alongside         │
│   existing `"name"`/`"allowed_values"` fields.                       │
├──────────────────────────────────────────────────────────────────────┤
│ Python runtime (toolr-py/python/toolr/_runner.py)                    │
│                                                                        │
│   _coerce_args:                                                      │
│     - build a `localns` dict up front from the manifest's per-       │
│       argument enum `module`/`name` fields: for each Enum-typed      │
│       argument, lazily `importlib.import_module(module)` +           │
│       `getattr(mod, name)`, keyed by `name` in localns.               │
│     - call `get_type_hints(target, localns=localns)` instead of      │
│       plain `get_type_hints(target)` — this fixes both the            │
│       TYPE_CHECKING case (name now resolvable) and the "whole        │
│       function's hints wiped by one bad annotation" case (localns    │
│       supplies exactly the missing names, everything else still      │
│       resolves from real globals as before).                         │
│     - the existing `except Exception: hints = {}` fallback stays as  │
│       a last resort for anything localns doesn't cover, but is no    │
│       longer the common path for this scenario.                     │
└──────────────────────────────────────────────────────────────────────┘
```

## Verification (done during design, not implementation)

Both empirical claims behind this design were checked directly, not assumed:

```text
$ get_type_hints() on a function whose only annotation is TYPE_CHECKING-guarded
  -> NameError: name 'Environment' is not defined
  (true both with a quoted forward-ref annotation and with
  `from __future__ import annotations`; ruff_python_ast parses the annotation
  as Expr::Name either way — the future-import is a no-op for our AST walker)

$ Same function, get_type_hints(fn, localns={'Environment': Environment})
  -> succeeds: {'env': <enum 'Environment'>}
```

This confirms both (a) the runtime bug is real and (b) the `localns` fix works.

## Decisions made during review (for the record)

- **Rejected:** monkeypatching `typing.TYPE_CHECKING = True` before importing the target module.
  Forces the *entire* guarded block to execute at the target module's own import time — which can
  contain more than the needed import (e.g. a genuinely dev-only/stub-only import not installed
  at runtime), and re-introduces the circular-import problem `TYPE_CHECKING` exists to dodge.
  Lazy, per-parameter import at coercion time (long after all modules have finished loading) has
  neither problem.
- **Rejected:** treating `EnumTable`'s "single unambiguous cross-module def" and "identical
  members" heuristics as sufficient long-term. They stay as the *last-resort* tie-break for
  `try/except` branch disagreement (a case where we deliberately don't know which branch actually
  ran), but the primary path is always "did an explicit import say so."
- **Rejected:** general dotted-attribute-chain resolution (`import foo.bar.baz` +
  `foo.bar.baz.X`) for arbitrary user classes. Kept for `toolr.types`/`toolr.sources` only (already
  shipped, already tested, narrow surface). New code must not extend this pattern.
- **Rejected:** single-hop-only import resolution (no chain-following). `__init__.py` re-exports
  are common practice in the wild; supporting them requires the same cycle-guarded chain walk as
  aliasing, so there's no simplification to be had by refusing chains.
- **`from __future__ import annotations`:** confirmed to be a complete no-op for the static
  parser (doesn't change what `ruff_python_ast` produces). Raises the *importance* of getting
  static resolution right, since it removes Python's own def-time validation, but the resolver
  we're building already treats "must resolve" as a hard, always-on invariant, so this doesn't
  change scope — see prior point in this same review.

## Correction found during implementation (Task 8)

This design originally stated the `SupportedType::Enum.module` field would require bumping
`RUNNER_SCHEMA_VERSION` (`crates/toolr-core/src/execute/spec.rs`) and `toolr-py`'s `SCHEMA_VERSION`
in lock-step. That was wrong: those constants govern the *dispatch-time* JSON payload between the
`toolr` binary and the Python runner subprocess (`ExecutionSpec`/`DispatchSpec` — raw argument
*values*, not type schema). `SupportedType` is never part of that payload. The field actually lives
in the *static build-time manifest* (`manifest/model.rs::Argument.resolved_type`), whose own
`SCHEMA_VERSION` doc comment says "bump on breaking format changes" — and adding a
`#[serde(default)]`-annotated field to an existing variant is non-breaking (old manifests without
the field still deserialise; the field defaults to an empty string, and the field is only ever read
for `Enum`-typed arguments where a normal rebuild will populate it correctly anyway). No version
bump was needed anywhere; `#[serde(default)]` on the new field was sufficient.
