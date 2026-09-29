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

use toolr_core::parser::SupportedType;

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
    // Path / Email types carry shell-completion hints derived from the
    // type itself (a `DirectoryPath` completes directories, a `FilePath` files).
    let arg = match path_rule(inner) {
        Some((_, check)) => arg.value_hint(path_hint(check)),
        None if matches!(inner, SupportedType::Email) => arg.value_hint(ValueHint::EmailAddress),
        None => arg,
    };
    match inner {
        SupportedType::Int => arg.value_parser(clap::value_parser!(i64)),
        SupportedType::Float => arg.value_parser(clap::value_parser!(f64)),
        SupportedType::Bool => arg.value_parser(clap::value_parser!(bool)),
        SupportedType::Str => arg,
        SupportedType::Path
        | SupportedType::AbsolutePath
        | SupportedType::NewPath
        | SupportedType::ResolvedPath
        | SupportedType::FilePath
        | SupportedType::DirectoryPath
        | SupportedType::ExecutablePath
        | SupportedType::WritableDirectoryPath => {
            let (form, check) = path_rule(inner).expect("path variant has a rule");
            arg.value_parser(path_parser(form, check))
        }
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
        SupportedType::List(elem) => apply_value_parser(arg, elem),
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

/// How a path type shapes the value it hands to Python.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathForm {
    AsTyped,
    Absolute,
    Canonical,
}

/// What a path type checks on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathCheck {
    None,
    Exists,
    File,
    Dir,
    Executable,
    WritableDir,
    New,
}

/// The form and check for a path type; `None` for non-path types. The
/// `every_path_kind_has_a_path_rule_and_no_other_kind_does` test keeps
/// this in step with `SupportedType::is_path`.
fn path_rule(ty: &SupportedType) -> Option<(PathForm, PathCheck)> {
    Some(match ty {
        SupportedType::Path => (PathForm::AsTyped, PathCheck::None),
        SupportedType::AbsolutePath => (PathForm::Absolute, PathCheck::None),
        SupportedType::NewPath => (PathForm::Absolute, PathCheck::New),
        SupportedType::ResolvedPath => (PathForm::Canonical, PathCheck::Exists),
        SupportedType::FilePath => (PathForm::Canonical, PathCheck::File),
        SupportedType::DirectoryPath => (PathForm::Canonical, PathCheck::Dir),
        SupportedType::ExecutablePath => (PathForm::Canonical, PathCheck::Executable),
        SupportedType::WritableDirectoryPath => (PathForm::Canonical, PathCheck::WritableDir),
        _ => return None,
    })
}

fn path_hint(check: PathCheck) -> ValueHint {
    match check {
        PathCheck::File => ValueHint::FilePath,
        PathCheck::Dir | PathCheck::WritableDir => ValueHint::DirPath,
        PathCheck::Executable => ValueHint::ExecutablePath,
        PathCheck::None | PathCheck::Exists | PathCheck::New => ValueHint::AnyPath,
    }
}

/// Error messages name the path as the user typed it.
fn path_parser(form: PathForm, check: PathCheck) -> ValueParser {
    ValueParser::new(move |s: &str| -> Result<PathBuf, String> {
        let typed = std::path::Path::new(s);
        let path = match form {
            PathForm::AsTyped => typed.to_path_buf(),
            PathForm::Absolute => absolutise(typed)?,
            PathForm::Canonical => {
                if !typed
                    .try_exists()
                    .map_err(|e| format!("invalid path `{s}`: {e}"))?
                {
                    return Err(format!("path does not exist: {s}"));
                }
                // `dunce` drops Windows' `\\?\` verbatim prefix when the plain
                // form is valid; elsewhere it is `std::fs::canonicalize`.
                dunce::canonicalize(typed).map_err(|e| format!("invalid path `{s}`: {e}"))?
            }
        };
        check_path(&path, check, s)?;
        Ok(path)
    })
}

fn absolutise(path: &std::path::Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|e| format!("could not resolve cwd: {e}"))?;
    Ok(cwd.join(path))
}

fn check_path(path: &std::path::Path, check: PathCheck, typed: &str) -> Result<(), String> {
    let require = |ok: bool, msg: &str| if ok { Ok(()) } else { Err(format!("{msg}: {typed}")) };
    match check {
        PathCheck::None => Ok(()),
        PathCheck::Exists => require(
            path.try_exists()
                .map_err(|e| format!("invalid path `{typed}`: {e}"))?,
            "path does not exist",
        ),
        PathCheck::File => require(path.is_file(), "path is not a regular file"),
        PathCheck::Dir => require(path.is_dir(), "path is not a directory"),
        PathCheck::Executable => {
            require(path.is_file(), "path is not a regular file")?;
            require(is_executable(path), "path is not executable")
        }
        PathCheck::WritableDir => {
            require(path.is_dir(), "path is not a directory")?;
            require(is_writable_dir(path), "directory is not writable")
        }
        PathCheck::New => {
            // `symlink_metadata` so a dangling symlink counts as existing:
            // writing through it would create its target.
            match path.symlink_metadata() {
                Ok(_) => return Err(format!("path already exists: {typed}")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("invalid path `{typed}`: {e}")),
            }
            match path.parent() {
                Some(parent) if !parent.is_dir() => Err(format!(
                    "parent directory does not exist: {}",
                    parent.display()
                )),
                _ => Ok(()),
            }
        }
    }
}

#[cfg(unix)]
fn access_ok(path: &std::path::Path, mode: libc::c_int) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is NUL-terminated and outlives the call.
    unsafe { libc::access(c_path.as_ptr(), mode) == 0 }
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    access_ok(path, libc::X_OK)
}

#[cfg(unix)]
fn is_writable_dir(path: &std::path::Path) -> bool {
    access_ok(path, libc::W_OK)
}

#[cfg(windows)]
fn is_executable(path: &std::path::Path) -> bool {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    exts.split(';')
        .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(ext))
}

// Directory ACLs, not the read-only attribute, decide writability on Windows.
#[cfg(windows)]
fn is_writable_dir(path: &std::path::Path) -> bool {
    tempfile::tempfile_in(path).is_ok()
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
        Command::new("test").arg(apply_value_parser(Arg::new("v").long("v"), ty))
    }

    use std::fs;
    use tempfile::TempDir;
    use toolr_core::parser::types::SupportedTypeKind;

    fn parse(ty: &SupportedType, value: &str) -> Result<PathBuf, String> {
        build_command_with(ty)
            .try_get_matches_from(["test", "--v", value])
            .map(|m| m.get_one::<PathBuf>("v").unwrap().clone())
            .map_err(|e| e.to_string())
    }

    fn s(p: &std::path::Path) -> &str {
        p.to_str().unwrap()
    }

    #[test]
    fn every_path_kind_has_a_path_rule_and_no_other_kind_does() {
        for kind in SupportedTypeKind::ALL {
            let ty = kind.representative();
            assert_eq!(ty.is_path(), path_rule(&ty).is_some(), "{kind:?}");
        }
    }

    #[test]
    fn file_path_canonicalises_dot_dot_segments() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        fs::write(tmp.path().join("f.txt"), "x").unwrap();
        let typed = tmp.path().join("sub").join("..").join("f.txt");
        let got = parse(&SupportedType::FilePath, s(&typed)).unwrap();
        assert_eq!(got, dunce::canonicalize(tmp.path().join("f.txt")).unwrap());
    }

    #[test]
    fn file_path_rejects_a_directory_naming_the_typed_path() {
        let tmp = TempDir::new().unwrap();
        let err = parse(&SupportedType::FilePath, s(tmp.path())).unwrap_err();
        assert!(
            err.contains(&format!("path is not a regular file: {}", s(tmp.path()))),
            "got: {err}"
        );
    }

    #[test]
    fn canonical_types_reject_a_missing_path_as_does_not_exist() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("missing");
        for ty in [
            SupportedType::ResolvedPath,
            SupportedType::FilePath,
            SupportedType::DirectoryPath,
            SupportedType::ExecutablePath,
            SupportedType::WritableDirectoryPath,
        ] {
            let err = parse(&ty, s(&missing)).unwrap_err();
            assert!(
                err.contains(&format!("path does not exist: {}", s(&missing))),
                "{ty:?} got: {err}"
            );
        }
    }

    #[test]
    fn directory_path_rejects_a_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        let err = parse(&SupportedType::DirectoryPath, s(&file)).unwrap_err();
        assert!(
            err.contains(&format!("path is not a directory: {}", s(&file))),
            "got: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_path_accepts_a_symlink_to_a_directory_and_returns_the_target() {
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let got = parse(&SupportedType::DirectoryPath, s(&link)).unwrap();
        assert_eq!(got, dunce::canonicalize(real).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn executable_path_follows_the_mode_bits() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let tool = tmp.path().join("tool");
        fs::write(&tool, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o644)).unwrap();
        let err = parse(&SupportedType::ExecutablePath, s(&tool)).unwrap_err();
        assert!(
            err.contains(&format!("path is not executable: {}", s(&tool))),
            "got: {err}"
        );
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(parse(&SupportedType::ExecutablePath, s(&tool)).is_ok());
    }

    #[test]
    fn executable_path_rejects_a_directory_as_not_a_file() {
        let tmp = TempDir::new().unwrap();
        let err = parse(&SupportedType::ExecutablePath, s(tmp.path())).unwrap_err();
        assert!(err.contains("path is not a regular file"), "got: {err}");
    }

    #[test]
    fn writable_directory_path_accepts_a_fresh_temp_dir() {
        let tmp = TempDir::new().unwrap();
        let got = parse(&SupportedType::WritableDirectoryPath, s(tmp.path())).unwrap();
        assert_eq!(got, dunce::canonicalize(tmp.path()).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn writable_directory_path_rejects_a_read_only_directory() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let ro = tmp.path().join("ro");
        fs::create_dir(&ro).unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
        let got = parse(&SupportedType::WritableDirectoryPath, s(&ro));
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
        if !running_as_root() {
            let err = got.unwrap_err();
            assert!(
                err.contains(&format!("directory is not writable: {}", s(&ro))),
                "got: {err}"
            );
        }
    }

    #[test]
    fn new_path_accepts_a_missing_file_in_an_existing_dir() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("out.txt");
        assert_eq!(parse(&SupportedType::NewPath, s(&target)).unwrap(), target);
    }

    #[test]
    fn new_path_absolutises_a_relative_path() {
        let got = parse(&SupportedType::NewPath, "not-here-4f1c2a.txt").unwrap();
        assert!(got.is_absolute(), "got: {}", got.display());
        assert!(got.ends_with("not-here-4f1c2a.txt"));
    }

    #[test]
    fn new_path_rejects_an_existing_path() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        let err = parse(&SupportedType::NewPath, s(&file)).unwrap_err();
        assert!(
            err.contains(&format!("path already exists: {}", s(&file))),
            "got: {err}"
        );
    }

    #[test]
    fn new_path_rejects_a_missing_parent() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("missing").join("out.txt");
        let err = parse(&SupportedType::NewPath, s(&target)).unwrap_err();
        assert!(
            err.contains(&format!(
                "parent directory does not exist: {}",
                tmp.path().join("missing").display()
            )),
            "got: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn new_path_treats_a_dangling_symlink_as_existing() {
        let tmp = TempDir::new().unwrap();
        let link = tmp.path().join("dangling");
        std::os::unix::fs::symlink(tmp.path().join("nowhere"), &link).unwrap();
        let err = parse(&SupportedType::NewPath, s(&link)).unwrap_err();
        assert!(err.contains("path already exists"), "got: {err}");
    }

    /// root passes `access(2)` and directory permission checks regardless of
    /// mode bits, so mode-based assertions only hold for other users.
    #[cfg(unix)]
    fn running_as_root() -> bool {
        // SAFETY: `geteuid` has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    /// Makes a file inside a mode-0o000 directory, so any lookup of it fails
    /// with a permission error (except as root).
    #[cfg(unix)]
    fn unreadable_child(tmp: &TempDir) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmp.path().join("locked");
        fs::create_dir(&dir).unwrap();
        let file = dir.join("f.txt");
        fs::write(&file, "x").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).unwrap();
        (dir, file)
    }

    #[cfg(unix)]
    #[test]
    fn resolved_path_reports_a_permission_error_as_invalid_not_missing() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let (dir, file) = unreadable_child(&tmp);
        let got = parse(&SupportedType::ResolvedPath, s(&file));
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        if !running_as_root() {
            let err = got.unwrap_err();
            assert!(err.contains("invalid path"), "got: {err}");
            assert!(!err.contains("does not exist"), "got: {err}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn new_path_reports_a_permission_error_as_invalid_not_free() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let (dir, file) = unreadable_child(&tmp);
        let got = parse(&SupportedType::NewPath, s(&file));
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        if !running_as_root() {
            let err = got.unwrap_err();
            assert!(err.contains("invalid path"), "got: {err}");
        }
    }

    #[test]
    fn path_hints_follow_the_type() {
        use clap::ValueHint;
        for (ty, hint) in [
            (SupportedType::Path, ValueHint::AnyPath),
            (SupportedType::NewPath, ValueHint::AnyPath),
            (SupportedType::ResolvedPath, ValueHint::AnyPath),
            (SupportedType::FilePath, ValueHint::FilePath),
            (SupportedType::DirectoryPath, ValueHint::DirPath),
            (SupportedType::ExecutablePath, ValueHint::ExecutablePath),
            (SupportedType::WritableDirectoryPath, ValueHint::DirPath),
        ] {
            let arg = apply_value_parser(Arg::new("v").long("v"), &ty);
            assert_eq!(arg.get_value_hint(), hint, "{ty:?}");
        }
    }

    #[test]
    fn a_literal_default_naming_a_missing_file_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("missing.toml");
        let cmd = Command::new("test").arg(
            apply_value_parser(Arg::new("v").long("v"), &SupportedType::FilePath)
                .default_value(s(&missing).to_string()),
        );
        let err = cmd.try_get_matches_from(["test"]).unwrap_err();
        assert!(err.to_string().contains("path does not exist"), "got: {err}");
    }

    #[test]
    fn list_of_file_paths_checks_every_element() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        let ty = SupportedType::List(Box::new(SupportedType::FilePath));
        let cmd = Command::new("test")
            .arg(apply_value_parser(Arg::new("v").long("v"), &ty).action(clap::ArgAction::Append));
        let err = cmd
            .try_get_matches_from(["test", "--v", s(&file), "--v", s(tmp.path())])
            .unwrap_err();
        assert!(err.to_string().contains("path is not a regular file"), "got: {err}");
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

    #[cfg(windows)]
    #[test]
    fn canonical_types_hand_back_plain_paths_on_windows() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        for (ty, value) in [
            (SupportedType::ResolvedPath, tmp.path()),
            (SupportedType::FilePath, file.as_path()),
            (SupportedType::DirectoryPath, tmp.path()),
            (SupportedType::WritableDirectoryPath, tmp.path()),
        ] {
            let got = parse(&ty, s(value)).unwrap();
            let shown = got.to_string_lossy().into_owned();
            assert!(!shown.starts_with(r"\\?\"), "{ty:?} got a verbatim path: {shown}");
            assert!(got.is_absolute(), "{ty:?} got: {shown}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn file_path_on_windows_resolves_forward_slashes_and_dot_dot() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        let file = tmp.path().join("f.txt");
        fs::write(&file, "x").unwrap();
        let typed = format!("{}/sub/../f.txt", s(tmp.path()));
        let got = parse(&SupportedType::FilePath, &typed).unwrap();
        assert_eq!(got, dunce::canonicalize(&file).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_path_with_an_interior_nul_is_neither_executable_nor_writable() {
        let path = std::path::Path::new("tool\0name");
        assert!(!is_executable(path));
        assert!(!is_writable_dir(path));
    }

    #[cfg(windows)]
    #[test]
    fn executable_path_on_windows_rejects_a_file_without_an_extension() {
        let tmp = TempDir::new().unwrap();
        let tool = tmp.path().join("tool");
        fs::write(&tool, "").unwrap();
        let err = parse(&SupportedType::ExecutablePath, s(&tool)).unwrap_err();
        assert!(
            err.contains(&format!("path is not executable: {}", s(&tool))),
            "got: {err}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn executable_path_on_windows_follows_pathext() {
        let tmp = TempDir::new().unwrap();
        let exe = tmp.path().join("tool.exe");
        let txt = tmp.path().join("tool.txt");
        fs::write(&exe, "").unwrap();
        fs::write(&txt, "").unwrap();
        assert!(parse(&SupportedType::ExecutablePath, s(&exe)).is_ok());
        let err = parse(&SupportedType::ExecutablePath, s(&txt)).unwrap_err();
        assert!(
            err.contains(&format!("path is not executable: {}", s(&txt))),
            "got: {err}"
        );
    }
}
