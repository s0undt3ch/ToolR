//! Synthetic manifest entries for toolr's own built-in subcommands.
//!
//! The completion engine in [`toolr_core::complete`] is purely
//! manifest-driven — it knows nothing about the binary's own subcommands
//! (`self`, `project`). This module *derives* the entries directly from
//! [`crate::cli::build_command`] by walking the clap tree, so the engine
//! can offer them as candidates. The synthetic [`Command`] entries are
//! never invoked; only their `name`, `group`, `arguments[].name`,
//! `arguments[].kind`, and `arguments[].allowed_values` matter for
//! completion classification.
//!
//! Because the entries are derived, there is nothing to hand-maintain
//! here: adding or changing a `self`/`project` subcommand in
//! [`crate::cli::build_command`] flows through automatically. Hidden
//! internal helpers (`__complete`, `__install-uv-now`) carry
//! `.hide(true)` in the CLI builder and are skipped — they don't appear in
//! `--help` and shouldn't be tab-completed either.

use clap::{Arg, ArgAction, Command as ClapCommand, ValueHint};
use toolr_core::complete::Candidate;
use toolr_core::manifest::{ArgMetadata, Argument, ArgumentKind, Command, Group, Manifest, Origin};
use toolr_core::parser::SupportedType;

/// The two top-level builtin subtrees, walked out of the clap command.
const BUILTIN_ROOTS: &[&str] = &["project", "self"];

/// Synthetic [`Group`] + [`Command`] entries derived from the built-in
/// subcommand tree wired up in [`crate::cli::build_command`]. Callers
/// (currently only `dispatch::complete::run_complete`) merge these into a
/// loaded manifest before delegating to
/// [`toolr_core::complete::serve_completions`].
pub fn built_in_completion_entries() -> (Vec<Group>, Vec<Command>) {
    let root = crate::cli::build_command(&Manifest::empty());
    let mut groups = Vec::new();
    let mut commands = Vec::new();

    for name in BUILTIN_ROOTS {
        let Some(sub) = root
            .get_subcommands()
            .find(|c| c.get_name() == *name && !c.is_hide_set())
        else {
            continue;
        };
        // Each builtin root (`self`, `project`) is itself a group node.
        walk_group(sub, None, &mut groups, &mut commands);
    }

    (groups, commands)
}

/// Recursively walk a clap subcommand that represents a *group* node,
/// emitting a [`Group`] for it and classifying each visible child as
/// either a nested group (has its own subcommands) or a leaf
/// [`Command`].
fn walk_group(
    cmd: &ClapCommand,
    parent: Option<&str>,
    groups: &mut Vec<Group>,
    commands: &mut Vec<Command>,
) {
    let name = cmd.get_name().to_string();
    let full_path = match parent {
        Some(p) => format!("{p}.{name}"),
        None => name.clone(),
    };
    groups.push(Group {
        name: name.clone(),
        title: about_text(cmd),
        description: String::new(),
        parent: parent.map(str::to_string),
        origin: Origin::Static,
    });

    for child in cmd.get_subcommands() {
        if child.is_hide_set() {
            continue;
        }
        if child.get_subcommands().next().is_some() {
            // A child that itself owns subcommands is a nested group.
            walk_group(child, Some(&full_path), groups, commands);
        } else {
            commands.push(leaf_command(child, &full_path));
        }
    }
}

/// Build a synthetic leaf [`Command`] from a clap subcommand. The
/// synthetic entry is never invoked — the binary intercepts
/// `self`/`project` before any manifest lookup — so `module`/`function`
/// are empty; only the shape the completion classifier reads is filled.
fn leaf_command(cmd: &ClapCommand, group: &str) -> Command {
    Command {
        name: cmd.get_name().to_string(),
        group: group.to_string(),
        module: String::new(),
        function: String::new(),
        summary: about_text(cmd),
        description: String::new(),
        arguments: cmd.get_arguments().filter_map(derive_argument).collect(),
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

/// Map a clap [`Arg`] onto a synthetic [`Argument`], or `None` when the
/// arg shouldn't surface as a completion candidate.
///
/// `--help` is filtered out: the engine offers it for free at every
/// group node, so listing it here would duplicate it. Hidden args are
/// skipped for the same reason hidden subcommands are.
///
/// Arg → [`ArgumentKind`] mapping:
/// - positional                     → `Positional`
/// - `ArgAction::SetTrue`/`SetFalse` → `Flag`
/// - `ArgAction::Count`             → `Count`
/// - `ArgAction::Append`            → `Repeated`
/// - value-taking option (`Set`, …) → `Optional`
fn derive_argument(arg: &Arg) -> Option<Argument> {
    if arg.get_id() == "help" || arg.is_hide_set() {
        return None;
    }
    let kind = if arg.is_positional() {
        ArgumentKind::Positional
    } else {
        match arg.get_action() {
            ArgAction::SetTrue | ArgAction::SetFalse => ArgumentKind::Flag,
            ArgAction::Count => ArgumentKind::Count,
            ArgAction::Append => ArgumentKind::Repeated,
            _ => ArgumentKind::Optional,
        }
    };
    let allowed_values = arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_string())
        .collect();
    Some(Argument {
        // The engine matches a committed `--flag` against `a.name` after
        // stripping `--`, and renders flag candidates as
        // `--{name.replace('_', "-")}`. clap's id is already the
        // hyphenated long-flag stem, so it round-trips.
        name: arg.get_id().to_string(),
        kind,
        help: arg.get_help().map(ToString::to_string).unwrap_or_default(),
        default: None,
        type_annotation: None,
        resolved_type: path_type(arg.get_value_hint()),
        allowed_values,
        metadata: ArgMetadata::default(),
        long_flag: None,
    })
}

/// The path type a built-in's clap hint stands for, so its value completes
/// from the file system like a user command's path argument.
fn path_type(hint: ValueHint) -> Option<SupportedType> {
    match hint {
        ValueHint::AnyPath => Some(SupportedType::Path),
        ValueHint::FilePath => Some(SupportedType::FilePath),
        ValueHint::DirPath => Some(SupportedType::DirectoryPath),
        ValueHint::ExecutablePath => Some(SupportedType::ExecutablePath),
        _ => None,
    }
}

/// clap's `about` rendered to a plain string (empty when unset).
fn about_text(cmd: &ClapCommand) -> String {
    cmd.get_about().map(ToString::to_string).unwrap_or_default()
}

/// One root flag as the scan sees it, read from the clap command.
#[derive(Debug, Default, PartialEq, Eq)]
struct RootFlagSpec {
    id: String,
    long: String,
    /// Every spelling: `--long`, `--alias`, `-s`.
    spellings: Vec<String>,
    help: String,
    /// Offered as a completion: not hidden, and neither `--help` (the engine offers it) nor `--version`.
    offered: bool,
    takes_value: bool,
    /// The value type the scan can check; `None` for a type it does not know.
    value: Option<ValueKind>,
    /// `--version`: clap prints and exits, so nothing after it completes.
    ends_parsing: bool,
    conflicts: Vec<String>,
}

/// An enum so another parser type is one variant plus one [`accepts_value`] arm; a spec whose `value` is
/// `None` has a type the scan does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Float,
}

/// The checkable value type behind an arg's parser, if the scan knows it.
fn value_kind(arg: &Arg) -> Option<ValueKind> {
    (arg.get_value_parser().type_id() == std::any::TypeId::of::<f64>()).then_some(ValueKind::Float)
}

/// What [`scan_root_flags`] found in the committed tokens.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RootScan {
    /// Tokens left for the engine: everything after the leading root flags, plus the in-progress word.
    pub rest: Vec<String>,
    /// Ids of the root flags given, sorted (aliases and shorts resolved).
    pub seen: Vec<String>,
    /// clap rejects the line (duplicate, conflict, bad or missing value), so nothing completes.
    pub dead_end: bool,
    /// The root flags, derived once per completion.
    specs: Vec<RootFlagSpec>,
}

impl RootScan {
    /// The root long flags still valid after the ones seen: not given yet and not conflicting with a given one.
    pub fn offerable_long_flags(&self) -> Vec<Candidate> {
        let blocked: Vec<&str> = self
            .specs
            .iter()
            .filter(|s| self.seen.contains(&s.id))
            .flat_map(|s| {
                std::iter::once(s.id.as_str()).chain(s.conflicts.iter().map(String::as_str))
            })
            .collect();
        let mut flags: Vec<Candidate> = self
            .specs
            .iter()
            .filter(|s| s.offered && !blocked.contains(&s.id.as_str()))
            .map(|s| Candidate::new(format!("--{}", s.long), s.help.clone()))
            .collect();
        toolr_core::complete::sort_and_dedup_by_value(&mut flags);
        flags
    }
}

fn root_flag_specs() -> Vec<RootFlagSpec> {
    let root = crate::cli::root_command();
    let mut specs: Vec<RootFlagSpec> = root
        .get_arguments()
        .filter_map(|a| {
            Some(RootFlagSpec {
                id: a.get_id().to_string(),
                long: a.get_long()?.to_string(),
                spellings: crate::cli::spellings(a),
                help: a.get_help().map(ToString::to_string).unwrap_or_default(),
                offered: !a.is_hide_set()
                    && a.get_id() != "help"
                    && !matches!(a.get_action(), ArgAction::Version),
                takes_value: a.get_action().takes_values(),
                value: value_kind(a),
                ends_parsing: matches!(a.get_action(), ArgAction::Version),
                conflicts: root
                    .get_arg_conflicts_with(a)
                    .iter()
                    .map(|c| c.get_id().to_string())
                    .collect(),
            })
        })
        .collect();
    mirror_conflicts(&mut specs);
    specs
}

/// Make each conflict list symmetric. clap enforces a conflict both ways at parse time, but the listing is
/// one-sided even after `build()` (`--quiet` names `--debug`, `--debug` does not name `--quiet`).
fn mirror_conflicts(specs: &mut [RootFlagSpec]) {
    let pairs: Vec<(String, String)> = specs
        .iter()
        .flat_map(|s| s.conflicts.iter().map(|c| (s.id.clone(), c.clone())))
        .collect();
    for (a, b) in pairs {
        for spec in specs.iter_mut() {
            if spec.id == b && !spec.conflicts.contains(&a) {
                spec.conflicts.push(a.clone());
            }
        }
    }
}

/// Whether clap accepts `value` for a value-taking root flag. An attached value (`--flag=-1`) is
/// unambiguous; a detached one that starts with `-` reads as another flag. A value type the scan does not
/// know never passes, so the line dead-ends rather than being checked as the wrong type.
fn accepts_value(spec: &RootFlagSpec, value: &str, attached: bool) -> bool {
    match spec.value {
        Some(ValueKind::Float) => {
            value.parse::<f64>().is_ok() && (attached || !value.starts_with('-'))
        }
        None => false,
    }
}

/// The root flags given so far in the leading run.
#[derive(Default)]
struct Seen<'a>(Vec<&'a RootFlagSpec>);

impl<'a> Seen<'a> {
    /// Record `spec`; false when clap would reject it. `ends_parsing` makes `--version` a dead end whenever
    /// it is seen (clap prints and exits); a repeat or a conflict with a flag already seen is one too.
    fn take(&mut self, spec: &'a RootFlagSpec) -> bool {
        let clash = spec.ends_parsing
            || self
                .0
                .iter()
                .any(|s| s.id == spec.id || spec.conflicts.contains(&s.id));
        self.0.push(spec);
        !clash
    }
}

/// How one committed token moved the scan.
enum Step {
    /// The scan used this many tokens.
    Consumed(usize),
    /// Not a root flag: the leading run ends here.
    Stop,
    /// clap rejects the line.
    DeadEnd,
}

fn verdict(ok: bool, used: usize) -> Step {
    if ok {
        Step::Consumed(used)
    } else {
        Step::DeadEnd
    }
}

/// `--name`, `--name=value` or `--name value`; `next` is the committed token after `long`, if any.
fn scan_long<'a>(
    specs: &'a [RootFlagSpec],
    seen: &mut Seen<'a>,
    long: &str,
    next: Option<&str>,
) -> Step {
    let (name, attached) = match long.split_once('=') {
        Some((n, v)) => (n, Some(v)),
        None => (long, None),
    };
    let Some(spec) = specs
        .iter()
        .find(|s| s.spellings.contains(&format!("--{name}")))
    else {
        return Step::Stop;
    };
    match (spec.takes_value, attached) {
        (false, Some(_)) => Step::DeadEnd,
        (false, None) => verdict(seen.take(spec), 1),
        (true, Some(v)) => verdict(accepts_value(spec, v, true) && seen.take(spec), 1),
        // No committed token after the flag means the in-progress word is its value: a dead end.
        (true, None) => verdict(
            next.is_some_and(|v| accepts_value(spec, v, false)) && seen.take(spec),
            2,
        ),
    }
}

/// A `-xyz` cluster of short flags; every char must be a known short flag that takes no value.
fn scan_cluster<'a>(specs: &'a [RootFlagSpec], seen: &mut Seen<'a>, cluster: &str) -> Step {
    let flags: Option<Vec<&RootFlagSpec>> = cluster
        .chars()
        .map(|c| {
            let short = format!("-{c}");
            specs
                .iter()
                .find(|s| !s.takes_value && s.spellings.contains(&short))
        })
        .collect();
    match flags {
        Some(flags) => verdict(flags.into_iter().all(|f| seen.take(f)), 1),
        None => Step::Stop,
    }
}

/// Strip the leading run of root flags from the completion tokens (the last one is the in-progress word).
///
/// Root flags are not global, so only a leading run counts; the scan mirrors what clap accepts and flags the
/// lines it rejects as a dead end.
pub fn scan_root_flags(tokens: &[String]) -> RootScan {
    let specs = root_flag_specs();
    let committed = tokens.len().saturating_sub(1);
    let mut seen = Seen::default();
    let mut i = 0;
    while i < committed {
        let tok = tokens[i].as_str();
        let next = (i + 1 < committed).then(|| tokens[i + 1].as_str());
        let step = if let Some(long) = tok.strip_prefix("--").filter(|l| !l.is_empty()) {
            scan_long(&specs, &mut seen, long, next)
        } else if let Some(cluster) = tok
            .strip_prefix('-')
            .filter(|c| !c.is_empty() && !c.starts_with('-'))
        {
            scan_cluster(&specs, &mut seen, cluster)
        } else {
            Step::Stop
        };
        match step {
            // Advance past the flag and, for a detached value, the value too.
            Step::Consumed(used) => i += used,
            Step::Stop => break,
            Step::DeadEnd => {
                return RootScan {
                    dead_end: true,
                    ..RootScan::default()
                };
            }
        }
    }
    let mut ids: Vec<String> = seen.0.iter().map(|s| s.id.clone()).collect();
    ids.sort();
    RootScan {
        rest: tokens[i..].to_vec(),
        seen: ids,
        dead_end: false,
        specs,
    }
}

#[cfg(test)]
mod tests;
