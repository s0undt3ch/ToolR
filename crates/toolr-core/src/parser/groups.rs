//! Extract `group = command_group(...)` assignments from a module AST.

use std::collections::HashMap;

use ruff_python_ast::visitor::{Visitor, walk_except_handler, walk_expr, walk_stmt};
use ruff_python_ast::{
    Alias, ExceptHandler, Expr, ExprCall, ModModule, Stmt, StmtAnnAssign, StmtAssign,
};

use super::commands::{Binding, ToolrNames, UnsupportedCommand, UnsupportedCommandKind};
use super::parse_docstring;

/// A group binding extracted from source. The `var` is the Python local name
/// that subsequent `@var.command` decorators reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupBinding {
    pub var: String,
    /// The group's leaf name.
    pub name: String,
    /// The parent group's dotted path, `None` at the top level.
    pub parent: Option<String>,
    /// The group's fields as this declaration gives them.
    pub fields: GroupFields,
    /// Whether the call passes a non-empty description or docstring, readable
    /// or not. A group no declaration describes fails the build.
    pub describes: bool,
    /// Whether the call surely passes a docstring together with a description
    /// or long description, which Python's `command_group` refuses.
    pub contradictory: bool,
    /// Whether the call may pass a description or docstring the static parser
    /// can't see: through `*`/`**` unpacking, or as the `__doc__` of a module
    /// without a docstring.
    pub unreadable: bool,
}

impl GroupBinding {
    /// The group's dotted path, built like `Group::full_path`.
    pub fn full_path(&self) -> String {
        match &self.parent {
            Some(parent) => format!("{parent}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// One `command_group` field as the static parser reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Field {
    /// Not passed, `None`, or empty: no opinion.
    #[default]
    Empty,
    /// A string literal, or the module docstring through `__doc__`.
    Text(String),
    /// Passed as any other expression, such as a name or a call. It has an
    /// opinion, but not one the build can read or compare.
    Opaque,
}

impl Field {
    fn from_text(text: String) -> Self {
        if text.is_empty() {
            Field::Empty
        } else {
            Field::Text(text)
        }
    }

    /// The readable text, or `""`.
    pub fn text(&self) -> &str {
        match self {
            Field::Text(text) => text,
            _ => "",
        }
    }
}

/// The fields one declaration gives a group, or, with another `T`, what the
/// build keeps of each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupFields<T = Field> {
    pub title: T,
    pub description: T,
    pub long_description: T,
}

impl<T> GroupFields<T> {
    /// Each field with its `command_group` keyword, in the order Python's
    /// `command_group` compares them.
    pub fn named(&self) -> [(&'static str, &T); 3] {
        [
            (TITLE.keyword, &self.title),
            (DESCRIPTION.keyword, &self.description),
            (LONG_DESCRIPTION.keyword, &self.long_description),
        ]
    }

    /// [`Self::named`], mutably.
    pub fn named_mut(&mut self) -> [(&'static str, &mut T); 3] {
        [
            (TITLE.keyword, &mut self.title),
            (DESCRIPTION.keyword, &mut self.description),
            (LONG_DESCRIPTION.keyword, &mut self.long_description),
        ]
    }

    /// The fields with `f` applied to each.
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> GroupFields<U> {
        GroupFields {
            title: f(self.title),
            description: f(self.description),
            long_description: f(self.long_description),
        }
    }
}

/// A `command_group` parameter: its keyword, and its position in
/// `command_group(name, title, description, long_description, docstring,
/// parent)`, the signature of `command_group` in
/// `crates/toolr-py/python/toolr/_decorators.py`.
///
/// `toolr.testing._groups._Param` mirrors these in Python; change both
/// together.
#[derive(Debug, Clone, Copy)]
struct Param {
    keyword: &'static str,
    position: usize,
}

const TITLE: Param = Param {
    keyword: "title",
    position: 1,
};
const DESCRIPTION: Param = Param {
    keyword: "description",
    position: 2,
};
const LONG_DESCRIPTION: Param = Param {
    keyword: "long_description",
    position: 3,
};
const DOCSTRING: Param = Param {
    keyword: "docstring",
    position: 4,
};
const PARENT: Param = Param {
    keyword: "parent",
    position: 5,
};

/// Walk the module's top-level statements and collect group bindings.
///
/// `module_docstring` is `None` when the module has no docstring, as
/// `__doc__` is then `None` at runtime.
///
/// Nested groups (`child = parent.command_group("name", ...)`) get their
/// `parent` field set to the parent binding's `full_path()`, so the
/// CLI builder can reconstruct the hierarchy. Source order matters:
/// parents must be assigned before children (which Python already
/// requires).
pub fn extract_groups(
    module: &ModModule,
    module_docstring: Option<&str>,
    global_vars: &HashMap<String, String>,
    names: &ToolrNames,
    module_path: &str,
) -> Vec<GroupBinding> {
    let source = Source::new(module, module_docstring);
    let mut out: Vec<GroupBinding> = Vec::new();
    for stmt in &module.body {
        if let Some(binding) = group_binding(stmt, &source, &out, global_vars, names, module_path) {
            out.push(binding);
        }
    }
    out
}

/// Whether the module binds `__doc__` outside a function or class body, so
/// `__doc__` may not be its docstring when a call reads it.
///
/// `toolr.testing._groups._binds_doc` mirrors this in Python; change both
/// together.
fn binds_doc(module: &ModModule) -> bool {
    let mut finder = DocBinding(false);
    finder.visit_body(&module.body);
    finder.0
}

struct DocBinding(bool);

impl<'a> Visitor<'a> for DocBinding {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if !matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
            walk_stmt(self, stmt);
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Name(n) => self.0 |= n.id.as_str() == "__doc__" && !n.ctx.is_load(),
            _ => walk_expr(self, expr),
        }
    }

    fn visit_alias(&mut self, alias: &'a Alias) {
        let bound = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
        self.0 |= bound.split('.').next() == Some("__doc__");
    }

    fn visit_except_handler(&mut self, handler: &'a ExceptHandler) {
        let ExceptHandler::ExceptHandler(h) = handler;
        self.0 |= h.name.as_ref().is_some_and(|n| n.as_str() == "__doc__");
        walk_except_handler(self, handler);
    }
}

/// What `__doc__` is when a `command_group` call reads it.
///
/// `toolr.testing._groups._DocState` mirrors this in Python; change both
/// together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocState<'a> {
    /// The module binds `__doc__`, so it may not be the docstring.
    Rebound,
    /// The module has no docstring: `__doc__` is `None`.
    Missing,
    /// The module docstring is empty or whitespace-only.
    Blank,
    /// The module docstring.
    Text(&'a str),
}

/// Whether an argument is something other than `None` at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Passes {
    Never,
    /// Unless an expression the parser can't read is `None`.
    Maybe,
    Surely,
}

/// What a `command_group` call's arguments can refer to.
///
/// `toolr.testing._groups._Source` mirrors this in Python; change both
/// together.
struct Source<'a> {
    doc: DocState<'a>,
}

impl<'a> Source<'a> {
    fn new(module: &ModModule, module_docstring: Option<&'a str>) -> Self {
        let doc = match module_docstring {
            _ if binds_doc(module) => DocState::Rebound,
            None => DocState::Missing,
            Some(doc) if doc.trim().is_empty() => DocState::Blank,
            Some(doc) => DocState::Text(doc),
        };
        Source { doc }
    }

    /// Read one argument. A `__doc__` reference is the module docstring, and
    /// one that is empty or whitespace-only has no opinion. Any expression
    /// but a string literal, `None` or `__doc__` is opaque.
    ///
    /// `toolr.testing._groups._read` mirrors this in Python; change both
    /// together.
    fn read(&self, expr: Option<&Expr>) -> Field {
        match expr {
            None | Some(Expr::NoneLiteral(_)) => Field::Empty,
            Some(e) if is_doc(e) => match self.doc {
                DocState::Rebound => Field::Opaque,
                DocState::Missing | DocState::Blank => Field::Empty,
                DocState::Text(doc) => Field::Text(doc.to_string()),
            },
            Some(e) => literal_str(e).map_or(Field::Opaque, Field::from_text),
        }
    }

    /// Whether `expr` is an argument other than `None` at runtime. A
    /// literal surely is, and the `__doc__` of a module with a docstring.
    ///
    /// `toolr.testing._groups._passes` mirrors this in Python; change both
    /// together.
    fn passes(&self, expr: Option<&Expr>) -> Passes {
        match expr {
            None | Some(Expr::NoneLiteral(_)) => Passes::Never,
            Some(e) if is_doc(e) => match self.doc {
                DocState::Missing => Passes::Never,
                DocState::Rebound => Passes::Maybe,
                DocState::Blank | DocState::Text(_) => Passes::Surely,
            },
            Some(e) if e.is_literal_expr() => Passes::Surely,
            Some(_) => Passes::Maybe,
        }
    }

    /// Whether `expr` is the `__doc__` of a module whose docstring is
    /// missing or blank.
    fn is_blank_doc(&self, expr: Option<&Expr>) -> bool {
        expr.is_some_and(is_doc) && matches!(self.doc, DocState::Missing | DocState::Blank)
    }
}

fn is_doc(expr: &Expr) -> bool {
    matches!(expr, Expr::Name(n) if n.id.as_str() == "__doc__")
}

/// The call a module-level statement binds, and the name it binds it to, when
/// the statement has a shape the parser reads: `var = call`, `var: T = call`
/// or a bare `call`. Not yet known to be `command_group`.
///
/// Mirrored in `toolr.testing._groups._supported_group_call`; change both together.
fn supported_group_call(stmt: &Stmt) -> Option<(Option<String>, &ExprCall)> {
    match stmt {
        Stmt::Assign(StmtAssign { targets, value, .. }) => {
            let name = single_name_target(targets)?;
            let Expr::Call(call) = value.as_ref() else {
                return None;
            };
            Some((Some(name), call))
        }
        Stmt::AnnAssign(StmtAnnAssign {
            target,
            value: Some(value),
            ..
        }) => {
            let (Expr::Name(name), Expr::Call(call)) = (target.as_ref(), value.as_ref()) else {
                return None;
            };
            Some((Some(name.id.as_str().to_string()), call))
        }
        Stmt::Expr(expr_stmt) => {
            let Expr::Call(call) = expr_stmt.value.as_ref() else {
                return None;
            };
            Some((None, call))
        }
        _ => None,
    }
}

/// Every `command_group(...)` call the parser can't bind, one entry each, in
/// source order: anything but the whole value of a supported module-level
/// statement. Such a group, and every command on it, would otherwise vanish
/// without a word.
///
/// Mirrored in `toolr.testing._shapes._unsupported_group_bindings`; change
/// both together.
pub fn detect_unsupported_group_bindings(
    module: &ModModule,
    module_path: &str,
    names: &ToolrNames,
) -> Vec<UnsupportedCommand> {
    let mut finder = UnboundGroupCalls {
        names,
        module_path,
        out: Vec::new(),
    };
    for stmt in &module.body {
        let bound = supported_group_call(stmt)
            .is_some_and(|(_, call)| is_command_group_call(call, names, module_path));
        if !bound {
            finder.visit_stmt(stmt);
        }
    }
    finder.out
}

struct UnboundGroupCalls<'a> {
    names: &'a ToolrNames<'a>,
    module_path: &'a str,
    out: Vec<UnsupportedCommand>,
}

impl<'a> Visitor<'a> for UnboundGroupCalls<'_> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr
            && is_command_group_call(call, self.names, self.module_path)
        {
            self.out.push(UnsupportedCommand {
                module: self.module_path.to_string(),
                function: "command_group".to_string(),
                kind: UnsupportedCommandKind::GroupBinding,
            });
        }
        walk_expr(self, expr);
    }
}

/// The binding `stmt` makes, when it is a `command_group(...)` call.
fn group_binding(
    stmt: &Stmt,
    source: &Source<'_>,
    local: &[GroupBinding],
    global_vars: &HashMap<String, String>,
    names: &ToolrNames,
    module_path: &str,
) -> Option<GroupBinding> {
    // Two recognised shapes:
    //   1. `var = command_group(...)` or `var: T = command_group(...)` —
    //      group-binding style.
    //   2. `command_group("path", ...)` — bare expression-statement,
    //      the string-path style. No binding var to capture
    //      (the leaf name becomes a synthetic var so `@var.command`
    //      keeps working when a user mixes styles).
    let (var_name, call) = supported_group_call(stmt)?;
    if !is_command_group_call(call, names, module_path) {
        return None;
    }
    // Parent path can come from three source patterns:
    //   1. Method-call style: `child = parent.command_group(...)` —
    //      look up `parent` locally first, then in the global
    //      cross-file binding map.
    //   2. Keyword-arg with variable reference:
    //      `child = command_group(..., parent=parent_var)`.
    //   3. Keyword-arg with literal string:
    //      `child = command_group(..., parent="ci")`.
    //
    // Plus the dotted-name shape — `command_group("ci.helm-diff-pr-comment", ...)`
    // — splits the leaf off inside `parse_group_call` and overrides
    // whatever parent_path we resolve here.
    let parent_path = parent_var_name(call)
        .or_else(|| parent_kwarg_var(call))
        .and_then(|pv| resolve_var(&pv, local, global_vars))
        .or_else(|| parent_kwarg_literal(call));
    let chosen_var = var_name.clone().unwrap_or_default();
    let mut binding = parse_group_call(call, &chosen_var, source, parent_path)?;
    // For bare-form (no explicit var), use the leaf name as the
    // synthetic var so `@<leaf>.command` decorators still
    // resolve when authors mix the two styles. Skip when
    // there's a real binding name.
    if var_name.is_none() {
        binding.var = binding.name.clone();
    }
    Some(binding)
}

/// Resolve a variable name to its group's full_path. Checks
/// already-extracted local bindings first, then the cumulative global
/// map of bindings seen across previously-processed files.
fn resolve_var(
    var: &str,
    local: &[GroupBinding],
    global_vars: &HashMap<String, String>,
) -> Option<String> {
    local
        .iter()
        .find(|b| b.var == var)
        .map(GroupBinding::full_path)
        .or_else(|| global_vars.get(var).cloned())
}

/// If the call is a method invocation like `docker.command_group(...)`,
/// return the head variable name (`"docker"`). For free-function
/// `command_group(...)` returns `None`.
fn parent_var_name(call: &ExprCall) -> Option<String> {
    let Expr::Attribute(attr) = call.func.as_ref() else {
        return None;
    };
    match attr.value.as_ref() {
        Expr::Name(n) => Some(n.id.as_str().to_string()),
        _ => None,
    }
}

/// If the call passes `parent="..."` (literal string), by keyword or
/// position, return that string.
fn parent_kwarg_literal(call: &ExprCall) -> Option<String> {
    argument(call, PARENT).and_then(literal_str)
}

/// If the call passes `parent=<var>` (variable reference), by keyword or
/// position, return the variable name so the caller can resolve it
/// against the local + global binding tables.
fn parent_kwarg_var(call: &ExprCall) -> Option<String> {
    match argument(call, PARENT)? {
        Expr::Name(n) => Some(n.id.as_str().to_string()),
        _ => None,
    }
}

fn single_name_target(targets: &[Expr]) -> Option<String> {
    if targets.len() != 1 {
        return None;
    }
    match &targets[0] {
        Expr::Name(n) => Some(n.id.as_str().to_string()),
        _ => None,
    }
}

/// Whether `call` is toolr's `command_group`: spelt so, an alias the import
/// tables resolve to it, or any `<x>.command_group(...)`.
///
/// `toolr.testing._groups._is_command_group` mirrors this in Python, resolving
/// aliases from source with `_binding`; change both together.
fn is_command_group_call(call: &ExprCall, names: &ToolrNames, module: &str) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => {
            n.id.as_str() == "command_group"
                || names.command_group(module, n.id.as_str()) == Binding::Toolr
        }
        Expr::Attribute(a) => a.attr.as_str() == "command_group",
        _ => false,
    }
}

fn parse_group_call(
    call: &ExprCall,
    var: &str,
    source: &Source<'_>,
    parent: Option<String>,
) -> Option<GroupBinding> {
    let raw_name = call.arguments.args.first().and_then(literal_str)?;
    // Dotted name form (`command_group("ci.helm-diff-pr-comment", ...)`):
    // split the leaf off the parent path. Explicit `parent=` (from the
    // earlier resolution path) takes a back seat — pick one style.
    let (name, parent) = if let Some(idx) = raw_name.rfind('.') {
        let dotted_parent = raw_name[..idx].to_string();
        let leaf = raw_name[idx + 1..].to_string();
        (leaf, Some(dotted_parent))
    } else {
        (raw_name, parent)
    };
    let docstring = source.read(argument(call, DOCSTRING));
    // When `docstring=` is supplied, split it the same way `parser/commands.rs`
    // splits function docstrings: the first paragraph becomes the short
    // `title` (clap's `about`, shown in the parent's command listing) and
    // the full rendered docstring (short + long + Examples/Notes/…) becomes
    // the long `description` (clap's `long_about`, shown on `--help`).
    // An explicit title or description still wins, so callers can override
    // one side without losing the other.
    let parsed_doc = match &docstring {
        Field::Text(raw) => parse_docstring(raw),
        _ => None,
    };
    let from_docstring = |part: fn(&crate::Docstring) -> String| match docstring {
        Field::Opaque => Field::Opaque,
        _ => parsed_doc
            .as_ref()
            .map_or(Field::Empty, |d| Field::from_text(part(d))),
    };
    let title = match source.read(argument(call, TITLE)) {
        Field::Empty => from_docstring(|d| d.short_description.clone()),
        title => title,
    };
    let description = match source.read(argument(call, DESCRIPTION)) {
        Field::Empty => from_docstring(crate::Docstring::full_description),
        description => description,
    };
    // `command_group` drops the long description when a docstring is given,
    // and a docstring that may be `None` leaves it unknown.
    let docstring_passes = source.passes(argument(call, DOCSTRING));
    let long_description = match source.read(argument(call, LONG_DESCRIPTION)) {
        _ if docstring_passes == Passes::Surely => Field::Empty,
        Field::Text(_) if docstring_passes == Passes::Maybe => Field::Opaque,
        long => long,
    };
    Some(GroupBinding {
        var: var.to_string(),
        name,
        parent,
        fields: GroupFields {
            title,
            description,
            long_description,
        },
        describes: describes(call, source),
        contradictory: contradictory(call, source),
        unreadable: unreadable(call, source),
    })
}

/// Whether the call passes a non-empty description or docstring, readable or
/// not. An empty description, or an empty or whitespace-only docstring,
/// leaves the group undescribed.
///
/// `toolr.testing._groups._describes` mirrors this in Python; change both
/// together.
fn describes(call: &ExprCall, source: &Source<'_>) -> bool {
    source.read(argument(call, DESCRIPTION)) != Field::Empty
        || match source.read(argument(call, DOCSTRING)) {
            Field::Text(raw) => !raw.trim().is_empty(),
            Field::Opaque => true,
            Field::Empty => false,
        }
}

/// Whether the call surely passes a docstring with a description or long
/// description. Only values surely passed contradict: one the parser can't
/// read may be `None` at runtime.
///
/// `toolr.testing._groups._contradictory` mirrors this in Python; change both
/// together.
fn contradictory(call: &ExprCall, source: &Source<'_>) -> bool {
    let surely = |param| source.passes(argument(call, param)) == Passes::Surely;
    surely(DOCSTRING) && (surely(DESCRIPTION) || surely(LONG_DESCRIPTION))
}

/// Whether the call may pass a description or docstring the parser can't
/// see: through `*`/`**` unpacking, or as the `__doc__` of a module without
/// a docstring.
///
/// `toolr.testing._groups._unreadable` mirrors this in Python; change both
/// together.
fn unreadable(call: &ExprCall, source: &Source<'_>) -> bool {
    let unpacks = call.arguments.args.iter().any(Expr::is_starred_expr)
        || call.arguments.keywords.iter().any(|k| k.arg.is_none());
    unpacks
        || source.is_blank_doc(argument(call, DESCRIPTION))
        || source.is_blank_doc(argument(call, DOCSTRING))
}

/// The argument `command_group` binds to `param`, passed by keyword or by
/// position.
///
/// `toolr.testing._groups._argument` mirrors this in Python; change both
/// together.
fn argument(call: &ExprCall, param: Param) -> Option<&Expr> {
    call.arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().map(|n| n.as_str()) == Some(param.keyword))
        .map(|k| &k.value)
        .or_else(|| call.arguments.args.get(param.position))
}

fn literal_str(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_python_file;
    use crate::parser::symbols::ImportTable;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn parse_src(src: &str) -> ModModule {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        parse_python_file(f.path()).unwrap()
    }

    /// Import tables for `(module path, source)` pairs, all in `tools/`.
    fn imports(modules: &[(&str, &str)]) -> HashMap<String, ImportTable> {
        modules
            .iter()
            .map(|(path, src)| {
                let m = parse_src(src);
                (path.to_string(), ImportTable::from_module(&m, path, false))
            })
            .collect()
    }

    /// `extract_groups` on `src` parsed as `tools.m`, alongside `others`.
    fn groups_in(src: &str, others: &[(&str, &str)]) -> Vec<GroupBinding> {
        let mut modules = vec![("tools.m", src)];
        modules.extend_from_slice(others);
        let all = imports(&modules);
        extract_groups(
            &parse_src(src),
            None,
            &HashMap::new(),
            &ToolrNames::new(&all),
            "tools.m",
        )
    }

    /// `extract_groups` on a module with no imports beyond its own.
    fn groups_in_module(m: &ModModule, doc: Option<&str>) -> Vec<GroupBinding> {
        let all = HashMap::from([(
            "tools.m".to_string(),
            ImportTable::from_module(m, "tools.m", false),
        )]);
        extract_groups(m, doc, &HashMap::new(), &ToolrNames::new(&all), "tools.m")
    }

    fn paths(groups: &[GroupBinding]) -> Vec<(String, String)> {
        groups
            .iter()
            .map(|g| (g.var.clone(), g.full_path()))
            .collect()
    }

    fn pair(var: &str, path: &str) -> (String, String) {
        (var.to_string(), path.to_string())
    }

    #[test]
    fn aliased_import_binds_a_group() {
        let groups = groups_in(
            "from toolr import command_group as cg\ng = cg(\"alias\", \"Alias\", \"Aliased.\")\nkid = g.command_group(\"kid\", \"Kid\", \"Kid.\")\n",
            &[],
        );
        assert_eq!(
            paths(&groups),
            [pair("g", "alias"), pair("kid", "alias.kid")]
        );
        assert_eq!(groups[0].fields.description.text(), "Aliased.");
    }

    #[test]
    fn toolr_module_alias_attribute_binds_a_group() {
        let groups = groups_in(
            "import toolr as t\ng = t.command_group(\"t\", \"T\", \"T.\")\n",
            &[],
        );
        assert_eq!(paths(&groups), [pair("g", "t")]);
    }

    #[test]
    fn assigned_alias_binds_a_group() {
        let groups = groups_in(
            "import toolr\ncg = toolr.command_group\ng = cg(\"assigned\", \"A\", \"A.\")\n",
            &[],
        );
        assert_eq!(paths(&groups), [pair("g", "assigned")]);
    }

    #[test]
    fn reexported_alias_binds_a_group() {
        let groups = groups_in(
            "from tools._common import cg\ncg(\"re\", \"Re\", \"Re.\")\n",
            &[("tools._common", "from toolr import command_group as cg\n")],
        );
        assert_eq!(paths(&groups), [pair("re", "re")]);
    }

    #[test]
    fn annotated_binding_binds_a_group() {
        let groups = groups_in(
            "h: CommandGroup = command_group(\"ann\", \"Ann\", \"Ann.\")\nkid = h.command_group(\"kid\", \"Kid\", \"Kid.\")\n",
            &[],
        );
        assert_eq!(paths(&groups), [pair("h", "ann"), pair("kid", "ann.kid")]);
    }

    #[test]
    fn annotated_alias_binds_a_group() {
        let groups = groups_in(
            "import toolr\nfrom toolr import command_group\nfrom toolr import command_group as other\ncg: object = command_group\ntcg: object = toolr.command_group\nlater: object = cg\na = cg(\"a\", \"A\", \"A.\")\nb = tcg(\"b\", \"B\", \"B.\")\nc = later(\"c\", \"C\", \"C.\")\nd = other(\"d\", \"D\", \"D.\")\n",
            &[],
        );
        assert_eq!(
            paths(&groups),
            [
                pair("a", "a"),
                pair("b", "b"),
                pair("c", "c"),
                pair("d", "d")
            ]
        );
    }

    #[test]
    fn annotated_alias_of_something_else_is_not_a_group() {
        let groups = groups_in(
            "from toolr import command_group as cg\ncg: object = print\nsub: object = print.x()\ng = cg(\"x\", \"X\", \"X.\")\n",
            &[],
        );
        assert!(groups.is_empty());
    }

    #[test]
    fn try_except_fallback_binding_wins() {
        let groups = groups_in(
            "try:\n    from toolr import command_group as cg\nexcept ImportError:\n    cg = None\ng = cg(\"d\", \"D\", \"D.\")\n",
            &[],
        );
        assert!(groups.is_empty());
    }

    #[test]
    fn del_does_not_unbind_an_alias() {
        let groups = groups_in(
            "from toolr import command_group as cg\ng = cg(\"c\", \"C\", \"C.\")\ndel cg\n",
            &[],
        );
        assert_eq!(paths(&groups), [pair("g", "c")]);
    }

    #[test]
    fn annotation_without_a_value_binds_nothing() {
        let groups = groups_in(
            "g: CommandGroup\nh: int = 1\nk.x: int = command_group(\"k\", \"K\", \"K.\")\n",
            &[],
        );
        assert!(groups.is_empty());
    }

    #[test]
    fn bare_annotation_does_not_hide_an_imported_alias() {
        let groups = groups_in(
            "from toolr import command_group as cg\ncg: object\ng = cg(\"x\", \"X\", \"Group x.\")\n",
            &[],
        );
        assert_eq!(paths(&groups), [pair("g", "x")]);
    }

    #[test]
    fn self_copy_resolves_through_the_import() {
        let groups = groups_in(
            "from toolr import command_group as cg\ncg = cg\ng = cg(\"y\", \"Y\", \"Y.\")\n",
            &[],
        );
        assert_eq!(paths(&groups), [pair("g", "y")]);
        let groups = groups_in(
            "from toolr import command_group as cg\ncg: object = cg\ng = cg(\"y\", \"Y\", \"Y.\")\n",
            &[],
        );
        assert_eq!(paths(&groups), [pair("g", "y")]);
    }

    #[test]
    fn self_copy_resolves_through_a_star_import() {
        let groups = groups_in(
            "from tools.lib import *\ncg = cg\ng = cg(\"s\", \"S\", \"S.\")\n",
            &[("tools.lib", "from toolr import command_group as cg\n")],
        );
        assert_eq!(paths(&groups), [pair("g", "s")]);
    }

    #[test]
    fn self_copy_of_nothing_is_not_a_group() {
        let groups = groups_in("cg = cg\ng = cg(\"n\", \"N\", \"N.\")\n", &[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn local_def_alias_is_not_a_group() {
        let groups = groups_in(
            "def cg(*args):\n    pass\ng = cg(\"local\", \"L\", \"L.\")\n",
            &[],
        );
        assert!(groups.is_empty());
    }

    #[test]
    fn alias_from_an_unreadable_module_is_not_a_group() {
        let groups = groups_in(
            "from elsewhere import cg\ng = cg(\"unknown\", \"U\", \"U.\")\n",
            &[],
        );
        assert!(groups.is_empty());
    }

    #[test]
    fn literal_spelling_counts_even_when_local() {
        let groups = groups_in(
            "def command_group(*args):\n    pass\ng = command_group(\"lit\", \"L\", \"L.\")\n",
            &[],
        );
        assert_eq!(paths(&groups), [pair("g", "lit")]);
    }

    #[test]
    fn extracts_command_group_with_literal_args() {
        let src = r#"group = command_group("ci", "CI utilities")"#;
        let m = parse_src(src);
        let groups = groups_in_module(&m, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].var, "group");
        assert_eq!(groups[0].name, "ci");
        assert_eq!(groups[0].fields.title.text(), "CI utilities");
    }

    #[test]
    fn explicit_title_keeps_docstring_full_description() {
        // When the caller passes both a positional title and a docstring,
        // the title wins for the short slot but the docstring's rendered
        // ``full_description`` (short + long + sections) still populates
        // `description` (clap's `long_about`).
        let src = r#"group = command_group("ci", "CI utilities", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = groups_in_module(&m, Some("First paragraph short.\n\nLong paragraph body."));
        assert_eq!(groups[0].fields.title.text(), "CI utilities");
        assert_eq!(
            groups[0].fields.description.text(),
            "First paragraph short.\n\nLong paragraph body."
        );
    }

    #[test]
    fn docstring_first_paragraph_becomes_title() {
        // Single-line docstring: short_description fills `title`,
        // `description` ends up the same single paragraph (no long
        // body, no sections to append).
        let src = r#"group = command_group("widget", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = groups_in_module(&m, Some("Example single-line docstring for a group."));
        assert_eq!(
            groups[0].fields.title.text(),
            "Example single-line docstring for a group."
        );
        assert_eq!(
            groups[0].fields.description.text(),
            "Example single-line docstring for a group."
        );
    }

    #[test]
    fn docstring_long_paragraph_becomes_description() {
        // Two-paragraph docstring: first paragraph → title.
        // `description` is the full render (short + long), so
        // `--help` re-states the blurb plus the long body.
        let src = r#"group = command_group("ci", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = groups_in_module(
            &m,
            Some("Short blurb for parent listing.\n\nLonger prose shown by --help only."),
        );
        assert_eq!(
            groups[0].fields.title.text(),
            "Short blurb for parent listing."
        );
        assert_eq!(
            groups[0].fields.description.text(),
            "Short blurb for parent listing.\n\nLonger prose shown by --help only."
        );
    }

    #[test]
    fn description_kwarg_with_docstring_is_contradictory() {
        // Python's `command_group` refuses both; the build reports it.
        let src = r#"group = command_group("ci", description="kw-desc", docstring=__doc__)"#;
        let m = parse_src(src);
        let groups = groups_in_module(&m, Some("Short title.\n\nLong paragraph."));
        assert!(groups[0].contradictory);
        assert!(groups[0].describes);
    }

    #[test]
    fn title_only_call_does_not_describe() {
        let m = parse_src(r#"group = command_group("ci", "CI")"#);
        let groups = groups_in_module(&m, None);
        assert!(!groups[0].describes);
        assert!(!groups[0].contradictory);
    }

    #[test]
    fn parses_description_from_third_positional() {
        let src = r#"group = command_group("n", "t", "desc body")"#;
        let m = parse_src(src);
        let groups = groups_in_module(&m, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].fields.description.text(), "desc body");
    }

    #[test]
    fn description_kwarg_wins_over_positional() {
        let src = r#"group = command_group("n", "t", "pos", description="kw")"#;
        let m = parse_src(src);
        let groups = groups_in_module(&m, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].fields.description.text(), "kw");
    }

    #[test]
    fn ignores_non_command_group_assignments() {
        let src = "x = 1\ny = some_other_func(\"ci\")\n";
        let m = parse_src(src);
        assert!(groups_in_module(&m, None).is_empty());
    }

    #[test]
    fn nested_group_records_parent_full_path() {
        let src = r#"docker = command_group("docker", "Docker")
image = docker.command_group("image", "Image")
"#;
        let m = parse_src(src);
        let groups = groups_in_module(&m, None);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "docker");
        assert_eq!(groups[0].parent, None);
        assert_eq!(groups[0].full_path(), "docker");
        assert_eq!(groups[1].name, "image");
        assert_eq!(groups[1].parent.as_deref(), Some("docker"));
        assert_eq!(groups[1].full_path(), "docker.image");
    }

    #[test]
    fn explicit_parent_kwarg_nests_under_named_group() {
        // Matches the `command_group("...", parent="ci")` signature
        // exposed by `toolr._registry.command_group`. The parent
        // doesn't have to be declared in the same file — we trust the
        // string as the dotted full_path.
        let src = r#"
helm_diff = command_group("helm-diff-pr-comment", "Helm diff", parent="ci")
"#;
        let m = parse_src(src);
        let groups = groups_in_module(&m, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "helm-diff-pr-comment");
        assert_eq!(groups[0].parent.as_deref(), Some("ci"));
        assert_eq!(groups[0].full_path(), "ci.helm-diff-pr-comment");
    }

    #[test]
    fn two_level_nesting_concatenates_full_path() {
        let src = r#"a = command_group("a", "A")
b = a.command_group("b", "B")
c = b.command_group("c", "C")
"#;
        let m = parse_src(src);
        let groups = groups_in_module(&m, None);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[2].full_path(), "a.b.c");
        assert_eq!(groups[2].parent.as_deref(), Some("a.b"));
    }

    /// What `detect_unsupported_group_bindings` reports for `src` parsed as `tools.m`.
    fn unbound_in(src: &str) -> Vec<UnsupportedCommand> {
        let m = parse_src(src);
        let all = imports(&[("tools.m", src)]);
        detect_unsupported_group_bindings(&m, "tools.m", &ToolrNames::new(&all))
    }

    fn assert_unbound(src: &str, count: usize) {
        let found = unbound_in(src);
        assert_eq!(found.len(), count, "{src}: {found:?}");
        for u in &found {
            assert_eq!(u.module, "tools.m");
            assert_eq!(u.function, "command_group");
            assert_eq!(u.kind, UnsupportedCommandKind::GroupBinding);
        }
    }

    #[test]
    fn plain_top_level_bindings_are_not_reported() {
        assert_unbound(
            "from toolr import command_group\na = command_group(\"a\", \"A\", \"A.\")\nb: T = command_group(\"b\", \"B\", \"B.\")\ncommand_group(\"c\", \"C\", \"C.\")\nk = a.command_group(\"k\", \"K\", \"K.\")\n",
            0,
        );
    }

    #[test]
    fn tuple_target_reports_each_call() {
        assert_unbound(
            "a, b = command_group(\"a\", \"A\", \"A.\"), command_group(\"b\", \"B\", \"B.\")\n",
            2,
        );
    }

    #[test]
    fn chained_target_is_reported() {
        assert_unbound("a = b = command_group(\"a\", \"A\", \"A.\")\n", 1);
    }

    #[test]
    fn walrus_is_reported() {
        assert_unbound("(g := command_group(\"g\", \"G\", \"G.\"))\n", 1);
    }

    #[test]
    fn compound_statement_bodies_are_reported() {
        for head in [
            "if x:\n    ",
            "try:\n    pass\nfinally:\n    ",
            "with x:\n    ",
            "for _ in x:\n    ",
            "while x:\n    ",
            "match x:\n    case 1:\n        ",
        ] {
            let src = format!("{head}g = command_group(\"g\", \"G\", \"G.\")\n");
            assert_unbound(&src, 1);
        }
        assert_unbound("if x:\n    command_group(\"g\", \"G\", \"G.\")\n", 1);
    }

    #[test]
    fn function_and_class_bodies_are_reported() {
        assert_unbound(
            "def make():\n    return command_group(\"g\", \"G\", \"G.\")\n\n\nclass C:\n    g = command_group(\"h\", \"H\", \"H.\")\n",
            2,
        );
    }

    #[test]
    fn call_nested_in_an_expression_is_reported() {
        assert_unbound("command_group(\"g\", \"G\", \"G.\").command(f)\n", 1);
        assert_unbound(
            "g = command_group(\"g\", \"G\", \"G.\", parent=command_group(\"p\", \"P\", \"P.\"))\n",
            0,
        );
    }

    #[test]
    fn aliased_call_under_an_if_is_reported() {
        assert_unbound(
            "from toolr import command_group as cg\nif x:\n    g = cg(\"g\", \"G\", \"G.\")\n",
            1,
        );
    }

    #[test]
    fn unrelated_call_under_an_if_is_not_reported() {
        assert_unbound("def cg(*a):\n    return a\n\n\nif x:\n    h = cg(1)\n", 0);
    }

    #[test]
    fn supported_group_call_matches_group_binding() {
        let src = "from toolr import command_group\na = command_group(\"a\", \"A\", \"A.\")\nb: T = command_group(\"b\", \"B\", \"B.\")\ncommand_group(\"c\", \"C\", \"C.\")\nx, y = command_group(\"d\", \"D\", \"D.\"), 1\n(w := command_group(\"e\", \"E\", \"E.\"))\nz = other(1)\n";
        let m = parse_src(src);
        let all = imports(&[("tools.m", src)]);
        let names = ToolrNames::new(&all);
        let source = Source::new(&m, None);
        let bound: Vec<bool> = m
            .body
            .iter()
            .skip(1)
            .map(|s| group_binding(s, &source, &[], &HashMap::new(), &names, "tools.m").is_some())
            .collect();
        assert_eq!(bound, [true, true, true, false, false, false]);
        assert_eq!(unbound_in(src).len(), 2);
    }
}
