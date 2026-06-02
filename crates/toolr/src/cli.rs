use std::collections::HashMap;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Arg, ArgAction, Command};

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
    let mut g = Command::new(group.name.clone()).about(group.title.clone());
    if !group.description.is_empty() {
        g = g.long_about(group.description.clone());
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

/// Construct the full clap Command tree, given a loaded manifest.
/// User-defined groups appear as top-level subcommands.
pub fn build_command(manifest: &Manifest) -> Command {
    // All root-level "output" flags live under a single `--help` heading
    // so users see them as a coherent block. They tweak how toolr's own
    // output renders *and* how `ctx.run(...)` subprocesses behave by
    // default (timeouts, output watchdog). Per-call `ctx.run(timeout_secs=)`
    // / `no_output_timeout_secs=` arguments still override these.
    const OUTPUT_HEADING: &str = "Output Options";
    let mut root = Command::new("toolr")
        .version(env!("CARGO_PKG_VERSION"))
        .about("In-project CLI tooling support")
        .styles(help_styles())
        .disable_help_subcommand(true)
        // `--debug` / `--quiet` and the new timing flags are root-level
        // options — they go before the subcommand (`toolr --debug ci
        // hello`). They're intentionally *not* `global(true)` so they
        // don't clutter every subcommand's --help output.
        .next_help_heading(OUTPUT_HEADING)
        .arg(
            Arg::new("debug")
                .short('d')
                .long("debug")
                .action(ArgAction::SetTrue)
                .help_heading(OUTPUT_HEADING)
                .help(crate::markdown::render(
                    "Increase verbosity (also enables `DEBUG` logging).",
                )),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .action(ArgAction::SetTrue)
                .conflicts_with("debug")
                .help_heading(OUTPUT_HEADING)
                .help(crate::markdown::render("Suppress non-error output.")),
        )
        .arg(
            Arg::new("timestamps")
                .long("timestamps")
                .alias("ts")
                .action(ArgAction::SetTrue)
                .conflicts_with("no-timestamps")
                .help_heading(OUTPUT_HEADING)
                .help(crate::markdown::render(
                    "Prepend ISO-8601 timestamps to log lines.",
                )),
        )
        .arg(
            Arg::new("no-timestamps")
                .long("no-timestamps")
                .alias("nts")
                .action(ArgAction::SetTrue)
                .help_heading(OUTPUT_HEADING)
                .help(crate::markdown::render(
                    "Suppress log-line timestamps (default; overrides `--timestamps`).",
                )),
        )
        .arg(
            Arg::new("timeout-secs")
                .long("timeout-secs")
                .alias("timeout")
                .value_name("SECONDS")
                .value_parser(clap::value_parser!(f64))
                .help_heading(OUTPUT_HEADING)
                .help(crate::markdown::render(
                    "Default timeout applied to every `ctx.run(...)` subprocess \
                     (per-call `timeout_secs=` wins when set).",
                )),
        )
        .arg(
            Arg::new("no-output-timeout-secs")
                .long("no-output-timeout-secs")
                .alias("nots")
                .value_name("SECONDS")
                .value_parser(clap::value_parser!(f64))
                .help_heading(OUTPUT_HEADING)
                .help(crate::markdown::render(
                    "Default no-output watchdog applied to every `ctx.run(...)` \
                     subprocess — abort if no stdout/stderr for this many \
                     seconds. Per-call `no_output_timeout_secs=` wins when set.",
                )),
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
                        Arg::new("yes")
                            .long("yes")
                            .short('y')
                            .action(ArgAction::SetTrue)
                            .help("Auto-approve overwriting any conflicting scaffold files"),
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
                // Migration shim: parses `toolr project deps <anything>` so we
                // can emit a tailored "removed in 0.22; use `project venv`"
                // error from `dispatch_project`. Hidden from `--help`. Drop
                // this subcommand after 0.23 once users have migrated.
                Command::new("deps")
                    .hide(true)
                    .allow_external_subcommands(true)
                    .about("(removed in 0.22) use `toolr project venv` instead"),
            )
            .subcommand(
                Command::new("venv")
                    .about("Inspect, sync, and operate on the tools venv")
                    .subcommand_required(true)
                    .subcommand(
                        Command::new("path").about("Print the absolute path to the tools venv"),
                    )
                    .subcommand(
                        Command::new("shell")
                            .about("Spawn a subshell with the tools venv activated"),
                    )
                    .subcommand(
                        Command::new("sync")
                            .about("Sync the tools venv against tools/pyproject.toml + tools/uv.lock (no-op when fresh)")
                            .arg(
                                Arg::new("force")
                                    .long("force")
                                    .short('f')
                                    .action(ArgAction::SetTrue)
                                    .help("Re-run `uv sync` even when the freshness stamp says the venv is up to date"),
                            )
                            .arg(
                                Arg::new("quiet")
                                    .long("quiet")
                                    .short('q')
                                    .action(ArgAction::SetTrue)
                                    .help("Silent on success and on benign unattended-mode exits (no toolr/uv output)"),
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
                            .about("Refresh tools/uv.lock without applying (wraps `uv lock`)")
                            .arg(
                                Arg::new("quiet")
                                    .long("quiet")
                                    .short('q')
                                    .action(ArgAction::SetTrue)
                                    .help("Pass --quiet to uv"),
                            )
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
                            .about("Add one or more packages to tools/pyproject.toml (wraps `uv add`)")
                            .arg(
                                Arg::new("packages")
                                    .value_name("PACKAGE")
                                    .num_args(1..)
                                    .required(true)
                                    .help("Package spec (`name`, `name@version`, `name>=1.2`, …) — passed through to uv"),
                            )
                            .arg(
                                Arg::new("quiet")
                                    .long("quiet")
                                    .short('q')
                                    .action(ArgAction::SetTrue)
                                    .help("Pass --quiet to uv"),
                            ),
                    )
                    .subcommand(
                        Command::new("remove")
                            .about("Remove one or more packages from tools/pyproject.toml (wraps `uv remove`)")
                            .arg(
                                Arg::new("packages")
                                    .value_name("PACKAGE")
                                    .num_args(1..)
                                    .required(true)
                                    .help("Package name to remove (must already appear in tools/pyproject.toml)"),
                            )
                            .arg(
                                Arg::new("quiet")
                                    .long("quiet")
                                    .short('q')
                                    .action(ArgAction::SetTrue)
                                    .help("Pass --quiet to uv"),
                            ),
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
                            .conflicts_with("package_positional")
                            .help(
                                "Path to the package's source tree (bypasses the tools-venv lookup)",
                            ),
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
                            .help("Override the output path"),
                    )
                    .arg(
                        Arg::new("schema-version")
                            .long("schema-version")
                            .value_name("N")
                            .value_parser(clap::value_parser!(u32))
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

fn build_user_command(cmd: &toolr_core::manifest::Command) -> Command {
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
            Some(toolr_core::parser::SupportedType::Optional(_))
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
    meta: &toolr_core::manifest::ArgMetadata,
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

#[cfg(test)]
mod cli_tree_tests {
    use super::*;
    use toolr_core::manifest::{Argument, ArgumentKind, Command, Group, Manifest, Origin};

    fn empty_arg(name: &str, kind: ArgumentKind) -> Argument {
        Argument {
            name: name.into(),
            kind,
            help: String::new(),
            default: None,
            type_annotation: None,
            resolved_type: None,
            allowed_values: vec![],
            path_constraints: None,
            metadata: Default::default(),
            long_flag: None,
        }
    }

    fn dispatcher(name: &str, group: &str, args: Vec<Argument>) -> Command {
        Command {
            name: name.into(),
            group: group.into(),
            module: format!("tools.{name}"),
            function: name.into(),
            summary: String::new(),
            description: String::new(),
            arguments: args,
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher: true,
        }
    }

    fn child(name: &str, group: &str, dispatcher_module: &str, dispatcher_fn: &str) -> Command {
        Command {
            name: name.into(),
            group: group.into(),
            module: dispatcher_module.into(),
            function: dispatcher_fn.into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: Some(format!("argparse:{group}")),
            is_dispatcher: false,
        }
    }

    fn normal_leaf(name: &str, group: &str) -> Command {
        Command {
            name: name.into(),
            group: group.into(),
            module: format!("tools.{name}"),
            function: name.into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: None,
            is_dispatcher: false,
        }
    }

    fn group(name: &str) -> Group {
        Group {
            name: name.into(),
            title: name.into(),
            description: String::new(),
            parent: None,
            origin: Origin::Static,
        }
    }

    fn build_for(manifest: Manifest) -> clap::Command {
        let groups: Vec<&Group> = manifest.groups.iter().collect();
        let mut children_map: std::collections::HashMap<Option<String>, Vec<&Group>> =
            std::collections::HashMap::new();
        for g in &groups {
            children_map
                .entry(g.parent.clone())
                .or_default()
                .push(g);
        }
        let top: Vec<&Group> = manifest.groups.iter().filter(|g| g.parent.is_none()).collect();
        build_group_subtree(top[0], &manifest, &children_map)
    }

    #[test]
    fn dispatcher_hosts_two_grafted_children() {
        let dispatcher_cmd = dispatcher(
            "job",
            "jenkins",
            vec![empty_arg("cpu", ArgumentKind::Optional)],
        );
        // Grafted children live under the dispatcher's dotted_name
        // ("jenkins.job"), mirroring the argparse pipeline's output.
        let migrate = child("migrate", "jenkins.job", "tools.job", "job");
        let runserver = child("runserver", "jenkins.job", "tools.job", "job");

        let manifest = Manifest {
            schema_version: 1,
            static_hash: String::new(),
            third_party_hash: String::new(),
            groups: vec![group("jenkins")],
            commands: vec![dispatcher_cmd, migrate, runserver],
        };

        let jenkins = build_for(manifest);

        let group_subs: Vec<&str> = jenkins.get_subcommands().map(|c| c.get_name()).collect();
        assert_eq!(group_subs, vec!["job"]);

        let job = jenkins.find_subcommand("job").expect("job under jenkins");
        let mut job_subs: Vec<&str> = job.get_subcommands().map(|c| c.get_name()).collect();
        job_subs.sort();
        assert_eq!(job_subs, vec!["migrate", "runserver"]);
    }

    #[test]
    fn two_dispatchers_in_one_group_each_host_their_own_children() {
        let build_cmd = dispatcher("build", "docker", vec![]);
        let image_cmd = dispatcher("image", "docker", vec![]);
        let build_child = child("compile", "docker.build", "tools.build", "build");
        let image_child = child("push", "docker.image", "tools.image", "image");

        let manifest = Manifest {
            schema_version: 1,
            static_hash: String::new(),
            third_party_hash: String::new(),
            groups: vec![group("docker")],
            commands: vec![build_cmd, image_cmd, build_child, image_child],
        };

        let docker = build_for(manifest);
        let build_sub = docker.find_subcommand("build").unwrap();
        let image_sub = docker.find_subcommand("image").unwrap();

        let build_subs: Vec<&str> = build_sub.get_subcommands().map(|c| c.get_name()).collect();
        let image_subs: Vec<&str> = image_sub.get_subcommands().map(|c| c.get_name()).collect();
        assert_eq!(build_subs, vec!["compile"]);
        assert_eq!(image_subs, vec!["push"]);
    }

    #[test]
    fn dispatcher_and_normal_leaf_coexist_in_one_group() {
        let dispatcher_cmd = dispatcher("job", "jenkins", vec![]);
        let migrate = child("migrate", "jenkins.job", "tools.job", "job");
        let status = normal_leaf("status", "jenkins");

        let manifest = Manifest {
            schema_version: 1,
            static_hash: String::new(),
            third_party_hash: String::new(),
            groups: vec![group("jenkins")],
            commands: vec![dispatcher_cmd, migrate, status],
        };

        let jenkins = build_for(manifest);
        let mut group_subs: Vec<&str> = jenkins.get_subcommands().map(|c| c.get_name()).collect();
        group_subs.sort();
        assert_eq!(group_subs, vec!["job", "status"]);

        let job = jenkins.find_subcommand("job").unwrap();
        let job_subs: Vec<&str> = job.get_subcommands().map(|c| c.get_name()).collect();
        assert_eq!(job_subs, vec!["migrate"]);

        let status_sub = jenkins.find_subcommand("status").unwrap();
        assert_eq!(status_sub.get_subcommands().count(), 0);
    }

    #[test]
    fn underscored_alias_lets_clap_accept_both_spellings() {
        // Mirrors the argparse scanner's output for
        // `add_argument('--skip_warm_cache', action='store_true')`:
        // the canonical CLI form is dashed (so help and shell
        // completion offer the dashed spelling), but the upstream
        // underscored form is registered as a hidden alias.
        let mut arg = empty_arg("skip_warm_cache", ArgumentKind::Flag);
        arg.metadata.aliases.push("--skip_warm_cache".into());
        arg.long_flag = Some("--skip_warm_cache".into());

        let mut cmd = normal_leaf("wrapup-company", "jenkins");
        cmd.arguments = vec![arg];

        let clap_cmd = build_user_command(&cmd);

        // Dashed form parses.
        let dashed = clap_cmd
            .clone()
            .try_get_matches_from(vec!["wrapup-company", "--skip-warm-cache"])
            .expect("dashed form must parse");
        assert!(dashed.get_flag("skip_warm_cache"));

        // Underscored form (the upstream argparse spelling) parses too.
        let underscored = clap_cmd
            .clone()
            .try_get_matches_from(vec!["wrapup-company", "--skip_warm_cache"])
            .expect("underscored form must parse");
        assert!(underscored.get_flag("skip_warm_cache"));
    }

    #[test]
    fn dispatcher_with_matching_leaf_name_hoists_children_onto_group() {
        // command_group("django") + def django(...) → dotted_name == "django"
        // (matches the leaf), so the dispatcher's children appear
        // directly under the group, not under a redundant `django`
        // subcommand. User types `toolr django migrate`, not
        // `toolr django django migrate`.
        let dispatcher_cmd = dispatcher("django", "django", vec![]);
        let migrate = child("migrate", "django", "tools.dispatcher", "django");

        let manifest = Manifest {
            schema_version: 1,
            static_hash: String::new(),
            third_party_hash: String::new(),
            groups: vec![group("django")],
            commands: vec![dispatcher_cmd, migrate],
        };

        let django = build_for(manifest);
        let group_subs: Vec<&str> = django.get_subcommands().map(|c| c.get_name()).collect();
        assert_eq!(group_subs, vec!["migrate"]);
    }
}
