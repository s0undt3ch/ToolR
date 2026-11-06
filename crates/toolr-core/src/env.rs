//! Env reads that must treat an empty value as unset.

use std::path::PathBuf;

/// Treats an empty value as unset, per the XDG spec: an empty path would resolve against the cwd.
pub fn non_empty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// An `XDG_*_HOME` value counts only when it is an absolute path; the spec says to ignore relative
/// ones, as `dirs` does, so they never resolve against the cwd.
pub fn xdg_dir_env(name: &str) -> Option<PathBuf> {
    non_empty_env_path(name).filter(|p| p.is_absolute())
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

    #[test]
    fn xdg_dir_env_rejects_unset_empty_and_relative() {
        let mut env = EnvGuard::write();
        env.remove("TOOLR_TEST_VAR");
        assert_eq!(xdg_dir_env("TOOLR_TEST_VAR"), None);
        env.set("TOOLR_TEST_VAR", "");
        assert_eq!(xdg_dir_env("TOOLR_TEST_VAR"), None);
        env.set("TOOLR_TEST_VAR", "relative/dir");
        assert_eq!(xdg_dir_env("TOOLR_TEST_VAR"), None);
    }

    #[test]
    fn xdg_dir_env_accepts_absolute() {
        let mut env = EnvGuard::write();
        let abs = std::env::temp_dir();
        env.set("TOOLR_TEST_VAR", &abs);
        assert_eq!(xdg_dir_env("TOOLR_TEST_VAR"), Some(abs));
    }
}
