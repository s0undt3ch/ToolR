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

    #[test]
    fn whitespace_only_spec_is_skipped() {
        let uv = UvBinary {
            path: std::path::PathBuf::from("/nonexistent/uv"),
            version: (0, 0, 0),
            source: crate::uv::UvSource::Path,
        };
        let cfg = ToolrConfig {
            editable_install: vec!["   ".into(), "\t\n".into(), "".into()],
            ..Default::default()
        };
        let outcomes = perform_editable_installs(
            &uv,
            &cfg,
            Path::new("/tmp"),
            Path::new("/tmp/python"),
        );
        assert_eq!(outcomes.len(), 3);
        for o in &outcomes {
            assert!(
                matches!(o, EditableOutcome::Skipped(_)),
                "expected Skipped, got {o:?}",
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_exit_from_stub_uv_produces_failure_with_status_message() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let stub = tmp.path().join("uv");
        let mut f = std::fs::File::create(&stub).unwrap();
        // Stub uv exits 7 — exercises the `Ok(status) if !success()` arm.
        writeln!(f, "#!/bin/sh\nexit 7").unwrap();
        drop(f);
        let mut perms = std::fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub, perms).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 0, 0),
            source: crate::uv::UvSource::Path,
        };
        let cfg = ToolrConfig {
            editable_install: vec!["some-package".into()],
            ..Default::default()
        };
        let outcomes = perform_editable_installs(
            &uv,
            &cfg,
            Path::new("/tmp"),
            Path::new("/tmp/python"),
        );
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            EditableOutcome::Failed { spec, message } => {
                assert_eq!(spec, "some-package");
                assert!(message.contains("uv pip install exited"), "message={message}");
            }
            other => panic!("expected Failed outcome, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn successful_stub_uv_invocation_records_installed_outcome() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let stub = tmp.path().join("uv");
        let mut f = std::fs::File::create(&stub).unwrap();
        writeln!(f, "#!/bin/sh\nexit 0").unwrap();
        drop(f);
        let mut perms = std::fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub, perms).unwrap();
        let uv = UvBinary {
            path: stub,
            version: (0, 0, 0),
            source: crate::uv::UvSource::Path,
        };
        let cfg = ToolrConfig {
            // Use the "." shorthand so the repo_root resolution branch runs.
            editable_install: vec![".".into(), "some-package".into()],
            ..Default::default()
        };
        let outcomes = perform_editable_installs(
            &uv,
            &cfg,
            Path::new("/my/repo"),
            Path::new("/tmp/python"),
        );
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0], EditableOutcome::Installed(".".into()));
        assert_eq!(outcomes[1], EditableOutcome::Installed("some-package".into()));
    }

    #[test]
    fn warn_failures_is_noop_for_non_failures() {
        // Pure smoke test for the no-failure branch — `warn_failures` reads
        // each outcome and short-circuits unless it's `Failed`. Asserting
        // the function returns at all is enough; stderr capture in unit
        // tests is fiddly and adds no signal beyond "did it iterate".
        let outcomes = vec![
            EditableOutcome::Installed("pkg-a".into()),
            EditableOutcome::Skipped("".into()),
        ];
        warn_failures(&outcomes);
    }

    #[test]
    fn warn_failures_iterates_over_failures() {
        // Drives the `if let EditableOutcome::Failed { ... }` arm at least
        // once so the `eprintln!` line is exercised. The output goes to
        // stderr; verifying contents isn't worth the std::io plumbing.
        let outcomes = vec![EditableOutcome::Failed {
            spec: "broken-pkg".into(),
            message: "stub failure".into(),
        }];
        warn_failures(&outcomes);
    }
}
