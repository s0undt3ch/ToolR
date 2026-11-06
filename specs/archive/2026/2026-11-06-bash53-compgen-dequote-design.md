# Bash path completion: quoted and escaped words: design

Audit follow-up FU-24 (remainder). Pre-existing on `main` at `f65b07a2`.

## Problem

Bash path completion (`_toolr_complete_paths` in `crates/toolr-core/src/complete/scripts/bash.sh`) fails on
words that hold a backslash escape or a quote. The finding was filed as "bash 5.3 `compgen -f` dequotes its word",
but reproduction shows the interactive bug is the same on bash 5.1, 5.2 and 5.3. Only the `bash -c` test harness
sees a 5.3-only difference, which is why it looked version-specific.

### Reproduction

Tools: bash 5.1.16, 5.2.37 and 5.3.0 from `mise` (`conda:bash@<version>`; `mise ls-remote conda:bash` offers nothing
older than 5.1.16). Bash 4.4 could not be built locally, so the floor is untested here (see "Decisions"). The system
bash is 3.2, which the script refuses.

Harness: the dev `toolr` binary, a scratch project with `--cfg: pathlib.Path` and `--out: DirectoryPath`, and an
interactive bash on a pty (`--rcfile` sources the script, `INPUTRC=/dev/null`, `TERM=dumb`). The harness types the
line, presses Tab, then a `bind -x` key prints `READLINE_LINE`. A wrapper around `_toolr_complete` logged `COMP_WORDS`
and `COMPREPLY`. Files under `files/`: `back\slash`, `m\n` (backslash, `n`), `it's`, `x'y`, `x'z`, `q"d`,
`with space`, `zeta`, `sub/`.

Every row below gave the same result on 5.1.16, 5.2.37 and 5.3.0.

| Typed after `toolr ci lint --cfg` | Line after Tab                | `COMPREPLY`             | Verdict              |
| --------------------------------- | ----------------------------- | ----------------------- | -------------------- |
| `files/z`                         | `files/zeta`                  | `files/zeta`            | ok                   |
| `files/with\ s`                   | `files/with\ space`           | `files/with\ space`     | ok                   |
| `files/back\\`                    | `files/back\\slash`           | `files/back\\slash`     | ok                   |
| `files/back\\s`                   | unchanged                     | empty                   | **nothing offered**  |
| `files/m\\n`                      | unchanged                     | empty                   | **nothing offered**  |
| `files/x\'y`                      | unchanged                     | empty                   | **nothing offered**  |
| `files/x\'`                       | unchanged                     | `files/x\'y files/x\'z` | ok (two matches)     |
| `"files/wi`                       | `"files/with\ space"`         | `files/with\ space`     | **mangled**          |
| `'files/wi`                       | `'files/with\ space'`         | `files/with\ space`     | **mangled**          |
| `"files/it'`                      | `"files/it\'s"`               | `files/it\'s`           | **mangled**          |
| `'files/q"`                       | `'files/q\"d'`                | `files/q\"d`            | **mangled**          |
| `files/"wi`                       | `files/"files/with\ space"`   | `files/with\ space`     | **mangled**          |
| `"~/files/z`                      | `"\~/files/zeta"`             | `\~/files/zeta`         | **wrong file**       |
| `"$HOME/fi`                       | unchanged                     | empty                   | nothing offered      |
| `$HOME/files/z`                   | `$HOME/files/zeta`            | `$HOME/files/zeta`      | ok                   |
| `~/files/z`                       | `~/files/zeta`                | `~/files/zeta`          | ok                   |
| `--cfg=files/back\\s` (one word)  | unchanged                     | empty                   | **nothing offered**  |

"Mangled" means the inserted text parses to a different path: inside `"…"` or `'…'`, the `\` before the space stays a literal
backslash, so `"files/with\ space"` names a file that does not exist. `"~/…"` is a literal directory named
`~`, not the home directory.

The reported second symptom, `"$HOME/fi<TAB>` inserting `"\$HOME/files"`, no longer reproduces. Since #681 the
`$`-guard drops the word: `_toolr_var_re` is anchored at `^`, so the leading `"` hides the `$NAME/` prefix, the word
still holds a `$`, and the guard returns nothing. Native bash completes it.

The `compgen` builtin itself, run with `bash -c` (no readline, as the `Drive` test harness does):

| Word given to `compgen -f --` | 5.1.16 and 5.2.37 | 5.3.0          |
| ----------------------------- | ----------------- | -------------- |
| `files/back\s`                | `files/back\slash` | same          |
| `files/x'`                    | `x'y`, `x'z`      | same           |
| `"files/x'`                   | nothing           | `"files/x'y`, `"files/x'z` |
| `"files/p`, `'files/p`        | nothing           | `"files/plain`, `'files/plain` |
| `$HOME/f`                     | nothing           | `$HOME/files`  |

So 5.3 also dequotes and expands outside a live completion; before 5.3 that only happens inside one.

## Root cause

Inside a live completion, readline records whether the word held a quote or backslash. `compgen -f` and `compgen -d`
then run readline's filename dequoting on their argument before matching: backslashes are dropped and `'`/`"` open
and close quotes. The script hands `compgen` `_toolr_raw_cur`, which it has already unescaped, so the word is dequoted
twice: `files/back\\s` becomes `files/back\s`, then `files/backs`. Bash 5.3 applies the same dequoting (and `$`
expansion) outside readline too, which is why only 5.3 showed it under the `bash -c` harness.

A second, separate defect: the word parser treats `"` and `'` as ordinary characters. Readline, though, replaces only
the text after an open quote and closes the quote itself on a single match, so the `printf %q` replies land inside the
quotes and change meaning.

## Design

Two changes in `bash.sh`. Both apply to every supported bash version, with no version checks.

### 1. List paths with a glob, not `compgen`

`_toolr_complete_paths` stops passing user text to `compgen`. A new `_toolr_list_paths MODE WORD`, run in the
process substitution that feeds the reply loop, splits the typed path into a directory part (up to and including the
last `/`) and a leaf prefix, and lists `"$WORD"*`:

```bash
set +f
unset GLOBIGNORE
shopt -s nullglob dotglob
shopt -u failglob nocaseglob
for _toolr_name in "$_toolr_word"*; do
    printf '%s\0' "$_toolr_name"
done
```

- The quoted part of a pattern is matched literally, so nothing is dequoted, expanded or run. Only the trailing `*`
  globs. The user's `failglob`, `nocaseglob` and `GLOBIGNORE` stay as they were, since the subshell owns the options.
- Names are NUL-separated and read with `read -r -d ''`, so a name holding a newline also lists correctly.
- `.` and `..` are dropped from the glob output (bash 5.2's `globskipdots` and older bash disagree on them) and added
  back when the leaf starts with `.` and is a prefix of them, as `compgen -f` does today.
- `-d` mode keeps only names where `[[ -d $name ]]`. The directory marking (`/` and `compopt -o nospace`) is unchanged.
- The `$`/backtick guard from #681 stays exactly where it is and still returns early. It no longer protects a
  `compgen` call, but keeping it means no behaviour change for those words in this fix.

`~` and `~user` need the home directory, which `compgen` used to resolve. A bare `~/` with `HOME` set is handled
like a typed `$HOME/`: the glob lists under the value of `HOME`, and each name is put back as `~/` plus the quoted
rest. `~user`, and `~` with `HOME` unset, keep today's `compgen` path, including its `compgen -d` probe for
directories, but only when the word matches `^~([A-Za-z0-9._][A-Za-z0-9._-]*)?(/|$)` and holds no `\`, `'` or `"`.
Then readline's dequoting has nothing to change. Any other `~user` word offers nothing. The script has no `eval`.
The `./` hack for a `$NAME` value starting with `~` goes away: a quoted pattern never tilde-expands.

### 2. Parse quotes in the cursor word and quote replies for the open quote

The unescape loop in `_toolr_complete` becomes a small shell-word parser over `_toolr_cur`:

- Outside quotes: `\x` is `x`; `'` opens a single-quoted run; `"` opens a double-quoted run. Word-break handling
  (`_toolr_raw_prefix`, the `$`/`@` special-prefix rule) is unchanged and applies only here.
- In `'…'`: everything is literal until the closing `'`.
- In `"…"`: `\` escapes only `$`, `` ` ``, `"` and `\`; other characters are literal.
- If the word ends inside an open quote, readline replaces only the text after that quote. So `_toolr_raw_prefix`
  becomes the raw text before the opening quote, and `_toolr_qchar` records `'` or `"`.
- `_toolr_tilde_ok` is set only when the word starts with an unquoted `~`, and `_toolr_var_ok` only when it starts
  with an unescaped `$`, bare or right after an opening `"`. `"~/x` is then a literal `~` directory, and `'$HOME/x`
  holds a literal `$` that the guard drops.

`_toolr_quote` takes the quote context:

- none: `printf %q` as today (with the bash 4 `~` fix);
- `"`: backslash-escape `\`, `"`, `$` and `` ` ``;
- `'`: replace each `'` with `'\''`. Readline then appends the closing `'`, giving `'it'\''s'`.

Choice values go through the same `_toolr_quote`, so `--mode "dry r<TAB>` inserts `"dry run"` instead of
`"dry\ run"`. A `$NAME` typed after `"` is kept bare in the reply, so `"$HOME/fi<TAB>` inserts `"$HOME/files/"`.

`_toolr_args` hands `toolr __complete` the dequoted word, so the engine matches `dry r` rather than `"dry r`.

### Alternatives rejected

- Gate on `BASH_VERSINFO` and pre-escape the word for 5.3: the dequoting also happens on 5.1 and 5.2 inside readline.
- Re-escape the word so a second dequote restores it: whether `compgen` dequotes depends on what readline saw in the
  line, which the function cannot observe.
- `compgen -G` with a pattern: it goes through the same readline dequoting.
- `compopt -o filenames` and let readline quote: readline would then also quote choice values and the `--opt=`
  prefix, and the colon and `$NAME` handling from #673/#681 would need redoing.
- A completion library (clap_complete, carapace): out of scope by policy.

### Not changed

zsh and fish already use native file completion. `completion-ignore-case`, `FIGNORE` and `mark-symlinked-directories`
are not mirrored by the glob; they were not honoured reliably through `compgen` from a `-F` function either.

## Testing

The interactive defect only shows on a live readline, so the regression tests that matter are pty tests. `Drive`
(`bash -c`) tests can't see the double dequote on 5.1 or 5.2, so on Linux CI they would pass with the bug still
there. They still pin the reply text for each quote context.

Versions: every `bashes()` entry. CI has bash 5.2.21 (ubuntu-latest) and Homebrew bash 5.3.x (macOS). Locally,
`mise x conda:bash@5.1.16`, `@5.2.37` and `@5.3` were used; `bashes()` doesn't search mise installs, so run the tests
with each one first on `PATH`.

`crates/toolr/tests/complete_shells.rs`:

- `PATH_FILES` gains `files/it's.txt` and `files/q"d.txt`, appended so existing `quoted[i]` indexes stay put.
- New pty test `bash_tab_completes_quoted_and_escaped_paths_end_to_end`, sharing the rcfile and `run_on_pty` setup with
  `bash_tab_completes_words_with_a_colon_end_to_end` through a small helper. It checks the `LINE:` that the `toolr()`
  shim prints after shell parsing.
- New `Drive` tests for the reply text in each quote context, and for `"~/` and `'$HOME/`.
- Existing tests that must keep passing unchanged: `bash_path_keeps_a_typed_leading_tilde`,
  `bash_path_keeps_a_typed_leading_variable`, `bash_path_variable_value_is_a_literal_directory`,
  `bash_path_never_runs_a_typed_substitution`, `bash_attached_path_never_runs_a_typed_substitution`,
  `bash_path_marks_a_tilde_user_directory`, `bash_colon_after_a_typed_tilde_marks_the_directory`,
  `bash_colon_in_a_file_name_completes_after_the_colon`, `bash_completion_survives_errexit`,
  `bash_path_completion_scales_linearly_with_the_listing`, `bash_attached_path_completes_the_part_after_the_equals`,
  `old_bash_script_offers_nothing_on_a_path_slot` and the colon end-to-end test.

## Review fixes

Three review rounds on the PR changed the shipped script from the design above:

- **Listing.** `_toolr_list_paths` holds the glob. It runs `set +f`, then `unset GLOBIGNORE` before
  `shopt -s nullglob dotglob`: unsetting `GLOBIGNORE` turns `dotglob` off, which had hidden every dotfile. Dotfiles are
  now listed with no `.` typed, as `compgen -f` did, whatever the user's `GLOBIGNORE`.
- **`.` and `..`** are added back only under a directory that exists (`[[ -d ${dir:-.} ]]`), so `nosuch/.` offers
  nothing.
- **Bare prefixes.** A typed `~/` or `$NAME/` is put back as typed, slash included, and only the rest is quoted. A `~`
  followed by `$'…'` (a name holding a newline) would not tilde-expand.
- **`~user`.** `_toolr_list_user_paths` runs `compgen` for words matching `^~([A-Za-z0-9._][A-Za-z0-9._-]*)?(/|$)` that
  hold no backslash or quote. `compgen` prints one name per line, so lines that don't start with the typed `~user`
  (the tail of a name holding a newline) are dropped, for the listing and the directory probe alike.
- **`!` inside `"…"`.** Interactive bash history-expands `!` even inside double quotes, and `\!` there keeps its
  backslash. With `set -H` on, each `!` in a reply becomes `"\!"` (close, escape, reopen). Readline adds no closing
  quote to a reply that ends in a quote, so a name ending in `!` gets a trailing `""`. For a directory the reply ends in
  `/` instead and readline closes the quote itself, so that `""` is stripped before the `/`.
- **Structure.** The word parser is `_toolr_parse_word`, and `_toolr_complete_paths MODE OPT_PREFIX` derives the path
  prefix and the `--opt=` lead itself.

## Release note

Yes. User-visible fix: `fix(complete)` with a queued note.

## Decisions (2026-11-06)

1. Bash 4.4 stays the documented floor. It is tested elsewhere; this change is checked locally on 5.1.16, 5.2.37 and
   5.3 from mise, and uses no bash feature newer than 4.4. The CI matrix is unchanged here.
2. `~user` keeps `compgen`, after the name is validated, and only when the rest holds no `\`, `'` or `"`. No `eval`.
3. The `$`/backtick guard stays as it is.
