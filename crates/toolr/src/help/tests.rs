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
fn no_color_requires_a_non_empty_value() {
    use std::ffi::OsStr;
    assert!(!no_color_requested(None));
    assert!(!no_color_requested(Some(OsStr::new(""))));
    assert!(no_color_requested(Some(OsStr::new("1"))));
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
}

#[test]
fn short_mode_omits_long_body() {
    let md = render_markdown(&widget(), "widget", HelpMode::Short, 100);
    assert!(md.contains("Manages widgets"), "about line present");
    assert!(
        !md.contains("widget make"),
        "long body should be absent in short mode"
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
    assert!(
        md.contains("[COMMAND]"),
        "optional subcommand → [COMMAND]: {md}"
    );
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
fn options_section_always_shows_synthetic_help() {
    // Even leaves with no flags of their own get `-h, --help` —
    // clap's global help arg doesn't propagate into get_arguments()
    // on subcommands, so we synthesise the row at every level.
    let cmd = Command::new("noops").subcommand(Command::new("kid"));
    let md = render_markdown(&cmd, "noops", HelpMode::Long, 100);
    assert!(
        md.contains("**Options:**"),
        "Options section always renders for the synthetic --help row: {md}"
    );
    assert!(
        md.contains("-h, --help"),
        "synthetic --help row always present: {md}"
    );
}

#[test]
fn two_column_block_uses_two_space_label_gap() {
    // Single-line text: exactly `  <label>  <text>` — 2-space leading
    // indent + label padded to width + 2-space gap before the text.
    // Matches clap's default short-label layout.
    let mut out = String::new();
    push_two_column_block(&mut out, "name", 4, "the description");
    assert_eq!(out, "  name  the description\n");
}

#[test]
fn two_column_block_pads_label_to_width() {
    // Shorter labels get left-padded to the group width so the text
    // column lines up. Padding lives inside the label column; the
    // 2-space gap is added on top.
    let mut out = String::new();
    push_two_column_block(&mut out, "ex", 7, "short");
    assert_eq!(out, "  ex       short\n");
}

#[test]
fn two_column_block_continuation_lines_hang_under_text() {
    // Multi-line text hangs under the text column on continuation
    // lines: leading indent (2) + label width + gap (2) = 2 + width
    // + 2 non-breaking spaces of leading whitespace on every line
    // after the first. NBSPs (U+00A0) keep CommonMark from
    // misreading the deep indent as a code block, which would
    // make termimad strip the leading whitespace.
    let mut out = String::new();
    push_two_column_block(&mut out, "name", 4, "first\nsecond\nthird");
    let pad = "\u{a0}".repeat(8);
    let expected = format!("  name  first\n{pad}second\n{pad}third\n");
    assert_eq!(out, expected);
}

#[test]
fn positionals_render_with_clap_two_space_gap() {
    // End-to-end: `Arguments:` rows use the same 2-space gap and the
    // label column widens to the longest label. Required → `<name>`,
    // optional → `[name]`.
    let cmd = Command::new("argy")
        .arg(Arg::new("required").required(true).help("req help"))
        .arg(Arg::new("optional").help("opt help"));
    let md = render_markdown(&cmd, "argy", HelpMode::Long, 100);
    assert!(
        md.contains("  <required>  req help"),
        "required positional uses 2-space gap: {md}"
    );
    assert!(
        md.contains("  [optional]  opt help"),
        "optional positional uses 2-space gap: {md}"
    );
}

#[test]
fn multi_value_positionals_get_an_ellipsis() {
    let cmd = Command::new("argy")
        .arg(Arg::new("one").required(true).help("one help"))
        .arg(
            Arg::new("many")
                .value_name("PACKAGE")
                .num_args(1..)
                .required(true)
                .help("many help"),
        )
        .arg(Arg::new("rest").num_args(0..).help("rest help"))
        .arg(Arg::new("pair").num_args(2).help("pair help"));
    let md = render_markdown(&cmd, "argy", HelpMode::Long, 100);
    assert!(
        md.contains("argy <one> <PACKAGE>... [rest]... [pair]"),
        "usage: {md}"
    );
    assert!(md.contains("  <PACKAGE>...  many help"), "arguments: {md}");
    assert!(md.contains("  <one>"), "single value has no ellipsis: {md}");
    assert!(
        !md.contains("<one>..."),
        "single value has no ellipsis: {md}"
    );
}

#[test]
fn value_options_show_a_placeholder_named_after_the_option() {
    let cmd = Command::new("argy")
        .arg(Arg::new("name").long("name").help("Who to greet."))
        .arg(
            Arg::new("path")
                .long("path")
                .value_name("FILE")
                .help("Where to write."),
        )
        .arg(Arg::new("max_age").long("max-age").help("Age."))
        .arg(Arg::new("force").long("force").action(ArgAction::SetTrue));
    let md = render_markdown(&cmd, "argy", HelpMode::Long, 100);
    assert!(md.contains("--name <name>"), "{md}");
    assert!(md.contains("--path <FILE>"), "{md}");
    assert!(md.contains("--max-age <max-age>"), "{md}");
    assert!(!md.contains("--force <"), "{md}");
}

#[test]
fn commands_block_renders_with_clap_two_space_gap() {
    // The `Commands:` block reuses push_two_column_block, so the
    // spacing matches `Arguments:`. Sub-name column is padded to the
    // longest sibling name.
    let cmd = Command::new("root")
        .subcommand(Command::new("short").about("a"))
        .subcommand(Command::new("longer-name").about("b"));
    let md = render_markdown(&cmd, "root", HelpMode::Long, 100);
    assert!(
        md.contains("  short        a"),
        "short name padded to longest sibling width with 2-space gap: {md}"
    );
    assert!(
        md.contains("  longer-name  b"),
        "longest sibling has no padding before the 2-space gap: {md}"
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

#[test]
fn positionals_render_possible_values_and_default_in_long_mode() {
    // Positionals with a fixed value_parser (or default) must show
    // `[possible values: …]` / `[default: …]` just like flags do.
    // Long mode only — short mode keeps positionals to one line.
    let cmd = Command::new("printer").arg(
        Arg::new("shell")
            .value_parser(["bash", "zsh", "fish"])
            .default_value("bash")
            .help("Shell to emit a completion script for"),
    );
    let md = render_markdown(&cmd, "printer", HelpMode::Long, 100);
    assert!(
        md.contains("[possible values: bash, zsh, fish]"),
        "positional possible values listed in long mode: {md}"
    );
    assert!(
        md.contains("[default: bash]"),
        "positional default listed in long mode: {md}"
    );
}

#[test]
fn positionals_omit_annotations_in_short_mode() {
    // Short mode (the `-h` path) keeps the Arguments block compact.
    let cmd = Command::new("printer").arg(
        Arg::new("shell")
            .value_parser(["bash", "zsh", "fish"])
            .default_value("bash")
            .help("Shell to emit a completion script for"),
    );
    let md = render_markdown(&cmd, "printer", HelpMode::Short, 100);
    assert!(
        !md.contains("[possible values:"),
        "short mode suppresses possible values: {md}"
    );
    assert!(
        !md.contains("[default:"),
        "short mode suppresses defaults: {md}"
    );
}

#[test]
fn after_help_renders_last_in_both_modes() {
    let cmd = widget().after_help("Short footer.");
    for mode in [HelpMode::Short, HelpMode::Long] {
        let out = render_markdown(&cmd, "widget", mode, 80);
        assert!(out.ends_with("Short footer.\n"), "{out}");
    }
    let cmd = widget().after_help("Short.").after_long_help("Long.");
    assert!(render_markdown(&cmd, "widget", HelpMode::Long, 80).ends_with("Long.\n"));
    assert!(render_markdown(&cmd, "widget", HelpMode::Short, 80).ends_with("Short.\n"));
}

#[test]
fn commands_render_before_options() {
    // Match clap's native section order: Arguments → Commands →
    // Options. The Options block (synthetic `-h, --help` at
    // minimum) sits at the bottom of the page.
    let cmd = Command::new("root").subcommand(Command::new("kid").about("hi"));
    let md = render_markdown(&cmd, "root", HelpMode::Long, 100);
    let cmd_pos = md.find("**Commands:**").expect("Commands block present");
    let opt_pos = md.find("**Options:**").expect("Options block present");
    assert!(cmd_pos < opt_pos, "Commands renders before Options: {md}");
}
