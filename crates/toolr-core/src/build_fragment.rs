//! Build a third-party `ManifestFragment` from a plugin's source tree.
//!
//! Pure-Rust replacement for the legacy Python `toolr.build` module.
//! Walks the package's `.py` files via the same AST pipeline used by
//! `build_static_manifest`, applies a plugin-aware module-path prefix,
//! and filters out anything that doesn't belong to the target package.

use std::path::{Path, PathBuf};

use crate::parser::{list_python_files, module_path_for_prefix, parse_python_file};
use crate::parser::{
    commands::extract_commands,
    groups::extract_groups,
    symbols::{ArgSectionTable, EnumTable, TypeAliasTable},
    types::{SourcesImports, TypeImports, TypeResolutionError},
};
use crate::third_party::{
    FragmentArgument, FragmentCommand, FragmentGroup, ManifestFragment,
};

/// Error type for `build_third_party_fragment`.
#[derive(Debug, thiserror::Error)]
pub enum BuildFragmentError {
    #[error("source directory `{path}` is a namespace package (no __init__.py); namespace packages are not supported")]
    NamespacePackage { path: PathBuf },
    #[error("source directory `{path}` does not exist or is not a directory")]
    MissingSourceDir { path: PathBuf },
    #[error("package `{package}` declares no toolr commands - nothing to write")]
    EmptyPackage { package: String },
    #[error("parse error in {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
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

/// Build a `ManifestFragment` for `package_name` by AST-walking
/// `source_dir`. See spec `specs/2026-05-22-rust-build-manifest-design.md`.
pub fn build_third_party_fragment(
    source_dir: &Path,
    package_name: &str,
    schema_version: u32,
) -> Result<ManifestFragment, BuildFragmentError> {
    // Reject missing dirs and namespace packages up front.
    if !source_dir.is_dir() {
        return Err(BuildFragmentError::MissingSourceDir {
            path: source_dir.to_path_buf(),
        });
    }
    if !source_dir.join("__init__.py").is_file() {
        return Err(BuildFragmentError::NamespacePackage {
            path: source_dir.to_path_buf(),
        });
    }

    // Pass 1: cross-file enum / alias / arg-section tables.
    let py_files = list_python_files(source_dir);
    let mut enums = EnumTable::default();
    let mut aliases = TypeAliasTable::default();
    let mut sections = ArgSectionTable::default();
    for path in &py_files {
        let module = parse_python_file(path).map_err(|e| BuildFragmentError::Parse {
            path: path.clone(),
            source: e,
        })?;
        enums.merge(EnumTable::from_module(&module));
        aliases.merge(TypeAliasTable::from_module(&module));
        sections.merge(ArgSectionTable::from_module(&module));
    }

    // Pass 2: groups + commands.
    let mut all_groups: Vec<crate::manifest::Group> = Vec::new();
    let mut all_commands: Vec<crate::manifest::Command> = Vec::new();
    let mut seen_groups: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut global_vars: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut type_errors: Vec<TypeResolutionError> = Vec::new();

    for path in &py_files {
        let module = parse_python_file(path).map_err(|e| BuildFragmentError::Parse {
            path: path.clone(),
            source: e,
        })?;
        let module_path = module_path_for_prefix(source_dir, path, package_name);
        let module_doc = module_docstring(&module);
        let bindings = extract_groups(&module, &module_doc, &global_vars);
        let type_imports = TypeImports::from_module(&module);
        let sources_imports = SourcesImports::from_module(&module);
        let commands = extract_commands(
            &module,
            &module_path,
            &bindings,
            &enums,
            &type_imports,
            &sources_imports,
            &aliases,
            &sections,
            &global_vars,
            &mut type_errors,
        );
        for binding in &bindings {
            global_vars.insert(binding.var.clone(), binding.group.full_path());
        }
        for binding in bindings {
            if seen_groups.insert(binding.group.full_path()) {
                all_groups.push(binding.group);
            }
        }
        all_commands.extend(commands);
    }

    if !type_errors.is_empty() {
        return Err(BuildFragmentError::UnsupportedTypes(type_errors));
    }

    // Filter: only keep commands whose `module` belongs to package_name.
    let prefix_dot = format!("{package_name}.");
    all_commands.retain(|c| c.module == package_name || c.module.starts_with(&prefix_dot));

    // Derive surviving groups from surviving commands.
    let surviving_group_names: std::collections::HashSet<&str> =
        all_commands.iter().map(|c| c.group.as_str()).collect();
    all_groups.retain(|g| surviving_group_names.contains(g.full_path().as_str()));

    if all_groups.is_empty() && all_commands.is_empty() {
        return Err(BuildFragmentError::EmptyPackage {
            package: package_name.to_string(),
        });
    }

    // Sort: groups by name, commands by (group, name).
    all_groups.sort_by_key(|g| g.full_path());
    all_commands.sort_by(|a, b| (a.group.as_str(), a.name.as_str()).cmp(&(b.group.as_str(), b.name.as_str())));

    Ok(ManifestFragment {
        toolr_schema_version: schema_version,
        package: package_name.to_string(),
        groups: all_groups
            .into_iter()
            .map(|g| FragmentGroup {
                name: g.full_path(),
                title: g.title,
                description: g.description,
            })
            .collect(),
        commands: all_commands
            .into_iter()
            .map(|c| FragmentCommand {
                name: c.name,
                group: c.group,
                module: c.module,
                function: c.function,
                summary: c.summary,
                description: c.description,
                arguments: c
                    .arguments
                    .into_iter()
                    .map(|a| FragmentArgument {
                        name: a.name,
                        kind: a.kind,
                        help: a.help,
                        default: a.default,
                        type_annotation: a.type_annotation,
                        allowed_values: a.allowed_values,
                    })
                    .collect(),
                imports: c.imports,
            })
            .collect(),
    })
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

    #[test]
    fn rejects_namespace_package_without_init() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("pkg")).unwrap();
        // No __init__.py.
        let err =
            build_third_party_fragment(&tmp.path().join("pkg"), "pkg", 1).unwrap_err();
        assert!(matches!(err, BuildFragmentError::NamespacePackage { .. }));
    }

    #[test]
    fn builds_single_command_plugin_fragment() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("mypkg");
        write(&pkg, "__init__.py", "");
        write(
            &pkg,
            "commands.py",
            r#""""Plugin commands."""
from toolr import Context
from toolr import command_group

third_party_group = command_group(
    "third-party",
    "Third Party Tools",
    "Tools contributed by a third-party plugin.",
)

@third_party_group.command("hello")
def hello_command(ctx: Context, name: str = "World") -> None:
    """Say hello to someone.

    Args:
        ctx: The execution context.
        name: Name to greet (default: World).
    """
    ctx.print(f"Hello, {name}")
"#,
        );

        let fragment = build_third_party_fragment(&pkg, "mypkg", 1).unwrap();
        assert_eq!(fragment.toolr_schema_version, 1);
        assert_eq!(fragment.package, "mypkg");
        assert_eq!(fragment.groups.len(), 1);
        assert_eq!(fragment.groups[0].name, "third-party");
        assert_eq!(fragment.groups[0].title, "Third Party Tools");
        assert_eq!(
            fragment.groups[0].description,
            "Tools contributed by a third-party plugin."
        );
        assert_eq!(fragment.commands.len(), 1);
        let cmd = &fragment.commands[0];
        assert_eq!(cmd.name, "hello");
        assert_eq!(cmd.group, "third-party");
        assert_eq!(cmd.module, "mypkg.commands");
        assert_eq!(cmd.function, "hello_command");
        assert_eq!(cmd.summary, "Say hello to someone.");
        assert_eq!(cmd.arguments.len(), 1);
        let arg = &cmd.arguments[0];
        assert_eq!(arg.name, "name");
        assert_eq!(arg.type_annotation.as_deref(), Some("str"));
        assert_eq!(arg.default.as_deref(), Some("World"));
        assert_eq!(arg.help, "Name to greet (default: World).");
    }

    /// Mirror the canonical `examples/plugin-package/.../commands.py` and
    /// assert the generated fragment equals the committed JSON byte-for-byte.
    ///
    /// We compare ManifestFragment values (not raw bytes) because byte
    /// equality is asserted by the CLI golden test in Task 8. Here we
    /// only need the *value* to match — serialisation ordering is a
    /// separate concern (Task 6).
    #[test]
    fn matches_committed_example_plugin_fragment() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp.path().join("toolr_example_plugin");
        write(&pkg, "__init__.py", "");
        write(
            &pkg,
            "commands.py",
            r#""""Command groups exposed by the example plugin.

The decorators here populate the toolr command registry at import time.
For static discovery (the canonical path), the same groups are also
serialised into ``toolr-manifest.json`` shipped alongside this module.
"""

from __future__ import annotations

from toolr import Context
from toolr import command_group

third_party_group = command_group(
    "third-party",
    "Third Party Tools",
    "Tools contributed by a third-party plugin.",
)


@third_party_group.command("hello")
def hello_command(ctx: Context, name: str = "World") -> None:
    """Say hello to someone.

    Args:
        ctx: The execution context.
        name: Name to greet (default: World).
    """
    ctx.print(f"Hello, {name} from toolr-plugin-example!")


@third_party_group.command("version")
def version_command(ctx: Context) -> None:
    """Show the version of the example plugin.

    Args:
        ctx: The execution context.
    """
    ctx.print("toolr-plugin-example version 1.0.0")


utils_group = command_group(
    "utils",
    "Utility Commands",
    "General utility commands shipped by the example plugin.",
)


@utils_group.command("echo")
def echo_command(ctx: Context, message: str, repeat: int = 1) -> None:
    """Echo a message multiple times.

    Args:
        ctx: The execution context.
        message: Message to echo.
        repeat: Number of times to repeat the message (default: 1).
    """
    for i in range(repeat):
        ctx.print(f"[{i + 1}] {message}")


@utils_group.command("info")
def info_command(ctx: Context) -> None:
    """Show information about the example plugin.

    Args:
        ctx: The execution context.
    """
    ctx.print("toolr-plugin-example information:")
    ctx.print("- Name: toolr-plugin-example")
    ctx.print("- Version: 1.0.0")
    ctx.print("- Description: Canonical example of a third-party toolr plugin")
"#,
        );

        let fragment =
            build_third_party_fragment(&pkg, "toolr_example_plugin", 1).unwrap();

        // Load the committed reference fragment.
        let reference_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples/plugin-package/src/toolr_example_plugin/toolr-manifest.json");
        let reference: ManifestFragment = serde_json::from_str(
            &std::fs::read_to_string(&reference_path).expect("read committed manifest"),
        )
        .expect("parse committed manifest");

        assert_eq!(fragment, reference, "regenerated fragment differs from committed manifest");
    }
}
