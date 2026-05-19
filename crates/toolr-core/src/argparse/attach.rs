//! Graft scanned commands under parent dispatchers, with validation.

use std::collections::HashMap;

use thiserror::Error;

use crate::argparse::config::ArgparseBlock;
use crate::argparse::scan::ScannedCommand;
use crate::manifest::{Command, Origin};

#[derive(Debug, Error)]
pub enum AttachError {
    #[error("source {block:?} attaches to unknown parent {parent:?}{hint}")]
    UnknownParent {
        block: String,
        parent: String,
        hint: String,
    },
    #[error("parent {parent:?} has no DispatchCommand-annotated keyword parameter")]
    NotADispatcher { parent: String },
    #[error(
        "child name collision on parent {parent:?}: {name:?} is provided by both {a:?} and {b:?}"
    )]
    Collision {
        parent: String,
        name: String,
        a: String,
        b: String,
    },
}

pub fn validate_attachments(
    blocks: &[ArgparseBlock],
    parents: &HashMap<String, (String, String)>,
) -> Result<(), AttachError> {
    let known: std::collections::BTreeSet<&str> = parents.keys().map(|s| s.as_str()).collect();
    for block in blocks {
        for attachment in &block.attach {
            if !parents.contains_key(&attachment.parent) {
                let hint = closest_parent_hint(&attachment.parent, &known);
                return Err(AttachError::UnknownParent {
                    block: block.name.clone(),
                    parent: attachment.parent.clone(),
                    hint,
                });
            }
        }
    }
    Ok(())
}

pub fn validate_no_collisions(
    children_by_parent: &HashMap<String, Vec<Command>>,
) -> Result<(), AttachError> {
    for (parent, children) in children_by_parent {
        let mut seen: HashMap<&str, &str> = HashMap::new();
        for child in children {
            let source = child.dispatched_from.as_deref().unwrap_or("?");
            if let Some(prev_source) = seen.get(child.name.as_str()) {
                if *prev_source != source {
                    return Err(AttachError::Collision {
                        parent: parent.clone(),
                        name: child.name.clone(),
                        a: (*prev_source).into(),
                        b: source.into(),
                    });
                }
            }
            seen.insert(&child.name, source);
        }
    }
    Ok(())
}

fn closest_parent_hint(
    target: &str,
    candidates: &std::collections::BTreeSet<&str>,
) -> String {
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        let dist = edit_distance(target, candidate);
        if best.is_none_or(|(d, _)| dist < d) {
            best = Some((dist, candidate));
        }
    }
    match best {
        Some((d, name)) if d <= 3 => format!(" (did you mean {name:?}?)"),
        _ => String::new(),
    }
}

pub fn graft_children(
    block: &ArgparseBlock,
    scanned: &[ScannedCommand],
    parents: &HashMap<String, (String, String)>,
) -> Result<HashMap<String, Vec<Command>>, AttachError> {
    let mut out: HashMap<String, Vec<Command>> = HashMap::new();
    for attachment in &block.attach {
        let (module, function) =
            parents
                .get(&attachment.parent)
                .ok_or_else(|| AttachError::UnknownParent {
                    block: block.name.clone(),
                    parent: attachment.parent.clone(),
                    hint: String::new(),
                })?;
        let entries = out.entry(attachment.parent.clone()).or_default();
        for sc in scanned {
            entries.push(Command {
                name: sc.name.clone(),
                group: attachment.parent.clone(),
                module: module.clone(),
                function: function.clone(),
                summary: sc.summary.clone(),
                description: sc.description.clone(),
                arguments: sc.arguments.clone(),
                imports: vec![],
                origin: Origin::Static,
                dispatched_from: Some(format!("argparse:{}", block.name)),
            });
        }
    }
    Ok(out)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev = (0..=b.len()).collect::<Vec<_>>();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::argparse::config::Attachment;
    use crate::manifest::Origin;

    fn parents_with_django() -> HashMap<String, (String, String)> {
        let mut m = HashMap::new();
        m.insert(
            "django".into(),
            ("tools.dispatcher".into(), "django".into()),
        );
        m
    }

    fn block(name: &str, parent: &str) -> ArgparseBlock {
        ArgparseBlock {
            name: name.into(),
            scan_paths: vec![],
            common_args: vec![],
            attach: vec![Attachment {
                parent: parent.into(),
            }],
        }
    }

    #[test]
    fn unknown_parent_with_hint() {
        let err = validate_attachments(&[block("django", "djnago")], &parents_with_django())
            .unwrap_err();
        match err {
            AttachError::UnknownParent { hint, .. } => {
                assert!(
                    hint.contains("django"),
                    "hint did not surface the close match: {hint:?}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn collision_is_detected() {
        let cmd = |source: &str| Command {
            name: "migrate".into(),
            group: "django".into(),
            module: "tools.dispatcher".into(),
            function: "django".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Static,
            dispatched_from: Some(source.into()),
        };
        let children: HashMap<String, Vec<Command>> =
            HashMap::from([("django".into(), vec![cmd("a"), cmd("b")])]);
        let err = validate_no_collisions(&children).unwrap_err();
        match err {
            AttachError::Collision { name, .. } => assert_eq!(name, "migrate"),
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn graft_emits_one_child_per_scanned_with_dispatched_from() {
        let block = ArgparseBlock {
            name: "django".into(),
            scan_paths: vec![],
            common_args: vec![],
            attach: vec![Attachment {
                parent: "django".into(),
            }],
        };
        let scanned = vec![ScannedCommand {
            name: "migrate".into(),
            summary: "Migrate".into(),
            description: "".into(),
            arguments: vec![],
            warnings: vec![],
        }];
        let children = graft_children(&block, &scanned, &parents_with_django()).unwrap();
        assert_eq!(children.len(), 1);
        let django_children = children.get("django").unwrap();
        assert_eq!(django_children[0].name, "migrate");
        assert_eq!(
            django_children[0].dispatched_from.as_deref(),
            Some("argparse:django")
        );
        assert_eq!(django_children[0].module, "tools.dispatcher");
        assert_eq!(django_children[0].function, "django");
    }
}
