//! Per-`SupportedType` clap value-parser wiring.
//!
//! Every parser here turns a CLI string into the right typed value at
//! clap parse-time, giving fast, native-feeling errors *before* a
//! Python subprocess is ever started. The runner subprocess receives
//! a typed JSON wire payload (numbers for int/float, strings for
//! everything else, with values pre-validated by these parsers).

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};

use clap::Arg;
use clap::ValueHint;
use clap::builder::ValueParser;
use email_address::EmailAddress;
use jiff::SignedDuration;
use jiff::civil::DateTime;
use jiff::fmt::strtime;
use pep440_rs::Version as Pep440Version;
use uuid::Uuid;

use toolr_core::manifest::Argument;
use toolr_core::parser::{PathCompletion, SupportedType};

/// Choices clap lists in `[possible values: ...]`. Values with control or
/// invisible format characters are dropped, as the completion engine does,
/// since they would reach the terminal raw through clap's own error output.
pub fn printable_choices(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter(|v| !toolr_core::text::has_unsafe_name_chars(v))
        .cloned()
        .collect()
}

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
    let arg = match inner.path_completion() {
        Some(completion) => arg.value_hint(path_hint(completion)),
        None if matches!(inner, SupportedType::Email) => arg.value_hint(ValueHint::EmailAddress),
        None => arg,
    };
    match inner {
        SupportedType::Int => arg.value_parser(clap::value_parser!(i64)),
        SupportedType::Float => arg.value_parser(clap::value_parser!(f64)),
        SupportedType::Bool => arg.value_parser(bool_parser()),
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
        SupportedType::Literal(values) => arg.value_parser(printable_choices(values)),
        SupportedType::Enum { values, .. } => arg.value_parser(printable_choices(values)),
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

/// `1/true/yes` and `0/false/no`, the same spellings the `TOOLR_*` booleans take.
fn bool_parser() -> ValueParser {
    ValueParser::new(|value: &str| {
        toolr_core::env::parse_bool(value)
            .ok_or_else(|| "expected one of 1, true, yes, 0, false, no".to_string())
    })
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

fn path_hint(completion: PathCompletion) -> ValueHint {
    match completion {
        PathCompletion::Any => ValueHint::AnyPath,
        PathCompletion::File => ValueHint::FilePath,
        PathCompletion::Directory => ValueHint::DirPath,
        PathCompletion::Executable => ValueHint::ExecutablePath,
    }
}

/// Error messages name the path as the user typed it. Relative values
/// resolve against the process cwd.
fn path_parser(form: PathForm, check: PathCheck) -> ValueParser {
    ValueParser::new(move |s: &str| -> Result<PathBuf, String> {
        let cwd = std::env::current_dir().map_err(|e| format!("could not resolve cwd: {e}"))?;
        resolve_with(form, check, Path::new(s), &cwd)
    })
}

/// How a deferred path arg resolves: an `Optional` flag of a path type
/// that resolves on disk (every one but bare `Path`, which Python resolves
/// after the runner's chdir) with a relative literal clap default. Its
/// default must resolve from the repo root after parsing, not by clap
/// against the cwd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferredPath {
    form: PathForm,
    check: PathCheck,
}

impl DeferredPath {
    /// Resolve and check `raw` against `base` rather than the process cwd.
    /// Errors name `raw` as given, in the clap value parser's wording.
    pub fn resolve(self, raw: &Path, base: &Path) -> Result<PathBuf, String> {
        resolve_with(self.form, self.check, raw, base)
    }
}

/// The [`DeferredPath`] for `arg`, or `None` when clap resolves it as usual.
pub fn deferred_path(arg: &Argument) -> Option<DeferredPath> {
    let default = crate::cli::literal_cli_default(arg)?;
    if Path::new(default).is_absolute() {
        return None;
    }
    let (form, check) = path_rule(unwrap_optional(arg.resolved_type.as_ref()?))?;
    (form != PathForm::AsTyped).then_some(DeferredPath { form, check })
}

/// True for a path type whose value reaches Python as typed, unresolved
/// (bare `Path`): the runner's chdir decides what a relative one means.
pub fn passes_path_as_typed(ty: &SupportedType) -> bool {
    path_rule(ty).is_some_and(|(form, _)| form == PathForm::AsTyped)
}

fn resolve_with(
    form: PathForm,
    check: PathCheck,
    raw: &Path,
    base: &Path,
) -> Result<PathBuf, String> {
    let s = raw.to_string_lossy();
    let path = match form {
        PathForm::AsTyped => raw.to_path_buf(),
        PathForm::Absolute => base.join(raw),
        PathForm::Canonical => {
            let joined = base.join(raw);
            if !joined
                .try_exists()
                .map_err(|e| format!("invalid path `{s}`: {e}"))?
            {
                return Err(format!("path does not exist: {s}"));
            }
            // `dunce` drops Windows' `\\?\` verbatim prefix when the plain
            // form is valid; elsewhere it is `std::fs::canonicalize`.
            dunce::canonicalize(&joined).map_err(|e| format!("invalid path `{s}`: {e}"))?
        }
    };
    check_path(&path, check, &s)?;
    Ok(path)
}

fn check_path(path: &std::path::Path, check: PathCheck, typed: &str) -> Result<(), String> {
    let require = |ok: bool, msg: &str| {
        if ok {
            Ok(())
        } else {
            Err(format!("{msg}: {typed}"))
        }
    };
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
// The probe creates a temp file because a Win32 ACL check is far more code; it can fire
// directory watchers or fail spuriously when an antivirus holds the file.
#[cfg(windows)]
fn is_writable_dir(path: &std::path::Path) -> bool {
    tempfile::tempfile_in(path).is_ok()
}

fn datetime_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        rfc3339_to_utc(s).map_err(|e| format!("expected RFC 3339 datetime, got `{s}`: {e}"))
    })
}

const SHAPE: &str = "expected `YYYY-MM-DDTHH:MM:SS[.f]` followed by `Z` or `±HH:MM`";
const OUT_OF_RANGE: &str = "year is outside 0001..=9999";

/// RFC 3339 datetime -> UTC string `YYYY-MM-DDTHH:MM:SS[.frac]+00:00`.
///
/// Goes through `civil::DateTime`, not `Timestamp`: `Timestamp::MAX` ends about
/// 26 hours before `9999-12-31T23:59:59Z`, which is a common sentinel value.
fn rfc3339_to_utc(s: &str) -> Result<String, String> {
    let (body, offset_secs) = split_offset(s).ok_or(SHAPE)?;
    let civil = parse_civil(body).ok_or(SHAPE)?;
    let utc = civil
        .checked_sub(SignedDuration::from_secs(offset_secs))
        .map_err(|_| OUT_OF_RANGE)?;
    if !(1..=9999).contains(&utc.year()) {
        return Err(OUT_OF_RANGE.into());
    }
    Ok(format!("{utc}+00:00"))
}

/// Splits `Z`, `z`, `+HH:MM`, `-HH:MM` or `\u{2212}HH:MM` off the end.
fn split_offset(s: &str) -> Option<(&str, i64)> {
    if let Some(body) = s.strip_suffix(['Z', 'z']) {
        return Some((body, 0));
    }
    let i = s.rfind(['+', '-', '\u{2212}'])?;
    let (body, off) = s.split_at(i);
    let mut chars = off.chars();
    let sign = if chars.next()? == '+' { 1 } else { -1 };
    let off = chars.as_str().as_bytes();
    if off.len() != 5 || off[2] != b':' {
        return None;
    }
    let h = two_digits(&off[0..2])?;
    let m = two_digits(&off[3..5])?;
    (h <= 23 && m <= 59).then_some((body, sign * (h * 3600 + m * 60)))
}

fn parse_civil(b: &str) -> Option<DateTime> {
    let b = b.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let year = two_digits(&b[0..2])? * 100 + two_digits(&b[2..4])?;
    let nanos = match &b[19..] {
        [] => 0,
        [b'.', frac @ ..]
            if (1..=9).contains(&frac.len()) && frac.iter().all(u8::is_ascii_digit) =>
        {
            let n: i64 = std::str::from_utf8(frac).ok()?.parse().ok()?;
            n * 10_i64.pow(9 - frac.len() as u32)
        }
        _ => return None,
    };
    // A leap second becomes :59; chrono let :60 through and Python then crashed on it.
    let second = match two_digits(&b[17..19])? {
        60 => 59,
        s => s,
    };
    DateTime::new(
        year as i16,
        two_digits(&b[5..7])? as i8,
        two_digits(&b[8..10])? as i8,
        two_digits(&b[11..13])? as i8,
        two_digits(&b[14..16])? as i8,
        second as i8,
        nanos as i32,
    )
    .ok()
}

fn two_digits(b: &[u8]) -> Option<i64> {
    match b {
        [a @ b'0'..=b'9', c @ b'0'..=b'9'] => Some(i64::from((a - b'0') * 10 + (c - b'0'))),
        _ => None,
    }
}

fn date_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        // jiff only skips ASCII whitespace; chrono skipped any Unicode whitespace.
        strtime::parse("%Y-%m-%d", s.trim_start())
            .and_then(|t| t.to_date())
            .map(|d| d.strftime("%Y-%m-%d").to_string())
            .map_err(|e| format!("expected YYYY-MM-DD, got `{s}`: {e}"))
    })
}

fn time_parser() -> ValueParser {
    ValueParser::new(|s: &str| -> Result<String, String> {
        let trimmed = s.trim_start();
        strtime::parse("%H:%M:%S", trimmed)
            .or_else(|_| strtime::parse("%H:%M:%S%.f", trimmed))
            .and_then(|t| t.to_time())
            .map(|t| t.strftime("%H:%M:%S%.f").to_string())
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
mod tests;
