//! Best-effort post-sync editable installs.

use std::path::Path;
use std::process::Command;

use crate::uv::UvBinary;

use super::config::ToolrConfig;

/// Outcome of one editable-install attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditableOutcome {
    Installed(String),
    Skipped(String),
    Failed { spec: String, message: String },
}

/// Run `uv pip install -e <spec>` for each configured entry. The
/// `repo_root` resolves the `"."` shorthand to an absolute path so
/// the venv doesn't depend on cwd.
pub fn perform_editable_installs(
    uv: &UvBinary,
    config: &ToolrConfig,
    repo_root: &Path,
    venv_python: &Path,
) -> Vec<EditableOutcome> {
    let mut out = Vec::with_capacity(config.editable_install.len());
    for spec in &config.editable_install {
        if spec.trim().is_empty() {
            out.push(EditableOutcome::Skipped(spec.clone()));
            continue;
        }
        let resolved = if spec == "." || spec == "./" {
            repo_root.display().to_string()
        } else {
            spec.clone()
        };
        let result = Command::new(&uv.path)
            .arg("pip")
            .arg("install")
            .arg("--python")
            .arg(venv_python)
            .arg("-e")
            .arg(&resolved)
            .status();
        match result {
            Ok(status) if status.success() => out.push(EditableOutcome::Installed(spec.clone())),
            Ok(status) => out.push(EditableOutcome::Failed {
                spec: spec.clone(),
                message: format!("uv pip install exited with {status:?}"),
            }),
            Err(e) => out.push(EditableOutcome::Failed {
                spec: spec.clone(),
                message: e.to_string(),
            }),
        }
    }
    out
}

/// Emit a stderr line per failed install. Toolr does not abort on
/// failure — tools that need the repo will surface a normal ImportError
/// at execute time.
pub fn warn_failures(outcomes: &[EditableOutcome]) {
    for outcome in outcomes {
        if let EditableOutcome::Failed { spec, message } = outcome {
            eprintln!("toolr: warning: editable install of `{spec}` failed: {message}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_produces_no_outcomes() {
        let uv = UvBinary {
            path: std::path::PathBuf::from("/nonexistent/uv"),
            version: (0, 0, 0),
            source: crate::uv::UvSource::Path,
        };
        let cfg = ToolrConfig::default();
        let outcomes = perform_editable_installs(
            &uv,
            &cfg,
            Path::new("/tmp"),
            Path::new("/tmp/python"),
        );
        assert!(outcomes.is_empty());
    }

    #[test]
    fn missing_uv_binary_produces_failure_outcome_not_panic() {
        let uv = UvBinary {
            path: std::path::PathBuf::from("/nonexistent/uv-toolr-test"),
            version: (0, 0, 0),
            source: crate::uv::UvSource::Path,
        };
        let cfg = ToolrConfig {
            editable_install: vec![".".into()],
            ..Default::default()
        };
        let outcomes = perform_editable_installs(
            &uv,
            &cfg,
            Path::new("/tmp"),
            Path::new("/tmp/python"),
        );
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0], EditableOutcome::Failed { .. }));
    }
}
