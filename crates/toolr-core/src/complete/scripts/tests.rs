use super::*;

#[test]
fn shell_display_matches_clap_value_strings() {
    // The Display impl is used in CLI rendering — pin every variant.
    assert_eq!(Shell::Bash.to_string(), "bash");
    assert_eq!(Shell::Zsh.to_string(), "zsh");
    assert_eq!(Shell::Fish.to_string(), "fish");
    assert_eq!(Shell::PowerShell.to_string(), "powershell");
}

#[test]
fn shell_from_str_accepts_every_supported_name() {
    assert_eq!("bash".parse::<Shell>().unwrap(), Shell::Bash);
    assert_eq!("zsh".parse::<Shell>().unwrap(), Shell::Zsh);
    assert_eq!("fish".parse::<Shell>().unwrap(), Shell::Fish);
    assert_eq!("powershell".parse::<Shell>().unwrap(), Shell::PowerShell);
    assert_eq!("pwsh".parse::<Shell>().unwrap(), Shell::PowerShell);
}

#[test]
fn shell_from_str_rejects_unknown_shell_with_helpful_message() {
    let err = "nushell".parse::<Shell>().unwrap_err();
    let s = err.to_string();
    assert!(s.contains("unsupported shell"));
    assert!(s.contains("nushell"));
    // Error mentions the supported set so users have the next-action keywords.
    assert!(s.contains("bash"));
    assert!(s.contains("zsh"));
    assert!(s.contains("fish"));
    assert!(s.contains("powershell"));
}

#[test]
fn shell_from_str_is_case_sensitive() {
    // Document the behaviour: "Bash" doesn't parse — clap canonicalises
    // values via Display, so anything we route through Shell::from_str
    // sees the lowercase form. If we change to case-insensitive in the
    // future this test flips.
    assert!("Bash".parse::<Shell>().is_err());
    assert!("BASH".parse::<Shell>().is_err());
}

#[test]
fn completion_script_is_non_empty_for_every_shell() {
    for s in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
        assert!(
            !completion_script(s).is_empty(),
            "{s} should have an embedded script",
        );
    }
}

#[test]
fn powershell_script_is_ascii() {
    // Windows PowerShell 5.1 reads a BOM-less script in the ANSI code page, and must read this
    // one to reach the version guard and print its warning.
    assert!(completion_script(Shell::PowerShell).is_ascii());
}
