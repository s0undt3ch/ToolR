use clap::{Arg, ArgAction, Command};

use _rust_utils::manifest::{ArgumentKind, Manifest};

const RESERVED_GROUPS: &[&str] = &["self", "project"];

fn user_group_collides(name: &str) -> bool {
    RESERVED_GROUPS.contains(&name)
}

/// Construct the full clap Command tree, given a loaded manifest.
/// User-defined groups appear as top-level subcommands.
pub fn build_command(manifest: &Manifest) -> Command {
    let mut root = Command::new("toolr")
        .version(env!("CARGO_PKG_VERSION"))
        .about("In-project CLI tooling support")
        .disable_help_subcommand(true)
        .arg(
            Arg::new("debug")
                .short('d')
                .long("debug")
                .action(ArgAction::SetTrue)
                .global(true)
                .help("Increase verbosity"),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .action(ArgAction::SetTrue)
                .global(true)
                .conflicts_with("debug")
                .help("Suppress non-error output"),
        );

    for group in &manifest.groups {
        if user_group_collides(&group.name) {
            eprintln!(
                "toolr: warning: ignoring user-defined group `{}` — \
                 this name is reserved by toolr itself.",
                group.name
            );
            continue;
        }
        let mut g = Command::new(group.name.clone()).about(group.title.clone());
        if !group.description.is_empty() {
            g = g.long_about(group.description.clone());
        }
        for cmd in manifest.commands.iter().filter(|c| c.group == group.name) {
            g = g.subcommand(build_user_command(cmd));
        }
        root = root.subcommand(g);
    }

    root = root.subcommand(
        Command::new("project")
            .about("Operations on the current repo's tools/ directory")
            .subcommand_required(true)
            .subcommand(
                Command::new("deps")
                    .about("Tools-venv dependency management")
                    .subcommand_required(true)
                    .subcommand(Command::new("sync").about("Run `uv sync` against tools/")),
            )
            .subcommand(
                Command::new("venv")
                    .about("Inspect or activate the tools venv")
                    .subcommand_required(true)
                    .subcommand(
                        Command::new("path").about("Print the absolute path to the tools venv"),
                    )
                    .subcommand(
                        Command::new("shell")
                            .about("Spawn a subshell with the tools venv activated"),
                    ),
            ),
    );

    root = root.subcommand(
        Command::new("self")
            .about("Operations on toolr itself")
            .subcommand_required(true)
            .arg_required_else_help(true)
            .subcommand(
                Command::new("completion")
                    .about("Manage shell completion scripts")
                    .subcommand_required(true)
                    .arg_required_else_help(true)
                    .subcommand(
                        Command::new("print")
                            .about("Print the completion script for a shell to stdout")
                            .arg(
                                Arg::new("shell")
                                    .required(true)
                                    .value_parser(["bash", "zsh", "fish"])
                                    .help("Shell to emit a completion script for"),
                            ),
                    ),
            ),
    );

    root = root.subcommand(
        Command::new("__build-static-manifest")
            .hide(true)
            .about("(internal) Regenerate the static manifest in place"),
    );

    root = root.subcommand(
        Command::new("__complete")
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
    );

    root = root.subcommand(
        Command::new("__install-uv-now")
            .hide(true)
            .about("(internal) Force-install toolr-managed uv now"),
    );

    root
}

fn build_user_command(cmd: &_rust_utils::manifest::Command) -> Command {
    let mut c = Command::new(cmd.name.clone()).about(cmd.summary.clone());
    if !cmd.description.is_empty() {
        c = c.long_about(cmd.description.clone());
    }
    for arg in &cmd.arguments {
        let mut a = Arg::new(arg.name.clone()).help(arg.help.clone());
        match arg.kind {
            ArgumentKind::Positional => {
                a = a.required(true);
            }
            ArgumentKind::Optional => {
                a = a.long(arg.name.clone()).required(false);
                if let Some(def) = &arg.default {
                    a = a.default_value(def.clone());
                }
            }
            ArgumentKind::Flag => {
                a = a.long(arg.name.clone()).action(ArgAction::SetTrue);
            }
        }
        if !arg.allowed_values.is_empty() {
            a = a.value_parser(arg.allowed_values.clone());
        }
        c = c.arg(a);
    }
    c
}
