# Boolean parameters are flags that default to `False`

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-02. A `bool = True` parameter's
  default was silently ignored.
- **Plan:** [`2026-10-30-bool-flags-plan.md`](./2026-10-30-bool-flags-plan.md)

## Problem

The static parser turns every keyword parameter annotated `bool` into a
`Flag` (`crates/toolr-core/src/parser/signatures.rs::classify_keyword_kind`).
The CLI builds every `Flag` as a clap `ArgAction::SetTrue`
(`crates/toolr/src/cli.rs`), and the spec builder always reads it with
`matches.get_flag(...)` (`crates/toolr/src/execute_build.rs`).

The result:

- `verbose: bool = True` reaches Python as `False` unless `--verbose` is
  passed. The `True` default can never be observed, and nothing can turn
  the option off.
- A keyword-only `bool` with no default (`*, verbose: bool`) is recorded as
  "required" in the manifest but silently resolves to `False`.
- `bool | None`, a positional `bool`, `*args: bool` and `list[bool]` are
  value-taking arguments that need `--x true` on the command line. Nobody
  types that willingly, and no command in this repo uses any of them.
- A module-level alias such as `Verbose = Annotated[bool, arg(env="V")]`
  is classified syntactically as `Optional`. It becomes a value-taking
  `--verbose true|false` instead of a flag, because the syntactic
  classifier can't see through the alias.

## Decision

A `bool` parameter on a native command is a flag, and it defaults to
`False`. Passing the flag sets it to `True`. Nothing else is accepted.

| Signature | Result |
|---|---|
| `verbose: bool = False` | `--verbose` flag (unchanged) |
| `*, verbose: bool = False` | `--verbose` flag (unchanged) |
| `Annotated[bool, arg(...)] = False` | `--verbose` flag (unchanged) |
| alias of `bool` / `Annotated[bool, ...]`, `= False` | `--verbose` flag (**fixed**: was value-taking) |
| `colour: bool = True` | build error |
| `*, verbose: bool` (no default) | build error |
| `flag: bool` (positional, no default) | build error |
| `*flags: bool` | build error |
| `colour: bool \| None = None`, `Optional[bool]` | build error |
| `list[bool]`, `tuple[bool, ...]`, `list[bool] \| None` | build error |

### Why reject, not generate `--x/--no-x`

We considered emitting a `--colour/--no-colour` pair for `True` defaults.
Rejection won on maintainability:

- **One check in one place.** The rule lives in the parser. With a pair,
  six code paths would each need to know the negative spelling: the CLI
  builder, the spec builder, two completion sites, the dispatcher-flag
  guard, and the split of `env`/aliases across the two args.
- **argparse sources stay safe.** Scanned `store_false` arguments are also
  `Flag` with `default=True`. A pair rule keyed on the default would have
  given them a `--no-x` that the dispatcher then drops. They never pass
  through the native parser check, so rejection can't touch them.
- **Double negatives come free.** `no_cache: bool = True` is just another
  `True` default.
- **It's the reversible direction.** Relaxing an error later breaks no one.
  Shipping the pair and then rejecting would break every command and script
  that relies on it.

We also rejected the "pure toggle" (`--colour` turns colour *off* when the
default is `True`), because the flag's name would lie to the CLI user.

### Why reject `bool | None`

`colour: bool | None = None` produces `--colour true|false`. That's an
unpleasant CLI, and its real use case is a three-state value. A three-state
value is better expressed as `Literal["auto", "always", "never"]` or an
`Enum`, which is self-documenting in `--help` and completes on TAB.

## Error messages

Each error goes through the existing `BuildError::UnsupportedTypes` path,
so every violation in the tree is reported at once, prefixed with
`<module>::<function> argument `<name>` (annotated `<annotation>`):`.

`<name>` is the parameter name. `<neg>` is `no_<name>`, unless `<name>`
already starts with `no_`, in which case `<neg>` is `<name>` with the
prefix removed.

| Case | Message after the prefix |
|---|---|
| `bool = True` | ``boolean flags must default to `False`. Write `<neg>: bool = False` (CLI: `--<neg-hyphenated>`) and test `if <neg>:` in the body, or use `Literal[...]`/an `Enum` for more than two states.`` |
| keyword `bool`, no default | ``boolean flags must default to `False`. Write `<name>: bool = False`.`` |
| positional `bool` / `*args: bool` | ``a `bool` parameter is a flag; give it a default of `False` and pass it by keyword (`<name>: bool = False`).`` |
| `bool` inside `Optional`/`list`/`tuple` | ``a `bool` can only be a plain flag (`<name>: bool = False`). Use `Literal[...]` or an `Enum` for a value with more than two states.`` |

For `no_cache: bool = True`, `<neg>` is `cache`, so the message suggests
`cache: bool = False`.

## Scope

- **In scope:** native commands parsed from `tools/**/*.py`, and plugin
  packages built with `toolr self build-manifest` (same parser).
- **Out of scope:** argparse-scanned sources (`[tool.toolr.argparse.*]`).
  They keep their current `store_true`/`store_false` handling.
- **Out of scope:** manifests already built and published by third-party
  plugins. They are consumed as-is; a `Flag` with `default: "true"` in an
  old fragment keeps today's behaviour until the plugin is rebuilt.
- **No schema change.** No new manifest field and no new `ArgumentKind`.
  `RUNNER_SCHEMA_VERSION`, Python `SCHEMA_VERSION` and the manifest schema
  version stay as they are.
- **`Count`** (`toolr.types.Count`) is a separate type and is not affected.

## Documentation

`docs/writing-commands/arguments.md`, section "Boolean flags", must say:

- A `bool` parameter is a flag. It defaults to `False`; passing the flag
  sets it to `True`.
- Name booleans after what passing the flag does: `dry_run`, `force`,
  `skip_tests`, `no_cache`.
- For a feature that is on by default, write `no_colour: bool = False`
  (CLI `--no-colour`). In the body, either test the flag as written
  (`if no_colour: disable_colour()`), or rename it once at the top
  (`colour = not no_colour`) and write positive logic below.
- Don't stack negatives: `no_skip`, `disable_no_cache`.
- For more than two states, use `Literal["auto", "always", "never"]` or an
  `Enum`, not `bool | None`.

The `toolr-command-authoring` skill's prose must carry the same rule.

## Release notes

This is a breaking change for any command that uses one of the rejected
shapes. The `UNRELEASED.md` entry must say so, list the rejected shapes,
and give the rewrite for each.
