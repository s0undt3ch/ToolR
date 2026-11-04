//! Extract `@<group>.command`-decorated function definitions.

use std::collections::HashMap;

use ruff_python_ast::statement_visitor::{StatementVisitor, walk_body, walk_stmt};
use ruff_python_ast::{Arguments, Decorator, Expr, ModModule, Stmt, StmtFunctionDef};

use super::groups::GroupBinding;
use super::parse_docstring;
use super::signatures::extract_arguments;
use super::symbols::ArgSectionTable;
use super::symbols::ConstTable;
use super::symbols::EnumTable;
use super::symbols::ImportTable;
use super::symbols::TypeAliasTable;
use super::types::{SourcesImports, TypeImports, TypeResolutionError, resolve_arguments};
use crate::manifest::{Command, Origin};

type GlobalVars = HashMap<String, String>;

/// Walk module body for functions decorated with `@<var>.command`
/// (group-binding style) or `@command(group="…")` (string-path style).
/// Emit one Command per match.
#[allow(clippy::too_many_arguments)] // each context parameter is distinct; bundling would obscure call sites.
pub fn extract_commands(
    module: &ModModule,
    module_path: &str,
    bindings: &[GroupBinding],
    enums: &EnumTable,
    all_imports: &HashMap<String, ImportTable>,
    consts: &ConstTable,
    type_imports: &TypeImports,
    sources: &SourcesImports,
    aliases: &TypeAliasTable,
    sections: &ArgSectionTable,
    global_vars: &GlobalVars,
    errors: &mut Vec<TypeResolutionError>,
) -> Vec<Command> {
    // Map `binding_var → group_full_path`. Local bindings shadow the
    // global map so a same-named variable defined in this file wins
    // over an inherited one; otherwise the global cross-file map
    // covers `from ._common import group; @group.command` style.
    let by_var = binding_vars(bindings, global_vars);
    let empty = ImportTable::default();
    let names = ToolrNames(all_imports.get(module_path).unwrap_or(&empty));
    let mut out = Vec::new();
    for stmt in &module.body {
        let Stmt::FunctionDef(func) = stmt else {
            continue;
        };
        let Some(target) = command_decorator(&func.decorator_list, names) else {
            continue;
        };
        let (group_full_path, override_name) = match target {
            CommandDecorator::Binding {
                var, explicit_name, ..
            } => {
                let Some(group_name) = by_var.get(var.as_str()) else {
                    continue;
                };
                (group_name.clone(), explicit_name)
            }
            CommandDecorator::StringPath {
                explicit_name,
                group,
                ..
            } => {
                // `group=` is required on the string-path form. If it's
                // missing or the targeted group isn't registered
                // anywhere, we still emit the command — the build-
                // time validator surfaces a clear error pointing the
                // user at `command_group("…")`.
                (group.unwrap_or_default(), explicit_name)
            }
        };
        out.push(build_command(
            func,
            &group_full_path,
            override_name.as_deref(),
            module_path,
            enums,
            all_imports,
            consts,
            type_imports,
            sources,
            aliases,
            sections,
            errors,
        ));
    }
    out
}

/// `binding_var → group_full_path` for one module: its own bindings shadow
/// the ones earlier files made visible.
pub fn binding_vars(bindings: &[GroupBinding], global_vars: &GlobalVars) -> GlobalVars {
    let mut by_var = global_vars.clone();
    for b in bindings {
        by_var.insert(b.var.clone(), b.group.full_path());
    }
    by_var
}

/// Why toolr can't read a command declaration without running it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedCommandKind {
    /// The decorated `def` isn't directly in the module body.
    Nested,
    /// `group=` isn't a string literal.
    NonLiteralGroup,
    /// The positional name or `name=` isn't a string literal.
    NonLiteralName,
    /// More than one toolr command decorator on the same `def`.
    Stacked,
}

impl std::fmt::Display for UnsupportedCommandKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Nested => {
                "`@command` must decorate a top-level `def`, which toolr reads without running it."
            }
            Self::NonLiteralGroup => "`group=` must be a string literal.",
            Self::NonLiteralName => "the command name must be a string literal.",
            Self::Stacked => "a function can carry only one command decorator.",
        })
    }
}

/// A command declaration the static parser can't read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedCommand {
    /// Dotted python module the command lives in (`tools.foo.bar`).
    pub module: String,
    /// Python function name of the offending command.
    pub function: String,
    pub kind: UnsupportedCommandKind,
}

/// Every unsupported command declaration in `module`, in source order.
///
/// `toolr.testing._shapes._unsupported_commands` mirrors this in Python;
/// change both together.
pub fn detect_unsupported_commands(
    module: &ModModule,
    module_path: &str,
    imports: &ImportTable,
    by_var: &GlobalVars,
) -> Vec<UnsupportedCommand> {
    let mut finder = UnsupportedFinder {
        module_path,
        names: ToolrNames(imports),
        by_var,
        depth: 0,
        out: Vec::new(),
    };
    for stmt in &module.body {
        finder.visit_stmt(stmt);
    }
    finder.out
}

struct UnsupportedFinder<'a> {
    module_path: &'a str,
    names: ToolrNames<'a>,
    by_var: &'a GlobalVars,
    /// How many bodies deep the walk is; module-level statements are at 0.
    depth: usize,
    out: Vec<UnsupportedCommand>,
}

impl UnsupportedFinder<'_> {
    fn check(&mut self, func: &StmtFunctionDef) {
        // `@<var>.command` counts only on a module-level group.
        let matched: Vec<_> = func
            .decorator_list
            .iter()
            .filter_map(|d| {
                let target = parse_string_path_command(&d.expression, self.names)
                    .or_else(|| parse_binding_command(&d.expression))?;
                match target {
                    CommandDecorator::Binding { var, .. } if !self.by_var.contains_key(&var) => {
                        None
                    }
                    CommandDecorator::Binding { .. } => Some((d, false)),
                    CommandDecorator::StringPath { .. } => Some((d, true)),
                }
            })
            .collect();
        let Some(&(decorator, string_path)) = matched.first() else {
            return;
        };
        let mut push = |kind| {
            self.out.push(UnsupportedCommand {
                module: self.module_path.to_string(),
                function: func.name.as_str().to_string(),
                kind,
            });
        };
        if self.depth > 0 {
            push(UnsupportedCommandKind::Nested);
            return;
        }
        if matched.len() > 1 {
            push(UnsupportedCommandKind::Stacked);
            return;
        }
        let Expr::Call(call) = &decorator.expression else {
            return;
        };
        let non_literal = |e: &Expr| !matches!(e, Expr::StringLiteral(_));
        let keyword = |name: &str| {
            call.arguments
                .keywords
                .iter()
                .find(|k| k.arg.as_ref().map(ruff_python_ast::Identifier::as_str) == Some(name))
                .map(|k| &k.value)
        };
        if string_path && keyword("group").is_some_and(non_literal) {
            push(UnsupportedCommandKind::NonLiteralGroup);
        }
        if call.arguments.args.first().is_some_and(non_literal)
            || keyword("name").is_some_and(non_literal)
        {
            push(UnsupportedCommandKind::NonLiteralName);
        }
    }
}

impl<'a> StatementVisitor<'a> for UnsupportedFinder<'_> {
    fn visit_body(&mut self, body: &'a [Stmt]) {
        self.depth += 1;
        walk_body(self, body);
        self.depth -= 1;
    }

    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if let Stmt::FunctionDef(func) = stmt {
            self.check(func);
        }
        walk_stmt(self, stmt);
    }
}

/// Which flavour of `@command` decorator we saw on a function, plus the
/// metadata pulled out of it.
#[cfg_attr(test, derive(Debug, PartialEq))]
enum CommandDecorator {
    /// Group binding: `@<binding>.command` or `@<binding>.command("name")` —
    /// `<binding>` is a Python variable name that must resolve to a
    /// known `CommandGroup`. The call form allows overriding the CLI
    /// name via a leading string positional.
    Binding {
        var: String,
        explicit_name: Option<String>,
        /// Both a positional string and a `name=` keyword were passed —
        /// an ambiguous override the build-time validator rejects via
        /// [`CommandNameConflict`].
        name_conflict: bool,
    },
    /// String path: `@command(group="dotted.path")` or `@command("name", group=...)`
    /// or bare `@command` (no group — caught by validator).
    StringPath {
        explicit_name: Option<String>,
        group: Option<String>,
        /// See [`CommandDecorator::Binding::name_conflict`].
        name_conflict: bool,
    },
}

/// A command whose decorator passed the CLI-name override *both*
/// positionally and via `name=`. Surfaced as a batch build error so the
/// user fixes the ambiguity rather than silently getting one of the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandNameConflict {
    /// Dotted python module the command lives in (`tools.foo.bar`).
    pub module: String,
    /// Python function name of the offending command.
    pub function: String,
}

/// A command whose docstring gives it no `--help` summary line: no
/// docstring at all, an empty or blank one, or only sections such as
/// `Args:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingDocstring {
    /// Dotted python module the command lives in (`tools.foo.bar`).
    pub module: String,
    /// Python function name of the offending command.
    pub function: String,
}

/// Every command in `commands` without a summary line, in order.
pub fn missing_docstrings(commands: &[Command]) -> Vec<MissingDocstring> {
    commands
        .iter()
        .filter(|c| c.summary.trim().is_empty())
        .map(|c| MissingDocstring {
            module: c.module.clone(),
            function: c.function.clone(),
        })
        .collect()
}

/// The indented bullet list both manifest builders print under their
/// "commands without a docstring" heading.
pub fn format_missing_docstrings(missing: &[MissingDocstring]) -> String {
    missing
        .iter()
        .map(|m| {
            format!(
                "  - {}::{}: add a docstring. Its first line is the command's `--help` summary.",
                m.module, m.function
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Resolve the explicit CLI-name override from a `command(...)` call's
/// arguments. The name may come from the first positional string literal
/// (`command("collect")`) or the `name=` keyword (`command(name="collect")`),
/// but not both. Returns `(resolved_name, conflict)`: when both are
/// present, `resolved_name` is `None` and `conflict` is `true`.
fn resolve_explicit_name(args: &Arguments) -> (Option<String>, bool) {
    let positional = args.args.first().and_then(literal_str);
    let keyword = keyword_str(args, "name");
    match (positional, keyword) {
        (Some(_), Some(_)) => (None, true),
        (Some(name), None) | (None, Some(name)) => (Some(name), false),
        (None, None) => (None, false),
    }
}

/// Walk a module for `@command`/`@<var>.command` decorators that passed
/// the name both positionally and via `name=`, returning one
/// [`CommandNameConflict`] per offender.
pub fn detect_name_conflicts(
    module: &ModModule,
    module_path: &str,
    imports: &ImportTable,
) -> Vec<CommandNameConflict> {
    let names = ToolrNames(imports);
    let mut out = Vec::new();
    for stmt in &module.body {
        let Stmt::FunctionDef(func) = stmt else {
            continue;
        };
        let Some(target) = command_decorator(&func.decorator_list, names) else {
            continue;
        };
        let conflict = match target {
            CommandDecorator::Binding { name_conflict, .. }
            | CommandDecorator::StringPath { name_conflict, .. } => name_conflict,
        };
        if conflict {
            out.push(CommandNameConflict {
                module: module_path.to_string(),
                function: func.name.as_str().to_string(),
            });
        }
    }
    out
}

/// Which of a module's names are toolr's `command` or the `toolr` module.
#[derive(Clone, Copy)]
struct ToolrNames<'a>(&'a ImportTable);

impl ToolrNames<'_> {
    /// `import toolr [as t]`; `import toolr.types` binds `toolr` too.
    fn is_toolr_module(self, name: &str) -> bool {
        self.0
            .resolve_module_binding(name)
            .is_some_and(|m| m == "toolr" || (name == "toolr" && m.starts_with("toolr.")))
    }

    /// A name `from toolr import command [as …]` or `from toolr import *` binds.
    /// Any other `command` (click's, a local `def`) isn't toolr's.
    fn is_command(self, name: &str) -> bool {
        if self.0.local_binding_wins(name) {
            return false;
        }
        self.0
            .candidates(name)
            .iter()
            .any(|c| c.module == "toolr" && c.original_name == "command")
            || (name == "command" && self.0.star_import_modules().iter().any(|m| m == "toolr"))
    }
}

/// The first decorator toolr reads as a command, and what it read.
fn command_decorator(decorators: &[Decorator], names: ToolrNames) -> Option<CommandDecorator> {
    decorators.iter().find_map(|d| {
        parse_string_path_command(&d.expression, names)
            .or_else(|| parse_binding_command(&d.expression))
    })
}

/// Recognise the group-binding `@<var>.command` shape, as well as the call form
/// `@<var>.command("explicit-name")` which overrides the derived CLI
/// name. Both shapes resolve `<var>` against the binding tables in
/// `extract_commands`.
fn parse_binding_command(expr: &Expr) -> Option<CommandDecorator> {
    // Bare attribute: `@<var>.command`.
    if let Expr::Attribute(attr) = expr {
        if attr.attr.as_str() != "command" {
            return None;
        }
        let Expr::Name(n) = attr.value.as_ref() else {
            return None;
        };
        return Some(CommandDecorator::Binding {
            var: n.id.as_str().to_string(),
            explicit_name: None,
            name_conflict: false,
        });
    }
    // Call form: `@<var>.command(...)` — func is an Attribute whose
    // value is a Name. The first positional, if a string literal, is
    // the CLI-name override.
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Attribute(attr) = call.func.as_ref() else {
        return None;
    };
    if attr.attr.as_str() != "command" {
        return None;
    }
    let Expr::Name(n) = attr.value.as_ref() else {
        return None;
    };
    // The CLI-name override may be the first positional string literal
    // (`@<var>.command("name")`) or the `name=` keyword
    // (`@<var>.command(name="name")`); the bound `command(self, name)`
    // method accepts both shapes at runtime, but not both at once.
    let (explicit_name, name_conflict) = resolve_explicit_name(&call.arguments);
    Some(CommandDecorator::Binding {
        var: n.id.as_str().to_string(),
        explicit_name,
        name_conflict,
    })
}

/// Local copy of the `literal_str` helper used elsewhere in the parser.
/// Keeps this module self-contained without exposing a parser-wide
/// helper as public API.
fn literal_str(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => None,
    }
}

/// Find a string-literal keyword argument by name, e.g. `name="…"`.
fn keyword_str(args: &Arguments, name: &str) -> Option<String> {
    args.keywords
        .iter()
        .find(|k| k.arg.as_ref().map(ruff_python_ast::Identifier::as_str) == Some(name))
        .and_then(|k| literal_str(&k.value))
}

/// Recognise the string-path `@command` / `@command("name", group="…")` shape,
/// also spelt `@toolr.command`, `@t.command` or an alias of `command`.
fn parse_string_path_command(expr: &Expr, names: ToolrNames) -> Option<CommandDecorator> {
    let is_toolr_command = |callee: &Expr| match callee {
        Expr::Name(n) => names.is_command(n.id.as_str()),
        Expr::Attribute(attr) => {
            attr.attr.as_str() == "command"
                && matches!(attr.value.as_ref(), Expr::Name(n) if names.is_toolr_module(n.id.as_str()))
        }
        _ => false,
    };
    // Bare `@command` / `@toolr.command`.
    if is_toolr_command(expr) {
        return Some(CommandDecorator::StringPath {
            explicit_name: None,
            group: None,
            name_conflict: false,
        });
    }
    let Expr::Call(call) = expr else {
        return None;
    };
    if !is_toolr_command(&call.func) {
        return None;
    }
    // The explicit name may be the first positional string literal
    // (`@command("name", group=…)`) or the `name=` keyword
    // (`@command(name="name", group=…)`), but not both.
    let (explicit_name, name_conflict) = resolve_explicit_name(&call.arguments);
    // `group=` kwarg, string literal only.
    let group = keyword_str(&call.arguments, "group");
    Some(CommandDecorator::StringPath {
        explicit_name,
        group,
        name_conflict,
    })
}

/// Extract the raw docstring of a function (the leading string-literal
/// statement in its body), or an empty string if it has none.
fn function_docstring(func: &StmtFunctionDef) -> String {
    let Some(first) = func.body.first() else {
        return String::new();
    };
    let Stmt::Expr(e) = first else {
        return String::new();
    };
    let Expr::StringLiteral(s) = e.value.as_ref() else {
        return String::new();
    };
    s.value.to_str().to_string()
}

#[allow(clippy::too_many_arguments)] // contextual parameters; bundling would obscure call sites.
fn build_command(
    func: &StmtFunctionDef,
    group: &str,
    override_name: Option<&str>,
    module_path: &str,
    enums: &EnumTable,
    all_imports: &HashMap<String, ImportTable>,
    consts: &ConstTable,
    type_imports: &TypeImports,
    sources: &SourcesImports,
    aliases: &TypeAliasTable,
    sections: &ArgSectionTable,
    errors: &mut Vec<TypeResolutionError>,
) -> Command {
    let raw_doc = function_docstring(func);
    let parsed = parse_docstring(&raw_doc);
    let summary = parsed
        .as_ref()
        .map(|d| d.short_description.clone())
        .unwrap_or_default();
    // Hand clap a multi-section render (short + long + Examples/Notes/…)
    // for `--help` so users see more than the first paragraph there.
    // The `summary` field separately drives `-h`'s shorter `about` slot.
    let description = parsed
        .as_ref()
        .map(|d| d.full_description())
        .unwrap_or_default();
    let mut arguments = extract_arguments(func, enums, all_imports, consts, sources, module_path);
    if let Some(d) = parsed.as_ref() {
        for arg in &mut arguments {
            if let Some(Some(help)) = d.params.get(&arg.name) {
                arg.help = help.clone();
            }
        }
    }
    resolve_arguments(
        func,
        &mut arguments,
        enums,
        all_imports,
        type_imports,
        sources,
        aliases,
        sections,
        module_path,
        errors,
    );
    // Backfill `allowed_values` from `resolved_type` for arguments whose
    // type annotation isn't a plain `Expr::Name` — `extract_arguments`
    // runs before `resolve_arguments` and only consults `EnumTable` /
    // `literal_values` on a bare name, so these land here with
    // `allowed_values: []` even though the resolver went on to fill
    // `resolved_type` with the answer:
    //   - a `Name("Mode")` where `Mode = Literal["fast", "slow"]` lives
    //     in another module (a type alias, not a bare Literal/Enum).
    //   - `Environment | None` / `Optional[Environment]` — `BinOp` and
    //     `Subscript` annotations, which `referenced_name` doesn't match
    //     at all, so the enum/literal underneath never gets a look-in.
    // Peel one `Optional` layer (the CLI-visible choices are the same
    // whether or not the flag is required) and copy the inner
    // `Literal`/`Enum` values across.
    for arg in &mut arguments {
        if arg.allowed_values.is_empty() {
            let inner = match &arg.resolved_type {
                Some(crate::parser::types::SupportedType::Optional(inner)) => inner.as_ref(),
                Some(other) => other,
                None => continue,
            };
            match inner {
                crate::parser::types::SupportedType::Literal(values) => {
                    arg.allowed_values = values.clone();
                }
                crate::parser::types::SupportedType::Enum { values, .. } => {
                    arg.allowed_values = values.clone();
                }
                _ => {}
            }
        }
    }
    let cli_name = override_name
        .map(str::to_string)
        .unwrap_or_else(|| func.name.as_str().replace('_', "-"));
    Command {
        name: cli_name,
        group: group.to_string(),
        module: module_path.to_string(),
        function: func.name.as_str().to_string(),
        summary,
        description,
        arguments,
        origin: Origin::Static,
        dispatched_from: None,
        is_dispatcher: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::groups::extract_groups;
    use crate::parser::parse_python_file;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn parse_src(src: &str) -> ModModule {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        parse_python_file(f.path()).unwrap()
    }

    /// `all_imports` for a module parsed as `tools.ci`, so `from toolr import command` counts.
    fn imports_of(m: &ModModule) -> HashMap<String, ImportTable> {
        ["tools.ci", "tools.repro", "tools.x"]
            .into_iter()
            .map(|path| (path.to_string(), ImportTable::from_module(m, path, false)))
            .collect()
    }

    #[test]
    fn extracts_decorated_function_as_command() {
        let src = r#"group = command_group("ci", "CI utilities")

@group.command
def generate_build_matrix(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "generate-build-matrix");
        assert_eq!(commands[0].group, "ci");
        assert_eq!(commands[0].function, "generate_build_matrix");
        assert_eq!(commands[0].module, "tools.ci");
    }

    #[test]
    fn ignores_functions_with_unknown_group_var() {
        let src = r#"
@other.command
def x(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = vec![];
        let commands = extract_commands(
            &m,
            "tools.x",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(commands.is_empty());
    }

    #[test]
    fn ignores_undecorated_functions() {
        let src = r#"group = command_group("ci", "CI utilities")

def bare_function(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(commands.is_empty());
    }

    #[test]
    fn populates_summary_from_first_docstring_line() {
        let src = r#"group = command_group("ci", "CI utilities")

@group.command
def hello(ctx):
    """Say hello."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert_eq!(commands[0].summary, "Say hello.");
    }

    #[test]
    fn string_path_decorator_attaches_to_named_group() {
        let src = r#"from toolr import command
command_group("ci", "CI utilities")

@command(group="ci")
def check_run_build(ctx):
    """Check the run."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "check-run-build");
        assert_eq!(commands[0].group, "ci");
    }

    #[test]
    fn string_path_decorator_supports_explicit_name() {
        let src = r#"from toolr import command
command_group("ci.helm-diff-pr-comment", "Helm diff")

@command("snippet-checker", group="ci.helm-diff-pr-comment")
def check_snippets(ctx):
    """Check snippets."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "snippet-checker");
        assert_eq!(commands[0].group, "ci.helm-diff-pr-comment");
        assert_eq!(commands[0].function, "check_snippets");
    }

    #[test]
    fn string_path_decorator_supports_name_keyword() {
        let src = r#"from toolr import command
command_group("ci.helm-diff-pr-comment", "Helm diff")

@command(name="snippet-checker", group="ci.helm-diff-pr-comment")
def check_snippets(ctx):
    """Check snippets."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert!(
            detect_name_conflicts(
                &m,
                "tools.ci",
                &ImportTable::from_module(&m, "tools.ci", false)
            )
            .is_empty()
        );
        assert_eq!(commands.len(), 1);
        // The `name=` keyword wins over the `check-snippets` function name.
        assert_eq!(commands[0].name, "snippet-checker");
        assert_eq!(commands[0].group, "ci.helm-diff-pr-comment");
        assert_eq!(commands[0].function, "check_snippets");
    }

    #[test]
    fn optional_list_keyword_resolves_to_repeated() {
        // Regression for #435: `list[T] | None` was classified as
        // `Optional` (single-value) by the syntactic pass, since it
        // can't see through the `| None` union — only the post-resolution
        // override in `resolve_arguments` catches it.
        let src = r#"from toolr import command
command_group("repro", "Repro")

@command(group="repro")
def go(ctx, items: list[str] | None = None):
    """Repro."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.repro",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert_eq!(commands.len(), 1);
        let items = &commands[0].arguments[0];
        assert_eq!(items.name, "items");
        assert_eq!(items.kind, crate::manifest::ArgumentKind::Repeated);
        assert_eq!(
            items.resolved_type,
            Some(crate::parser::types::SupportedType::Optional(Box::new(
                crate::parser::types::SupportedType::List(Box::new(
                    crate::parser::types::SupportedType::Str
                ))
            )))
        );
    }

    #[test]
    fn positional_only_params_resolve_in_lockstep() {
        let src = r#"from toolr import command
command_group("repro", "Repro")

@command(group="repro")
def go(ctx, name: str, /, count: int, *rest: int):
    """Repro."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.repro",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        use crate::manifest::ArgumentKind;
        use crate::parser::types::SupportedType;
        let got: Vec<_> = commands[0]
            .arguments
            .iter()
            .map(|a| (a.name.as_str(), a.kind, a.resolved_type.clone()))
            .collect();
        assert_eq!(
            got,
            [
                ("name", ArgumentKind::Positional, Some(SupportedType::Str)),
                ("count", ArgumentKind::Positional, Some(SupportedType::Int)),
                (
                    "rest",
                    ArgumentKind::VarPositional,
                    Some(SupportedType::Int)
                ),
            ]
        );
    }

    #[test]
    fn optional_tuple_keyword_resolves_to_repeated() {
        // Same fix as `optional_list_keyword_resolves_to_repeated`, for
        // the other collection shape `is_list_like_annotation` accepts.
        let src = r#"from toolr import command
command_group("repro", "Repro")

@command(group="repro")
def go(ctx, pair: tuple[str, int] | None = None):
    """Repro."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.repro",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert_eq!(commands.len(), 1);
        let pair = &commands[0].arguments[0];
        assert_eq!(pair.name, "pair");
        assert_eq!(pair.kind, crate::manifest::ArgumentKind::Repeated);
        assert_eq!(
            pair.resolved_type,
            Some(crate::parser::types::SupportedType::Optional(Box::new(
                crate::parser::types::SupportedType::Tuple(vec![
                    crate::parser::types::SupportedType::Str,
                    crate::parser::types::SupportedType::Int,
                ])
            )))
        );
    }

    #[test]
    fn binding_decorator_rejects_positional_and_name_keyword() {
        let src = r#"group = command_group("ci", "CI")

@group.command("positional", name="keyword")
def do_thing(ctx):
    pass
"#;
        let m = parse_src(src);
        let func = m
            .body
            .iter()
            .find_map(|s| match s {
                Stmt::FunctionDef(f) => Some(f),
                _ => None,
            })
            .unwrap();
        // The decorator resolves to no explicit name, flagged as a conflict.
        assert_eq!(
            command_decorator(
                &func.decorator_list,
                ToolrNames(&ImportTable::from_module(&m, "tools.ci", false))
            )
            .unwrap(),
            CommandDecorator::Binding {
                var: "group".to_string(),
                explicit_name: None,
                name_conflict: true,
            }
        );
        let conflicts = detect_name_conflicts(
            &m,
            "tools.ci",
            &ImportTable::from_module(&m, "tools.ci", false),
        );
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].module, "tools.ci");
        assert_eq!(conflicts[0].function, "do_thing");
    }

    #[test]
    fn string_path_decorator_rejects_positional_and_name_keyword() {
        let src = r#"from toolr import command
command_group("ci", "CI")

@command("positional", name="keyword", group="ci")
def do_thing(ctx):
    pass
"#;
        let m = parse_src(src);
        let func = m
            .body
            .iter()
            .find_map(|s| match s {
                Stmt::FunctionDef(f) => Some(f),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            command_decorator(
                &func.decorator_list,
                ToolrNames(&ImportTable::from_module(&m, "tools.ci", false))
            )
            .unwrap(),
            CommandDecorator::StringPath {
                explicit_name: None,
                group: Some("ci".to_string()),
                name_conflict: true,
            }
        );
        let conflicts = detect_name_conflicts(
            &m,
            "tools.ci",
            &ImportTable::from_module(&m, "tools.ci", false),
        );
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].function, "do_thing");
    }

    #[test]
    fn binding_decorator_works() {
        let src = r#"group = command_group("ci", "CI")

@group.command
def hello(ctx):
    """Hi."""
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].group, "ci");
    }

    #[test]
    fn binding_decorator_attribute_form_no_explicit_name() {
        let src = r#"group = command_group("ci", "CI")

@group.command
def do_thing(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        // Probe the decorator parser directly to assert variant shape.
        let func = m
            .body
            .iter()
            .find_map(|s| match s {
                Stmt::FunctionDef(f) => Some(f),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            command_decorator(
                &func.decorator_list,
                ToolrNames(&ImportTable::from_module(&m, "tools.ci", false))
            )
            .unwrap(),
            CommandDecorator::Binding {
                var: "group".to_string(),
                explicit_name: None,
                name_conflict: false,
            }
        );
        // And confirm the function's own name is used (kebab-cased).
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "do-thing");
    }

    #[test]
    fn binding_decorator_call_form_with_explicit_name() {
        let src = r#"group = command_group("ci", "CI")

@group.command("hello")
def do_thing(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let func = m
            .body
            .iter()
            .find_map(|s| match s {
                Stmt::FunctionDef(f) => Some(f),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            command_decorator(
                &func.decorator_list,
                ToolrNames(&ImportTable::from_module(&m, "tools.ci", false))
            )
            .unwrap(),
            CommandDecorator::Binding {
                var: "group".to_string(),
                explicit_name: Some("hello".to_string()),
                name_conflict: false,
            }
        );
        // CLI name comes from the decorator argument, not the function.
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "hello");
        assert_eq!(commands[0].function, "do_thing");
    }

    #[test]
    fn binding_decorator_call_form_with_name_keyword() {
        let src = r#"group = command_group("ci", "CI")

@group.command(name="collect")
def collect_data(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let func = m
            .body
            .iter()
            .find_map(|s| match s {
                Stmt::FunctionDef(f) => Some(f),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            command_decorator(
                &func.decorator_list,
                ToolrNames(&ImportTable::from_module(&m, "tools.ci", false))
            )
            .unwrap(),
            CommandDecorator::Binding {
                var: "group".to_string(),
                explicit_name: Some("collect".to_string()),
                name_conflict: false,
            }
        );
        // The `name=` keyword wins over the hyphenated function name
        // (`collect`, not the `collect-data` the function name would give).
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "collect");
        assert_eq!(commands[0].function, "collect_data");
    }

    #[test]
    fn binding_decorator_call_form_without_name_arg() {
        let src = r#"group = command_group("ci", "CI")

@group.command()
def do_thing(ctx):
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let func = m
            .body
            .iter()
            .find_map(|s| match s {
                Stmt::FunctionDef(f) => Some(f),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            command_decorator(
                &func.decorator_list,
                ToolrNames(&ImportTable::from_module(&m, "tools.ci", false))
            )
            .unwrap(),
            CommandDecorator::Binding {
                var: "group".to_string(),
                explicit_name: None,
                name_conflict: false,
            }
        );
        // Falls back to function name (kebab-cased) when no override.
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "do-thing");
    }

    #[test]
    fn populates_arg_help_from_args_section() {
        let src = r#"group = command_group("ci", "CI utilities")

@group.command
def hello(ctx, name="world"):
    """Say hello.

    Args:
        name: Who to greet.
    """
    pass
"#;
        let m = parse_src(src);
        let bindings = extract_groups(&m, "", &HashMap::new());
        let commands = extract_commands(
            &m,
            "tools.ci",
            &bindings,
            &EnumTable::default(),
            &imports_of(&m),
            &ConstTable::default(),
            &TypeImports::default(),
            &SourcesImports::default(),
            &TypeAliasTable::default(),
            &ArgSectionTable::default(),
            &HashMap::new(),
            &mut Vec::new(),
        );
        let name_arg = commands[0]
            .arguments
            .iter()
            .find(|a| a.name == "name")
            .unwrap();
        assert_eq!(name_arg.help, "Who to greet.");
    }
}
