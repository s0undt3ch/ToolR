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
use clap::builder::ValueParser;
use email_address::EmailAddress;
use uuid::Uuid;

use _rust_utils::parser::SupportedType;

/// Attach the right `value_parser` to a clap `Arg` for the given
/// supported type. `Optional(T)` is unwrapped automatically — the
/// optionality is expressed via `required=false` on the caller side.
///
/// **Wire format contract:** all the "validated complex" types
/// (DateTime, UUID, IP, Email, ...) return their value as a **String**
/// after validation. Only `int` / `float` / `bool` get typed clap
/// storage (mapped to JSON numbers / booleans on the wire). Path-flavour
/// types get clap-stored as `PathBuf` because the parser also does
/// resolution (absolutize / canonicalize) before handing the value
/// off. `extract_value` mirrors this split when reading.
pub fn apply_value_parser(arg: Arg, ty: &SupportedType) -> Arg {
    let inner = unwrap_optional(ty);
    match inner {
        SupportedType::Int => arg.value_parser(clap::value_parser!(i64)),
        SupportedType::Float => arg.value_parser(clap::value_parser!(f64)),
        SupportedType::Bool => arg.value_parser(clap::value_parser!(bool)),
        SupportedType::Str => arg,
        SupportedType::Path => arg.value_parser(clap::value_parser!(PathBuf)),
        SupportedType::AbsolutePath => arg.value_parser(absolute_path_parser()),
        SupportedType::ResolvedPath => arg.value_parser(resolved_path_parser()),
        SupportedType::DateTime => arg.value_parser(datetime_parser()),
        SupportedType::Date => arg.value_parser(date_parser()),
        SupportedType::Time => arg.value_parser(time_parser()),
        SupportedType::Uuid => arg.value_parser(uuid_parser()),
        SupportedType::Ipv4 => arg.value_parser(ipv4_parser()),
        SupportedType::Ipv6 => arg.value_parser(ipv6_parser()),
        SupportedType::Email => arg.value_parser(email_parser()),
        SupportedType::Literal(values) => arg.value_parser(values.clone()),
        SupportedType::Enum { values, .. } => arg.value_parser(values.clone()),
        // For collection kinds we configure the *element* parser; clap's
        // `num_args` / `Append` semantics are set by the caller.
        SupportedType::List(elem) => apply_value_parser(arg, elem),
        // Tuple is heterogeneous; clap doesn't easily express per-slot
        // parsers, so we leave it as the default string parser and let
        // msgspec do final coercion on the runner side.
        SupportedType::Tuple(_) => arg,
        SupportedType::Optional(_) => unreachable!("unwrap_optional handled this"),
    }
}

fn unwrap_optional(ty: &SupportedType) -> &SupportedType {
    match ty {
        SupportedType::Optional(inner) => inner.as_ref(),
        other => other,
    }
}

fn absolute_path_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<PathBuf, String> {
        let p = PathBuf::from(s);
        if p.is_absolute() {
            Ok(p)
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&p))
                .map_err(|e| format!("could not resolve cwd: {e}"))
        }
    })
}

fn resolved_path_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<PathBuf, String> {
        std::path::Path::new(s)
            .canonicalize()
            .map_err(|e| format!("invalid path `{s}`: {e}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Command;

    fn build_command_with(ty: &SupportedType) -> Command {
        Command::new("test").arg(apply_value_parser(Arg::new("v").long("v"), ty))
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
