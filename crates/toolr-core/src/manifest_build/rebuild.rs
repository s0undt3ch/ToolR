//! High-level rebuild orchestration. Exposes [`rebuild_manifest_full`]
//! for bootstrap and `project manifest rebuild`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::hash::compute_third_party_hash;
use crate::hash::context_unless_symlinked;
use crate::manifest::{acquire_rebuild_lock, write_manifest};
use crate::parser::{BuildError, build_static_manifest_with_venv};

/// Result of a rebuild, returned for diagnostics / CLI output.
#[derive(Debug)]
pub struct RebuildOutcome {
    pub manifest_path: PathBuf,
    pub group_count: usize,
    pub command_count: usize,
    pub warnings: Vec<String>,
}

/// Full rebuild: the static manifest (first-party AST parse plus the
/// third-party fragments found in the venv), stamped with the
/// third-party hash, then written. Executes nothing — `venv_root` is
/// only searched for `site-packages/*/toolr-manifest.json` (via
/// `build_static_manifest_with_venv`) and hashed via
/// [`compute_third_party_hash`].
pub fn rebuild_manifest_full(project_root: &Path, venv_root: &Path) -> Result<RebuildOutcome> {
    let tools = project_root.join("tools");
    let manifest_path = tools.join(".toolr-manifest.json");
    // An explicit rebuild always runs; the lock only serialises it with
    // concurrent auto-rebuilds.
    let _lock = acquire_rebuild_lock(&manifest_path);
    let manifest = crate::hash::build_to_persist(&tools, || {
        let mut manifest = build_static_manifest_with_venv(&tools, venv_root)
            .map_err(BuildError::into_anyhow)
            .map_err(|e| {
                context_unless_symlinked(
                    e,
                    "building static manifest (incl. third-party fragments)",
                )
            })?;
        manifest.third_party_hash = compute_third_party_hash(venv_root)?;
        anyhow::Ok(manifest)
    })?;
    write_manifest(&manifest_path, &manifest)?;
    Ok(RebuildOutcome {
        manifest_path,
        group_count: manifest.groups.len(),
        command_count: manifest.commands.len(),
        warnings: manifest
            .build_warnings
            .iter()
            .chain(manifest.plugin_warnings.iter().map(|w| &w.message))
            .map(|w| crate::text::strip_control_chars(w))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::EnvGuard;
    use tempfile::TempDir;

    #[test]
    fn full_rebuild_writes_static_manifest() {
        let (_env, _cache_home) = EnvGuard::temp_cache_home();
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        let tools = project.join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::write(
            tools.join("ci.py"),
            "\"\"\"CI utilities.\"\"\"\ngroup = command_group(\"ci\", \"CI utilities\", \"CI utilities.\")\n@group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n    pass\n",
        )
        .unwrap();
        let venv = project.join("venv");
        std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/foo-1.0.0.dist-info"))
            .unwrap();
        let outcome = rebuild_manifest_full(project, &venv).unwrap();
        assert!(outcome.manifest_path.is_file());
        let m = crate::manifest::load_manifest(&outcome.manifest_path).unwrap();
        let group_names: Vec<_> = m.groups.iter().map(|g| g.name.as_str()).collect();
        assert!(group_names.contains(&"ci"));
        assert!(!m.third_party_hash.is_empty());
    }

    #[test]
    fn full_rebuild_reports_plugin_warnings() {
        let (_env, _cache_home) = EnvGuard::temp_cache_home();
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::create_dir(project.join("tools")).unwrap();
        let venv = project.join("venv");
        let pkg = venv.join("lib/python3.13/site-packages/demo");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("toolr-manifest.json"),
            r#"{"toolr_schema_version": 1, "package": "demo"}"#,
        )
        .unwrap();
        let outcome = rebuild_manifest_full(project, &venv).unwrap();
        assert_eq!(
            outcome.warnings,
            [
                "skipping plugin demo: built with toolr schema 1, this toolr needs >= 2. \
              Rebuild the plugin with toolr >= 0.34.0."
            ]
        );
    }

    /// `project manifest rebuild` reports the local build's warnings too.
    #[test]
    fn full_rebuild_reports_build_warnings() {
        let (_env, _cache_home) = EnvGuard::temp_cache_home();
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        let tools = project.join("tools");
        std::fs::create_dir(&tools).unwrap();
        for (module, title) in [("m1", "Grp"), ("m2", "Other")] {
            std::fs::write(
                tools.join(format!("{module}.py")),
                format!("g = command_group(\"grp\", \"{title}\", \"Grp\")\n"),
            )
            .unwrap();
        }
        let venv = project.join("venv");
        std::fs::create_dir(&venv).unwrap();
        let outcome = rebuild_manifest_full(project, &venv).unwrap();
        assert_eq!(
            outcome.warnings,
            [
                "group `grp` is declared more than once; keeping the title from tools.m1 and \
              ignoring the different title in tools.m2"
            ]
        );
    }
}
