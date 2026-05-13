//! Per-`SupportedType` clap value-parser wiring.
//!
//! Every parser here turns a CLI string into the right typed value at
//! clap parse-time, giving fast, native-feeling errors *before* a
//! Python subprocess is ever started. The runner subprocess receives
//! a typed JSON wire payload (numbers for int/float, strings for
//! everything else, with values pre-validated by these parsers).

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use clap::Arg;
use clap::ValueHint;
use clap::builder::ValueParser;
use email_address::EmailAddress;
use pep440_rs::Version as Pep440Version;
use uuid::Uuid;

use _rust_utils::parser::{PathConstraints, SupportedType};

/// Attach the right `value_parser` to a clap `Arg` for the given
/// supported type. `Optional(T)` is unwrapped automatically — the
/// optionality is expressed via `required=false` on the caller side.
/// `path_constraints` layers on top of any path-flavoured type to add
/// `must_exist` / `must_be_file` / `must_be_dir` checks.
///
/// **Wire format contract:** all the "validated complex" types
/// (DateTime, UUID, IP, Email, ...) return their value as a **String**
/// after validation. Only `int` / `float` / `bool` get typed clap
/// storage (mapped to JSON numbers / booleans on the wire). Path-flavour
/// types get clap-stored as `PathBuf` because the parser also does
/// resolution (absolutize / canonicalize) before handing the value
/// off. `extract_value` mirrors this split when reading.
pub fn apply_value_parser(
    arg: Arg,
    ty: &SupportedType,
    path_constraints: Option<&PathConstraints>,
) -> Arg {
    let inner = unwrap_optional(ty);
    let pc = path_constraints.copied().unwrap_or_default();
    // Path / Email types carry shell-completion hints derived from the
    // type itself; path constraints refine them further (must_be_dir →
    // DirPath, must_be_file → FilePath).
    let arg = match inner {
        SupportedType::Path | SupportedType::AbsolutePath | SupportedType::ResolvedPath => {
            let hint = if pc.must_be_dir {
                ValueHint::DirPath
            } else if pc.must_be_file {
                ValueHint::FilePath
            } else {
                ValueHint::AnyPath
            };
            arg.value_hint(hint)
        }
        SupportedType::Email => arg.value_hint(ValueHint::EmailAddress),
        _ => arg,
    };
    match inner {
        SupportedType::Int => arg.value_parser(clap::value_parser!(i64)),
        SupportedType::Float => arg.value_parser(clap::value_parser!(f64)),
        SupportedType::Bool => arg.value_parser(clap::value_parser!(bool)),
        SupportedType::Str => arg,
        SupportedType::Path => arg.value_parser(path_parser(false, false, pc)),
        SupportedType::AbsolutePath => arg.value_parser(path_parser(true, false, pc)),
        SupportedType::ResolvedPath => arg.value_parser(path_parser(true, true, pc)),
        SupportedType::DateTime => arg.value_parser(datetime_parser()),
        SupportedType::Date => arg.value_parser(date_parser()),
        SupportedType::Time => arg.value_parser(time_parser()),
        SupportedType::Uuid => arg.value_parser(uuid_parser()),
        SupportedType::Ipv4 => arg.value_parser(ipv4_parser()),
        SupportedType::Ipv6 => arg.value_parser(ipv6_parser()),
        SupportedType::Email => arg.value_parser(email_parser()),
        SupportedType::Version => arg.value_parser(version_parser()),
        // Count is wired via ArgAction::Count, which consumes no value
        // and stores a u8. No value_parser to set; let clap handle it.
        SupportedType::Count => arg,
        SupportedType::Literal(values) => arg.value_parser(values.clone()),
        SupportedType::Enum { values, .. } => arg.value_parser(values.clone()),
        // For collection kinds we configure the *element* parser; clap's
        // `num_args` / `Append` semantics are set by the caller.
        SupportedType::List(elem) => apply_value_parser(arg, elem, path_constraints),
        // Heterogeneous tuples: clap can't apply a per-slot value_parser
        // for the same Arg, so we constrain the *arity* and let msgspec
        // coerce each slot to the right type against the function's
        // `tuple[T1, T2, ...]` hint. Slot count comes from the resolved
        // type — see `arity_for` in the cli builder.
        SupportedType::Tuple(_) => arg,
        SupportedType::Optional(_) => unreachable!("unwrap_optional handled this"),
    }
}

/// If `ty` (after unwrapping `Optional`) is a heterogeneous `Tuple`,
/// return its slot count; otherwise `None`.
pub fn tuple_arity(ty: &SupportedType) -> Option<usize> {
    match unwrap_optional(ty) {
        SupportedType::Tuple(elts) => Some(elts.len()),
        _ => None,
    }
}

fn unwrap_optional(ty: &SupportedType) -> &SupportedType {
    match ty {
        SupportedType::Optional(inner) => inner.as_ref(),
        other => other,
    }
}

/// Build a path value-parser with three orthogonal knobs:
/// - `absolutise`: join relative paths to cwd (no fs check).
/// - `canonical`: full `canonicalize()` — symlinks resolved, must exist.
/// - `constraints`: optional `must_exist`/`must_be_file`/`must_be_dir`
///   layered on top. `must_be_file`/`must_be_dir` imply `must_exist`.
///
/// `canonical=true` already enforces existence; the constraint checks
/// then run against the resolved path. With `canonical=false` the
/// checks run against the (possibly absolutised) input as the user
/// passed it.
fn path_parser(absolutise: bool, canonical: bool, constraints: PathConstraints) -> ValueParser {
    ValueParser::new(move |s: &str| -> Result<PathBuf, String> {
        let mut path = PathBuf::from(s);
        if canonical {
            path = std::path::Path::new(s)
                .canonicalize()
                .map_err(|e| format!("invalid path `{s}`: {e}"))?;
        } else if absolutise && !path.is_absolute() {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("could not resolve cwd: {e}"))?;
            path = cwd.join(&path);
        }
        // Constraint checks. Skip when already enforced by canonical.
        if constraints.requires_existence() && !path.exists() {
            return Err(format!("path does not exist: {}", path.display()));
        }
        if constraints.must_be_file && !path.is_file() {
            return Err(format!("path is not a regular file: {}", path.display()));
        }
        if constraints.must_be_dir && !path.is_dir() {
            return Err(format!("path is not a directory: {}", path.display()));
        }
        Ok(path)
    })
}

fn datetime_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
            .map_err(|e| format!("expected RFC 3339 datetime, got `{s}`: {e}"))
    })
}

fn date_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(|d| d.format("%Y-%m-%d").to_string())
            .map_err(|e| format!("expected YYYY-MM-DD, got `{s}`: {e}"))
    })
}

fn time_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        NaiveTime::parse_from_str(s, "%H:%M:%S")
            .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S%.f"))
            .map(|t| t.format("%H:%M:%S%.f").to_string())
            .map_err(|e| format!("expected HH:MM:SS[.fff], got `{s}`: {e}"))
    })
}

fn uuid_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        Uuid::parse_str(s)
            .map(|u| u.hyphenated().to_string())
            .map_err(|e| format!("invalid UUID `{s}`: {e}"))
    })
}

fn ipv4_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        s.parse::<Ipv4Addr>()
            .map(|a| a.to_string())
            .map_err(|e| format!("invalid IPv4 `{s}`: {e}"))
    })
}

fn ipv6_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        s.parse::<Ipv6Addr>()
            .map(|a| a.to_string())
            .map_err(|e| format!("invalid IPv6 `{s}`: {e}"))
    })
}

fn email_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        EmailAddress::parse_with_options(s, email_address::Options::default())
            .map(|_| s.to_string())
            .map_err(|e| format!("invalid email `{s}`: {e}"))
    })
}

fn version_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        s.parse::<Pep440Version>()
            .map(|v| v.to_string())
            .map_err(|e| format!("invalid PEP 440 version `{s}`: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Command;

    fn build_command_with(ty: &SupportedType) -> Command {
        Command::new("test").arg(apply_value_parser(Arg::new("v").long("v"), ty, None))
    }

    fn build_command_with_constraints(ty: &SupportedType, pc: PathConstraints) -> Command {
        Command::new("test").arg(apply_value_parser(Arg::new("v").long("v"), ty, Some(&pc)))
    }

    #[test]
    fn int_parser_accepts_integer_and_rejects_letters() {
        let cmd = build_command_with(&SupportedType::Int);
        let ok = cmd
            .clone()
            .try_get_matches_from(["test", "--v", "42"])
            .unwrap();
        assert_eq!(*ok.get_one::<i64>("v").unwrap(), 42);
        let err = cmd.try_get_matches_from(["test", "--v", "abc"]).unwrap_err();
        assert!(err.to_string().contains("invalid"));
    }

    #[test]
    fn float_parser_accepts_floats() {
        let cmd = build_command_with(&SupportedType::Float);
        let ok = cmd
            .try_get_matches_from(["test", "--v", "2.5"])
            .unwrap();
        assert!((*ok.get_one::<f64>("v").unwrap() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn datetime_parser_validates_rfc3339() {
        let cmd = build_command_with(&SupportedType::DateTime);
        assert!(
            cmd.clone()
                .try_get_matches_from(["test", "--v", "2026-05-12T10:00:00Z"])
                .is_ok()
        );
        assert!(
            cmd.try_get_matches_from(["test", "--v", "not-a-date"])
                .is_err()
        );
    }

    #[test]
    fn uuid_parser_validates_hyphenated_form() {
        let cmd = build_command_with(&SupportedType::Uuid);
        assert!(
            cmd.clone()
                .try_get_matches_from(["test", "--v", "550e8400-e29b-41d4-a716-446655440000"])
                .is_ok()
        );
        assert!(cmd.try_get_matches_from(["test", "--v", "not-a-uuid"]).is_err());
    }

    #[test]
    fn ipv4_parser_validates_dotted_quad() {
        let cmd = build_command_with(&SupportedType::Ipv4);
        let ok = cmd
            .clone()
            .try_get_matches_from(["test", "--v", "10.0.0.1"])
            .unwrap();
        assert_eq!(ok.get_one::<String>("v").unwrap(), "10.0.0.1");
        assert!(cmd.try_get_matches_from(["test", "--v", "10.0.0.999"]).is_err());
    }

    #[test]
    fn ipv6_parser_validates_compressed_form() {
        let cmd = build_command_with(&SupportedType::Ipv6);
        let ok = cmd
            .clone()
            .try_get_matches_from(["test", "--v", "::1"])
            .unwrap();
        assert_eq!(ok.get_one::<String>("v").unwrap(), "::1");
        assert!(cmd.try_get_matches_from(["test", "--v", "not-ipv6"]).is_err());
    }

    #[test]
    fn email_parser_validates_addresses() {
        let cmd = build_command_with(&SupportedType::Email);
        assert!(
            cmd.clone()
                .try_get_matches_from(["test", "--v", "user@example.com"])
                .is_ok()
        );
        assert!(cmd.try_get_matches_from(["test", "--v", "not-an-email"]).is_err());
    }

    #[test]
    fn version_parser_accepts_pep440_flavours() {
        let cmd = build_command_with(&SupportedType::Version);
        // Plain semver-shaped.
        assert!(
            cmd.clone()
                .try_get_matches_from(["test", "--v", "1.2.3"])
                .is_ok()
        );
        // PEP 440 dev/post/pre + local segment.
        assert!(
            cmd.clone()
                .try_get_matches_from(["test", "--v", "1.0.dev2+local.foo"])
                .is_ok()
        );
        assert!(
            cmd.clone()
                .try_get_matches_from(["test", "--v", "1.2.0a3.post1"])
                .is_ok()
        );
        // Garbage rejected with a pointed error.
        let err = cmd
            .try_get_matches_from(["test", "--v", "not-a-version"])
            .unwrap_err();
        assert!(err.to_string().contains("PEP 440"), "got: {err}");
    }

    #[test]
    fn literal_parser_restricts_to_allowed_values() {
        let cmd = build_command_with(&SupportedType::Literal(vec![
            "a".into(),
            "b".into(),
        ]));
        assert!(cmd.clone().try_get_matches_from(["test", "--v", "a"]).is_ok());
        assert!(cmd.try_get_matches_from(["test", "--v", "c"]).is_err());
    }

    #[test]
    fn absolute_path_parser_absolutises_relative_paths() {
        let cmd = build_command_with(&SupportedType::AbsolutePath);
        let m = cmd
            .try_get_matches_from(["test", "--v", "subdir/file"])
            .unwrap();
        let got = m.get_one::<PathBuf>("v").unwrap();
        assert!(got.is_absolute(), "got: {}", got.display());
        assert!(got.ends_with("subdir/file"));
    }

    #[test]
    fn resolved_path_parser_requires_existence() {
        let cmd = build_command_with(&SupportedType::ResolvedPath);
        let tmp = std::env::temp_dir();
        let m = cmd
            .clone()
            .try_get_matches_from(["test", "--v", tmp.to_str().unwrap()])
            .unwrap();
        assert!(m.get_one::<PathBuf>("v").unwrap().is_absolute());
        assert!(
            cmd.try_get_matches_from([
                "test",
                "--v",
                "/this/path/definitely/does/not/exist/97e8f3bd",
            ])
            .is_err()
        );
    }

    #[test]
    fn path_with_must_exist_rejects_missing() {
        let pc = PathConstraints {
            must_exist: true,
            ..Default::default()
        };
        let cmd = build_command_with_constraints(&SupportedType::Path, pc);
        let err = cmd
            .try_get_matches_from(["test", "--v", "/does/not/exist/xyz123"])
            .unwrap_err();
        assert!(
            err.to_string().contains("does not exist"),
            "got: {err}"
        );
    }

    #[test]
    fn path_with_must_be_file_rejects_directory() {
        let pc = PathConstraints {
            must_be_file: true,
            ..Default::default()
        };
        let cmd = build_command_with_constraints(&SupportedType::Path, pc);
        let tmp = std::env::temp_dir();
        let err = cmd
            .try_get_matches_from(["test", "--v", tmp.to_str().unwrap()])
            .unwrap_err();
        assert!(
            err.to_string().contains("not a regular file"),
            "got: {err}"
        );
    }

    #[test]
    fn path_with_must_be_dir_accepts_directory() {
        let pc = PathConstraints {
            must_be_dir: true,
            ..Default::default()
        };
        let cmd = build_command_with_constraints(&SupportedType::Path, pc);
        let tmp = std::env::temp_dir();
        let m = cmd
            .try_get_matches_from(["test", "--v", tmp.to_str().unwrap()])
            .unwrap();
        let got = m.get_one::<PathBuf>("v").unwrap();
        assert!(got.is_dir());
    }

    #[test]
    fn optional_wrapper_is_transparent_to_the_parser() {
        let cmd = build_command_with(&SupportedType::Optional(Box::new(SupportedType::Int)));
        assert_eq!(
            *cmd.try_get_matches_from(["test", "--v", "5"])
                .unwrap()
                .get_one::<i64>("v")
                .unwrap(),
            5
        );
    }
}
