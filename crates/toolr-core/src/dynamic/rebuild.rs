//! High-level rebuild orchestration for both static-plus-dynamic and
//! dynamic-only refresh paths.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::hash::compute_third_party_hash;
use super::merge::merge_dynamic;
use super::runner::run_introspect;
use crate::manifest::{load_manifest, write_manifest};
use crate::parser::build_static_manifest;

/// Result of a rebuild, returned for diagnostics / CLI output.
#[derive(Debug)]
pub struct RebuildOutcome {
    pub manifest_path: PathBuf,
    pub group_count: usize,
    pub command_count: usize,
    pub warnings: Vec<String>,
}

/// Full rebuild: static layer + dynamic layer + write.
///
/// `python` is the absolute path to the tools-venv Python interpreter
/// (resolved by Plan 3's `toolr_core::venv::resolve_venv_path`).
/// `venv_root` is the venv directory used by [`compute_third_party_hash`].
pub fn rebuild_manifest_full(
    project_root: &Path,
    python: &Path,
    venv_root: &Path,
) -> Result<RebuildOutcome> {
    let tools = project_root.join("tools");
    let base = build_static_manifest(&tools).with_context(|| "building static manifest layer")?;
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

/// Dynamic-only refresh: reuse the on-disk manifest's static layer,
/// strip its dynamic entries, run the helper, re-merge, and write.
///
/// Cheap relative to a full rebuild — used at execute time when only the
/// venv has changed.
pub fn rebuild_dynamic_only(
    project_root: &Path,
    python: &Path,
    venv_root: &Path,
) -> Result<RebuildOutcome> {
    use crate::manifest::Origin;

    let tools = project_root.join("tools");
    let manifest_path = tools.join(".toolr-manifest.json");
    let mut base = load_manifest(&manifest_path)
        .with_context(|| format!("loading {}", manifest_path.display()))?;
    // Drop everything dynamic; keep the static and third-party skeleton intact.
    base.groups
        .retain(|g| matches!(g.origin, Origin::Static | Origin::ThirdParty));
    base.commands
        .retain(|c| matches!(c.origin, Origin::Static | Origin::ThirdParty));

    let payload =
        run_introspect(python, &tools).with_context(|| "running dynamic-layer introspect helper")?;
    let warnings = payload.warnings.clone();
    let mut merged = merge_dynamic(base, payload);
    merged.third_party_hash = compute_third_party_hash(venv_root)?;
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
