# Bash completion: ANSI-C quoted words (`$'…'`): design

Audit follow-up FU-40, found in round 2 of PR #690. Present on `main`.

**Written against PR #690's script** (branch `fix/bash53-compgen-dequote`, `bash.sh` as of its tip), which rewrote
`_toolr_parse_word`, `_toolr_quote`, `_toolr_complete_paths` and `_toolr_list_paths` and introduced the open-quote
handling this builds on. #690 must land first; the line references below are to that script, not to `main`'s.

## Problem

toolr itself inserts `$'…'` for a path name holding a newline or control character: `$'files/an\nd'`,
`~/$'files/an\nd'` and `$HOME/$'files/an\nd'`. Once such a word is on the line, Tab can no longer extend it, and
the insertion itself can leave the line broken.

### Reproduction

Bash 5.1.16, 5.2.37 and 5.3.0 (mise `conda-bash`) all behave the same. `files/` holds `an\nd` and `an\nx` (a real
newline), `plain`, `it's`, `tab\there`. Two harnesses: a direct call of `_toolr_complete` with `COMP_LINE`,
`COMP_WORDS` and `COMP_CWORD` set and a `toolr` stub returning 10 (`COMPLETE_PATHS`), and round 2's pty harness
with the same stub (a key bound with `bind -x` prints `READLINE_LINE`).

| Typed after `--cfg`           | `COMPREPLY`                           | Result                                     |
| ----------------------------- | ------------------------------------- | ------------------------------------------ |
| `files/an`                    | `$'files/an\nd'`, `$'files/an\nx'`    | ok, but readline inserts `$'files/an\n`    |
| `$'files/an`                  | none                                  | **nothing offered**                        |
| `files/$'an`                  | none                                  | **nothing offered**                        |
| `$'files/an\nd'`              | none                                  | **nothing offered**                        |
| `~/files/an`                  | `~/$'files/an\nd'`, `…x'`             | ok, same open insertion                    |
| `~/$'files/an`, `~/files/$'an`| none                                  | **nothing offered**                        |
| `$HOME/$'files/an`            | none                                  | **nothing offered**                        |
| `files/plain'`                | one empty string                      | **`''` candidate** (exact match)           |

Pressing Enter after the first row's Tab shows `PS2>`: the pty line is `toolr x --cfg $'files/an` plus a literal
newline inside an unclosed `$'`. That is the second symptom of FU-40. The last row is the third: the name equals
the text before the open quote, so the remainder is empty and the reply array holds `''`; with several matches
the list shows a blank entry and the common prefix is empty.

## Root cause

1. `_toolr_parse_word` knows `\`, `'` and `"`, not `$'`. For `$'files/an` it appends the `$` as an ordinary
   character, then opens a plain single quote. `_toolr_raw_cur` is `$files/an`, and the no-execution guard in
   `_toolr_complete_paths` (`[[ $_toolr_raw_cur != *[\$\`]* ]] || return 0`) drops it. For a closed
   `$'an\nd'/` the same happens, and the word would not have matched anyway: the backslash-n stays two characters.
2. Replies for a bare word come from `printf %q`, which quotes a control character as one `$'…'` run over the
   whole name. Readline inserts the longest common prefix of the replies, and that prefix can end anywhere in the
   run, including mid-escape.
3. For an open quote, `_toolr_complete_paths` reuses `_toolr_quote_after`; a candidate equal to
   `_toolr_raw_prefix` leaves `_toolr_q` empty and still appends it.

## Design

Three changes in `bash.sh`, for every supported bash. No `eval`; nothing typed is ever expanded.

### 1. `_toolr_parse_word` decodes `$'…'`

Outside any quote, a `$` immediately followed by `'` opens an ANSI-C run. The `$` is not appended to
`_toolr_raw_cur`: readline keeps the text up to and including the `'`, so `_toolr_raw_prefix` (the text before the
open quote) excludes it, the same as for a plain `'`. `_toolr_qchar` becomes `$'` while the run is open. `"$'`,
`'$'` and `\$'` are not openers; `$` there is literal, as today.

Inside the run, characters are literal until an unescaped `'`, except `\`, which decodes:

| Escape                              | Decoded to                                  |
| ----------------------------------- | ------------------------------------------- |
| `\a \b \e \E \f \n \r \t \v`        | the control character                       |
| `\\ \' \" \?`                       | the character after the backslash           |
| `\N`, `\NN`, `\NNN` (octal digits)  | the byte                                    |
| `\xH`, `\xHH`                       | the byte                                    |
| `\uH…` (1-4), `\UH…` (1-8)          | the character, in the current locale        |
| a lone `\` at the end of the word   | dropped: the user is mid-escape             |

Anything else is declined: an unknown escape (`\q`), `\cX`, an escape that decodes to NUL (`\0`, `\x00`,
`\u0000`), and `\u`/`\U` where the locale can't encode the result. A declined word sets `_toolr_declined=1`, and
`_toolr_complete` returns no completions for it (no `toolr __complete` call, no listing). This is the same
outcome the guard gives today, reached before any engine call. Bash itself keeps an unknown escape's backslash;
declining is stricter on purpose, since a guess about what the user means gains nothing here.

Decoding uses `printf -v out "\\x$digits"` (or `\\u`, `\\U`, `\\$octal`). The format string is a fixed prefix plus
digits the parser has already matched against `[0-9A-Fa-f]{1,2|4|8}` or `[0-7]{1,3}`: no `%`, no `$`, no
quote. `printf -v` never parses or expands its output. The decoder is a closed `case` over the escape letter and
nothing else. The `\u` path is checked by comparing the result with the format text, since bash in a C locale
prints `é` literally: a result starting with `\` is declined.

**Safety.** Nothing here widens what runs on Tab. The decoder only appends characters to `_toolr_raw_cur`, a
string, and the existing guard still runs after decoding, on the decoded value. So `$'$(touch X)'`,
`$'\x24(touch X)'`, `$'\044(touch X)'`, `$'$(…)'`, `$'\x60touch X\x60'` and a literal backtick all decode
to a `$` or backtick and are dropped. The guard stays absolute: no `$` or backtick, typed or decoded, reaches a
listing, `compgen` or `${!…}`. The listers still take the value as a quoted glob part (`"$_toolr_word"*`), which
matches literally.

Two follow-ups close gaps that decoding would open:

- `_toolr_var_ok` and `_toolr_tilde_ok` are computed from the typed word `_toolr_cur`, not the decoded value, and
  `_toolr_var_ok` now needs `$` followed by a name start or `{` (not `'`). Otherwise `$'\x24HOME/files/` decodes
  to `$HOME/files/`, matches `_toolr_var_re` and is listed under `$HOME`, while bash would see a literal `$HOME`
  directory. `_toolr_tilde_ok` additionally needs the text up to the first `/` to hold no quote, backslash or
  `$'`: bash does not expand `~$'x'/`, and without this a `~user` form with a quoted part would reach
  `compgen`. (Step 0 of the plan checks whether `main` already misbehaves on `~"root"/`, and adds that case
  to the same fix if so.)
- A decoded `~` or `$NAME` at the start is data: `$'~/files/'` lists a directory literally named `~`.

### 2. Reply quoting

`_toolr_quote` gains one context and one form.

**Open `$'` (`_toolr_qchar` is `$'`).** The reply is inserted inside the run, so it is escaped as ANSI-C:
`\` becomes `\\`, `'` becomes `\'`, a control character becomes `\n \t \r \a \b \f \v \e` or `\NNN` (octal, the
form `printf %q` itself uses), and everything else is unchanged. Readline appends the closing `'` on a single
match, so `$'files/solo\nname'` closes correctly. No change for names without those characters beyond the
two escapes.

**Closed forms: group each control character separately.** When `printf %q` would use `$'…'` (the result holds an
unescaped `$'`), build the reply as the concatenation of `printf %q` of each character, so a control character is
its own run: `files/an$'\n'd`, not `$'files/an\nd'`. The common prefix of `files/an$'\n'd` and `files/an$'\n'x` is
`files/an$'\n'`, which is closed. That is the mechanism that keeps a common-prefix insertion from leaving `$'`
open. The fast path (no control character, so no `$'`) is the current single `printf %q`, so the
`scales_linearly` test is unaffected. The same grouping applies in the `"` and `'` contexts, where the reply
closes the quote, adds the run and reopens it (`files/an"$'\n'"d`, `files/an'$'\n''d`): today a newline goes in
raw there, and readline cannot insert it as a line break.

Names that differ **at** the control character (`a\nb` and `a\tb`) still give a common prefix `a$'\`. Replying
with a shorter or synthetic candidate would either remove the listing on a second Tab or add a junk entry, so this
is accepted and documented (open question 1). Typed from a bare word it is the only way left to reach an open
quote; typed from an open `$'` the open state is the user's own.

### 3. Exact-match candidates

In `_toolr_complete_paths`, a candidate equal to `_toolr_raw_prefix` has no text after the open quote to offer.
It is skipped unless it is a directory, where the reply is `/` (readline then closes the quote after it). The
`''` entry goes away; a lone exact file under an open quote now offers nothing, and the user closes the quote
themselves. Applies to `'`, `"` and `$'` contexts alike.

### Not changed

- The engine, `toolr __complete`, and `_toolr_list_paths`. The listing is already a literal glob.
- `~user` forms: `compgen` still takes only words with no backslash or quote, so an ANSI-C `~user` word offers
  nothing. Choice values reuse `_toolr_parse_word` and `_toolr_quote`, so a value with a newline now completes
  the same way, but no path-specific code changes for it.
- Zsh and fish, which hand the word to their own parsers.

### Alternatives rejected

- **Decline every `$'` word** (return nothing, as the guard does now). Safe, but a name toolr itself inserted
  would be a dead end; the user has to retype it in another form.
- **Re-parse with `eval "x=$'…'"`.** One line, but it runs on Tab and is exactly what the guard exists to prevent.
- **A sentinel candidate** equal to the typed word, to force the common prefix back to the typed text. It lists
  as a junk entry.
- **Escaping a newline with a backslash.** `\<newline>` is a line continuation to bash and disappears.

## Open questions

1. The residual open `$'` for names differing at a control character: accept (recommended; very rare), or
   always reply with one candidate there and give up the listing.
2. Whether `!` inside an open or closed `$'…'` is history-expanded on bash 5.1 to 5.3. Plan step 5 checks it with
   `set -H`; if it expands, `!` must be escaped in the `$'` form as `\041`.
3. `\u`/`\U` depend on the locale. Decline in a non-UTF-8 locale (as designed), or fall back to decoding only
   when the result is a single valid character.
