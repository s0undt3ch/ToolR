//! Embedded shell-completion scripts.

use std::fmt;
use std::str::FromStr;

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

impl fmt::Display for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::PowerShell => "powershell",
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
            "powershell" | "pwsh" => Ok(Shell::PowerShell),
            other => Err(anyhow!(
                "unsupported shell: {other} (expected bash, zsh, fish, or powershell)"
            )),
        }
    }
}

const BASH_SCRIPT: &str = include_str!("scripts/bash.sh");
const ZSH_SCRIPT: &str = include_str!("scripts/zsh.zsh");
const FISH_SCRIPT: &str = include_str!("scripts/fish.fish");
const POWERSHELL_SCRIPT: &str = include_str!("scripts/powershell.ps1");

/// Return the static completion script for the given shell.
pub fn completion_script(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => BASH_SCRIPT,
        Shell::Zsh => ZSH_SCRIPT,
        Shell::Fish => FISH_SCRIPT,
        Shell::PowerShell => POWERSHELL_SCRIPT,
    }
}

#[cfg(test)]
mod tests;
