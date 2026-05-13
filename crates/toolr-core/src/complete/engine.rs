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

    // No committed tokens → completing the top-level group name.
    if committed.is_empty() {
        return Slot::Group { prefix };
    }

    // Walk down the group tree as far as the committed tokens match
    // groups. The cursor lands at one of three places:
    //   - inside a (possibly nested) group — next token is either a
    //     subgroup or a command at this level;
    //   - on a specific command — next tokens are its args;
    //   - somewhere unrecognised — no completions.
    let mut group_path: Vec<&str> = Vec::new();
    let mut cursor = 0usize;
    while cursor < committed.len() {
        let candidate_path_with_next = if group_path.is_empty() {
            committed[cursor].clone()
        } else {
            format!("{}.{}", group_path.join("."), committed[cursor])
        };
        if manifest
            .groups
            .iter()
            .any(|g| g.full_path() == candidate_path_with_next)
        {
            group_path.push(committed[cursor].as_str());
            cursor += 1;
            continue;
        }
        break;
    }

    // No committed token matched a group → not a recognised invocation.
    if group_path.is_empty() {
        return Slot::None;
    }

    let group_full_path = group_path.join(".");

    // If the user has only committed group tokens, we're completing
    // either a subgroup or a command at this level.
    if cursor == committed.len() {
        return Slot::Command {
            group: group_full_path,
            prefix,
        };
    }

    // Next committed token is the command name on the resolved group.
    let command_name = &committed[cursor];
    let Some(command) = manifest
        .commands
        .iter()
        .find(|c| c.group == group_full_path && &c.name == command_name)
    else {
        return Slot::None;
    };

    // Everything after the command name is the argument zone.
    let arg_tokens = &committed[cursor + 1..];

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
        .filter(|a| {
            matches!(
                a.kind,
                ArgumentKind::Positional | ArgumentKind::VarPositional
            )
        })
        .collect();
    if let Some(&arg) = positional_args.get(positional_index) {
        return Slot::Positional {
            argument: arg,
            prefix,
        };
    }
    // If the user is past the last fixed positional but there's a
    // trailing variadic, keep completing against that.
    if let Some(&arg) = positional_args.last() {
        if matches!(arg.kind, ArgumentKind::VarPositional) {
            return Slot::Positional {
                argument: arg,
                prefix,
            };
        }
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

/// Top-level group names matching `prefix`.
fn groups(manifest: &Manifest, prefix: &str) -> Vec<String> {
    manifest
        .groups
        .iter()
        .filter(|g| g.parent.is_none())
        .map(|g| g.name.clone())
        .filter(|name| name.starts_with(prefix))
        .collect()
}

/// At the resolved group level, candidates are both *child groups*
/// (their leaf name) and *direct commands*. Lets `toolr docker <Tab>`
/// complete to `image`, `container`, plus any commands attached to
/// `docker` itself.
fn commands(manifest: &Manifest, group: &str, prefix: &str) -> Vec<String> {
    let mut out: Vec<String> = manifest
        .commands
        .iter()
        .filter(|c| c.group == group)
        .map(|c| c.name.clone())
        .collect();
    out.extend(
        manifest
            .groups
            .iter()
            .filter(|g| g.parent.as_deref() == Some(group))
            .map(|g| g.name.clone()),
    );
    out.into_iter().filter(|name| name.starts_with(prefix)).collect()
}

fn flags(command: &Command, prefix: &str) -> Vec<String> {
    command
        .arguments
        .iter()
        .filter(|a| {
            !matches!(
                a.kind,
                ArgumentKind::Positional | ArgumentKind::VarPositional
            )
        })
        .map(|a| format!("--{}", a.name.replace('_', "-")))
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
