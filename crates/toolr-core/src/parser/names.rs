//! Build-time rejection of names a terminal or shell would mishandle.

use crate::manifest::{Command, Group};
use crate::text::{escape_control_chars as escape, has_control_chars, has_invisible_format_chars};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameItem {
    Group,
    Command,
    GroupRef,
    Argument,
    Flag,
    Choice,
}

impl std::fmt::Display for NameItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Group => "group name",
            Self::Command => "command name",
            Self::GroupRef => "group reference",
            Self::Argument => "argument name",
            Self::Flag => "flag",
            Self::Choice => "choice value",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidName {
    /// Empty for groups: a group has no defining module in the manifest.
    pub module: String,
    /// The function for commands, the full group path for groups.
    pub owner: String,
    pub item: NameItem,
    /// Escaped for display, so it is safe to print.
    pub value: String,
    pub reason: &'static str,
}

pub(crate) const CONTROL: &str = "contains a control character";
pub(crate) const INVISIBLE: &str = "contains an invisible formatting character";
pub(crate) const WHITESPACE: &str = "contains whitespace";

/// Choices may hold spaces; they reach the shell quoted.
fn choice_violation(s: &str) -> Option<&'static str> {
    if has_control_chars(s) {
        Some(CONTROL)
    } else if has_invisible_format_chars(s) {
        Some(INVISIBLE)
    } else {
        None
    }
}

/// One reason per value, in order: control, invisible, whitespace.
fn name_violation(s: &str) -> Option<&'static str> {
    if let Some(reason) = choice_violation(s) {
        Some(reason)
    } else if s.chars().any(char::is_whitespace) {
        Some(WHITESPACE)
    } else {
        None
    }
}

pub(crate) fn validate_names(groups: &[Group], commands: &[Command]) -> Vec<InvalidName> {
    let mut out = Vec::new();
    let group_paths: Vec<String> = groups.iter().map(Group::full_path).collect();
    for g in groups {
        if let Some(reason) = name_violation(&g.name) {
            out.push(InvalidName {
                module: String::new(),
                owner: escape(&g.full_path()),
                item: NameItem::Group,
                value: escape(&g.name),
                reason,
            });
        }
    }
    for c in commands {
        let mut push = |item, value: &str, reason| {
            out.push(InvalidName {
                module: escape(&c.module),
                owner: escape(&c.function),
                item,
                value: escape(value),
                reason,
            });
        };
        if let Some(reason) = name_violation(&c.name) {
            push(NameItem::Command, &c.name, reason);
        }
        // A ref under a declared group is already reported against that group.
        if let Some(reason) = name_violation(&c.group) {
            let under_declared = group_paths.iter().any(|p| {
                c.group == *p
                    || c.group
                        .strip_prefix(p.as_str())
                        .is_some_and(|r| r.starts_with('.'))
            });
            if !under_declared {
                push(NameItem::GroupRef, &c.group, reason);
            }
        }
        for a in &c.arguments {
            // A bad flag usually derives the name too; report it once.
            if let Some(reason) = a.long_flag.as_deref().and_then(name_violation) {
                push(
                    NameItem::Flag,
                    a.long_flag.as_deref().unwrap_or_default(),
                    reason,
                );
            } else if let Some(reason) = name_violation(&a.name) {
                push(NameItem::Argument, &a.name, reason);
            }
            for v in &a.allowed_values {
                if let Some(reason) = choice_violation(v) {
                    push(NameItem::Choice, v, reason);
                }
            }
        }
    }
    out
}
