//! Install the embedded shell-completion script into the standard
//! location for the target shell.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::scripts::{Shell, completion_script};

pub struct InstallOptions {
    pub shell: Shell,
    /// `$XDG_DATA_HOME` for the bash and PowerShell scripts. `None` falls back to
    /// `$HOME/.local/share` for bash and to `data_dir` for PowerShell.
    pub xdg_data_home: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME` for the fish script. `None` falls back to
    /// `$HOME/.config`.
    pub xdg_config_home: Option<PathBuf>,
    /// Path to use as the user's home directory.
    pub home: PathBuf,
    /// Platform data directory (`dirs::data_dir()`), used for PowerShell when
    /// `xdg_data_home` is unset.
    pub data_dir: Option<PathBuf>,
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

/// Compute the target install path for `shell`. `data_dir` is only read for PowerShell, which has no
/// completion directory of its own, so its script lives under toolr's data directory.
pub fn install_path_for(
    shell: Shell,
    xdg_override: Option<&Path>,
    home: &Path,
    data_dir: Option<&Path>,
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
        Shell::PowerShell => xdg_override
            .or(data_dir)
            .map(|base| base.join("toolr/completions/toolr.ps1"))
            .ok_or_else(|| anyhow!("no data directory; set XDG_DATA_HOME")),
    }
}

/// Write the embedded script to the chosen location.
pub fn install_script(opts: &InstallOptions) -> Result<InstallOutcome> {
    let (xdg, data_dir) = match opts.shell {
        Shell::Bash => (opts.xdg_data_home.as_deref(), None),
        Shell::Fish => (opts.xdg_config_home.as_deref(), None),
        Shell::Zsh => (None, None),
        Shell::PowerShell => (opts.xdg_data_home.as_deref(), opts.data_dir.as_deref()),
    };
    let path = install_path_for(opts.shell, xdg, &opts.home, data_dir)?;

    let payload = completion_script(opts.shell);
    if payload.is_empty() {
        return Err(anyhow!("no embedded completion script for {}", opts.shell));
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

/// Well-known bash-completion 2.x loader scripts, relative to the sysroot.
/// 1.x never reads `~/.local/share/bash-completion/completions/`, so its
/// loaders are left out, and so is `etc/profile.d/bash_completion.sh`, which
/// Homebrew's 1.x formula installs too. `etc/bash_completion` is a 2.x shim
/// on Debian.
pub(crate) const BASH_COMPLETION_SYSROOT_PATHS: &[&str] = &[
    "usr/share/bash-completion/bash_completion",
    "etc/bash_completion",
    "usr/local/share/bash-completion/bash_completion",
    "opt/homebrew/share/bash-completion/bash_completion",
    "home/linuxbrew/.linuxbrew/share/bash-completion/bash_completion",
    "opt/local/share/bash-completion/bash_completion",
];

/// Paths where bash-completion's loader script may live. Probes files, not
/// directories: other packages create `share/bash-completion/completions/`
/// without installing the loader.
pub(crate) fn bash_completion_candidates(
    sysroot: &Path,
    xdg_data_dirs: Option<&OsStr>,
    homebrew_prefix: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = BASH_COMPLETION_SYSROOT_PATHS
        .iter()
        .map(|rel| sysroot.join(rel))
        .collect();
    if let Some(dirs) = xdg_data_dirs {
        out.extend(
            std::env::split_paths(dirs)
                .filter(|dir| !dir.as_os_str().is_empty())
                .map(|dir| dir.join("bash-completion/bash_completion")),
        );
    }
    if let Some(prefix) = homebrew_prefix {
        out.push(prefix.join("share/bash-completion/bash_completion"));
    }
    out
}

/// Whether bash-completion looks installed. Unknown layouts read as "not
/// installed", so the caller errs toward printing its hint.
pub fn bash_completion_installed(
    sysroot: &Path,
    xdg_data_dirs: Option<&OsStr>,
    homebrew_prefix: Option<&Path>,
) -> bool {
    bash_completion_candidates(sysroot, xdg_data_dirs, homebrew_prefix)
        .iter()
        .any(|p| p.is_file())
}

/// Quote `s` as a single word for POSIX shells: bare when every character
/// is in a conservative safe set, single-quoted otherwise. It doesn't guard a
/// leading `-`; callers pass absolute paths.
pub fn shell_quote(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "/._-+:,@%".contains(c);
    if !s.is_empty() && s.chars().all(safe) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r#"'\''"#))
}

/// Quote `s` as one PowerShell single-quoted word. Inside single quotes nothing expands; PowerShell
/// also reads the smart quotes U+2018 to U+201B as `'`, so each of them is doubled too.
pub fn powershell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}'..='\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}
