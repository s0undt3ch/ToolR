# Show names next to descriptions in zsh listings (FU-46)

Status: implemented (PR #721). Audit follow-up FU-46, found by reading `zsh.zsh` after #713 (FU-34) merged.

## Problem

`toolr __complete` prints one `value<TAB>description` line per candidate (`crates/toolr/src/dispatch.rs`).
`crates/toolr-core/src/complete/scripts/zsh.zsh` splits the lines into `values` and `descriptions`. When
20 or fewer candidates match, it calls `compadd "${expl[@]}" -d descriptions -a values`. `compadd -d`
replaces the text shown in the listing, so the list holds only descriptions. The names are gone. Past 20
matches the script uses the plain grid, which shows names and no descriptions.

## Reproduction

zsh 5.9 on a pty, a stand-in `_demo` completer with values `lint`, `pos`, `dry run` and one description each,
the same `compadd` call as `zsh.zsh`, then Tab after `demo `:

```text
A positional and a flag.  Dry description           Lint things.
```

No command name appears. The listing is also sorted by name while the labels aren't, so the reader can't map
a description back to a command.

## Constraints kept

- `matcher-list` (FU-34, #713): the script still calls `_description` and passes `expl` to every `compadd`,
  and `-O matched` still counts matches before choosing the layout.
- Quoted and escaped partials (FU-33): no `-Q`, so zsh re-quotes each match; values go through
  `-a values` untouched.
- First Tab on autoload (`loadautofunc` guard at the end of the script): untouched.
- Attached `--opt=` values: `compset -P 1 '*='` still runs before any `compadd`.
- The 20-match threshold and the large-list grid stay as they are.

## Options

| Option | Result | Verdict |
| --- | --- | --- |
| `_describe -t toolr toolr pairs` with `name:desc` pairs | Aligned `name -- desc`, sorted. Probed: works. Names need `\:` and probably `\\` escaping, and `_describe` has no `-O`, so the 20-match count still needs a separate `compadd -O` pass over the raw values. | Rejected. A value-escaping step is new risk on every candidate, and it replaces the `compadd` call the quoted-partials work depends on. |
| `compadd -d` with `name -- desc` display strings, no `-l` | Probed: names appear, but zsh arrays the strings in columns, so long descriptions share a row with other entries. | Rejected. Names come back, but the layout is hard to read. |
| `compadd -l -d display -a values`, `display` built as `name -- desc` with the name padded | Probed: one row per match, aligned, names and descriptions together. Matches narrowed by `matcher-list` (`l:|=* r:|=*`, `n`) keep their own descriptions. The `descriptions` header from `zstyle ... format` still reads `HDR<toolr>`. | Recommended. |

`-l` is documented in zshcompwid as listing the `-d` display strings one per line; the probe confirms it.
The documentation text itself was not available offline, so the wording is from memory.

## Design

Keep the structure of `zsh.zsh`. In the 20-or-fewer branch only:

1. Build `descriptions` as today (drop an empty trailing description, strip up to the first tab).
2. Find the widest of the matched values with `${(m)#...}` semantics, so wide characters count by display
   width. A plain `${#v}` would misalign CJK names.
3. Build `display` in one loop of at most 20 entries: `${(mr:w:)value} -- ${description}`, or just
   `${value}` when the description is empty, so a missing description still shows the name.
4. Call `compadd "${expl[@]}" -l -d display -a values`.

The width comes from the matched candidates only, so a long candidate the typed word has already excluded
can't pad the visible rows or wrap them on a narrow terminal; the column may shift as the word narrows. Values are never put through `eval` or expansion; they only fill `display`,
which `compadd` prints as text. A value or description holding `%` is shown literally because `-d`
strings are not prompt-expanded (unverified; the plan's live test covers it).

The large-list branch is unchanged.

## The tab case

A value containing a literal tab would split wrongly at `${lines%%$'\t'*}`. That can't reach the script:
`complete::engine` drops every candidate whose value has a control or invisible character
(`has_unsafe_name_chars`, `engine.rs`), and `Candidate::new` collapses control characters, tabs included, in
descriptions to a space (`strip_control_chars`). The static root flags merged in `dispatch.rs` are fixed
strings. Decision: accepted as already prevented, no script change. The plan adds one test that pins the
engine behaviour at the wire, so a later change to either filter fails loudly.

## Testing

- A live pty test beside `zsh_tab_lists_descriptions_when_the_matches_are_few`
  (`crates/toolr/tests/complete_shells.rs`) asserts that a row holds `cmd03` and `Summary number 03.` together.
  The existing live test only looks for the description text, so it passes today. The new one fails on `main`
  because the listing holds the description alone.
- The stub-driver tests that expect `COMPADD:-d descriptions -a values` change to the new call and read the
  `display` array.
- The `matcher-list` live test and the header test stay green unchanged.

## Out of scope

The fish and bash scripts, the 20-match threshold, and colouring the names (`list-colors`).
