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
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn escapes_metacharacters_in_components() {
        let escaped = escape_path(Path::new("/src/[wip]/x*y?/repo")).unwrap();
        assert_eq!(escaped, "/src/[[]wip[]]/x[*]y[?]/repo");
    }

    #[test]
    fn relative_path_keeps_its_shape() {
        let escaped = escape_path(Path::new("a/[b]")).unwrap();
        assert_eq!(escaped, format!("a{MAIN_SEPARATOR}[[]b[]]"));
    }

    #[test]
    fn dot_components_stay_unescaped() {
        let escaped = escape_path(Path::new("./[a]/../b")).unwrap();
        let sep = MAIN_SEPARATOR;
        assert_eq!(escaped, format!(".{sep}[[]a[]]{sep}..{sep}b"));
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_prefix_stays_literal() {
        let escaped = escape_path(Path::new(r"\\?\C:\src\[wip]")).unwrap();
        assert_eq!(escaped, r"\\?\C:\src\[[]wip[]]");
    }

    #[test]
    fn canonical_path_globs_to_itself() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("[wip]");
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        let canonical = dir.canonicalize().unwrap();
        let pattern = format!("{}/*", escape_path(&canonical).unwrap());
        let found: Vec<_> = glob::glob(&pattern).unwrap().flatten().collect();
        assert_eq!(found, [canonical.join("inner")]);
    }
}
