//! Filesystem constraints declared via `arg(must_exist=True, ...)`.
//!
//! Layered on top of `Path` / `AbsolutePath` / `ResolvedPath` parameters,
//! these flags tell the CLI surface what disk checks to apply when
//! parsing a path value. They're extracted from `Annotated[Path,
//! arg(must_exist=True, must_be_file=True, ...)]` declarations.

use ruff_python_ast::Expr;
use serde::{Deserialize, Serialize};

use super::is_toolr_arg_call;

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
