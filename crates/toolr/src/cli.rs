use std::collections::HashMap;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::builder::{PossibleValue, PossibleValuesParser};
use clap::{Arg, ArgAction, Command, ValueHint};

/// Palette for `--help` output. Yellow + bold for section headers and
/// `Usage:`, green for arg names and choice values.
fn help_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .usage(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .literal(AnsiColor::Green.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Green.on_default())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Red.on_default())
        .error(AnsiColor::Red.on_default() | Effects::BOLD)
}

use toolr_core::manifest::{ArgumentKind, Group, Manifest};
use toolr_core::text::strip_control_chars_per_line;

/// Drop every user group (and its commands) rooted at the name of a built-in command, hidden ones
/// included (clap panics on a duplicated subcommand name). Returns the dropped top-level group
/// names so [`build_command`] can warn; completion calls it silently, so it offers what a run accepts.
pub(crate) fn drop_reserved_user_groups(manifest: &mut Manifest) -> Vec<String> {
    let reserved = |path: &str| {
        let top = path.split('.').next().unwrap_or(path);
        builtin_names().iter().any(|b| b == top)
    };
    let dropped = manifest
        .groups
        .iter()
        .filter(|g| g.parent.is_none() && reserved(&g.name))
        .map(|g| g.name.clone())
        .collect();
    manifest.groups.retain(|g| !reserved(&g.full_path()));
    manifest.commands.retain(|c| !reserved(&c.group));
    dropped
}

/// Index the manifest's groups by their parent's `full_path()`. Top-level
/// groups (no parent) live under the `None` key.
fn children_by_parent(manifest: &Manifest) -> HashMap<Option<String>, Vec<&Group>> {
    let mut map: HashMap<Option<String>, Vec<&Group>> = HashMap::new();
    for g in &manifest.groups {
        map.entry(g.parent.clone()).or_default().push(g);
    }
    map
}

/// Compute the dotted name a dispatcher is addressable by from the
/// CLI. Mirrors `toolr_core::parser::build::dotted_name`: a command
/// whose `name` matches the leaf segment of its `group` is addressable
/// as the group path itself; otherwise it's `"<group>.<name>"` (or
/// just `name` when the group is empty). The argparse pipeline sets
/// each grafted child's `group` field to this dotted name (the
/// attachment.parent), so we use the same value to look children up.
fn dispatcher_dotted_name(cmd: &toolr_core::manifest::Command) -> String {
    let leaf = cmd.group.rsplit('.').next().unwrap_or(cmd.group.as_str());
    if !cmd.group.is_empty() && cmd.name == leaf {
        cmd.group.clone()
    } else if cmd.group.is_empty() {
        cmd.name.clone()
    } else {
        format!("{}.{}", cmd.group, cmd.name)
    }
}

/// Recursively build a clap `Command` subtree for `group`, attaching
/// direct commands and child groups discovered via `children`.
fn build_group_subtree(
    group: &Group,
    manifest: &Manifest,
    children: &HashMap<Option<String>, Vec<&Group>>,
) -> Command {
    let full_path = group.full_path();
    let mut g = Command::new(group.name.clone())
        .disable_help_flag(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .about(strip_control_chars_per_line(&group.title));
    if !group.description.is_empty() {
        g = g.long_about(strip_control_chars_per_line(&group.description));
    }

    // For each NON-grafted command in this group, decide whether to
    // build it as a dispatcher (own args + the children-bucket as
    // subcommands) or as a normal leaf. Grafted children live under
    // the dispatcher's dotted name (set by `graft_children` from the
    // `[[attach]] parent = "..."` value).
    //
    // When the dispatcher's name matches the group's leaf segment
    // (e.g. `command_group("django")` + `def django(...)`), its dotted
    // name equals the group path. In that case its grafted children
    // are hoisted directly onto the group itself, so the user types
    // `toolr django migrate` rather than `toolr django django migrate`.
    let group_leaf = full_path.rsplit('.').next().unwrap_or(full_path.as_str());
    for cmd in manifest
        .commands
        .iter()
        .filter(|c| c.group == full_path && c.dispatched_from.is_none())
    {
        if cmd.is_dispatcher {
            let dotted = dispatcher_dotted_name(cmd);
            let dispatched_children: Vec<&toolr_core::manifest::Command> = manifest
                .commands
                .iter()
                .filter(|child| child.group == dotted && child.dispatched_from.is_some())
                .collect();
            if cmd.name == group_leaf {
                // Hoist: the dispatcher's children become direct
                // subcommands of the group, and the dispatcher itself
                // disappears as a redundant CLI hop.
                for child in &dispatched_children {
                    g = g.subcommand(build_user_command(child));
                }
            } else {
                g = g.subcommand(build_dispatcher_command(cmd, &dispatched_children));
            }
        } else {
            g = g.subcommand(build_user_command(cmd));
        }
    }

    if let Some(child_groups) = children.get(&Some(full_path)) {
        for child in child_groups {
            g = g.subcommand(build_group_subtree(child, manifest, children));
        }
    }
    g
}

fn build_dispatcher_command(
    dispatcher: &toolr_core::manifest::Command,
    children: &[&toolr_core::manifest::Command],
) -> Command {
    let mut c = build_user_command(dispatcher).subcommand_required(true);
    for child in children {
        c = c.subcommand(build_user_command(child));
    }
    c
}

// These flags tweak how toolr's own output renders *and* how `ctx.run(...)` subprocesses
// behave by default (timeouts, output watchdog); per-call `ctx.run(timeout_secs=)` /
// `no_output_timeout_secs=` arguments still override the latter. They belong to toolr, so
// they are accepted only at the root, before the first subcommand: `toolr --debug ci hello`.
pub(crate) const ROOT_HEADING: &str = "ToolR Options";

/// Printed under [`ROOT_HEADING`] in root help, so the placement rule sits next to the flags.
pub(crate) const ROOT_HEADING_NOTE: &str =
    "toolr's options go before the command: toolr --debug <command> ...";

// Root help only: an every-page footer was removed for costing a line on every help page.
const ROOT_AFTER_HELP: &str = "Documentation: https://toolr.readthedocs.io/\n\
     For AI agents: https://toolr.readthedocs.io/llms.txt";

/// The root `toolr` command: its output flags and the combined `-h`/`--help` arg.
fn build_root_command() -> Command {
    Command::new("toolr")
        // Without this, clap infers the usage-string binary name from
        // `argv[0]` — `toolr.exe` on Windows vs `toolr` elsewhere — so
        // help output (and tests asserting on it) would differ by platform.
        .bin_name("toolr")
        .version(env!("CARGO_PKG_VERSION"))
        .disable_version_flag(true)
        .about("In-project CLI tooling support")
        .after_help(ROOT_AFTER_HELP)
        .styles(help_styles())
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .args(root_flag_args())
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::SetTrue)
                // Help only: the other root flags stay root-only.
                .global(true)
                .help("Print help"),
        )
        // Declared explicitly: clap adds its own `-V` only when it builds the
        // command, after the help renderer has listed the arguments.
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help("Print version"),
        )
}

/// The root command without subcommands, built once: `root_flag` and `RootArgv` read its args.
pub(crate) fn root_command() -> &'static Command {
    static ROOT: std::sync::OnceLock<Command> = std::sync::OnceLock::new();
    ROOT.get_or_init(build_root_command)
}

/// The root flag `spelling` names (`-q`, `--timeout-secs`, an alias such as `--timeout`).
/// Sitting under `ROOT_HEADING` is what makes a root arg one of these flags: one without
/// the heading is never hinted.
fn root_flag(spelling: &str) -> Option<&'static Arg> {
    root_command().get_arguments().find(|a| {
        a.get_help_heading() == Some(ROOT_HEADING) && spellings(a).iter().any(|s| s == spelling)
    })
}

/// Every spelling of `arg`: `--long`, `--alias` and `-s`. The one enumeration the root-flag
/// scanners share.
pub(crate) fn spellings(arg: &Arg) -> Vec<String> {
    let longs = arg
        .get_long()
        .into_iter()
        .chain(arg.get_all_aliases().unwrap_or_default());
    longs
        .map(|l| format!("--{l}"))
        .chain(arg.get_short().map(|c| format!("-{c}")))
        .collect()
}

/// The root flag a command-line token names, as a spelling: `--timeout-secs=5` names
/// `--timeout-secs`, a short cluster of root letters only (`-qd`) names its first letter, and
/// anything else, such as `-qx` or `-dev`, names nothing.
fn root_flag_spelling(word: &str) -> Option<String> {
    if let Some(long) = word.strip_prefix("--") {
        let spelling = format!("--{}", long.split('=').next()?);
        return root_flag(&spelling).map(|_| spelling);
    }
    let letters = word.strip_prefix('-').filter(|l| !l.is_empty())?;
    letters
        .chars()
        .all(|c| root_flag(&format!("-{c}")).is_some())
        .then(|| format!("-{}", letters.chars().next().unwrap_or_default()))
}

/// A toolr root flag found where it does not belong, with the line telling where it goes.
pub(crate) struct Misplaced {
    spelling: String,
    pub(crate) hint: String,
}

impl Misplaced {
    /// The clap-style error and the hint, for callers that raise the error themselves.
    pub(crate) fn message(&self) -> String {
        format!(
            "unexpected argument '{}' found\n{}",
            self.spelling, self.hint
        )
    }
}

/// Characters that can be pasted into a shell without quoting. A value is echoed in the
/// example only when it is made of these, and does not start with `-` (it would read as a flag).
const SHELL_SAFE: &str = "_.,:/@%+-";

/// The root flag that `words[0]` names after the command `path`, if it is one. The example
/// puts the flag in front of `path`, with `...` for everything else; when root flags were
/// already typed before the command (`typed_before`) a literal `…` stands for them. The flag's
/// value is shown only when plain (`--x=v`, or the next word), otherwise its placeholder.
pub(crate) fn misplaced_flag_error(
    words: &[&str],
    path: &str,
    typed_before: bool,
) -> Option<Misplaced> {
    let spelling = root_flag_spelling(words.first()?)?;
    let flag = root_flag(&spelling)?;
    let plain = |v: &str| {
        !v.is_empty()
            && !v.starts_with('-')
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || SHELL_SAFE.contains(c))
    };
    let shown = match words {
        _ if !flag.get_action().takes_values() => spelling.clone(),
        [w, ..] if w.starts_with(&format!("{spelling}=")) && plain(&w[spelling.len() + 1..]) => {
            (*w).to_string()
        }
        [w, v, ..] if *w == spelling && plain(v) => format!("{spelling} {v}"),
        _ => {
            let name = flag
                .get_value_names()
                .and_then(|n| n.first())
                .map_or("VALUE", |n| n.as_str());
            format!("{spelling} <{name}>")
        }
    };
    let lead = if typed_before { "\u{2026} " } else { "" };
    let path = if path.is_empty() {
        String::new()
    } else {
        format!(" {path}")
    };
    let hint = format!(
        "toolr: `{spelling}` is a toolr option; put it before the command: `toolr {lead}{shown}{path} ...`"
    );
    Some(Misplaced { spelling, hint })
}

/// The `-d` / `-q` / timing flags. Root-only: a copy on a command would let a Python
/// parameter of the same name or short collide with toolr's own flag. A new one also needs a
/// row in docs/cli.md and a read in `execute_build::output_options_from_matches`.
fn root_flag_args() -> Vec<Arg> {
    vec![
        Arg::new("debug")
            .short('d')
            .long("debug")
            .action(ArgAction::SetTrue)
            .help_heading(ROOT_HEADING)
            .help("Increase verbosity (also enables `DEBUG` logging)."),
        // The `-q` rule, stated once: it drops success lines, progress and advice, never
        // errors, refusals or warnings. A warning means the result may be incomplete, and
        // a script that hides it can't tell. Other code points here instead of restating it.
        Arg::new("quiet")
            .short('q')
            .long("quiet")
            .action(ArgAction::SetTrue)
            .conflicts_with("debug")
            .help_heading(ROOT_HEADING)
            .help("Suppress non-error output except warnings, and never prompt to install uv."),
        Arg::new("timestamps")
            .long("timestamps")
            .alias("ts")
            .action(ArgAction::SetTrue)
            .conflicts_with("no-timestamps")
            .help_heading(ROOT_HEADING)
            .help("Prepend ISO-8601 timestamps to log lines."),
        Arg::new("no-timestamps")
            .long("no-timestamps")
            .alias("nts")
            .action(ArgAction::SetTrue)
            .help_heading(ROOT_HEADING)
            .help("Suppress log-line timestamps (default; overrides `--timestamps`)."),
        Arg::new("timeout-secs")
            .long("timeout-secs")
            .alias("timeout")
            .value_name("SECONDS")
            .value_parser(clap::value_parser!(f64))
            .help_heading(ROOT_HEADING)
            .help(
                "Default timeout applied to every `ctx.run(...)` subprocess \
                 (per-call `timeout_secs=` wins when set).",
            ),
        Arg::new("no-output-timeout-secs")
            .long("no-output-timeout-secs")
            .alias("nots")
            .value_name("SECONDS")
            .value_parser(clap::value_parser!(f64))
            .help_heading(ROOT_HEADING)
            .help(
                "Default no-output watchdog applied to every `ctx.run(...)` \
                 subprocess — abort if no stdout/stderr for this many \
                 seconds. Per-call `no_output_timeout_secs=` wins when set.",
            ),
    ]
}

/// `toolr project init`.
fn build_project_init_subcommand() -> Command {
    Command::new("init")
        .disable_help_flag(true)
        .about("Scaffold tools/ in the current directory")
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Overwrite an existing tools/ directory"),
        )
        .arg(
            Arg::new("no-sync")
                .long("no-sync")
                .action(ArgAction::SetTrue)
                .help("Skip the automatic `uv sync` after scaffolding"),
        )
        .arg(
            Arg::new("venv-location")
                .long("venv-location")
                .value_name("LOCATION")
                .value_parser(["cache", "in-tree"])
                .default_value("cache")
                .help("Where the tools venv should live"),
        )
        .arg(
            Arg::new("no-example")
                .long("no-example")
                .action(ArgAction::SetTrue)
                .help("Skip generating tools/example.py"),
        )
        .arg(
            Arg::new("python")
                .long("python")
                .value_name("VERSION")
                .help(
                    "`requires-python` value for tools/pyproject.toml \
                     (defaults to python3's >=major.minor, never below the \
                     oldest Python toolr supports)",
                ),
        )
        .arg(
            Arg::new("yes")
                .long("yes")
                .short('y')
                .action(ArgAction::SetTrue)
                .help("Auto-approve overwriting any conflicting scaffold files"),
        )
}

/// `toolr project venv` and its subcommands.
fn build_project_venv_subcommand() -> Command {
    Command::new("venv")
        .disable_help_flag(true)
        .about("Inspect, sync, and operate on the tools venv")
        .subcommand_required(true)
        .subcommand(
            Command::new("path")
                .disable_help_flag(true)
                .about("Print the absolute path to the tools venv"),
        )
        .subcommand(
            Command::new("shell")
                .disable_help_flag(true)
                .about("Spawn a subshell with the tools venv activated"),
        )
        .subcommand(
            Command::new("run")
                .disable_help_flag(true)
                .about("Run a command inside the managed tools venv (auto-syncs first; use --no-sync in CI)")
                .arg(
                    Arg::new("no-sync")
                        .long("no-sync")
                        .action(ArgAction::SetTrue)
                        .help("Do not sync first; run against the existing venv and error if it is missing or stale (CI-deterministic)"),
                )
                .arg(
                    Arg::new("command")
                        .value_name("CMD")
                        .required(true)
                        .num_args(1..)
                        .trailing_var_arg(true)
                        .allow_hyphen_values(true)
                        .help("Command to run in the venv, e.g. `pytest tests/`. toolr's own flags must come before the command (or a `--`)."),
                ),
        )
        .subcommand(
            Command::new("sync")
                .disable_help_flag(true)
                .about("Sync the tools venv (no-op unless tools/pyproject.toml, uv.lock, uv.toml or .python-version changed since the last sync; an in-tree venv toolr didn't build is removed and rebuilt)")
                .arg(
                    Arg::new("if-project")
                        .long("if-project")
                        .action(ArgAction::SetTrue)
                        .help("Exit 0, silently, when there is nothing to sync: not in a toolr project, no readable venv location, or uv absent and not installable without asking (for shell and editor hooks)"),
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .short('f')
                        .action(ArgAction::SetTrue)
                        .help("Re-run `uv sync` even when none of those files changed (e.g. after changing UV_* environment variables or user-level uv config)"),
                )
                .arg(
                    Arg::new("upgrade")
                        .long("upgrade")
                        .short('U')
                        .action(ArgAction::SetTrue)
                        .help("Re-resolve every package (passes --upgrade to uv). Combine with -P to also force specific packages."),
                )
                .arg(
                    Arg::new("upgrade-package")
                        .long("upgrade-package")
                        .short('P')
                        .value_name("PACKAGE")
                        .action(ArgAction::Append)
                        .help("Re-resolve a single package; pass repeatedly for multiple. Each <PACKAGE> must be declared in tools/pyproject.toml."),
                ),
        )
        .subcommand(
            Command::new("lock")
                .disable_help_flag(true)
                .about("Refresh tools/uv.lock without applying (wraps `uv lock`)")
                .arg(
                    Arg::new("upgrade")
                        .long("upgrade")
                        .short('U')
                        .action(ArgAction::SetTrue)
                        .help("Re-resolve every package (--upgrade)"),
                )
                .arg(
                    Arg::new("upgrade-package")
                        .long("upgrade-package")
                        .short('P')
                        .value_name("PACKAGE")
                        .action(ArgAction::Append)
                        .help("Re-resolve a single package; pass repeatedly for multiple"),
                ),
        )
        .subcommand(
            Command::new("add")
                .disable_help_flag(true)
                .about("Add one or more packages to tools/pyproject.toml (wraps `uv add`)")
                .arg(
                    Arg::new("packages")
                        .value_name("PACKAGE")
                        .num_args(1..)
                        .required(true)
                        .help("Package spec (`name`, `name@version`, `name>=1.2`, …) — passed through to uv"),
                ),
        )
        .subcommand(
            Command::new("remove")
                .disable_help_flag(true)
                .about("Remove one or more packages from tools/pyproject.toml (wraps `uv remove`)")
                .arg(
                    Arg::new("packages")
                        .value_name("PACKAGE")
                        .num_args(1..)
                        .required(true)
                        .help("Package name to remove (must already appear in tools/pyproject.toml)"),
                ),
        )
}

/// The `toolr project` subtree.
fn build_project_subcommand() -> Command {
    Command::new("project")
        .disable_help_flag(true)
        .about("Operations on the current repo's tools/ directory")
        .subcommand_required(true)
        .subcommand(build_project_init_subcommand())
        .subcommand(build_project_venv_subcommand())
        .subcommand(
            Command::new("manifest")
                .disable_help_flag(true)
                .about("Manage the project's toolr manifest")
                .subcommand_required(true)
                .subcommand(
                    Command::new("rebuild")
                        .disable_help_flag(true)
                        .about("Regenerate the static + dynamic manifest in place"),
                ),
        )
        .subcommand(
            Command::new("trust")
                .disable_help_flag(true)
                .about("Allow toolr to run this repository's code, then sync its tools venv")
                .arg(
                    Arg::new("path")
                        .value_name("PATH")
                        .value_parser(clap::value_parser!(std::path::PathBuf))
                        .help("A path inside the repository (defaults to the current directory)"),
                )
                .arg(
                    Arg::new("no-sync")
                        .long("no-sync")
                        .action(ArgAction::SetTrue)
                        .help("Only record the trust; don't offer to sync the tools venv"),
                ),
        )
        .subcommand(
            Command::new("untrust")
                .disable_help_flag(true)
                .about("Revoke trust in this repository")
                .arg(
                    Arg::new("path")
                        .value_name("PATH")
                        .value_parser(clap::value_parser!(std::path::PathBuf))
                        .help("A path inside the repository (defaults to the current directory)"),
                ),
        )
}

/// `toolr self build-manifest`.
fn build_self_build_manifest_subcommand() -> Command {
    Command::new("build-manifest")
        .disable_help_flag(true)
        .about("Generate a third-party manifest fragment for a package")
        .arg(
            Arg::new("package_positional")
                .value_name("PACKAGE")
                .required(false)
                .conflicts_with("source-dir")
                .help("Dotted Python package name to introspect (looked up in the tools venv)"),
        )
        .arg(
            Arg::new("source-dir")
                .long("source-dir")
                .value_name("PATH")
                .value_hint(ValueHint::DirPath)
                .conflicts_with("package_positional")
                .help("Path to the package's source tree (bypasses the tools-venv lookup)"),
        )
        .arg(
            Arg::new("package")
                .long("package")
                .value_name("PKG")
                .requires("source-dir")
                .help(
                    "Package name to embed in the fragment when using --source-dir \
                     (defaults to the leaf directory name)",
                ),
        )
        .arg(
            Arg::new("output")
                .long("output")
                .value_name("PATH")
                .value_hint(ValueHint::FilePath)
                .help("Override the output path"),
        )
        .arg(
            Arg::new("check")
                .long("check")
                .action(ArgAction::SetTrue)
                .help("Verify the on-disk manifest matches regeneration"),
        )
}

/// `toolr self cache` and its subcommands.
fn build_self_cache_subcommand() -> Command {
    Command::new("cache")
        .disable_help_flag(true)
        .about("Manage the cache of per-repo virtualenvs")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("list")
                .disable_help_flag(true)
                .about("List every cached virtualenv with size and last-use timestamp"),
        )
        .subcommand(
            Command::new("prune")
                .disable_help_flag(true)
                .about("Remove orphan and stale cache entries")
                .arg(
                    Arg::new("all")
                        .long("all")
                        .action(ArgAction::SetTrue)
                        .help("Remove every cache entry"),
                )
                .arg(
                    Arg::new("stale-after-days")
                        .long("stale-after-days")
                        .value_name("DAYS")
                        .default_value("30")
                        .value_parser(clap::value_parser!(u32))
                        .help("Override the staleness threshold"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(ArgAction::SetTrue)
                        .help("Show what would be deleted without deleting"),
                )
                .arg(
                    Arg::new("yes")
                        .long("yes")
                        .short('y')
                        .action(ArgAction::SetTrue)
                        .help("Skip confirmation when used with --all"),
                ),
        )
}

/// Shells `self completion print|install` accept; `pwsh` is an alias, so `--help` lists one name.
fn shell_values() -> PossibleValuesParser {
    PossibleValuesParser::new([
        PossibleValue::new("bash"),
        PossibleValue::new("zsh"),
        PossibleValue::new("fish"),
        PossibleValue::new("powershell").alias("pwsh"),
    ])
}

/// `toolr self completion` and its subcommands.
fn build_self_completion_subcommand() -> Command {
    Command::new("completion")
        .disable_help_flag(true)
        .about("Manage shell completion scripts")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("print")
                .disable_help_flag(true)
                .about("Print the completion script for a shell to stdout")
                .arg(
                    Arg::new("shell")
                        .required(true)
                        .value_parser(shell_values())
                        .help("Shell to emit a completion script for"),
                ),
        )
        .subcommand(
            Command::new("install")
                .disable_help_flag(true)
                .about(
                    "Install the completion script for a shell into its \
                     standard location",
                )
                .arg(
                    Arg::new("shell")
                        .required(true)
                        .value_parser(shell_values())
                        .help("Shell to install the completion script for"),
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .action(ArgAction::SetTrue)
                        .help(
                            "Overwrite an existing differing file \
                             without prompting",
                        ),
                ),
        )
}

/// The `toolr self` subtree.
fn build_self_subcommand() -> Command {
    Command::new("self")
        .disable_help_flag(true)
        .about("Operations on toolr itself")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(build_self_build_manifest_subcommand())
        .subcommand(build_self_cache_subcommand())
        .subcommand(
            Command::new("trusted")
                .disable_help_flag(true)
                .about("List the repositories toolr trusts to run code")
                .arg(
                    Arg::new("prune")
                        .long("prune")
                        .action(ArgAction::SetTrue)
                        .help("Forget records whose repository no longer exists"),
                ),
        )
        .subcommand(build_self_completion_subcommand())
}

/// The hidden internal commands, in the order they join the root.
fn build_hidden_subcommands() -> [Command; 2] {
    [
        Command::new("__complete")
            .disable_help_flag(true)
            .hide(true)
            .about("(internal) Emit completion candidates for the shell scripts")
            .arg(
                Arg::new("cwd")
                    .required(true)
                    .help("Absolute path of the shell's working directory at Tab time"),
            )
            .arg(
                Arg::new("args")
                    .num_args(0..)
                    .trailing_var_arg(true)
                    .allow_hyphen_values(true)
                    .help("The user's argv minus the leading `toolr`"),
            ),
        Command::new("__install-uv-now")
            .disable_help_flag(true)
            .hide(true)
            .about("(internal) Force-install toolr-managed uv now"),
    ]
}

/// Construct the full clap Command tree, given a loaded manifest.
/// User-defined groups appear as top-level subcommands.
pub fn build_command(manifest: &Manifest) -> Command {
    let mut root = build_root_command();

    let mut manifest = manifest.clone();
    for name in drop_reserved_user_groups(&mut manifest) {
        eprintln!(
            "toolr: warning: ignoring user-defined group `{name}` — \
             this name is reserved by toolr itself."
        );
    }
    let manifest = &manifest;
    let children = children_by_parent(manifest);
    for group in children.get(&None).cloned().unwrap_or_default() {
        root = root.subcommand(build_group_subtree(group, manifest, &children));
    }

    root.subcommands(builtin_subcommands())
}

/// toolr's own top-level commands, hidden ones included, in the order they join the root.
fn builtin_subcommands() -> Vec<Command> {
    let mut builtins = vec![build_project_subcommand(), build_self_subcommand()];
    builtins.extend(build_hidden_subcommands());
    builtins
}

/// The names of [`builtin_subcommands`]: a first word that is none of these names a user command.
pub(crate) fn builtin_names() -> &'static [String] {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        builtin_subcommands()
            .iter()
            .map(|c| c.get_name().to_owned())
            .collect()
    })
}

/// The literal default clap gets for `arg`, if any. Only an `Optional`
/// flag gets one. An empty default means "no observable default at the
/// CLI", so Python's function default runs (e.g. `param: str | None =
/// None`). `<expr>` is toolr-core's sentinel for a default it couldn't
/// resolve to a literal (e.g. `Path("~/projects")`, `uuid.uuid4()`);
/// applying it would ship the string `"<expr>"` itself, so Python's own
/// default expression runs instead. A default with control or invisible
/// characters is dropped rather than stripped: the stripped form could
/// fall outside the filtered choices and make clap reject its own default.
pub(crate) fn literal_cli_default(arg: &toolr_core::manifest::Argument) -> Option<&str> {
    let def = arg.default.as_deref()?;
    let usable = arg.kind == ArgumentKind::Optional
        && !def.is_empty()
        && def != "<expr>"
        && !toolr_core::text::has_unsafe_name_chars(def);
    usable.then_some(def)
}

pub(crate) fn build_user_command(cmd: &toolr_core::manifest::Command) -> Command {
    // Run the docstring prose through the markdown→ANSI renderer
    // before handing it to clap. On a non-TTY (piped / captured help
    // text) the renderer returns plain text, so doc-snippet captures
    // remain stable.
    //
    // ``cmd.summary`` is the docstring's first paragraph (clap's
    // ``about``, shown in the parent listing and on ``-h``).
    // ``cmd.description`` is the Rust-rendered ``full_description``
    // (clap's ``long_about``, shown on ``--help``) — already includes
    // the leading summary plus the long body plus any Examples /
    // Notes / Warnings sections, so we hand it to clap as-is. The
    // ``Args:`` section never appears here because the parser folds
    // it into per-argument help instead.
    let summary = strip_control_chars_per_line(&cmd.summary);
    let long_about = if cmd.description.is_empty() {
        summary.clone()
    } else {
        strip_control_chars_per_line(&cmd.description)
    };
    let mut c = Command::new(cmd.name.clone())
        .disable_help_flag(true)
        .about(summary)
        .long_about(long_about);
    for arg in &cmd.arguments {
        let long_flag = arg.name.replace('_', "-");
        let mut a = Arg::new(arg.name.clone()).help(strip_control_chars_per_line(&arg.help));
        let is_optional_wrapper = matches!(
            arg.resolved_type,
            Some(toolr_core::parser::SupportedType::Optional(_))
        );
        match arg.kind {
            ArgumentKind::Positional => {
                a = a.required(!is_optional_wrapper);
            }
            ArgumentKind::Optional => {
                a = a.long(long_flag).required(false);
                if let Some(def) = literal_cli_default(arg) {
                    a = a.default_value(def.to_string());
                }
            }
            ArgumentKind::Flag => {
                a = a.long(long_flag).action(ArgAction::SetTrue);
            }
            ArgumentKind::Repeated => {
                // --name VALUE that may repeat; each occurrence appends.
                // `nargs == Some(Plus | Star)` (argparse `nargs="+"`/
                // `"*"`) additionally lets one occurrence take several
                // space-separated values — widening `num_args` beyond 1
                // is unsafe by default since a following positional
                // would get swallowed, so it's opt-in per argument
                // rather than blanket for the kind.
                a = a.long(long_flag).required(false).action(ArgAction::Append);
                a = match arg.metadata.nargs {
                    Some(toolr_core::manifest::Nargs::Plus)
                    | Some(toolr_core::manifest::Nargs::Star) => a.num_args(1..),
                    _ => a.num_args(1),
                };
            }
            ArgumentKind::VarPositional => {
                // Trailing variadic positional. Zero values is valid by
                // default (native `*args`, argparse `nargs="*"` or
                // `argparse.REMAINDER`); `nargs == Some(Plus)` (argparse
                // `nargs="+"`) demands one or more instead.
                a = if arg.metadata.nargs == Some(toolr_core::manifest::Nargs::Plus) {
                    a.required(true).num_args(1..)
                } else {
                    a.required(false).num_args(0..)
                };
                a = a.trailing_var_arg(true);
            }
            ArgumentKind::Count => {
                // `-v`, `-vv`, `-vvv` → 1 / 2 / 3 via clap's
                // ArgAction::Count. Python receives the resulting int
                // through `toolr.types.Count` (which is `int`).
                a = a.long(long_flag).action(ArgAction::Count);
            }
            ArgumentKind::FixedArity => {
                // Exactly N values in a single occurrence (argparse
                // `nargs=N`). Keyword-style (`--pair a b`) when the
                // scanner recorded a literal flag spelling; positional
                // (`files a b`, no flag, always required) otherwise —
                // mirrors the Positional/Optional split used everywhere
                // else in this function.
                let arity = match arg.metadata.nargs {
                    Some(toolr_core::manifest::Nargs::Fixed(n)) => n,
                    _ => unreachable!("FixedArity always carries Nargs::Fixed(n)"),
                };
                a = if arg.long_flag.is_some() {
                    a.long(long_flag).required(false)
                } else {
                    a.required(true)
                };
                a = a.num_args(arity);
            }
            ArgumentKind::OptionalPositional => {
                // Zero-or-one positional value (argparse positional
                // `nargs="?"`). No flag, not required — unlike
                // `Positional`, which this function always marks
                // required unless `resolved_type` says `T | None`
                // (which argparse-scanned args never carry).
                a = a.required(false);
            }
        }
        // Apply the per-type value_parser when we have structured type
        // info (preferred path). Fall back to the legacy
        // `allowed_values` list for third-party manifest fragments that
        // haven't been rebuilt against the new schema yet.
        if let Some(ty) = arg.resolved_type.as_ref() {
            // Heterogeneous tuples need clap to consume a fixed slot
            // count; per-slot coercion happens on the python side via
            // msgspec against the function's `tuple[T1, T2]` hint.
            if let Some(arity) = crate::value_parsers::tuple_arity(ty) {
                a = a.num_args(arity);
            }
            a = crate::value_parsers::apply_value_parser(a, ty);
            // A relative default names a path under the repo root. The
            // unchecked parser turns off clap's check for every value of this
            // arg, typed and env-var ones too; `execute_build` resolves each
            // after parsing, against the cwd or the repo root by its source.
            if crate::value_parsers::deferred_path(arg).is_some() {
                a = a.value_parser(clap::value_parser!(std::path::PathBuf));
            }
        } else if !arg.allowed_values.is_empty() {
            a = a.value_parser(crate::value_parsers::printable_choices(&arg.allowed_values));
        }
        a = apply_arg_metadata(a, &arg.metadata);
        c = c.arg(a);
    }
    c
}

/// Translate the parser-harvested `ArgMetadata` into clap `Arg` calls.
/// Every field is independently optional — empty / `None` means "leave
/// clap's default behaviour alone."
fn apply_arg_metadata(mut a: Arg, meta: &toolr_core::manifest::ArgMetadata) -> Arg {
    for alias in &meta.aliases {
        if alias.is_empty() {
            continue;
        }
        // Single-char entries become clap shorts (`-v`); longer entries
        // become long aliases (`--also-this`).
        let stripped = alias.trim_start_matches('-');
        if stripped.chars().count() == 1 {
            if let Some(c) = stripped.chars().next() {
                a = a.short(c);
            }
        } else if !stripped.is_empty() {
            a = a.alias(stripped.to_string());
        }
    }
    if let Some(name) = &meta.metavar {
        a = a.value_name(strip_control_chars_per_line(name));
    }
    if let Some(env) = &meta.env {
        a = a.env(env);
    }
    if meta.hide {
        a = a.hide(true);
    }
    if let Some(order) = meta.display_order {
        a = a.display_order(order as usize);
    }
    if !meta.conflicts_with.is_empty() {
        a = a.conflicts_with_all(meta.conflicts_with.clone());
    }
    if !meta.requires.is_empty() {
        a = a.requires_all(meta.requires.clone());
    }
    if let Some(section) = &meta.help_section {
        a = a.help_heading(strip_control_chars_per_line(&section.title));
    }
    a
}

#[cfg(test)]
mod tests;
