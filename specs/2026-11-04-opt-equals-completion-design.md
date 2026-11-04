# `--opt=value` completion: design

Follow-up FU-15, logged by the path completion work
(`specs/archive/2026/2026-11-04-path-completion-design.md`, decision 4). Stacked on
`fix/bash-completion-words` (FU-16 colons, FU-17 IFS).

## Problem

Completing the value in `--opt=value` form returns nothing, for every value type and for
built-ins too. The engine (`crates/toolr-core/src/complete/engine.rs`) sees `--mode=a` as the
in-progress word, finds that it starts with `--`, and classifies it as a flag-name slot. No flag
name starts with `--mode=a`, so the result is empty.

### What `__complete` returns today

Reproduced on `main` at `bf5648f6` with the dev binary and a scratch repo whose `tools/ci.py`
declares `mode: Literal["alpha", "abc", "beta"]`, `cfg: pathlib.Path | None` and
`out: DirectoryPath | None`:

| Request (`toolr __complete "$PWD" …`)            | Output                   |
| ------------------------------------------------ | ------------------------ |
| `ci lint --mode a`                               | `abc`, `alpha`, exit 0   |
| `ci lint --mode=` / `--mode=a`                   | nothing, exit 0          |
| `ci lint --cfg=` / `--out=files/`                | nothing, exit 0 (not 10/11) |
| `self build-manifest --output=` / `--source-dir=` | nothing, exit 0         |
| `project init --venv-location=c`                 | nothing, exit 0 (`--venv-location c` gives `cache`) |
| `ci lint --mode = a` (bash 4+ split shape)       | nothing, exit 0          |
| `ci lint --mode = ""`                            | flag names, exit 0       |

### What each shell sends

Measured with a logging `toolr` shim on `PATH`, driving each shell through a pty:

- **bash 3.2** (`/bin/bash`, the only bash on the machine). `COMP_WORDBREAKS` contains `=`
  and `:`, yet `COMP_WORDS` keeps `--mode=a` as one word (`cword=3`). Readline's own word,
  passed to the completion function as `$2`, is `a`, and readline replaces only that part.
  A prototype returning bare `alpha` turned `--mode=al` into `--mode=alpha`, and bare
  `compgen -f` names turned `--cfg=files/w` into `--cfg=files/with space.txt`.
- **bash 4+** splits `COMP_WORDS` at every `COMP_WORDBREAKS` character, so the words are
  `--mode`, `=`, `a`. Not validated locally (no bash 4+ installed). CI's Linux bash 5 must
  confirm it through the tests below.
- **zsh** passes the whole `--mode=a` word to `__complete`. A prototype that runs
  `compset -P 1 '*='` and then `compadd` bare values inserts `--mode=alpha`, and
  `compset` plus `_files` inserts `--cfg=files/with space.txt`. `compset -P 1` stops at the
  first `=`, so `--mode=x` completes to `--mode=x=~root`.
- **fish 4.9.3**: `commandline -ct` is the whole `--mode=a`, and fish filters candidates
  against it. Bare `alpha` gives nothing; `--mode=alpha` works. `__fish_complete_path
  "--cfg=/tm"` already returns `--cfg=/tmp/`, and `__fish_complete_directories "--out=/tm"`
  returns `--out=/tmp/`.

So the engine must recognise the form, and every script needs a change: the shells disagree
on whether the `--opt=` part belongs in the candidate.

## Goals

- `--mode=<TAB>` offers the choices, and `--mode=a<TAB>` filters them, in bash, zsh and fish.
- A path-typed `--cfg=<TAB>` falls back to native file (or directory) completion of the part
  after `=`, keeping `--cfg=` in the line.
- Built-in options get the same (`self build-manifest --output=`, `project init
  --venv-location=`).
- Values and file names stay data: no `eval`, nothing executed, errexit- and nounset-safe on
  bash 3.2.

## Non-goals

- Short attached values. clap accepts `-c=value` and `-cvalue`, but toolr's short spellings
  are one-char aliases only, and `-cvalue` can't be told apart from a cluster of short flags
  without clap's full parser. `-c value` already works.
- Root flags (`toolr --log-level=…`). `merge_root_flags_at_top_level` serves names only.
- Tilde expansion after `--opt=`. bash, zsh (without `MAGIC_EQUAL_SUBST`) and fish all leave
  `--cfg=~/x` unexpanded when the command runs. Completion keeps a typed `~` as typed and
  doesn't try to fix the shell's semantics.
- Accepting bash 4's split shape (`--mode`, `=`, `a`) in the engine. FU-16 rebuilds the
  shell word before calling `__complete`, so the engine sees one shape. See decision 1.

## Design

### Engine: the attached value slot

`classify_leaf_args` and the no-child branch of `classify_dispatcher` check, before
`pending_flag_value`, whether the in-progress word is `--name=partial`:

```rust
/// `--name=partial`: the value slot of `--name`, with `partial` as the prefix. Splits at the
/// first `=`, since a value may itself contain one.
fn attached_value<'a>(command: &'a Command, word: &str) -> Option<Slot<'a>>
```

It returns `None` unless the word starts with `--` and contains `=`. Otherwise it splits with
`split_once('=')` and resolves the name with `find_flag`, so long aliases work too. A
value-taking flag gives `Slot::FlagValue { argument, prefix: partial }`. A boolean flag, an
unknown name or a bare `--=` gives `Slot::None`: nothing to offer, and never flag names.

Because the slot is the ordinary `FlagValue`, everything downstream is shared:
`allowed_values` filtering, the path check in `complete()` (exit 10/11), and the unsafe-value
filter. Candidates are **bare values** (`alpha`, not `--mode=alpha`). Committed
`--flag=value` tokens already count as one token in `positional_indices`. A test pins that.

Built-ins need no change: `builtin_completions::derive_argument` already copies clap's
possible values into `allowed_values` and value hints into `resolved_type`, and the synthetic
commands go through the same engine.

### Where the `--opt=` prefix lives: the scripts

The engine returns bare values, and each script adds what its shell needs:

| Shell | Values (exit 0)                                   | Paths (exit 10 / 11)                         |
| ----- | ------------------------------------------------- | -------------------------------------------- |
| bash  | bare value, trimmed to readline's word (`$2`)     | `compgen` on the part after `=`, trimmed the same way |
| zsh   | `compset -P 1 '*='`, then `compadd` bare values   | `compset -P 1 '*='`, then `_files` / `_files -/` |
| fish  | print `$opt_prefix$value` for each line           | unchanged: `__fish_complete_path "$current"` handles `--opt=` |

Why not have the engine print `--mode=alpha`:

- The path fallback can't carry a prefix: exit 10/11 keep stdout empty, because a zsh or fish
  script from before the path work inserts any stdout line as a candidate. So every script
  splits the word at the first `=` anyway, and doing it for values too keeps one rule.
- An old bash script given `--mode=alpha` for the word `--mode=a` on bash 3.2 inserts it in
  place of readline's `a`, leaving `--mode=--mode=alpha`. Bare values match nothing in an old
  script, which is today's behaviour.
- A format flag on `__complete` would make a new script fail against an old binary, which
  rejects the unknown option.

The split condition is the same everywhere: the word matches `--?*=*`, the prefix is
everything up to and including the first `=`, and the value is the rest. If the engine didn't
resolve a value-taking flag it returns exit 0 with no candidates, so a script splitting a word
the engine rejected adds nothing.

### bash

FU-16 rebuilds the in-progress word from `COMP_LINE` / `COMP_POINT` (so bash 4+'s `--mode`,
`=`, `a` arrives as `--mode=a`, as on 3.2) and trims each candidate to what readline replaces,
in the manner of bash-completion's `__ltrim_colon_completions`, for `:`. FU-15 reuses both:

- `__complete` receives the rebuilt word unchanged; the engine splits it.
- On exit 0, a candidate matches when it starts with the value part (`${raw_cur#*=}`). The full
  text is `opt_prefix$value`. FU-16's trim strips the part of the word before readline's
  word (`$2`), and the result is quoted with `_toolr_quote`. This also covers a value holding
  `=` or `:`: for `--mode=x=~r`, readline's word is `~r`, so `~root` is inserted, escaped as
  usual.
- On exit 10/11, `_toolr_complete_paths` gains the option prefix as an argument. It runs
  `compgen` on the value part, so a typed leading `~` keeps its current handling, and each
  name gets the same prefix-then-trim step before quoting. The `/` suffix and `nospace` rules
  are unchanged.
- If `=` has been removed from `COMP_WORDBREAKS`, `$2` is the whole word, nothing is trimmed,
  and `--mode=alpha` is inserted whole, which is correct.

**Reconcile with FU-16.** This assumes FU-16 provides a rebuilt word and a readline-word trim.
Its spec (`specs/2026-11-04-bash-completion-words-design.md` on `fix/bash-completion-words`)
didn't exist when this was written. If it lands a different shape, Task 3 adapts to it and
doesn't add a second word rebuilder or trim.

### zsh

When `${words[CURRENT]}` matches `--?*=*`, the script calls `compset -P 1 '*='` before
`compadd` (exit 0) or `_files` (exit 10/11). `compset` moves `--mode=` into `IPREFIX`, so zsh
matches and quotes the bare values itself. `-P 1` stops at the first `=`.

### fish

`set -l opt_prefix (string match -r -- '^--[^=]+=' $current)`. On exit 0, each line is printed
as `"$opt_prefix$line"` (the description after the tab is kept). Paths need nothing new.
`__fish_complete_path` and `__fish_complete_directories` strip and re-add `--opt=` themselves
(checked on fish 4.9.3; the shell test covers the CI version).

### Upgrade path

Same as the path work: users need to reinstall the script. With an old script, a new binary
offers nothing for `--opt=`, as now. A new script against an old binary gets the old empty
result. Release note:

> `--opt=value` now tab-completes the value, choices or file paths, in bash, zsh and fish, for
> both your commands and built-ins such as `toolr self build-manifest --output=`. Re-run
> `toolr self completion install <shell> --force` to pick it up.

### Docs

- `docs/cli.md` `__complete`: one sentence saying a `--name=partial` word completes as the
  value of `--name`, with bare values. The exit-code table is unchanged.
- `docs/concepts.md` "Tab completion": `--opt=value` completes like `--opt value`.

## Testing

- **Engine** (`complete/tests.rs`): `--mode=` gives every choice and `--mode=a` filters them;
  `--mode=x=` matches a value containing `=`; `--cfg=` gives `Paths` and `--out=` gives
  `Directories`; a long alias resolves; `--verbose=` (boolean) and `--nope=` give nothing,
  not flag names; a dispatcher's own `--flag=` and a grafted child's `--flag=` both work;
  `--mode=alpha ""` still counts positionals correctly.
- **Binary** (`crates/toolr/tests/complete_paths.rs`): `--cfg=""` exits 10 and `--out=""`
  exits 11 with empty stdout; `--mode=` exits 0 with bare `a` and `b`;
  `self build-manifest --output=` exits 10, `--source-dir=` exits 11;
  `project init --venv-location=c` gives `cache`.
- **bash** (`complete_shells.rs`, every `bashes()` entry): the driver gains the readline word
  `$2` and a 4+-shaped variant (`words=(… --mode = dr)`, `cword` on `dr`), so the bash 5 path
  runs on CI. Cases: `--mode=` offers every choice quoted; `--mode=dr` filters;
  `--mode=x=` inserts the trimmed, escaped `~root`; `--cfg=files/` gives the `%q` file names
  with `/` on `files/sub/`; `--out=files/` gives only `files/sub/`; `--cfg=~/` keeps the bare
  `~`; no sentinel file appears; `set -euo pipefail` callers see rc 0 and no stderr. The
  old-script test gains a `--mode=dr` case: nothing offered.
- **zsh** (`zsh_complete` driver): stub `compset` to record its arguments. `--mode=a` records
  `COMPSET:-P 1 *=` then `COMPADD:…`; `--cfg=files/` records `COMPSET` then `FILES:`;
  `--out=` records `FILES:-/`; `--mode a` (no `=`) records no `compset`.
- **fish** (`complete -C`): `toolr ci lint --mode=dr` lists `--mode=dry run` and `--mode=dry walk`;
  `--cfg=files/` lists `--cfg=files/with space.txt` and `--cfg=files/sub/`; `--out=files/`
  lists only `--out=files/sub/`; the sentinel stays absent.
- Patch coverage of 99% on `engine.rs`. Scripts and `crates/*/tests` are excluded.

## Decisions

Proposed 2026-11-04, for confirmation:

1. The engine accepts one word shape, `--name=partial`. It doesn't also accept bash 4's split
   `--name`, `=`, `partial`: that would make a literal `=` positional ambiguous, and FU-16
   already rebuilds the word. Old bash scripts on bash 4+ therefore keep offering nothing.
2. The `--opt=` prefix is added in the scripts. The engine returns bare values.
3. Short attached values (`-c=value`, `-cvalue`) and root flags stay out of scope.
