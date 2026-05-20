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
    /// The embedded script is now on disk at `path`. `prior` describes
    /// what (if anything) was there before the write so the caller can
    /// distinguish a fresh install, a forced re-write of identical
    /// content, and a forced replacement of differing content.
    Wrote { path: PathBuf, prior: PriorState },
    /// `--force` was not set and the on-disk content already matched
    /// the embedded payload; nothing was written.
    AlreadyInstalled { path: PathBuf },
    /// `--force` was not set and the on-disk content differs from the
    /// embedded payload; nothing was written.
    SkippedNeedsForce { path: PathBuf },
}

#[derive(Debug)]
pub enum PriorState {
    /// No file existed at the target path before the write.
    None,
    /// A file existed and matched the new payload byte-for-byte. The
    /// caller passed `--force`, so the file was overwritten anyway.
    Identical,
    /// A file existed and its content differed from the new payload.
    Differed,
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

    let prior = match std::fs::read_to_string(&path) {
        Ok(existing) if existing == payload => {
            // File matches the embedded payload. Honour `--force`
            // literally and rewrite anyway (useful when the caller
            // wants to refresh mtime or recover from a manual edit
            // that happened to round-trip back to the canonical
            // content); otherwise treat as a no-op.
            if !opts.force {
                return Ok(InstallOutcome::AlreadyInstalled { path });
            }
            PriorState::Identical
        }
        Ok(_) => {
            if !opts.force {
                return Ok(InstallOutcome::SkippedNeedsForce { path });
            }
            PriorState::Differed
        }
        Err(_) => PriorState::None,
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, payload)?;
    Ok(InstallOutcome::Wrote { path, prior })
}
