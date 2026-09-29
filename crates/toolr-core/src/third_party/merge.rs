//! Merge parsed third-party fragments into a project `Manifest`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::model::ManifestFragment;
use super::parse::ThirdPartyError;
use crate::manifest::{Command, Group, Manifest, Origin, PluginWarning, PluginWarningKind};

/// Consume `fragments`, each paired with the file it was read from, merging their groups +
/// commands into `base`.
///
/// Conflict resolution:
/// - A group/command pair already present in `base` (from `tools/**/*.py`)
///   wins; the third-party entry is skipped and a `Shadowed` warning is
///   appended to `base.plugin_warnings`.
/// - A group/command pair declared by two different third-party packages
///   produces `ThirdPartyError::DuplicateCommand`.
/// - Groups merge by `full_path()`: if a third-party fragment declares a
///   group already present in `base` or in a prior fragment, the existing
///   group's title/description are kept.
///
/// Merged entries are tagged `Origin::ThirdParty`, and commands lose any
/// argparse-dispatch flags, which only a local build may set.
pub fn merge_into_manifest(
    mut base: Manifest,
    fragments: Vec<(ManifestFragment, PathBuf)>,
) -> Result<Manifest, ThirdPartyError> {
    // (group, command) → the local command's module, which wins and names the shadow warning.
    let local: HashMap<(String, String), String> = base
        .commands
        .iter()
        .map(|c| ((c.group.clone(), c.name.clone()), c.module.clone()))
        .collect();
    // (group, command) → the plugin that merged it, to catch plugin-to-plugin collisions.
    let mut owner: HashMap<(String, String), String> = HashMap::new();

    let mut known_groups: HashSet<String> = base.groups.iter().map(Group::full_path).collect();

    for (fragment, path) in fragments {
        for mut fg in fragment.groups {
            if known_groups.insert(fg.full_path()) {
                fg.origin = Origin::ThirdParty;
                base.groups.push(fg);
            }
        }
        for mut fc in fragment.commands {
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
            if let Some(first) = owner.get(&key) {
                return Err(ThirdPartyError::DuplicateCommand {
                    group: fc.group,
                    name: fc.name,
                    first_package: first.clone(),
                    second_package: fragment.package.clone(),
                });
            }
            owner.insert(key, fragment.package.clone());
            fc.origin = Origin::ThirdParty;
            fc.dispatched_from = None;
            fc.is_dispatcher = false;
            base.commands.push(fc);
        }
    }

    Ok(base)
}

/// `tools/<file> defines <group> <name>, hiding the one from <pkg>`. The file comes from the
/// module path alone, so a package module reads as `<pkg>.py` rather than `__init__.py`.
fn shadow_message(local_module: &str, plugin_cmd: &Command, package: &str) -> String {
    let file = format!("{}.py", local_module.replace('.', "/"));
    let command = if plugin_cmd.group.is_empty() {
        plugin_cmd.name.clone()
    } else {
        format!("{} {}", plugin_cmd.group.replace('.', " "), plugin_cmd.name)
    };
    format!("{file} defines {command}, hiding the one from {package}")
}
