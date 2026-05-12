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
                Command::new("init")
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
                                 (defaults to the running Python's >=major.minor)",
                            ),
                    )
                    .arg(
                        Arg::new("quiet")
                            .long("quiet")
                            .short('q')
                            .action(ArgAction::SetTrue)
                            .help("Suppress informational output"),
                    ),
            )
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
            )
            .subcommand(
                Command::new("manifest")
                    .about("Manage the project's toolr manifest")
                    .subcommand_required(true)
                    .subcommand(
                        Command::new("rebuild")
                            .about("Regenerate the static + dynamic manifest in place"),
                    ),
            ),
    );

    root = root.subcommand(
        Command::new("self")
            .about("Operations on toolr itself")
            .subcommand_required(true)
            .arg_required_else_help(true)
            .subcommand(
                Command::new("build-manifest")
                    .about("Generate a third-party manifest fragment for a package")
                    .arg(
                        Arg::new("package")
                            .required(true)
                            .help("Dotted Python package name to introspect"),
                    )
                    .arg(
                        Arg::new("output")
                            .long("output")
                            .value_name("PATH")
                            .help("Override the output path"),
                    )
                    .arg(
                        Arg::new("python")
                            .long("python")
                            .value_name("PATH")
                            .help("Path to a Python interpreter to use"),
                    )
                    .arg(
                        Arg::new("schema-version")
                            .long("schema-version")
                            .value_name("N")
                            .help("Pin the emitted schema version"),
                    )
                    .arg(
                        Arg::new("check")
                            .long("check")
                            .action(ArgAction::SetTrue)
                            .help("Verify the on-disk manifest matches regeneration"),
                    ),
            )
            .subcommand(
                Command::new("cache")
                    .about("Manage the cache of per-repo virtualenvs")
                    .subcommand_required(true)
                    .arg_required_else_help(true)
                    .subcommand(
                        Command::new("list").about(
                            "List every cached virtualenv with size and last-use timestamp",
                        ),
                    )
                    .subcommand(
                        Command::new("prune")
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
                    ),
            )
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
                    )
                    .subcommand(
                        Command::new("install")
                            .about(
                                "Install the completion script for a shell into its \
                                 standard location",
                            )
                            .arg(
                                Arg::new("shell")
                                    .required(true)
                                    .value_parser(["bash", "zsh", "fish"])
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
        let long_flag = arg.name.replace('_', "-");
        let mut a = Arg::new(arg.name.clone()).help(arg.help.clone());
        match arg.kind {
            ArgumentKind::Positional => {
                a = a.required(true);
            }
            ArgumentKind::Optional => {
                a = a.long(long_flag).required(false);
                if let Some(def) = &arg.default {
                    a = a.default_value(def.clone());
                }
            }
            ArgumentKind::Flag => {
                a = a.long(long_flag).action(ArgAction::SetTrue);
            }
            ArgumentKind::Repeated => {
                // --name VALUE that may repeat; each occurrence appends.
                a = a
                    .long(long_flag)
                    .required(false)
                    .action(ArgAction::Append)
                    .num_args(1);
            }
            ArgumentKind::VarPositional => {
                // Trailing variadic positional. Required=false because
                // zero values is a valid invocation.
                a = a
                    .required(false)
                    .num_args(0..)
                    .trailing_var_arg(true);
            }
        }
        if !arg.allowed_values.is_empty() {
            a = a.value_parser(arg.allowed_values.clone());
        }
        c = c.arg(a);
    }
    c
}
