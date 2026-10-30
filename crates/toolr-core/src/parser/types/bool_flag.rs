//! The native-command rule that a `bool` parameter is a flag defaulting
//! to `False`. argparse-scanned sources never pass through here.

use super::supported::{SupportedType, UnsupportedType};
use crate::manifest::{Argument, ArgumentKind};

/// Why a `bool`-typed parameter can't become a `--flag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoolFlagError {
    /// `name: bool = True`; `suggested` is the negated name to use instead.
    DefaultTrue { suggested: String },
    /// Keyword `bool` with no default.
    NoDefault,
    /// Positional or `*args` `bool`.
    NotKeyword,
    /// `bool` nested inside `Optional`, `list` or `tuple`.
    NotPlainFlag,
}

impl BoolFlagError {
    pub(super) fn render(
        &self,
        argument: &str,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::DefaultTrue { suggested } => write!(
                f,
                "boolean flags must default to `False`. Write `{suggested}: bool = False` \
                 (CLI: `--{cli}`) and test `if {suggested}:` in the body, or use \
                 `Literal[...]`/an `Enum` for more than two states.",
                cli = suggested.replace('_', "-"),
            ),
            Self::NoDefault => write!(
                f,
                "boolean flags must default to `False`. Write `{argument}: bool = False`."
            ),
            Self::NotKeyword => write!(
                f,
                "a `bool` parameter is a flag; give it a default of `False` and pass it by \
                 keyword (`{argument}: bool = False`)."
            ),
            Self::NotPlainFlag => write!(
                f,
                "a `bool` can only be a plain flag (`{argument}: bool = False`). Use \
                 `Literal[...]` or an `Enum` for a value with more than two states."
            ),
        }
    }
}

/// Enforce the bool-flag rule on a resolved parameter, promoting an alias
/// of `bool` (classified syntactically as `Optional`) to `Flag`.
pub(super) fn check_bool_flag(
    ty: &SupportedType,
    arg: &mut Argument,
) -> Result<(), UnsupportedType> {
    let error = match ty {
        SupportedType::Bool => {
            if arg.kind == ArgumentKind::Optional {
                arg.kind = ArgumentKind::Flag;
            }
            match (&arg.kind, arg.default.as_deref()) {
                (ArgumentKind::Positional | ArgumentKind::VarPositional, _) => {
                    BoolFlagError::NotKeyword
                }
                (_, None) => BoolFlagError::NoDefault,
                (_, Some("false")) => return Ok(()),
                (_, Some(_)) => BoolFlagError::DefaultTrue {
                    suggested: negated(&arg.name),
                },
            }
        }
        _ if contains_bool(ty) => BoolFlagError::NotPlainFlag,
        _ => return Ok(()),
    };
    Err(UnsupportedType::BoolFlag {
        argument: arg.name.clone(),
        error,
    })
}

fn contains_bool(ty: &SupportedType) -> bool {
    match ty {
        SupportedType::Bool => true,
        SupportedType::Optional(inner) | SupportedType::List(inner) => contains_bool(inner),
        SupportedType::Tuple(items) => items.iter().any(contains_bool),
        _ => false,
    }
}

fn negated(name: &str) -> String {
    match name.strip_prefix("no_") {
        Some(rest) => rest.to_string(),
        None => format!("no_{name}"),
    }
}
