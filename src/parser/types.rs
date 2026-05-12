//! Resolve a Python parameter annotation to a [`SupportedType`].
//!
//! Toolr supports an explicit, opinionated set of parameter types. Any
//! annotation outside that set is rejected at manifest build time with
//! a clear pointer to `toolr.types` as the extension namespace.
//!
//! The resolver is name-based — it inspects the textual annotation as
//! parsed by `ruff_python_parser` plus a small [`TypeImports`] table
//! that tracks which symbols in this module were imported from
//! `toolr.types`. That lets us resolve `from toolr.types import
//! ResolvedPath as RP` style aliases without doing a full symbol-table
//! pass over the file.

use ruff_python_ast::{Expr, ExprCall, ModModule, Stmt, StmtFunctionDef};
use serde::{Deserialize, Serialize};

use super::symbols::EnumTable;
use crate::manifest::Argument;

/// Filesystem constraints layered on top of a `Path`/`AbsolutePath`/
/// `ResolvedPath` parameter, expressed via `arg(must_exist=True, ...)`
/// inside `Annotated[Path, arg(...)]`. Top-level fields fold together
/// — for example `must_be_file` implies `must_exist`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathConstraints {
    #[serde(default)]
    pub must_exist: bool,
    #[serde(default)]
    pub must_be_file: bool,
    #[serde(default)]
    pub must_be_dir: bool,
}

impl PathConstraints {
    pub fn is_empty(&self) -> bool {
        !self.must_exist && !self.must_be_file && !self.must_be_dir
    }
    /// Whether any kind of disk check is required.
    pub fn requires_existence(&self) -> bool {
        self.must_exist || self.must_be_file || self.must_be_dir
    }
}

/// Every annotation shape toolr recognises end-to-end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum SupportedType {
    Str,
    Int,
    Float,
    Bool,
    /// `pathlib.Path` — string passes through unchanged.
    Path,
    /// `toolr.types.AbsolutePath` — absolutised against cwd, no fs check.
    AbsolutePath,
    /// `toolr.types.ResolvedPath` — canonicalised, must exist.
    ResolvedPath,
    DateTime,
    Date,
    Time,
    Uuid,
    Ipv4,
    Ipv6,
    /// `toolr.types.Email` — RFC-5321-ish address (single `local@domain`
    /// pair, no comments / display name). Runtime value is `str`.
    Email,
    /// `Literal["a", "b"]` — string validated against the allowed set.
    Literal(Vec<String>),
    /// Enum subclass resolved via [`EnumTable`].
    Enum {
        name: String,
        values: Vec<String>,
    },
    /// `list[T]` / `List[T]` — repeated keyword that appends.
    List(Box<SupportedType>),
    /// Heterogeneous `tuple[T1, T2, ...]`.
    Tuple(Vec<SupportedType>),
    /// `T | None` / `Optional[T]` — same as T at the CLI surface, but
    /// the parameter is not required.
    Optional(Box<SupportedType>),
}

impl SupportedType {
    /// Strip an `Optional(T)` wrapper to `T`, returning whether the
    /// original was wrapped. Helps the CLI-build path treat
    /// `T | None` as "T with required=false".
    pub fn unwrap_optional(self) -> (Self, bool) {
        match self {
            SupportedType::Optional(inner) => (*inner, true),
            other => (other, false),
        }
    }
}

/// Reasons annotation resolution can fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedType {
    /// A bare name we don't recognise (e.g. `datetime.datetime` without
    /// going through `toolr.types`).
    UnknownName(String),
    /// `Annotated[T, ...]` wrapper — supported, but the inner T was
    /// unsupported (we surface the inner error).
    Inner(Box<UnsupportedType>),
    /// `T | None` with both sides not-None (we only support
    /// `T | None`, not arbitrary unions).
    UnsupportedUnion(String),
    /// A subscript shape we don't handle (e.g. `dict[K, V]`).
    UnsupportedShape(String),
}

/// A typed-annotation rejection with full context for diagnostic output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeResolutionError {
    /// Dotted module path the offending command lives in (`tools.foo.bar`).
    pub module: String,
    /// Python function name of the command.
    pub function: String,
    /// Parameter name on that function.
    pub argument: String,
    /// Textual rendering of the unsupported annotation as it appeared
    /// in source — for the user-facing message.
    pub annotation: String,
    /// The underlying [`UnsupportedType`] reason.
    pub reason: UnsupportedType,
}

impl std::fmt::Display for TypeResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{module}::{function} argument `{arg}` (annotated `{annotation}`): {reason}",
            module = self.module,
            function = self.function,
            arg = self.argument,
            annotation = self.annotation,
            reason = self.reason,
        )
    }
}

impl std::fmt::Display for UnsupportedType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownName(n) => write!(
                f,
                "type `{n}` is not supported. Use a primitive (int, float, bool, str, pathlib.Path), \
                 a Literal[...] or Enum, or one of the aliases under `toolr.types`."
            ),
            Self::Inner(inner) => inner.fmt(f),
            Self::UnsupportedUnion(s) => write!(f, "unsupported union `{s}`; only `T | None` is recognised."),
            Self::UnsupportedShape(s) => write!(f, "unsupported generic shape `{s}`."),
        }
    }
}

/// Records which symbols in the current module were imported from
/// `toolr.types` (or its submodules), and whether the user did
/// `import toolr.types as X` so `X.Foo` resolves cleanly too.
#[derive(Debug, Default, Clone)]
pub struct TypeImports {
    /// Symbol → canonical toolr.types name. E.g. `RP → "ResolvedPath"`
    /// after `from toolr.types import ResolvedPath as RP`.
    direct: std::collections::HashMap<String, String>,
    /// Module alias for `import toolr.types as X`. The aliased name
    /// (or `toolr` for `import toolr.types` without alias) maps onto
    /// `toolr.types`.
    module_aliases: Vec<String>,
}

impl TypeImports {
    /// Walk the module's top-level statements to find imports from
    /// `toolr.types`.
    pub fn from_module(module: &ModModule) -> Self {
        let mut imports = Self::default();
        for stmt in &module.body {
            match stmt {
                Stmt::ImportFrom(import) => {
                    let module_name = import
                        .module
                        .as_ref()
                        .map(|m| m.as_str())
                        .unwrap_or_default();
                    if module_name != "toolr.types" {
                        continue;
                    }
                    for alias in &import.names {
                        let canonical = alias.name.as_str().to_string();
                        let local = alias
                            .asname
                            .as_ref()
                            .map(|n| n.as_str().to_string())
                            .unwrap_or_else(|| canonical.clone());
                        imports.direct.insert(local, canonical);
                    }
                }
                Stmt::Import(import) => {
                    for alias in &import.names {
                        if alias.name.as_str() == "toolr.types" {
                            let local = alias
                                .asname
                                .as_ref()
                                .map(|n| n.as_str().to_string())
                                .unwrap_or_else(|| "toolr".to_string());
                            imports.module_aliases.push(local);
                        }
                    }
                }
                _ => {}
            }
        }
        imports
    }

    /// If `name` was imported from `toolr.types`, return its canonical
    /// `toolr.types` symbol name.
    fn resolve_direct(&self, name: &str) -> Option<&str> {
        self.direct.get(name).map(String::as_str)
    }

    /// If `expr` is `<alias>.X` for an alias that points at `toolr.types`,
    /// return `X`. Handles `toolr.types.X` (when imported as
    /// `import toolr.types`) too.
    fn resolve_attribute<'a>(&self, expr: &'a Expr) -> Option<&'a str> {
        let Expr::Attribute(attr) = expr else {
            return None;
        };
        let head = match attr.value.as_ref() {
            Expr::Name(n) => n.id.as_str().to_string(),
            Expr::Attribute(inner) => match inner.value.as_ref() {
                Expr::Name(n) if n.id.as_str() == "toolr" && inner.attr.as_str() == "types" => {
                    "toolr.types".to_string()
                }
                _ => return None,
            },
            _ => return None,
        };
        if head == "toolr.types" {
            return Some(attr.attr.as_str());
        }
        if self.module_aliases.iter().any(|a| a == &head) {
            return Some(attr.attr.as_str());
        }
        None
    }
}

/// Walk a function's parameters and populate `resolved_type` on each
/// matching [`Argument`]. Unsupported annotations are pushed to `errors`
/// with `module` / function-name context, and the corresponding
/// `Argument.resolved_type` stays `None`.
pub fn resolve_arguments(
    func: &StmtFunctionDef,
    arguments: &mut [Argument],
    enums: &EnumTable,
    type_imports: &TypeImports,
    module: &str,
    errors: &mut Vec<TypeResolutionError>,
) {
    let params = func.parameters.as_ref();
    let function = func.name.as_str().to_string();
    let mut i = 0usize;
    // First positional in the signature is `ctx`; skip it.
    for p in params.args.iter().skip(1) {
        resolve_one(
            p.parameter.annotation.as_deref(),
            &mut arguments[i],
            enums,
            type_imports,
            module,
            &function,
            errors,
        );
        i += 1;
    }
    if let Some(vararg) = params.vararg.as_deref() {
        resolve_one(
            vararg.annotation.as_deref(),
            &mut arguments[i],
            enums,
            type_imports,
            module,
            &function,
            errors,
        );
        i += 1;
    }
    for p in &params.kwonlyargs {
        resolve_one(
            p.parameter.annotation.as_deref(),
            &mut arguments[i],
            enums,
            type_imports,
            module,
            &function,
            errors,
        );
        i += 1;
    }
}

fn resolve_one(
    annotation: Option<&Expr>,
    arg: &mut Argument,
    enums: &EnumTable,
    type_imports: &TypeImports,
    module: &str,
    function: &str,
    errors: &mut Vec<TypeResolutionError>,
) {
    let Some(expr) = annotation else {
        // Bare-typed argument with no annotation — leave resolved_type
        // empty so the CLI builder falls back to string semantics. The
        // python registry still imposes its own runtime checks.
        return;
    };
    arg.path_constraints = extract_path_constraints(expr);
    match resolve(expr, enums, type_imports) {
        Ok(ty) => arg.resolved_type = Some(ty),
        Err(reason) => errors.push(TypeResolutionError {
            module: module.to_string(),
            function: function.to_string(),
            argument: arg.name.clone(),
            annotation: arg.type_annotation.clone().unwrap_or_default(),
            reason,
        }),
    }
}

/// Walk an `Annotated[T, arg(...), ...]` annotation and extract
/// `PathConstraints` from any `arg(...)` call inside it. Returns
/// `None` if the annotation isn't `Annotated[...]` or carries no
/// path-related arg() metadata.
pub fn extract_path_constraints(annotation: &Expr) -> Option<PathConstraints> {
    let Expr::Subscript(sub) = annotation else {
        return None;
    };
    let head = match sub.value.as_ref() {
        Expr::Name(n) => n.id.as_str(),
        Expr::Attribute(a) => a.attr.as_str(),
        _ => return None,
    };
    if head != "Annotated" {
        return None;
    }
    let elts: Vec<&Expr> = match sub.slice.as_ref() {
        Expr::Tuple(t) => t.elts.iter().collect(),
        single => vec![single],
    };
    let mut constraints = PathConstraints::default();
    let mut hit = false;
    for elt in elts.iter().skip(1) {
        let Expr::Call(call) = elt else { continue };
        if !is_toolr_arg_call(call) {
            continue;
        }
        for kw in &call.arguments.keywords {
            let Some(name) = kw.arg.as_ref().map(|n| n.as_str()) else {
                continue;
            };
            let Expr::BooleanLiteral(b) = &kw.value else { continue };
            match name {
                "must_exist" => {
                    constraints.must_exist = b.value;
                    hit = true;
                }
                "must_be_file" => {
                    constraints.must_be_file = b.value;
                    hit = true;
                }
                "must_be_dir" => {
                    constraints.must_be_dir = b.value;
                    hit = true;
                }
                _ => {}
            }
        }
    }
    if hit { Some(constraints) } else { None }
}

fn is_toolr_arg_call(call: &ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(n) => n.id.as_str() == "arg",
        Expr::Attribute(a) => a.attr.as_str() == "arg",
        _ => false,
    }
}

/// Resolve a parameter annotation to a [`SupportedType`].
pub fn resolve(
    annotation: &Expr,
    enums: &EnumTable,
    imports: &TypeImports,
) -> Result<SupportedType, UnsupportedType> {
    match annotation {
        Expr::Name(n) => resolve_name(n.id.as_str(), enums, imports),
        Expr::Attribute(_) => {
            if let Some(toolr_name) = imports.resolve_attribute(annotation) {
                return resolve_toolr_types_name(toolr_name);
            }
            // `pathlib.Path` (or its aliases) — the only stdlib
            // dotted-name we natively recognise. Anything else is a
            // diagnostic.
            let rendered = render_attribute(annotation);
            if rendered == "pathlib.Path" {
                return Ok(SupportedType::Path);
            }
            Err(UnsupportedType::UnknownName(rendered))
        }
        Expr::Subscript(sub) => resolve_subscript(sub, enums, imports),
        Expr::BinOp(op) => resolve_bin_op(op, enums, imports),
        _ => Err(UnsupportedType::UnknownName(annotation_to_label(annotation))),
    }
}

fn resolve_name(
    name: &str,
    enums: &EnumTable,
    imports: &TypeImports,
) -> Result<SupportedType, UnsupportedType> {
    if let Some(canonical) = imports.resolve_direct(name) {
        return resolve_toolr_types_name(canonical);
    }
    match name {
        "str" => Ok(SupportedType::Str),
        "int" => Ok(SupportedType::Int),
        "float" => Ok(SupportedType::Float),
        "bool" => Ok(SupportedType::Bool),
        "Path" => Ok(SupportedType::Path),
        _ => {
            if let Some(values) = enums.lookup(name) {
                Ok(SupportedType::Enum {
                    name: name.to_string(),
                    values: values.to_vec(),
                })
            } else {
                Err(UnsupportedType::UnknownName(name.to_string()))
            }
        }
    }
}

fn resolve_toolr_types_name(name: &str) -> Result<SupportedType, UnsupportedType> {
    match name {
        "DateTime" => Ok(SupportedType::DateTime),
        "Date" => Ok(SupportedType::Date),
        "Time" => Ok(SupportedType::Time),
        "UUID" => Ok(SupportedType::Uuid),
        "IPv4" => Ok(SupportedType::Ipv4),
        "IPv6" => Ok(SupportedType::Ipv6),
        "AbsolutePath" => Ok(SupportedType::AbsolutePath),
        "ResolvedPath" => Ok(SupportedType::ResolvedPath),
        "Email" => Ok(SupportedType::Email),
        other => Err(UnsupportedType::UnknownName(format!("toolr.types.{other}"))),
    }
}

fn resolve_subscript(
    sub: &ruff_python_ast::ExprSubscript,
    enums: &EnumTable,
    imports: &TypeImports,
) -> Result<SupportedType, UnsupportedType> {
    let head = match sub.value.as_ref() {
        Expr::Name(n) => n.id.as_str(),
        Expr::Attribute(a) => a.attr.as_str(),
        _ => return Err(UnsupportedType::UnsupportedShape(annotation_to_label(sub.value.as_ref()))),
    };
    match head {
        "Literal" => {
            let values = literal_string_values(sub.slice.as_ref());
            if values.is_empty() {
                Err(UnsupportedType::UnsupportedShape("Literal[...]".into()))
            } else {
                Ok(SupportedType::Literal(values))
            }
        }
        "list" | "List" => {
            let inner = resolve(sub.slice.as_ref(), enums, imports)
                .map_err(|e| UnsupportedType::Inner(Box::new(e)))?;
            Ok(SupportedType::List(Box::new(inner)))
        }
        "tuple" | "Tuple" => {
            let parts = tuple_element_exprs(sub.slice.as_ref());
            let resolved: Result<Vec<_>, _> = parts
                .into_iter()
                .map(|elt| resolve(elt, enums, imports))
                .collect();
            Ok(SupportedType::Tuple(
                resolved.map_err(|e| UnsupportedType::Inner(Box::new(e)))?,
            ))
        }
        "Optional" => {
            let inner = resolve(sub.slice.as_ref(), enums, imports)
                .map_err(|e| UnsupportedType::Inner(Box::new(e)))?;
            Ok(SupportedType::Optional(Box::new(inner)))
        }
        "Annotated" => {
            // The first element is the underlying type; the rest are
            // metadata we ignore here. Validation of `arg(...)` lives
            // in a separate pass.
            let exprs = tuple_element_exprs(sub.slice.as_ref());
            let first = exprs
                .first()
                .ok_or_else(|| UnsupportedType::UnsupportedShape("Annotated[]".into()))?;
            resolve(first, enums, imports)
        }
        other => Err(UnsupportedType::UnsupportedShape(other.to_string())),
    }
}

fn resolve_bin_op(
    op: &ruff_python_ast::ExprBinOp,
    enums: &EnumTable,
    imports: &TypeImports,
) -> Result<SupportedType, UnsupportedType> {
    if !matches!(op.op, ruff_python_ast::Operator::BitOr) {
        return Err(UnsupportedType::UnsupportedShape(format!("{:?}", op.op)));
    }
    // Recognise `T | None` (in either order) as Optional[T]. Anything
    // else is a union we can't handle.
    let (lhs, rhs) = (op.left.as_ref(), op.right.as_ref());
    let none_lhs = matches!(lhs, Expr::NoneLiteral(_));
    let none_rhs = matches!(rhs, Expr::NoneLiteral(_));
    match (none_lhs, none_rhs) {
        (false, true) => {
            let inner = resolve(lhs, enums, imports)
                .map_err(|e| UnsupportedType::Inner(Box::new(e)))?;
            Ok(SupportedType::Optional(Box::new(inner)))
        }
        (true, false) => {
            let inner = resolve(rhs, enums, imports)
                .map_err(|e| UnsupportedType::Inner(Box::new(e)))?;
            Ok(SupportedType::Optional(Box::new(inner)))
        }
        _ => Err(UnsupportedType::UnsupportedUnion(annotation_to_label(
            &Expr::BinOp(op.clone()),
        ))),
    }
}

fn literal_string_values(expr: &Expr) -> Vec<String> {
    let elts: Vec<&Expr> = match expr {
        Expr::Tuple(t) => t.elts.iter().collect(),
        single => vec![single],
    };
    elts.into_iter()
        .filter_map(|e| match e {
            Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
            _ => None,
        })
        .collect()
}

fn tuple_element_exprs(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Tuple(t) => t.elts.iter().collect(),
        single => vec![single],
    }
}

fn render_attribute(expr: &Expr) -> String {
    match expr {
        Expr::Name(n) => n.id.as_str().to_string(),
        Expr::Attribute(a) => format!("{}.{}", render_attribute(a.value.as_ref()), a.attr),
        _ => annotation_to_label(expr),
    }
}

fn annotation_to_label(expr: &Expr) -> String {
    match expr {
        Expr::Name(n) => n.id.as_str().to_string(),
        Expr::Attribute(_) => render_attribute(expr),
        Expr::Subscript(s) => format!(
            "{}[...]",
            match s.value.as_ref() {
                Expr::Name(n) => n.id.as_str(),
                _ => "...",
            }
        ),
        Expr::BinOp(_) => "<union>".to_string(),
        _ => "<expr>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_python_file;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn module(src: &str) -> ModModule {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(src.as_bytes()).unwrap();
        parse_python_file(f.path()).unwrap()
    }

    fn first_annotation(src: &str) -> (ModModule, Expr) {
        let m = module(src);
        for stmt in &m.body {
            if let Stmt::FunctionDef(func) = stmt {
                let p = &func.parameters.args[0];
                if let Some(ann) = p.parameter.annotation.as_deref() {
                    return (m.clone(), ann.clone());
                }
            }
        }
        panic!("no annotated function");
    }

    #[test]
    fn primitives_resolve() {
        let (_, ann) = first_annotation("def f(x: int): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap(),
            SupportedType::Int
        );
        let (_, ann) = first_annotation("def f(x: float): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap(),
            SupportedType::Float
        );
        let (_, ann) = first_annotation("def f(x: bool): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap(),
            SupportedType::Bool
        );
        let (_, ann) = first_annotation("def f(x: str): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap(),
            SupportedType::Str
        );
    }

    #[test]
    fn bare_path_name_is_supported() {
        let (_, ann) = first_annotation("def f(x: Path): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap(),
            SupportedType::Path
        );
    }

    #[test]
    fn pathlib_path_attribute_is_supported() {
        let (_, ann) = first_annotation("def f(x: pathlib.Path): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap(),
            SupportedType::Path
        );
    }

    #[test]
    fn toolr_types_resolved_path_via_from_import() {
        let src = "from toolr.types import ResolvedPath\ndef f(x: ResolvedPath): pass\n";
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let imports = TypeImports::from_module(&m);
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &imports).unwrap(),
            SupportedType::ResolvedPath
        );
    }

    #[test]
    fn toolr_types_via_alias() {
        let src = "from toolr.types import ResolvedPath as RP\ndef f(x: RP): pass\n";
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let imports = TypeImports::from_module(&m);
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &imports).unwrap(),
            SupportedType::ResolvedPath
        );
    }

    #[test]
    fn toolr_types_via_module_import() {
        let src = "import toolr.types\ndef f(x: toolr.types.AbsolutePath): pass\n";
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let imports = TypeImports::from_module(&m);
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &imports).unwrap(),
            SupportedType::AbsolutePath
        );
    }

    #[test]
    fn unknown_dotted_name_errors_with_pointer_to_toolr_types() {
        let (_, ann) = first_annotation("def f(x: datetime.datetime): pass\n");
        let err =
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("datetime.datetime"), "msg was: {msg}");
        assert!(msg.contains("toolr.types"), "msg was: {msg}");
    }

    #[test]
    fn list_of_int_resolves() {
        let (_, ann) = first_annotation("def f(x: list[int]): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap(),
            SupportedType::List(Box::new(SupportedType::Int))
        );
    }

    #[test]
    fn tuple_str_int_resolves_heterogeneous() {
        let (_, ann) = first_annotation("def f(x: tuple[str, int]): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap(),
            SupportedType::Tuple(vec![SupportedType::Str, SupportedType::Int])
        );
    }

    #[test]
    fn literal_resolves_string_values() {
        let (_, ann) = first_annotation(
            "from typing import Literal\ndef f(x: Literal[\"a\", \"b\"]): pass\n",
        );
        let SupportedType::Literal(values) =
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap()
        else {
            panic!("expected Literal");
        };
        assert_eq!(values, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn optional_via_bin_or_with_none() {
        let (_, ann) = first_annotation("def f(x: int | None): pass\n");
        assert_eq!(
            resolve(&ann, &EnumTable::default(), &TypeImports::default()).unwrap(),
            SupportedType::Optional(Box::new(SupportedType::Int))
        );
    }

    /// Pin every `toolr.types.X` name the rust side knows about.
    ///
    /// The python-side companion lives at `tests/test_types_module.py`
    /// (`EXPECTED_TOOLR_TYPES_NAMES`). Anything added in one place
    /// without the other will break this test or its python twin,
    /// so the public surface can't silently drift.
    #[test]
    fn toolr_types_names_match_python_surface() {
        let names = [
            "AbsolutePath",
            "Date",
            "DateTime",
            "Email",
            "IPv4",
            "IPv6",
            "ResolvedPath",
            "Time",
            "UUID",
        ];
        for name in names {
            assert!(
                resolve_toolr_types_name(name).is_ok(),
                "rust resolver doesn't know about `toolr.types.{name}` — \
                 add it to `resolve_toolr_types_name` or remove it from \
                 the EXPECTED_TOOLR_TYPES_NAMES list in \
                 tests/test_types_module.py"
            );
        }
        // Anything else should be rejected.
        for spurious in ["NotARealType", "Foo", "AbsolutePath2"] {
            assert!(
                resolve_toolr_types_name(spurious).is_err(),
                "rust resolver unexpectedly accepted `toolr.types.{spurious}`"
            );
        }
    }

    #[test]
    fn enum_subclass_resolves_via_table() {
        let src = "from enum import StrEnum\n\nclass Mode(StrEnum):\n    FAST = \"fast\"\n    SLOW = \"slow\"\n\ndef f(x: Mode): pass\n";
        let m = module(src);
        let (_, ann) = first_annotation(src);
        let mut enums = EnumTable::default();
        enums.merge(EnumTable::from_module(&m));
        let resolved = resolve(&ann, &enums, &TypeImports::default()).unwrap();
        assert_eq!(
            resolved,
            SupportedType::Enum {
                name: "Mode".into(),
                values: vec!["fast".into(), "slow".into()],
            }
        );
    }
}
