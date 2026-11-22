use super::*;
use clap::Command;

fn build_command_with(ty: &SupportedType) -> Command {
    Command::new("test").arg(apply_value_parser(Arg::new("v").long("v"), ty))
}

fn parse_string(ty: &SupportedType, value: &str) -> Result<String, clap::Error> {
    build_command_with(ty)
        .try_get_matches_from(["test", "--v", value])
        .map(|m| m.get_one::<String>("v").unwrap().clone())
}

#[test]
fn datetime_parser_normalises_to_utc_offset() {
    for (input, expected) in [
        ("2026-05-11T14:00:00+02:00", "2026-05-11T12:00:00+00:00"),
        ("2026-05-11T12:00:00Z", "2026-05-11T12:00:00+00:00"),
    ] {
        assert_eq!(
            parse_string(&SupportedType::DateTime, input).unwrap(),
            expected
        );
    }
}

#[test]
fn datetime_parser_keeps_fractional_seconds() {
    assert_eq!(
        parse_string(&SupportedType::DateTime, "2026-05-11T12:00:00.123456789Z").unwrap(),
        "2026-05-11T12:00:00.123456789+00:00"
    );
}

#[test]
fn datetime_parser_accepts_what_chrono_accepted() {
    for (input, expected) in [
        ("2026-05-11t12:00:00z", "2026-05-11T12:00:00+00:00"),
        ("2026-05-11 12:00:00Z", "2026-05-11T12:00:00+00:00"),
        ("2026-05-11T12:00:00-00:00", "2026-05-11T12:00:00+00:00"),
        ("2026-05-11T12:00:00+23:59", "2026-05-10T12:01:00+00:00"),
        // U+2212 MINUS SIGN as the offset sign
        (
            "2026-05-11T12:00:00\u{2212}02:00",
            "2026-05-11T14:00:00+00:00",
        ),
        ("2026-05-11T12:00:00.5+05:30", "2026-05-11T06:30:00.5+00:00"),
        ("9999-12-31T23:59:59Z", "9999-12-31T23:59:59+00:00"),
        ("9999-12-30T23:59:59Z", "9999-12-30T23:59:59+00:00"),
        (
            "9999-12-31T23:59:59.999999999Z",
            "9999-12-31T23:59:59.999999999+00:00",
        ),
        ("0001-01-01T00:00:00Z", "0001-01-01T00:00:00+00:00"),
    ] {
        assert_eq!(
            parse_string(&SupportedType::DateTime, input).unwrap(),
            expected,
            "{input}"
        );
    }
}

#[test]
fn datetime_parser_rejects_what_chrono_rejected_or_python_cannot_read() {
    for input in [
        "2026-05-11T14:00:00+02",
        "2026-05-11T14:00:00+0200",
        "2026-05-11T12:00:00,5Z",
        "2026-05-11T12:00Z",
        "2026-05-11T12Z",
        "20260511T120000Z",
        "2026-05-11T12:00:00+00:00:30",
        "2026-05-11T12:00:00+24:00",
        "2026-05-11T12:00:00+25:00",
        "2026-05-11T12:00:00+00:60",
        "2026-05-11T12:00:00.Z",
        "2026-05-11T12:00:00",
        "+002026-05-11T12:00:00Z",
        "2026-05-11T12:00:00+02:00[Europe/Paris]",
        "2026-05-11T12:00:00Z[UTC]",
        " 2026-05-11T12:00:00Z",
        "2026-05-11T12:00:00Z ",
        "2026-02-30T12:00:00Z",
        "2026-05-11T24:00:00Z",
        "2026-05-11T12:00:61Z",
        // before year 1 or after year 9999 once converted to UTC
        "0001-01-01T00:00:00+00:01",
        "0000-12-31T23:59:59Z",
        "9999-12-31T23:59:59-00:01",
        "",
    ] {
        let err = parse_string(&SupportedType::DateTime, input).unwrap_err();
        assert!(
            err.to_string().contains("expected RFC 3339 datetime, got"),
            "{input:?}"
        );
    }
}

#[test]
fn datetime_parser_clamps_leap_second() {
    assert_eq!(
        parse_string(&SupportedType::DateTime, "2026-12-31T23:59:60Z").unwrap(),
        "2026-12-31T23:59:59+00:00"
    );
}

#[test]
fn datetime_and_time_parsers_reject_more_than_nine_fraction_digits() {
    for (ty, input) in [
        (SupportedType::DateTime, "2026-05-11T12:00:00.1234567891Z"),
        (SupportedType::Time, "12:34:56.1234567891"),
    ] {
        assert!(parse_string(&ty, input).is_err(), "{input}");
    }
}

#[test]
fn datetime_parser_names_the_range_when_the_utc_year_overflows() {
    for input in ["9999-12-31T23:59:59-01:00", "0000-12-31T23:59:59Z"] {
        let err = parse_string(&SupportedType::DateTime, input).unwrap_err();
        assert!(
            err.to_string().contains("year is outside 0001..=9999"),
            "{input}"
        );
    }
}

#[test]
fn datetime_parser_rejects_garbage() {
    let err = parse_string(&SupportedType::DateTime, "yesterday").unwrap_err();
    assert!(
        err.to_string()
            .contains("expected RFC 3339 datetime, got `yesterday`")
    );
}

#[test]
fn date_parser_normalises_and_rejects_invalid() {
    assert_eq!(
        parse_string(&SupportedType::Date, "2026-1-5").unwrap(),
        "2026-01-05"
    );
    for bad in ["2026-02-30", "not-a-date"] {
        let err = parse_string(&SupportedType::Date, bad).unwrap_err();
        assert!(err.to_string().contains("expected YYYY-MM-DD"), "{bad}");
    }
}

#[test]
fn time_parser_accepts_whole_and_fractional_seconds() {
    for (input, expected) in [
        ("12:34:56", "12:34:56"),
        ("1:2:3", "01:02:03"),
        ("12:34:56.5", "12:34:56.5"),
    ] {
        assert_eq!(parse_string(&SupportedType::Time, input).unwrap(), expected);
    }
    let err = parse_string(&SupportedType::Time, "24:00:00").unwrap_err();
    assert!(err.to_string().contains("expected HH:MM:SS[.fff]"));
}

#[test]
fn date_and_time_parsers_skip_leading_unicode_whitespace_like_chrono() {
    for (ty, input, expected) in [
        (SupportedType::Date, "\u{a0}2026-01-05", "2026-01-05"),
        (SupportedType::Date, " \t2026-01-05", "2026-01-05"),
        (SupportedType::Time, "\u{a0}12:34:56", "12:34:56"),
        (SupportedType::Time, "\u{2003}12:34:56.5", "12:34:56.5"),
    ] {
        assert_eq!(parse_string(&ty, input).unwrap(), expected, "{input:?}");
    }
    for (ty, input) in [
        (SupportedType::Date, "2026-01-05\u{a0}"),
        (SupportedType::Time, "12:34:56 "),
    ] {
        assert!(parse_string(&ty, input).is_err(), "{input:?}");
    }
}

#[test]
fn date_parser_rejects_years_beyond_four_digits() {
    for input in ["+10000-01-05", "-10000-01-01"] {
        assert!(
            parse_string(&SupportedType::Date, input).is_err(),
            "{input}"
        );
    }
}

#[test]
fn time_parser_clamps_leap_second() {
    assert_eq!(
        parse_string(&SupportedType::Time, "12:34:60").unwrap(),
        "12:34:59"
    );
}

#[test]
fn bool_takes_the_shared_spellings_and_rejects_the_rest() {
    let cmd = build_command_with(&SupportedType::Bool);
    for (value, expected) in [
        ("1", true),
        ("YES", true),
        ("true", true),
        ("0", false),
        ("no", false),
        ("false", false),
    ] {
        let m = cmd
            .clone()
            .try_get_matches_from(["test", "--v", value])
            .unwrap();
        assert_eq!(m.get_one::<bool>("v"), Some(&expected), "{value}");
    }
    let err = cmd
        .clone()
        .try_get_matches_from(["test", "--v", "maybe"])
        .unwrap_err();
    assert!(
        err.to_string().contains("expected one of 1, true, yes"),
        "{err}"
    );
}

#[test]
fn literal_and_enum_choices_drop_control_char_values() {
    let values = vec!["ok".to_string(), "bad\x1b[2J".to_string()];
    let literal = SupportedType::Literal(values.clone());
    let err = build_command_with(&literal)
        .try_get_matches_from(["test", "--v", "bad\x1b[2J"])
        .unwrap_err();
    assert!(!err.to_string().contains('\x1b'), "{err}");
    let enumeration = SupportedType::Enum {
        name: "E".into(),
        module: "m".into(),
        values,
    };
    assert!(
        build_command_with(&enumeration)
            .try_get_matches_from(["test", "--v", "ok"])
            .is_ok()
    );
}

use std::fs;
use tempfile::TempDir;
use toolr_core::manifest::ArgumentKind;
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
        (SupportedType::NewPath, ValueHint::DirPath),
        (SupportedType::AbsolutePath, ValueHint::AnyPath),
        (
            SupportedType::Optional(Box::new(SupportedType::FilePath)),
            ValueHint::FilePath,
        ),
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
    assert!(
        err.to_string().contains("path does not exist"),
        "got: {err}"
    );
}

fn deferred(ty: SupportedType) -> DeferredPath {
    deferred_path(&optional_arg(ty, Some("rel"))).unwrap()
}

#[test]
fn deferred_path_resolves_against_the_given_base() {
    let base = TempDir::new().unwrap();
    fs::create_dir(base.path().join("tools")).unwrap();
    fs::write(base.path().join("tools").join("a.toml"), "x").unwrap();
    let got = deferred(SupportedType::FilePath).resolve(Path::new("tools/a.toml"), base.path());
    assert_eq!(
        got,
        Ok(dunce::canonicalize(base.path().join("tools").join("a.toml")).unwrap())
    );
}

#[test]
fn deferred_path_errors_name_the_raw_value() {
    let base = TempDir::new().unwrap();
    let got = deferred(SupportedType::Optional(Box::new(SupportedType::FilePath)))
        .resolve(Path::new("tools/missing.toml"), base.path());
    assert_eq!(
        got,
        Err("path does not exist: tools/missing.toml".to_string())
    );
}

#[test]
fn deferred_absolute_path_joins_base() {
    let base = TempDir::new().unwrap();
    let got = deferred(SupportedType::AbsolutePath).resolve(Path::new("build/out"), base.path());
    assert_eq!(got, Ok(base.path().join("build/out")));
}

fn optional_arg(ty: SupportedType, default: Option<&str>) -> Argument {
    Argument {
        name: "cfg".into(),
        kind: ArgumentKind::Optional,
        help: String::new(),
        default: default.map(str::to_string),
        type_annotation: None,
        resolved_type: Some(ty),
        allowed_values: vec![],
        metadata: toolr_core::manifest::ArgMetadata::default(),
        long_flag: None,
    }
}

#[test]
fn deferred_path_cases() {
    let opt = |ty| SupportedType::Optional(Box::new(ty));
    for (ty, default) in [
        (SupportedType::FilePath, "tools/pyproject.toml"),
        (opt(SupportedType::DirectoryPath), "."),
        (SupportedType::NewPath, "out"),
    ] {
        let arg = optional_arg(ty.clone(), Some(default));
        assert!(deferred_path(&arg).is_some(), "{ty:?} = {default:?}");
    }
    let absolute = std::env::temp_dir();
    for (ty, default) in [
        (SupportedType::Path, Some("tools/pyproject.toml")),
        (SupportedType::FilePath, absolute.to_str()),
        (SupportedType::FilePath, Some("")),
        (SupportedType::FilePath, Some("<expr>")),
        (SupportedType::FilePath, Some("bad\x1b[2J.toml")),
        (SupportedType::Str, Some("tools/pyproject.toml")),
        (SupportedType::FilePath, None),
    ] {
        let arg = optional_arg(ty.clone(), default);
        assert!(deferred_path(&arg).is_none(), "{ty:?} = {default:?}");
    }
    let mut positional = optional_arg(SupportedType::FilePath, Some("tools/pyproject.toml"));
    positional.kind = ArgumentKind::Positional;
    assert!(deferred_path(&positional).is_none());
    let mut untyped = optional_arg(SupportedType::FilePath, Some("tools/pyproject.toml"));
    untyped.resolved_type = None;
    assert!(deferred_path(&untyped).is_none());
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
    assert!(
        err.to_string().contains("path is not a regular file"),
        "got: {err}"
    );
}

#[test]
fn int_parser_accepts_integer_and_rejects_letters() {
    let cmd = build_command_with(&SupportedType::Int);
    let ok = cmd
        .clone()
        .try_get_matches_from(["test", "--v", "42"])
        .unwrap();
    assert_eq!(*ok.get_one::<i64>("v").unwrap(), 42);
    let err = cmd
        .try_get_matches_from(["test", "--v", "abc"])
        .unwrap_err();
    assert!(err.to_string().contains("invalid"));
}

#[test]
fn float_parser_accepts_floats() {
    let cmd = build_command_with(&SupportedType::Float);
    let ok = cmd.try_get_matches_from(["test", "--v", "2.5"]).unwrap();
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
    assert!(
        cmd.try_get_matches_from(["test", "--v", "not-a-uuid"])
            .is_err()
    );
}

#[test]
fn ipv4_parser_validates_dotted_quad() {
    let cmd = build_command_with(&SupportedType::Ipv4);
    let ok = cmd
        .clone()
        .try_get_matches_from(["test", "--v", "10.0.0.1"])
        .unwrap();
    assert_eq!(ok.get_one::<String>("v").unwrap(), "10.0.0.1");
    assert!(
        cmd.try_get_matches_from(["test", "--v", "10.0.0.999"])
            .is_err()
    );
}

#[test]
fn ipv6_parser_validates_compressed_form() {
    let cmd = build_command_with(&SupportedType::Ipv6);
    let ok = cmd
        .clone()
        .try_get_matches_from(["test", "--v", "::1"])
        .unwrap();
    assert_eq!(ok.get_one::<String>("v").unwrap(), "::1");
    assert!(
        cmd.try_get_matches_from(["test", "--v", "not-ipv6"])
            .is_err()
    );
}

#[test]
fn email_parser_validates_addresses() {
    let cmd = build_command_with(&SupportedType::Email);
    assert!(
        cmd.clone()
            .try_get_matches_from(["test", "--v", "user@example.com"])
            .is_ok()
    );
    assert!(
        cmd.try_get_matches_from(["test", "--v", "not-an-email"])
            .is_err()
    );
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
    let cmd = build_command_with(&SupportedType::Literal(vec!["a".into(), "b".into()]));
    assert!(
        cmd.clone()
            .try_get_matches_from(["test", "--v", "a"])
            .is_ok()
    );
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
        assert!(
            !shown.starts_with(r"\\?\"),
            "{ty:?} got a verbatim path: {shown}"
        );
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
