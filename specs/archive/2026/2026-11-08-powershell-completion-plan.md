# PowerShell Tab Completion — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a PowerShell completion script (Windows PowerShell 5.1 and pwsh 7) that delegates to
`toolr __complete`, plus `toolr self completion print|install powershell` (alias `pwsh`) and a
`$HOME`-independent install.

**Architecture:** A new embedded `powershell.ps1` registers a native argument completer. It maps
the command AST to engine words, starts `toolr __complete` through `System.Diagnostics.Process`
with MSVC-escaped arguments (one path for every edition), and quotes replies so PowerShell's parser
reads them back as the exact value. Rust gains a `Shell::PowerShell` variant, an install path under
toolr's data directory, a profile hint, and a home-directory fallback. The `__complete` protocol is
unchanged.

**Tech Stack:** Rust (toolr-core `full`, `dirs`), PowerShell 5.1-compatible script, real pwsh and
powershell.exe in tests, `mise exec powershell@<version>` for pwsh.

**Spec:** `specs/2026-11-08-powershell-completion-design.md` — read it in full; this plan does not
repeat its rules (word mapping table, quoting rules, exit-code handling, install hint text).

## Global Constraints

- The script is pure ASCII and parses on PowerShell 3+; it runs only 5.1-compatible syntax: no
  ternary, `??`, `&&`/`||` chains, `class`, `using`, `$IsWindows`, `$IsCoreCLR`, `-AsHashtable`,
  `Join-String`, `ForEach-Object -Parallel`.
- The script never evaluates user text: no `Invoke-Expression`, no `& $word`, no
  `ExpandString`. AST `.Value` only.
- Never pass engine words through `& toolr …`; always `System.Diagnostics.Process` with
  `_toolr_msvc_arg`.
- `install.rs` stays free of `dirs`; data and home directories are injected.
- `$HOME` keeps priority over `dirs::home_dir()`; Unix install paths for bash, zsh and fish are
  byte-identical to today's.
- No change to `toolr __complete`'s arguments, output format or exit codes.
- 99% patch coverage (`cargo xtask patch-coverage --base main`). The script is outside coverage;
  the Rust arms carry the metric. Close gaps with real tests.
- Format touched Rust files only (`rustfmt --edition 2024 <file>`), never whole-crate `cargo fmt`.
- Conventional Commits; no `Co-Authored-By` trailer; British English in prose and comments.
- Tests that need pwsh are `#[ignore]`d; run with `--ignored`, a missing shell panics.

## Review Focus

1. **Inertness.** No fixture value or typed partial (`$(New-Item …)`, `@(…)`, backticks, `;`,
   `--%`, smart quotes) ever creates a sentinel, on 5.1 or 7. Test in Task 3.
2. **Native-argument round-trip.** Empty words, `"`, trailing backslashes, spaces and non-ASCII
   reach the engine byte-exact on 5.1 and 7, including under `$PSNativeCommandArgumentPassing =
   'Legacy'`. Test in Task 3.
3. **Reply round-trip.** Every reply re-parses through PowerShell's parser to exactly the full value
   for no-quote, `'` and `"` partials, including attached `--opt=` values. Test in Task 3.
4. **Hostile sessions.** `$ErrorActionPreference = 'Stop'`, `Set-StrictMode -Version Latest`,
   `$PSNativeCommandUseErrorActionPreference = $true`, a non-FileSystem location, and a user
   function named `toolr` neither error at the prompt nor recurse. Test in Task 3.
5. **Home fallback.** A stock Windows account without `HOME` installs; Unix paths are unchanged.
   Test in Task 2.

---

### Task 1: `Shell::PowerShell`, CLI values and a stub script

**Model:** sonnet — mechanical enum and clap wiring with clear tests. **Reviewer:** sonnet.

**Files:**

- Modify: `crates/toolr-core/src/complete/scripts.rs`
- Create: `crates/toolr-core/src/complete/scripts/powershell.ps1` (stub: version guard and an empty
  `_toolr_complete`, filled in Task 3)
- Modify: `crates/toolr-core/src/complete/mod.rs` (doc comment lists four shells)
- Modify: `crates/toolr/src/cli.rs` (`build_self_completion_subcommand`)
- Modify: `crates/toolr/src/builtin_completions.rs` (test expectation)

**Interfaces:**

```rust
pub enum Shell { Bash, Zsh, Fish, PowerShell }
// Display: "powershell". FromStr: "powershell" | "pwsh" -> PowerShell.
// Error: "unsupported shell: {other} (expected bash, zsh, fish, or powershell)"
const POWERSHELL_SCRIPT: &str = include_str!("scripts/powershell.ps1");
```

clap: both `shell` args use
`PossibleValuesParser::new([PossibleValue::new("bash"), PossibleValue::new("zsh"),
PossibleValue::new("fish"), PossibleValue::new("powershell").alias("pwsh")])`. Check that
`get_one::<String>("shell")` returns `"pwsh"` for the alias (clap keeps the typed value), which
`Shell::from_str` then maps; pin it in a test.

- [ ] **Step 1: Write the failing tests** in `scripts.rs`'s test module:
    - `shell_display_matches_clap_value_strings` gains `Shell::PowerShell` → `"powershell"`.
    - `shell_from_str_accepts_every_supported_name` (renamed from `…the_three…`): adds
      `"powershell"` and `"pwsh"`.
    - `shell_from_str_rejects_unknown_shell_with_helpful_message`: input becomes `"nushell"`;
      asserts the message contains all four names.
    - `completion_script_is_non_empty_for_every_shell`: loop includes `Shell::PowerShell`.
    - `powershell_script_is_ascii`: `completion_script(Shell::PowerShell).is_ascii()`.
- [ ] **Step 2:** `cargo test -p toolr-core complete::scripts` — fails.
- [ ] **Step 3:** Implement the variant, the stub script (the spec's `if`/`else` guard with
      `Register-ArgumentCompleter` in the `else`), and the clap values.
- [ ] **Step 4:** In `builtin_completions.rs`, the shell-values test expects
      `["bash", "fish", "powershell", "zsh"]`. In `crates/toolr/tests/cli_smoke.rs` add
      `completion_print_pwsh_matches_powershell` (stdout of `print pwsh` == `print powershell`,
      both succeed) and `completion_print_rejects_unknown_shell` if not already covered.
- [ ] **Step 5:** `cargo test -p toolr-core complete::scripts` and
      `cargo test -p toolr builtin_completions` and `cargo test -p toolr --test cli_smoke` pass.
- [ ] **Step 6:** Commit `feat(completion): add the powershell shell value`.

### Task 2: Install path, profile hint and home fallback

**Model:** sonnet — small Rust changes with injected inputs. **Reviewer:** opus — cross-platform
path behaviour (Review Focus 5).

**Files:**

- Modify: `crates/toolr-core/src/complete/install.rs`
- Modify: `crates/toolr-core/src/complete/mod.rs` (re-export `powershell_quote`)
- Modify: `crates/toolr/src/dispatch.rs`
- Modify: `crates/toolr/Cargo.toml` (`dirs.workspace = true`)
- Create: `crates/toolr/tests/completion_install_powershell.rs`

**Interfaces:**

```rust
// install.rs
pub struct InstallOptions {
    // existing fields unchanged, plus:
    /// Platform data directory (`dirs::data_dir()`), used when `xdg_data_home` is unset.
    pub data_dir: Option<PathBuf>,
}
pub fn install_path_for(
    shell: Shell,
    xdg_override: Option<&Path>,
    home: &Path,
    data_dir: Option<&Path>,
) -> Result<PathBuf>;
// PowerShell: xdg_override.or(data_dir) .join("toolr/completions/toolr.ps1");
// error "no data directory; set XDG_DATA_HOME" when both are None.
/// Quote `s` as one PowerShell single-quoted word: `'` and U+2018..U+201B doubled.
pub fn powershell_quote(s: &str) -> String;

// dispatch.rs
fn resolve_home(env_home: Option<PathBuf>, fallback: impl FnOnce() -> Option<PathBuf>) -> anyhow::Result<PathBuf>;
fn dirs_home() -> anyhow::Result<PathBuf>; // resolve_home(non_empty_env_path("HOME"), dirs::home_dir)
fn powershell_profile_hint(path: &Path, windows: bool) -> String;
fn print_powershell_profile_hint(shell: CompletionShell, path: &Path); // no-op for other shells
```

`install_script` passes `opts.xdg_data_home` and `opts.data_dir` for PowerShell. Every existing
caller of `install_path_for` gains `None` for `data_dir`. `run_completion_install` sets
`data_dir: dirs::data_dir()` and calls `print_powershell_profile_hint` beside
`print_bash_source_hint`, after `Wrote` and `AlreadyInstalled`, unless `--quiet`.
`powershell_profile_hint` takes `windows` as a parameter (callers pass `cfg!(windows)`) so both
texts are unit-tested on every host.

- [ ] **Step 1: Write the failing unit tests.**
    - `install.rs`: `powershell_install_path_uses_xdg_data_home`,
      `powershell_install_path_falls_back_to_data_dir`,
      `powershell_install_path_without_any_data_dir_errors`,
      `other_shells_ignore_data_dir` (bash/zsh/fish paths unchanged when `data_dir` is `Some`),
      `powershell_quote_plain_path`, `powershell_quote_path_with_space`,
      `powershell_quote_doubles_straight_and_smart_single_quotes`.
    - `dispatch.rs` test module: `resolve_home_prefers_env`,
      `resolve_home_falls_back_when_env_missing`, `resolve_home_errors_when_both_missing`
      (message contains `cannot pick install path`), `powershell_profile_hint_quotes_the_path`,
      `powershell_profile_hint_on_windows_names_execution_policy`,
      `powershell_profile_hint_without_utf8_path_names_no_path` (`#[cfg(unix)]`, built with
      `OsStr::from_bytes`).
- [ ] **Step 2:** Run them — fail.
- [ ] **Step 3:** Implement. Update the comment near `dispatch.rs:932` to describe the new helper.
- [ ] **Step 4: Binary tests** in `completion_install_powershell.rs`, modelled on
      `completion_install_hint.rs` (tempdir `HOME`, `XDG_DATA_HOME`):
    - `install_powershell_writes_script_under_xdg_data_home` — file at
      `<data>/toolr/completions/toolr.ps1` equals `print powershell` output.
    - `install_powershell_prints_profile_hint` — stdout contains `. '<path>'` and `$PROFILE`.
    - `install_pwsh_alias_installs_the_same_file`.
    - `install_powershell_twice_is_already_up_to_date`.
    - `install_powershell_refuses_a_changed_file_without_force` and
      `install_powershell_force_replaces_a_changed_file`.
    - `install_powershell_quiet_drops_the_hint`.
    - `install_without_home_uses_platform_home` (`#[cfg(windows)]`): `env_remove("HOME")`,
      `XDG_DATA_HOME` set; succeeds. On Unix the fallback is covered by the unit tests only, since
      `dirs::home_dir()` would resolve the real home.
- [ ] **Step 5:** `cargo test -p toolr-core complete::install`, `cargo test -p toolr dispatch`,
      `cargo test -p toolr --test completion_install_powershell` pass.
- [ ] **Step 6:** Commit `feat(completion): install the powershell script and resolve home without $HOME`.

### Task 3: The PowerShell script and real-shell tests

**Model:** opus — quoting, native-argument passing and inertness across two runtimes.
**Reviewer:** opus — Review Focus 1 to 4.

**Files:**

- Modify: `crates/toolr-core/src/complete/scripts/powershell.ps1`
- Create: `crates/toolr/tests/complete_powershell.rs`
- Modify: `CONTRIBUTING.md` (the `--ignored` command for the PowerShell tests)

**Script structure** (function names fixed; all `_toolr_`-prefixed):

```powershell
# header: what it is, install line, known limits (spec "Known limits")
if ($PSVersionTable.PSVersion -lt [version]'5.1') {
    Write-Warning 'toolr completion needs PowerShell 5.1 or newer'
} else {
    function global:_toolr_msvc_arg([string]$Word) { ... }        # spec "Calling the engine"
    function global:_toolr_word_value($Ast) { ... }              # spec word table; $null = offer nothing
    function global:_toolr_quote([string]$Value, [string]$Open) { ... }  # spec "Quoting replies"
    function global:_toolr_exe { ... }                           # Get-Command, .exe on Windows
    function global:_toolr_run([string]$Exe, [string]$Cwd, [string[]]$Words) { ... }  # -> @{ Code; Lines }
    function global:_toolr_complete($wordToComplete, $commandAst, $cursorPosition) { ... }
    Register-ArgumentCompleter -Native -CommandName 'toolr', 'toolr.exe' -ScriptBlock ${function:_toolr_complete}
}
```

**Test harness** in `complete_powershell.rs`:

```rust
fn powershells() -> Vec<PathBuf>;            // pwsh on PATH; + powershell.exe on Windows; missing panics
struct Fixture { tmp: TempDir, sentinels: Vec<PathBuf> }
fn fixture() -> Fixture;                     // spec "Real-shell tests" values and files
struct TabResult { index: usize, length: usize, matches: Vec<Match> }
struct Match { text: String, list: String, tip: String, kind: String }
fn tab(ps: &Path, fx: &Fixture, prelude: &str, line: &str, cursor: Option<usize>) -> TabResult;
fn parse_value(ps: &Path, reply: &str) -> Option<String>; // `x <reply>` -> single element .Value
```

`tab` writes the driver `.ps1` (UTF-8 with BOM, so 5.1 reads non-ASCII fixture lines correctly),
runs it with `-NoProfile -NonInteractive -ExecutionPolicy Bypass -File`, and parses the JSON.

- [ ] **Step 1: Write the failing tests**, each looping over `powershells()`:
    - `powershell_empty_word_offers_every_value` — `toolr ci lint --mode` plus a space lists every fixture value;
      list text is the bare value.
    - `powershell_values_are_inert` — after `tab` on every prefix of every value, no sentinel exists.
    - `powershell_replies_round_trip_through_the_parser` — for each value and each partial style
      (none, `'`, `"`), `parse_value(reply) == value`.
    - `powershell_single_quoted_partial_matches`, `powershell_double_quoted_partial_matches`,
      `powershell_backtick_escaped_partial_matches`, `powershell_smart_quoted_partial_matches`.
    - `powershell_double_quoted_partial_keeps_double_quotes` — reply starts with `"`.
    - `powershell_earlier_quoted_words_reach_the_engine_unquoted` — `toolr 'ci' lint --mode d`.
    - `powershell_attached_option_value_keeps_the_option` — `--mode=dr` → `'--mode=dry run'`,
      round-trips to `--mode=dry run`.
    - `powershell_flags_complete` — `toolr ci lint --` offers `--mode`, `--cfg`, `--out`,
      `--verbose`, and tooltips are the descriptions.
    - `powershell_double_dash_and_parameter_words_pass_through` — `toolr ci lint --` and
      `toolr --quiet ci`, each followed by a space, reach the engine.
    - `powershell_variable_or_subexpression_word_offers_nothing` — `$x`, `"a$(x)"`, `a,b` as the
      cursor word and as an earlier word; no sentinel; result falls back to files (assert no fixture
      value appears).
    - `powershell_path_slot_completes_files` — `--cfg fi` → `./files` container; `--cfg files/w`
      → `'./files/with space.txt'` (or the platform separator form) round-tripping to the path.
    - `powershell_directory_slot_completes_only_directories` — `--out files/` → only `sub`.
    - `powershell_attached_path_keeps_the_option` — `--cfg=files/w` → one word round-tripping to
      `--cfg=<path>`.
    - `powershell_wildcard_characters_in_paths_stay_literal` — a partial or result holding `*`, `?`
      or `[...]` reaches the native command as the real name: the partial is passed re-quoted in
      single quotes, results are unescaped only when `Escape(Unescape(p)) -ceq p`.
    - `powershell_tilde_path_expands_to_the_home` — a leading `~` (then `/`, `\` or the end) becomes
      `(Get-PSProvider FileSystem).Home`, since a quoted `~` isn't expanded.
    - `powershell_cursor_mid_word_completes_the_whole_word` — pins the documented limit.
    - `powershell_no_tools_dir_completes_builtins` — outside the fixture, `toolr se` → `self`.
    - `powershell_toolr_exe_name_completes` — `toolr.exe se` → `self`.
    - `powershell_unicode_value_round_trips` — `é` arrives intact (Review Focus 2).
    - `powershell_legacy_argument_passing_round_trips` — prelude
      `$PSNativeCommandArgumentPassing = 'Legacy'`; earlier words `''`, `a"b`, `e\` reach the
      engine (assert via a value slot that depends on them, or a `ci lint` positional count).
    - `powershell_hostile_session_settings_stay_quiet` — prelude sets
      `$ErrorActionPreference = 'Stop'`, `Set-StrictMode -Version Latest`,
      `$PSNativeCommandUseErrorActionPreference = $true`; completion still works and the driver's
      stderr is empty.
    - `powershell_user_function_named_toolr_does_not_recurse` — prelude defines
      `function toolr { throw 'called' }`; completion still works.
    - `powershell_non_filesystem_location_offers_no_values` — `Set-Location Env:`; no fixture value.
    - `powershell_version_guard_wraps_the_whole_script` — parse the printed script with
      `[Parser]::ParseInput`; assert one top-level `IfStatementAst`, condition text
      `$PSVersionTable.PSVersion -lt [version]'5.1'`, `if` body is the exact `Write-Warning`, and
      `Register-ArgumentCompleter` sits in the `else`. This is the only check of the warning path:
      `$PSVersionTable` is read-only, so it is not executed.
- [ ] **Step 2:** Run `mise exec powershell@7.6.9 -- cargo test -p toolr --test complete_powershell -- --ignored`
      — fails against the stub.
- [ ] **Step 3:** Implement the script per the spec. Keep comments to one line explaining why.
- [ ] **Step 4:** Tests pass on pwsh 7.6.9 locally. If a Windows machine is not available, note in
      the PR that the 5.1 loop is verified by Windows CI (Task 4).
- [ ] **Step 5:** Commit `feat(completion): add the powershell completion script`.

### Task 4: CI

**Model:** sonnet — workflow YAML following existing steps. **Reviewer:** sonnet.

**Files:**

- Modify: `.github/workflows/_test.yml`

**Steps:**

- [ ] **Step 1:** Confirm the Windows job runs the Rust integration tests (the
      `Run Rust Tests with Coverage` step uses `cargo llvm-cov … --tests` on every OS) and that
      the completion tests run in their own `--ignored` pass under `mise exec powershell@<version>`.
- [ ] **Step 2:** After the Rust test step, add two Windows-only steps
      (`if: runner.os == 'Windows'`), `shell: powershell` and `shell: pwsh`, each running the snippet
      below (expecting major version 7 in the `pwsh` step).
- [ ] **Step 3:** `prek run --all-files` (actionlint) passes.
- [ ] **Step 4:** Commit `ci(completion): exercise powershell completion on 5.1 and pwsh`.

The smoke snippet for Step 2:

```powershell
$PSVersionTable.PSVersion
if ($PSVersionTable.PSVersion.Major -ne 5) { throw "expected Windows PowerShell 5.1" }
Invoke-Expression (& toolr self completion print powershell | Out-String)
$r = TabExpansion2 -inputScript 'toolr se' -cursorColumn 8
if (-not ($r.CompletionMatches.CompletionText -contains 'self')) { throw "no completion" }
```

### Task 5: Docs and release note

**Model:** haiku — prose edits and snippet regeneration. **Reviewer:** sonnet.

**Files:**

- Modify: `docs/cli.md`, `README.md`, `docs/installation/index.md`
- Modify: `crates/toolr/src/dispatch.rs` (`COMPLETE_PATHS` doc comment lists `powershell.ps1`)
- Regenerate: `docs/cli-files/self-completion-print-help.txt`,
  `docs/cli-files/self-completion-install-help.txt`
- Create: a `release-notes/` entry

**Steps:**

- [ ] **Step 1:** docs/cli.md per spec "Docs and release note": shell list, PowerShell install
      paragraph (path, `. '<path>'` in `$PROFILE.CurrentUserAllHosts`, execution policy,
      `Invoke-Expression` route, 5.1 floor and below-floor warning, alias registration with
      `${function:_toolr_complete}`, known limits), `__complete` names `powershell.ps1`.
- [ ] **Step 2:** README and installation page: `# or zsh / fish / powershell`.
- [ ] **Step 3:** `toolr pre-commit regen-doc-snippets`.
- [ ] **Step 4:** `toolr release-notes add --text "toolr now completes in PowerShell (Windows
      PowerShell 5.1 and PowerShell 7): run \`toolr self completion install powershell\` and add the
      printed line to your profile. Installing completion no longer needs \`HOME\` to be set."`
- [ ] **Step 5:** `prek run --all-files` and `mkdocs build --strict` pass;
      `cargo xtask build-skill-refs --check` passes (no `__all__` change expected).
- [ ] **Step 6:** Commit `docs(completion): document powershell completion`.

### Task 6: Full verification and archive

**Model:** sonnet. **Reviewer:** none (gates are mechanical).

- [ ] **Step 1:** `mise run test`, polling the output every 30–60 s.
- [ ] **Step 2:** `cargo llvm-cov --workspace --exclude toolr-py --tests --lcov --output-path
      target/coverage/lcov.info -- --test-threads=1`, then
      `cargo xtask patch-coverage --base main`; close gaps with real tests.
- [ ] **Step 3:** `toolr release-notes check --base main`.
- [ ] **Step 4:** `git mv specs/2026-11-08-powershell-completion-design.md specs/archive/2026/` and
      the plan; commit `docs(specs): archive the powershell completion spec`.
