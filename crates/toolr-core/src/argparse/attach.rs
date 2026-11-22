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
            if let Some(prev_source) = seen.get(child.name.as_str())
                && *prev_source != source
            {
                return Err(AttachError::Collision {
                    parent: parent.clone(),
                    name: child.name.clone(),
                    a: (*prev_source).into(),
                    b: source.into(),
                });
            }
            seen.insert(&child.name, source);
        }
    }
    Ok(())
}

/// Drop intra-source duplicates from each parent's child list, returning
/// a warning string per duplicate that was discarded.
///
/// Two files matched by the same source can share a stem when an app
/// layout puts a command of the same name in multiple Django apps, or
/// when a glob is too broad. The historical behaviour was to silently
/// pass them through to clap, which then panicked at startup with a
/// "command name is duplicated" assert. Keeping the first occurrence
/// and warning preserves the build while telling the user exactly which
/// child name collided so they can tighten their scan glob or rename
/// one of the files.
///
/// Cross-source collisions remain a hard error — they're a structural
/// configuration problem that needs the user's resolution before any
/// build can produce a stable manifest. See [`validate_no_collisions`].
pub fn dedup_intra_source(children_by_parent: &mut HashMap<String, Vec<Command>>) -> Vec<String> {
    let mut warnings = Vec::new();
    for (parent, children) in children_by_parent.iter_mut() {
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        let original = std::mem::take(children);
        for child in original {
            let source = child.dispatched_from.clone().unwrap_or_default();
            let key = (child.name.clone(), source.clone());
            if seen.contains(&key) {
                warnings.push(format!(
                    "argparse source {source:?} grafted child {:?} multiple times under {parent:?}; \
                     keeping the first occurrence — tighten the scan glob or rename one of the files",
                    child.name
                ));
                continue;
            }
            seen.insert(key);
            children.push(child);
        }
    }
    warnings
}

fn closest_parent_hint(target: &str, candidates: &std::collections::BTreeSet<&str>) -> String {
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
                origin: Origin::Static,
                dispatched_from: Some(format!("argparse:{}", block.name)),
                is_dispatcher: false,
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
mod tests;
