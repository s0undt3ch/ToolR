# argparse scanner: full `nargs` support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the argparse scanner first-class representations for `nargs=N`
(fixed arity), `argparse.REMAINDER`, and positional `nargs="?"`, end-to-end
through the manifest, clap CLI construction, completion, and dispatch argv
reconstruction — closing s0undt3ch/ToolR#423.

**Architecture:** Two new `ArgumentKind` variants (`FixedArity`,
`OptionalPositional`); REMAINDER reuses the existing `VarPositional` variant.
A single `ArgMetadata.nargs: Option<Nargs>` field replaces the two
now-redundant `multi_value_occurrence`/`require_at_least_one` bools and also
carries the fixed-arity count. The wire schema (`ArgSchema.nargs`) goes from
always-`None` to populated, `ArgSchema.multi_value_occurrence` is deleted, and
`DispatchCommand.argv` (Python) is taught to read `nargs` directly. This is a
protocol change, so `RUNNER_SCHEMA_VERSION`/`SCHEMA_VERSION` bump 2 → 3.

**Tech Stack:** Rust (toolr-core, toolr binary, clap), Python (toolr-py,
msgspec).

## Global Constraints

- Conventional Commits for every commit (`fix(argparse): …`, `feat(cli): …`).
- No `--no-verify`. Pre-commit (`prek`) must pass on every commit.
- `RUNNER_SCHEMA_VERSION` (Rust) and `SCHEMA_VERSION` (Python) must be bumped
  together, kept numerically identical — enforced by
  `crates/toolr-core/tests/schema_version_lockstep.rs`.
- Manifest `SCHEMA_VERSION` (`crates/toolr-core/src/manifest/model.rs`) stays
  at `1` — new `ArgumentKind` variants and the `ArgMetadata.nargs` field are
  additive, matching how prior variants (`Count`) and metadata fields
  (`multi_value_occurrence`, `require_at_least_one`) were added without a
  bump.
- `cargo xtask build-skill-refs --check` must pass (public surface changed —
  new `ArgumentKind` variants); regenerate with `cargo xtask build-skill-refs`
  and commit the diff.
- Queue a release note in `UNRELEASED.md`. Never hand-edit `CHANGELOG.md`.
- Run `mise run test` before considering the branch done (touches both Rust
  and Python).

---

### Task 1: `Nargs` enum + new `ArgumentKind` variants + `ArgMetadata.nargs`

**Files:**

- Modify: `crates/toolr-core/src/manifest/model.rs`
- Test: same file, `#[cfg(test)]` (or nearest existing manifest test module —
  check `crates/toolr-core/src/manifest/mod.rs`/`tests.rs` for where
  `ArgMetadata`/`ArgumentKind` round-trip tests currently live and add
  alongside them)

**Interfaces:**

- Produces: `pub enum Nargs { Question, Plus, Star, Fixed(usize) }` (derives
  `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`).
- Produces: `ArgumentKind::FixedArity`, `ArgumentKind::OptionalPositional`.
- Produces: `ArgMetadata.nargs: Option<Nargs>` (replaces
  `multi_value_occurrence: bool` and `require_at_least_one: bool` — delete
  both fields).
- [ ] **Step 1: Add the `Nargs` enum, immediately above `ArgumentKind`**

```rust
/// Extra `nargs` shape carried from an argparse `add_argument(...)` call,
/// beyond what `ArgumentKind` alone implies.
///
/// - `Repeated`: `Some(Plus | Star)` means a single occurrence takes
///   several space-separated values (`nargs="+"`/`"*"`) rather than one
///   value per occurrence (`action="append"`, which leaves this `None`).
/// - `VarPositional`: `Some(Plus)` requires at least one value;
///   `Some(Star)` or `None` allows zero (covers `nargs="*"` and
///   `argparse.REMAINDER`, which are equivalent for this purpose).
/// - `FixedArity`: always `Some(Fixed(n))` — the exact value count.
/// - Every other kind: always `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nargs {
    Question,
    Plus,
    Star,
    Fixed(usize),
}
```

- [ ] **Step 2: Add the two new `ArgumentKind` variants**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentKind {
    /// Single required positional value (`def f(ctx, name: str)`).
    Positional,
    /// Single optional keyword (`--name VALUE`, with a default).
    Optional,
    /// No-value boolean keyword (`--verbose`, `bool = False`).
    Flag,
    /// Repeatable keyword that appends each occurrence
    /// (`def f(ctx, items: list[str] = [])` → `--items a --items b`).
    Repeated,
    /// Variadic trailing positional (`def f(ctx, *files: str)` → `toolr ... a.py b.py`).
    /// Also covers argparse positional `nargs="*"`/`"+"`/`argparse.REMAINDER`.
    VarPositional,
    /// Counting flag (`-vvv` → 3) via clap `ArgAction::Count`. Drives
    /// `toolr.types.Count`-annotated parameters; the runtime value
    /// passed to the Python function is the resulting integer.
    Count,
    /// Fixed-arity value(s) in a single occurrence (argparse `nargs=N`).
    /// Keyword-style (`--pair a b`) when `long_flag` is `Some`;
    /// positional (`files a b`, no flag) when `None`. The exact count
    /// lives on `ArgMetadata.nargs` as `Nargs::Fixed(n)`.
    FixedArity,
    /// Zero-or-one positional value (argparse positional `nargs="?"`).
    /// Distinct from `Positional` (always required) and `VarPositional`
    /// (zero-or-more, trailing/greedy).
    OptionalPositional,
}
```

- [ ] **Step 3: Replace the two bools on `ArgMetadata` with `nargs`**

Delete these two fields (and their doc comments):

```rust
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multi_value_occurrence: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub require_at_least_one: bool,
```

Add in their place:

```rust
    /// Extra `nargs` shape from the source. See [`Nargs`] for the
    /// per-kind meaning. `None` for every kind that doesn't carry one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nargs: Option<Nargs>,
```

- [ ] **Step 4: Update `ArgMetadata::is_empty`**

```rust
impl ArgMetadata {
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
            && self.metavar.is_none()
            && self.env.is_none()
            && !self.hide
            && self.display_order.is_none()
            && self.help_section.is_none()
            && self.conflicts_with.is_empty()
            && self.requires.is_empty()
            && self.nargs.is_none()
    }
}
```

- [ ] **Step 5: `cargo check -p toolr-core` — expect a wall of errors**

Every non-exhaustive `match` on `ArgumentKind` across the workspace now fails
to compile (that's intentional — it's the compiler enumerating every call
site the rest of this plan must touch). Every reference to
`.metadata.multi_value_occurrence` / `.metadata.require_at_least_one` also
fails. Do not fix them here — that's Tasks 2–7. Just confirm the error list
matches: `crates/toolr-core/src/argparse/scan.rs`,
`crates/toolr-core/src/parser/build.rs`, `crates/toolr/src/cli.rs`,
`crates/toolr/src/execute_build.rs`, and any manifest/schema test fixtures
that construct `ArgMetadata { .. }` with the old field names.

- [ ] **Step 6: Commit**

This won't compile yet (expected — the rest of the plan fixes call sites).
Stage and hold locally; fold into Task 2's commit instead of committing
broken code:

```bash
git add crates/toolr-core/src/manifest/model.rs
```

(No commit yet — continue straight into Task 2, then commit both together.)

---

### Task 2: Scanner — parse int/REMAINDER nargs, classify the new kinds, populate `Nargs`

**Files:**

- Modify: `crates/toolr-core/src/argparse/scan.rs`
- Test: same file, `#[cfg(test)] mod tests` at the bottom

**Interfaces:**

- Consumes: `Nargs`, `ArgumentKind::FixedArity`, `ArgumentKind::OptionalPositional`
  from Task 1.
- Produces: `build_argument` now sets `metadata.nargs` correctly and
  `classify_kind` returns the two new kinds; `nargs_value(expr) -> Option<String>`
  helper (replaces the `literal_str` call for the `nargs` kwarg) that also
  handles `NumberLiteral` ints and the `argparse.REMAINDER` attribute.
- [ ] **Step 1: Write the new/updated scanner tests**

Replace the existing `nargs_plus_and_star_classify_as_repeated` test (it
currently asserts `--pair` falls back to `Optional` with a warning — that
behavior is what we're removing) with:

```rust
#[test]
fn nargs_plus_and_star_classify_as_repeated() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--names', nargs='+', help='Names to greet')
    parser.add_argument('--tags', nargs='*', help='Optional tags')
"#;
    let scanned = scan_source("greet", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::Repeated);
    assert_eq!(scanned.arguments[0].metadata.nargs, Some(Nargs::Plus));
    assert_eq!(scanned.arguments[1].kind, ArgumentKind::Repeated);
    assert_eq!(scanned.arguments[1].metadata.nargs, Some(Nargs::Star));
}

#[test]
fn nargs_int_classifies_as_fixed_arity_keyword_and_positional() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--pair', nargs=2, help='Exactly two values')
    parser.add_argument('files', nargs=3, help='Exactly three files')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::FixedArity);
    assert_eq!(scanned.arguments[0].metadata.nargs, Some(Nargs::Fixed(2)));
    assert!(scanned.arguments[0].long_flag.is_some());
    assert_eq!(scanned.arguments[1].kind, ArgumentKind::FixedArity);
    assert_eq!(scanned.arguments[1].metadata.nargs, Some(Nargs::Fixed(3)));
    assert!(scanned.arguments[1].long_flag.is_none());
    assert!(
        scanned.warnings.iter().all(|w| !w.contains("unsupported nargs")),
        "fixed-arity nargs shouldn't warn any more, got {:?}",
        scanned.warnings
    );
}

#[test]
fn nargs_int_with_append_action_still_warns() {
    // Repeating groups of N (nargs=2 + action="append") isn't represented
    // — out of scope, still degrades to a single value with a warning.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--pairs', nargs=2, action='append', help='Pairs')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::Repeated);
    assert!(
        scanned.warnings.iter().any(|w| w.contains("--pairs") && w.contains("nargs=2")),
        "expected an unsupported-nargs warning, got {:?}",
        scanned.warnings
    );
}

#[test]
fn positional_nargs_question_mark_classifies_as_optional_positional() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--maybe', nargs='?', help='Optional single value')
    parser.add_argument('maybe_label', nargs='?', help='Optional positional')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::Optional);
    assert_eq!(scanned.arguments[1].kind, ArgumentKind::OptionalPositional);
    assert!(
        scanned.warnings.iter().all(|w| !w.contains("nargs")),
        "nargs=\"?\" on either style shouldn't warn any more, got {:?}",
        scanned.warnings
    );
}

#[test]
fn positional_nargs_plus_and_star_classify_as_var_positional() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('labels', nargs='+', help='One or more labels')
    parser.add_argument('extra', nargs='*', help='Zero or more extras')
"#;
    let scanned = scan_source("test", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::VarPositional);
    assert_eq!(scanned.arguments[0].metadata.nargs, Some(Nargs::Plus));
    assert_eq!(scanned.arguments[1].kind, ArgumentKind::VarPositional);
    assert_eq!(scanned.arguments[1].metadata.nargs, Some(Nargs::Star));
}

#[test]
fn positional_nargs_remainder_classifies_as_var_positional() {
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('rest', nargs=argparse.REMAINDER, help='Everything after')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert_eq!(scanned.arguments[0].kind, ArgumentKind::VarPositional);
    assert_eq!(scanned.arguments[0].metadata.nargs, Some(Nargs::Star));
    assert!(
        scanned.warnings.iter().all(|w| !w.contains("nargs")),
        "REMAINDER on a positional shouldn't warn any more, got {:?}",
        scanned.warnings
    );
}

#[test]
fn keyword_style_remainder_still_warns() {
    // argparse.REMAINDER on a keyword-style arg is nonsensical/unsupported
    // in argparse itself; scope this to positionals only and keep warning.
    let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--rest', nargs=argparse.REMAINDER, help='Weird')
"#;
    let scanned = scan_source("cmd", source).unwrap();
    assert!(
        scanned.warnings.iter().any(|w| w.contains("--rest") && w.contains("REMAINDER")),
        "expected an unsupported-nargs warning, got {:?}",
        scanned.warnings
    );
}
```

Update the two existing tests that assert the *old* behavior
(`nargs_question_mark_warns_on_positional_but_not_on_a_flag` — delete it, it's
superseded by `positional_nargs_question_mark_classifies_as_optional_positional`
above; and the old `nargs_plus_and_star_classify_as_repeated`'s `--pair`
assertions — already rewritten above without the `--pair` case, which moved to
`nargs_int_classifies_as_fixed_arity_keyword_and_positional`).

- [ ] **Step 2: Run the new tests to confirm they fail**

```sh
cargo test -p toolr-core --lib argparse::scan -- --nocapture
```

Expect compile errors (new variants/fields referenced don't exist in
`classify_kind`'s return type usage yet — actually they exist from Task 1;
expect *assertion* failures instead: `--pair`/`files` still classify as
`Optional`/`Positional` with the old warning).

- [ ] **Step 3: Add `use crate::manifest::Nargs;` to the imports**

```rust
use crate::manifest::{ArgMetadata, Argument, ArgumentKind, Nargs};
```

- [ ] **Step 4: Add the nargs-value parser, replacing the `literal_str` call**

Add this new function near `nargs_kwarg_repr`:

```rust
/// Normalise a `nargs=...` value into the internal string form
/// `classify_kind` and the warning gate match on: the literal string for
/// `"?"`/`"+"`/`"*"`, the decimal string for an int count, or the sentinel
/// `"REMAINDER"` for `argparse.REMAINDER` (recognised by attribute name
/// only — tolerant of any import alias for the `argparse` module).
fn nargs_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        Expr::NumberLiteral(n) => match &n.value {
            ruff_python_ast::Number::Int(i) => Some(i.to_string()),
            _ => None,
        },
        Expr::Attribute(a) if a.attr.as_str() == "REMAINDER" => {
            Some("REMAINDER".to_string())
        }
        _ => None,
    }
}
```

In `build_argument`, replace:

```rust
            "nargs" => {
                nargs = literal_str(&kw.value);
                nargs_repr = nargs_kwarg_repr(&kw.value);
            }
```

with:

```rust
            "nargs" => {
                nargs = nargs_value(&kw.value);
                nargs_repr = nargs_kwarg_repr(&kw.value);
            }
```

- [ ] **Step 5: Update `classify_kind`**

```rust
fn classify_kind(is_keyword_style: bool, action: Option<&str>, nargs: Option<&str>) -> ArgumentKind {
    if !is_keyword_style {
        return match nargs {
            Some("+") | Some("*") | Some("REMAINDER") => ArgumentKind::VarPositional,
            Some("?") => ArgumentKind::OptionalPositional,
            Some(n) if n.parse::<usize>().is_ok() => ArgumentKind::FixedArity,
            _ => ArgumentKind::Positional,
        };
    }
    match action {
        Some("store_true") | Some("store_false") => ArgumentKind::Flag,
        Some("append") => ArgumentKind::Repeated,
        _ => match nargs {
            Some("+") | Some("*") => ArgumentKind::Repeated,
            Some(n) if n.parse::<usize>().is_ok() => ArgumentKind::FixedArity,
            _ => ArgumentKind::Optional,
        },
    }
}
```

- [ ] **Step 6: Update the warning gate**

Replace:

```rust
    if let Some(raw) = &nargs_repr {
        let handled = matches!(nargs.as_deref(), Some("+") | Some("*"))
            || (is_keyword_style && nargs.as_deref() == Some("?"));
        if !handled {
            warnings.push(format!(
                "argparse: {name_for_warning}: unsupported nargs={raw} (treated as a single value; may not match argparse's runtime behaviour; see https://github.com/s0undt3ch/ToolR/issues/423)"
            ));
        }
    }
```

with:

```rust
    if let Some(raw) = &nargs_repr {
        let is_fixed_arity = nargs.as_deref().is_some_and(|n| n.parse::<usize>().is_ok());
        // nargs=N + action="append" (repeating groups of N) isn't
        // representable — stays unsupported/warned.
        let fixed_arity_with_append = is_fixed_arity && action.as_deref() == Some("append");
        let handled = !fixed_arity_with_append
            && (matches!(nargs.as_deref(), Some("+") | Some("*") | Some("?"))
                || (!is_keyword_style && nargs.as_deref() == Some("REMAINDER"))
                || is_fixed_arity);
        if !handled {
            warnings.push(format!(
                "argparse: {name_for_warning}: unsupported nargs={raw} (treated as a single value; may not match argparse's runtime behaviour; see https://github.com/s0undt3ch/ToolR/issues/423)"
            ));
        }
    }
```

- [ ] **Step 7: Update the metadata-population block**

Replace:

```rust
    if matches!(nargs.as_deref(), Some("+") | Some("*")) {
        if is_keyword_style {
            metadata.multi_value_occurrence = true;
        } else if nargs.as_deref() == Some("+") {
            metadata.require_at_least_one = true;
        }
    }
```

with (this runs after `let kind = classify_kind(...)` a few lines above it —
keep it in the same position):

```rust
    metadata.nargs = match kind {
        ArgumentKind::Repeated if is_keyword_style => match nargs.as_deref() {
            Some("+") => Some(Nargs::Plus),
            Some("*") => Some(Nargs::Star),
            // action="append": one value per occurrence, no extra shape.
            _ => None,
        },
        ArgumentKind::VarPositional => match nargs.as_deref() {
            Some("+") => Some(Nargs::Plus),
            // "*" and REMAINDER are both zero-or-more.
            _ => Some(Nargs::Star),
        },
        ArgumentKind::FixedArity => {
            nargs.as_deref().and_then(|n| n.parse::<usize>().ok()).map(Nargs::Fixed)
        }
        _ => None,
    };
```

- [ ] **Step 8: Run the tests to confirm they pass**

```sh
cargo test -p toolr-core --lib argparse::scan
```

- [ ] **Step 9: `cargo check -p toolr-core` and confirm remaining errors are
  scoped to `parser/build.rs`**

```sh
cargo check -p toolr-core
```

- [ ] **Step 10: Commit (both Task 1 and Task 2 together — Task 1 alone
  didn't compile)**

```bash
git add crates/toolr-core/src/manifest/model.rs crates/toolr-core/src/argparse/scan.rs
git commit -m "feat(argparse): classify nargs=N, REMAINDER, and positional nargs=\"?\"

Adds ArgumentKind::FixedArity and ArgumentKind::OptionalPositional.
REMAINDER reuses VarPositional (same clap shape and dispatch argv as
nargs=\"*\"). Folds the now-redundant multi_value_occurrence and
require_at_least_one ArgMetadata bools into a single nargs: Option<Nargs>
field, since both were always fully derivable from nargs once it's
actually populated instead of always None.

Part of #423."
```

---

### Task 3: Positional-arity validation covers `OptionalPositional` and positional `FixedArity`

**Files:**

- Modify: `crates/toolr-core/src/parser/build.rs`
- Test: same file, existing test module for `validate_positional_arity`
  (search `mod tests` in this file for existing `PositionalArityError` cases
  and add alongside them)

**Interfaces:**

- Consumes: `ArgumentKind::FixedArity`, `ArgumentKind::OptionalPositional`
  (Task 1).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn optional_positional_participates_in_zero_or_one_rules() {
    let commands = vec![cmd_with_args(
        "x",
        vec![
            arg(ArgumentKind::OptionalPositional, "first", None),
            arg(ArgumentKind::OptionalPositional, "second", None),
        ],
    )];
    let errors = validate_positional_arity(&commands);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors[0].kind,
        PositionalArityErrorKind::MultipleZeroOrOne { .. }
    ));
}

#[test]
fn required_positional_after_optional_positional_errors() {
    let commands = vec![cmd_with_args(
        "x",
        vec![
            arg(ArgumentKind::OptionalPositional, "opt", None),
            arg(ArgumentKind::Positional, "req", None),
        ],
    )];
    let errors = validate_positional_arity(&commands);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors[0].kind,
        PositionalArityErrorKind::RequiredAfterZeroOrOne { .. }
    ));
}

#[test]
fn positional_fixed_arity_after_optional_positional_errors() {
    // A fixed-N positional slot is always present, same as a plain
    // required Positional — can't follow a zero-or-one slot either.
    let commands = vec![cmd_with_args(
        "x",
        vec![
            arg(ArgumentKind::OptionalPositional, "opt", None),
            arg(ArgumentKind::FixedArity, "pair", None), // long_flag: None
        ],
    )];
    let errors = validate_positional_arity(&commands);
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors[0].kind,
        PositionalArityErrorKind::RequiredAfterZeroOrOne { .. }
    ));
}

#[test]
fn keyword_fixed_arity_does_not_compete_for_positional_slots() {
    let commands = vec![cmd_with_args(
        "x",
        vec![
            arg(ArgumentKind::OptionalPositional, "opt", None),
            arg_keyword(ArgumentKind::FixedArity, "pair"), // long_flag: Some("--pair")
        ],
    )];
    assert!(validate_positional_arity(&commands).is_empty());
}
```

Add the two small helpers these use if the test module doesn't already have
equivalents (check first — `arg(...)`/`cmd_with_args(...)`-shaped helpers
likely already exist for the other `validate_positional_arity` tests; reuse
them, only add `arg_keyword` if genuinely missing):

```rust
fn arg_keyword(kind: ArgumentKind, name: &str) -> crate::manifest::Argument {
    let mut a = arg(kind, name, None);
    a.long_flag = Some(format!("--{name}"));
    a
}
```

- [ ] **Step 2: Run to confirm failure**

```sh
cargo test -p toolr-core --lib parser::build::tests -- positional
```

Expect compile failure first (new arms not yet added to the `match`), then
assertion failures once it compiles against Task 1's variants alone.

- [ ] **Step 3: Update `validate_positional_arity`**

Replace the `match arg.kind { ... }` block:

```rust
            match arg.kind {
                ArgumentKind::VarPositional => {
                    var_positional = Some(arg.name.as_str());
                }
                ArgumentKind::Positional | ArgumentKind::OptionalPositional => {
                    let is_optional = matches!(arg.kind, ArgumentKind::OptionalPositional)
                        || matches!(arg.resolved_type, Some(SupportedType::Optional(_)));
                    if is_optional {
                        if let Some(first) = zero_or_one {
                            errors.push(PositionalArityError {
                                module: cmd.module.clone(),
                                command: cmd.name.clone(),
                                kind: PositionalArityErrorKind::MultipleZeroOrOne {
                                    first: first.to_string(),
                                    second: arg.name.clone(),
                                },
                            });
                        } else {
                            zero_or_one = Some(arg.name.as_str());
                        }
                    } else if let Some(zo) = zero_or_one {
                        errors.push(PositionalArityError {
                            module: cmd.module.clone(),
                            command: cmd.name.clone(),
                            kind: PositionalArityErrorKind::RequiredAfterZeroOrOne {
                                required: arg.name.clone(),
                                zero_or_one: zo.to_string(),
                            },
                        });
                    }
                }
                // Positional-style FixedArity (no long_flag) always
                // consumes a fixed slot — same ordering rule as a plain
                // required Positional. Keyword-style FixedArity doesn't
                // compete for positional slots at all.
                ArgumentKind::FixedArity if arg.long_flag.is_none() => {
                    if let Some(zo) = zero_or_one {
                        errors.push(PositionalArityError {
                            module: cmd.module.clone(),
                            command: cmd.name.clone(),
                            kind: PositionalArityErrorKind::RequiredAfterZeroOrOne {
                                required: arg.name.clone(),
                                zero_or_one: zo.to_string(),
                            },
                        });
                    }
                }
                // Keyword-like kinds don't compete for positional slots.
                ArgumentKind::Optional
                | ArgumentKind::Flag
                | ArgumentKind::Repeated
                | ArgumentKind::Count
                | ArgumentKind::FixedArity => {}
            }
```

- [ ] **Step 4: Run to confirm pass, then run the whole crate**

```sh
cargo test -p toolr-core --lib parser::build
cargo check -p toolr-core
```

Confirm `toolr-core` now compiles cleanly end to end (no more errors from
Task 1's variant additions inside this crate).

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/parser/build.rs
git commit -m "fix(parser): OptionalPositional and positional FixedArity join zero-or-one arity rules

Part of #423."
```

---

### Task 4: clap `Arg` construction for the new kinds (`crates/toolr/src/cli.rs`)

**Files:**

- Modify: `crates/toolr/src/cli.rs`
- Test: same file, `cli_tree_tests` module at the bottom

**Interfaces:**

- Consumes: `ArgumentKind::FixedArity`, `ArgumentKind::OptionalPositional`,
  `ArgMetadata.nargs: Option<Nargs>` (Tasks 1–2).

- [ ] **Step 1: Write the failing tests**

Add near the existing `ArgumentKind::Repeated`/`VarPositional` tests in
`cli_tree_tests`:

```rust
#[test]
fn fixed_arity_keyword_gets_exact_num_args() {
    let mut arg = empty_arg("pair", ArgumentKind::FixedArity);
    arg.long_flag = Some("--pair".to_string());
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(2));
    let cmd = cmd_with(vec![arg]);
    let built = build_command(&cmd);
    let a = built.get_arguments().find(|a| a.get_id() == "pair").unwrap();
    assert_eq!(a.get_num_args().unwrap().min_values(), 2);
    assert_eq!(a.get_num_args().unwrap().max_values(), 2);
    assert!(a.get_long().is_some());
    assert!(!a.is_required_set());
}

#[test]
fn fixed_arity_positional_gets_exact_num_args_and_is_required() {
    let mut arg = empty_arg("files", ArgumentKind::FixedArity);
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(3));
    let cmd = cmd_with(vec![arg]);
    let built = build_command(&cmd);
    let a = built.get_arguments().find(|a| a.get_id() == "files").unwrap();
    assert_eq!(a.get_num_args().unwrap().min_values(), 3);
    assert!(a.get_long().is_none());
    assert!(a.is_required_set());
}

#[test]
fn optional_positional_is_not_required() {
    let arg = empty_arg("maybe", ArgumentKind::OptionalPositional);
    let cmd = cmd_with(vec![arg]);
    let built = build_command(&cmd);
    let a = built.get_arguments().find(|a| a.get_id() == "maybe").unwrap();
    assert!(!a.is_required_set());
    assert!(a.get_long().is_none());
}

#[test]
fn repeated_nargs_star_widens_num_args_via_metadata() {
    let mut arg = empty_arg("names", ArgumentKind::Repeated);
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Star);
    let cmd = cmd_with(vec![arg]);
    let built = build_command(&cmd);
    let a = built.get_arguments().find(|a| a.get_id() == "names").unwrap();
    assert_eq!(a.get_num_args().unwrap().min_values(), 1);
    assert!(a.get_num_args().unwrap().max_values() > 1);
}

#[test]
fn var_positional_nargs_plus_requires_at_least_one() {
    let mut arg = empty_arg("labels", ArgumentKind::VarPositional);
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Plus);
    let cmd = cmd_with(vec![arg]);
    let built = build_command(&cmd);
    let a = built.get_arguments().find(|a| a.get_id() == "labels").unwrap();
    assert!(a.is_required_set());
}
```

(`cmd_with`, `empty_arg`, `build_command` already exist in this test module —
reuse them. Note the existing `empty_arg` sets `long_flag: None`; tests above
that need keyword-style set it explicitly, matching how the scanner would.)

- [ ] **Step 2: Run to confirm failure**

```sh
cargo test -p toolr --lib cli_tree_tests
```

Compile failure first: the `match arg.kind { ... }` in `build_command` is
non-exhaustive once `FixedArity`/`OptionalPositional` exist.

- [ ] **Step 3: Update the `match arg.kind` block**

Replace the `ArgumentKind::Repeated` and `ArgumentKind::VarPositional` arms,
and add the two new ones (this also drops the old
`arg.metadata.multi_value_occurrence`/`require_at_least_one` reads in favor
of `arg.metadata.nargs`):

```rust
            ArgumentKind::Repeated => {
                // --name VALUE that may repeat; each occurrence appends.
                // `nargs == Some(Plus | Star)` (argparse `nargs="+"`/`"*"`)
                // additionally lets one occurrence take several
                // space-separated values — widening `num_args` beyond 1
                // is unsafe by default since a following positional
                // would get swallowed, so it's opt-in per argument
                // rather than blanket for the kind.
                a = a.long(long_flag).required(false).action(ArgAction::Append);
                a = match arg.metadata.nargs {
                    Some(toolr_core::manifest::Nargs::Plus)
                    | Some(toolr_core::manifest::Nargs::Star) => a.num_args(1..),
                    _ => a.num_args(1),
                };
            }
            ArgumentKind::VarPositional => {
                // Trailing variadic positional. Zero values is valid by
                // default (native `*args`, argparse `nargs="*"` or
                // `argparse.REMAINDER`); `nargs == Some(Plus)` (argparse
                // `nargs="+"`) demands one or more instead.
                a = if arg.metadata.nargs == Some(toolr_core::manifest::Nargs::Plus) {
                    a.required(true).num_args(1..)
                } else {
                    a.required(false).num_args(0..)
                };
                a = a.trailing_var_arg(true);
            }
            ArgumentKind::Count => {
                // `-v`, `-vv`, `-vvv` → 1 / 2 / 3 via clap's
                // ArgAction::Count. Python receives the resulting int
                // through `toolr.types.Count` (which is `int`).
                a = a.long(long_flag).action(ArgAction::Count);
            }
            ArgumentKind::FixedArity => {
                // Exactly N values in a single occurrence (argparse
                // `nargs=N`). Keyword-style (`--pair a b`) when the
                // scanner recorded a literal flag spelling; positional
                // (`files a b`, no flag, always required) otherwise —
                // mirrors the Positional/Optional split used everywhere
                // else in this function.
                let arity = match arg.metadata.nargs {
                    Some(toolr_core::manifest::Nargs::Fixed(n)) => n,
                    _ => unreachable!("FixedArity always carries Nargs::Fixed(n)"),
                };
                a = if arg.long_flag.is_some() {
                    a.long(long_flag).required(false)
                } else {
                    a.required(true)
                };
                a = a.num_args(arity);
            }
            ArgumentKind::OptionalPositional => {
                // Zero-or-one positional value (argparse positional
                // `nargs="?"`). No flag, not required — unlike
                // `Positional`, which this function always marks
                // required unless `resolved_type` says `T | None` (which
                // argparse-scanned args never carry).
                a = a.required(false);
            }
```

Note: `Count` didn't move — it's shown above only for placement context; the
new arms (`FixedArity`, `OptionalPositional`) are appended after it in the
same `match`.

- [ ] **Step 4: Run to confirm pass**

```sh
cargo test -p toolr --lib cli_tree_tests
cargo check -p toolr
```

Confirm remaining compile errors are scoped to `execute_build.rs` and
`crates/toolr-core/src/complete/engine.rs` /
`crates/toolr/src/builtin_completions.rs` (the latter two only if they use
exhaustive matches — verify with the compiler output; Task 5 covers whichever
of these actually error).

- [ ] **Step 5: Commit**

```bash
git add crates/toolr/src/cli.rs
git commit -m "feat(cli): build clap Arg for FixedArity and OptionalPositional

Consolidates the Repeated/VarPositional num_args decisions onto the new
ArgMetadata.nargs field.

Part of #423."
```

---

### Task 5: Completion engine — positional-slot and flag-detection filters

**Files:**

- Modify: `crates/toolr-core/src/complete/engine.rs`
- Test: same file, existing completion test module (search `mod tests`)

**Interfaces:**

- Consumes: `ArgumentKind::FixedArity`, `ArgumentKind::OptionalPositional`.

`engine.rs` uses `matches!` (not exhaustive `match`), so the compiler won't
force these — they're behavioral gaps that need manual attention. Two
filters need the new kinds added so completion still finds positional slots
and still offers flag-name completion when appropriate:

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn optional_positional_is_a_completable_positional_slot() {
    let cmd = cmd_with_args(vec![Argument {
        kind: ArgumentKind::OptionalPositional,
        ..test_arg("maybe")
    }]);
    let slot = classify_leaf_args(&cmd, &[], String::new());
    assert!(matches!(slot, Slot::Positional { argument, .. } if argument.name == "maybe"));
}

#[test]
fn positional_fixed_arity_is_a_completable_positional_slot() {
    let cmd = cmd_with_args(vec![Argument {
        kind: ArgumentKind::FixedArity,
        long_flag: None,
        ..test_arg("pair")
    }]);
    let slot = classify_leaf_args(&cmd, &[], String::new());
    assert!(matches!(slot, Slot::Positional { argument, .. } if argument.name == "pair"));
}

#[test]
fn keyword_fixed_arity_offers_flag_completion() {
    let cmd = cmd_with_args(vec![Argument {
        kind: ArgumentKind::FixedArity,
        long_flag: Some("--pair".to_string()),
        ..test_arg("pair")
    }]);
    let slot = classify_leaf_args(&cmd, &[], String::new());
    assert!(matches!(slot, Slot::Flag { .. }));
}
```

(Adapt to whatever test-fixture helpers — `cmd_with_args`, `test_arg` — this
file's existing completion tests already use; check the `mod tests` block for
the actual names before writing these verbatim.)

- [ ] **Step 2: Run to confirm failure**

```sh
cargo test -p toolr-core --lib complete::engine
```

- [ ] **Step 3: Update the two positional-slot filters in `classify_leaf_args`**

```rust
    let positional_args: Vec<&Argument> = command
        .arguments
        .iter()
        .filter(|a| {
            matches!(
                a.kind,
                ArgumentKind::Positional
                    | ArgumentKind::VarPositional
                    | ArgumentKind::OptionalPositional
            ) || (a.kind == ArgumentKind::FixedArity && a.long_flag.is_none())
        })
        .collect();
```

```rust
    let has_flags = command.arguments.iter().any(|a| {
        !(matches!(
            a.kind,
            ArgumentKind::Positional | ArgumentKind::VarPositional | ArgumentKind::OptionalPositional
        ) || (a.kind == ArgumentKind::FixedArity && a.long_flag.is_none()))
    });
```

Leave the earlier `ArgumentKind::VarPositional`-only checks (lines ~208 and
~410 per the design doc's file list) alone — those specifically mean "is this
the *trailing greedy* positional", which only `VarPositional` is; `FixedArity`
and `OptionalPositional` consume exactly one fixed slot each, same as
`Positional`, so they're correctly excluded from that narrower check.

- [ ] **Step 4: Run to confirm pass**

```sh
cargo test -p toolr-core --lib complete::engine
cargo check -p toolr-core
cargo check -p toolr
```

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/complete/engine.rs
git commit -m "fix(complete): recognise FixedArity and OptionalPositional as positional slots

Part of #423."
```

---

### Task 6: Runner schema version bump (Rust + Python, lockstep)

**Files:**

- Modify: `crates/toolr-core/src/execute/spec.rs`
- Modify: `crates/toolr-py/python/toolr/_runner.py`
- Test: `crates/toolr-core/src/execute/spec.rs` (`schema_version_constant_is_2`
  test — rename/update), `crates/toolr-core/tests/schema_version_lockstep.rs`
  (should already assert equality generically; just confirm it still passes)
- [ ] **Step 1: Bump the Rust constant and its test**

```rust
pub const RUNNER_SCHEMA_VERSION: u32 = 3;
```

```rust
    #[test]
    fn schema_version_constant_is_3() {
        assert_eq!(RUNNER_SCHEMA_VERSION, 3);
    }
```

- [ ] **Step 2: Bump the Python constant**

```python
SCHEMA_VERSION: int = 3
```

- [ ] **Step 3: Run the lockstep test**

```sh
cargo test -p toolr-core --test schema_version_lockstep
```

- [ ] **Step 4: Commit**

```bash
git add crates/toolr-core/src/execute/spec.rs crates/toolr-py/python/toolr/_runner.py
git commit -m "feat(runner): bump dispatch schema to 3 for nargs support

Fixed-arity args change command_args[name] from a scalar to an array
under the existing optional/positional wire kinds, and nargs goes from
always-None to populated with reconstruction-relevant values — an old
runner can't decode either correctly.

Part of #423."
```

---

### Task 7: Wire mapping — `execute_build.rs` (`kind`/`nargs`/value extraction)

**Files:**

- Modify: `crates/toolr/src/execute_build.rs`
- Test: same file, `#[cfg(test)] mod tests`

**Interfaces:**

- Consumes: `ArgumentKind::FixedArity`, `ArgumentKind::OptionalPositional`,
  `ArgMetadata.nargs` (Tasks 1–2), bumped `RUNNER_SCHEMA_VERSION` (Task 6).
- Produces: `argument_to_arg_schema` now emits real `nargs` values;
  `extract_value` handles the two new kinds.
- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn argument_to_arg_schema_maps_fixed_arity_keyword_to_optional_with_int_nargs() {
    let mut arg = arg_of("pair", ArgumentKind::FixedArity, SupportedType::Str);
    arg.long_flag = Some("--pair".to_string());
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(2));
    let schema = argument_to_arg_schema(&arg);
    assert_eq!(schema.kind, "optional");
    assert_eq!(schema.nargs, Some(serde_json::json!(2)));
}

#[test]
fn argument_to_arg_schema_maps_fixed_arity_positional_to_positional_with_int_nargs() {
    let mut arg = arg_of("files", ArgumentKind::FixedArity, SupportedType::Str);
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Fixed(3));
    let schema = argument_to_arg_schema(&arg);
    assert_eq!(schema.kind, "positional");
    assert_eq!(schema.nargs, Some(serde_json::json!(3)));
}

#[test]
fn argument_to_arg_schema_maps_optional_positional_to_positional_with_question_nargs() {
    let arg = arg_of("maybe", ArgumentKind::OptionalPositional, SupportedType::Str);
    let schema = argument_to_arg_schema(&arg);
    assert_eq!(schema.kind, "positional");
    assert_eq!(schema.nargs, Some(serde_json::json!("?")));
}

#[test]
fn argument_to_arg_schema_maps_repeated_nargs_star_to_repeated_with_star() {
    let mut arg = arg_of("tags", ArgumentKind::Repeated, SupportedType::Str);
    arg.metadata.nargs = Some(toolr_core::manifest::Nargs::Star);
    let schema = argument_to_arg_schema(&arg);
    assert_eq!(schema.kind, "repeated");
    assert_eq!(schema.nargs, Some(serde_json::json!("*")));
}

#[test]
fn build_spec_extracts_fixed_arity_arg_as_json_array() {
    let cmd = cmd_with(vec![arg_of("pair", ArgumentKind::FixedArity, SupportedType::Str)]);
    let matches = parse(&["cmd", "--pair", "a", "b"]); // adapt to this file's existing `parse` helper/fixture pattern
    let spec = build_spec(&cmd, &matches, Path::new("/repo"), &OutputOptions::default());
    assert_eq!(
        spec.args.get("pair"),
        Some(&Value::Array(vec![Value::String("a".into()), Value::String("b".into())]))
    );
}
```

(The last test needs a command/clap-matches fixture that actually applies
`num_args(2)` — reuse this file's existing `cmd_with`/`parse` pattern used by
`build_spec_extracts_tuple_arg_with_num_args`, which already exercises the
same `num_args` + multi-value-extraction shape for tuples; adapt rather than
inventing new plumbing.)

- [ ] **Step 2: Run to confirm failure**

```sh
cargo test -p toolr --lib execute_build
```

Compile failure first (non-exhaustive `match arg.kind` in
`argument_to_arg_schema` and `extract_value`).

- [ ] **Step 3: Update `argument_to_arg_schema`**

```rust
fn argument_to_arg_schema(arg: &Argument) -> ArgSchemaSpec {
    let kind = match arg.kind {
        ArgumentKind::Positional | ArgumentKind::OptionalPositional => "positional",
        ArgumentKind::Optional => "optional",
        ArgumentKind::Flag | ArgumentKind::Count => "flag",
        ArgumentKind::Repeated | ArgumentKind::VarPositional => "repeated",
        ArgumentKind::FixedArity => {
            if arg.long_flag.is_some() {
                "optional"
            } else {
                "positional"
            }
        }
    };
    let nargs = match (arg.kind, arg.metadata.nargs) {
        (ArgumentKind::OptionalPositional, _) => Some(serde_json::json!("?")),
        (_, Some(toolr_core::manifest::Nargs::Fixed(n))) => Some(serde_json::json!(n)),
        (_, Some(toolr_core::manifest::Nargs::Plus)) => Some(serde_json::json!("+")),
        (_, Some(toolr_core::manifest::Nargs::Star)) => Some(serde_json::json!("*")),
        (_, Some(toolr_core::manifest::Nargs::Question)) => Some(serde_json::json!("?")),
        (_, None) => None,
    };
    ArgSchemaSpec {
        name: arg.name.clone(),
        kind: kind.to_string(),
        help: arg.help.clone(),
        default: arg.default.clone(),
        choices: if arg.allowed_values.is_empty() {
            None
        } else {
            Some(arg.allowed_values.clone())
        },
        metavar: arg.metadata.metavar.clone(),
        type_annotation: arg.type_annotation.clone(),
        nargs,
        long_flag: arg.long_flag.clone(),
    }
}
```

Note `multi_value_occurrence: arg.metadata.multi_value_occurrence` is gone
from the struct literal — that field no longer exists on `ArgSchemaSpec`
(removed in the next step).

- [ ] **Step 4: Remove `multi_value_occurrence` from `ArgSchemaSpec`**

In `crates/toolr-core/src/execute/spec.rs`, delete:

```rust
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multi_value_occurrence: bool,
```

and tighten the `nargs` field's doc comment (it's no longer "always `None`"):

```rust
    /// `"*" | "+" | "?"` or an integer; serde encodes whichever variant.
    /// `Repeated`/`VarPositional`: `"+"`/`"*"` when a single occurrence
    /// takes several values (argparse `nargs="+"`/`"*"`/`REMAINDER`),
    /// `None` when each occurrence takes one value (`action="append"`,
    /// or native `*args`). `Optional`/`Positional` (from `FixedArity`):
    /// an int, the exact value count. `Positional` (from
    /// `OptionalPositional`): `"?"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nargs: Option<serde_json::Value>,
```

- [ ] **Step 5: Update `extract_value`**

```rust
    match arg.kind {
        ArgumentKind::Flag => {
            let v = matches.get_flag(arg.name.as_str());
            Some(Value::Bool(v))
        }
        ArgumentKind::Count => {
            let n = matches.get_count(arg.name.as_str());
            Some(Value::Number(u64::from(n).into()))
        }
        ArgumentKind::Positional | ArgumentKind::Optional | ArgumentKind::OptionalPositional => {
            extract_scalar(arg, matches)
        }
        ArgumentKind::Repeated | ArgumentKind::VarPositional | ArgumentKind::FixedArity => {
            Some(Value::Array(extract_many(arg, matches)))
        }
    }
```

`OptionalPositional` reuses `extract_scalar` unchanged — clap's
`matches.get_one` already returns `None` when the optional positional wasn't
supplied, and the caller (`build_spec`) already skips `None` results, so an
absent `nargs="?"` value is simply omitted from `args`, exactly like an
unset `Optional`.

- [ ] **Step 6: Extend the relative-path multi-value check**

In `arg_has_relative_cli_path`:

```rust
    let is_many = matches!(
        arg.resolved_type.as_ref().map(unwrap_optional),
        Some(SupportedType::Tuple(_))
    ) || matches!(
        arg.kind,
        ArgumentKind::Repeated | ArgumentKind::VarPositional | ArgumentKind::FixedArity
    );
```

- [ ] **Step 7: Run to confirm pass**

```sh
cargo test -p toolr --lib execute_build
cargo check --workspace
```

This should be the point where `cargo check --workspace` is clean again.

- [ ] **Step 8: Commit**

```bash
git add crates/toolr/src/execute_build.rs crates/toolr-core/src/execute/spec.rs
git commit -m "feat(dispatch): populate nargs on the wire, drop multi_value_occurrence

ArgSchema.nargs now carries real values instead of always None.
FixedArity reuses the optional/positional wire kinds (command_args
becomes a JSON array); OptionalPositional needs no argv changes — an
absent value is already omitted.

Part of #423."
```

---

### Task 8: Python — `ArgSchema` drops `multi_value_occurrence`; `argv` reads `nargs`; positional-repeated flag bug fix

**Files:**

- Modify: `crates/toolr-py/python/toolr/sources/_types.py`
- Modify: `crates/toolr-py/python/toolr/sources/_dispatch.py`
- Test: `tests/sources/test_dispatch.py`

**Interfaces:**

- Consumes: `ArgSchema.nargs` populated per Task 7's mapping.

- [ ] **Step 1: Write the failing tests**

Add to `tests/sources/test_dispatch.py`'s `test_argv_reconstruction`
parametrize list:

```python
        # Fixed-arity keyword (nargs=2) — one flag, N values.
        (
            {"pair": ["a", "b"]},
            [ArgSchema(name="pair", kind="optional", help="", nargs=2)],
            ["--pair", "a", "b"],
        ),
        # Fixed-arity positional (nargs=3) — N bare values, no flag.
        (
            {"files": ["x", "y", "z"]},
            [ArgSchema(name="files", kind="positional", help="", nargs=3)],
            ["x", "y", "z"],
        ),
        # Positional repeated (VarPositional dispatch) — bare values,
        # no flag prepended. This was broken before: the old code always
        # called `_flag_for_arg`, even with no `long_flag` recorded.
        (
            {"labels": ["a", "b", "c"]},
            [ArgSchema(name="labels", kind="repeated", help="", nargs="*")],
            ["a", "b", "c"],
        ),
        # Keyword repeated with nargs="+" — same single-occurrence,
        # many-values form as before, now driven by `nargs` not the
        # (removed) `multi_value_occurrence` bool.
        (
            {"customer_ids": ["100087163", "100180680", "10033857"]},
            [ArgSchema(name="customer_ids", kind="repeated", help="", nargs="+")],
            ["--customer-ids", "100087163", "100180680", "10033857"],
        ),
```

Update the existing test that used `multi_value_occurrence=True` (the
`customer_ids` case already in the file) to use `nargs="+"` instead — that's
the case duplicated above; delete the old `multi_value_occurrence=True`
version so there's exactly one `customer_ids` case, using `nargs`.

Also add, in a new test function, a regression check for the specific bug:

```python
def test_argv_positional_repeated_omits_flag_even_with_multiple_elements():
    schema = CommandSchema(
        name="x",
        summary="",
        description="",
        arguments=[ArgSchema(name="args", kind="repeated", help="", nargs="*")],
    )
    dc = DispatchCommand(command="x", command_args={"args": ["a", "b"]}, schema=schema)
    assert dc.argv == ["a", "b"]
```

- [ ] **Step 2: Run to confirm failure**

```sh
uv run pytest tests/sources/test_dispatch.py -v
```

Expect: `ArgSchema` construction fails if `multi_value_occurrence` kwarg is
still required anywhere else in the file (it isn't used in these new cases,
so this should just fail on the `argv` assertions instead once `_types.py` no
longer has the field — check both directions).

- [ ] **Step 3: Remove `multi_value_occurrence` from `ArgSchema`**

In `crates/toolr-py/python/toolr/sources/_types.py`, delete:

```python
    multi_value_occurrence: bool = False
    """`kind == "repeated"` only: source is argparse `nargs="+"`/`"*"`,
    not `action="append"`.

    A single occurrence takes several space-separated values
    (`--name a b c`) rather than accumulating one value per occurrence
    (`--name a --name b --name c`). `DispatchCommand.argv` uses this to
    pick the correct invocation form — emitting the append form for a
    genuine `nargs="+"` target silently drops every value but the last.
    """
```

- [ ] **Step 4: Rewrite `DispatchCommand.argv`**

```python
    @property
    def argv(self) -> list[str]:
        """Argparse-shaped argv reconstructed from `command_args` per `schema`.

        For each argument in `schema.arguments` that appears in
        `command_args`, emit the appropriate token(s):

        - `positional`, no `nargs` → bare value.
        - `positional`, `nargs == "?"` → bare value when present (an
          absent zero-or-one value never appears in `command_args`).
        - `positional`, `nargs` an int (fixed arity) → N bare values.
        - `flag` → `--name` when truthy, omitted when falsy.
        - `optional`, no `nargs` → `--name value`, omitted when
          value == default.
        - `optional`, `nargs` an int (fixed arity) → `--name` once,
          followed by all N values.
        - `repeated`, `nargs in ("+", "*")` → `--name value1 value2 ...`
          in one occurrence (argparse `nargs="+"`/`"*"`/`REMAINDER` on a
          keyword-style arg).
        - `repeated`, `nargs is None` (`action="append"`) →
          `--name value` once per element.
        - `repeated` with no `long_flag` (a genuinely positional
          `VarPositional`/`REMAINDER` source) → bare values, no flag at
          all, regardless of `nargs`.

        Keys in `command_args` not found in `schema.arguments` raise
        ValueError so typos surface loudly.
        """
        known = {a.name for a in self.schema.arguments}
        for key in self.command_args:
            if key not in known:
                msg = f"DispatchCommand.argv: unknown argument {key!r} (not in schema)"
                raise ValueError(msg)

        out: list[str] = []
        for arg in self.schema.arguments:
            if arg.name not in self.command_args:
                continue
            value = self.command_args[arg.name]
            if arg.kind == "positional":
                if isinstance(arg.nargs, int):
                    out.extend(str(element) for element in value)
                else:
                    out.append(str(value))
            elif arg.kind == "flag":
                if value:
                    out.append(_flag_for_arg(arg))
            elif arg.kind == "optional":
                if isinstance(arg.nargs, int):
                    out.append(_flag_for_arg(arg))
                    out.extend(str(element) for element in value)
                elif arg.default is None or str(value) != arg.default:
                    out.extend([_flag_for_arg(arg), str(value)])
            elif arg.kind == "repeated":
                if arg.long_flag is None:
                    out.extend(str(element) for element in value)
                elif arg.nargs in ("+", "*"):
                    out.append(_flag_for_arg(arg))
                    out.extend(str(element) for element in value)
                else:
                    for element in value:
                        out.extend([_flag_for_arg(arg), str(element)])
        return out
```

Note the `elif arg.kind == "repeated": if arg.long_flag is None: ...` branch
fixes the bug described in the design doc: a genuinely positional
`VarPositional`/`REMAINDER` source (no recorded `long_flag`) never gets a
synthetic `--name` prepended, matching the `"positional"` branch's bare-value
style.

- [ ] **Step 5: Run to confirm pass**

```sh
uv run pytest tests/sources/test_dispatch.py -v
```

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-py/python/toolr/sources/_types.py crates/toolr-py/python/toolr/sources/_dispatch.py tests/sources/test_dispatch.py
git commit -m "fix(dispatch): reconstruct argv from nargs; drop multi_value_occurrence

Also fixes DispatchCommand.argv always prepending a synthetic flag for
a genuinely positional 'repeated' (VarPositional) source — untested
until REMAINDER support started exercising a real positional-repeated
dispatch path.

Part of #423."
```

---

### Task 9: Regenerate skill refs, remove the stale #423 warning cross-link, release note

**Files:**

- Modify: `UNRELEASED.md`
- Regenerate: whatever `skills/*/references/*.md` files `cargo xtask
  build-skill-refs` touches (new `ArgumentKind` variants are public surface)
- Modify: `crates/toolr-core/src/argparse/scan.rs` (doc comment above the
  warning block — the one added in commit `53697587`, "link the
  unsupported-nargs warning to #423" — needs to describe the *remaining*
  unsupported cases only, not all of #423's original scope)
- [ ] **Step 1: Update the doc comment above the warning block in `scan.rs`**

The comment currently describes int nargs / REMAINDER / positional `"?"` as
all unsupported and links to #423 as the tracking issue for all of them.
Since this plan closes out all three, rewrite it to describe only what's
still genuinely unsupported:

```rust
    // `nargs=N` combined with `action="append"` (repeating groups of N
    // values) isn't representable by any `ArgumentKind` — degrades to a
    // single value silently. `argparse.REMAINDER` on a keyword-style arg
    // is nonsensical in argparse itself and stays unsupported too. Warn
    // so these are visible rather than silently wrong at runtime.
```

- [ ] **Step 2: `cargo xtask build-skill-refs --check`, then regenerate**

```sh
cargo xtask build-skill-refs --check || cargo xtask build-skill-refs
git status --short
```

Stage whatever it regenerated.

- [ ] **Step 3: Add the release note**

Append to `UNRELEASED.md`:

```markdown
- The argparse scanner now gives `nargs=N` (fixed-arity), positional
  `nargs="?"`, and `argparse.REMAINDER` first-class support end to end —
  clap arg construction, tab completion, and dispatch argv reconstruction
  all handle these correctly instead of silently degrading to a single
  value. Also fixes `DispatchCommand.argv` prepending a spurious flag for
  a genuinely positional repeated (`nargs="+"`/`"*"`) source.
```

- [ ] **Step 4: Commit**

```bash
git add UNRELEASED.md crates/toolr-core/src/argparse/scan.rs
# plus whatever build-skill-refs regenerated
git commit -m "docs(argparse): narrow the unsupported-nargs warning; release note

Part of #423, closes #423."
```

---

### Task 10: Full verification pass

**Files:** none (verification only)

- [ ] **Step 1: Run the full umbrella test suite**

```sh
mise run test
```

This runs the skill-refs drift gate, `cargo test --workspace`, and `pytest`.
Per CLAUDE.md: poll output every 30–60s rather than fire-and-forget — this
run can stall.

- [ ] **Step 2: Run pre-commit on everything**

```sh
prek run --all-files
```

- [ ] **Step 3: Manually dogfood one of each new form**

```sh
cargo run -p toolr -- self build-manifest toolr_example_plugin
```

(Or against a scratch `tools/` dir with an argparse-wrapped command declaring
`nargs=2`, `nargs=argparse.REMAINDER`, and a positional `nargs="?"` — confirm
`toolr <cmd> --help` shows sane arity and `toolr <cmd> --pair a b` /
equivalent actually round-trips through dispatch without error.)

- [ ] **Step 4: Push and open the PR**

Confirm with the user before pushing (this repo's git-spice branch is
already stacked and tracked — `git-spice branch submit --draft` per
CLAUDE.md's stacked-PR convention). Do not push without explicit
confirmation.
