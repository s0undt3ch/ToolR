//! High-level rebuild orchestration. Exposes [`rebuild_manifest_full`]
//! for bootstrap and `project manifest rebuild`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::hash::compute_third_party_hash;
use super::merge::merge_dynamic;
use super::runner::run_introspect;
use crate::manifest::write_manifest;
use crate::parser::build_static_manifest_with_venv;

/// Result of a rebuild, returned for diagnostics / CLI output.
#[derive(Debug)]
pub struct RebuildOutcome {
    pub manifest_path: PathBuf,
    pub group_count: usize,
    pub command_count: usize,
    pub warnings: Vec<String>,
}

/// Full rebuild: static layer (including third-party fragments globbed
/// from the venv) + dynamic layer + write.
///
/// `python` is the absolute path to the tools-venv Python interpreter
/// (resolved by Plan 3's `toolr_core::venv::resolve_venv_path`).
/// `venv_root` is the venv directory: both globbed for
/// `site-packages/*/toolr-manifest.json` (via
/// `build_static_manifest_with_venv`) and hashed via
/// [`compute_third_party_hash`].
pub fn rebuild_manifest_full(
    project_root: &Path,
    python: &Path,
    venv_root: &Path,
) -> Result<RebuildOutcome> {
    let tools = project_root.join("tools");
    let base = build_static_manifest_with_venv(&tools, venv_root)
        .with_context(|| "building static manifest layer (incl. third-party glob-merge)")?;
    let payload =
        run_introspect(python, &tools).with_context(|| "running dynamic-layer introspect helper")?;
    let warnings = payload.warnings.clone();
    let mut merged = merge_dynamic(base, payload);
    merged.third_party_hash = compute_third_party_hash(venv_root)?;
    let manifest_path = tools.join(".toolr-manifest.json");
    write_manifest(&manifest_path, &merged)?;
    Ok(RebuildOutcome {
        manifest_path,
        group_count: merged.groups.len(),
        command_count: merged.commands.len(),
        warnings,
    })
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn fake_python_emitting(tmp: &Path, body: &str) -> PathBuf {
        let path = tmp.join("python");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "cat <<'__EOF__'").unwrap();
        writeln!(f, "{body}").unwrap();
        writeln!(f, "__EOF__").unwrap();
        drop(f);
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn full_rebuild_writes_combined_manifest() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        let tools = project.join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::write(
            tools.join("ci.py"),
            "\"\"\"CI utilities.\"\"\"\ngroup = command_group(\"ci\", \"CI utilities\")\n@group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n    pass\n",
        )
        .unwrap();
        let venv = project.join("venv");
        std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/foo-1.0.0.dist-info"))
            .unwrap();
        let py = fake_python_emitting(
            project,
            r#"{"payload_schema_version":1,"groups":[{"name":"legacy","title":"Legacy","description":"","origin":"static"}],"commands":[{"name":"widget","group":"legacy","module":"third","function":"widget","summary":"","description":"","arguments":[],"imports":[],"origin":"static"}],"warnings":[]}"#,
        );
        let outcome = rebuild_manifest_full(project, &py, &venv).unwrap();
        assert!(outcome.manifest_path.is_file());
        let m = crate::manifest::load_manifest(&outcome.manifest_path).unwrap();
        let group_names: Vec<_> = m.groups.iter().map(|g| g.name.as_str()).collect();
        assert!(group_names.contains(&"ci"));
        assert!(group_names.contains(&"legacy"));
        assert!(!m.third_party_hash.is_empty());
    }
}
