# argparse scanner: full `nargs` support

- **Status:** design
- **Date:** 2026-08-04
- **Issue:** [s0undt3ch/ToolR#423](https://github.com/s0undt3ch/ToolR/issues/423)
- **Branch:** stacked on `fix-argparse-repeated-flag-form` (git-spice), own branch

## Problem

The argparse scanner's `ArgumentKind` has no representation for `nargs=N` (fixed
int arity), `argparse.REMAINDER`, or a keyword-omitted `nargs="?"` on a
positional. All three degrade to a single-value `Positional`/`Optional` today,
and the scanner emits a warning flagging the mismatch
(`crates/toolr-core/src/argparse/scan.rs`, the `nargs_repr` warning block).
`DispatchCommand.argv` (Python) then reconstructs the wrong invocation for
anything wrapping a real CLI built on these forms.

This is not a clap limitation — `Arg::num_args(n)` already does fixed counts.
The gap is entirely toolr's own representation and plumbing.

## Scope

All three forms get first-class treatment:

- `nargs=N` (int) — keyword or positional.
- `argparse.REMAINDER` — positional only (its only sane use in argparse itself;
  a keyword-style occurrence keeps warning as unsupported).
- Positional `nargs="?"` — zero-or-one value.

Not in scope: `nargs=N` combined with `action="append"` (repeating groups of N
values) — stays unsupported/warned. Rare combination, no evidence of use.

## `ArgumentKind`: two new variants, not three

- **`FixedArity`** — nargs=N, keyword or positional style. Disambiguated the
  same way the rest of the codebase already does: `arg.long_flag.is_some()` ⟺
  keyword-style (this invariant already holds — `long_flag` is `None` for every
  positional and `Some` for every keyword-style arg).
- **`OptionalPositional`** — positional `nargs="?"`: zero-or-one occurrence,
  single value. Distinct from `Positional` (always required) and
  `VarPositional` (zero-or-more, greedy/trailing) — it's neither.
- **`argparse.REMAINDER` gets no new variant.** It classifies as
  `VarPositional`, identically to positional `nargs="*"` — same clap shape
  (trailing, zero-or-more), same dispatch argv. The only work is teaching the
  nargs-value parser to recognise the `argparse.REMAINDER` attribute
  expression and treat it as `"*"` from that point on.

## One metadata field replaces three

Today `ArgMetadata` (manifest) and `ArgSchema` (Python wire type) each carry two
special-purpose, redundant fields:

- `multi_value_occurrence: bool` — true iff `is_keyword_style && nargs ∈
  {"+","*"}` for a `Repeated` arg. Fully derivable from `nargs` once `nargs` is
  actually populated (it's always `None` today — that's the whole reason this
  bool exists).
- `require_at_least_one: bool` — true iff `nargs == "+"` on a `VarPositional`.
  Same story.

Once we populate `nargs` properly for `Repeated`/`VarPositional`/`FixedArity`,
both bools become redundant restatements of `nargs`. Rather than bolt on a
third special-purpose field (`arity: Option<usize>`) alongside two that are now
provably derivable from the thing we're about to start populating anyway, fold
all three into one field:

```rust
// crates/toolr-core/src/manifest/model.rs
pub enum Nargs {
    Question,
    Plus,
    Star,
    Fixed(usize),
}

pub struct ArgMetadata {
    // ...
    pub nargs: Option<Nargs>, // replaces multi_value_occurrence + require_at_least_one
}
```

`OptionalPositional` does not need an `ArgMetadata.nargs` entry — the "?" shape
is already implied by the kind itself.

This single field drives every clap decision that the two bools used to split
across three call sites in `crates/toolr/src/cli.rs`:

- `Repeated` + `nargs ∈ {Plus, Star}` → `num_args(1..)`; `nargs == None` (source
  `action="append"`) → `num_args(1)`.
- `VarPositional` + `nargs == Plus` → `required(true).num_args(1..)`;
  `nargs ∈ {Star, None}` → `required(false).num_args(0..)` (REMAINDER lands
  here as `Star`).
- `FixedArity` + `nargs == Fixed(n)` → `num_args(n)`.

## Wire format (`toolr.sources.ArgSchema` / `DispatchCommand.argv`)

No new wire field. `ArgSchema.nargs: Literal["*", "+", "?"] | int | None`
already exists and has been unused (`always None`) since it was added — this
work is what populates it. `multi_value_occurrence` is deleted from the
Python struct.

- `FixedArity` maps to the existing `"optional"`/`"positional"` wire `kind`
  strings (same `long_flag.is_some()` split), with `nargs` set to the int
  arity. `command_args[name]` becomes a JSON array of length N instead of a
  scalar.
- `VarPositional` and `OptionalPositional` both map to wire `kind="positional"`
  — not `"repeated"` — with `nargs` set to `"+"`/`"*"` (`VarPositional`) or
  `"?"` (`OptionalPositional`). `"repeated"` is reserved for genuinely
  keyword-style `Repeated` args, populating `nargs` as `"+"`/`"*"`/`None`.
  `DispatchCommand.argv`'s `"repeated"` branch switches from
  `if arg.multi_value_occurrence` to `if arg.nargs in ("+", "*")`; its
  `"positional"` branch grows a matching `isinstance(nargs, int) or nargs in
  ("+", "*")` case for the array-valued forms. An absent `OptionalPositional`
  value needs no `argv` change — it's already omitted by the existing
  "skip if not in `command_args`" loop.

### Bug found and fixed in passing

`DispatchCommand.argv`'s `"repeated"` branch unconditionally calls
`_flag_for_arg(arg)` — including for a genuinely positional `VarPositional`
arg, since `VarPositional` used to share the `"repeated"` wire `kind` with
keyword-style `Repeated`. `arg.long_flag is None` is *not* a reliable signal
to special-case there: it also legitimately occurs for keyword-style
`Repeated` args from native commands or older manifests that never recorded
a literal flag spelling (`_flag_for_arg` synthesizes `--name` for exactly
that case). Conflating the two would misclassify real keyword args as
positional.

The actual fix is a cleaner wire-kind split, not a runtime check:
`VarPositional` and `OptionalPositional` now map to wire `kind="positional"`
(never `"repeated"`) — every positional-style arg (no CLI flag, ever) shares
one wire kind, parameterised by `nargs` for its exact arity, so
`DispatchCommand.argv` never has to guess whether a `"repeated"` arg has a
flag to emit. This was untested before since no positional `nargs="+"/"*"`
argparse arg had ever been dispatched through this path; REMAINDER support
exercises it for the first time.

## Schema version bump

`RUNNER_SCHEMA_VERSION` (Rust, `crates/toolr-core/src/execute/spec.rs`) and
`SCHEMA_VERSION` (Python, `crates/toolr-py/python/toolr/_runner.py`) both bump
2 → 3. Triggers, per the existing "when to bump" doc comment:

- `command_args[name]` changes shape (scalar → array) under the existing
  `"optional"`/`"positional"` wire kinds when `nargs` is an int — an old
  runner reading a new binary's spec would do `str([...])` instead of
  iterating.
- `nargs` goes from always-`None` to populated, and `argv`'s reconstruction
  logic changes to consume it — an old runner ignores it and misreconstructs
  argv for every one of these forms.
- `multi_value_occurrence` is removed from `ArgSchema` — an old runner's
  schema decode of a struct built with `msgspec` would need the field to still
  exist (or a compatible default) to decode a *new* runner's serialized
  fixture in the other direction; treating this as a clean cut is simpler and
  is exactly what the version gate exists to signal.

## Scanner changes (`crates/toolr-core/src/argparse/scan.rs`)

- `nargs` kwarg parsing currently only handles `nargs="+"/"*"/"?"` via
  `literal_str` (string literals). Extend to capture `Expr::NumberLiteral` ints
  and to recognise the `argparse.REMAINDER` attribute expression (normalising
  it to the same internal representation as `"*"` from that point on).
- `classify_kind`: keyword-style + int nargs (and not `action="append"`) →
  `FixedArity`; positional-style + int nargs → `FixedArity`; positional-style +
  `"?"` → `OptionalPositional`; positional-style + `REMAINDER` → `VarPositional`
  (alongside the existing `"+"`/`"*"` → `VarPositional` mapping).
- The `nargs_repr` "unsupported" warning's `handled` set grows to match: int
  nargs (unless paired with `action="append"`), `REMAINDER` on a positional,
  and `"?"` on a positional (today only keyword `"?"` is silently accepted;
  positional `"?"` gets first-class treatment now instead of a warning).

## Validation (`crates/toolr-core/src/parser/build.rs`)

`validate_positional_arity` already tracks a "zero-or-one positional" slot
(today detected via `resolved_type == Some(SupportedType::Optional(_))`, which
never fires for argparse-scanned args since those never get a `resolved_type`).
Extend the same zero-or-one bookkeeping to also fire on
`ArgumentKind::OptionalPositional`, so the existing "required positional after
a zero-or-one" and "multiple zero-or-one positionals" errors apply uniformly
regardless of source.

## Touched files

- `crates/toolr-core/src/manifest/model.rs` — `Nargs` enum, `ArgumentKind`
  variants, `ArgMetadata.nargs` (removes `multi_value_occurrence`,
  `require_at_least_one`).
- `crates/toolr-core/src/argparse/scan.rs` — nargs parsing, `classify_kind`,
  warning gate.
- `crates/toolr-core/src/parser/build.rs` — `validate_positional_arity`.
- `crates/toolr-core/src/execute/spec.rs` — `RUNNER_SCHEMA_VERSION` → 3;
  `ArgSchemaSpec` drops `multi_value_occurrence`.
- `crates/toolr/src/cli.rs` — clap `Arg` construction for the two new kinds and
  the consolidated `nargs`-driven branches for `Repeated`/`VarPositional`.
- `crates/toolr/src/execute_build.rs` — `kind`/`nargs` wire mapping,
  `extract_value` for `FixedArity`/`OptionalPositional`.
- `crates/toolr-core/src/complete/engine.rs`, `crates/toolr-core/src/third_party/model.rs`,
  `crates/toolr/src/builtin_completions.rs` — exhaustive `ArgumentKind` matches
  (compiler-directed).
- `crates/toolr-py/python/toolr/_runner.py` — `SCHEMA_VERSION` → 3.
- `crates/toolr-py/python/toolr/sources/_types.py` — drop
  `multi_value_occurrence` from `ArgSchema`.
- `crates/toolr-py/python/toolr/sources/_dispatch.py` — `argv` property:
  `nargs`-driven branches, the positional-repeated flag fix.
- `UNRELEASED.md` — release note.

## Testing

- Rust: `scan.rs` unit tests for `nargs=2` (keyword and positional),
  `nargs=argparse.REMAINDER`, positional `nargs="?"`, and the still-warned
  `nargs=N` + `action="append"` combination.
- Rust: `parser/build.rs` clap-construction tests asserting `num_args` for the
  new kinds; `validate_positional_arity` test for `OptionalPositional` in the
  zero-or-one slot.
- Rust: `execute_build.rs` extraction tests for `FixedArity` (array value) and
  `OptionalPositional` (present/absent).
- Python: `tests/sources/test_dispatch.py` cases for fixed-arity
  optional/positional argv reconstruction and the fixed positional-repeated
  (no-flag) case.
- `cargo xtask build-skill-refs --check` — public surface changed
  (`ArgumentKind` variants), regenerate and commit.
