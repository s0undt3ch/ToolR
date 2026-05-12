use clap::{Arg, ArgAction, Command};

use _rust_utils::manifest::{ArgumentKind, Manifest};

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
        let mut g = Command::new(group.name.clone()).about(group.title.clone());
        if !group.description.is_empty() {
            g = g.long_about(group.description.clone());
        }
        for cmd in manifest.commands.iter().filter(|c| c.group == group.name) {
            g = g.subcommand(build_user_command(cmd));
        }
        root = root.subcommand(g);
    }

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
