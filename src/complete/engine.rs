//! Pure prefix-matching completion engine. No I/O.

use crate::manifest::{Argument, ArgumentKind, Command, Manifest};

/// Compute the list of completion candidates for a tokenised command
/// line. `tokens` is everything after `toolr` itself — for example
/// `["ci", "hello", "--na"]`. The last token is treated as the
/// in-progress word and is matched as a prefix; earlier tokens are
/// matched exactly.
///
/// The returned vector is alphabetically sorted and deduplicated.
pub fn serve_completions(manifest: &Manifest, tokens: &[String]) -> Vec<String> {
    let mut out = match classify(manifest, tokens) {
        Slot::Group { prefix } => groups(manifest, &prefix),
        Slot::Command { group, prefix } => commands(manifest, &group, &prefix),
        Slot::Flag { command, prefix } => flags(command, &prefix),
        Slot::FlagValue { argument, prefix } => values(argument, &prefix),
        Slot::Positional { argument, prefix } => values(argument, &prefix),
        Slot::None => Vec::new(),
    };
    out.sort();
    out.dedup();
    out
}

enum Slot<'a> {
    Group {
        prefix: String,
    },
    Command {
        group: String,
        prefix: String,
    },
    Flag {
        command: &'a Command,
        prefix: String,
    },
    FlagValue {
        argument: &'a Argument,
        prefix: String,
    },
    Positional {
        argument: &'a Argument,
        prefix: String,
    },
    None,
}

fn classify<'a>(manifest: &'a Manifest, tokens: &[String]) -> Slot<'a> {
    // The last token is the in-progress word; anything earlier is
    // considered "committed". An empty `tokens` slice is treated as a
    // single empty token (the user just typed `toolr <Tab>`).
    if tokens.is_empty() {
        return Slot::Group {
            prefix: String::new(),
        };
    }
    let prefix = tokens.last().cloned().unwrap_or_default();
    let committed = &tokens[..tokens.len() - 1];

    // No committed tokens → completing the group name.
    if committed.is_empty() {
        return Slot::Group { prefix };
    }

    // First committed token is the group.
    let group_name = &committed[0];
    let Some(_group) = manifest.groups.iter().find(|g| &g.name == group_name) else {
        return Slot::None;
    };

    // One committed token (the group) → completing the command name.
    if committed.len() == 1 {
        return Slot::Command {
            group: group_name.clone(),
            prefix,
        };
    }

    // Two+ committed tokens → group, command, then args.
    let command_name = &committed[1];
    let Some(command) = manifest
        .commands
        .iter()
        .find(|c| &c.group == group_name && &c.name == command_name)
    else {
        return Slot::None;
    };

    // From committed[2..], figure out what argument we're inside.
    let arg_tokens = &committed[2..];

    // If the previous committed token was a `--flag`, we're completing
    // that flag's value.
    if let Some(prev) = arg_tokens.last() {
        if let Some(flag_name) = prev.strip_prefix("--") {
            if let Some(arg) = command.arguments.iter().find(|a| a.name == flag_name) {
                if !matches!(arg.kind, ArgumentKind::Flag) {
                    return Slot::FlagValue {
                        argument: arg,
                        prefix,
                    };
                }
            }
        }
    }

    // Otherwise: if the in-progress word starts with `--`, complete to a
    // flag name. If not, treat it as the next positional value.
    if prefix.starts_with("--") || prefix == "-" {
        return Slot::Flag { command, prefix };
    }

    // Positional path: count how many positional values have already
    // been provided in `arg_tokens` (skipping `--flag value` pairs and
    // bare `--flag` boolean flags) and pick the matching Argument.
    let positional_index = count_positionals_consumed(command, arg_tokens);
    let positional_args: Vec<&Argument> = command
        .arguments
        .iter()
        .filter(|a| matches!(a.kind, ArgumentKind::Positional))
        .collect();
    if let Some(&arg) = positional_args.get(positional_index) {
        return Slot::Positional {
            argument: arg,
            prefix,
        };
    }

    Slot::None
}

fn count_positionals_consumed(command: &Command, arg_tokens: &[String]) -> usize {
    let mut idx = 0usize;
    let mut i = 0usize;
    while i < arg_tokens.len() {
        let t = &arg_tokens[i];
        if let Some(flag_name) = t.strip_prefix("--") {
            if let Some(arg) = command.arguments.iter().find(|a| a.name == flag_name) {
                if matches!(arg.kind, ArgumentKind::Flag) {
                    i += 1;
                    continue;
                }
                // --flag value pair
                i += 2;
                continue;
            }
            // Unknown flag — skip just the token.
            i += 1;
            continue;
        }
        idx += 1;
        i += 1;
    }
    idx
}

fn groups(manifest: &Manifest, prefix: &str) -> Vec<String> {
    manifest
        .groups
        .iter()
        .map(|g| g.name.clone())
        .filter(|name| name.starts_with(prefix))
        .collect()
}

fn commands(manifest: &Manifest, group: &str, prefix: &str) -> Vec<String> {
    manifest
        .commands
        .iter()
        .filter(|c| c.group == group)
        .map(|c| c.name.clone())
        .filter(|name| name.starts_with(prefix))
        .collect()
}

fn flags(command: &Command, prefix: &str) -> Vec<String> {
    command
        .arguments
        .iter()
        .filter(|a| !matches!(a.kind, ArgumentKind::Positional))
        .map(|a| format!("--{}", a.name))
        .filter(|flag| flag.starts_with(prefix))
        .collect()
}

fn values(argument: &Argument, prefix: &str) -> Vec<String> {
    argument
        .allowed_values
        .iter()
        .filter(|v| v.starts_with(prefix))
        .cloned()
        .collect()
}
