# Bash completion word handling: design

**Status:** Approved (2026-11-04). Decisions recorded under "Decisions"; the bash 4.4 floor
supersedes the bash 3.2 parts of the design below.

Follow-up findings FU-16 (low, bug) and FU-17 (low, bug), fixed together because both live in
how `crates/toolr-core/src/complete/scripts/bash.sh` builds the words it sends to
`toolr __complete`.

## Problem

### FU-16: words containing a wordbreak character

Bash's default `COMP_WORDBREAKS` is `` "'@><=;|&(: `` plus whitespace. Readline replaces only the
text after the last of those characters in the word under the cursor. Since bash 4, `COMP_WORDS`
is also split at them.

The script takes its words from bash-completion's `_init_completion`, which rebuilds nothing by
default. Bash 5 sends `files/host:p` to the engine as three words, `files/host`, `:` and `p`.
Bash 3.2 sends it whole, but each reply holds the whole word and readline puts it after the `:`,
so the typed prefix appears twice. Fixed choice values with a `:` or `=` (`Literal["a:b"]`)
break the same way.

### FU-17: a user `IFS` without a space

`local args=("${words[@]:1:cword-1}" "$raw_cur")` relies on a quoted array slice expanding to
one word per element. Bash 3.2 joins the slice with the first character of `IFS` when `IFS`
lacks a space. `IFS=$'\n'`, `$'\t'` and `:` all trigger this. `IFS=''` does not. Bash 4.4 and 5.2
are unaffected (checked in `bash:4.4` and `bash:5` containers).

### Third finding: the script needs bash-completion

`_init_completion || return` makes bash-completion 2.x a hard dependency. Without it every Tab
prints `bash: _init_completion: command not found` and completes nothing. bash-completion 2.x
doesn't load on bash 3.2 (the 2.16 loader in `/etc/profile.d` requires 4.2), so stock macOS
`/bin/bash` 3.2 never had working toolr completion. The
shell tests missed this because their driver stubs `_init_completion`.

### Reproduction

On `main` at `bf5648f6`, with the dev binary and a scratch repo whose `tools/ci.py` declares
`mode: Literal["fast", "a:b", "a:c", "x=yes"]` and `cfg: pathlib.Path | None`, plus the files
`files/host:port.txt` and `files/other.txt`. Engine, called directly:

| `toolr __complete "$PWD" …`  | Result                             |
| ---------------------------- | ---------------------------------- |
| `ci lint --cfg files/host:p` | exit 10 (complete paths)           |
| `ci lint --cfg files/host : p` | exit 0, no candidates            |
| `ci lint --mode a:`          | `a:b`, `a:c`                       |
| `'ci lint --mode' ''`        | exit 0, no candidates              |

Real Tab presses ran through an interactive bash on a pty. `_init_completion` was either a
stand-in that copies `COMP_WORDS` or, on Linux, the real bash-completion 2.16:

| Shell                          | Typed                        | `COMP_WORDS` at the word  | Outcome                            |
| ------------------------------ | ---------------------------- | ------------------------- | ---------------------------------- |
| bash 3.2.57, no bash-completion | `--mode f`                  | -                         | `_init_completion: command not found` |
| bash 3.2.57, stand-in          | `--cfg files/host:p`         | `files/host:p`            | line becomes `files/host:files/host:port.txt` |
| bash 3.2.57, stand-in          | `--mode a:`                  | `a:`                      | line becomes `a:a:`                |
| bash 3.2.57, stand-in          | `--mode x=y`                 | `x=y`                     | line becomes `x=x=yes`             |
| bash 5.2.37                    | `--cfg files/host:p`         | `files/host`, `:`, `p`    | no candidates                      |
| bash 5.2.37, bash-completion 2.16 | `--mode a:b`              | `a`, `:`, `b`             | no candidates                      |
| bash 3.2.57, stand-in, `IFS=$'\n'` | `--mode f`               | `f`                       | engine argv `<ci lint --mode> <f>`, no candidates |
| bash 5.2.37, stand-in, `IFS=$'\n'` | `--mode f`               | `f`                       | `fast`                             |

The Linux runs used a stub `toolr` that logs its argv, because the dev binary is a macOS build.

## Goals

- Names and values containing `:` or `=` complete in bash 4.4 and 5, as values and as file names,
  without duplicating the typed prefix.
- A word typed earlier that contains a wordbreak reaches the engine as one word.
- The script runs without bash-completion.
- On a bash older than 4.4, sourcing prints one warning line and registers nothing.
- Hardening stays: no `eval`, values and file names are never executed, and the script stays
  `set -e`- and `set -u`-safe.

## Scope

- In: `bash.sh`, `complete_shells.rs`, the macOS CI job (it installs a bash 4.4+), `docs/cli.md`
  (bash floor, and the install path at line 283, which named `~/.bash_completion.d/` while
  `install.rs` writes `~/.local/share/bash-completion/completions/`), a release note.
- Bash floor: completion needs bash 4.4 or newer. On an older bash, sourcing the script prints
  one platform-neutral line to stderr and registers nothing. FU-17 was bash 3.2-only and is
  closed as unsupported.
- Out: `--opt=value` completion (FU-15). An `--opt=` word is rebuilt whole and sent to the
  engine, which returns nothing for it, as today.
- FU-15 builds on this: it reuses `raw_cur` (the whole word) and `raw_prefix` (the part readline
  keeps) to split `--opt=` from the value.
- Out: words with an unescaped `'` or `"`. Readline does not break inside a quote, and such words
  match nothing today either.
- Out: changing where `toolr self completion install bash` writes the script. Only
  bash-completion's lazy loader reads that directory, so without the package the user sources the
  script from `~/.bashrc`, as its header says. Changing the location is a follow-up decision.

## Design

### Rebuild words from `COMP_LINE`

`_toolr_complete` drops `_init_completion` and rebuilds the words itself. It walks
`COMP_LINE` up to `COMP_POINT`. Before each `COMP_WORDS[i]` it strips leading whitespace, then
consumes the word literally. A piece with no whitespace before it is appended to the previous
word. At `COMP_CWORD` the rest of the line up to the cursor is the current word, so a cursor
inside a word completes only what precedes it, as `_init_completion` did.

This hardcodes no break characters and costs one loop. On bash 3.2, where `COMP_WORDS` is never
split at wordbreaks, it rebuilds the same words. `args` is then built element by element from the
rebuilt words, so the array slice goes away.

Rejected:

- `_init_completion -n :=` with `__ltrim_colon_completions`. It keeps the bash-completion
  dependency, which bash 3.2 can't meet.
- Re-tokenising `COMP_LINE` from scratch. It means reimplementing shell quoting.
- Removing `:` from `COMP_WORDBREAKS`. It changes completion for every command in the user's
  shell.

### Keep the prefix readline will not replace

The loop that turns `cur` into `raw_cur` (it drops one level of backslash escaping) also records
`raw_prefix`: `raw_cur` up to and including the last unescaped character that is in
`COMP_WORDBREAKS`, leaving out whitespace and quote characters. Escaped breaks (`a\:b`) don't
count, because readline doesn't break on them either. `COMP_WORDBREAKS` is read as
`${COMP_WORDBREAKS-}` for `set -u`.

Candidates are filtered against the full `raw_cur`, as today. When `raw_prefix` is non-empty,
each reply is `_toolr_quote "${value#"$raw_prefix"}"`, so only the remainder is quoted and
inserted. The typed prefix stays as the user wrote it.

The quoted-form match (`$q == "$cur"*`) is unchanged.

`_toolr_complete_paths` takes `raw_prefix` as a third argument and trims each `compgen` name the
same way before quoting. A leading `~` is inside the prefix whenever the prefix is non-empty, so
the bare-`~` handling only applies when the prefix is empty. Directory marking and `compopt`
use the full name, as now.

The script keeps three separate variables: `cur` (typed), `raw_cur` and `raw_prefix`.

### `IFS`

Superseded by decision 5: FU-17 needs bash 3.2, so this section no longer applies.

`_toolr_complete` starts with `local IFS=$' \t\n'`. `_toolr_complete_paths` inherits it through
bash's dynamic scope. The `read` loops already set `IFS` per command (`IFS=$'\t' read`,
`IFS= read`), so they are unaffected. With the slice gone this is belt and braces, but it also
guards future unquoted expansions.

### Prototype

A prototype of this design (`proto.sh`, scratchpad) ran through the same pty drives:

| Shell       | Typed                                              | Outcome                     |
| ----------- | -------------------------------------------------- | --------------------------- |
| bash 3.2.57 | `--cfg files/host:p`                               | `files/host:port.txt`       |
| bash 3.2.57 | `--mode x=y`                                       | `x=yes`                     |
| bash 3.2.57 | `--mode a:`                                        | lists `b`, `c` after `a:`   |
| bash 3.2.57 | `--cfg files/host:port.txt --mode f`               | `fast`                      |
| bash 3.2.57, `IFS=$'\n'`, `set -eu` | `--cfg files/host:p`       | `files/host:port.txt`       |
| bash 5.2.37 | `--cfg files/host:p`                               | `files/host:port.txt`       |
| bash 5.2.37 | `--cfg files/host:port.txt --mode f`               | engine argv `--cfg files/host:port.txt --mode f`, `fast` |
| bash 5.2.37, `IFS=$'\n'`, `set -eu` | `--cfg files/host:p`       | `files/host:port.txt`       |

The 3.2 runs used no `_init_completion` at all.

### Release note

> Bash completion now works for file names and choice values containing `:` or `=` (bash split
> them into separate words), and without the bash-completion package, which the script no longer
> needs. It now needs bash 4.4 or newer: on an older bash, sourcing the script prints one warning
> line and registers nothing. Completion never worked on bash 3.2, so nothing that worked stops
> working. Re-run `toolr self completion install bash --force`, or re-source the script, to pick
> it up.

## Testing

`complete_shells.rs`, on every `bashes()` entry.

- **Driver.** `run_bash_script` sets `COMP_LINE`, `COMP_POINT`, `COMP_WORDS` and `COMP_CWORD` the
  way bash does, and no longer defines `_init_completion` (only the `OLD_BASH_SCRIPT` test keeps
  the stub). So every test also proves the script runs without bash-completion. A `Split`
  argument chooses between the bash 3.2 shape (whole words) and the bash 4+ shape (`:` and `=`
  as separate words). Tests with a break character run both shapes on every bash.
- **Fixture.** The `Literal` gains `"a:b"`. `PATH_FILES` gains `files/host:port.txt`.
- **New cases.**
    - `--cfg files/host:p` gives `port.txt`.
    - `--mode a:` gives `b`.
    - An earlier `--cfg files/host:port.txt` still lets `--mode fa` give `fast`.
    - A file name with both `:` and a space quotes only the remainder.
    - `IFS=$'\n'` prepended to the script, as the errexit test prepends `set -e`: `--mode fa`
      gives `fast` and `--cfg files/host:p` gives `port.txt`.
    - `set -e` and `set -eu` variants of the colon cases.
- **Changed case.** `bash_tilde_is_escaped`: the `x=~` and `x=\~` inputs now expect `\~root`.
  The old expectation `x=\~root` was the `=` duplication bug, hidden by the stub.
- **End to end.** One test per bash runs an interactive `bash --rcfile` on a pty via
  `common::run_on_pty`, with `toolr ci lint --cfg files/host:p<TAB><Enter>` preloaded. The rcfile
  sources the script and wraps `toolr` in a function that runs `command toolr` for `__complete`
  and otherwise prints `LINE:$*`. The test asserts `LINE:ci lint --cfg files/host:port.txt`. This
  is the only test where readline itself splits and inserts. The prototype ran this shape.
- **Coverage.** `scripts/` and `crates/*/tests` are both excluded from Codecov, so patch coverage
  isn't engaged unless a change touches `src/`.

## Decisions

Taken 2026-11-04:

1. The script drops the bash-completion dependency (`_init_completion`). Bash 5 without the
   package hits the same `command not found`. Tests run the real script with nothing
   pre-defined, and one named test fails on the old script for exactly that error.
2. An end-to-end test presses Tab in a real interactive bash on a pty, next to the driver tests.
3. A word with an unescaped open quote is left untrimmed, and the limitation is documented.
4. `docs/cli.md` gets the install path the code uses. `install.rs` is unchanged.
5. Completion needs bash 4.4 or newer. On an older bash the script prints one line, for example
   `toolr: tab completion needs bash >= 4.4 (this is 3.2.57(1)-release); upgrade bash to enable
   it.`, and does not register the function. The version check uses bash 3.2-safe syntax, so
   sourcing never errors. User-facing text names no package manager. Every bash 3.2 workaround
   goes: the newline-string `~` scan (an associative array replaces it), the `compopt` guards,
   the empty-array `set -u` handling. FU-17 and its `local IFS` go with them, because nothing
   on 4.4+ depends on `IFS` once the array slice is gone. Shell tests run on bash 4.4+ only and
   fail under `CI` when none is found. The macOS CI job installs bash with Homebrew, as it does
   fish. One test sources the script under `/bin/bash` 3.2, when present, and checks for the
   single warning line and no registration.

Not breaking: stock macOS bash 3.2 never completed anything, because every Tab failed on the
missing `_init_completion` (bash-completion 2.x doesn't load on 3.2).
