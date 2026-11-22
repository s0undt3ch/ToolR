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
