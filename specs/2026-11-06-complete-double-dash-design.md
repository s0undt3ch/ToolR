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

All changes are in `crates/toolr-core/src/complete/engine.rs`. No script change: the scripts already pass
a committed `--` through, and the attached-form `--opt=` prefixing from #685 is untouched.

### FU-31: a committed `--` ends option parsing

```rust
/// Result of walking the committed argument tokens of one command.
struct ArgScan {
    /// Indices into the walked tokens of every positional value; never the `--` itself.
    positionals: Vec<usize>,
    /// Index of the committed `--`, when there is one.
    terminator: Option<usize>,
}

fn scan_args(command: &Command, arg_tokens: &[String]) -> ArgScan;
```

`scan_args` is today's `positional_indices` loop plus one arm: a token equal to `--` records its index and
pushes every later index as a positional, then stops. It replaces `positional_indices`; the dispatcher uses it too.

`classify_leaf_args` calls `scan_args` once, up front. When `terminator.is_some()`:

- skip `attached_value`, `pending_flag_value` and the `--`/`-` → `Slot::Flag` branch, so `--mode=x`,
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

`values` adds `.filter(|v| dash_ok || !v.starts_with('-'))`. `candidates` passes `attached` for a
`FlagValue` and `after_terminator` for a `Positional`. `attached_value` sets `attached: true`;
`pending_flag_value`'s callers set `false`; the two `Slot::Positional` constructors set
`terminator.is_some()`.

Consequences: `--opt <TAB>` and `--opt -<TAB>` drop `--dash`/`-x`; `--opt=<TAB>` and `--opt=-<TAB>` offer
them; a positional `--dir` choice shows only after `--`. Dispatcher flag values go through the same
`pending_flag_value` call sites, so they are covered without extra code.

## Tests

Unit tests in `crates/toolr-core/src/complete/tests.rs` and one binary-level test in
`crates/toolr/tests/complete_smoke.rs`; no shell-script test, as no script changes. Details in the plan.

## Open questions

1. Resolved: clap rejects `jenkins job -- wrapup_company 1` (`unexpected argument 'wrapup_company'`, with a tip to
   remove the `--`), while `jenkins job wrapup_company -- 1` parses. The engine returns nothing for a `--`
   before the child name, as designed.
2. `ci lint --mode -- <TAB>` (`--` as a flag's value) is left as today: `scan_args` lets the flag consume it,
   and clap rejects the line anyway.
3. If the other branch's `bash.sh` rewrite starts dropping a committed `--` from the tokens it sends, FU-31
   silently stops working in bash. Worth one bash-level check after both land; not a design dependency.
