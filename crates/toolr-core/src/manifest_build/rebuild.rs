//! High-level rebuild orchestration. Exposes [`rebuild_manifest_full`]
//! for bootstrap and `project manifest rebuild`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::hash::compute_third_party_hash;
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

/// Full rebuild: the static manifest (first-party AST parse plus the
/// third-party fragments globbed from the venv), stamped with the
/// third-party hash, then written. Executes nothing — `venv_root` is
/// only globbed for `site-packages/*/toolr-manifest.json` (via
/// `build_static_manifest_with_venv`) and hashed via
/// [`compute_third_party_hash`].
pub fn rebuild_manifest_full(project_root: &Path, venv_root: &Path) -> Result<RebuildOutcome> {
    let tools = project_root.join("tools");
    let mut manifest = build_static_manifest_with_venv(&tools, venv_root)
        .with_context(|| "building static manifest (incl. third-party glob-merge)")?;
    manifest.third_party_hash = compute_third_party_hash(venv_root)?;
    let manifest_path = tools.join(".toolr-manifest.json");
    write_manifest(&manifest_path, &manifest)?;
    Ok(RebuildOutcome {
        manifest_path,
        group_count: manifest.groups.len(),
        command_count: manifest.commands.len(),
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn full_rebuild_writes_static_manifest() {
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
        let outcome = rebuild_manifest_full(project, &venv).unwrap();
        assert!(outcome.manifest_path.is_file());
        let m = crate::manifest::load_manifest(&outcome.manifest_path).unwrap();
        let group_names: Vec<_> = m.groups.iter().map(|g| g.name.as_str()).collect();
        assert!(group_names.contains(&"ci"));
        assert!(!m.third_party_hash.is_empty());
    }
}
