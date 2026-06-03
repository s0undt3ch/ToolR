//! Render styled `--help` output by feeding docstring markdown through
//! `termimad` end-to-end. clap's built-in help renderer is bypassed
//! (see `cli.rs` and `dispatch.rs`); this module is the renderer.

use termimad::MadSkin;

#[allow(dead_code)] // wired up in Task 6
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HelpMode {
    /// `-h` — title + summary + usage + condensed options + subcommands.
    Short,
    /// `--help` — full markdown body + verbose options + subcommands + bugs.
    Long,
}

/// Resolve the rendering width.
///
/// Order: `$COLUMNS` if set and parseable; otherwise
/// `termimad::terminal_size().0 as usize`. Honoring `$COLUMNS` keeps
/// `regen-doc-snippets.py` (which pins `COLUMNS=100`) producing stable
/// captures even when stdout is not a TTY and crossterm's ioctl
/// fallback would return termimad's 50-col default.
#[allow(dead_code)] // wired up in Task 5
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

/// Pick a `MadSkin` for the given output context. Caller decides
/// `is_tty` (from `std::io::stdout().is_terminal()`) and `no_color`
/// (from `NO_COLOR` env presence). Non-TTY or `NO_COLOR` set ⇒
/// plain skin (no ANSI; markdown structure still renders).
#[allow(dead_code)] // wired up in Task 5
fn skin_for(is_tty: bool, no_color: bool) -> MadSkin {
    if !is_tty || no_color {
        MadSkin::no_style()
    } else {
        MadSkin::default_dark()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Template constants
// ──────────────────────────────────────────────────────────────────────────────

const TPL_TITLE: &str = "# **${name}** ${version}";
const TPL_INTRODUCTION: &str = "${about-text}";
const TPL_USAGE: &str = "**Usage:** `${name} [OPTIONS]${positional-args}`";

// `${possible_values}` and `${default}` come from clap-help with their own
// leading-space + label prefix (e.g. " Possible values: [a, b]", " Default: `x`"),
// or empty when absent. Inline-concatenate them on the help line so empty values
// disappear cleanly; on their own template lines they'd leave a stray blank row.
const TPL_OPTIONS: &str = "\
**Options:**
${option-lines
* **${short}** **${long}** ${value-braced}
  ${help}${possible_values}${default}
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

// ──────────────────────────────────────────────────────────────────────────────
// Public print entry point + pure render function
// ──────────────────────────────────────────────────────────────────────────────

use std::io::IsTerminal;

use clap::Command;
use clap_help::Printer;
use termimad::minimad::TextTemplate;
use termimad::FmtText;

/// Render help for `cmd` and print to stdout. `bin_path` is the
/// dotted command chain (`"toolr self build-manifest"`). Honors
/// `NO_COLOR`, `$COLUMNS`, and non-TTY stdout.
#[allow(dead_code)] // wired up in Task 6/7
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
        HelpMode::Short => cmd.get_about().map(|s| s.to_string()).unwrap_or_default(),
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
            let help = a.get_help().map(|h| h.to_string()).unwrap_or_default();
            let first = help.split('\n').next().unwrap_or("").to_string();
            a.help(first)
        });
    }
    cmd
}

fn populate_subcommands(printer: &mut Printer<'_>, cmd: &Command) {
    for child in cmd.get_subcommands() {
        if child.is_hide_set() {
            continue;
        }
        let name = child.get_name().to_string();
        let summary = child
            .get_about()
            .map(|s| s.to_string().lines().next().unwrap_or("").to_string())
            .unwrap_or_default();
        let sub = printer.expander_mut().sub("subcommand-lines");
        sub.set("sub-name", &name);
        sub.set("sub-summary", &summary);
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
}
