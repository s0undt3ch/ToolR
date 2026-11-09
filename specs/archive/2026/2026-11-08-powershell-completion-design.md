# PowerShell tab completion

## Why

Audit finding F-37: toolr completes in bash, zsh and fish only. `Shell` in
`crates/toolr-core/src/complete/scripts.rs` has three variants, and
`toolr self completion install` resolves its paths from `$HOME`, which `dispatch.rs::dirs_home`
requires, so the install fails on a stock Windows account where `HOME` is unset. Windows is a
first-class install target (`installation/install.ps1`, the PowerShell sections of `README.md` and
`docs/installation/index.md`), and its users get no completion at all.

## Goals

- A PowerShell completion script, embedded like the other three, that registers a native argument
  completer for `toolr` and delegates every decision to the existing `toolr __complete` engine.
- `toolr self completion print powershell` and `toolr self completion install powershell`, with
  `pwsh` accepted as an alias.
- The install command works without `$HOME`.
- Values and paths are inserted as inert text: no completion ever runs a command, expands a
  variable or evaluates a sub-expression, matching the guarantees the other three scripts give.
- Real-shell tests on pwsh 7 everywhere, and on Windows PowerShell 5.1 on Windows CI.

## Non-goals

- Replacing any script with a completion library. The engine protocol stays toolr's own.
- Editing the user's `$PROFILE` (see "Install").
- Completion in PowerShell ISE, or for a `toolr` reached through a `.cmd`/`.bat` shim (see
  "Calling the engine").
- A PowerShell lint hook (PSScriptAnalyzer) in pre-commit.
- Changing the bash, zsh or fish scripts, or the `__complete` protocol.

## Decided by the user

- Windows PowerShell 5.1 is the floor, and pwsh 7.x must also work. The user allowed a 7.x floor if
  5.1 needed significant workarounds; the list in "Supported versions" is short and contained, so
  5.1 stays.
- Below 5.1 the script prints one warning, `toolr completion needs PowerShell 5.1 or newer`, and
  registers nothing.
- The script is written in 5.1-compatible syntax and copes with 5.1's native-argument quirks.
- Windows CI exercises the completer under both `powershell` (5.1) and `pwsh`, as
  `.github/workflows/install-smoke.yml` already does for `install.ps1`.

## Supported versions

Windows PowerShell 5.1 and PowerShell 7.x (tested on 7.6). Everything the script uses exists in
5.1: `Register-ArgumentCompleter -Native` and `TabExpansion2` (5.0), the four-argument
`CompletionResult` constructor, `CompletionCompleters::CompleteFilename`, the parser AST, and
`System.Diagnostics.Process`.

The quirk that matters is not "5.1 against 7" but legacy native-argument passing, used by 5.1, by
pwsh 7.0 to 7.2, and by any session that sets `$PSNativeCommandArgumentPassing = 'Legacy'`. A probe
on pwsh 7.6.9 under `Legacy` passed the words `''`, `a"b`, `c d` to `/bin/sh` as `[ab][c][d]`:
the empty word vanished, the quote was eaten, and the space split the word. Passing the engine's
words with `& toolr …` is therefore unreliable on the floor version.

Each 5.1 workaround and its cost:

| # | 5.1 problem | Workaround | Cost |
|---|---|---|---|
| 1 | Legacy native-argument passing drops empty words and mangles `"` | Start `toolr` with `System.Diagnostics.Process` and a hand-built `Arguments` string escaped with the MSVC rules both .NET runtimes and Rust's `std::env::args` parse. One code path for every edition | One ~8-line function. Probe on 7.6.9 round-tripped `''`, `a"b`, `c d`, `e\`, `f g\`, `"`, `h\"i`, `é ü`, a tab and `\\server\x y` exactly |
| 2 | `&` decodes native stdout with `[Console]::OutputEncoding`, the OEM code page on Windows in both editions, so `é` garbles | `StandardOutputEncoding` is UTF-8 on the same `ProcessStartInfo` | One line; not 5.1-specific |
| 3 | A native command's stderr under `$ErrorActionPreference = 'Stop'` throws `NativeCommandError` in 5.1 | stderr is redirected on the `Process` and drained; the completer body runs in `try`/`catch` and offers nothing on any exception | No extra code beyond row 1, plus the `try` |
| 4 | Syntax: no ternary, `??`, `&&`/`\|\|` chains, `class`, `$IsWindows`/`$IsCoreCLR` (unset in 5.1 and fatal under a profile's `Set-StrictMode`) | Write to the 5.1 subset; the 5.1 CI run is the check | Discipline only |
| 5 | 5.1 reads a BOM-less `.ps1` as the ANSI code page | Keep the script pure ASCII (smart quotes as `[char]0x2018` and so on); a unit test asserts `is_ascii()` | One test |
| 6 | Stock Windows clients have execution policy `Restricted`, so a dot-sourced file doesn't load | Docs and the install hint name `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, and offer the file-less `Invoke-Expression` route | Docs only |

Row 1 is the only real code, and it is shared by all editions rather than a 5.1 branch: it also
covers pwsh 7.0 to 7.2 and users who set `Legacy`. A 7.x floor would not remove it unless the floor
were 7.3, so dropping 5.1 buys almost nothing.

### Version guard

The script is one top-level `if`/`else`:

```powershell
if ($PSVersionTable.PSVersion -lt [version]'5.1') {
    Write-Warning 'toolr completion needs PowerShell 5.1 or newer'
} else {
    # everything else
}
```

An `if`/`else` rather than `return`, so the guard works the same whether the script is dot-sourced
or run through `Invoke-Expression`. The whole script must also parse on PowerShell 3 and 4, so the
`else` branch avoids syntax those versions reject (`class`, `using`); `[T]::new()` parses on any
version and only runs inside the `else`.

`$PSVersionTable` is read-only even in a child scope (assigning it on 7.6.9 fails with "Cannot
overwrite variable PSVersionTable because it is read-only or constant"), and CI has no PowerShell
below 5.1, so the warning branch can't be executed in a test. It is checked by inspection plus a
structural test: pwsh's own parser must see the script as a single `IfStatementAst` whose condition
is `$PSVersionTable.PSVersion -lt [version]'5.1'`, whose `if` body is exactly that `Write-Warning`,
and whose `else` body holds the registration.

## CLI surface

- `Shell::PowerShell`, displayed `powershell`. `Shell::from_str` accepts `powershell` and `pwsh`;
  the unsupported-shell error lists all four names.
- clap's `value_parser` for `self completion print|install <shell>` becomes
  `PossibleValue::new("powershell").alias("pwsh")` beside the existing three, so `--help` lists
  `powershell` and `pwsh` still parses. TAB after `toolr self completion install` offers
  `bash fish powershell zsh`.
- `toolr self completion print powershell` prints the script; `print pwsh` prints the same bytes.

## The script

`crates/toolr-core/src/complete/scripts/powershell.ps1`, embedded with `include_str!` like the
others. Its header lists the known limits, as the other scripts' headers do.

It defines one global function, `_toolr_complete`, taking the native completer's
`param($wordToComplete, $commandAst, $cursorPosition)`, and registers it:

```powershell
Register-ArgumentCompleter -Native -CommandName 'toolr', 'toolr.exe' -ScriptBlock ${function:_toolr_complete}
```

A named function lets a user register an alias the same way
(`Register-ArgumentCompleter -Native -CommandName t -ScriptBlock ${function:_toolr_complete}`),
which docs/cli.md documents next to bash's `complete -F _toolr_complete t`.

Inside the function, before anything else: `$ErrorActionPreference = 'Continue'`,
`$PSNativeCommandUseErrorActionPreference = $false` (7.3+; a plain local assignment, harmless on
5.1) and `Set-StrictMode -Off`, all function-local, so a strict user profile can't turn a quiet
completion into an error at the prompt.

### Words to engine arguments

`$wordToComplete` is not used: a probe showed it closes an open quote (`'dry r` arrives as
`'dry r'`) and holds the whole token, not a dequoted value. The script reads the AST instead.
Measured on pwsh 7.6.9:

- `$commandAst.CommandElements[0]` is the command name; it is dropped, as the other scripts drop
  `toolr`.
- Elements wholly before the cursor (`Extent.EndOffset -lt $cursorPosition`) are earlier words.
- The element with `Extent.StartOffset -lt $cursorPosition -le Extent.EndOffset` is the cursor
  word. When none matches (the cursor follows whitespace) the cursor word is `''`.
- Elements after the cursor are dropped.

Each word is mapped by AST type:

| AST | Example | Engine argument |
|---|---|---|
| `StringConstantExpressionAst` | `dry`, `'dry r`, `"dry r`, `a` + backtick-space, `--opt='dry r`, `‘dry r` | `.Value`, which the parser has already dequoted: `dry r`, `--opt=dry r` |
| `ExpandableStringExpressionAst` without nested expressions | `"a<backtick>$b"` | `.Value` (`a$b`) |
| `CommandParameterAst` | `--`, `-v`, `-x:y` | `.Extent.Text` (the probe showed `--` parses as a parameter, so its text is the only faithful form) |
| `ConstantExpressionAst` | `4`, `0x10`, `1kb` | `.Extent.Text`: a native command gets the typed text, not the number (`/bin/echo 0x10 1kb` prints `0x10 1kb`), so `--jobs 4` doesn't stop completion. Added during implementation |
| `ExpandableStringExpressionAst` with nested expressions, `VariableExpressionAst`, `SubExpressionAst`, `ArrayLiteralAst` (`a,b`), `ArrayExpressionAst`, anything else | `"a$(x)"`, `$x`, `$(rm`, `a,b`, `@(` | Offer nothing: the value PowerShell would pass at run time is unknown, and `a,b` would even pass two words |

This is the zsh rule (`(Q)` removes quoting, never expands) expressed through PowerShell's parser,
which also handles smart quotes, backtick escapes and unclosed quotes without hand-written
dequoting. Nothing is ever evaluated.

A cursor inside a word completes the whole word, and PowerShell replaces the whole token: that is
native PowerShell behaviour, and a documented known limit (bash completes only the part before the
cursor).

The working directory is `(Get-Location).ProviderPath` when `(Get-Location).Provider.Name` is
`FileSystem`. In any other provider (`Env:`, `HKLM:`, `Function:`) the completer offers nothing:
`$PWD` there is not a directory toolr can search for `tools/`.

### Calling the engine

The script resolves the executable with
`Get-Command -Name toolr -CommandType Application -ErrorAction SilentlyContinue`, skipping any
function or alias named `toolr` so a user wrapper can't recurse. On Windows it takes the first
result whose extension is `.exe`; a `.cmd`/`.bat` shim is skipped, because `cmd.exe` re-parses its
arguments and would act on `&`, `|`, `%` inside a word. When there is no usable executable the
completer offers nothing. On Linux and macOS the first result is used. The script detects Windows
with `[System.IO.Path]::DirectorySeparatorChar -eq '\'`, not `$IsWindows`, which 5.1 lacks.

It then starts `toolr __complete <cwd> <words…>` with `System.Diagnostics.Process`:

- `Arguments` is the words joined by spaces, each escaped by `_toolr_msvc_arg`: a word with no
  whitespace or `"` and not empty goes bare; otherwise backslashes before a `"` or the end are
  doubled, `"` becomes `\"`, and the word is wrapped in `"`. These are the rules .NET Framework
  hands to `CreateProcess`, .NET (Core) parses on Unix, and Rust's `std::env::args` decodes on
  Windows.
- `UseShellExecute = $false`, `CreateNoWindow = $true`, `WorkingDirectory` is the cwd.
- stdout and stderr are redirected; stdout is decoded as UTF-8 without BOM. stderr is drained with
  `ReadToEndAsync()` started before stdout is read, so neither pipe can fill and deadlock; its
  content is discarded, as every other script sends it to `/dev/null`.
- The exit code is read after `WaitForExit()`.

Lines are split on `"`n"` with a trailing `"`r"` trimmed, then on the first tab into value and
description.

### Exit codes and replies

The exit-code table in docs/cli.md (`0`, `1`, `10`, `11`) applies unchanged.

**Exit 0.** Each candidate becomes
`[System.Management.Automation.CompletionResult]::new($text, $value, 'ParameterValue', $tip)`:

- `$value` is the full value: the engine's value with `--opt=` put back when the cursor word was
  `--opt=…` (the engine returns bare values; docs/cli.md already says each script restores the
  prefix).
- `$text` is `$value` quoted for insertion (below).
- `$tip` is the description, or `$value` when the description is empty, because the constructor
  rejects an empty tooltip (probed).

**Exit 10 / 11.** The script calls
`[System.Management.Automation.CompletionCompleters]::CompleteFilename($partial)`, where `$partial` is
the cursor word's parsed value (a typed `--opt=` stripped) re-quoted in single quotes. PowerShell's
own file completion then does the work, with its own quoting (`'./with space.txt'` was probed). The
partial is the parsed value, not the raw text, so a double-quoted or backtick-escaped partial
reaches the completer as the text the user means; single quotes keep it inert. It stays a wildcard
pattern (`*` globs), as in PowerShell's native-command completion; escaping it would match nothing.
Exit 11 keeps only results whose `ResultType` is `ProviderContainer`. For each result:

- Its text is dequoted through the parser. The text is escaped for `-Path` (`` a`[1`] ``) but a
  native command needs the real name, so it is unescaped with `[WildcardPattern]::Unescape`, and only
  when `Escape(Unescape(p)) -ceq p`. Anything that doesn't round-trip (an undoubled backtick, say)
  keeps its text, so a character is never lost.
- A leading `~` followed by `/`, `\` or the end is expanded to `(Get-PSProvider FileSystem).Home`.
  The reply is quoted, and PowerShell doesn't expand a quoted `~`, so the native command would
  receive a literal `~`.
- The result is re-quoted whole with `--opt=` in front, so the reply is one word.

**Exit 1, any other code, or no candidates.** The completer returns nothing. PowerShell then falls
back to its default file-name completion: probed for both an empty return and `@()`. The only way
to suppress it, returning the typed word as the sole result, fails when the word is empty (the
constructor rejects empty text) and leaves a one-item menu otherwise. So the fallback is accepted
and documented as a known limit; see "Decisions".

### Quoting replies

PowerShell replaces the whole token at the cursor, opening quote included (probed: replacement
index at the token start, length covering the quote). The script therefore always emits a complete
word in its own quoting, unlike bash, which has to fit inside a quote the user opened.

- **Bare** when the value is non-empty, matches `^[A-Za-z0-9_./\\:=+-]+$`, and does not start with
  `~` (pwsh 7.4+ tilde-expands a bare native argument on Unix). The set leaves out `,` (array
  operator), `@` (splat), `$`, backtick, `;`, `&`, `|`, `(`, `)`, `{`, `}`, `<`, `>`, `#`, `%`
  (`--%` stops parsing), whitespace and every straight or smart quote.
- **Double-quoted** when the cursor word opened with `"` or a smart double quote: `"` + value with a
  backtick before each backtick, `$`, `"`, U+201C, U+201D and U+201E + `"`. This keeps the user's
  chosen quote style.
- **Single-quoted** otherwise: `'` + value with each `'`, U+2018, U+2019, U+201A and U+201B doubled +
  `'`. Inside single quotes nothing expands. The script doubles the five characters itself rather
  than rely on `CodeGeneration.EscapeSingleQuotedStringContent`, so 5.1's copy of that method
  doesn't need checking.

An attached value is quoted as one word with its option: `'--mode=dry run'`, which the parser
reads back as `--mode=dry run` (probed for the partial form).

The oracle for every reply is PowerShell's own parser: parsing `x <reply>` must yield one
`StringConstantExpressionAst` (or a nested-expression-free `ExpandableStringExpressionAst`) whose
`.Value` equals the full value. The tests assert exactly that for every fixture value and quote
style.

## Install

`toolr self completion install powershell` writes the script to
`<data dir>/toolr/completions/toolr.ps1`, where `<data dir>` is `$XDG_DATA_HOME` when set and
non-empty, else the platform data directory from `dirs::data_dir()` (`%APPDATA%` on Windows,
`~/Library/Application Support` on macOS, `~/.local/share` on Linux). This is the precedence
`toolr_core::uv::toolr_data_dir` already uses for the managed uv.

The other shells write a standalone file to a directory their shell loads by itself, and print a
hint (bash) or reminder (zsh) when the user has more to do; none edits a startup file, and
`install.ps1` likewise only prints a note when its prefix is not on `$env:Path`. PowerShell has no
completion directory: something in a profile has to load the file. Editing `$PROFILE` from Rust is
fragile: its path differs per edition (`Documents\WindowsPowerShell\` against
`Documents\PowerShell\`, `~/.config/powershell/` on Unix), per host, and `Documents` is often
redirected to OneDrive, so only PowerShell itself can evaluate it. So install follows precedent:
it writes the file and prints

```text
toolr: add `. '<path>'` to your PowerShell profile (`$PROFILE.CurrentUserAllHosts`) to load it
```

with `<path>` single-quoted by a new `powershell_quote` (the PowerShell counterpart of
`shell_quote`). On Windows the hint adds that the execution policy must allow local scripts
(`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`). `--quiet` drops the hint, as it drops the
bash one. A non-UTF-8 path gets the hint without a path, as `bash_source_hint` does.

The `AlreadyInstalled` / `SkippedNeedsForce` / `--force` behaviour is the shared one in
`install_script`; nothing PowerShell-specific.

docs/cli.md also documents the file-less route, the analogue of `source <(…)`:

```powershell
Invoke-Expression (& toolr self completion print powershell | Out-String)
```

It costs a `toolr` start on every shell start and needs no file or execution-policy change.

### Home directory

`dirs_home()` in `dispatch.rs` becomes "`$HOME` when set and non-empty, else `dirs::home_dir()`".
`$HOME` stays first so existing behaviour, Git Bash/MSYS users and the integration tests' `HOME`
seam are unchanged; `dirs::home_dir()` covers a stock Windows account (Known Folder API) and a Unix
account with `HOME` unset (passwd entry). The error when both fail keeps its current shape:
`cannot determine the home directory; cannot pick install path`. The decision is a pure helper,
`resolve_home(env_home, fallback)`, so both branches are unit-tested without mutating the process
environment. `dirs` is added to `crates/toolr/Cargo.toml` from the workspace; `install.rs` stays
free of it by taking the data directory as a parameter.

## Freshness and caching

Nothing new. Manifest freshness lives in the engine (`resolve_manifest_at_tab`): it checks the
cached manifest's hashes, re-parses `tools/` in memory on drift and never writes, whichever script
called it. The PowerShell script is static and reads nothing at load time, like the others. An
installed script that predates a toolr upgrade is reported by `install` as needing `--force`, the
shared behaviour.

## Tests

### Real-shell tests

A new `crates/toolr/tests/complete_powershell.rs`, not gated on `unix`. `complete_shells.rs` is
`#![cfg(unix)]` and its fixture uses file names Windows can't hold (`q"d.txt`, `host:port.txt`), so
the PowerShell tests get their own fixture: a project whose `ci lint` command has a `Literal` with
PowerShell-hostile values (`dry run`, `dry walk`, `it's`, `a"b`, `` a`b ``, `$(New-Item SENTINEL)`,
`@(New-Item SENTINEL2)`, `a,b`, `x;y`, `--%`, `~root`, `#c`, smart-quoted `‘q’`, `é`), a `Path`
option and a `DirectoryPath` option, and `files/` holding `with space.txt`, `it's.txt` and `sub/`.

Each test writes a driver `.ps1` into the fixture, which dot-sources the printed script,
`Set-Location`s, calls `TabExpansion2 -inputScript $line -cursorColumn $n`, and prints
`ConvertTo-Json -Compress` of the replacement index and length and each match's text, list text,
tooltip and result type. The test spawns `<ps> -NoProfile -NonInteractive -ExecutionPolicy Bypass
-File driver.ps1` with the built `toolr` first on `PATH`.

`powershells()` mirrors `bashes()`: it returns `pwsh` when on `PATH`, plus `powershell.exe` on
Windows. Locally an empty list skips with a message; under `CI` it fails, and on Windows CI both
interpreters are required. Every behaviour test loops over the list, so Windows CI runs each one
under 5.1 and 7.

### Windows PowerShell 5.1 in CI

`_test.yml` already runs `cargo llvm-cov … --tests` on `windows-latest`, where `powershell.exe` is
always present, so the loop above runs every completion test under 5.1 inside the existing Windows
job. As the user asked, the Windows job also gets two explicit smoke steps, one with
`shell: powershell` and one with `shell: pwsh`, after the test run. Each loads the script printed by
the built binary through `Invoke-Expression`, checks `$PSVersionTable.PSVersion.Major` (5 and 7
respectively, as `install-smoke.yml` does), and asserts that `TabExpansion2` on `toolr se` offers
`self`. This catches a script that breaks only when loaded in a real host session rather than under
`-File`.

### pwsh on Linux and macOS

pwsh 7 is pinned in `mise.toml` (`powershell = "7.6.9"`) and `mise.lock`, so CI's
`mise install --locked` provides it on every runner and contributors get it from `mise install`.
fish and zsh come from apt/brew steps in `_test.yml` instead; pwsh diverges because mise already
ships it and pinning keeps the tested version identical everywhere. CONTRIBUTING.md notes that the
PowerShell tests need it.

### Unit and binary tests

- `scripts.rs`: Display/FromStr cover `powershell` and `pwsh`; the script is non-empty and ASCII.
- `install.rs`: `install_path_for(Shell::PowerShell, …)` with and without an `XDG_DATA_HOME`
  override; `powershell_quote` for a plain path, one with a space and one with `'`.
- `dispatch.rs`: `resolve_home` for env set, env empty with fallback, both missing;
  `powershell_profile_hint` for a UTF-8 and a non-UTF-8 path.
- Binary tests: `print pwsh` equals `print powershell`; install writes the file, prints the hint,
  is idempotent and honours `--force` and `--quiet`.
- `builtin_completions.rs`: the shell-value test expects `bash fish powershell zsh`.

## Docs and release note

- `docs/cli.md`: `<shell>` lists `powershell` (alias `pwsh`); a PowerShell paragraph under
  `install` (path, profile line, execution policy, the `Invoke-Expression` route, the 5.1 floor,
  alias registration, known limits); the `__complete` section names `powershell.ps1` among the
  scripts.
- `README.md` and `docs/installation/index.md`: "or zsh / fish / powershell".
- `toolr pre-commit regen-doc-snippets` refreshes the `self-completion-*-help.txt` snippets.
- The `COMPLETE_PATHS` doc comment in `dispatch.rs` lists `powershell.ps1`.
- A release note via `toolr release-notes add`.

## Known limits (script header and docs)

- No candidates falls back to PowerShell's file-name completion.
- A cursor inside a word completes the whole word.
- A word holding a variable, sub-expression or array (`$x`, `"a$(x)"`, `a,b`) completes nothing,
  and so does anything after one.
- A `toolr` reached only through a `.cmd`/`.bat` shim completes nothing.
- A file-name partial treats `*`, `?` and `[...]` as wildcards, as PowerShell's own completion does,
  so a partial holding a whole `[...]` pair matches nothing.
- Words after `--%` (the stop-parsing token) complete nothing.
- Windows PowerShell 5.1 never calls a native completer for a bare `-` or `--`, or a word like `-x`;
  type a letter after `--`. PowerShell 6 fixed this: issue
  [PowerShell/PowerShell#2912](https://github.com/PowerShell/PowerShell/issues/2912) by PR #2919,
  and PR #3633 for `--`.
- Not supported in PowerShell ISE.

## Decisions (user, 2026-11-08)

1. **Install does not edit `$PROFILE`.** It writes `toolr.ps1` under the data directory and prints
   the dot-source line, as specced in "Install". A later `--profile` flag could add editing if users
   ask.
2. **PowerShell's own file-name fallback is accepted** when toolr has no candidates. docs/cli.md
   documents it as a known limit.
3. **Windows PowerShell 5.1 is the floor** and pwsh 7.x is supported. Below 5.1 the script prints
   the warning specced in "Version guard" and registers nothing.
4. **No completion library.** The script stays toolr's own and delegates to `toolr __complete`.
