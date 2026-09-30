//! Merge parsed third-party fragments into a project `Manifest`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::model::ManifestFragment;
use crate::manifest::{Command, Group, Manifest, Origin, PluginWarning, PluginWarningKind};

/// Where people vote for choosing a clash winner in configuration.
const RESOLVE_ISSUE_URL: &str = "https://github.com/s0undt3ch/ToolR/issues/522";

/// One plugin's definition of a command, held until every fragment is read.
struct Definition {
    package: String,
    path: PathBuf,
    command: Command,
}

/// Consume `fragments`, each paired with the file it was read from, merging their groups +
/// commands into `base`.
///
/// Conflict resolution:
/// - A group/command pair already present in `base` (from `tools/**/*.py`)
///   wins; the third-party entry is skipped and a `Shadowed` warning is
///   appended to `base.plugin_warnings`.
/// - A group/command pair declared by two or more third-party packages is
///   disabled: none of them is merged, and one `Conflict` warning names them all.
/// - Groups merge by `full_path()`: if a third-party fragment declares a
///   group already present in `base` or in a prior fragment, the existing
///   group's title/description are kept.
///
/// Warnings follow discovery order: every `Shadowed`, then every `Conflict`.
/// Merged entries are tagged `Origin::ThirdParty`, and commands lose any
/// argparse-dispatch flags, which only a local build may set.
pub fn merge_into_manifest(
    mut base: Manifest,
    fragments: Vec<(ManifestFragment, PathBuf)>,
) -> Manifest {
    // (group, command) → the local command's module, which wins and names the shadow warning.
    let local: HashMap<(String, String), String> = base
        .commands
        .iter()
        .map(|c| ((c.group.clone(), c.name.clone()), c.module.clone()))
        .collect();
    // A HashMap alone iterates in random order, so `keys` keeps first-seen order.
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut definitions: HashMap<(String, String), Vec<Definition>> = HashMap::new();

    let mut known_groups: HashSet<String> = base.groups.iter().map(Group::full_path).collect();

    for (fragment, path) in fragments {
        for mut fg in fragment.groups {
            if known_groups.insert(fg.full_path()) {
                fg.origin = Origin::ThirdParty;
                base.groups.push(fg);
            }
        }
        for fc in fragment.commands {
            let key = (fc.group.clone(), fc.name.clone());
            if let Some(module) = local.get(&key) {
                base.plugin_warnings.push(PluginWarning {
                    package: fragment.package.clone(),
                    path: path.clone(),
                    kind: PluginWarningKind::Shadowed,
                    message: shadow_message(module, &fc, &fragment.package),
                });
                continue;
            }
            let defs = definitions.entry(key.clone()).or_default();
            if defs.is_empty() {
                keys.push(key);
            }
            // Only a hand-edited fragment lists a command twice; keep its first copy.
            if defs.iter().any(|d| d.package == fragment.package) {
                continue;
            }
            defs.push(Definition {
                package: fragment.package.clone(),
                path: path.clone(),
                command: fc,
            });
        }
    }

    for key in keys {
        let mut defs = definitions.remove(&key).unwrap_or_default();
        if defs.len() > 1 {
            base.plugin_warnings.push(conflict_warning(&defs));
            continue;
        }
        let Some(Definition { mut command, .. }) = defs.pop() else {
            continue;
        };
        command.origin = Origin::ThirdParty;
        command.dispatched_from = None;
        command.is_dispatcher = false;
        base.commands.push(command);
    }

    base
}

/// The command as typed after `toolr`: `docker image build`, or `hello` at top level.
fn command_path(cmd: &Command) -> String {
    if cmd.group.is_empty() {
        cmd.name.clone()
    } else {
        format!("{} {}", cmd.group.replace('.', " "), cmd.name)
    }
}

/// `tools/<file> defines <group> <name>, hiding the one from <pkg>`. The file comes from the
/// module path alone, so a package module reads as `<pkg>.py` rather than `__init__.py`.
fn shadow_message(local_module: &str, plugin_cmd: &Command, package: &str) -> String {
    let file = format!("{}.py", local_module.replace('.', "/"));
    format!(
        "{file} defines {}, hiding the one from {package}. \
         Choosing a winner in config is tracked in {RESOLVE_ISSUE_URL}",
        command_path(plugin_cmd)
    )
}

/// One warning for a command that `defs` (two or more packages) all define.
fn conflict_warning(defs: &[Definition]) -> PluginWarning {
    let first = &defs[0];
    let packages: Vec<&str> = defs.iter().map(|d| d.package.as_str()).collect();
    PluginWarning {
        package: first.package.clone(),
        path: first.path.clone(),
        kind: PluginWarningKind::Conflict,
        message: format!(
            "{} is defined by more than one plugin ({}), so it is disabled. \
             Uninstall all but one. Choosing a winner in config is tracked in {RESOLVE_ISSUE_URL}",
            command_path(&first.command),
            packages.join(", ")
        ),
    }
}
