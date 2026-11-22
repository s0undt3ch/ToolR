//! Turn a literal filesystem path into a glob prefix.

use std::path::{Component, MAIN_SEPARATOR, Path};

use glob::Pattern;

/// Escape `path` so `glob` matches it literally, ready for a wildcard tail.
///
/// Only normal components are escaped. A Windows prefix such as `\\?\C:`
/// from `canonicalize` must stay verbatim: escaping its `?` turns it into
/// a UNC path that doesn't exist. Returns `None` for a non-UTF-8 path.
pub fn escape_path(path: &Path) -> Option<String> {
    let mut out = String::new();
    let mut needs_separator = false;
    for component in path.components() {
        let segment = match component {
            Component::Prefix(prefix) => {
                out.push_str(prefix.as_os_str().to_str()?);
                continue;
            }
            Component::RootDir => {
                out.push(MAIN_SEPARATOR);
                needs_separator = false;
                continue;
            }
            Component::CurDir => ".".to_string(),
            Component::ParentDir => "..".to_string(),
            Component::Normal(name) => Pattern::escape(name.to_str()?),
        };
        if needs_separator {
            out.push(MAIN_SEPARATOR);
        }
        out.push_str(&segment);
        needs_separator = true;
    }
    Some(out)
}

#[cfg(test)]
mod tests;
