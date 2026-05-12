//! Embedded shell-completion scripts.

use std::fmt;

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

/// Return the static completion script for the given shell. Filled in
/// by Tasks 5-7.
pub fn completion_script(_shell: Shell) -> &'static str {
    ""
}
