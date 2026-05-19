//! Merge a dynamic-layer payload into a base (static) manifest.

use std::collections::HashSet;

use super::payload::DynamicPayload;
use crate::manifest::Manifest;

/// Merge `payload` into `base`. Returns the resulting manifest.
///
/// Conflict policy:
/// - A group present in `base.groups` with the same `name` as one in the
///   payload keeps the static definition; the dynamic copy is dropped.
/// - A command present in `base.commands` with the same `(group, name)`
///   as one in the payload keeps the static definition; the dynamic copy
///   is dropped.
/// - The resulting manifest's `dynamic_hash` is **not** touched here —
///   callers stamp it from `compute_dynamic_hash` after the venv state
///   they used to produce `payload`.
pub fn merge_dynamic(mut base: Manifest, payload: DynamicPayload) -> Manifest {
    let existing_groups: HashSet<String> = base.groups.iter().map(|g| g.name.clone()).collect();
    let existing_cmds: HashSet<(String, String)> = base
        .commands
        .iter()
        .map(|c| (c.group.clone(), c.name.clone()))
        .collect();

    for g in payload.groups {
        if !existing_groups.contains(&g.name) {
            base.groups.push(g);
        }
    }
    for c in payload.commands {
        let key = (c.group.clone(), c.name.clone());
        if !existing_cmds.contains(&key) {
            base.commands.push(c);
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Command, Group, Manifest, Origin, SCHEMA_VERSION};

    fn cmd(group: &str, name: &str, origin: Origin) -> Command {
        Command {
            name: name.into(),
            group: group.into(),
            module: format!("tools.{group}"),
            function: name.replace('-', "_"),
            summary: "".into(),
            description: "".into(),
            arguments: vec![],
            imports: vec![],
            origin,
            dispatched_from: None,
        }
    }

    fn grp(name: &str, origin: Origin) -> Group {
        Group {
            name: name.into(),
            title: name.into(),
            description: "".into(),
            parent: None,
            origin,
        }
    }

    fn base_with(groups: Vec<Group>, commands: Vec<Command>) -> Manifest {
        Manifest {
            schema_version: SCHEMA_VERSION,
            static_hash: "h".into(),
            dynamic_hash: "".into(),
            groups,
            commands,
        }
    }

    #[test]
    fn dynamic_only_entries_get_appended() {
        let base = base_with(vec![], vec![]);
        let payload = DynamicPayload {
            payload_schema_version: 1,
            groups: vec![grp("legacy", Origin::Dynamic)],
            commands: vec![cmd("legacy", "widget", Origin::Dynamic)],
            warnings: vec![],
        };
        let merged = merge_dynamic(base, payload);
        assert_eq!(merged.groups.len(), 1);
        assert_eq!(merged.commands.len(), 1);
        assert_eq!(merged.groups[0].origin, Origin::Dynamic);
    }

    #[test]
    fn static_group_wins_over_dynamic_with_same_name() {
        let base = base_with(vec![grp("ci", Origin::Static)], vec![]);
        let payload = DynamicPayload {
            payload_schema_version: 1,
            // Dynamic emits a "ci" group with conflicting metadata.
            groups: vec![Group {
                name: "ci".into(),
                title: "FROM DYNAMIC".into(),
                description: "".into(),
                parent: None,
                origin: Origin::Dynamic,
            }],
            commands: vec![],
            warnings: vec![],
        };
        let merged = merge_dynamic(base, payload);
        assert_eq!(merged.groups.len(), 1);
        assert_eq!(merged.groups[0].origin, Origin::Static);
        assert_ne!(merged.groups[0].title, "FROM DYNAMIC");
    }

    #[test]
    fn static_command_wins_over_dynamic_with_same_group_and_name() {
        let base = base_with(
            vec![grp("ci", Origin::Static)],
            vec![cmd("ci", "hello", Origin::Static)],
        );
        let payload = DynamicPayload {
            payload_schema_version: 1,
            groups: vec![],
            commands: vec![cmd("ci", "hello", Origin::Dynamic)],
            warnings: vec![],
        };
        let merged = merge_dynamic(base, payload);
        assert_eq!(merged.commands.len(), 1);
        assert_eq!(merged.commands[0].origin, Origin::Static);
    }

    #[test]
    fn merge_preserves_existing_dynamic_hash() {
        let mut base = base_with(vec![], vec![]);
        base.dynamic_hash = "preserved".into();
        let merged = merge_dynamic(
            base,
            DynamicPayload {
                payload_schema_version: 1,
                groups: vec![],
                commands: vec![],
                warnings: vec![],
            },
        );
        assert_eq!(merged.dynamic_hash, "preserved");
    }
}
