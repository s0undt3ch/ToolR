//! Render styled `--help` output by building a single markdown string
//! ourselves (clap-style layout: Usage / Commands / grouped Options) and
//! feeding it through `termimad` so docstring markdown (headings, code
//! spans, fenced blocks) renders end-to-end.
//!
//! Clap's built-in help machinery is bypassed; this module is the
//! renderer. See `cli.rs` (which disables clap's auto `--help`) and
//! `dispatch.rs` (which intercepts `--help`/`-h` and calls `print`).

use std::io::IsTerminal;

use clap::{Arg, Command};
use termimad::{FmtText, MadSkin};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HelpMode {
    /// `-h` — title + summary + usage + condensed options + subcommands.
    Short,
    /// `--help` — full markdown body + verbose options + subcommands + bugs.
    Long,
}

const BUGS_URL: &str = "https://github.com/s0undt3ch/ToolR/issues";

// ──────────────────────────────────────────────────────────────────────────────
// Public entry point
// ──────────────────────────────────────────────────────────────────────────────

/// Render help for `cmd` and print to stdout. `bin_path` is the dotted
/// command chain (`"toolr self build-manifest"`). Honors `NO_COLOR`,
/// `$COLUMNS`, and non-TTY stdout.
pub fn print(cmd: &Command, bin_path: &str, mode: HelpMode) {
    let is_tty = std::io::stdout().is_terminal();
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let width = resolve_width();
    let md = render_markdown(cmd, bin_path, mode, width);
    let skin = skin_for(is_tty, no_color);
    let fmt = FmtText::from(&skin, &md, Some(width));
    print!("{fmt}");
}

// ──────────────────────────────────────────────────────────────────────────────
// Skin + width helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Pick a `MadSkin` for the given output context. Caller decides
/// `is_tty` (from `std::io::stdout().is_terminal()`) and `no_color`
/// (from `NO_COLOR` env presence). Non-TTY or `NO_COLOR` set ⇒ plain
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
/// keeps `regen-doc-snippets.py` (which pins `COLUMNS=100`) producing
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
    push_positionals(&mut out, cmd, width);
    push_options_by_heading(&mut out, cmd, mode, width);
    push_commands(&mut out, cmd);
    if mode == HelpMode::Long {
        push_bugs_footer(&mut out);
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
        let placeholder = positional_placeholder(arg);
        let bracketed = if arg.is_required_set() {
            format!(" <{placeholder}>")
        } else {
            format!(" [{placeholder}]")
        };
        spec.push_str(&bracketed);
    }
    if has_subs {
        spec.push_str(if subs_required { " <COMMAND>" } else { " [COMMAND]" });
    }
    out.push_str(&format!("**Usage:** `{bin_path}{spec}`\n\n"));
}

/// `Arguments:` block — only emitted when the command has positionals.
/// Each line: `  <NAME>    help text`. Names are padded to the longest
/// in the group so descriptions align in a single column.
fn push_positionals(out: &mut String, cmd: &Command, _width: usize) {
    let args: Vec<&Arg> = cmd.get_positionals().filter(|a| !a.is_hide_set()).collect();
    if args.is_empty() {
        return;
    }
    out.push_str("**Arguments:**\n");
    let labels: Vec<String> = args
        .iter()
        .map(|a| {
            let p = positional_placeholder(a);
            if a.is_required_set() {
                format!("<{p}>")
            } else {
                format!("[{p}]")
            }
        })
        .collect();
    let width = labels.iter().map(|l| l.len()).max().unwrap_or(0);
    for (arg, label) in args.iter().zip(labels.iter()) {
        let help = arg.get_help().map(|h| h.to_string()).unwrap_or_default();
        push_two_column_block(out, label, width, &help);
    }
    out.push('\n');
}

/// `Options:` plus any custom `help_heading` groups. Args with no
/// heading appear under "Options:" first; the rest appear under their
/// own bold heading in the order clap reports them.
fn push_options_by_heading(out: &mut String, cmd: &Command, mode: HelpMode, width: usize) {
    let args: Vec<&Arg> = cmd
        .get_arguments()
        .filter(|a| !a.is_positional() && !a.is_hide_set())
        .collect();
    if args.is_empty() {
        return;
    }

    let mut default_group: Vec<&Arg> = Vec::new();
    // Use a Vec so we preserve insertion order (the order clap returns
    // args), rather than a HashMap which would alphabetise.
    let mut named_groups: Vec<(String, Vec<&Arg>)> = Vec::new();
    for arg in args {
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

    if !default_group.is_empty() {
        out.push_str("**Options:**\n");
        for arg in default_group {
            push_one_option(out, arg, mode, width);
        }
        out.push('\n');
    }
    for (heading, args) in named_groups {
        out.push_str(&format!("**{heading}:**\n"));
        for arg in args {
            push_one_option(out, arg, mode, width);
        }
        out.push('\n');
    }
}

/// One option entry. Two-line form: signature on line 1, indented help
/// (plus `possible values:` / `default:` annotations on their own
/// indented lines) on subsequent lines. Short mode trims help to its
/// first line. Help text is pre-wrapped at the available width minus
/// indent so termimad doesn't re-wrap and drop our hanging indent.
fn push_one_option(out: &mut String, arg: &Arg, mode: HelpMode, width: usize) {
    let signature = render_option_signature(arg);
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
            push_wrapped(
                out,
                &format!("[default: {}]", default.to_string_lossy()),
                OPTION_HELP_INDENT,
                width,
            );
        }
    }
}

/// Hanging-indent width for option help — matches clap's default
/// layout (10 columns). The indent is built from non-breaking spaces
/// (`U+00A0`) in `push_wrapped` so CommonMark doesn't see it as a
/// 4+-space code block; genuine fenced code blocks in docstrings keep
/// their own background styling unaffected.
const OPTION_HELP_INDENT: usize = 10;

/// `-X, --long-name <VALUE>` style signature. Long-only options are
/// padded so the long column lines up across all entries in a group —
/// matching clap's default layout. The pad uses non-breaking spaces
/// (`U+00A0`) so the line doesn't accidentally trip CommonMark's
/// 4-space code-block trigger when termimad parses it.
fn render_option_signature(arg: &Arg) -> String {
    // Width of `-X, ` in display columns — 4 cells.
    const SHORT_PAD: &str = "\u{a0}\u{a0}\u{a0}\u{a0}";
    let short_or_pad: String = arg
        .get_short()
        .map(|c| format!("-{c}, "))
        .unwrap_or_else(|| SHORT_PAD.to_string());
    let long = arg.get_long().map(|l| format!("--{l}")).unwrap_or_default();
    let value = if arg.get_action().takes_values() {
        arg.get_value_names()
            .and_then(|names| names.first())
            .map(|v| format!(" <{v}>"))
            .unwrap_or_default()
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
    let children: Vec<&Command> = cmd
        .get_subcommands()
        .filter(|c| !c.is_hide_set())
        .collect();
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

/// `**Report bugs to**: <https://...>`. Long mode only.
fn push_bugs_footer(out: &mut String) {
    out.push_str(&format!("**Report bugs to**: <{BUGS_URL}>\n"));
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

/// Two-column block: `  <label-padded-to-width>    <text>`. When `text`
/// has multiple lines, the continuation lines are indented past the
/// label column so they hang under the description.
fn push_two_column_block(out: &mut String, label: &str, width: usize, text: &str) {
    let indent_size = 2 + width + 4; // leading "  " + label-col + 4-space gap
    let indent: String = " ".repeat(indent_size);
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    out.push_str(&format!(
        "  {label:<width$}    {first}\n",
        label = label,
        width = width,
        first = first
    ));
    for line in lines {
        out.push_str(&format!("{indent}{line}\n"));
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
mod tests {
    use super::*;
    use clap::{Arg, ArgAction, Command};

    #[test]
    fn tty_no_color_unset_returns_styled_skin() {
        let s = skin_for(true, false);
        let plain = MadSkin::no_style();
        assert!(s.bold != plain.bold);
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

    fn widget() -> Command {
        Command::new("widget")
            .about("Manages widgets")
            .long_about("Manages widgets.\n\n## Examples\n\nMake one:\n\n```\nwidget make\n```")
            .arg(
                Arg::new("force")
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("Force the operation.\nSecond help line."),
            )
            .subcommand(Command::new("make").about("Make a widget"))
    }

    #[test]
    fn long_mode_includes_full_body_and_subcommands() {
        let md = render_markdown(&widget(), "widget", HelpMode::Long, 100);
        assert!(md.contains("## Examples"), "## Examples heading present");
        assert!(md.contains("widget make"), "examples body present");
        assert!(md.contains("**Commands:**"), "commands section");
        assert!(md.contains("Report bugs to"), "bugs footer");
    }

    #[test]
    fn short_mode_omits_long_body_and_bugs() {
        let md = render_markdown(&widget(), "widget", HelpMode::Short, 100);
        assert!(md.contains("Manages widgets"), "about line present");
        assert!(
            !md.contains("widget make"),
            "long body should be absent in short mode"
        );
        assert!(
            !md.contains("Report bugs to"),
            "bugs footer absent in short mode"
        );
    }

    #[test]
    fn usage_uses_bin_path() {
        let md = render_markdown(&widget(), "toolr widget", HelpMode::Long, 100);
        assert!(
            md.contains("toolr widget"),
            "bin path drives usage line: {md}"
        );
    }

    #[test]
    fn leaf_command_omits_commands_section() {
        let cmd = Command::new("leaf").about("Just a leaf");
        let md = render_markdown(&cmd, "leaf", HelpMode::Long, 100);
        assert!(
            !md.contains("**Commands:**"),
            "no Commands header on leaf cmd"
        );
    }

    #[test]
    fn options_are_grouped_by_help_heading() {
        let cmd = Command::new("grouper")
            .arg(
                Arg::new("plain")
                    .long("plain")
                    .action(ArgAction::SetTrue)
                    .help("default-heading flag"),
            )
            .arg(
                Arg::new("debug")
                    .long("debug")
                    .action(ArgAction::SetTrue)
                    .help_heading("Output Options")
                    .help("output-heading flag"),
            );
        let md = render_markdown(&cmd, "grouper", HelpMode::Long, 100);
        assert!(md.contains("**Options:**"), "default group heading present");
        assert!(
            md.contains("**Output Options:**"),
            "custom heading group present"
        );
        // Default group should come before custom heading.
        let opt_pos = md.find("**Options:**").unwrap();
        let out_pos = md.find("**Output Options:**").unwrap();
        assert!(opt_pos < out_pos, "default group renders first");
    }

    #[test]
    fn usage_shows_command_optional_by_default() {
        let cmd = Command::new("root").subcommand(Command::new("kid"));
        let md = render_markdown(&cmd, "root", HelpMode::Long, 100);
        assert!(md.contains("[COMMAND]"), "optional subcommand → [COMMAND]: {md}");
    }

    #[test]
    fn usage_shows_command_required_when_subcommand_required() {
        let cmd = Command::new("root")
            .subcommand_required(true)
            .subcommand(Command::new("kid"));
        let md = render_markdown(&cmd, "root", HelpMode::Long, 100);
        assert!(
            md.contains("<COMMAND>"),
            "subcommand_required → <COMMAND>: {md}"
        );
    }

    #[test]
    fn options_section_omitted_when_no_options() {
        let cmd = Command::new("noops").subcommand(Command::new("kid"));
        let md = render_markdown(&cmd, "noops", HelpMode::Long, 100);
        assert!(
            !md.contains("**Options:**"),
            "no Options section when there are no flags: {md}"
        );
    }

    #[test]
    fn possible_values_and_default_render_in_long_mode() {
        let cmd = Command::new("setlog").arg(
            Arg::new("level")
                .long("level")
                .value_name("level")
                .value_parser(["debug", "info", "warn"])
                .default_value("info")
                .help("Logging level."),
        );
        let md = render_markdown(&cmd, "setlog", HelpMode::Long, 100);
        assert!(
            md.contains("[possible values: debug, info, warn]"),
            "possible values listed: {md}"
        );
        assert!(md.contains("[default: info]"), "default listed: {md}");
    }
}
