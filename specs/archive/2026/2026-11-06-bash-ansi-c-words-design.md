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

## Changes during implementation

The design was a hypothesis; these parts changed once tested against bash 5.1.16, 5.2.37 and 5.3.0 on a pty.

- **Step 0.** On `main`, `~"root"/`, `~\root/` and `~$'root'/` all reached `compgen` (the guard checks the
  dequoted word, which holds no quote). The tilde hardening closes it, pinned by
  `bash_path_tilde_with_a_quoted_part_never_reaches_compgen`.
- **`\u` locale check.** A `printf -v` result starting with `\` does not detect a non-UTF-8 locale: run as a
  script with `LC_ALL=C`, bash's `printf '\u00e9'` emits the UTF-8 bytes anyway. The decoder now probes the
  locale itself: a two-byte `é` (`$'\xc3\xa9'`) has length 1 only in a UTF-8 locale. The `\` check stays, for
  code points `printf` can't encode.
- **Variables.** `_toolr_var_ok` became a regex on the typed word that captures the typed `$NAME` or `${NAME}`,
  and the decoded `$NAME/` must equal it. Otherwise `$HO$'ME'/`, `$HO"ME"/` or `$HO\ME/` decode to `$HOME/` and
  would list under `$HOME`, where bash expands `$HO`. The `"` and `\` forms were already wrong on `main`.
- **Closing a quote.** Readline appends the closing quote after a single match only if the character before
  the cursor isn't already that quote. A reply ending in the open quote character (`dq"` under `"`, `sq'` under
  `'` or `$'`, a trailing control character under `"` or `'`) therefore left the quote open. `_toolr_quote`
  now appends one more quote character in that case and sets `_toolr_qextra`, which a directory reply strips
  before its `/`. This generalises #690's `""` for a trailing `!`.
- **C1 controls.** In a UTF-8 locale `[[:cntrl:]]` matches U+0085, and its code point in octal (`\205`) is
  not its bytes. The escape inside a run is taken from `printf %q`, which escapes each byte (`\302\205`).
- **`$` in `COMP_WORDBREAKS`.** With `$` added to the default breaks, readline still replaces a whole word
  holding a closed `$'…'`. Treating that `$` as a special-prefix break doubled the text on the line, so the
  `$` of `$'` is never a break (pinned end to end).
- **Plan step 1's value test.** The engine drops any choice value holding a control character, so
  `bash_value_with_a_newline_completes_in_ansi_c_quotes` can't exist. `bash_value_completes_in_ansi_c_quotes`
  checks that choice values decode the same way instead.
- **Exact match for values.** The `''` reply also came from choice values (`--mode fast'`); the same skip
  applies there.

- **A `'` inside `$'…'`.** Readline knows only `'`, `"` and backslash, so it sees `$'…'` as `'…'`, where a
  backslash is literal and `\'` closes the quote. A reply holding `\'` (the design's `$'` escaping) or a typed
  `\'` left readline with no open quote while the script still had `$'` open, and readline inserted the reply
  unquoted: a name `q'$(touch X)` ran `touch X` on Tab and Enter (found in review). Inside a run `'` is now
  `\047`, and a typed `\'` is declined. Residual: names differing at `'` and `\001` share the prefix `\0`,
  which then decodes to NUL and offers nothing.

- **A reply starting with the open quote.** Readline's `insert_match` won't double an opening quote: a reply
  starting with the quote the user opened just before the replaced text overwrites that quote, so the reply's
  quoting flips and the rest lands unquoted. The `'`/`"` forms of a leading control character
  (`'$'\001''…`) hit it, so `dir/'` Tab Enter on a file `\001;touch X;#` ran `touch X` (found in review). On
  `main` the same rule let a file `!;touch X;#` run under `"` with `set -H` (reply `"\!"…`), gave the wrong
  argv for `';…` under `'`, and garbled `r'`/`r''x` after Tab, Backspace, Tab. `_toolr_quote` now starts such
  a reply with the quote doubled.
- **`$'…'` forms readline misreads.** Readline treats a `'` right after a `\` as escaped inside `$'…'`, so a
  run closing as `\\'` misplaced the word start (`$'b\\':z` Tab became `z`), and an earlier word holding `\'`
  in a run put its quoting out of step for the rest of the line. Both now offer nothing: the line up to the
  cursor goes through the same parser first, and a run closing on a `'` after a `\` is declined.

## Open questions (resolved)

1. Names differing at the control character itself: accepted. `files/nl\nx` and `files/nl\tz` give the
   common prefix `files/nl$'\`, pinned by `bash_path_common_prefix_of_grouped_replies_is_closed`.
2. `!` inside `$'…'` is not history-expanded under `set -H` on 5.1.16, 5.2.37 or 5.3.0, open or closed, so
   no `\041` is needed. Outside a run, `printf %q` already gives `\!`. Pinned by
   `bash_tab_keeps_a_bang_literal_in_ansi_c_quotes_end_to_end`.
3. `\u`/`\U` outside a UTF-8 locale: declined, with the locale probe above.
