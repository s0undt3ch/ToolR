//! Environment-variable helpers shared by toolr's crates.

use std::path::PathBuf;

/// The value of `name` as a path, or `None` when it is unset or empty.
///
/// An empty value reads as unset, as the XDG base directory spec requires; using it would resolve
/// relative to the cwd.
pub fn non_empty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::EnvGuard;

    #[test]
    fn unset_variable_is_none() {
        let mut env = EnvGuard::write();
        env.remove("TOOLR_TEST_VAR");
        assert_eq!(non_empty_env_path("TOOLR_TEST_VAR"), None);
    }

    #[test]
    fn empty_variable_is_none() {
        let mut env = EnvGuard::write();
        env.set("TOOLR_TEST_VAR", "");
        assert_eq!(non_empty_env_path("TOOLR_TEST_VAR"), None);
    }

    #[test]
    fn set_variable_is_its_path() {
        let mut env = EnvGuard::write();
        env.set("TOOLR_TEST_VAR", "/some/dir");
        assert_eq!(
            non_empty_env_path("TOOLR_TEST_VAR"),
            Some(PathBuf::from("/some/dir"))
        );
    }
}
