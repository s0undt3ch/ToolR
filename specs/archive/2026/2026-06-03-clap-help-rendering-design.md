# Clap-help-driven `--help` rendering

**Status**: design

**Date**: 2026-06-03

**Topic**: Replace clap's default `--help` renderer with `clap-help` (by Canop,
the author of `termimad`) so docstring markdown — including sections, code
blocks, bullet lists, headings — renders as styled output in the terminal and
as readable plain text when captured.

## Motivation

`toolr` already passes Python docstrings through `termimad` before handing the
ANSI strings to `clap`. The current path has two limitations:

1. **clap owns the layout.** `wrap_help` re-flows the pre-rendered ANSI text,
   sometimes mangling escape sequences and column alignment. Markdown
   structure (headings, fenced code) is flattened by the time clap renders it.
2. **Sections are flat.** `Docstring::full_description()` emits `Examples:`,
   `Notes:`, etc. as bare label lines, not markdown headings. They render as
   plain text in `--help`, with no visual anchor for the section.

We want richer rendering: section headings act as headings, code blocks render
as code blocks, bullet lists as lists. The Python docstring becomes a
first-class markdown document that drives the help page.

## Decisions made during brainstorming

1. **Use `clap-help` + a small glue layer** (chosen over termimad-direct or
   vendoring `clap-help` with PR #4 applied). The glue handles the two
   features `clap-help` doesn't ship: `bin_name` plumbing for nested
   subcommand help and a `subcommand-lines` template entry.
2. **Promote section labels to markdown headings at source**:
   `Docstring::full_description()` in `toolr-core` emits `## Examples`,
   `## Notes`, etc. rather than `Examples:`. User-authored markdown headings
   in `long_description` pass through verbatim.
3. **Keep the short (`-h`) vs long (`--help`) distinction.** `-h` shows
   title + summary + USAGE + condensed options + subcommands list. `--help`
   shows the full markdown description (including all sections) + verbose
   options + subcommands list + bugs footer.
4. **Apply everywhere.** Root command, built-in groups (`self`, `project`,
   `ci`, `bench`, `version`), and manifest-derived plugin commands all use
   the new renderer. No phased rollout.
5. **Wire `bugs` to the issue tracker.** Render a footer linking to
   `https://github.com/s0undt3ch/ToolR/issues` on `--help` (long mode only).
6. **Honor `$COLUMNS`.** clap-help/termimad do not consult `$COLUMNS`; the
   chain bottoms out at `crossterm::terminal::size()`. We add an explicit
   `$COLUMNS` check in our render loop so user overrides and the
   `regen-doc-snippets.py` pinned width keep working.

## Architecture

```text
┌─────────────┐    ┌────────────────┐    ┌──────────────────────────┐
│  main.rs    │───▶│  dispatch.rs   │───▶│ help::print(cmd, path,    │
│ argv parse  │    │ intercept -h / │    │ mode)                     │
└─────────────┘    │ --help; locate │    │   ├─ set bin_name         │
                   │ resolved cmd   │    │   ├─ build Printer        │
                   └────────────────┘    │   ├─ inject subcommands   │
                                         │   ├─ resolve width        │
                                         │   │   ($COLUMNS → ioctl)  │
                                         │   ├─ pick MadSkin         │
                                         │   │   (TTY / NO_COLOR)    │
                                         │   └─ render + print       │
                                         └──────────────────────────┘
                                                  │
                                                  ▼
                                         ┌──────────────────────────┐
                                         │ docstrings::             │
                                         │   full_description()     │
                                         │ (now emits `## Heading`) │
                                         └──────────────────────────┘
```

Outer to inner:

1. **`main.rs`** parses argv with clap. Clap's built-in `--help` is disabled
   via `Command::disable_help_flag(true)` propagated to every subcommand.
   `-h` and `--help` are redefined as explicit `ArgAction::SetTrue` global
   flags so dispatch can detect them.
2. **`dispatch.rs`** inspects matches before normal command execution. If
   `--help` or `-h` fired at any level, it resolves the deepest matched
   command, computes the `bin_path` from the matched chain, and calls
   `crate::help::print(&cmd, &bin_path, mode)`. Process exits 0 after print.
3. **`crate::help`** (new module) sets `bin_name` on the cloned command,
   builds a `clap_help::Printer`, populates subcommand listings when
   children exist, runs our own render loop (resolving width from
   `$COLUMNS` first), and writes to stdout.
4. **`toolr-core::docstrings::full_description`** now emits markdown headings
   for sections. clap-help reads this via `cmd.get_long_about()`.
5. **`crate::markdown`** is deleted. The pre-render pass is no longer needed.

## Component design

### New module: `crates/toolr/src/help.rs`

Single public entry point:

```rust
pub enum HelpMode { Short, Long }

/// Render help for `cmd` and print to stdout. `bin_path` is the dotted
/// command chain (e.g. "toolr self build-manifest"). Honors NO_COLOR,
/// $COLUMNS, and non-TTY stdout.
pub fn print(cmd: &clap::Command, bin_path: &str, mode: HelpMode);
```

Private helpers:

- `bin_named(cmd: &Command, bin_path: &str) -> Command` — clones `cmd` and
  calls `.bin_name(bin_path.to_string())`. Workaround for clap-help reading
  `get_bin_name()` (or falling back to `get_name()`) when building USAGE.
- `make_printer(cmd: Command, mode: HelpMode) -> Printer<'_>` — constructs
  `Printer::new(cmd).with_skin(skin())` and registers the right template
  set for `mode`.
- `inject_subcommands(printer: &mut Printer, cmd: &Command)` — when
  `cmd.has_subcommands()`, populates the expander's `subcommand-lines`
  sub-expansion (via `printer.expander_mut().sub("subcommand-lines")`)
  with one entry per visible child: `sub-name`, `sub-summary` (first
  line of child's `about`). Hidden subcommands (`Command::hide(true)`)
  are excluded. The `subcommands` template *string* is supplied by
  `templates_for(mode)` only when the command has children — there is
  no separate `template_keys_mut()` call, since we drive the render
  loop from our own ordered list.
- `skin() -> MadSkin` — TTY-aware:
    - TTY + `NO_COLOR` unset → `MadSkin::default_dark()`.
    - Non-TTY OR `NO_COLOR` set (any value) → `MadSkin::no_style()`
  (markdown structure renders without ANSI).
- `resolve_width() -> usize` — `$COLUMNS` parsed as `usize` if set, else
  `termimad::terminal_size().0 as usize`.
- `templates_for(mode: HelpMode) -> &'static [(&'static str, &'static str)]` —
  ordered list of `(key, template_str)` pairs for the mode. We own these
  as `const`s in `help.rs`; clap-help's `Printer` does not expose its
  internal template map publicly (`templates` and `template_keys` are
  private fields, no getters), so we cannot read them back from
  `Printer`. We hand-roll the ordered list and drive the render loop
  ourselves.
- `render(printer: &mut Printer, skin: &MadSkin, width: usize, mode: HelpMode)` —
  walks `templates_for(mode)` in order. For each `(key, tpl_str)`:
  builds `TextTemplate::from(tpl_str)`; expands via
  `printer.expander_mut().expand(&tpl)` (uses clap-help's expander,
  which `Printer::new` populates with `${name}`, `${version}`,
  `${option-lines}`, positionals, etc.); hands the expanded text to
  `termimad::FmtText::from_text(skin, text, Some(width))`; prints to
  stdout. We bypass `printer.print_help()` and `print_template()`
  entirely because both hard-wire their width source (cap-only) and
  emit straight to stdout — neither lets us inject our resolved width.

**Rendering pipeline summary** — we feed width ourselves to
`termimad::FmtText::from_text`, which is the same renderer
`clap-help` uses internally. Same `MadSkin`, same expander, same
output structure; the only difference is our width source honors
`$COLUMNS`.

### Changes to `crates/toolr-core/src/docstrings.rs`

`Docstring::full_description()` rewrites the section title emission:

| Old | New |
|-----|-----|
| `Examples:` | `## Examples` |
| `Notes:` | `## Notes` |
| `Warnings:` | `## Warnings` |
| `See Also:` | `## See Also` |
| `References:` | `## References` |
| `Todo:` | `## Todo` |
| `Deprecated:` | `## Deprecated` |
| `Version Added: <v>` | `## Version Added\n\n<v>` |
| `Version Changed:` | `## Version Changed` |

User-authored markdown headings inside `long_description` pass through
verbatim. The function still returns a single `String`; only the
serialized format changes.

Existing unit tests asserting on the bare-label format will need
updating.

### Deletion: `crates/toolr/src/markdown.rs`

The module disappears. clap-help renders markdown internally via
termimad; there is no longer a pre-render pass. The
`normalize_rst_backticks` helper is dropped — CommonMark treats
``\`\`code\`\``` as a valid code span, so termimad should render it
natively. (Verified by smoke test during implementation; if termimad
needs the normalization, port the helper into
`toolr-core::docstrings` as a parser-side fixup, applied once at
parse time.)

### Changes to `crates/toolr/src/cli.rs`

- Drop every `crate::markdown::render(...)` call. Hand raw markdown
  strings to `.about()`, `.long_about()`, `.help()`.
- Apply `.disable_help_flag(true)` on root and every built subcommand.
  Done in `build_command` and any per-group `Command::new(...)` site.
- Add explicit global `-h` / `--help` flags with `ArgAction::SetTrue`
  and `global(true)` so dispatch can detect them at any level.

### Changes to `crates/toolr/src/dispatch.rs`

New early step before normal command dispatch:

```rust
if matches.get_flag("help") || matches.get_flag("help_short") {
    let (resolved_cmd, bin_path, mode) = resolve_help_target(&matches, &root_cmd);
    crate::help::print(&resolved_cmd, &bin_path, mode);
    std::process::exit(0);
}
```

`resolve_help_target` walks the matched subcommand chain to the deepest
level where the flag was set and returns the corresponding
`clap::Command` (looked up in the built tree), the dotted path
(`"toolr self build-manifest"`), and `Long` for `--help` or `Short`
for `-h`. If both are set, long wins (last wins).

### Workspace `Cargo.toml`

- Add `clap-help = "1.5"`.
- Change `clap` features from `["derive", "env", "string", "wrap_help"]`
  to `["derive", "env", "string"]`. (`wrap_help` is unused once clap is
  no longer rendering help.)
- `termimad = "0.34"` already present, unchanged.

## Templates

All template strings live as `const &str` at the top of
`crates/toolr/src/help.rs`.

### Default clap-help keys we keep

- `title` — `# **${name}** ${version}`
- `usage` — `**Usage:** \`${name} [OPTIONS]${positional-args}\``
- `positionals` — clap-help default (table of `${name}` / `${help}`)

### Customized per mode

**`introduction`** (the prose body):

The template *string* is the literal placeholder `"${about-text}"` in
both modes. We register a custom expander variable `about-text` on the
`Printer` and populate its value at render time:

- **Long mode**: `printer.expander_mut().set("about-text", cmd.get_long_about().unwrap_or_default())`.
  This is the full markdown — summary, long body, `## Examples`,
  `## Notes`, fenced code blocks, etc.
- **Short mode**: `printer.expander_mut().set("about-text", cmd.get_about().unwrap_or_default())`.
  Just the first paragraph.

We use an expander variable (not setting the docstring text as the
template directly) so that literal `${...}` sequences inside a
docstring don't collide with minimad's template syntax. The
docstring is *data*, not a template.

**`options`** (iterates `option-lines`):

- **Long mode** — multi-line per option:

  ```text
  ${option-lines
  * **${short}** **${long}** ${value-braced}
    ${help}
    ${possible_values}
    ${default}
  }
  ```

- **Short mode** — single line per option:

  ```text
  ${option-lines
  * **${short}** **${long}** ${value-braced}  ${help-first-line}
  }
  ```

  `help-first-line` is a derived expander var we populate (split
  `${help}` at first `\n`).

### New key: `subcommands`

Not built into clap-help. Included in our `templates_for(mode)`
ordered list (after `"options"`) **only when**
`cmd.has_subcommands()`:

```text
**Commands:**
${subcommand-lines
* **${sub-name}** ${sub-summary}
}
```

Populated by `inject_subcommands` via
`printer.expander_mut().sub("subcommand-lines")`.

### New key: `bugs`

Replaces clap-help's empty-by-default `bugs`. Shown on **`--help` (long)
only**:

```text
\n**Report bugs to**: https://github.com/s0undt3ch/ToolR/issues\n
```

### Template ordering

For both modes: `title` → `introduction` → `usage` → `positionals` →
`options` → `subcommands` → `bugs`.

Same order short and long; only the *contents* of `introduction` and
`options` differ between modes. `subcommands` is omitted when the
command has no children; `bugs` is omitted in short mode.

### Command archetypes — what shows up

| Archetype | Examples | `introduction` | `positionals` | `options` | `subcommands` | `bugs` (long only) |
|-----------|----------|---------------|---------------|-----------|---------------|--------------------|
| Root | `toolr` | global flags blurb | (empty) | global flags | yes | yes |
| Group | `toolr self`, `toolr ci` | group description | (empty) | global flags | yes | yes |
| Leaf | `toolr self build-manifest` | full docstring + sections | per-arg | per-arg | (omitted) | yes |

## Edge cases

### TTY / non-TTY / color

- TTY stdout + `NO_COLOR` unset → `MadSkin::default_dark()`.
- Non-TTY stdout OR `NO_COLOR` set → `MadSkin::no_style()`. Structure
  (headings, bullets, fenced code) still renders; ANSI is suppressed.
- Width: `$COLUMNS` (parsed as `usize`) → `termimad::terminal_size().0
  as usize` fallback.
- No `--color` CLI flag introduced. Out of scope.

### `--help` interception edge cases

- **Mixed flags**: `toolr --debug self --help` resolves to `toolr self`'s
  help (deepest level where the flag fired). `--debug` is a no-op in
  help dispatch — we never run the real command.
- **`--help` plus required positional missing**:
  `toolr self build-manifest --help` (no `PACKAGE`) prints help and
  exits 0. clap's missing-required-argument check is bypassed because
  we intercept before validation.
- **`-h` and `--help` both given**: `--help` (long) wins.
- **`--help` after `--`**: treated as a positional. Matches clap default.

### Commands with empty/minimal docstrings

- Empty `long_about`: `introduction` block renders empty. No section
  header is emitted (we control the template — no awkward "empty
  introduction" line).
- Empty `about` (no summary): short help shows title + USAGE + options,
  no introduction block. Acceptable.
- Plugin commands with malformed metadata: docstring parsing already
  fails earlier; we never reach `help::print` for those. No new failure
  mode.

### Doc snippet regeneration

`.pre-commit-hooks/regen-doc-snippets.py` already pins `COLUMNS=100`
and runs `toolr` with `stdin=subprocess.DEVNULL`. Because we honor
`$COLUMNS` in `resolve_width()` and select `MadSkin::no_style()` on
non-TTY stdout, captured `.txt` snippets remain stable and readable.
Implementation includes a single explicit regen step:
`prek run --all-files` followed by visual review of the diff.

### Integration tests

`crates/toolr/tests/` (assert_cmd-based) — anything matching exact
help strings will need updating. Structural assertions (presence of
"USAGE", a flag name) likely still pass. Multi-line expected strings
will be rewritten alongside the snippet regen.

### `builtin_completions.rs`

Mirrors `cli::build_command` structurally. Help text isn't in the
completion data, so it's unaffected. `--help` / `-h` remain
completable flags (they exist; they're just routed differently).

### Skill refs regen

`cargo xtask build-skill-refs --check` runs first in `mise run test`.
We add a public surface item (`crate::help::print`) and remove
`crate::markdown`. Skill refs track public-facing surface only — they
shouldn't change, but we run regen to confirm.

## What is explicitly out of scope

- Color themes beyond `default_dark`. No `--color` flag, no light-mode
  skin switching.
- Per-command custom templates. All commands share one template set.
- Backward-compat shim (`TOOLR_LEGACY_HELP=1` etc.). Clean cut.
- Upstreaming subcommand support to `clap-help` (PR #4). Tracked, not
  blocking.

## Open implementation questions

Resolved during plan/implementation, not design:

1. Does termimad's parser handle Sphinx-style ``\`\`code\`\``` (double
   backticks) natively? CommonMark allows it. Smoke test on first PR;
   if it doesn't, port `normalize_rst_backticks` into
   `toolr-core::docstrings` as a parse-time fixup.
2. Do we need to replicate clap-help's `set_rendering_width` /
   `content_width` pass to keep option columns visually aligned across
   templates? clap-help's `print_help_content_width` does a second
   pass that resizes each `FmtText` to the max content width across
   the help page. If alignment drifts in our simpler one-pass loop,
   add the same two-pass behavior (collect `FmtText`s first, compute
   `max(content_width)`, then `set_rendering_width(max)` on each
   before printing). Verify on first PR.
3. Consider upstreaming a `Printer::with_width(usize)` (non-capping)
   or public getters for `templates`/`template_keys`/`skin` to
   `clap-help`. Would let us drop the custom render loop. Tracked as
   a follow-up, not blocking this work.

## Files touched

| Path | Change |
|------|--------|
| `crates/toolr/src/help.rs` | **new** module |
| `crates/toolr/src/cli.rs` | drop `markdown::render` calls; `disable_help_flag(true)`; add global `-h`/`--help` flags |
| `crates/toolr/src/dispatch.rs` | intercept `-h`/`--help`; resolve target; call `help::print` |
| `crates/toolr/src/main.rs` | mod declaration for `help` |
| `crates/toolr/src/markdown.rs` | **deleted** |
| `crates/toolr-core/src/docstrings.rs` | promote section labels to `## Heading`; update unit tests |
| `crates/toolr/tests/**` | update assertions that pattern-match help output |
| `Cargo.toml` (workspace) | add `clap-help = "1.5"`; drop `wrap_help` from clap features |
| `docs/**/*.txt` | regenerate snippets via `regen-doc-snippets.py` |
| `UNRELEASED.md` | changelog entry |

## CHANGELOG entry (drafted)

```text
### Changed

- `--help` output now uses `clap-help` + `termimad` end-to-end, rendering
  docstring markdown (headings, sections, fenced code blocks, bullet
  lists) as styled output in the terminal. Sections like `Examples`,
  `Notes`, `Warnings` are now markdown headings rather than flat labels.
  `$COLUMNS` is still honored.

### Added

- `--help` includes a "Report bugs to" footer pointing at the issue
  tracker.

### Removed

- `crate::markdown` (internal) — clap-help renders markdown directly
  via termimad; the pre-render pass is no longer needed.
- `wrap_help` feature on the `clap` dependency.
```
