//! Environment reads, in two halves.
//!
//! Paths treat an empty value as unset. Use [`xdg_dir_env`] for `XDG_*_HOME` and
//! [`non_empty_env_path`] for other path variables (`HOME`, prefixes), which no spec requires to
//! be absolute.
//!
//! Booleans share one rule: `1`, `true` or `yes` is on, `0`, `false` or `no` is off, ignoring case
//! and surrounding whitespace. Every `TOOLR_*` boolean goes through [`env_truthy`], and bool
//! option values parse with [`parse_bool`].

use std::path::PathBuf;

/// Treats an empty value as unset, since an empty path would resolve against the cwd.
pub fn non_empty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// An `XDG_*_HOME` value counts only when it is an absolute path; the spec says to ignore relative
/// ones, as `dirs` does, so they never resolve against the cwd.
pub fn xdg_dir_env(name: &str) -> Option<PathBuf> {
    non_empty_env_path(name).filter(|p| p.is_absolute())
}

/// `1`, `true` or `yes` is true and `0`, `false` or `no` is false, ignoring case and surrounding
/// whitespace; anything else is `None`.
pub fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// Whether the boolean variable `name` is on: [`parse_bool`] says true. Unset, empty and any other
/// value are off.
pub fn env_truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| parse_bool(&v) == Some(true))
}

#[cfg(test)]
mod tests;
