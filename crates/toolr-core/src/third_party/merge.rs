//! Merge parsed third-party fragments into a project `Manifest`.

use std::collections::{HashMap, HashSet};

use log::debug;

use super::model::{FragmentArgument, FragmentCommand, FragmentGroup, ManifestFragment};
use super::parse::ThirdPartyError;
use crate::manifest::{Argument, Command, Group, Manifest, Origin};

/// Consume `fragments`, merging their groups + commands into `base`.
///
/// Conflict resolution:
/// - A group/command pair already present in `base` (from `tools/**/*.py`)
///   wins; the third-party entry is skipped (with a debug log).
/// - A group/command pair declared by two different third-party packages
///   produces `ThirdPartyError::DuplicateCommand`.
/// - Groups merge by `name`: if a third-party fragment declares a group
///   already present in `base` or in a prior fragment, the existing
///   group's title/description are kept.
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

    let mut known_groups: HashSet<String> = base.groups.iter().map(|g| g.name.clone()).collect();

    for fragment in fragments {
        for fg in fragment.groups {
            if known_groups.insert(fg.name.clone()) {
                base.groups.push(group_from_fragment(fg));
            }
        }
        for fc in fragment.commands {
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
            base.commands.push(command_from_fragment(fc));
        }
    }

    Ok(base)
}

fn group_from_fragment(fg: FragmentGroup) -> Group {
    Group {
        name: fg.name,
        title: fg.title,
        description: fg.description,
        parent: None,
        origin: Origin::ThirdParty,
    }
}

fn command_from_fragment(fc: FragmentCommand) -> Command {
    Command {
        name: fc.name,
        group: fc.group,
        module: fc.module,
        function: fc.function,
        summary: fc.summary,
        description: fc.description,
        arguments: fc
            .arguments
            .into_iter()
            .map(argument_from_fragment)
            .collect(),
        origin: Origin::ThirdParty,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

fn argument_from_fragment(fa: FragmentArgument) -> Argument {
    Argument {
        name: fa.name,
        kind: fa.kind,
        help: fa.help,
        default: fa.default,
        type_annotation: fa.type_annotation,
        // Third-party fragments don't carry structured type info yet —
        // they ship pre-validated string defaults and rely on whatever
        // the manifest builder can re-derive at execute time. A future
        // schema extension will let fragments record their own
        // SupportedType.
        resolved_type: None,
        path_constraints: None,
        allowed_values: fa.allowed_values,
        metadata: crate::manifest::ArgMetadata::default(),
        // Third-party manifest fragments aren't argparse-grafted, so
        // they have no source-literal flag to preserve.
        long_flag: None,
    }
}
