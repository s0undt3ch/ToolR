# Completion after `--` and dash-leading choices (FU-31, FU-32)

Two engine bugs in `toolr __complete`, both on `main`, fixed together because both change what
`classify_leaf_args` hands to `values()`.

## Reproduction (dev binary on `main`, scratch project with `ci lint` and `ci pick`)

`ci lint` has `target: Literal["alpha","beta"]`, `--mode: Literal["fast","full"]` and `--verbose`.
`ci pick` has `--opt: Literal["--dash","-x","plain"]`. Invocation:
`toolr __complete "$PWD" <tokens…>`; stdout is `value<TAB>description` per line.

| Tokens | Got | Wanted |
| --- | --- | --- |
| `ci lint -- --mode=` | `fast`, `full` | nothing |
| `ci lint -- --` | `--help`, `--mode`, `--verbose` | nothing |
| `ci lint -- --mode alpha ""` | `alpha`, `beta` | nothing (`--mode` and `alpha` are both positionals) |
| `ci lint -- alpha ""` | `--mode`, `--verbose` | nothing (one positional, already filled) |
| `ci lint alpha -- ""` | `--mode`, `--verbose` | nothing |
| `ci pick --opt ""` | `--dash`, `-x`, `plain` | `plain` |
| `ci pick --opt=-` | `--dash`, `-x` | unchanged |

`ci lint -- ""` already prints `alpha`, `beta`, but only because `positional_indices` skips the bare
`--` as an unknown `--flag`; it is accidental.

## What clap accepts (verified with `TOOLR_TRUST_ALL=1`, past-clap errors are "Python interpreter not found")

- `ci lint -- --mode=fast`: `invalid value '--mode=fast' for '<target>'`. Everything after `--` is a positional.
- `ci lint alpha -- extra`: `unexpected argument 'extra' found`. The `--` does not consume a positional slot.
- `ci pick --opt --dash` and `ci pick --opt -x`: `unexpected argument … found`. Rejected.
- `ci pick --opt=--dash` and `ci pick --opt=-x`: accepted.
- User arguments never get `allow_hyphen_values` (`cli.rs` sets it only on the dispatcher trailing args and
  `__complete`), so there is no per-arg escape hatch to mirror.
- A dash-leading positional choice is accepted only after `--`.

## Design

Changes are in `crates/toolr-core/src/complete/engine.rs`, plus `raw_complete_tokens` in
`crates/toolr/src/dispatch.rs` (round-1 fix below). No script change: the scripts already pass
a committed `--` through, and the attached-form `--opt=` prefixing from #685 is untouched.

### FU-31: a committed `--` ends option parsing

```rust
/// What the committed argument tokens of one command hold.
struct ArgScan {
    /// Indices of the positional values, never the `--` itself.
    positionals: Vec<usize>,
    /// Index of the committed `--`, when there is one.
    terminator: Option<usize>,
    /// A value-taking flag (`--name` or an empty `--name=`) is directly followed by `--`.
    missing_value: bool,
}

impl ArgScan {
    /// Whether clap rejects the line outright, so nothing completes: a flag missing its
    /// value before `--` and, for a dispatcher, a `--` before the child name.
    fn dead_end(&self, dispatcher: bool) -> bool;
}

fn scan_args(command: &Command, arg_tokens: &[String]) -> ArgScan;

/// Flag-vs-positional decision for a word clap still parses as an option.
fn flag_slot<'a>(command: &'a Command, arg_tokens: &[String], prefix: &str) -> Option<Slot<'a>>;
```

`scan_args` is the old `positional_indices` loop plus two arms: a token equal to `--` records its index and
pushes every later index as a positional, then stops; a value-taking flag (bare, or `--name=` with nothing
attached) followed by a committed `--` sets `missing_value`. It replaces `positional_indices`; the dispatcher
uses it too.

`classify_leaf_args` calls `scan_args` once, up front. When `terminator.is_some()`:

- skip `flag_slot` (`attached_value`, `pending_flag_value` and the `--`/`-` → `Slot::Flag` branch), so `--mode=x`,
  `--mode <TAB>` and `--` are positional words;
- pick the positional from `positionals.len()` exactly as now (variadic keeps completing);
- skip the final "command has flags, offer flag names" fallback; past the last positional the slot is
  `Slot::None`.

`complete()` needs no change: a `Slot::Positional` that is path-typed still returns `Paths`/`Directories`.

A `--` that is the in-progress word (`ci lint --<TAB>`) is not committed, so it still lists flags.

`classify_dispatcher`: if `terminator` falls before the child name (or there is no child yet), return
`Slot::None`; clap rejects that line (verified, see open questions). A `--` after the child
name reaches the child through the existing `&arg_tokens[child_index + 1..]` slice, where
`classify_leaf_args` handles it.

### FU-32: drop dash-leading choices where clap would read a flag

Choice: **drop in the separate form, keep in the attached form**. Rewriting the reply to `--opt=--dash`
would need a script change (the scripts prefix `--opt=` onto bare values, so the rewrite would double it),
and `--opt=-<TAB>` already gives the user a working path. No new candidate text, so the wire format is unchanged.

```rust
enum Slot<'a> {
    // …
    FlagValue { argument: &'a Argument, prefix: String, attached: bool },
    Positional { argument: &'a Argument, prefix: String, after_terminator: bool },
}

fn values(argument: &Argument, prefix: &str, dash_ok: bool) -> Vec<Candidate>;
```

`values` adds `.filter(|v| dash_ok || *v == "-" || !v.starts_with('-'))`, since a bare `-` is a plain value to
clap. `candidates` binds `attached` (for a `FlagValue`) and `after_terminator` (for a `Positional`) as `dash_ok`.
`attached_value` sets `attached: true`;
`pending_flag_value`'s callers set `false`; the two `Slot::Positional` constructors set
`terminator.is_some()`.

Consequences: `--opt <TAB>` and `--opt -<TAB>` drop `--dash`/`-x`; `--opt=<TAB>` and `--opt=-<TAB>` offer
them; a positional `--dir` choice shows only after `--`. Dispatcher flag values go through the same
`pending_flag_value` call sites, so they are covered without extra code.

### Review fixes

- Root `--`: `raw_complete_tokens` no longer strips a leading `--`. No script sends a separator, so
  `toolr -- <TAB>` offered groups that clap rejects (`toolr -- ci`). The engine now sees the committed `--` and
  offers nothing; a lone trailing `--` (`toolr --<TAB>`) is still the in-progress word and lists root flags.
- `--opt -- <TAB>`: `ArgScan` gains `missing_value`, set when a value-taking flag is directly followed by `--`.
  Clap errors there (`a value is required`), so leaf and dispatcher classification return `Slot::None`.
- A bare `-` is a plain value to clap, so `values()` always offers it; only choices longer than one character
  that start with `-` (`--dash`, `-x`, `-1`) are dropped in the separate form.
- Numeric negative choices such as `-1` follow the same rule, with an explicit test.
- Round 2: an empty `--opt=` followed by `--` is the same missing value as a bare `--opt`; `scan_args` folds both
  into one rule, and `ArgScan::dead_end` is the single gate both classifiers call.

## Tests

Unit tests in `crates/toolr-core/src/complete/tests.rs` and one binary-level test in
`crates/toolr/tests/complete_smoke.rs`; no shell-script test, as no script changes. Details in the plan.

## Open questions

1. Resolved: clap rejects `jenkins job -- wrapup_company 1` (`unexpected argument 'wrapup_company'`, with a tip to
   remove the `--`), while `jenkins job wrapup_company -- 1` parses. The engine returns nothing for a `--`
   before the child name, as designed.
2. Resolved: `ci lint --mode -- <TAB>` completes nothing (`ArgScan::missing_value`); clap says "a value is
   required".
3. If the other branch's `bash.sh` rewrite starts dropping a committed `--` from the tokens it sends, FU-31
   silently stops working in bash. Worth one bash-level check after both land; not a design dependency.
