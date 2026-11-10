//! Path env reads that must treat an empty value as unset. Use [`xdg_dir_env`] for `XDG_*_HOME`
//! and [`non_empty_env_path`] for other path variables (`HOME`, prefixes), which no spec requires
//! to be absolute. Every `TOOLR_*` boolean goes through [`env_truthy`], so they share one rule.

use std::path::PathBuf;

/// Treats an empty value as unset, since an empty path would resolve against the cwd.
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

/// `1`, `true` or `yes` is true and `0`, `false` or `no` is false, ignoring case and surrounding
/// whitespace; anything else is `None`.
pub fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// Whether the boolean variable `name` is on: [`parse_bool`] says true. Unset, empty and any other
/// value are off.
pub fn env_truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| parse_bool(&v) == Some(true))
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

    #[test]
    fn parse_bool_accepts_the_documented_spellings_only() {
        for on in ["1", "true", "TRUE", "Yes", " yes "] {
            assert_eq!(parse_bool(on), Some(true), "{on:?}");
        }
        for off in ["0", "false", "False", "NO", " no "] {
            assert_eq!(parse_bool(off), Some(false), "{off:?}");
        }
        for other in ["", "2", "on", "y", "enabled"] {
            assert_eq!(parse_bool(other), None, "{other:?}");
        }
    }

    #[test]
    fn env_truthy_is_true_only_for_a_true_spelling() {
        let mut env = EnvGuard::write();
        env.remove("TOOLR_TEST_VAR");
        assert!(!env_truthy("TOOLR_TEST_VAR"));
        for (value, expected) in [
            ("1", true),
            ("true", true),
            ("YES", true),
            ("0", false),
            ("false", false),
            ("", false),
            ("anything", false),
        ] {
            env.set("TOOLR_TEST_VAR", value);
            assert_eq!(env_truthy("TOOLR_TEST_VAR"), expected, "{value:?}");
        }
    }
}
