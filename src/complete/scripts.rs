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
// The fish constant is added by Task 7.

/// Return the static completion script for the given shell.
pub fn completion_script(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => BASH_SCRIPT,
        Shell::Zsh => ZSH_SCRIPT,
        Shell::Fish => "", // Task 7
    }
}
