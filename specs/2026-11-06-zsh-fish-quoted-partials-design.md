# zsh and fish: escaped and quoted partial values (FU-33)

Status: proposed. Audit follow-up FU-33, found in #685 round 2 and pre-existing.

Written against two open PRs, not `main` alone:

- #690 (`fix/bash53-compgen-dequote`) rewrites `bash.sh` with a quote-aware word parser
  (`_toolr_parse_word`) and quote-aware replies.
- #691 (`fix/complete-double-dash`) changes `engine.rs`. This design does not touch the engine, so the two
  don't conflict.

## Problem

The completion engine (`toolr __complete <cwd> <words…>`) matches the last word against raw values. A shell hands
the script that word as typed, with the user's quoting still on it, so nothing matches: `dry\ r`, `'dry r`,
`"dry r` against the choice `dry run`.

## Reproduction

Fixture: `Literal["fast", "dry run", "dry walk"]` on `--mode`, a `pathlib.Path` on `--cfg`, a file
`files/with space.txt`. Binary built from `main`; zsh 5.9, fish 4.9.3. "none" means no candidates.

| Shell | Typed partial | `main` | After the fix in this design |
| --- | --- | --- | --- |
| zsh | `--mode dry\ r` | none | `dry run` |
| zsh | `--mode 'dry r` | none | `dry run` |
| zsh | `--mode "dry r` | none | `dry run` |
| zsh | `--mode=dry\ r` | none | `dry run` |
| zsh | `--cfg files/with\ s` | `_files` (zsh's own, so it works) | unchanged |
| fish | `--mode dry\ r` | none | `dry run` |
| fish | `--mode 'dry r` | none | `dry run` |
| fish | `--mode "dry r` | none | `dry run` |
| fish | `--mode=dry\ r` | none | `--mode=dry run` |
| fish | `--cfg files/with\ s` | `files/with space.txt` (works) | unchanged, must stay on the raw word |
| bash on `main` | `--mode dry\ r` | `dry\ run` | covered by #690 |
| bash on `main` | `--mode "dry r`, `--mode 'dry r` | none | `dry run` on #690 |
| bash on `main` | `--cfg "files/with s` | none | `files/with space.txt` on #690 |

The zsh "after" column is from a stub driver (`compadd` replaced by a recorder, as in `complete_shells.rs`), not a
live Tab. The fish "after" column is from `complete -C` against a patched copy of `fish.fish`.
The bash rows were run against both scripts under bash 5.2 with a hand-built `COMP_*` environment.

Conclusion: #690 fixes the bash half of FU-33, including paths. What is left is **zsh and fish choice values**.
Paths already work on both, because they go through each shell's native path completer.
There is no bash remainder. One bash-adjacent gap stays out of scope: backtick and `$'…'` partials in bash.

## What each shell hands the script

- **zsh.** `$words[CURRENT]` and the earlier `$words` entries are raw: backslashes and quotes intact.
  `${(Q)…}` removes one level of quoting from a *closed* word.
  It leaves an *unterminated* quote untouched (`${(Q)"'dry r"}` is `'dry r`), so `'dry r` and `"dry r` need the
  closing quote appended before `(Q)`. `compstate[quote]` (`single`, `double`, `dollars`, `backtick`, empty) says which
  quote is open at the cursor.
- **fish.** `commandline -ct` returns the raw token (`dry\ r`, `'dry r`). `commandline -opc` returns the *earlier*
  tokens already unescaped. `string unescape` handles an unterminated quote.
  `__fish_complete_path` and `__fish_complete_directories` re-parse their argument as shell text
  (`complete -C"'' $target"`), so they want the raw form: unescaping `files/with\ s` before them yields nothing.

## Decision: unquote in the script, not the engine

Recommended: each script dequotes with its shell's own primitive and passes the engine plain text.

- The engine can't tell which dialect it is parsing (a backslash-space means different things inside zsh `"…"`
  and fish `'…'`).
  Adding a `--shell` flag and three parsers to Rust duplicates what each shell already does correctly.
- #690 settled the contract for bash: the engine gets the dequoted word. zsh and fish follow it, so the engine keeps
  one input shape and its tests stay shell-free.
- The earlier words already reach the engine unescaped on fish (`-opc`). Only zsh's earlier words and the current word
  need work, so the change is small.
- Cost: three scripts carry dequoting logic. Each is a couple of lines because the shell does the work.

Rejected: an engine-side `--raw` mode. It would put quote parsing on the Rust coverage gate and still need the
script to say which shell it came from.

## zsh

In `_toolr`:

- Dequote the current word and every earlier word with `(Q)`: `${(@Q)words[2,CURRENT-1]}`.
- For the current word, if `compstate[quote]` is `single`, `double` or `dollars`, append the matching closing
  character before `(Q)`, so the open quote is stripped. For `backtick`, offer nothing.
- The `--opt=value` test (`attached`) runs on the dequoted word.
- The `10`/`11` exits still call `_files` untouched: zsh's `_files` dequotes the word itself.

Reply quoting: keep `compadd -a values` **without `-Q`**. Without `-Q` zsh quotes each inserted match to fit the
quote state at the cursor (backslash outside quotes, inside `'…'` or `"…"` as needed), so the line stays valid.
`-Q` would insert the raw value and break on spaces, `$`, `'`. The existing test
`zsh_completion_never_executes_values` already forbids `-Q`; it stays.

Unverified premise, to be settled first in the plan: zsh removes the quotes from `PREFIX` before `_toolr` runs and
re-quotes through `compadd`. This is documented behaviour and it is how `_files` works. My attempt to confirm it
with a `zpty` session in this environment was inconclusive (the harness never reached a completion), so the first
plan task tries again and falls back to the stub driver plus the documented guarantee if `zpty` can't be made
reliable.

## fish

In `__toolr_complete`:

- Keep `set -l raw (commandline -ct)` and derive `current` as `string unescape -- $raw`.
- The engine gets `current`. `opt_prefix` and `bare` for the `0` reply branch come from `current`.
  fish matches the replies against the unescaped token, so `--mode=dry run` is right.
- The `10`/`11` branches keep using the raw word (`raw` minus the `--opt=` prefix) for `__fish_complete_path`.
  Unescaping there breaks paths with spaces (verified: it printed nothing).
- Print replies unescaped. fish escapes on insertion, as `complete -C` output shows (`dry run` for `dry\ r`).
  Do not pass `--no-escape`.

## Out of scope

- **FU-34** (zsh matcher-list: the engine prefix-filters before zsh's matcher runs, so `m:{a-z}={A-Z}` style matching
  never sees non-prefix candidates). Dequoting here makes the engine's prefix filter see the right text, nothing more.
  Letting zsh do the filtering is FU-34's decision and would change the engine contract; don't fold it in.
- bash backtick and `$'…'` partials; nothing in #690's remit changed here.
- Engine changes (#691's territory).

## Open questions

1. Is a `zpty` test stable enough for CI, or do we keep the stub driver and rely on documented zsh behaviour for the
   reply-quoting half? Recommendation: stub driver in the suite, a one-off manual `zpty` check recorded in the PR.
2. zsh `dollars` quoting (`$'dry r`): close with `'` and `(Q)` handles `$'…'`? Needs a stub test; if not, treat as
   `backtick` (offer nothing).
3. Should a stacked order matter? #690 only touches `bash.sh` and its tests; this work touches `zsh.zsh`, `fish.fish`
   and new tests in `complete_shells.rs`. Both edit `complete_shells.rs`, so expect a textual conflict in that file
   if neither merges first. Rebase on whichever lands first.
