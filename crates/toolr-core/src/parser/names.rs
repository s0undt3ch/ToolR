//! Build-time rejection of names a terminal or shell would mishandle.

use crate::manifest::{Command, Group};
use crate::text::{escape_control_chars as escape, has_control_chars};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameItem {
    Group,
    Command,
    Flag,
    Choice,
}

impl std::fmt::Display for NameItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Group => "group name",
            Self::Command => "command name",
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

const CONTROL: &str = "contains a control character";
const WHITESPACE: &str = "contains whitespace";

/// One reason per value; a control character outranks whitespace.
fn strict_reason(s: &str) -> Option<&'static str> {
    if has_control_chars(s) {
        Some(CONTROL)
    } else if s.chars().any(char::is_whitespace) {
        Some(WHITESPACE)
    } else {
        None
    }
}

pub(crate) fn validate_names(groups: &[Group], commands: &[Command]) -> Vec<InvalidName> {
    let mut out = Vec::new();
    for g in groups {
        if let Some(reason) = strict_reason(&g.name) {
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
        if let Some(reason) = strict_reason(&c.name) {
            push(NameItem::Command, &c.name, reason);
        }
        for a in &c.arguments {
            if let Some(reason) = a.long_flag.as_deref().and_then(strict_reason) {
                push(
                    NameItem::Flag,
                    a.long_flag.as_deref().unwrap_or_default(),
                    reason,
                );
            }
            for v in &a.allowed_values {
                if has_control_chars(v) {
                    push(NameItem::Choice, v, CONTROL);
                }
            }
        }
    }
    out
}
