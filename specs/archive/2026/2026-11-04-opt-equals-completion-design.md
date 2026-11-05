# `--opt=value` completion: design

Follow-up FU-15, logged by the path completion work
(`specs/archive/2026/2026-11-04-path-completion-design.md`, decision 4). Builds on FU-16 and
FU-17, which have landed on `main`
(`specs/archive/2026/2026-11-04-bash-completion-words-design.md`).
Line references (`bash.sh:107` and so on) point at the scripts on `main` before this change.

## Problem

Completing the value in `--opt=value` form returns nothing, for every value type and for
built-ins too. The engine (`crates/toolr-core/src/complete/engine.rs`) sees `--mode=a` as the
in-progress word, finds that it starts with `--`, and classifies it as a flag-name slot. No flag
name starts with `--mode=a`, so the result is empty.

### What `__complete` returns today

Reproduced on `main` at `3013af73` with the dev binary and a scratch repo whose `tools/ci.py`
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

### What each shell sends

Measured with a logging `toolr` shim on `PATH`, driving each shell through a pty:

- **bash ≥ 4.4**, the only bash the script supports: it refuses older ones
  (`bash.sh:2`, `:202-209`). Bash splits `COMP_WORDS` at every `COMP_WORDBREAKS` character,
  so the words are `--mode`, `=`, `a`. The script rebuilds the in-progress word from
  `COMP_LINE` / `COMP_POINT` (`bash.sh:30-50`), so `__complete` receives `--mode=a` whole.
  The shell tests cover both shapes through the `Split::Whole` / `Split::AtBreaks` driver
  variants (`SPLITS`, `complete_shells.rs:157-195`). The script never reads `$2`. Readline
  replaces only the text after the last wordbreak, and the script works that part out itself
  (`_toolr_raw_prefix`, `bash.sh:58-78`).
- **zsh** passes the whole `--mode=a` word to `__complete`. A prototype that runs
  `compset -P 1 '*='` and then `compadd` bare values inserts `--mode=alpha`, and
  `compset` plus `_files` inserts `--cfg=files/with space.txt`. `compset -P 1` stops at the
  first `=`, so `--mode=x` completes to `--mode=x=~root`.
- **fish 4.9.3**: `commandline -ct` is the whole `--mode=a`, and fish filters candidates
  against it. Bare `alpha` gives nothing; `--mode=alpha` works. `__fish_complete_path
  "--cfg=/tm"` already returns `--cfg=/tmp/`, and `__fish_complete_directories "--out=/tm"`
  returns `--out=/tmp/`, but fish 3.6 globs the whole word and returns nothing.

So the engine must recognise the form, and every script needs a change: the shells disagree
on whether the `--opt=` part belongs in the candidate.

## Goals

- `--mode=<TAB>` offers the choices, and `--mode=a<TAB>` filters them, in bash, zsh and fish.
- A path-typed `--cfg=<TAB>` falls back to native file (or directory) completion of the part
  after `=`, keeping `--cfg=` in the line.
- Built-in options get the same (`self build-manifest --output=`, `project init
  --venv-location=`).
- Values and file names stay data: no `eval`, nothing executed, errexit- and nounset-safe on
  bash ≥ 4.4.

## Non-goals

- Short attached values. clap accepts `-c=value` and `-cvalue`, but toolr's short spellings
  are one-char aliases only, and `-cvalue` can't be told apart from a cluster of short flags
  without clap's full parser. `-c value` already works.
- Root flags (`toolr --log-level=…`). `merge_root_flags_at_top_level` serves names only.
- Tilde expansion after `--opt=`. bash, zsh (without `MAGIC_EQUAL_SUBST`) and fish all leave
  `--cfg=~/x` unexpanded when the command runs. Completion keeps a typed `~` as typed and
  doesn't try to fix the shell's semantics.
- Accepting bash 4's split shape (`--mode`, `=`, `a`) in the engine. The bash script rebuilds
  the word before calling `__complete`, so the engine sees one shape. See decision 1.

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
| bash  | `--opt=` plus the value, trimmed to what readline replaces | path completion of the part after `=`, `--opt=` put back when readline replaces it |
| zsh   | `compset -P 1 '*='`, then `compadd` bare values   | `compset -P 1 '*='`, then `_files` / `_files -/` |
| fish  | print `$opt_prefix$value` for each line           | strip `--opt=`, complete the bare path, print `$opt_prefix$line` |

Why not have the engine print `--mode=alpha`:

- The path fallback can't carry a prefix: exit 10/11 keep stdout empty, because a zsh or fish
  script from before the path work inserts any stdout line as a candidate. So every script
  splits the word at the first `=` anyway, and doing it for values too keeps one rule.
- A format flag on `__complete` would make a new script fail against an old binary, which
  rejects the unknown option.

Today's `main` scripts stay harmless against the new binary. bash filters bare values out,
because they don't start with the typed `--mode=a` (`bash.sh:107`); on exit 10/11 it hands
`compgen -f -- "--cfg=files/"` the whole word, which lists nothing. zsh calls `_files` on the
whole word, as it does for any other path slot.

The split condition is the same everywhere: the word matches `--?*=*` (fish: `^--[^=]+=`),
the prefix is everything up to and including the first `=`, and the value is the rest. The
spellings differ on odd words such as `--=x`, which no flag resolves. If the engine didn't
resolve a value-taking flag it returns exit 0 with no candidates, so a script splitting a word
the engine rejected adds nothing.

### bash

The script already rebuilds the in-progress word (`bash.sh:30-50`) and unescapes it into
`_toolr_raw_cur`. `_toolr_raw_prefix` is the part of it readline keeps, up to the last
wordbreak (`bash.sh:58-78`). With the default `COMP_WORDBREAKS`, which holds `=`, typing
`--mode=a` gives `_toolr_raw_cur=--mode=a` and `_toolr_raw_prefix=--mode=`. `__complete`
receives `_toolr_raw_cur` unchanged; the engine splits it.

- **Values (exit 0).** When `_toolr_raw_cur` matches `--?*=*`, the script sets
  `_toolr_opt_prefix=${_toolr_raw_cur%%=*}=`, otherwise it leaves it empty. Each value becomes
  `_toolr_full=$_toolr_opt_prefix$_toolr_value`, and the existing filter, quote and
  `_toolr_quote_after` trim (`bash.sh:105-109`) run on `_toolr_full`. With `=` a break, the
  trim strips `--mode=` and `alpha` is inserted. A value holding `=` or `:` works the same way:
  for `--mode=x=~r`, `_toolr_raw_prefix` is `--mode=x=`, so `\~root` is inserted.
- **Paths (exit 10/11).** The script strips `_toolr_opt_prefix` from both `_toolr_raw_cur` and
  `_toolr_raw_prefix` before calling `_toolr_complete_paths`. Otherwise the `^`-anchored
  `$NAME/` regex (`bash.sh:124`, `:131`) and the `$`/backtick guard (`:151`) would see
  `--cfg=$HOME/` and drop it. The `/` suffix and `nospace` rules are unchanged.
- **`=` not in `COMP_WORDBREAKS`.** Readline then replaces the whole `--cfg=…` word, so every
  reply needs the option prefix put back. For values, `_toolr_raw_prefix` is empty and the
  whole `--mode=alpha` is inserted. For paths, `_toolr_complete_paths` takes a fourth argument,
  the text readline replaces before the path and that each reply must start with: `--cfg=` when
  `_toolr_raw_prefix` doesn't start with the option prefix, empty otherwise. It is prepended to
  every reply, plain names, a bare `~` and a kept `$NAME/` alike.

Known limitation: the split is keyed on the word's shape, so `--mode --x=y` (an allowed value
starting with `--`) splits on the wrong `=`.

### zsh

When `${words[CURRENT]}` matches `--?*=*`, the script calls `compset -P 1 '*='` before
`compadd` (exit 0) or `_files` (exit 10/11). `compset` moves `--mode=` into `IPREFIX`, so zsh
matches and quotes the bare values itself. `-P 1` stops at the first `=`.

It also fixes an empty-candidate bug. `lines=("${(@f)$(…)}")` turns empty output into one
empty element, so the `${#values} == 0` check (`zsh.zsh:44`) never fires and `compadd ""`
runs. After `compset -P 1 '*='` that could insert a stray space on `--verbose=<TAB>`. The
script drops empty lines, and calls `compset` before `compadd` only when there are values.

### fish

`set -l opt_prefix (string match -r -- '^--[^=]+=' $current)`. On exit 0, each line is printed
as `"$opt_prefix$line"` (the description after the tab is kept). Paths do the same: fish 3.7+
strips `--opt=` inside `__fish_complete_path`, but fish 3.6 doesn't, so the script strips it,
completes the bare path and prints `"$opt_prefix$line"` for each result.

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
- **bash** (`complete_shells.rs`, every `bashes()` entry): every case runs on both `SPLITS`
  (`Split::Whole`, `Split::AtBreaks`), plus a `.breaks()` variant with `=` taken out of
  `COMP_WORDBREAKS` (in the script and in the driver's split). Cases: `--mode=` offers every
  choice quoted; `--mode=dr` filters; `--mode=x=` inserts the trimmed, escaped `\~root`;
  `--cfg=files/` gives the `%q` file names with `/` on `files/sub/`; `--out=files/` gives only
  `files/sub/`; `--cfg=~/` keeps the bare `~`; `--cfg=$HOME/` keeps `$HOME/` bare;
  `--cfg=$(touch …)/` and its backtick form offer nothing and leave no sentinel; with `=`
  removed from `COMP_WORDBREAKS`, the whole `--mode=alpha` / `--cfg=files/…` reply comes back;
  `set -euo pipefail` callers see rc 0 and no stderr. The old-script test gains a `--mode=dr`
  case: nothing offered. The pty end-to-end test gains `--mode=` and `--cfg=` lines.
- **zsh** (`zsh_complete` driver): stub `compset` to record its arguments. `--mode=a` records
  `COMPSET:-P 1 *=` then `COMPADD:…`; `--cfg=files/` records `COMPSET` then `FILES:`;
  `--out=` records `FILES:-/`; `--mode a` (no `=`) records no `compset`; `--verbose=` records
  nothing at all.
- **fish** (`complete -C`): `toolr ci lint --mode=dr` lists `--mode=dry run` and `--mode=dry walk`;
  `--cfg=files/` lists `--cfg=files/with space.txt` and `--cfg=files/sub/`; `--out=files/`
  lists only `--out=files/sub/`; the sentinel stays absent.
- Patch coverage of 99% on `engine.rs`. Scripts and `crates/*/tests` are excluded.

## Decisions

Proposed 2026-11-04, for confirmation:

1. The engine accepts one word shape, `--name=partial`. It doesn't also accept bash 4's split
   `--name`, `=`, `partial`: that would make a literal `=` positional ambiguous, and the bash
   script already rebuilds the word.
2. The `--opt=` prefix is added in the scripts. The engine returns bare values.
3. Short attached values (`-c=value`, `-cvalue`) and root flags stay out of scope.
