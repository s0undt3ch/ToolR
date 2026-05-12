//! Decision logic + executor for installing a toolr-managed uv.

use std::io::{IsTerminal, Write};

use super::UvError;

/// What the discovery + consent flow tells the caller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallDecision {
    /// uv was found and is usable as-is. No install required.
    AlreadyAvailable,
    /// uv is not available and the caller wants us to install it.
    Install,
    /// uv is not available and the user declined (or stdin is not a TTY
    /// and no auto-yes was set). Caller should surface a friendly error.
    Refuse,
}

/// How the toolr binary was invoked, for non-interactive decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConsentMode {
    /// `--yes` was passed.
    pub yes_flag: bool,
    /// `TOOLR_AUTO_INSTALL_UV=1` is set in the environment.
    pub auto_install_env: bool,
}

impl ConsentMode {
    pub fn from_env() -> Self {
        Self {
            yes_flag: false,
            auto_install_env: std::env::var_os("TOOLR_AUTO_INSTALL_UV")
                .is_some_and(|v| v == "1"),
        }
    }
}

/// Pure decision logic. `path_found` is whether `find_uv_on_path` returned
/// `Some`. `managed_found` is whether `find_managed_uv` returned `Some`.
/// `stdin_tty` controls whether to prompt; in tests / pipes we never
/// prompt and rely on `consent` instead.
pub fn decide_install(
    path_found: bool,
    managed_found: bool,
    consent: ConsentMode,
    stdin_tty: bool,
) -> InstallDecision {
    if path_found || managed_found {
        return InstallDecision::AlreadyAvailable;
    }
    if consent.yes_flag || consent.auto_install_env {
        return InstallDecision::Install;
    }
    if !stdin_tty {
        return InstallDecision::Refuse;
    }
    match prompt_for_consent() {
        Ok(true) => InstallDecision::Install,
        _ => InstallDecision::Refuse,
    }
}

/// Print the prompt and return `true` for "install".
fn prompt_for_consent() -> Result<bool, UvError> {
    let mut stderr = std::io::stderr();
    writeln!(
        stderr,
        "toolr needs uv (https://docs.astral.sh/uv/) and didn't find it on PATH."
    )?;
    let managed = super::managed_uv_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "$XDG_DATA_HOME/toolr/bin/uv".to_string());
    writeln!(stderr, "  [I] Install it for me at {managed}")?;
    writeln!(
        stderr,
        "  [M] I'll install it manually (see https://docs.astral.sh/uv/getting-started/installation/)"
    )?;
    write!(stderr, "Choice [I/M] ")?;
    stderr.flush()?;
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;
    Ok(matches!(buf.trim(), "I" | "i" | ""))
}

/// Convenience that combines a TTY check + decision.
pub fn decide_install_auto(
    path_found: bool,
    managed_found: bool,
    consent: ConsentMode,
) -> InstallDecision {
    decide_install(
        path_found,
        managed_found,
        consent,
        std::io::stdin().is_terminal(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_available_when_path_present() {
        assert_eq!(
            decide_install(true, false, ConsentMode::default(), false),
            InstallDecision::AlreadyAvailable
        );
    }

    #[test]
    fn already_available_when_managed_present() {
        assert_eq!(
            decide_install(false, true, ConsentMode::default(), false),
            InstallDecision::AlreadyAvailable
        );
    }

    #[test]
    fn yes_flag_forces_install_even_without_tty() {
        let consent = ConsentMode { yes_flag: true, ..Default::default() };
        assert_eq!(
            decide_install(false, false, consent, false),
            InstallDecision::Install
        );
    }

    #[test]
    fn auto_install_env_forces_install_even_without_tty() {
        let consent = ConsentMode { auto_install_env: true, ..Default::default() };
        assert_eq!(
            decide_install(false, false, consent, false),
            InstallDecision::Install
        );
    }

    #[test]
    fn refuses_non_interactive_without_consent() {
        assert_eq!(
            decide_install(false, false, ConsentMode::default(), false),
            InstallDecision::Refuse
        );
    }
}
