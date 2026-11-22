//! Build a complete static `Manifest` from a `tools/` directory.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::hash::{SymlinkedModules, list_python_files, snapshot_from, static_hash_of};
use crate::manifest::{ArgumentKind, Command, Group, Manifest, Origin};
use crate::parser::names::{InvalidName, validate_names};
use crate::parser::types::{SourcesImports, SupportedType, TypeImports, TypeResolutionError};
use crate::parser::{
    commands::{
        CommandNameConflict, MissingDocstring, ToolrNames, UnsupportedCommand, binding_vars,
        detect_name_conflicts, detect_unsupported_commands, extract_commands,
        format_missing_docstrings, missing_docstrings,
    },
    groups::{Field, GroupFields, detect_unsupported_group_bindings, extract_groups},
    parse_python_file,
    symbols::{ArgSectionTable, EnumTable, ImportTable, TypeAliasTable},
};
use crate::third_party::{ThirdPartyError, discover_and_merge};

/// A built manifest and its build's warnings, which the manifest doesn't
/// keep: they're printed once, by whoever built it.
#[derive(Debug)]
pub struct BuiltManifest {
    pub manifest: Manifest,
    pub warnings: Vec<String>,
}

impl BuiltManifest {
    /// The build's warnings, then the manifest's plugin warnings, with
    /// control characters stripped for the terminal.
    pub fn all_warnings(&self) -> impl Iterator<Item = String> + '_ {
        self.warnings
            .iter()
            .chain(self.manifest.plugin_warnings.iter().map(|w| &w.message))
            .map(|w| crate::text::strip_control_chars(w))
    }
}

/// A manifest read from disk, whose build's warnings were printed then.
impl From<Manifest> for BuiltManifest {
    fn from(manifest: Manifest) -> Self {
        Self {
            manifest,
            warnings: Vec::new(),
        }
    }
}

/// Build the static portion of a manifest from a tools directory.
///
/// Surfaces every unsupported-type rejection in a single batch via
/// [`BuildError::UnsupportedTypes`] so users see all the offending
/// annotations at once rather than one-at-a-time on each rebuild.
pub fn build_static_manifest(tools_dir: &Path) -> Result<BuiltManifest> {
    build_static_manifest_inner(tools_dir).map_err(BuildError::into_anyhow)
}

fn build_static_manifest_inner(tools_dir: &Path) -> std::result::Result<BuiltManifest, BuildError> {
    // Name validation is deferred so argparse-scanned flags are reported in
    // the same batch as the native names.
    // One walk feeds both the parser and the hash.
    let py_files = list_python_files(tools_dir)?;
    // Hashed before anything is parsed, never after: a file edited during
    // the build must leave a record that no longer matches it, not one
    // describing bytes the manifest doesn't reflect.
    let input_records = snapshot_from(tools_dir, &py_files, None).map_err(BuildError::Build)?;
    let built = build_commands_inner(tools_dir, &py_files, "tools", false)?;

    let mut manifest = Manifest {
        static_hash: static_hash_of(&input_records.files),
        toolr_version: env!("CARGO_PKG_VERSION").to_string(),
        groups: built.groups,
        commands: built.commands,
        input_records,
        ..Manifest::empty()
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
    let grafted =
        crate::argparse::run_for_project(project_root, &parents).map_err(BuildError::Argparse)?;

    // Native names and grafted argparse flags are validated together.
    // Unlike fragment builds, unknown-group refs were checked before names:
    // names wait so grafted argparse flags land in the same batch.
    let mut parents_sorted: Vec<_> = grafted.children_by_parent.iter().collect();
    parents_sorted.sort_by(|a, b| a.0.cmp(b.0));
    let grafted_children: Vec<Command> = parents_sorted
        .into_iter()
        .flat_map(|(_, children)| children.iter().cloned())
        .collect();
    let mut checked = manifest.commands.clone();
    checked.extend(grafted_children);
    let name_errors = validate_names(&manifest.groups, &checked);
    if !name_errors.is_empty() {
        return Err(BuildError::InvalidNames(name_errors));
    }

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

    Ok(BuiltManifest {
        manifest,
        warnings: built.warnings,
    })
}

/// Parse every `.py` under `source_root` and run every validation shared by
/// the local `tools/` build and the plugin fragment build.
pub(crate) fn build_commands(
    source_root: &Path,
    module_prefix: &str,
) -> std::result::Result<BuiltTree, BuildError> {
    let py_files = list_python_files(source_root)?;
    build_commands_inner(source_root, &py_files, module_prefix, true)
}

fn build_commands_inner(
    source_root: &Path,
    py_files: &[PathBuf],
    module_prefix: &str,
    check_names: bool,
) -> std::result::Result<BuiltTree, BuildError> {
    // Pass 1: build cross-file enum + type-alias + arg-section tables
    // from every module so later passes can resolve symbols regardless
    // of which file they live in.
    let mut enums = EnumTable::default();
    let mut aliases = TypeAliasTable::default();
    let mut sections = ArgSectionTable::default();
    let mut all_imports: std::collections::HashMap<String, ImportTable> =
        std::collections::HashMap::new();
    for path in py_files {
        let module = parse_python_file(path).map_err(BuildError::Build)?;
        let module_path = module_path_for_prefix(source_root, path, module_prefix);
        let is_package = path.file_stem().map(|s| s == "__init__").unwrap_or(false);
        all_imports.insert(
            module_path.clone(),
            ImportTable::from_module(&module, &module_path, is_package),
        );
        enums.merge(EnumTable::from_module(&module, &module_path));
        aliases.merge(TypeAliasTable::from_module_at(&module, &module_path));
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
    let mut all_commands = Vec::new();
    // Each group's declarations so far, in first-declaration order, and the
    // index of each group's full path in it.
    let mut seen_groups: Vec<SeenGroup> = Vec::new();
    let mut seen_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut contradictory: Vec<ContradictoryGroup> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut global_vars: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut type_errors: Vec<TypeResolutionError> = Vec::new();
    let mut name_conflicts: Vec<CommandNameConflict> = Vec::new();
    let mut unsupported: Vec<UnsupportedCommand> = Vec::new();
    let mut unsupported_groups: Vec<UnsupportedCommand> = Vec::new();
    let names = ToolrNames::new(&all_imports);
    for path in py_files {
        let module = parse_python_file(path).map_err(BuildError::Build)?;
        let module_path = module_path_for_prefix(source_root, path, module_prefix);
        let module_doc = module_docstring(&module);
        name_conflicts.extend(detect_name_conflicts(&module, &module_path, &names));
        let bindings = extract_groups(
            &module,
            module_doc.as_deref(),
            &global_vars,
            &names,
            &module_path,
        );
        unsupported_groups.extend(detect_unsupported_group_bindings(
            &module,
            &module_path,
            &names,
        ));
        unsupported.extend(detect_unsupported_commands(
            &module,
            &module_path,
            &names,
            &binding_vars(&bindings, &global_vars),
        ));
        let type_imports = TypeImports::from_module(&module);
        let sources_imports = SourcesImports::from_module(&module);
        let consts = super::symbols::ConstTable::from_module(&module);
        let commands = extract_commands(
            &module,
            &module_path,
            &bindings,
            &enums,
            &all_imports,
            &names,
            &consts,
            &type_imports,
            &sources_imports,
            &aliases,
            &sections,
            &global_vars,
            &mut type_errors,
        );
        // Make this file's bindings visible to subsequent files.
        for binding in &bindings {
            global_vars.insert(binding.var.clone(), binding.full_path());
        }
        // Dedup groups by *full_path* (not just leaf name), so nested
        // groups in different branches with the same leaf name (e.g.
        // `ci.image` + `docker.image`) both survive. The first declaration
        // wins, as in Python: a later one fills a field the first left
        // empty, and is warned about where both are set and differ.
        for binding in bindings {
            let path = binding.full_path();
            if binding.contradictory {
                contradictory.push(ContradictoryGroup {
                    group: path.clone(),
                    module: module_path.clone(),
                });
            }
            match seen_index.get(&path) {
                Some(&index) => {
                    let seen = &mut seen_groups[index];
                    if !seen.modules.contains(&module_path) {
                        seen.modules.push(module_path.clone());
                    }
                    seen.describes |= binding.describes;
                    seen.unreadable |= binding.unreadable;
                    warnings.extend(redeclaration_warning(
                        &path,
                        &seen.fields,
                        &binding.fields,
                        &module_path,
                    ));
                    fill_empty_fields(&mut seen.fields, &binding.fields, &module_path);
                }
                None => {
                    seen_index.insert(path.clone(), seen_groups.len());
                    seen_groups.push(SeenGroup {
                        path,
                        name: binding.name,
                        parent: binding.parent,
                        modules: vec![module_path.clone()],
                        describes: binding.describes,
                        unreadable: binding.unreadable,
                        fields: binding.fields.map(|field| Kept {
                            field,
                            from: module_path.clone(),
                        }),
                    });
                }
            }
        }
        all_commands.extend(commands);
    }
    let all_groups: Vec<Group> = seen_groups.iter().map(SeenGroup::group).collect();

    // First, as Python's `command_group` raises these when the module imports.
    if !contradictory.is_empty() {
        return Err(BuildError::ContradictoryGroups(contradictory));
    }

    if !name_conflicts.is_empty() {
        return Err(BuildError::ConflictingCommandName(name_conflicts));
    }

    // Group problems first: a dropped group hides what its commands would report.
    let unsupported = [unsupported_groups, unsupported].concat();
    if !unsupported.is_empty() {
        return Err(BuildError::UnsupportedCommands(unsupported));
    }

    if !type_errors.is_empty() {
        return Err(BuildError::UnsupportedTypes(type_errors));
    }

    let undocumented = missing_docstrings(&all_commands);
    if !undocumented.is_empty() {
        return Err(BuildError::MissingDocstrings(undocumented));
    }

    let arity_errors = validate_positional_arity(&all_commands);
    if !arity_errors.is_empty() {
        return Err(BuildError::InvalidPositionalArity(arity_errors));
    }

    if check_names {
        let name_errors = validate_names(&all_groups, &all_commands);
        if !name_errors.is_empty() {
            return Err(BuildError::InvalidNames(name_errors));
        }
    }

    // Validate that every command points at a registered group. Catches
    // typos in `@command(group="ci.helm-diff-pre-comment")` and missing
    // `command_group("…")` declarations.
    let registered_paths: HashSet<String> = all_groups.iter().map(|g| g.full_path()).collect();
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
    if !unknown.is_empty() {
        return Err(BuildError::UnknownGroupRefs(unknown));
    }

    let duplicates = duplicate_commands(&all_commands);
    if !duplicates.is_empty() {
        return Err(BuildError::DuplicateCommands(duplicates));
    }

    let undescribed = undescribed_groups(&seen_groups);
    if !undescribed.is_empty() {
        return Err(BuildError::UndescribedGroups(undescribed));
    }

    Ok(BuiltTree {
        groups: all_groups,
        commands: all_commands,
        warnings,
    })
}

/// The declarations of one group seen so far by `build_commands_inner`.
struct SeenGroup {
    /// Dotted group path.
    path: String,
    name: String,
    parent: Option<String>,
    /// Every declaring module, first declaration first.
    modules: Vec<String>,
    /// Whether any declaration passes a description or docstring.
    describes: bool,
    /// Whether any declaration may pass one the static parser can't see.
    unreadable: bool,
    /// The fields kept so far: the first declaration's, with empty ones
    /// filled by later declarations.
    fields: GroupFields<Kept>,
}

impl SeenGroup {
    /// The group the manifest gets, with the kept fields' readable text.
    fn group(&self) -> Group {
        Group {
            name: self.name.clone(),
            title: self.fields.title.field.text().to_string(),
            description: self.fields.description.field.text().to_string(),
            parent: self.parent.clone(),
            origin: Origin::Static,
        }
    }
}

/// A kept group field and the module it came from.
struct Kept {
    field: Field,
    from: String,
}

/// Every group no declaration describes, in first-declaration order.
///
/// `toolr.testing._groups._undescribed_groups` mirrors this in Python; change
/// both together.
fn undescribed_groups(seen: &[SeenGroup]) -> Vec<UndescribedGroup> {
    seen.iter()
        .filter(|g| !g.describes)
        .map(|g| UndescribedGroup {
            group: g.path.clone(),
            modules: g.modules.clone(),
            unreadable: g.unreadable,
        })
        .collect()
}

/// A `command_group` call passing a docstring with a description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContradictoryGroup {
    /// Dotted group path.
    pub group: String,
    /// The module making the call.
    pub module: String,
}

/// A group none of whose declarations passes a description or docstring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndescribedGroup {
    /// Dotted group path.
    pub group: String,
    /// Every declaring module, in build order.
    pub modules: Vec<String>,
    /// Whether a declaration may pass a description or docstring the static
    /// parser can't see.
    pub unreadable: bool,
}

/// `toolr.testing._groups._format_contradictory_groups` mirrors this in
/// Python; change both together.
fn format_contradictory_groups(groups: &[ContradictoryGroup]) -> String {
    use crate::text::escape_control_chars;
    groups
        .iter()
        .map(|g| {
            format!(
                "  - `{}` in {}: pass `docstring=` or `description=`/`long_description=`, not both.",
                escape_control_chars(&g.group),
                escape_control_chars(&g.module)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `toolr.testing._groups._format_undescribed_groups` mirrors this in
/// Python; change both together.
fn format_undescribed_groups(groups: &[UndescribedGroup]) -> String {
    use crate::text::escape_control_chars;
    groups
        .iter()
        .map(|g| {
            let modules = g
                .modules
                .iter()
                .map(|m| escape_control_chars(m))
                .collect::<Vec<_>>()
                .join(", ");
            let hint = if g.unreadable {
                "give one declaration a description or docstring the static parser can read: \
                 a string literal, or the `__doc__` of a module with a docstring."
            } else {
                "give one declaration a description or docstring."
            };
            format!(
                "  - `{}` (declared in {modules}): {hint}",
                escape_control_chars(&g.group)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The groups and commands of one source tree, plus its build warnings.
#[derive(Debug)]
pub(crate) struct BuiltTree {
    pub(crate) groups: Vec<Group>,
    pub(crate) commands: Vec<Command>,
    pub(crate) warnings: Vec<String>,
}

/// The warning for a group re-declaration that disagrees with the kept
/// fields, naming the module each kept value came from. Python's
/// `command_group` logs the same disagreement at debug level in its own
/// words. `None` when they agree.
fn redeclaration_warning(
    path: &str,
    kept: &GroupFields<Kept>,
    later: &GroupFields,
    later_module: &str,
) -> Option<String> {
    // Only two readable values can differ: an empty one has no opinion, and
    // an unreadable one can't be compared.
    let differing: Vec<(&str, &str)> = kept
        .named()
        .into_iter()
        .zip(later.named())
        .filter(|((_, kept), (_, later))| {
            matches!((&kept.field, later), (Field::Text(a), Field::Text(b)) if a != b)
        })
        .map(|((name, kept), _)| (name, kept.from.as_str()))
        .collect();
    if differing.is_empty() {
        return None;
    }
    // `title, description from tools.a and long_description from tools.b`.
    let mut sources: Vec<(Vec<&str>, &str)> = Vec::new();
    for &(name, module) in &differing {
        match sources.iter_mut().find(|(_, m)| *m == module) {
            Some((names, _)) => names.push(name),
            None => sources.push((vec![name], module)),
        }
    }
    let kept_text = sources
        .iter()
        .map(|(names, module)| format!("the {} from {module}", names.join(", ")))
        .collect::<Vec<_>>()
        .join(" and ");
    let names: Vec<&str> = differing.iter().map(|(name, _)| *name).collect();
    Some(format!(
        "group `{path}` is declared more than once; keeping {kept_text} and ignoring the \
         different {} in {later_module}",
        names.join(", ")
    ))
}

/// Fill the kept fields a later declaration gives where they're empty, as
/// Python's `command_group` does, recording `module` as their source.
fn fill_empty_fields(kept: &mut GroupFields<Kept>, later: &GroupFields, module: &str) {
    for ((_, kept), (_, later)) in kept.named_mut().into_iter().zip(later.named()) {
        if kept.field == Field::Empty && *later != Field::Empty {
            *kept = Kept {
                field: later.clone(),
                from: module.to_string(),
            };
        }
    }
}

/// A `(group, name)` pair more than one function in one source tree defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCommand {
    /// Dotted group path, `""` at top level.
    pub group: String,
    /// The command's CLI name.
    pub name: String,
    /// `(module, function)` of every definition, in build order.
    pub definitions: Vec<(String, String)>,
}

/// Every `(group, name)` defined more than once, keyed in first-seen order.
fn duplicate_commands(commands: &[Command]) -> Vec<DuplicateCommand> {
    let mut index: std::collections::HashMap<(&str, &str), usize> =
        std::collections::HashMap::new();
    let mut all: Vec<DuplicateCommand> = Vec::new();
    for cmd in commands {
        let slot = *index
            .entry((cmd.group.as_str(), cmd.name.as_str()))
            .or_insert_with(|| {
                all.push(DuplicateCommand {
                    group: cmd.group.clone(),
                    name: cmd.name.clone(),
                    definitions: Vec::new(),
                });
                all.len() - 1
            });
        all[slot]
            .definitions
            .push((cmd.module.clone(), cmd.function.clone()));
    }
    all.retain(|d| d.definitions.len() > 1);
    all
}

/// `toolr.testing._shapes._format_unsupported_commands` mirrors this in
/// Python; change both together.
fn format_unsupported_commands(unsupported: &[UnsupportedCommand]) -> String {
    use crate::text::escape_control_chars;
    unsupported
        .iter()
        .map(|u| {
            format!(
                "  - {}::{}: {}",
                escape_control_chars(&u.module),
                escape_control_chars(&u.function),
                u.kind
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `toolr.testing._duplicates._format_duplicate_commands` mirrors this in
/// Python; change both together.
fn format_duplicate_commands(duplicates: &[DuplicateCommand]) -> String {
    use crate::text::escape_control_chars;
    duplicates
        .iter()
        .map(|d| {
            let path = crate::third_party::merge::command_path(&d.group, &d.name);
            let definitions = d
                .definitions
                .iter()
                .map(|(m, f)| format!("{}::{}", escape_control_chars(m), escape_control_chars(f)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "  - `{}` is defined more than once: {definitions}. \
                 Rename all but one, e.g. with `name=\"…\"`.",
                escape_control_chars(&path)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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
) -> Result<BuiltManifest, BuildError> {
    let base = build_static_manifest_inner(tools_dir)?;
    Ok(BuiltManifest {
        manifest: discover_and_merge(tools_venv, base.manifest).map_err(BuildError::ThirdParty)?,
        warnings: base.warnings,
    })
}

/// Error type covering both the local build and the third-party merge.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("static build error: {0}")]
    Build(#[source] anyhow::Error),
    #[error("third-party merge error: {0}")]
    ThirdParty(#[from] ThirdPartyError),
    #[error("invalid parameter declarations ({count}):\n{details}", count = .0.len(), details = format_type_errors(.0))]
    UnsupportedTypes(Vec<TypeResolutionError>),
    #[error("commands without a docstring ({count}):\n{details}", count = .0.len(), details = format_missing_docstrings(.0))]
    MissingDocstrings(Vec<MissingDocstring>),
    /// `toolr.testing._unknown_groups._format_unknown_groups` mirrors this
    /// message in Python; change both together.
    #[error("unknown group references ({count}):\n{details}", count = .0.len(), details = format_unknown_groups(.0))]
    UnknownGroupRefs(Vec<UnknownGroupRef>),
    #[error("invalid positional arity ({count}):\n{details}", count = .0.len(), details = format_positional_arity_errors(.0))]
    InvalidPositionalArity(Vec<PositionalArityError>),
    #[error("conflicting command name ({count}):\n{details}", count = .0.len(), details = format_name_conflicts(.0))]
    ConflictingCommandName(Vec<CommandNameConflict>),
    #[error("invalid names ({count}):\n{details}", count = .0.len(), details = format_invalid_names(.0))]
    InvalidNames(Vec<InvalidName>),
    /// `toolr.testing._shapes._format_unsupported_commands` mirrors this
    /// message in Python; change both together.
    #[error("unsupported command declarations ({count}):\n{details}", count = .0.len(), details = format_unsupported_commands(.0))]
    UnsupportedCommands(Vec<UnsupportedCommand>),
    /// `toolr.testing._duplicates._format_duplicate_commands` mirrors this
    /// message in Python; change both together.
    #[error("duplicate commands ({count}):\n{details}", count = .0.len(), details = format_duplicate_commands(.0))]
    DuplicateCommands(Vec<DuplicateCommand>),
    /// `toolr.testing._groups._format_contradictory_groups` mirrors this
    /// message in Python; change both together.
    #[error("command groups with contradictory arguments ({count}):\n{details}", count = .0.len(), details = format_contradictory_groups(.0))]
    ContradictoryGroups(Vec<ContradictoryGroup>),
    /// `toolr.testing._groups._format_undescribed_groups` mirrors this
    /// message in Python; change both together.
    #[error("command groups with no description ({count}):\n{details}", count = .0.len(), details = format_undescribed_groups(.0))]
    UndescribedGroups(Vec<UndescribedGroup>),
    #[error("argparse scanner error: {0}")]
    Argparse(#[from] crate::argparse::ArgparseError),
    #[error(transparent)]
    SymlinkedModules(#[from] SymlinkedModules),
}

impl BuildError {
    /// Convert for `anyhow` callers, so a build error reads the same with or
    /// without a venv: a local build error unwrapped, a symlinked-modules
    /// rejection kept typed so dispatch can tell it apart, and the rest
    /// flattened, since their messages already include their sources.
    pub fn into_anyhow(self) -> anyhow::Error {
        match self {
            BuildError::Build(e) => e,
            BuildError::SymlinkedModules(links) => links.into(),
            other => anyhow::anyhow!("{other}"),
        }
    }
}

fn format_invalid_names(errors: &[InvalidName]) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for (i, e) in errors.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        let place = if e.module.is_empty() {
            e.owner.clone()
        } else {
            format!("{}::{}", e.module, e.owner)
        };
        let _ = write!(&mut s, "  - {place} {} `{}`: {}", e.item, e.value, e.reason);
    }
    s
}

fn format_name_conflicts(conflicts: &[CommandNameConflict]) -> String {
    let mut s = String::new();
    for (i, c) in conflicts.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        use std::fmt::Write as _;
        let _ = write!(
            &mut s,
            "  - {}::{}: command name passed both positionally and via `name=`. \
             Pass it one way only, e.g. `command(name=\"…\")`.",
            c.module, c.function,
        );
    }
    s
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
    OptionalWithVarPositional {
        optional: String,
        var_positional: String,
    },
    /// A required positional (no default, not `T | None`) follows the
    /// zero-or-one slot. The parser can't backtrack to fill a required
    /// slot that comes after one we've already accepted as absent.
    RequiredAfterZeroOrOne {
        required: String,
        zero_or_one: String,
    },
}

impl std::fmt::Display for PositionalArityErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MultipleZeroOrOne { first, second } => write!(
                f,
                "two zero-or-one positionals `{first}` and `{second}`. Only one `T | None` positional \
                 is allowed per command; give one of them a default to make it a `--flag`."
            ),
            Self::OptionalWithVarPositional {
                optional,
                var_positional,
            } => write!(
                f,
                "the zero-or-one positional `{optional}` cannot coexist with the variadic positional \
                 `*{var_positional}` — both consume the trailing slot."
            ),
            Self::RequiredAfterZeroOrOne {
                required,
                zero_or_one,
            } => write!(
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
        let _ = write!(&mut s, "  - {}::{}: {}", err.module, err.command, err.kind,);
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
                ArgumentKind::Positional | ArgumentKind::OptionalPositional => {
                    let is_optional = matches!(arg.kind, ArgumentKind::OptionalPositional)
                        || matches!(arg.resolved_type, Some(SupportedType::Optional(_)));
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
                // Positional-style FixedArity (no long_flag) always
                // consumes a fixed slot — same ordering rule as a plain
                // required Positional. Keyword-style FixedArity doesn't
                // compete for positional slots at all.
                ArgumentKind::FixedArity if arg.long_flag.is_none() => {
                    if let Some(zo) = zero_or_one {
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
                ArgumentKind::Optional
                | ArgumentKind::Flag
                | ArgumentKind::Repeated
                | ArgumentKind::Count
                | ArgumentKind::FixedArity => {}
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

/// `toolr.testing._unknown_groups._format_unknown_groups` mirrors this in
/// Python; change both together.
fn format_unknown_groups(errors: &[UnknownGroupRef]) -> String {
    use crate::text::escape_control_chars;
    let mut s = String::new();
    for (i, err) in errors.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        use std::fmt::Write as _;
        let module = escape_control_chars(&err.module);
        let command = escape_control_chars(&err.command);
        if err.referenced.is_empty() {
            let _ = write!(
                &mut s,
                "  - {module}::{command}: `@command` is missing a `group=...` kwarg.",
            );
        } else {
            let _ = write!(
                &mut s,
                "  - {module}::{command}: references group `{}` which has no `command_group(...)` declaration.",
                escape_control_chars(&err.referenced),
            );
            if let Some(suggestion) = &err.suggestion {
                let _ = write!(
                    &mut s,
                    " Did you mean `{}`?",
                    escape_control_chars(suggestion)
                );
            }
        }
    }
    s
}

/// Return the closest registered group path to `target` by simple
/// edit-distance scoring, when it's within a small threshold. None if
/// nothing useful is close.
///
/// `toolr.testing._unknown_groups._nearest_group` mirrors this in Python;
/// change both together.
fn nearest_group(target: &str, registered: &HashSet<String>) -> Option<String> {
    let max = (target.len() / 3).max(2);
    let mut best: Option<(usize, &String)> = None;
    for candidate in registered {
        let d = edit_distance(target, candidate);
        if d > max {
            continue;
        }
        // `HashSet` order is random, so a tie goes to the smallest name.
        if best.is_none_or(|(bd, b)| d < bd || (d == bd && candidate < b)) {
            best = Some((d, candidate));
        }
    }
    best.map(|(_, s)| s.clone())
}

/// Plain Levenshtein distance (insert / delete / substitute).
///
/// `toolr.testing._unknown_groups._edit_distance` mirrors this in Python;
/// change both together.
pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
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
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
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

/// Compute a dotted module path for `file` rooted at `source_dir`, using
/// `prefix` as the leading namespace segment. `__init__.py` files
/// collapse to the prefix itself (the package root). Other files become
/// `<prefix>.<rel_no_ext_with_dots>`.
pub(crate) fn module_path_for_prefix(source_dir: &Path, file: &Path, prefix: &str) -> String {
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

/// The module's docstring, or `None` when it has none.
fn module_docstring(module: &ruff_python_ast::ModModule) -> Option<String> {
    use ruff_python_ast::Stmt;
    let Some(Stmt::Expr(e)) = module.body.first() else {
        return None;
    };
    match e.value.as_ref() {
        ruff_python_ast::Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
