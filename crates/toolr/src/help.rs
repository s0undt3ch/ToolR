//! Render styled `--help` output by building a single markdown string
//! ourselves (clap-style layout: Usage / Commands / grouped Options) and
//! feeding it through `termimad` so docstring markdown (headings, code
//! spans, fenced blocks) renders end-to-end.
//!
//! Clap's built-in help machinery is bypassed; this module is the
//! renderer. See `cli.rs` (which disables clap's auto `--help`) and
//! `dispatch` (which intercepts `--help`/`-h` and calls `print`).

use std::io::IsTerminal;

use clap::{Arg, Command};
use termimad::{FmtText, MadSkin};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HelpMode {
    /// `-h` — title + summary + usage + condensed options + subcommands.
    Short,
    /// `--help` — full markdown body + verbose options + subcommands.
    Long,
}

// ──────────────────────────────────────────────────────────────────────────────
// Public entry point
// ──────────────────────────────────────────────────────────────────────────────

/// Render help for `cmd` and print to stdout. `bin_path` is the dotted
/// command chain (`"toolr self build-manifest"`). Honors `NO_COLOR`,
/// `$COLUMNS`, and non-TTY stdout.
pub fn print(cmd: &Command, bin_path: &str, mode: HelpMode) {
    let is_tty = std::io::stdout().is_terminal();
    let no_color = no_color_requested(std::env::var_os("NO_COLOR").as_deref());
    let width = resolve_width();
    let md = render_markdown(cmd, bin_path, mode, width);
    let skin = skin_for(is_tty, no_color);
    let fmt = FmtText::from(&skin, &md, Some(width));
    print!("{fmt}");
}

// ──────────────────────────────────────────────────────────────────────────────
// Skin + width helpers
// ──────────────────────────────────────────────────────────────────────────────

/// no-color.org: `NO_COLOR` disables colour only when present and non-empty.
fn no_color_requested(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// Pick a `MadSkin` for the given output context. Caller decides
/// `is_tty` (from `std::io::stdout().is_terminal()`) and `no_color`
/// (from a non-empty `NO_COLOR`). Non-TTY or a non-empty `NO_COLOR` ⇒ plain
/// skin (no ANSI; markdown structure still renders).
fn skin_for(is_tty: bool, no_color: bool) -> MadSkin {
    if !is_tty || no_color {
        MadSkin::no_style()
    } else {
        MadSkin::default_dark()
    }
}

/// Resolve the rendering width. Order: `$COLUMNS` if set and parseable;
/// otherwise `termimad::terminal_size().0 as usize`. Honoring `$COLUMNS`
/// keeps `toolr pre-commit regen-doc-snippets` (which pins `COLUMNS=100`) producing
/// stable captures even when stdout is not a TTY and crossterm's ioctl
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

// ──────────────────────────────────────────────────────────────────────────────
// Markdown assembly
// ──────────────────────────────────────────────────────────────────────────────

/// Build the entire help page as a single markdown string. termimad
/// parses headings, bold, code spans, fenced code blocks, and bullet
/// lists in the result; the user's docstring (interpolated as the
/// "about" / "long about" section) flows in naturally as markdown.
fn render_markdown(cmd: &Command, bin_path: &str, mode: HelpMode, width: usize) -> String {
    let mut out = String::new();
    push_about(&mut out, cmd, mode);
    push_usage(&mut out, cmd, bin_path);
    push_positionals(&mut out, cmd, mode, width);
    // Match clap's native order: Arguments → Commands → Options. Options
    // — including the synthetic `-h, --help` — render last so they sit
    // closest to the prompt when the user reads the page bottom-up.
    push_commands(&mut out, cmd);
    push_options_by_heading(&mut out, cmd, mode, width);
    push_after_help(&mut out, cmd, mode);
    // Each section pushes a trailing blank-line separator. With the
    // bugs footer gone, the final section's separator becomes a stray
    // trailing blank line — collapse to exactly one terminating "\n".
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Top-of-page description. Long mode shows `long_about` (which already
/// contains the docstring's heading sections). Short mode shows only
/// the first-paragraph `about`.
fn push_about(out: &mut String, cmd: &Command, mode: HelpMode) {
    let text = match mode {
        HelpMode::Long => cmd
            .get_long_about()
            .map(|s| s.to_string())
            .unwrap_or_else(|| cmd.get_about().map(|s| s.to_string()).unwrap_or_default()),
        HelpMode::Short => cmd.get_about().map(|s| s.to_string()).unwrap_or_default(),
    };
    if !text.is_empty() {
        out.push_str(&text);
        out.push_str("\n\n");
    }
}

/// Footer set via clap's `after_help` / `after_long_help`. Long mode prefers
/// the long variant and falls back to the short one, as clap does.
fn push_after_help(out: &mut String, cmd: &Command, mode: HelpMode) {
    let short = cmd.get_after_help();
    let text = match mode {
        HelpMode::Long => cmd.get_after_long_help().or(short),
        HelpMode::Short => short,
    };
    if let Some(text) = text {
        out.push_str(&text.to_string());
        out.push_str("\n\n");
    }
}

/// `**Usage:** \`bin_path [OPTIONS] <ARGS>${positional-spec} [COMMAND]\``
/// Component spec computed from the actual command shape:
/// * `[OPTIONS]` appears only when the command has at least one visible
///   non-positional argument.
/// * `<NAME>` or `[NAME]` for each positional, required vs optional.
/// * `<COMMAND>` (required subcommand) or `[COMMAND]` (optional).
fn push_usage(out: &mut String, cmd: &Command, bin_path: &str) {
    let has_options = cmd
        .get_arguments()
        .any(|a| !a.is_positional() && !a.is_hide_set());
    let has_subs = cmd.get_subcommands().any(|c| !c.is_hide_set());
    let subs_required = cmd.is_subcommand_required_set();

    let mut spec = String::new();
    if has_options {
        spec.push_str(" [OPTIONS]");
    }
    for arg in cmd.get_positionals().filter(|a| !a.is_hide_set()) {
        spec.push(' ');
        spec.push_str(&positional_label(arg));
    }
    if has_subs {
        spec.push_str(if subs_required {
            " <COMMAND>"
        } else {
            " [COMMAND]"
        });
    }
    out.push_str(&format!("**Usage:** `{bin_path}{spec}`\n\n"));
}

/// `Arguments:` block — only emitted when the command has positionals.
/// Each line: `  <NAME>  help text`. Names are padded to the longest in
/// the group so descriptions align in a single column. In long mode
/// `[possible values: …]` and `[default: …]` annotations hang under
/// the description on their own lines (same shape as `push_one_option`
/// for flags).
fn push_positionals(out: &mut String, cmd: &Command, mode: HelpMode, width: usize) {
    let args: Vec<&Arg> = cmd.get_positionals().filter(|a| !a.is_hide_set()).collect();
    if args.is_empty() {
        return;
    }
    out.push_str("**Arguments:**\n");
    let labels: Vec<String> = args.iter().map(|a| positional_label(a)).collect();
    let label_width = labels.iter().map(|l| l.len()).max().unwrap_or(0);
    for (arg, label) in args.iter().zip(labels.iter()) {
        let mut body = arg.get_help().map(|h| h.to_string()).unwrap_or_default();
        if mode == HelpMode::Long {
            let pv: Vec<String> = arg
                .get_possible_values()
                .iter()
                .filter(|p| !p.is_hide_set())
                .map(|p| p.get_name().to_string())
                .collect();
            if !pv.is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(&format!("[possible values: {}]", pv.join(", ")));
            }
            if let Some(default) = arg.get_default_values().first() {
                let value = default.to_string_lossy();
                // `<expr>` is toolr-core's sentinel for an unresolvable
                // Python default expression — see push_one_option.
                if value != "<expr>" {
                    if !body.is_empty() {
                        body.push('\n');
                    }
                    body.push_str(&format!("[default: {value}]"));
                }
            }
        }
        // Wrap the body at the available column width before handing it
        // to push_two_column_block so multi-line annotations hang under
        // the description column without termimad re-wrapping them.
        let body = wrap_for_two_column(&body, label_width, width);
        push_two_column_block(out, label, label_width, &body);
    }
    out.push('\n');
}

/// Wrap `text` at `(page_width - leading_indent - label_width - gap)`
/// columns so it fits cleanly in the description column of a
/// two-column block. Mirrors what push_wrapped does for the Options
/// block; pulled out so push_positionals can pre-wrap before calling
/// push_two_column_block (which preserves explicit `\n` boundaries).
fn wrap_for_two_column(text: &str, label_width: usize, page_width: usize) -> String {
    let indent = 2 + label_width + 2;
    let max = page_width.saturating_sub(indent).max(20);
    let mut out = String::new();
    for (i, input_line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut current = String::new();
        for word in input_line.split_whitespace() {
            let projected = current.len() + (if current.is_empty() { 0 } else { 1 }) + word.len();
            if projected > max && !current.is_empty() {
                out.push_str(&current);
                out.push('\n');
                current.clear();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            out.push_str(&current);
        }
    }
    out
}

/// `Options:` plus any custom `help_heading` groups. Args with no
/// heading appear under "Options:" first; the rest appear under their
/// own bold heading in the order clap reports them.
fn push_options_by_heading(out: &mut String, cmd: &Command, mode: HelpMode, width: usize) {
    let args: Vec<&Arg> = cmd
        .get_arguments()
        .filter(|a| !a.is_positional() && !a.is_hide_set())
        .collect();

    let mut default_group: Vec<&Arg> = Vec::new();
    // Use a Vec so we preserve insertion order (the order clap returns
    // args), rather than a HashMap which would alphabetise.
    let mut named_groups: Vec<(String, Vec<&Arg>)> = Vec::new();
    let mut have_help_arg = false;
    for arg in args {
        if arg.get_id().as_str() == "help" {
            have_help_arg = true;
        }
        match arg.get_help_heading() {
            None => default_group.push(arg),
            Some(h) => {
                if let Some(slot) = named_groups.iter_mut().find(|(name, _)| name == h) {
                    slot.1.push(arg);
                } else {
                    named_groups.push((h.to_string(), vec![arg]));
                }
            }
        }
    }

    // Default `Options:` section always renders. When the resolved
    // command doesn't already carry the global `-h, --help` arg (clap
    // doesn't propagate globals into a subcommand's get_arguments()),
    // we synthesize a row so users see the flag listed at every level
    // — matching clap's native per-command --help expectation.
    let group_has_short = default_group.iter().any(|a| a.get_short().is_some()) || !have_help_arg;
    out.push_str("**Options:**\n");
    for arg in default_group {
        push_one_option(out, arg, mode, width, group_has_short);
    }
    if !have_help_arg {
        push_synthetic_help_option(out, width);
    }
    out.push('\n');

    for (heading, args) in named_groups {
        let group_has_short = args.iter().any(|a| a.get_short().is_some());
        out.push_str(&format!("**{heading}:**\n"));
        if heading == crate::cli::ROOT_HEADING {
            out.push('\n');
            push_wrapped(out, crate::cli::ROOT_HEADING_NOTE, 2, width);
            out.push('\n');
        }
        for arg in args {
            push_one_option(out, arg, mode, width, group_has_short);
        }
        out.push('\n');
    }
}

/// Render the standard `-h, --help    Print help` row. Used when the
/// resolved command doesn't already carry an explicit `help` arg
/// (i.e., on every subcommand, since clap doesn't propagate the root's
/// global help into per-command `get_arguments()` walks).
fn push_synthetic_help_option(out: &mut String, width: usize) {
    out.push_str("  -h, --help\n");
    push_wrapped(out, "Print help", OPTION_HELP_INDENT, width);
}

/// One option entry. Two-line form: signature on line 1, indented help
/// (plus `possible values:` / `default:` annotations on their own
/// indented lines) on subsequent lines. Short mode trims help to its
/// first line. Help text is pre-wrapped at the available width minus
/// indent so termimad doesn't re-wrap and drop our hanging indent.
/// `group_has_short` controls whether long-only options get a 4-column
/// short-flag pad (clap's behavior: align only when at least one option
/// in the group actually uses a short).
fn push_one_option(
    out: &mut String,
    arg: &Arg,
    mode: HelpMode,
    width: usize,
    group_has_short: bool,
) {
    let signature = render_option_signature(arg, group_has_short);
    out.push_str(&format!("  {signature}\n"));

    let help = arg.get_help().map(|h| h.to_string()).unwrap_or_default();
    let help = if mode == HelpMode::Short {
        help.lines().next().unwrap_or("").to_string()
    } else {
        help
    };
    push_wrapped(out, &help, OPTION_HELP_INDENT, width);

    if mode == HelpMode::Long {
        let pv: Vec<String> = arg
            .get_possible_values()
            .iter()
            .filter(|p| !p.is_hide_set())
            .map(|p| p.get_name().to_string())
            .collect();
        if !pv.is_empty() {
            push_wrapped(
                out,
                &format!("[possible values: {}]", pv.join(", ")),
                OPTION_HELP_INDENT,
                width,
            );
        }
        if let Some(default) = arg.get_default_values().first() {
            let value = default.to_string_lossy();
            // `<expr>` is toolr-core's sentinel for a Python default
            // expression we couldn't resolve to a literal (e.g.
            // `param: str = SOME_CONSTANT`). Showing it as a "default"
            // would mislead the user — there's nothing they could
            // paste back. Suppress.
            if value != "<expr>" {
                push_wrapped(
                    out,
                    &format!("[default: {value}]"),
                    OPTION_HELP_INDENT,
                    width,
                );
            }
        }
    }
}

/// Hanging-indent width for option help — matches clap's default
/// layout (10 columns). The indent is built from non-breaking spaces
/// (`U+00A0`) in `push_wrapped` so CommonMark doesn't see it as a
/// 4+-space code block; genuine fenced code blocks in docstrings keep
/// their own background styling unaffected.
const OPTION_HELP_INDENT: usize = 10;

/// `-X, --long-name <VALUE>` style signature. When the surrounding
/// group has any short flag, long-only entries get a 4-column pad so
/// the long column lines up (matching clap's default layout). When no
/// option in the group has a short, no pad is applied — saving 4
/// columns of horizontal real estate.
///
/// The pad uses non-breaking spaces (`U+00A0`) so the line doesn't
/// accidentally trip CommonMark's 4-space code-block trigger when
/// termimad parses it.
fn render_option_signature(arg: &Arg, group_has_short: bool) -> String {
    // Width of `-X, ` in display columns — 4 cells.
    const SHORT_PAD: &str = "\u{a0}\u{a0}\u{a0}\u{a0}";
    let short_or_pad: String = match (arg.get_short(), group_has_short) {
        (Some(c), _) => format!("-{c}, "),
        (None, true) => SHORT_PAD.to_string(),
        (None, false) => String::new(),
    };
    let long = arg.get_long().map(|l| format!("--{l}")).unwrap_or_default();
    let value = if arg.get_action().takes_values() {
        // Hyphenated like the flag; positionals keep the raw id, which is what clap's own errors print.
        let name = arg.get_value_names().and_then(|n| n.first()).map_or_else(
            || arg.get_id().as_str().replace('_', "-"),
            |v| v.to_string(),
        );
        format!(" <{name}>")
    } else {
        String::new()
    };
    let combined = match (long.is_empty(), arg.get_short()) {
        // Short-only — keep as just `-X` (no padding needed).
        (true, Some(c)) => format!("-{c}"),
        _ => format!("{short_or_pad}{long}"),
    };
    format!("{combined}{value}")
}

/// `Commands:` block — `  name    summary`, name column padded to the
/// longest in the group. Only emitted when the command has visible
/// children.
fn push_commands(out: &mut String, cmd: &Command) {
    let children: Vec<&Command> = cmd.get_subcommands().filter(|c| !c.is_hide_set()).collect();
    if children.is_empty() {
        return;
    }
    out.push_str("**Commands:**\n");
    let width = children
        .iter()
        .map(|c| c.get_name().len())
        .max()
        .unwrap_or(0);
    for child in children {
        let name = child.get_name();
        let summary = child
            .get_about()
            .map(|s| s.to_string().lines().next().unwrap_or("").to_string())
            .unwrap_or_default();
        push_two_column_block(out, name, width, &summary);
    }
    out.push('\n');
}

// ──────────────────────────────────────────────────────────────────────────────
// Small shared helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Pre-wrap `text` at `(width - indent)`, indenting every output line
/// by `indent` columns. Termimad would otherwise re-wrap our manually-
/// indented multi-line text and drop the hanging indent on continuation
/// lines; pre-wrapping at safe width means every emitted line already
/// fits and termimad preserves them verbatim.
///
/// The indent is built from non-breaking spaces (`U+00A0`) so it
/// renders as visual whitespace but doesn't count as ASCII leading
/// whitespace for CommonMark's 4+-space code-block trigger. This lets
/// us use clap's deep 10-column hanging indent without termimad
/// shading the help text background.
fn push_wrapped(out: &mut String, text: &str, indent: usize, width: usize) {
    if text.is_empty() {
        return;
    }
    let max = width.saturating_sub(indent).max(20);
    let pad: String = "\u{a0}".repeat(indent);
    for input_line in text.lines() {
        let mut current = String::new();
        for word in input_line.split_whitespace() {
            let needs_space = !current.is_empty();
            let projected = current.len() + (if needs_space { 1 } else { 0 }) + word.len();
            if projected > max && !current.is_empty() {
                out.push_str(&pad);
                out.push_str(&current);
                out.push('\n');
                current.clear();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            out.push_str(&pad);
            out.push_str(&current);
            out.push('\n');
        }
    }
}

/// Two-column block: `  <label-padded-to-width>  <text>`. When `text`
/// has multiple lines, the continuation lines are indented past the
/// label column so they hang under the description. The 2-space gap
/// between label and text matches clap's default short-label layout.
///
/// Continuation-line indent is built from non-breaking spaces
/// (`U+00A0`) so CommonMark doesn't treat lines deeper than the
/// 4-column code-block trigger as code blocks (which would cause
/// termimad to strip leading whitespace and break the hanging
/// indent). The label column on the first line uses regular ASCII
/// spaces — it's only 2 columns deep, well under the trigger.
fn push_two_column_block(out: &mut String, label: &str, width: usize, text: &str) {
    let indent_size = 2 + width + 2; // leading "  " + label-col + 2-space gap
    let indent: String = "\u{a0}".repeat(indent_size);
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    out.push_str(&format!(
        "  {label:<width$}  {first}\n",
        label = label,
        width = width,
        first = first
    ));
    for line in lines {
        out.push_str(&format!("{indent}{line}\n"));
    }
}

/// `<name>` or `[name]`, with a trailing `...` when it takes several values.
fn positional_label(arg: &Arg) -> String {
    let p = positional_placeholder(arg);
    let many = arg
        .get_num_args()
        .is_some_and(|r| r.max_values() > 1 && r.min_values() != r.max_values());
    let dots = if many { "..." } else { "" };
    if arg.is_required_set() {
        format!("<{p}>{dots}")
    } else {
        format!("[{p}]{dots}")
    }
}

/// Display name for a positional — the first value name if set, else
/// the arg id (matches clap's own placeholder choice).
fn positional_placeholder(arg: &Arg) -> String {
    arg.get_value_names()
        .and_then(|names| names.first())
        .map(|n| n.to_string())
        .unwrap_or_else(|| arg.get_id().as_str().to_string())
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
