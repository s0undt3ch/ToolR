//! Install the embedded shell-completion script into the standard
//! location for the target shell.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::scripts::{Shell, completion_script};

pub struct InstallOptions {
    pub shell: Shell,
    /// Override for `$XDG_DATA_HOME`. `None` means read from environment
    /// at call time (callers usually pass `None` in production).
    pub xdg_data_home: Option<PathBuf>,
    /// Override for `$XDG_CONFIG_HOME`. `None` means read from
    /// environment at call time.
    pub xdg_config_home: Option<PathBuf>,
    /// Path to use as the user's home directory.
    pub home: PathBuf,
    /// Overwrite a non-matching existing file without prompting.
    pub force: bool,
    /// Currently informational only — the file API is non-interactive.
    /// Reserved for future "prompt before overwrite" behaviour in the
    /// CLI dispatcher.
    pub interactive: bool,
}

#[derive(Debug)]
pub enum InstallOutcome {
    Wrote { path: PathBuf },
    AlreadyInstalled { path: PathBuf },
    SkippedNeedsForce { path: PathBuf },
}

/// Compute the target install path for `shell`.
pub fn install_path_for(
    shell: Shell,
    xdg_override: Option<&Path>,
    home: &Path,
) -> Result<PathBuf> {
    match shell {
        Shell::Bash => {
            let base = xdg_override
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".local/share"));
            Ok(base.join("bash-completion/completions/toolr"))
        }
        Shell::Zsh => Ok(home.join(".zfunc/_toolr")),
        Shell::Fish => {
            let base = xdg_override
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".config"));
            Ok(base.join("fish/completions/toolr.fish"))
        }
    }
}

/// Write the embedded script to the chosen location.
pub fn install_script(opts: &InstallOptions) -> Result<InstallOutcome> {
    let path = match opts.shell {
        Shell::Bash => {
            install_path_for(opts.shell, opts.xdg_data_home.as_deref(), &opts.home)?
        }
        Shell::Fish => {
            install_path_for(opts.shell, opts.xdg_config_home.as_deref(), &opts.home)?
        }
        Shell::Zsh => install_path_for(opts.shell, None, &opts.home)?,
    };

    let payload = completion_script(opts.shell);
    if payload.is_empty() {
        return Err(anyhow!(
            "no embedded completion script for {}",
            opts.shell
        ));
    }

    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing == payload {
            return Ok(InstallOutcome::AlreadyInstalled { path });
        }
        if !opts.force {
            return Ok(InstallOutcome::SkippedNeedsForce { path });
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, payload)?;
    Ok(InstallOutcome::Wrote { path })
}
