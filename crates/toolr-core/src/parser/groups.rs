//! Extract `group = command_group(...)` assignments from a module AST.

use std::collections::HashMap;

use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, walk_expr as walk_source_expr, walk_stmt as walk_source_stmt,
};
use ruff_python_ast::visitor::{Visitor, walk_except_handler, walk_expr, walk_stmt};
use ruff_python_ast::{
    Alias, ExceptHandler, Expr, ExprCall, ModModule, Parameters, Stmt, StmtAnnAssign, StmtAssign,
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

const NAME: Param = Param {
    keyword: "name",
    position: 0,
};
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
/// Nested groups (`command_group("parent.child", ...)` or `parent=...`) get
/// their `parent` field set to the parent's `full_path()`, so the CLI builder
/// can reconstruct the hierarchy. Source order matters: a `parent=<var>` must
/// be assigned before its children (which Python already requires).
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

/// The call and bound name of a module-level statement the parser binds as a
/// group: a supported shape whose call is `command_group`. Both the binding
/// pass and the detector below ask this, so a statement is bound or reported
/// in one place.
///
/// Mirrored in `toolr.testing._groups._bound_group_call`; change both together.
fn bound_group_call<'a>(
    stmt: &'a Stmt,
    names: &ToolrNames,
    module_path: &str,
) -> Option<(Option<String>, &'a ExprCall)> {
    supported_group_call(stmt).filter(|(_, call)| is_command_group_call(call, names, module_path))
}

/// Every toolr `command_group(...)` call the parser can't bind, one entry each,
/// in source order. This is the one statement of the placement rule; the
/// user-facing docs repeat it.
///
/// A `command_group(...)` call is supported only as the whole value of a
/// module-level `name = command_group(...)`, `name: T = command_group(...)` or
/// bare `command_group(...)` statement, with a string-literal name (positional
/// or `name=`). Any other call to toolr's `command_group` is reported:
///
/// - part of a tuple or chained target, or a walrus;
/// - under `if`, `try`, `with`, a loop or `match`;
/// - inside a function, lambda, comprehension or class;
/// - nested in another expression, such as a decorator or call argument;
/// - a supported statement whose name isn't a string literal;
/// - a supported statement calling `<x>.command_group(...)` on anything but the
///   `toolr` module: the removed method form, reported as `MethodGroup`.
///
/// Such a group, and every command on it, would otherwise vanish without a
/// word. `command_group` creates a group when none is registered, so a
/// name-only call is reported too. A supported statement is skipped whole,
/// annotation included.
///
/// A callee is toolr's when a name resolves to it through the import tables, or
/// an attribute's base is the `toolr` module. Only a parameter of the enclosing
/// `def` or `lambda` shadows the name; no other local rebinding does. The entry's
/// name is `command_group("<name>")` when the name argument is a string literal.
///
/// Mirrored in `toolr.testing._shapes._unsupported_group_bindings` and
/// `toolr.testing._groups._unbound_group_calls`; change all three together.
pub fn detect_unsupported_group_bindings(
    module: &ModModule,
    module_path: &str,
    names: &ToolrNames,
) -> Vec<UnsupportedCommand> {
    let mut finder = UnboundGroupCalls {
        names,
        module_path,
        shadowed: Vec::new(),
        out: Vec::new(),
    };
    for stmt in &module.body {
        match bound_group_call(stmt, names, module_path) {
            Some((_, call)) => {
                if argument(call, NAME).and_then(literal_str).is_none() {
                    finder.report(call);
                }
            }
            None => match supported_group_call(stmt) {
                // Mirrored in `toolr.testing._groups._unbound_group_calls`.
                Some((_, call)) if matches!(call.func.as_ref(), Expr::Attribute(a) if a.attr.as_str() == "command_group") => {
                    finder.report_as(call, UnsupportedCommandKind::MethodGroup)
                }
                _ => finder.visit_stmt(stmt),
            },
        }
    }
    finder.out
}

struct UnboundGroupCalls<'a> {
    names: &'a ToolrNames<'a>,
    module_path: &'a str,
    /// Parameter names of the enclosing functions and lambdas.
    shadowed: Vec<String>,
    out: Vec<UnsupportedCommand>,
}

impl UnboundGroupCalls<'_> {
    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed.iter().any(|s| s == name)
    }

    /// Whether `call` is toolr's `command_group`. Deliberately stricter than
    /// [`is_command_group_call`]: this one reports, that one binds.
    ///
    /// Mirrored in `toolr.testing._groups._unbound_group_calls` (`is_toolr_group_call`); change both together.
    fn is_toolr_group_call(&self, call: &ExprCall) -> bool {
        match call.func.as_ref() {
            Expr::Name(n) => {
                !self.is_shadowed(n.id.as_str())
                    && self.names.command_group(self.module_path, n.id.as_str()) == Binding::Toolr
            }
            Expr::Attribute(a) if a.attr.as_str() == "command_group" => {
                matches!(a.value.as_ref(), Expr::Name(n)
                    if !self.is_shadowed(n.id.as_str())
                        && self.names.is_toolr_module(self.module_path, n.id.as_str()))
            }
            _ => false,
        }
    }

    fn report(&mut self, call: &ExprCall) {
        self.report_as(call, UnsupportedCommandKind::GroupBinding);
    }

    /// Mirrored in `toolr.testing._shapes._group_call_label`; change both together.
    fn report_as(&mut self, call: &ExprCall, kind: UnsupportedCommandKind) {
        let function = match argument(call, NAME).and_then(literal_str) {
            Some(name) => format!("command_group(\"{name}\")"),
            None => "command_group".to_string(),
        };
        self.out.push(UnsupportedCommand {
            module: self.module_path.to_string(),
            function,
            kind,
        });
    }

    /// Visit `body` with `parameters` shadowing outer names. A parameter is the one
    /// rebinding that needs no flow analysis to know the name isn't toolr's, so
    /// no other local rebinding shadows.
    ///
    /// Mirrored in `toolr.testing._groups._parameter_names` and the stack walk of
    /// `_unbound_group_calls`; change both together.
    fn scoped(&mut self, parameters: Option<&Parameters>, visit: impl FnOnce(&mut Self)) {
        let depth = self.shadowed.len();
        if let Some(parameters) = parameters {
            self.shadowed
                .extend(parameters.iter().map(|p| p.name().as_str().to_string()));
        }
        visit(self);
        self.shadowed.truncate(depth);
    }
}

// Source order, not evaluation order, so the message lists problems as
// `toolr.testing._groups._unbound_group_calls` does, which sorts by position.
impl<'a> SourceOrderVisitor<'a> for UnboundGroupCalls<'_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        let Stmt::FunctionDef(func) = stmt else {
            walk_source_stmt(self, stmt);
            return;
        };
        // Decorators, defaults and annotations read the enclosing scope.
        for decorator in &func.decorator_list {
            self.visit_decorator(decorator);
        }
        self.visit_parameters(&func.parameters);
        if let Some(returns) = &func.returns {
            self.visit_expr(returns);
        }
        self.scoped(Some(&func.parameters), |this| this.visit_body(&func.body));
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Lambda(lambda) = expr {
            if let Some(parameters) = &lambda.parameters {
                self.visit_parameters(parameters);
            }
            self.scoped(lambda.parameters.as_deref(), |this| {
                this.visit_expr(&lambda.body);
            });
            return;
        }
        if let Expr::Call(call) = expr
            && self.is_toolr_group_call(call)
        {
            self.report(call);
        }
        walk_source_expr(self, expr);
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
    let (var_name, call) = bound_group_call(stmt, names, module_path)?;
    // Parent path can come from two source patterns:
    //   1. Keyword-arg with variable reference:
    //      `child = command_group(..., parent=parent_var)`.
    //   2. Keyword-arg with literal string:
    //      `child = command_group(..., parent="ci")`.
    //
    // Plus the dotted-name shape — `command_group("ci.helm-diff-pr-comment", ...)`
    // — splits the leaf off inside `parse_group_call` and overrides
    // whatever parent_path we resolve here.
    let parent_path = parent_kwarg_var(call)
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
/// tables resolve to it, or `command_group` on the `toolr` module.
///
/// `toolr.testing._groups._is_command_group` mirrors this in Python, resolving
/// aliases from source with `_binding`; change both together.
fn is_command_group_call(call: &ExprCall, names: &ToolrNames, module: &str) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => {
            n.id.as_str() == "command_group"
                || names.command_group(module, n.id.as_str()) == Binding::Toolr
        }
        Expr::Attribute(a) if a.attr.as_str() == "command_group" => {
            matches!(a.value.as_ref(), Expr::Name(n) if names.is_toolr_module(module, n.id.as_str()))
        }
        _ => false,
    }
}

fn parse_group_call(
    call: &ExprCall,
    var: &str,
    source: &Source<'_>,
    parent: Option<String>,
) -> Option<GroupBinding> {
    let raw_name = argument(call, NAME).and_then(literal_str)?;
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
mod tests;
