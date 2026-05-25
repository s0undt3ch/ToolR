//! Build a complete static `Manifest` from a `tools/` directory.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

use crate::hash::hash_tools_dir;
use crate::manifest::{ArgumentKind, Manifest, SCHEMA_VERSION};
use crate::parser::types::{SourcesImports, SupportedType, TypeImports, TypeResolutionError};
use crate::parser::{
    commands::extract_commands,
    groups::extract_groups,
    parse_python_file,
    symbols::{ArgSectionTable, EnumTable, TypeAliasTable},
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

    // Pass 1: build cross-file enum + type-alias + arg-section tables
    // from every module so later passes can resolve symbols regardless
    // of which file they live in.
    let mut enums = EnumTable::default();
    let mut aliases = TypeAliasTable::default();
    let mut sections = ArgSectionTable::default();
    for path in &py_files {
        let module = parse_python_file(path).map_err(BuildError::Build)?;
        enums.merge(EnumTable::from_module(&module));
        aliases.merge(TypeAliasTable::from_module(&module));
        sections.merge(ArgSectionTable::from_module(&module));
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

    let arity_errors = validate_positional_arity(&all_commands);
    if !arity_errors.is_empty() {
        return Err(BuildError::InvalidPositionalArity(arity_errors));
    }

    // Validate that every command points at a registered group. Catches
    // typos in `@command(group="ci.helm-diff-pre-comment")` and missing
    // `command_group("…")` declarations.
    let registered: HashSet<&str> =
        all_groups.iter().map(|g| g.name.as_str()).collect::<HashSet<_>>();
    let registered_paths: HashSet<String> =
        all_groups.iter().map(|g| g.full_path()).collect();
    let mut unknown = Vec::new();
    for cmd in &all_commands {
        if cmd.group.is_empty() {
            unknown.push(UnknownGroupRef {
                command: cmd.name.clone(),
                module: cmd.module.clone(),
                referenced: String::new(),
                suggestion: nearest_group(&cmd.group, &registered_paths),
            });
            continue;
        }
        if !registered_paths.contains(&cmd.group) {
            unknown.push(UnknownGroupRef {
                command: cmd.name.clone(),
                module: cmd.module.clone(),
                referenced: cmd.group.clone(),
                suggestion: nearest_group(&cmd.group, &registered_paths),
            });
        }
    }
    drop(registered);
    if !unknown.is_empty() {
        return Err(BuildError::UnknownGroupRefs(unknown));
    }

    let static_hash = hash_tools_dir(tools_dir).map_err(BuildError::Build)?;
    let mut manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash,
        third_party_hash: String::new(),
        groups: all_groups,
        commands: all_commands,
    };

    // Run the user's argparse scanner ([tool.toolr.argparse.*] in
    // tools/pyproject.toml) so its grafted children land in the same
    // static manifest layer alongside the user's @command-decorated
    // commands. The dotted-name derivation mirrors the CLI invocation
    // path: a dispatcher whose name matches its group's leaf segment
    // (`command_group("django")` + `def django(...)`) is addressable
    // as `"django"`; any other command is `"<group>.<name>"`.
    let parents: std::collections::HashMap<String, (String, String)> = manifest
        .commands
        .iter()
        .map(|c| (dotted_name(c), (c.module.clone(), c.function.clone())))
        .collect();

    let project_root = tools_dir.parent().unwrap_or(tools_dir);
    let grafted = crate::argparse::run_for_project(project_root, &parents)
        .map_err(BuildError::Argparse)?;

    // Splice grafted children into the manifest.
    for (_parent, mut children) in grafted.children_by_parent {
        manifest.commands.append(&mut children);
    }

    // Flip the dispatcher flag on each parent that received children.
    for cmd in manifest.commands.iter_mut() {
        if grafted.dispatchers.contains(&dotted_name(cmd)) {
            cmd.is_dispatcher = true;
        }
    }

    Ok(manifest)
}

/// Compute the dotted name a command is addressable by from the CLI
/// (mirrors the dispatcher's `command_group(name)` + `def <name>(...)`
/// pattern). A command whose `name` matches the leaf segment of its
/// `group` is addressable as the group path itself; otherwise it's
/// `"<group>.<name>"` (or just `name` when the group is empty).
fn dotted_name(cmd: &crate::manifest::Command) -> String {
    let leaf = cmd.group.rsplit('.').next().unwrap_or(cmd.group.as_str());
    if !cmd.group.is_empty() && cmd.name == leaf {
        cmd.group.clone()
    } else if cmd.group.is_empty() {
        cmd.name.clone()
    } else {
        format!("{}.{}", cmd.group, cmd.name)
    }
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
    #[error("unknown group references ({count}):\n{details}", count = .0.len(), details = format_unknown_groups(.0))]
    UnknownGroupRefs(Vec<UnknownGroupRef>),
    #[error("invalid positional arity ({count}):\n{details}", count = .0.len(), details = format_positional_arity_errors(.0))]
    InvalidPositionalArity(Vec<PositionalArityError>),
    #[error("argparse scanner error: {0}")]
    Argparse(#[from] crate::argparse::ArgparseError),
}

/// One command whose positional-argument layout violates the
/// zero-or-one positional rules. Surfaced as a batch via
/// [`BuildError::InvalidPositionalArity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionalArityError {
    pub module: String,
    pub command: String,
    pub kind: PositionalArityErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionalArityErrorKind {
    /// Two or more positionals declared `T | None` without a default.
    /// clap can't disambiguate which trailing arg fills which slot.
    MultipleZeroOrOne { first: String, second: String },
    /// A `T | None` positional sits alongside `*args: T` — both compete
    /// for the trailing slot.
    OptionalWithVarPositional { optional: String, var_positional: String },
    /// A required positional (no default, not `T | None`) follows the
    /// zero-or-one slot. The parser can't backtrack to fill a required
    /// slot that comes after one we've already accepted as absent.
    RequiredAfterZeroOrOne { required: String, zero_or_one: String },
}

impl std::fmt::Display for PositionalArityErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MultipleZeroOrOne { first, second } => write!(
                f,
                "two zero-or-one positionals `{first}` and `{second}`. Only one `T | None` positional \
                 is allowed per command; give one of them a default to make it a `--flag`."
            ),
            Self::OptionalWithVarPositional { optional, var_positional } => write!(
                f,
                "the zero-or-one positional `{optional}` cannot coexist with the variadic positional \
                 `*{var_positional}` — both consume the trailing slot."
            ),
            Self::RequiredAfterZeroOrOne { required, zero_or_one } => write!(
                f,
                "the required positional `{required}` follows the zero-or-one positional `{zero_or_one}`. \
                 Move required positionals before any `T | None` parameter."
            ),
        }
    }
}

fn format_positional_arity_errors(errors: &[PositionalArityError]) -> String {
    let mut s = String::new();
    for (i, err) in errors.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        use std::fmt::Write as _;
        let _ = write!(
            &mut s,
            "  - {}::{}: {}",
            err.module, err.command, err.kind,
        );
    }
    s
}

/// Walk every command and surface any positional-arity violation. Runs
/// after type resolution because the "zero-or-one" tag lives on
/// `Argument.resolved_type == Optional(_)`.
fn validate_positional_arity(commands: &[crate::manifest::Command]) -> Vec<PositionalArityError> {
    let mut errors = Vec::new();
    for cmd in commands {
        let mut zero_or_one: Option<&str> = None;
        let mut var_positional: Option<&str> = None;
        for arg in &cmd.arguments {
            match arg.kind {
                ArgumentKind::VarPositional => {
                    var_positional = Some(arg.name.as_str());
                }
                ArgumentKind::Positional => {
                    let is_optional = matches!(
                        arg.resolved_type,
                        Some(SupportedType::Optional(_))
                    );
                    if is_optional {
                        if let Some(first) = zero_or_one {
                            errors.push(PositionalArityError {
                                module: cmd.module.clone(),
                                command: cmd.name.clone(),
                                kind: PositionalArityErrorKind::MultipleZeroOrOne {
                                    first: first.to_string(),
                                    second: arg.name.clone(),
                                },
                            });
                        } else {
                            zero_or_one = Some(arg.name.as_str());
                        }
                    } else if let Some(zo) = zero_or_one {
                        // A non-optional positional after the zero-or-one slot.
                        errors.push(PositionalArityError {
                            module: cmd.module.clone(),
                            command: cmd.name.clone(),
                            kind: PositionalArityErrorKind::RequiredAfterZeroOrOne {
                                required: arg.name.clone(),
                                zero_or_one: zo.to_string(),
                            },
                        });
                    }
                }
                // Keyword-like kinds don't compete for positional slots.
                ArgumentKind::Optional | ArgumentKind::Flag | ArgumentKind::Repeated | ArgumentKind::Count => {}
            }
        }
        if let (Some(zo), Some(vp)) = (zero_or_one, var_positional) {
            errors.push(PositionalArityError {
                module: cmd.module.clone(),
                command: cmd.name.clone(),
                kind: PositionalArityErrorKind::OptionalWithVarPositional {
                    optional: zo.to_string(),
                    var_positional: vp.to_string(),
                },
            });
        }
    }
    errors
}

/// One command whose `group="…"` reference doesn't match any
/// `command_group("…")` declaration in `tools/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGroupRef {
    /// Command name (CLI-visible, hyphenated).
    pub command: String,
    /// Dotted python module the command lives in.
    pub module: String,
    /// The unknown group path the user typed. Empty string when the
    /// `@command` decorator didn't pass `group=` at all.
    pub referenced: String,
    /// Suggested alternative — the most similar registered group path
    /// by Levenshtein distance — to help with typos. `None` when no
    /// group is registered yet or no close match exists.
    pub suggestion: Option<String>,
}

fn format_unknown_groups(errors: &[UnknownGroupRef]) -> String {
    let mut s = String::new();
    for (i, err) in errors.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        use std::fmt::Write as _;
        if err.referenced.is_empty() {
            let _ = write!(
                &mut s,
                "  - {}::{}: `@command` is missing a `group=...` kwarg.",
                err.module, err.command,
            );
        } else {
            let _ = write!(
                &mut s,
                "  - {}::{}: references group `{}` which has no `command_group(...)` declaration.",
                err.module, err.command, err.referenced,
            );
            if let Some(suggestion) = &err.suggestion {
                let _ = write!(&mut s, " Did you mean `{suggestion}`?");
            }
        }
    }
    s
}

/// Return the closest registered group path to `target` by simple
/// edit-distance scoring, when it's within a small threshold. None if
/// nothing useful is close.
fn nearest_group(target: &str, registered: &HashSet<String>) -> Option<String> {
    let max = (target.len() / 3).max(2);
    let mut best: Option<(usize, &String)> = None;
    for candidate in registered {
        let d = edit_distance(target, candidate);
        if d > max {
            continue;
        }
        if best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, s)| s.clone())
}

/// Plain Levenshtein distance (insert / delete / substitute).
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
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

pub(crate) fn list_python_files(tools_dir: &Path) -> Vec<PathBuf> {
    // Skip dot-prefixed directories (`.venv`, `.git`, `.tox`,
    // `.mypy_cache`, `.ruff_cache`, …). Without this, walking
    // `tools/` after `uv sync` would harvest every installed
    // package's `.py` files from `tools/.venv/lib/python*/site-packages/`,
    // producing a manifest with garbage `module` paths like
    // `tools..venv.lib.site-packages.<pkg>.<mod>` that the runner then
    // fails to import. The root `tools_dir` itself is never skipped —
    // a leaf basename starting with `.` is fine if the user pointed
    // us there directly.
    let root = tools_dir.to_path_buf();
    let mut paths: Vec<_> = WalkDir::new(tools_dir)
        .into_iter()
        .filter_entry(|e| {
            if e.path() == root {
                return true;
            }
            !e.file_type().is_dir()
                || !e
                    .file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with('.'))
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"))
        .map(|e| e.into_path())
        .collect();
    paths.sort();
    paths
}

fn module_path_for(tools_dir: &Path, file: &Path) -> String {
    module_path_for_prefix(tools_dir, file, "tools")
}

/// Compute a dotted module path for `file` rooted at `source_dir`, using
/// `prefix` as the leading namespace segment. `__init__.py` files
/// collapse to the prefix itself (the package root). Other files become
/// `<prefix>.<rel_no_ext_with_dots>`.
pub(crate) fn module_path_for_prefix(
    source_dir: &Path,
    file: &Path,
    prefix: &str,
) -> String {
    let rel = file.strip_prefix(source_dir).unwrap_or(file);
    let mut parts: Vec<String> = rel
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.last().map(String::as_str) == Some("__init__") {
        parts.pop();
    }
    let mut out = String::from(prefix);
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

    /// `list_python_files` must NOT recurse into dot-prefixed
    /// directories like `tools/.venv/`. Without this, walking `tools/`
    /// after `uv sync` produces a manifest polluted with the installed
    /// packages' modules — bare `module_path_for` then yields garbage
    /// like `tools..venv.lib.site-packages.<pkg>.<mod>` that the runner
    /// fails to import (`No module named 'tools.'`).
    #[test]
    fn list_python_files_skips_dot_prefixed_directories() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        write(&tools, "ci.py", "x = 1\n");
        write(&tools, ".venv/lib/python3.13/site-packages/pkg/__init__.py", "");
        write(&tools, ".venv/lib/python3.13/site-packages/pkg/commands.py", "y = 2\n");
        write(&tools, ".git/hooks/pre-commit.py", "z = 3\n");

        let files = list_python_files(&tools);
        let rels: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(&tools).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(rels, vec!["ci.py".to_string()], "got: {rels:?}");
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

    /// `@command(group="…")` in one file referring to a
    /// `command_group(...)` declared in another file resolves cleanly
    /// regardless of the file scan order.
    #[test]
    fn cross_file_command_group_string_path_resolves() {
        let tmp = TempDir::new().unwrap();
        // gh_actions.py declares an @command(group="ci") even though
        // ci's command_group(...) lives in _common.py — which sorts
        // earlier, but the registry collection pass makes order
        // irrelevant anyway.
        write(
            tmp.path(),
            "tools/_common.py",
            r#""""CI utilities."""
command_group("ci", docstring=__doc__)
"#,
        );
        write(
            tmp.path(),
            "tools/gh_actions.py",
            r#""""GH Actions helpers."""
@command(group="ci")
def check_run_build(ctx):
    """Check."""
    pass
"#,
        );
        let m = build_static_manifest(&tmp.path().join("tools")).unwrap();
        assert_eq!(m.commands.len(), 1);
        assert_eq!(m.commands[0].group, "ci");
        assert_eq!(m.commands[0].name, "check-run-build");
    }

    /// Typo'd `group=` reference surfaces as a build error with a
    /// nearest-neighbour suggestion.
    #[test]
    fn unknown_group_ref_errors_with_suggestion() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/c.py",
            r#""""CI utilities."""
command_group("ci.helm-diff-pr-comment", docstring=__doc__)

@command(group="ci.helm-diff-pre-comment")
def backend(ctx):
    """Backend."""
    pass
"#,
        );
        let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown group references"), "got: {msg}");
        assert!(msg.contains("ci.helm-diff-pre-comment"), "got: {msg}");
        assert!(msg.contains("Did you mean"), "got: {msg}");
        assert!(msg.contains("ci.helm-diff-pr-comment"), "got: {msg}");
    }

    /// The user's `[tool.toolr.argparse.*]` blocks in
    /// `tools/pyproject.toml` graft children onto user-decorated
    /// dispatcher commands inside `build_static_manifest`. Verifies the
    /// dotted-name derivation that maps a `command_group("django")` +
    /// `def django(...)` dispatcher onto pyproject's `parent = "django"`.
    #[test]
    fn build_static_manifest_grafts_argparse_children() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/dispatcher.py",
            r#""""Django dispatcher."""
group = command_group("django", "Django")

@group.command
def django(ctx):
    """Dispatch to manage.py."""
    pass
"#,
        );
        write(
            tmp.path(),
            "apps/x/management/commands/migrate.py",
            "def add_arguments(self, parser):\n    parser.add_argument('--check', action='store_true')\n",
        );
        write(
            tmp.path(),
            "tools/pyproject.toml",
            r#"
[tool.toolr.argparse.django]
scan_paths = ["apps/*/management/commands/*.py"]

[[tool.toolr.argparse.django.attach]]
parent = "django"
"#,
        );

        let manifest = build_static_manifest(&tmp.path().join("tools")).unwrap();
        let names: std::collections::BTreeSet<_> =
            manifest.commands.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains("django"), "names: {names:?}");
        assert!(names.contains("migrate"), "names: {names:?}");
        let migrate = manifest
            .commands
            .iter()
            .find(|c| c.name == "migrate")
            .unwrap();
        assert_eq!(migrate.group, "django");
        assert_eq!(
            migrate.dispatched_from.as_deref(),
            Some("argparse:django"),
        );
        let django = manifest
            .commands
            .iter()
            .find(|c| c.name == "django")
            .unwrap();
        assert_eq!(migrate.module, django.module);
        assert_eq!(migrate.function, django.function);
        assert!(
            django.is_dispatcher,
            "expected dispatcher flag set on django",
        );
    }

    /// A dispatcher command with both a real CLI flag (`cpu`) and a
    /// `DispatchCommand`-annotated injection kwarg builds cleanly: the
    /// CLI flag lands on the manifest, the injection kwarg is dropped
    /// rather than rejected as "unsupported type `DispatchCommand`".
    #[test]
    fn dispatcher_with_real_flags_and_injection_kwarg_builds() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/dispatcher.py",
            r#""""Django dispatcher."""
from toolr.sources import DispatchCommand

group = command_group("django", "Django")

@group.command
def django(ctx, *, cpu: str = "1", dispatched: DispatchCommand) -> int:
    """Dispatch."""
    return 0
"#,
        );
        let manifest = build_static_manifest(&tmp.path().join("tools")).unwrap();
        let django = manifest
            .commands
            .iter()
            .find(|c| c.name == "django")
            .expect("django command present");
        let arg_names: Vec<&str> = django.arguments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            arg_names,
            vec!["cpu"],
            "expected dispatcher arguments to keep `cpu` and drop `dispatched`",
        );
    }

    /// Bare `@command` (no `group=` kwarg) is a build error pointing
    /// at the missing kwarg.
    #[test]
    fn bare_direct_command_missing_group_errors() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/c.py",
            r#""""CI utilities."""
command_group("ci", docstring=__doc__)

@command
def hello(ctx):
    """Hi."""
    pass
"#,
        );
        let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing a `group=...` kwarg"), "got: {msg}");
    }

    /// `T | None` (no default) is accepted as a single zero-or-one
    /// positional. This is the user's primary use case (e.g.
    /// `def bump(ctx, new_version: str | None)`).
    #[test]
    fn accepts_single_zero_or_one_positional() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/v.py",
            r#""""Version utils."""
group = command_group("version", "Version", docstring=__doc__)

@group.command
def bump(ctx, new_version: str | None) -> None:
    """Bump.

    Args:
        new_version: Explicit version.
    """
"#,
        );
        let m = build_static_manifest(&tmp.path().join("tools")).unwrap();
        let bump = m.commands.iter().find(|c| c.name == "bump").unwrap();
        assert_eq!(bump.arguments.len(), 1);
        assert_eq!(bump.arguments[0].kind, ArgumentKind::Positional);
        assert!(matches!(
            bump.arguments[0].resolved_type,
            Some(SupportedType::Optional(_))
        ));
    }

    #[test]
    fn rejects_multiple_zero_or_one_positionals() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/v.py",
            r#""""Bad."""
group = command_group("x", "X", docstring=__doc__)

@group.command
def f(ctx, a: str | None, b: str | None) -> None:
    """Bad.

    Args:
        a: first.
        b: second.
    """
"#,
        );
        let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("two zero-or-one positionals"), "got: {msg}");
        assert!(msg.contains("`a`"), "got: {msg}");
        assert!(msg.contains("`b`"), "got: {msg}");
    }

    #[test]
    fn rejects_required_positional_after_zero_or_one() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/v.py",
            r#""""Bad."""
group = command_group("x", "X", docstring=__doc__)

@group.command
def f(ctx, maybe: str | None, name: str) -> None:
    """Bad ordering.

    Args:
        maybe: optional.
        name: required.
    """
"#,
        );
        let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("required positional `name` follows"),
            "got: {msg}"
        );
    }

    #[test]
    fn rejects_zero_or_one_combined_with_var_positional() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/v.py",
            r#""""Bad."""
group = command_group("x", "X", docstring=__doc__)

@group.command
def f(ctx, maybe: str | None, *files: str) -> None:
    """Bad combo.

    Args:
        maybe: optional.
        files: trailing.
    """
"#,
        );
        let err = build_static_manifest(&tmp.path().join("tools")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("cannot coexist with the variadic positional"),
            "got: {msg}"
        );
    }

    /// `T | None = None` (with a default) stays a `--flag` and doesn't
    /// trip the zero-or-one validator. Regression guard.
    #[test]
    fn allows_optional_keyword_alongside_required_positional() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "tools/v.py",
            r#""""OK."""
group = command_group("x", "X", docstring=__doc__)

@group.command
def f(ctx, name: str, alias: str | None = None) -> None:
    """OK.

    Args:
        name: required.
        alias: optional flag.
    """
"#,
        );
        let m = build_static_manifest(&tmp.path().join("tools")).unwrap();
        let f = m.commands.iter().find(|c| c.name == "f").unwrap();
        // `alias` should be Optional (flag), not a positional.
        let kinds: Vec<ArgumentKind> = f.arguments.iter().map(|a| a.kind).collect();
        assert_eq!(kinds, vec![ArgumentKind::Positional, ArgumentKind::Optional]);
    }
}
