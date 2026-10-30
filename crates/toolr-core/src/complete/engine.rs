//! Pure prefix-matching completion engine. No I/O.

use crate::manifest::{Argument, ArgumentKind, Command, Manifest};
use crate::text::{has_unsafe_name_chars, strip_control_chars};

/// A single completion candidate: the word to insert, plus an optional
/// one-line description (a group's `title`, a command's `summary`, or a
/// flag's `help`). Shells that support per-candidate descriptions (fish
/// natively, zsh via `compadd -d`) render `description` alongside `value`;
/// others ignore it. Empty `description` means none is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub value: String,
    pub description: String,
}

impl Candidate {
    pub fn new(value: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            // The wire format is one `value<TAB>description` line per
            // candidate, and the text reaches a terminal: strip anything
            // that could forge candidates or inject escape sequences.
            description: strip_control_chars(&description.into()),
        }
    }

    pub(crate) fn bare(value: impl Into<String>) -> Self {
        Self::new(value, String::new())
    }
}

/// Compute the list of completion candidates for a tokenised command
/// line. `tokens` is everything after `toolr` itself — for example
/// `["ci", "hello", "--na"]`. The last token is treated as the
/// in-progress word and is matched as a prefix; earlier tokens are
/// matched exactly.
///
/// The returned vector is sorted by `value` and deduplicated on it.
pub fn serve_completions(manifest: &Manifest, tokens: &[String]) -> Vec<Candidate> {
    let mut out = match classify(manifest, tokens) {
        Slot::Group { prefix } => {
            if is_flag_prefix(&prefix) {
                group_node_flags(&prefix)
            } else {
                groups(manifest, &prefix)
            }
        }
        Slot::Command { group, prefix } => {
            if is_flag_prefix(&prefix) {
                group_node_flags(&prefix)
            } else {
                commands(manifest, &group, &prefix)
            }
        }
        Slot::Flag { command, prefix } => flags(command, &prefix),
        Slot::FlagValue { argument, prefix } => values(argument, &prefix),
        Slot::Positional { argument, prefix } => values(argument, &prefix),
        Slot::None => Vec::new(),
    };
    // Every candidate source in the engine passes here; names are already
    // dropped at load time. A value can't be both shown faithfully and kept
    // safe, so it is dropped.
    out.retain(|c| !has_unsafe_name_chars(&c.value));
    sort_and_dedup_by_value(&mut out);
    out
}

/// Sort by `value` and drop later duplicates sharing one. Shared with the
/// binary's own completion merging (root flags, built-in `self`/`project`
/// entries) so every call site orders and dedups candidates the same way.
pub fn sort_and_dedup_by_value(candidates: &mut Vec<Candidate>) {
    candidates.sort_by(|a, b| a.value.cmp(&b.value));
    candidates.dedup_by(|a, b| a.value == b.value);
}

fn is_flag_prefix(prefix: &str) -> bool {
    prefix.starts_with("--") || prefix == "-"
}

const HELP_DESCRIPTION: &str = "Print help";

/// Group nodes (the root and any sub-group) carry no argument schema in
/// the manifest, but clap injects `--help` on every subcommand. Offer it
/// when the user has explicitly typed a flag prefix; binaries layer
/// additional root-level flags on top via their own completion path.
fn group_node_flags(prefix: &str) -> Vec<Candidate> {
    ["--help"]
        .into_iter()
        .filter(|f| f.starts_with(prefix))
        .map(|f| Candidate::new(f, HELP_DESCRIPTION))
        .collect()
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

    if command.is_dispatcher {
        return classify_dispatcher(manifest, command, &group_full_path, arg_tokens, prefix);
    }

    classify_leaf_args(command, arg_tokens, prefix)
}

/// Token-classification for a regular (non-dispatcher) leaf command.
/// Factored out so the dispatcher branch can reuse it after recursing
/// into a chosen grafted child.
fn classify_leaf_args<'a>(command: &'a Command, arg_tokens: &[String], prefix: String) -> Slot<'a> {
    if let Some(argument) = pending_flag_value(command, arg_tokens) {
        return Slot::FlagValue { argument, prefix };
    }

    // Otherwise: if the in-progress word starts with `--`, complete to a
    // flag name. If not, treat it as the next positional value.
    if prefix.starts_with("--") || prefix == "-" {
        return Slot::Flag { command, prefix };
    }

    // Positional path: count how many positional values have already
    // been provided in `arg_tokens` (skipping `--flag value` pairs and
    // bare `--flag` boolean flags) and pick the matching Argument.
    let positional_index = positional_indices(command, arg_tokens).len();
    let positional_args: Vec<&Argument> = command
        .arguments
        .iter()
        .filter(|a| is_positional(a))
        .collect();
    if let Some(&arg) = positional_args.get(positional_index) {
        return Slot::Positional {
            argument: arg,
            prefix,
        };
    }
    // If the user is past the last fixed positional but there's a
    // trailing variadic, keep completing against that.
    if let Some(&arg) = positional_args.last()
        && matches!(arg.kind, ArgumentKind::VarPositional)
    {
        return Slot::Positional {
            argument: arg,
            prefix,
        };
    }

    // No positional candidate to suggest. If the command has any flags,
    // fall back to flag-name completion so the user has SOMETHING to
    // Tab into — particularly useful for argparse-style commands whose
    // entire surface is `--flag value` pairs (no positionals at all),
    // and for the trailing position past every fixed positional.
    if command.arguments.iter().any(|a| !is_positional(a)) {
        return Slot::Flag { command, prefix };
    }

    Slot::None
}

/// Token-classification for a dispatcher command: the dispatcher's
/// children are grafted at its dotted path (`<group>.<name>`), so
/// completing the "next positional" against a dispatcher means
/// suggesting a child command name. Once a child is chosen, completion
/// re-enters that child as a regular leaf.
fn classify_dispatcher<'a>(
    manifest: &'a Manifest,
    command: &'a Command,
    group_full_path: &str,
    arg_tokens: &[String],
    prefix: String,
) -> Slot<'a> {
    let dispatcher_path = if group_full_path.is_empty() {
        command.name.clone()
    } else {
        format!("{}.{}", group_full_path, command.name)
    };

    // If the user has already committed a positional value (the child
    // command name), recurse fully into that child's arg zone — flags,
    // flag values, and positional completions from here on belong to
    // the *child*, not the dispatcher. Checking this first prevents
    // the dispatcher's flag-name fallback from masking the child's.
    if let Some(&child_index) = positional_indices(command, arg_tokens).first() {
        let child_name = &arg_tokens[child_index];
        let Some(child) = manifest
            .commands
            .iter()
            .find(|c| c.group == dispatcher_path && &c.name == child_name)
        else {
            return Slot::None;
        };
        return classify_leaf_args(child, &arg_tokens[child_index + 1..], prefix);
    }

    // No child chosen yet — completion targets the dispatcher's own
    // surface (`--flag <TAB>`, `--<TAB>`) or the child-name slot.
    if let Some(argument) = pending_flag_value(command, arg_tokens) {
        return Slot::FlagValue { argument, prefix };
    }
    if prefix.starts_with("--") || prefix == "-" {
        return Slot::Flag { command, prefix };
    }

    Slot::Command {
        group: dispatcher_path,
        prefix,
    }
}

fn is_positional(argument: &Argument) -> bool {
    matches!(
        argument.kind,
        ArgumentKind::Positional | ArgumentKind::VarPositional | ArgumentKind::OptionalPositional
    ) || (argument.kind == ArgumentKind::FixedArity && argument.long_flag.is_none())
}

fn takes_value(argument: &Argument) -> bool {
    !matches!(argument.kind, ArgumentKind::Flag | ArgumentKind::Count)
}

/// The argument a committed flag token names, under every spelling the
/// clap CLI accepts (`cli::build_user_command`): `--` plus the name with
/// `_` → `-`, and each `arg(aliases=…)` entry (one char → `-c`, longer →
/// `--alias`). Manifest names are Python identifiers, so comparing them
/// to the typed token directly misses every multi-word flag.
fn find_flag<'a>(command: &'a Command, token: &str) -> Option<&'a Argument> {
    let (spelling, short) = match token.strip_prefix("--") {
        Some(long) => (long, false),
        None => (token.strip_prefix('-')?, true),
    };
    if spelling.is_empty() {
        return None;
    }
    command
        .arguments
        .iter()
        .filter(|a| !is_positional(a))
        .find(|a| {
            (!short && a.name.replace('_', "-") == spelling)
                || a.metadata.aliases.iter().any(|alias| {
                    let alias = alias.trim_start_matches('-');
                    (alias.chars().count() == 1) == short && alias == spelling
                })
        })
}

/// The value-taking flag whose value is being completed, when it's the
/// last committed token.
fn pending_flag_value<'a>(command: &'a Command, arg_tokens: &[String]) -> Option<&'a Argument> {
    arg_tokens
        .last()
        .and_then(|t| find_flag(command, t))
        .filter(|a| takes_value(a))
}

/// Indices of the positional values in `arg_tokens`, skipping each known
/// flag and, when it takes one, its value. An unknown `--flag` skips just
/// itself; `--flag=value` is one token, so it lands there too.
fn positional_indices(command: &Command, arg_tokens: &[String]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < arg_tokens.len() {
        let token = &arg_tokens[i];
        match find_flag(command, token) {
            Some(arg) if takes_value(arg) => i += 2,
            Some(_) => i += 1,
            None if token.starts_with("--") => i += 1,
            None => {
                out.push(i);
                i += 1;
            }
        }
    }
    out
}

/// Top-level group names matching `prefix`, described by their `title`.
fn groups(manifest: &Manifest, prefix: &str) -> Vec<Candidate> {
    manifest
        .groups
        .iter()
        .filter(|g| g.parent.is_none())
        .filter(|g| g.name.starts_with(prefix))
        .map(|g| Candidate::new(g.name.clone(), g.title.clone()))
        .collect()
}

/// At the resolved group level, candidates are both *child groups*
/// (their leaf name) and *direct commands*. Lets `toolr docker <Tab>`
/// complete to `image`, `container`, plus any commands attached to
/// `docker` itself. Each is described by its group `title` or command
/// `summary`.
fn commands(manifest: &Manifest, group: &str, prefix: &str) -> Vec<Candidate> {
    // Filter by prefix before cloning the description text, so a narrow
    // prefix doesn't pay for summaries/titles it's about to discard.
    let mut out: Vec<Candidate> = manifest
        .commands
        .iter()
        .filter(|c| c.group == group && c.name.starts_with(prefix))
        .map(|c| Candidate::new(c.name.clone(), c.summary.clone()))
        .collect();
    out.extend(
        manifest
            .groups
            .iter()
            .filter(|g| g.parent.as_deref() == Some(group) && g.name.starts_with(prefix))
            .map(|g| Candidate::new(g.name.clone(), g.title.clone())),
    );
    out
}

fn flags(command: &Command, prefix: &str) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = command
        .arguments
        .iter()
        .filter(|a| !is_positional(a))
        .filter_map(|a| {
            let value = format!("--{}", a.name.replace('_', "-"));
            value
                .starts_with(prefix)
                .then(|| Candidate::new(value, a.help.clone()))
        })
        .collect();
    // clap injects `--help` on every leaf, but only surface it when the
    // user is explicitly probing flags (`--`/`-` prefix). The empty-prefix
    // flag fallback that fires for argparse-style children still returns
    // only the command's own flags, so existing behavior is preserved.
    if is_flag_prefix(prefix) && "--help".starts_with(prefix) {
        out.push(Candidate::new("--help", HELP_DESCRIPTION));
    }
    out
}

fn values(argument: &Argument, prefix: &str) -> Vec<Candidate> {
    argument
        .allowed_values
        .iter()
        .filter(|v| v.starts_with(prefix))
        .map(|v| Candidate::bare(v.clone()))
        .collect()
}
