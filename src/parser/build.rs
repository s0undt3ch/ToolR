//! Build a complete static `Manifest` from a `tools/` directory.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

use crate::hash::hash_tools_dir;
use crate::manifest::{Manifest, SCHEMA_VERSION};
use crate::parser::types::{TypeImports, TypeResolutionError};
use crate::parser::{
    commands::extract_commands,
    groups::extract_groups,
    parse_python_file,
    symbols::{EnumTable, TypeAliasTable},
};
use crate::third_party::{ThirdPartyError, discover_and_merge};

/// Build the static portion of a manifest from a tools directory.
///
/// Surfaces every unsupported-type rejection in a single batch via
/// [`BuildError::UnsupportedTypes`] so users see all the offending
/// annotations at once rather than one-at-a-time on each rebuild.
pub fn build_static_manifest(tools_dir: &Path) -> Result<Manifest> {
    match build_static_manifest_inner(tools_dir) {
        Ok(m) => Ok(m),
        Err(BuildError::Build(e)) => Err(e),
        Err(other) => Err(anyhow::anyhow!("{other}")),
    }
}

fn build_static_manifest_inner(tools_dir: &Path) -> std::result::Result<Manifest, BuildError> {
    let py_files = list_python_files(tools_dir);

    // Pass 1: build cross-file enum + type-alias tables from every module.
    let mut enums = EnumTable::default();
    let mut aliases = TypeAliasTable::default();
    for path in &py_files {
        let module = parse_python_file(path).map_err(BuildError::Build)?;
        enums.merge(EnumTable::from_module(&module));
        aliases.merge(TypeAliasTable::from_module(&module));
    }

    // Pass 2: extract groups + commands per file using the merged tables.
    //
    // We also keep a cumulative `var_name → group_full_path` map across
    // files so cross-file imports work: `tools/ci/_common.py` declares
    // `group = command_group("ci")`, then `tools/ci/gh_actions.py` does
    // `from ._common import group; @group.command` — the static parser
    // doesn't follow the import, but the global map lets the second
    // file's decorators find `group`. Files are walked in sorted order
    // (alphabetical) which matches the conventional `_common.py` →
    // `gh_actions.py` etc. layout where the parent group lives in an
    // underscore-prefixed module.
    let mut all_groups = Vec::new();
    let mut all_commands = Vec::new();
    let mut seen_groups: HashSet<String> = HashSet::new();
    let mut global_vars: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut type_errors: Vec<TypeResolutionError> = Vec::new();
    for path in &py_files {
        let module = parse_python_file(path).map_err(BuildError::Build)?;
        let module_path = module_path_for(tools_dir, path);
        let module_doc = module_docstring(&module);
        let bindings = extract_groups(&module, &module_doc, &global_vars);
        let type_imports = TypeImports::from_module(&module);
        let commands = extract_commands(
            &module,
            &module_path,
            &bindings,
            &enums,
            &type_imports,
            &aliases,
            &global_vars,
            &mut type_errors,
        );
        // Make this file's bindings visible to subsequent files.
        for binding in &bindings {
            global_vars.insert(binding.var.clone(), binding.group.full_path());
        }
        // Dedup groups by *full_path* (not just leaf name), so nested
        // groups in different branches with the same leaf name (e.g.
        // `ci.image` + `docker.image`) both survive.
        for binding in bindings {
            if seen_groups.insert(binding.group.full_path()) {
                all_groups.push(binding.group);
            }
        }
        all_commands.extend(commands);
    }

    if !type_errors.is_empty() {
        return Err(BuildError::UnsupportedTypes(type_errors));
    }

    let static_hash = hash_tools_dir(tools_dir).map_err(BuildError::Build)?;
    Ok(Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash,
        dynamic_hash: String::new(),
        groups: all_groups,
        commands: all_commands,
    })
}

/// Like `build_static_manifest`, but also globs `tools_venv` for
/// third-party manifest fragments and merges them in.
pub fn build_static_manifest_with_venv(
    tools_dir: &Path,
    tools_venv: &Path,
) -> Result<Manifest, BuildError> {
    let base = build_static_manifest_inner(tools_dir)?;
    discover_and_merge(tools_venv, base).map_err(BuildError::ThirdParty)
}

/// Error type covering both the local build and the third-party merge.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("static build error: {0}")]
    Build(#[source] anyhow::Error),
    #[error("third-party merge error: {0}")]
    ThirdParty(#[from] ThirdPartyError),
    #[error("unsupported parameter types ({count}):\n{details}", count = .0.len(), details = format_type_errors(.0))]
    UnsupportedTypes(Vec<TypeResolutionError>),
}

fn format_type_errors(errors: &[TypeResolutionError]) -> String {
    let mut s = String::new();
    for (i, err) in errors.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        use std::fmt::Write as _;
        let _ = write!(&mut s, "  - {err}");
    }
    s
}

fn list_python_files(tools_dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = WalkDir::new(tools_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"))
        .map(|e| e.into_path())
        .collect();
    paths.sort();
    paths
}

fn module_path_for(tools_dir: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(tools_dir).unwrap_or(file);
    let mut parts: Vec<String> = rel
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.last().map(String::as_str) == Some("__init__") {
        parts.pop();
    }
    let mut out = String::from("tools");
    for p in parts {
        out.push('.');
        out.push_str(&p);
    }
    out
}

fn module_docstring(module: &ruff_python_ast::ModModule) -> String {
    use ruff_python_ast::Stmt;
    let Some(Stmt::Expr(e)) = module.body.first() else {
        return String::new();
    };
    if let ruff_python_ast::Expr::StringLiteral(s) = e.value.as_ref() {
        return s.value.to_str().to_string();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(tmp: &Path, name: &str, contents: &str) {
        let path = tmp.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    use crate::third_party::{
        FRAGMENT_SCHEMA_VERSION, FragmentCommand, FragmentGroup, ManifestFragment,
    };

    #[test]
    fn build_with_venv_merges_local_and_third_party() {
        let tmp = TempDir::new().unwrap();
        // Local tools/ side.
        write(
            tmp.path(),
            "tools/ci.py",
            r#""""CI utilities."""
group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def hello(ctx):
    """Say hello."""
    pass
"#,
        );
        // Fake tools venv with a third-party fragment.
        let venv = tmp.path().join("venv");
        let site = venv.join("lib").join("python3.13").join("site-packages");
        std::fs::create_dir_all(site.join("ext_pkg")).unwrap();
        let frag = ManifestFragment {
            toolr_schema_version: FRAGMENT_SCHEMA_VERSION,
            package: "ext_pkg".into(),
            groups: vec![FragmentGroup {
                name: "deploy".into(),
                title: "Deploy".into(),
                description: String::new(),
            }],
            commands: vec![FragmentCommand {
                name: "rollout".into(),
                group: "deploy".into(),
                module: "ext_pkg.commands".into(),
                function: "rollout".into(),
                summary: String::new(),
                description: String::new(),
                arguments: vec![],
                imports: vec![],
            }],
        };
        std::fs::write(
            site.join("ext_pkg").join("toolr-manifest.json"),
            serde_json::to_string(&frag).unwrap(),
        )
        .unwrap();

        let m = build_static_manifest_with_venv(&tmp.path().join("tools"), &venv).unwrap();
        let groups: Vec<_> = m.groups.iter().map(|g| g.name.as_str()).collect();
        assert!(groups.contains(&"ci"));
        assert!(groups.contains(&"deploy"));
        let cmds: Vec<_> = m.commands.iter().map(|c| c.name.as_str()).collect();
        assert!(cmds.contains(&"hello"));
        assert!(cmds.contains(&"rollout"));
    }

    #[test]
    fn builds_manifest_from_single_tools_file() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/ci.py",
            r#""""CI utilities."""
group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def hello(ctx):
    """Say hello."""
    pass
"#,
        );
        let m = build_static_manifest(&tmp.path().join("tools")).unwrap();
        assert_eq!(m.schema_version, SCHEMA_VERSION);
        assert_eq!(m.groups.len(), 1);
        assert_eq!(m.groups[0].name, "ci");
        assert_eq!(m.commands.len(), 1);
        assert_eq!(m.commands[0].name, "hello");
        assert!(!m.static_hash.is_empty());
    }
}
