# Safe Shell Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repo-derived strings can't execute in bash completion or inject control sequences into a terminal (F-01, F-13,
F-05).

**Architecture:** The bash completion script quotes candidates with `printf %q` instead of expanding them through
`compgen -W`. A shared `toolr_core::text` sanitiser strips control chars from descriptions and warnings, and the engine
drops candidates whose values contain them. The parser rejects bad names and values at build time with a new
`BuildError::InvalidNames`.

**Tech Stack:** Rust (toolr-core, toolr), bash 3.2+ / zsh / fish, `assert_cmd`.

**Spec:** [`specs/2026-10-30-safe-shell-output-design.md`](./2026-10-30-safe-shell-output-design.md)

## Global Constraints

- Control chars = `U+0000..=U+001F`, `U+007F`, `U+0080..=U+009F`.
- `bash.sh` must run on bash 3.2: no `mapfile`, no `${var@Q}`, no `declare -A`.
- Shell metacharacters in values stay legal at build time.
- No manifest schema change.
- Conventional Commits; the user-visible commit is `fix(complete): …`. No `Co-Authored-By`. Stage by path; never
  `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **Value with a space** (`Literal["dry run", "full"]`) must complete as one word, `dry\ run`. Task 1, test
   `bash_value_with_space_is_one_quoted_word`.
2. **Typed prefix containing a quote** (`--mode 'fa`): must not error out of the completion function or leak bash errors
   to the terminal. Task 1, test `bash_quoted_prefix_does_not_error` (asserts exit 0 and empty stderr; matching is a
   nice-to-have).
3. **Empty `cur`** (`--mode <TAB>`) must offer every value. Task 1, test `bash_empty_prefix_offers_all`.
4. **Third-party fragment with a control-char value**: the build doesn't own it, so the engine must still drop it at
   TAB. Task 2, test `engine_drops_control_char_values`.
5. **A `\n` in a description** must not create an extra candidate line. Task 2, test
   `description_newline_does_not_forge_candidates` (through the binary).

---

### Task 1: Bash script quotes instead of expanding

**Files:**

- Modify: `crates/toolr-core/src/complete/scripts/bash.sh`
- Create: `crates/toolr/tests/complete_shells.rs`

**Interfaces:** none produced.

**Test harness** (in `complete_shells.rs`, `#[cfg(unix)]`): build a fixture repo whose `tools/ci.py` declares:

```python
mode: Literal["fast", "dry run", "$(touch SENTINEL)", "`touch SENTINEL2`"] = "fast"
```

with `SENTINEL`/`SENTINEL2` replaced by absolute tempdir paths. Then run a bash process that:

- puts the built `toolr` first on `PATH` (directory of `assert_cmd::cargo::cargo_bin("toolr")`);
- defines a stub `_init_completion() { cur="${words[cword]}"; prev="${words[cword-1]}"; return 0; }`;
- sources the script from `toolr self completion print bash`;
- sets `words=(toolr ci lint --mode "<cur>")`, `cword=4`, calls `_toolr_complete`, and prints each `COMPREPLY` entry on
  its own line.

Run it with both `/bin/bash` (bash 3.2 on macOS) and `bash` from `PATH` when they differ. Pass the script text and
arguments through stdin or `-c` with positional parameters; never interpolate the fixture values into the command
string.

- [ ] **Step 1: Write the tests:**
    - `bash_completion_never_executes_values`: `cur=""`; neither sentinel exists afterwards.
    - `bash_value_is_shell_quoted`: `COMPREPLY` contains `\$\(touch\ <path>\)` (the `printf %q` form). Compute the
      expected string by running `printf %q` in the same bash; don't hard-code it.
    - `bash_value_with_space_is_one_quoted_word`: `cur="dr"` → exactly one entry, equal to `printf %q "dry run"`.
    - `bash_empty_prefix_offers_all`: `cur=""` → 4 entries.
    - `bash_quoted_prefix_does_not_error`: `cur="'fa"` → exit 0, empty stderr.
    - `zsh_completion_never_executes_values` and `fish_completion_never_executes_values`: skip when the shell isn't
      installed. Use `zsh -f` with `zpty`, and `fish --no-config` with `complete -C "toolr ci lint --mode "`. Assert the
      sentinels are absent. If zsh `zpty` proves unworkable, replace the zsh test with a source-reading check that the
      script uses `compadd -a` without `-Q`, and say so in the commit body.

- [ ] **Step 2: Run them and confirm the bash tests fail.** `cargo test -p toolr --test complete_shells` →
      `bash_completion_never_executes_values` fails (sentinel created).

- [ ] **Step 3: Rewrite the tail of `_toolr_complete`** per spec layer A. Keep the existing header comments; replace the
      `shellcheck disable=SC2207` line and its comment.

- [ ] **Step 4: Run.** `cargo test -p toolr --test complete_shells --test complete_smoke` → PASS. Run
      `shellcheck crates/toolr-core/src/complete/scripts/bash.sh` if `prek` doesn't already.

- [ ] **Step 5: Commit.** `fix(complete): quote bash candidates instead of expanding them`

### Task 2: Shared sanitiser in engine and warnings

**Files:**

- Create: `crates/toolr-core/src/text.rs` (+ `pub mod text;` in `crates/toolr-core/src/lib.rs`)
- Modify: `crates/toolr-core/src/complete/engine.rs` (`Candidate::new` ~19-28; the point where candidates are collected
  into the returned vector)
- Modify: `crates/toolr/src/main.rs` (~42-44)
- Test: unit tests in `text.rs` and `engine.rs`; integration tests in `crates/toolr/tests/complete_smoke.rs` and
  `crates/toolr/tests/plugin_warnings.rs`

**Interfaces (produced):**

- `pub fn has_control_chars(s: &str) -> bool`

- `pub fn strip_control_chars(s: &str) -> String`

- [ ] **Step 1: Write the tests:**
    - `text.rs`: `strip_removes_esc_and_c1` (`"a\x1b]0;x\x07b\u{9b}c"` → no char in the control ranges remains);
      `strip_collapses_spaces` (`"a\n\n b"` → `"a b"`); `has_control_chars` true for `"\x7f"`, false for `"dry run"` and
      `"$(x)"`.
    - `engine.rs`: `engine_drops_control_char_values`: a manifest `Argument` with
      `allowed_values = ["ok", "bad\x1b[2J"]` → candidates are only `ok`. `candidate_description_is_stripped`:
      `Candidate::new("v", "a\x1b]0;t\x07b").description` has no control chars.
    - `complete_smoke.rs`: `description_newline_does_not_forge_candidates`: write a manifest fragment (or a committed
      `tools/.toolr-manifest.json` matching the current hash) whose group description contains `"x\nevil\tdesc"` →
      `__complete` output has exactly one line per group.
    - `plugin_warnings.rs`: `plugin_warning_control_chars_are_stripped`: a warning message containing
      `\x1b]52;c;ZXZpbA==\x07` → stderr has no `\x1b`.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p toolr-core --lib text:: complete::` and
      `cargo test -p toolr --test complete_smoke --test plugin_warnings`.

- [ ] **Step 3: Implement `text.rs`,** switch `Candidate::new` to `strip_control_chars`, filter value-unsafe candidates
      in the engine, and sanitise warnings in `main.rs`.

- [ ] **Step 4: Run** the same commands → PASS.

- [ ] **Step 5: Commit.** `fix(complete): strip control characters from completion and warnings`

### Task 3: Build-time rejection

**Files:**

- Create: `crates/toolr-core/src/parser/names.rs` (+ `mod names;` in `crates/toolr-core/src/parser/mod.rs`)
- Modify: `crates/toolr-core/src/parser/build.rs`: add `BuildError::InvalidNames(Vec<InvalidName>)` with a
  `format_invalid_names` formatter next to the other variants (~249-262); call the validator right after
  `validate_positional_arity` (~179-182)
- Test: `crates/toolr-core/src/parser/build.rs` `mod tests`

**Interfaces (produced):**

- `InvalidName`:

    ```rust
    pub struct InvalidName {
        pub module: String,
        pub owner: String,
        pub item: NameItem,
        pub value: String,
        pub reason: &'static str,
    }
    ```

- `pub enum NameItem { Group, Command, Flag, Choice }`

- `pub(crate) fn validate_names(groups: &[Group], commands: &[Command]) -> Vec<InvalidName>`. Uses
  `crate::text::has_control_chars` and `char::is_whitespace`. Escape control chars in `value` with
  `char::escape_default` for display.

- [ ] **Step 1: Write the tests,** building a tree with the existing `tmp` + `write` helpers and asserting on
  `BuildError::InvalidNames`:
    - `group_name_with_space_rejected`: `command_group("my group", "T", "D")`.
    - `command_override_name_with_newline_rejected`: `@group.command(name="a\nb")` (or the override form `commands.rs`
      recognises).
    - `literal_value_with_esc_rejected`: `Literal["ok", "bad\x1b[2J"]`; the message shows `\u{1b}`.
    - `literal_value_with_space_and_dollar_allowed`: `Literal["dry run", "$(x)"]` builds.
    - `argparse_flag_with_space_rejected`: an argparse source with `add_argument("--bad flag")`, if the scanner accepts
      that literal; otherwise note in the commit that the scanner already rejects it.
    - `all_invalid_names_reported_together`: two bad names → `len() == 2`.

- [ ] **Step 2: Run them and confirm they fail.** `cargo test -p toolr-core --lib parser::build::tests`.

- [ ] **Step 3: Implement `names.rs`, the variant, the formatter and the call.**

- [ ] **Step 4: Run.** `cargo test -p toolr-core` → PASS.

- [ ] **Step 5: Commit.** `fix(parser): reject control characters and whitespace in names`

### Task 4: Release notes, findings, verification, archive

- [ ] **Step 1:** Add the spec's `UNRELEASED.md` text verbatim.
- [ ] **Step 2:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
- [ ] **Step 3:** Manual check on macOS `/bin/bash` 3.2 with bash-completion if installed: source
      `toolr self completion print bash` and TAB through a fixture with a `$(touch …)` value → no file is created; the
      inserted text is quoted.
- [ ] **Step 4:**
      `git mv specs/2026-10-30-safe-shell-output-design.md specs/2026-10-30-safe-shell-output-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link. Commit `docs(specs): archive safe shell output design and plan`.
