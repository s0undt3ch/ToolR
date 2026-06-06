<!-- rumdl-disable-file MD013 -->
<!-- Long prose paragraphs and quoted commit messages run past the 120-col cap. Plan docs are read in editor soft-wrap, not narrow terminals. -->

# clap-help-driven `--help` rendering — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace clap's default `--help` renderer with `clap-help` + `termimad`, so Python docstring markdown (headings, sections, code blocks, lists) renders as styled output. Promote `Examples:` / `Notes:` / etc. to proper `## Heading` markdown in `Docstring::full_description()`.

**Architecture:** New `crates/toolr/src/help.rs` module wraps `clap_help::Printer` for the expander + auto-populated option lines, but drives the final render loop ourselves so we can honor `$COLUMNS` (clap-help's `with_max_width` only caps). `dispatch.rs` intercepts `-h` / `--help` at any level, looks up the resolved `Command`, computes the dotted `bin_path`, and calls `crate::help::print`. clap's built-in help is disabled across the tree.

**Tech Stack:** Rust (clap 4, clap-help 1.5, termimad 0.34). Python (docstrings live in plugin code; format change propagates through `toolr-core::docstrings`). `mise run test` for the umbrella check. `prek run --all-files` for hooks (rumdl, doc snippets, regen-skill-refs).

**Spec:** `specs/2026-06-03-clap-help-rendering-design.md`.

---

## File Structure

| Path | Status | Responsibility |
|------|--------|----------------|
| `crates/toolr/src/help.rs` | new | Templates, skin, width resolution, render loop, public `print()` entry point |
| `crates/toolr/src/main.rs` | modify | Add `mod help;`; remove `mod markdown;` |
| `crates/toolr/src/cli.rs` | modify | `disable_help_flag(true)` on root + every group; add explicit global `-h`/`--help` flags; drop `crate::markdown::render(...)` wrappers |
| `crates/toolr/src/dispatch.rs` | modify | Intercept `-h`/`--help` before normal dispatch; resolve target Command + bin_path; call `help::print`; exit 0 |
| `crates/toolr/src/markdown.rs` | delete | clap-help renders markdown directly; pre-render pass no longer needed |
| `crates/toolr-core/src/docstrings.rs` | modify | `full_description()` emits `## Examples` / `## Notes` / etc. instead of bare label lines |
| `crates/toolr-core/src/docstrings/full_description_test.rs` | modify | Update assertions to expect heading format |
| `crates/toolr-core/src/parser/groups.rs` | modify (tests only) | Update `description` assertions that quoted the old flat label format |
| `crates/toolr-py/python/toolr/utils/_docstrings.py` | (no change) | Pure proxy; just exposes the bytes returned by Rust |
| `tests/utils/docstrings/test_advanced_features.py` | modify | Update `LONG_DOCSTRING_FULL_DESCRIPTION` constant to new format |
| `tests/decorators/test_decorators_unit.py` | modify | Update inline `description` assertions if any quote the flat-label format |
| `crates/toolr/tests/cli_smoke.rs` | modify | Update `--help` assertions for the new renderer |
| `crates/toolr/tests/dispatch_coverage.rs` | modify | Same as cli_smoke |
| Workspace `Cargo.toml` | modify | Add `clap-help = "1.5"`; remove `wrap_help` from clap features |
| `docs/**/*.txt` | regen | Snippet capture via `prek run --all-files` (existing hook) |
| `UNRELEASED.md` | modify | Changelog entries (`### Changed`, `### Added`, `### Removed`) |

---

## Task 1: Add `clap-help` to workspace dependencies

**Files:**

- Modify: workspace `Cargo.toml`
- Modify: `crates/toolr/Cargo.toml`
- [ ] **Step 1: Add `clap-help` to workspace `[workspace.dependencies]`**

Open `Cargo.toml`. Find the `[workspace.dependencies]` table. Add:

```toml
clap-help = "1.5"
```

Place alphabetically next to `clap = "4"`.

- [ ] **Step 2: Add `clap-help` to the `toolr` crate's deps**

Open `crates/toolr/Cargo.toml`. In `[dependencies]`, add:

```toml
clap-help.workspace = true
```

Place alphabetically near `clap.workspace = true`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p toolr`
Expected: clean exit. Lockfile updates to pin clap-help 1.5.x.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock crates/toolr/Cargo.toml
git commit -m "deps(toolr): add clap-help 1.5 for markdown --help rendering"
```

---

## Task 2: Promote section headings in `Docstring::full_description`

**Files:**

- Modify: `crates/toolr-core/src/docstrings.rs` (lines 41-119)
- Modify: `crates/toolr-core/src/docstrings/full_description_test.rs`
- Test: existing test file above
- [ ] **Step 1: Update unit tests to expect heading format (red)**

Open `crates/toolr-core/src/docstrings/full_description_test.rs`. Read the file end-to-end first. For every assertion that contains `Examples:`, `Notes:`, `Warnings:`, `See Also:`, `References:`, `Todo:`, `Deprecated:`, `Version Added:`, or `Version Changed:` in expected output, rewrite to expect the new markdown-heading form. Examples of replacements:

| Old expected fragment | New expected fragment |
|-----------------------|-----------------------|
| `"\nExamples:\n\n"` | `"\n## Examples\n\n"` |
| `"\n\nNotes:\n"` | `"\n\n## Notes\n\n"` |
| `"\n\nWarnings:\n"` | `"\n\n## Warnings\n\n"` |
| `"\n\nSee Also:\n"` | `"\n\n## See Also\n\n"` |
| `"\n\nReferences:\n"` | `"\n\n## References\n\n"` |
| `"\n\nTodo:\n"` | `"\n\n## Todo\n\n"` |
| `"\n\nDeprecated:\n"` | `"\n\n## Deprecated\n\n"` |
| `"\n\nVersion Added: "` | `"\n\n## Version Added\n\n"` |
| `"\n\nVersion Changed:\n"` | `"\n\n## Version Changed\n\n"` |

If a test asserts the full body verbatim (long string literal), replace the substring in place.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p toolr-core --lib full_description`
Expected: tests fail with assertion diffs showing old vs new heading format.

- [ ] **Step 3: Update `Docstring::full_description()` to emit headings (green)**

Open `crates/toolr-core/src/docstrings.rs`.

Replace lines 52-78 (the section-rendering body inside `full_description`) and `append_bullet_section` (lines 100-119) with:

```rust
        if !self.examples.is_empty() {
            out.push_str("\n\n## Examples\n");
            for example in &self.examples {
                let mut description = example.description.clone();
                if !description.starts_with("- ") && !description.starts_with("* ") {
                    description = format!("- {description}");
                }
                out.push_str("\n\n");
                out.push_str(&description);
                if !example.snippet.is_empty() {
                    out.push_str("\n\n```\n");
                    out.push_str(&example.snippet);
                    out.push_str("\n```");
                }
            }
        }

        append_bullet_section(&mut out, "Notes", &self.notes);
        append_bullet_section(&mut out, "Warnings", &self.warnings);
        append_bullet_section(&mut out, "See Also", &self.see_also);
        append_bullet_section(&mut out, "References", &self.references);
        append_bullet_section(&mut out, "Todo", &self.todo);

        if let Some(deprecated) = &self.deprecated {
            out.push_str("\n\n## Deprecated\n\n");
            out.push_str(deprecated);
        }

        if let Some(version_added) = &self.version_added {
            out.push_str("\n\n## Version Added\n\n");
            out.push_str(version_added);
        }

        if !self.version_changed.is_empty() {
            out.push_str("\n\n## Version Changed\n\n");
            for vc in &self.version_changed {
                out.push_str("- ");
                out.push_str(&vc.version);
                out.push_str(": ");
                out.push_str(&vc.description);
                out.push('\n');
            }
        }
```

And replace `append_bullet_section` (lines 100-119) with:

```rust
/// Append a `## Title\n\n- a\n- b` block to ``out`` when ``items`` is
/// non-empty. Existing leading ``- ``/``* `` bullets are preserved
/// verbatim; otherwise we prefix each line with ``- ``.
fn append_bullet_section(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str("\n\n## ");
    out.push_str(title);
    out.push_str("\n");
    for item in items {
        let prefixed = if item.starts_with("- ") || item.starts_with("* ") {
            item.clone()
        } else {
            format!("- {item}")
        };
        out.push('\n');
        out.push_str(&prefixed);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p toolr-core --lib full_description`
Expected: all pass.

- [ ] **Step 5: Check downstream callers' tests**

Run: `cargo test -p toolr-core --lib`
Expected: parser tests (`groups.rs`, `commands.rs`) referencing `full_description` may fail with assertion mismatches. For each failure, update the expected fragment in the test to match the new heading format. If the parser test was checking *structure* (e.g. "contains substring") rather than verbatim content, it may pass as-is.

Run: `cargo test -p toolr-core --lib` until clean.

- [ ] **Step 6: Update Python-side tests**

Run: `uv run pytest tests/utils/docstrings/test_advanced_features.py -v`
Expected: `test_full_description` fails because `LONG_DOCSTRING_FULL_DESCRIPTION` still contains flat labels.

Open `tests/utils/docstrings/test_advanced_features.py`. Locate `LONG_DOCSTRING_FULL_DESCRIPTION` and rewrite its expected text using the same replacement table as Step 1.

Run: `uv run pytest tests/utils/docstrings/test_advanced_features.py -v` until clean.

Also run: `uv run pytest tests/decorators/test_decorators_unit.py -v`. If any tests assert on `description`/`long_about` containing the old flat labels, update them. (Inspect the failures; the file has multiple tests but only the ones touching multi-section docstrings should diff.)

- [ ] **Step 7: Commit**

```bash
git add crates/toolr-core/src/docstrings.rs \
        crates/toolr-core/src/docstrings/full_description_test.rs \
        crates/toolr-core/src/parser/groups.rs \
        crates/toolr-core/src/parser/commands.rs \
        tests/utils/docstrings/test_advanced_features.py \
        tests/decorators/test_decorators_unit.py
git commit -m "feat(core): promote docstring section labels to markdown headings

\`Docstring::full_description\` now emits \`## Examples\`, \`## Notes\`,
etc. instead of bare \`Examples:\` / \`Notes:\` labels. Renderers
downstream see proper markdown headings, ready for clap-help."
```

Only add files that you actually modified. Skip any test file that did not need changes.

---

## Task 3: `help.rs` — module skeleton + skin selection

**Files:**

- Create: `crates/toolr/src/help.rs`
- Modify: `crates/toolr/src/main.rs` (add `mod help;`)
- [ ] **Step 1: Add `mod help;` to `main.rs`**

Open `crates/toolr/src/main.rs`. Find the existing `mod ...;` declarations (e.g. `mod markdown;`, `mod cli;`). Insert `mod help;` in alphabetical order.

- [ ] **Step 2: Create `help.rs` with `HelpMode` and `skin_for` (red)**

Create `crates/toolr/src/help.rs` with this content:

```rust
//! Render styled `--help` output by feeding docstring markdown through
//! `termimad` end-to-end. clap's built-in help renderer is bypassed
//! (see `cli.rs` and `dispatch.rs`); this module is the renderer.

use termimad::MadSkin;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HelpMode {
    /// `-h` — title + summary + usage + condensed options + subcommands.
    Short,
    /// `--help` — full markdown body + verbose options + subcommands + bugs.
    Long,
}

/// Pick a `MadSkin` for the given output context. Caller decides
/// `is_tty` (from `std::io::stdout().is_terminal()`) and `no_color`
/// (from `NO_COLOR` env presence). Non-TTY or `NO_COLOR` set ⇒
/// plain skin (no ANSI; markdown structure still renders).
fn skin_for(is_tty: bool, no_color: bool) -> MadSkin {
    if !is_tty || no_color {
        MadSkin::no_style()
    } else {
        MadSkin::default_dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tty_no_color_unset_returns_styled_skin() {
        let s = skin_for(true, false);
        let plain = MadSkin::no_style();
        assert!(s.bold != plain.bold, "default_dark should differ from no_style on bold styling");
    }

    #[test]
    fn non_tty_returns_plain_skin() {
        let s = skin_for(false, false);
        let plain = MadSkin::no_style();
        assert_eq!(s.bold, plain.bold);
    }

    #[test]
    fn no_color_set_returns_plain_skin_even_on_tty() {
        let s = skin_for(true, true);
        let plain = MadSkin::no_style();
        assert_eq!(s.bold, plain.bold);
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p toolr --lib help::tests`
Expected: all three tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/toolr/src/help.rs crates/toolr/src/main.rs
git commit -m "feat(toolr): add help module skeleton with TTY-aware skin"
```

---

## Task 4: `help.rs` — `resolve_width` helper

**Files:**

- Modify: `crates/toolr/src/help.rs`

- [ ] **Step 1: Write the failing tests (red)**

Append to `crates/toolr/src/help.rs` (before the existing `#[cfg(test)] mod tests`):

```rust
/// Resolve the rendering width.
///
/// Order: `$COLUMNS` if set and parseable; otherwise
/// `termimad::terminal_size().0 as usize`. Honoring `$COLUMNS` keeps
/// `regen-doc-snippets.py` (which pins `COLUMNS=100`) producing stable
/// captures even when stdout is not a TTY and crossterm's ioctl
/// fallback would return termimad's 50-col default.
fn resolve_width() -> usize {
    resolve_width_from(std::env::var("COLUMNS").ok().as_deref(), || {
        termimad::terminal_size().0 as usize
    })
}

fn resolve_width_from(env_columns: Option<&str>, fallback: impl FnOnce() -> usize) -> usize {
    env_columns
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(fallback)
}
```

Append these tests inside the existing `mod tests` block (before the closing `}`):

```rust
    #[test]
    fn columns_env_parses_to_width() {
        assert_eq!(resolve_width_from(Some("100"), || 50), 100);
    }

    #[test]
    fn columns_env_unset_uses_fallback() {
        assert_eq!(resolve_width_from(None, || 80), 80);
    }

    #[test]
    fn columns_env_empty_uses_fallback() {
        assert_eq!(resolve_width_from(Some(""), || 80), 80);
    }

    #[test]
    fn columns_env_zero_uses_fallback() {
        assert_eq!(resolve_width_from(Some("0"), || 80), 80);
    }

    #[test]
    fn columns_env_non_numeric_uses_fallback() {
        assert_eq!(resolve_width_from(Some("wide"), || 80), 80);
    }

    #[test]
    fn columns_env_whitespace_trimmed() {
        assert_eq!(resolve_width_from(Some(" 120 "), || 80), 120);
    }
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p toolr --lib help::tests`
Expected: all skin + width tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/toolr/src/help.rs
git commit -m "feat(toolr/help): resolve render width from \$COLUMNS"
```

---

## Task 5: `help.rs` — templates, expander wiring, render loop

**Files:**

- Modify: `crates/toolr/src/help.rs`

- [ ] **Step 1: Add template constants and `templates_for`**

Append to `help.rs` after `resolve_width_from`:

```rust
const TPL_TITLE: &str = "# **${name}** ${version}";
const TPL_INTRODUCTION: &str = "${about-text}";
const TPL_USAGE: &str = "**Usage:** `${name} [OPTIONS]${positional-args}`";

const TPL_OPTIONS: &str = "\
**Options:**
${option-lines
* **${short}** **${long}** ${value-braced}
  ${help}
  ${possible_values}
  ${default}
}";

const TPL_POSITIONALS: &str = "\
${positional-lines
* **${key}** ${help}
}";

const TPL_SUBCOMMANDS: &str = "\
**Commands:**
${subcommand-lines
* **${sub-name}** ${sub-summary}
}";

const TPL_BUGS: &str =
    "\n**Report bugs to**: <https://github.com/s0undt3ch/ToolR/issues>";

fn templates_for(mode: HelpMode, has_subcommands: bool) -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<(&'static str, &'static str)> = vec![
        ("title", TPL_TITLE),
        ("introduction", TPL_INTRODUCTION),
        ("usage", TPL_USAGE),
        ("positionals", TPL_POSITIONALS),
        ("options", TPL_OPTIONS),
    ];
    if has_subcommands {
        v.push(("subcommands", TPL_SUBCOMMANDS));
    }
    if mode == HelpMode::Long {
        v.push(("bugs", TPL_BUGS));
    }
    v
}
```

- [ ] **Step 2: Add the public `render_to_string` and `print` functions**

Append:

```rust
use std::io::IsTerminal;

use clap::Command;
use clap_help::Printer;
use minimad::TextTemplate;
use termimad::FmtText;

/// Render help for `cmd` and print to stdout. `bin_path` is the
/// dotted command chain (`"toolr self build-manifest"`). Honors
/// `NO_COLOR`, `$COLUMNS`, and non-TTY stdout.
pub fn print(cmd: &Command, bin_path: &str, mode: HelpMode) {
    let out = render_to_string(
        cmd,
        bin_path,
        mode,
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
        resolve_width(),
    );
    print!("{out}");
}

/// Pure render — no I/O, no env reads. Used directly by unit tests.
fn render_to_string(
    cmd: &Command,
    bin_path: &str,
    mode: HelpMode,
    is_tty: bool,
    no_color: bool,
    width: usize,
) -> String {
    let mut cmd = cmd.clone().bin_name(bin_path.to_string());
    let has_subs = cmd.get_subcommands().any(|c| !c.is_hide_set());

    // For Short mode: truncate every option's help to its first line
    // *before* `Printer::new` walks the args, so the same options
    // template renders concisely. Avoids needing a separate
    // short-mode options template or a derived expander variable.
    if mode == HelpMode::Short {
        cmd = truncate_arg_help_to_first_line(cmd);
    }

    let skin = skin_for(is_tty, no_color);
    let mut printer = Printer::new(cmd.clone()).with_skin(skin.clone());

    let about_text = match mode {
        HelpMode::Long => cmd
            .get_long_about()
            .map(|s| s.to_string())
            .unwrap_or_else(|| cmd.get_about().map(|s| s.to_string()).unwrap_or_default()),
        HelpMode::Short => cmd
            .get_about()
            .map(|s| s.to_string())
            .unwrap_or_default(),
    };
    printer.expander_mut().set("about-text", &about_text);

    if has_subs {
        populate_subcommands(&mut printer, &cmd);
    }

    let mut out = String::new();
    for (_, tpl_str) in templates_for(mode, has_subs) {
        let tpl = TextTemplate::from(tpl_str);
        let text = printer.expander_mut().expand(&tpl);
        let fmt = FmtText::from_text(&skin, text, Some(width));
        out.push_str(&fmt.to_string());
    }
    out
}

fn truncate_arg_help_to_first_line(mut cmd: Command) -> Command {
    let arg_ids: Vec<String> = cmd
        .get_arguments()
        .filter(|a| !a.is_positional() && !a.is_hide_set())
        .map(|a| a.get_id().as_str().to_string())
        .collect();
    for id in arg_ids {
        cmd = cmd.mut_arg(id, |a| {
            let help = a
                .get_help()
                .map(|h| h.to_string())
                .unwrap_or_default();
            let first = help.split('\n').next().unwrap_or("").to_string();
            a.help(first)
        });
    }
    cmd
}

fn populate_subcommands(printer: &mut Printer<'_>, cmd: &Command) {
    let sub = printer.expander_mut().sub("subcommand-lines");
    for child in cmd.get_subcommands() {
        if child.is_hide_set() {
            continue;
        }
        let name = child.get_name().to_string();
        let summary = child
            .get_about()
            .map(|s| s.to_string().lines().next().unwrap_or("").to_string())
            .unwrap_or_default();
        sub.set("sub-name", &name);
        sub.set("sub-summary", &summary);
    }
}
```

- [ ] **Step 3: Write tests for `render_to_string` (TDD)**

Append inside `mod tests`:

```rust
    use clap::{Arg, ArgAction, Command};

    fn fake_cmd() -> Command {
        Command::new("widget")
            .version("9.9.9")
            .about("Manages widgets")
            .long_about("Manages widgets.\n\n## Examples\n\nMake a widget:\n\n```\nwidget make\n```")
            .arg(
                Arg::new("force")
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("Force the operation.\nSecond help line."),
            )
            .subcommand(Command::new("make").about("Make a widget"))
    }

    #[test]
    fn long_mode_contains_full_about_with_examples_heading() {
        let out = render_to_string(&fake_cmd(), "widget", HelpMode::Long, false, true, 100);
        assert!(out.contains("widget"));
        assert!(out.contains("Make a widget"), "examples body should render: {out}");
        assert!(out.contains("Examples"), "## Examples heading should appear");
        assert!(out.contains("Report bugs to"), "bugs footer in long mode");
        assert!(out.contains("Commands"), "subcommands section");
    }

    #[test]
    fn short_mode_omits_bugs_footer_and_long_body() {
        let out = render_to_string(&fake_cmd(), "widget", HelpMode::Short, false, true, 100);
        assert!(out.contains("Manages widgets"));
        assert!(!out.contains("Report bugs to"), "no bugs footer in short mode");
        // The full long_about includes "Make a widget"; short mode uses about only.
        assert!(!out.contains("widget make"), "short mode skips long body");
    }

    #[test]
    fn render_uses_bin_path_for_usage() {
        let out = render_to_string(&fake_cmd(), "toolr widget", HelpMode::Long, false, true, 100);
        assert!(
            out.contains("toolr widget"),
            "bin_path should drive usage line: {out}"
        );
    }

    #[test]
    fn no_subcommands_omits_commands_section() {
        let cmd = Command::new("leaf").version("1.0").about("Just a leaf");
        let out = render_to_string(&cmd, "leaf", HelpMode::Long, false, true, 100);
        assert!(!out.contains("Commands:"), "no Commands header on leaf cmd");
    }
```

- [ ] **Step 4: Add `minimad` to deps if not transitively available**

Run: `cargo check -p toolr`
If the compiler complains about `unresolved import minimad`, add to the workspace `Cargo.toml`:

```toml
minimad = "0.13"
```

(It's a re-export-friendly dep of termimad; if termimad already re-exports `TextTemplate`, use `termimad::minimad::TextTemplate` and remove the direct dep. Verify before adding.)

- [ ] **Step 5: Run tests**

Run: `cargo test -p toolr --lib help::tests`
Expected: all tests pass. If `populate_subcommands`'s use of `expander_mut().sub("subcommand-lines")` doesn't compile because the API is `add_sub("subcommand-lines")` followed by `.set(...)` (or similar — verify against `clap_help::OwningTemplateExpander` v1.5.0), adapt. The intent is: for each visible child, add one row to the `subcommand-lines` sub-expansion with `sub-name` and `sub-summary` keys.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr/Cargo.toml crates/toolr/src/help.rs Cargo.toml Cargo.lock
git commit -m "feat(toolr/help): implement render loop with custom width

Templates and render path live entirely in \`crate::help\`. We use
\`clap_help::Printer\` to build the expander (it auto-populates
\`option-lines\`, \`positional-args\`, etc.), then drive the final
render through \`termimad::FmtText\` ourselves so \$COLUMNS overrides
work. Subcommand listing + bugs footer are wired into the template
list per mode."
```

---

## Task 6: Disable clap's built-in help; add global `-h` / `--help`

**Files:**

- Modify: `crates/toolr/src/cli.rs`

- [ ] **Step 1: Add `disable_help_flag(true)` to the root `Command`**

Open `crates/toolr/src/cli.rs`. Locate `pub fn build_command(manifest: &Manifest) -> Command` (line 128). Find the line that constructs the root `Command::new("toolr")...`. Add `.disable_help_flag(true)` to the builder chain.

- [ ] **Step 2: Add `disable_help_flag(true)` to every group + leaf builder**

In the same file, find every other `Command::new(...)` constructor (groups created from `manifest.command_groups`, leaf commands created from `cmd_to_clap`). Add `.disable_help_flag(true)` to each chain so subcommand-level `--help` doesn't get hijacked by clap.

Quick way: search the file for `Command::new(` and audit each call site.

- [ ] **Step 3: Add explicit global `--help` and `-h` arguments at the root**

After the root `Command::new("toolr")...` chain, add two `.arg(...)` calls that introduce the global flags:

```rust
        .arg(
            Arg::new("help")
                .long("help")
                .action(ArgAction::SetTrue)
                .global(true)
                .help("Print help"),
        )
        .arg(
            Arg::new("help_short")
                .short('h')
                .action(ArgAction::SetTrue)
                .global(true)
                .help("Print short help"),
        )
```

Place them with the other global flags (`--debug`, `--quiet`, etc.).

- [ ] **Step 4: Run cargo build to confirm it compiles**

Run: `cargo build -p toolr`
Expected: clean compile. `toolr --help` at this point parses but does nothing (no dispatch yet) — that's expected; we wire dispatch next.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr/src/cli.rs
git commit -m "refactor(toolr/cli): disable clap built-in help, add global help flags

Pre-step for the clap-help renderer wiring: clap's auto \`--help\`
handler is disabled across the command tree and we expose
\`help\`/\`help_short\` as global flags so \`dispatch\` can detect
them at any subcommand level."
```

---

## Task 7: Intercept `-h` / `--help` in dispatch and call `help::print`

**Files:**

- Modify: `crates/toolr/src/dispatch.rs`

- [ ] **Step 1: Read the current dispatch entry point**

Open `crates/toolr/src/dispatch.rs`. Locate the function that takes `ArgMatches` and the built `Command` and routes to handlers. Identify where you'd insert a preflight check before normal argument validation runs.

- [ ] **Step 2: Add a help-interception function**

Add at top-level in `dispatch.rs`:

```rust
use crate::help::{self, HelpMode};

/// Walk the matched subcommand chain. Return the deepest command name
/// path joined with spaces (e.g. "toolr self build-manifest") and the
/// `ArgMatches` at that level — used to look up the resolved
/// `clap::Command` in the built tree.
fn resolve_help_target<'a>(
    root: &'a clap::Command,
    matches: &clap::ArgMatches,
    root_name: &str,
) -> Option<(clap::Command, String, HelpMode)> {
    // Climb to the deepest matched subcommand where `help` or `help_short` is set.
    let mut cur_cmd = root;
    let mut cur_matches = matches;
    let mut path = vec![root_name.to_string()];

    loop {
        let next = cur_matches.subcommand();
        if let Some((name, sub_matches)) = next {
            // Only descend if either help flag is also active deeper.
            if sub_matches.get_flag("help") || sub_matches.get_flag("help_short")
                || subcommand_has_help_flag(sub_matches)
            {
                cur_cmd = cur_cmd.find_subcommand(name)?;
                cur_matches = sub_matches;
                path.push(name.to_string());
                continue;
            }
        }
        break;
    }

    let want_help = cur_matches.get_flag("help") || cur_matches.get_flag("help_short");
    if !want_help {
        return None;
    }
    let mode = if cur_matches.get_flag("help") {
        HelpMode::Long
    } else {
        HelpMode::Short
    };
    Some((cur_cmd.clone(), path.join(" "), mode))
}

fn subcommand_has_help_flag(matches: &clap::ArgMatches) -> bool {
    matches.get_flag("help")
        || matches.get_flag("help_short")
        || matches
            .subcommand()
            .map(|(_, m)| subcommand_has_help_flag(m))
            .unwrap_or(false)
}
```

(Adjust the visibility / argument signatures to match `dispatch.rs`'s existing dispatch entry function. The names of arg keys are `help` and `help_short` as set in Task 6.)

- [ ] **Step 3: Call `resolve_help_target` at the start of dispatch**

In the existing dispatch entry function, immediately after the root `ArgMatches` is available and before any normal command routing, insert:

```rust
    if let Some((resolved_cmd, bin_path, mode)) = resolve_help_target(&root_cmd, &matches, "toolr") {
        help::print(&resolved_cmd, &bin_path, mode);
        std::process::exit(0);
    }
```

Replace `root_cmd` and `matches` with the actual variable names in the function. `"toolr"` is the binary's display name — substitute the appropriate constant if one exists.

- [ ] **Step 4: Smoke-test by hand**

Run: `cargo run -p toolr -- --help`
Expected: styled markdown output, with `# **toolr**` title, `**Usage:**` line, options table, commands list, and `**Report bugs to**: …`.

Run: `cargo run -p toolr -- self build-manifest --help`
Expected: title says `toolr self build-manifest`, USAGE line uses the full chain.

Run: `cargo run -p toolr -- -h`
Expected: short output — no examples body, no bugs footer.

Run: `COLUMNS=60 cargo run -p toolr -- --help | head -20`
Expected: lines wrap at ~60 cols.

Run: `cargo run -p toolr -- --help | cat`
Expected: no ANSI escape sequences in the piped output; markdown structure visible as plain text.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr/src/dispatch.rs
git commit -m "feat(toolr): intercept --help/-h and dispatch to crate::help

\`dispatch\` walks the matched subcommand chain to the deepest level
where \`--help\` or \`-h\` is set, resolves the corresponding
\`clap::Command\`, and calls \`crate::help::print\`. \`--help\` wins
when both are set."
```

---

## Task 8: Remove `markdown::render` pre-rendering, delete `markdown.rs`

**Files:**

- Modify: `crates/toolr/src/cli.rs`
- Modify: `crates/toolr/src/main.rs`
- Delete: `crates/toolr/src/markdown.rs`
- [ ] **Step 1: Strip all `crate::markdown::render(...)` calls in `cli.rs`**

Open `crates/toolr/src/cli.rs`. Find every occurrence of `crate::markdown::render(...)` (there are several — at minimum lines 151, 162, 171, 181, 192, 204, 592, 596, 603 in the pre-change tree). Replace each `crate::markdown::render(<X>)` with `<X>` (i.e. pass the raw markdown string to `.about()`, `.long_about()`, `.help()`, etc. directly).

Example transformation (line 603):

```rust
// before
let mut a = Arg::new(arg.name.clone()).help(crate::markdown::render(&arg.help));
// after
let mut a = Arg::new(arg.name.clone()).help(arg.help.clone());
```

For `summary` / `long_about` blocks (around lines 592-600), the body becomes:

```rust
let summary = cmd.summary.clone();
let long_about = if cmd.description.is_empty() {
    summary.clone()
} else {
    cmd.description.clone()
};
let mut c = Command::new(cmd.name.clone())
    .about(summary)
    .long_about(long_about);
```

- [ ] **Step 2: Remove `mod markdown;` from `main.rs`**

Open `crates/toolr/src/main.rs`. Delete the `mod markdown;` line.

- [ ] **Step 3: Delete `markdown.rs`**

Run: `git rm crates/toolr/src/markdown.rs`

- [ ] **Step 4: Verify it compiles and tests pass**

Run: `cargo build -p toolr`
Expected: clean compile.

Run: `cargo test -p toolr --lib`
Expected: all `help::tests` pass; no markdown::tests remain.

- [ ] **Step 5: Manual smoke test**

Run: `cargo run -p toolr -- --help`
Run: `cargo run -p toolr -- self build-manifest --help`
Expected: still renders correctly — clap-help now sees raw markdown directly, no pre-render artifacts.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr/src/cli.rs crates/toolr/src/main.rs crates/toolr/src/markdown.rs
git commit -m "refactor(toolr): drop markdown pre-render layer

clap-help renders docstring markdown directly via termimad. The
\`crate::markdown\` module is no longer needed — its only consumer
(\`cli.rs\`) now hands raw markdown to clap-help."
```

---

## Task 9: Drop `wrap_help` feature from clap

**Files:**

- Modify: workspace `Cargo.toml`

- [ ] **Step 1: Edit workspace `Cargo.toml`**

Open `Cargo.toml`. Find the `clap` dep:

```toml
clap = { version = "4", features = ["derive", "env", "string", "wrap_help"] }
```

Remove `"wrap_help"`:

```toml
clap = { version = "4", features = ["derive", "env", "string"] }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --workspace`
Expected: clean. clap's wrap-help machinery is unused now that clap-help renders.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps(clap): drop wrap_help feature

clap-help owns wrapping now. clap is no longer rendering help text,
so its wrap_help machinery is dead weight."
```

---

## Task 10: Regenerate doc snippets

**Files:**

- Regen: `docs/**/*.txt`

- [ ] **Step 1: Run the regen hook**

Run: `prek run --all-files`
Expected: `Verify doc snippets are in sync` hook modifies many `.txt` files under `docs/`. Other hooks may also touch the spec/plan files we created — those are fine if rumdl etc. already passed earlier.

If the hook reports failures other than `Verify doc snippets are in sync`, fix them before proceeding.

- [ ] **Step 2: Visually inspect the diff**

Run: `git diff docs/ | head -200`
Look for:

- Section headings (`## Examples`, etc.) appear in the captured text.
- USAGE lines use the full `toolr <chain>` path.
- No ANSI escape sequences leaked into `.txt` files.
- `COLUMNS=100` pinning still wraps at ~100 cols.

If any captured snippet looks broken (truncated, mangled), root-cause before committing.

- [ ] **Step 3: Commit**

```bash
git add docs/
git commit -m "docs: regenerate help snippets for clap-help renderer"
```

---

## Task 11: Update integration tests

**Files:**

- Modify: `crates/toolr/tests/cli_smoke.rs`
- Modify: `crates/toolr/tests/dispatch_coverage.rs`
- Possibly modify: other `crates/toolr/tests/*.rs` that assert on `--help` output
- [ ] **Step 1: Run integration tests; capture failures**

Run: `cargo test -p toolr --test '*' 2>&1 | tee /tmp/toolr-int-fail.log`
Expected: some tests fail with assertion mismatches against new help output.

- [ ] **Step 2: For each failing assertion, update to match new output**

For each failure, read the source line, decide whether the test is:

1. **Structural** (checks "USAGE" or a flag name appears) — likely still passes; if it was matching against a removed substring like `"Examples:"`, update to `"## Examples"` or just `"Examples"`.
2. **Verbatim multi-line** (asserts the exact help body) — rewrite the expected text by running `cargo run -p toolr -- <args>` and copy-pasting the new output.

Prefer structural assertions over verbatim where possible — verbatim assertions will keep breaking as the renderer evolves.

- [ ] **Step 3: Run until clean**

Run: `cargo test -p toolr --test '*'`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add crates/toolr/tests/
git commit -m "test(toolr): update --help assertions for clap-help output"
```

---

## Task 12: Full umbrella verification

**Files:** (none; verification only)

- [ ] **Step 1: Run the umbrella check**

Run: `mise run test`
Expected: skill-refs drift gate passes, `cargo test --workspace` passes, `uv run pytest` passes.

If `cargo xtask build-skill-refs --check` fails, run `cargo xtask build-skill-refs` to regenerate, commit the result, and re-run `mise run test`.

- [ ] **Step 2: Run the slow distribution tests as a final smoke check**

Run: `uv run pytest -m distribution -x` (optional; only if you want extra confidence in the packaged wheel before opening the PR).
Expected: green or known-flaky.

- [ ] **Step 3: Manual end-to-end smoke tests**

```bash
# Styled TTY output
cargo run -p toolr -- --help

# Plain output (piped)
cargo run -p toolr -- --help | cat | head -40

# Short variant
cargo run -p toolr -- -h

# Nested subcommand
cargo run -p toolr -- self build-manifest --help

# $COLUMNS override
COLUMNS=60 cargo run -p toolr -- --help | head -30
COLUMNS=140 cargo run -p toolr -- --help | head -30

# NO_COLOR
NO_COLOR=1 cargo run -p toolr -- --help | head -20

# Hidden command should not appear (if any exist)
cargo run -p toolr -- --help | grep -v <hidden-cmd-name>
```

Eyeball every output — headings rendered, sections styled, no obvious layout breakage.

- [ ] **Step 4: No commit needed unless verification surfaced a fix**

---

## Task 13: Verify Sphinx-style double-backtick rendering

**Files:** (verification only; may modify `crates/toolr-core/src/docstrings.rs`)

This is the open-implementation-question check from the spec.

- [ ] **Step 1: Add a docstring with double-backticks to the sample repo**

In `docs/.fixtures/sample-repo/tools/`, locate or add a command whose docstring contains `` ``inline-code`` `` (double backticks). Example existing usage: search for it in the fixture (`grep -rn '\`\`' docs/.fixtures/sample-repo/tools/`).

If none exists, temporarily add to one command's docstring:

```python
"""Quick fixture command.

Uses ``kubectl`` to inspect the cluster.
"""
```

- [ ] **Step 2: Run `--help` and inspect the rendered output**

Run: `cd docs/.fixtures/sample-repo && cargo run -p toolr -- <that-command> --help`
Expected: `kubectl` (or whatever was in double-backticks) renders as inline code (highlighted in a TTY; clean in plain text).

If it renders correctly, the open question is resolved — no fixup needed. Revert any temporary additions to the fixture.

- [ ] **Step 3: If rendering is wrong (literal backticks visible, or escape sequences leaking)**

Port `normalize_rst_backticks` from the deleted `markdown.rs` into `crates/toolr-core/src/docstrings.rs` as a free function. Apply it once at parse time in `SimpleDocstringParser::parse` (the same module) — wherever the docstring text is captured into `short_description` / `long_description` / section bodies. Run the smoke test again until clean.

- [ ] **Step 4: Commit any fix**

```bash
git add crates/toolr-core/src/docstrings.rs <test-files>
git commit -m "fix(core/docstrings): normalize Sphinx-style double backticks

Termimad's CommonMark parser handles \`\`code\`\` natively in most
cases but [describe what failed]. Pre-normalize at parse time so
downstream renderers see canonical single-backtick markdown."
```

If no fix was needed, no commit.

---

## Task 14: UNRELEASED.md entry

**Files:**

- Modify: `UNRELEASED.md`

- [ ] **Step 1: Add changelog entries**

Open `UNRELEASED.md`. Add (creating the sections if missing, alphabetical inside each):

```markdown
### Changed

- `--help` output now uses `clap-help` + `termimad` end-to-end,
  rendering docstring markdown (headings, sections, fenced code
  blocks, bullet lists) as styled output in the terminal. Sections
  like `Examples`, `Notes`, `Warnings` now appear as markdown
  headings. `$COLUMNS` is still honored for width control.

### Added

- `--help` includes a "Report bugs to" footer pointing at
  <https://github.com/s0undt3ch/ToolR/issues>.

### Removed

- `crate::markdown` (internal) — clap-help renders markdown
  directly via termimad; the pre-render pass is no longer needed.
- `wrap_help` feature on the `clap` dependency.
```

- [ ] **Step 2: Commit**

```bash
git add UNRELEASED.md
git commit -m "docs(changelog): note clap-help-driven --help renderer"
```

---

## Task 15: Archive spec and plan (final step before opening PR)

**Files:**

- Move: `specs/2026-06-03-clap-help-rendering-design.md` → `specs/archive/2026/`
- Move: `specs/2026-06-03-clap-help-rendering-plan.md` → `specs/archive/2026/`

Per `CLAUDE.md`: the archive move is the final commit before opening the PR — same PR as the implementation, not a follow-up.

- [ ] **Step 1: Confirm archive directory exists**

Run: `ls specs/archive/2026/ 2>/dev/null || mkdir -p specs/archive/2026/`

- [ ] **Step 2: Move the spec and plan**

```bash
git mv specs/2026-06-03-clap-help-rendering-design.md specs/archive/2026/
git mv specs/2026-06-03-clap-help-rendering-plan.md   specs/archive/2026/
```

- [ ] **Step 3: Commit**

```bash
git commit -m "docs(specs): archive clap-help rendering design + plan"
```

- [ ] **Step 4: Open the PR**

The branch is ready. Use `git-spice branch submit --draft` per `CLAUDE.md`, then mark ready for review when CI is green.

---

## Self-Review Checklist (run after writing the plan, not at execution time)

1. **Spec coverage** — every spec section maps to a task?
   - §2.1 help module: Tasks 3, 4, 5 ✓
   - §2.2 docstrings heading promotion: Task 2 ✓
   - §2.3 markdown.rs deletion: Task 8 ✓
   - §2.4 cli.rs changes: Tasks 6, 8 ✓
   - §2.5 dispatch.rs changes: Task 7 ✓
   - §2.6 Cargo.toml: Tasks 1, 9 ✓
   - §3 templates: Task 5 ✓
   - §4.1 bugs footer: Task 5 (TPL_BUGS) ✓
   - §4.2 TTY / NO_COLOR / $COLUMNS: Tasks 3 + 4 ✓
   - §4.3 --help interception edges: Task 7 + Task 11 (test updates) ✓
   - §4.5 doc snippet regen: Task 10 ✓
   - §4.6 integration tests: Task 11 ✓
   - Open question §1 backticks: Task 13 ✓
   - Archive step (CLAUDE.md): Task 15 ✓

2. **Placeholder scan** — no "TBD", "TODO", "handle edge cases" without specifics. All code blocks have actual code, not pseudocode (except the dispatch one in Task 7 which requires looking at the existing function — flagged explicitly).

3. **Type consistency** — `HelpMode::Long` / `HelpMode::Short` used uniformly. `resolve_width()` returns `usize`. `templates_for(mode, has_subs)` signature consistent across tasks 5 and references.

4. **Known wobbles** — Task 5 has a hedge: clap-help's exact API for adding entries to a sub-expansion (`add_sub` vs `sub`, set vs setter return shape) may differ from the pseudocode. The task explicitly tells the implementer to verify against the v1.5.0 source and adapt. Same for Task 7's dispatch wiring, which depends on `dispatch.rs`'s actual entry-function shape.
