//! Embedded shell-completion scripts.

use std::fmt;
use std::str::FromStr;

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl fmt::Display for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
        })
    }
}

impl FromStr for Shell {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "bash" => Ok(Shell::Bash),
            "zsh" => Ok(Shell::Zsh),
            "fish" => Ok(Shell::Fish),
            other => Err(anyhow!(
                "unsupported shell: {other} (expected bash, zsh, or fish)"
            )),
        }
    }
}

const BASH_SCRIPT: &str = include_str!("scripts/bash.sh");
const ZSH_SCRIPT: &str = include_str!("scripts/zsh.zsh");
const FISH_SCRIPT: &str = include_str!("scripts/fish.fish");

/// Return the static completion script for the given shell.
pub fn completion_script(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => BASH_SCRIPT,
        Shell::Zsh => ZSH_SCRIPT,
        Shell::Fish => FISH_SCRIPT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_display_matches_clap_value_strings() {
        // The Display impl is used in CLI rendering — pin every variant.
        assert_eq!(Shell::Bash.to_string(), "bash");
        assert_eq!(Shell::Zsh.to_string(), "zsh");
        assert_eq!(Shell::Fish.to_string(), "fish");
    }

    #[test]
    fn shell_from_str_accepts_the_three_supported_names() {
        assert_eq!("bash".parse::<Shell>().unwrap(), Shell::Bash);
        assert_eq!("zsh".parse::<Shell>().unwrap(), Shell::Zsh);
        assert_eq!("fish".parse::<Shell>().unwrap(), Shell::Fish);
    }

    #[test]
    fn shell_from_str_rejects_unknown_shell_with_helpful_message() {
        let err = "powershell".parse::<Shell>().unwrap_err();
        let s = err.to_string();
        assert!(s.contains("unsupported shell"));
        assert!(s.contains("powershell"));
        // Error mentions the supported set so users have the next-action keywords.
        assert!(s.contains("bash"));
        assert!(s.contains("zsh"));
        assert!(s.contains("fish"));
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
        for s in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            assert!(
                !completion_script(s).is_empty(),
                "{s} should have an embedded script",
            );
        }
    }
}
