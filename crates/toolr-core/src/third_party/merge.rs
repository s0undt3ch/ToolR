//! Merge parsed third-party fragments into a project `Manifest`.

use std::collections::{HashMap, HashSet};

use log::debug;

use super::model::ManifestFragment;
use super::parse::ThirdPartyError;
use crate::manifest::{Group, Manifest, Origin};

/// Consume `fragments`, merging their groups + commands into `base`.
///
/// Conflict resolution:
/// - A group/command pair already present in `base` (from `tools/**/*.py`)
///   wins; the third-party entry is skipped (with a debug log).
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
    fragments: Vec<ManifestFragment>,
) -> Result<Manifest, ThirdPartyError> {
    // (group, command) → package that defined it. Used to detect
    // third-party-to-third-party collisions.
    let mut owner: HashMap<(String, String), String> = HashMap::new();
    for cmd in &base.commands {
        owner.insert(
            (cmd.group.clone(), cmd.name.clone()),
            "<project>".to_string(),
        );
    }

    let mut known_groups: HashSet<String> = base.groups.iter().map(Group::full_path).collect();

    for fragment in fragments {
        for mut fg in fragment.groups {
            if known_groups.insert(fg.full_path()) {
                fg.origin = Origin::ThirdParty;
                base.groups.push(fg);
            }
        }
        for mut fc in fragment.commands {
            let key = (fc.group.clone(), fc.name.clone());
            if let Some(first) = owner.get(&key) {
                if first == "<project>" {
                    debug!(
                        "third-party package `{}` declared command \
                         `{}/{}`, but `tools/` already defines it; \
                         keeping local",
                        fragment.package, fc.group, fc.name,
                    );
                    continue;
                }
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
