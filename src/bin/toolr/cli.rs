use std::collections::HashMap;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Arg, ArgAction, Command};

/// Palette for `--help` output. Yellow + bold for section headers and
/// `Usage:`, green for arg names and choice values — closer to the
/// argparse / rich-argparse look the legacy toolr shipped.
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

use _rust_utils::manifest::{ArgumentKind, Group, Manifest};

const RESERVED_GROUPS: &[&str] = &["self", "project"];

fn user_group_collides(name: &str) -> bool {
    RESERVED_GROUPS.contains(&name)
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

/// Recursively build a clap `Command` subtree for `group`, attaching
/// direct commands and child groups discovered via `children`.
fn build_group_subtree(
    group: &Group,
    manifest: &Manifest,
    children: &HashMap<Option<String>, Vec<&Group>>,
) -> Command {
    let full_path = group.full_path();
    let mut g = Command::new(group.name.clone()).about(group.title.clone());
    if !group.description.is_empty() {
        g = g.long_about(group.description.clone());
    }
    for cmd in manifest.commands.iter().filter(|c| c.group == full_path) {
        g = g.subcommand(build_user_command(cmd));
    }
    if let Some(child_groups) = children.get(&Some(full_path)) {
        for child in child_groups {
            g = g.subcommand(build_group_subtree(child, manifest, children));
        }
    }
    g
}

/// Construct the full clap Command tree, given a loaded manifest.
/// User-defined groups appear as top-level subcommands.
pub fn build_command(manifest: &Manifest) -> Command {
    let mut root = Command::new("toolr")
        .version(env!("CARGO_PKG_VERSION"))
        .about("In-project CLI tooling support")
        .styles(help_styles())
        .disable_help_subcommand(true)
        // `--debug` / `--quiet` are root-level options — they go
        // before the subcommand (`toolr --debug ci hello`). They're
        // intentionally *not* `global(true)` so they don't clutter
        // every subcommand's --help output.
        .arg(
            Arg::new("debug")
                .short('d')
                .long("debug")
                .action(ArgAction::SetTrue)
                .help("Increase verbosity"),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .action(ArgAction::SetTrue)
                .conflicts_with("debug")
                .help("Suppress non-error output"),
        );

    let children = children_by_parent(manifest);
    for group in children.get(&None).cloned().unwrap_or_default() {
        if user_group_collides(&group.name) {
            eprintln!(
                "toolr: warning: ignoring user-defined group `{}` — \
                 this name is reserved by toolr itself.",
                group.name
            );
            continue;
        }
        root = root.subcommand(build_group_subtree(group, manifest, &children));
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
    // Run the docstring prose through the markdown→ANSI renderer
    // before handing it to clap. On a non-TTY (piped / captured help
    // text) the renderer returns plain text, so doc-snippet captures
    // remain stable.
    //
    // The split between `about` (short, used in the parent's
    // subcommand listing) and `long_about` (full body, used on the
    // command's own `--help`) is deliberately preserved so parent
    // listings stay compact. The `-h` flag is then re-bound below to
    // trigger the long form so users get the full prose on either
    // flavour of help — matches the argparse-era expectation that
    // `-h` and `--help` show the same thing.
    let summary = crate::markdown::render(&cmd.summary);
    let long_about = if cmd.description.is_empty() {
        summary.clone()
    } else if cmd.summary.is_empty() {
        crate::markdown::render(&cmd.description)
    } else {
        crate::markdown::render(&format!("{}\n\n{}", cmd.summary, cmd.description))
    };
    let mut c = Command::new(cmd.name.clone())
        .about(summary)
        .long_about(long_about)
        .disable_help_flag(true)
        .arg(
            // Both `-h` and `--help` print the long form, since our
            // user-facing docstrings are usually short enough that
            // the "long form" is the right default everywhere.
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::HelpLong)
                .help("Print help"),
        );
    for arg in &cmd.arguments {
        let long_flag = arg.name.replace('_', "-");
        let mut a = Arg::new(arg.name.clone()).help(crate::markdown::render(&arg.help));
        let is_optional_wrapper = matches!(
            arg.resolved_type,
            Some(_rust_utils::parser::SupportedType::Optional(_))
        );
        match arg.kind {
            ArgumentKind::Positional => {
                a = a.required(!is_optional_wrapper);
            }
            ArgumentKind::Optional => {
                a = a.long(long_flag).required(false);
                if let Some(def) = &arg.default {
                    // Empty default means "no observable default at the
                    // CLI" — let Python's function default kick in
                    // (e.g. `param: str | None = None`).
                    if !def.is_empty() {
                        a = a.default_value(def.clone());
                    }
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
            ArgumentKind::Count => {
                // `-v`, `-vv`, `-vvv` → 1 / 2 / 3 via clap's
                // ArgAction::Count. Python receives the resulting int
                // through `toolr.types.Count` (which is `int`).
                a = a.long(long_flag).action(ArgAction::Count);
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
            a = crate::value_parsers::apply_value_parser(a, ty, arg.path_constraints.as_ref());
        } else if !arg.allowed_values.is_empty() {
            a = a.value_parser(arg.allowed_values.clone());
        }
        a = apply_arg_metadata(a, &arg.metadata);
        c = c.arg(a);
    }
    c
}

/// Translate the parser-harvested `ArgMetadata` into clap `Arg` calls.
/// Every field is independently optional — empty / `None` means "leave
/// clap's default behaviour alone."
fn apply_arg_metadata(
    mut a: Arg,
    meta: &_rust_utils::manifest::ArgMetadata,
) -> Arg {
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
        a = a.value_name(name.clone());
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
        a = a.help_heading(section.title.clone());
    }
    a
}
