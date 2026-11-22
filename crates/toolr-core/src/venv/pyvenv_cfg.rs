//! Read keys from a venv's `pyvenv.cfg`.

use std::fs;
use std::path::Path;

/// The trimmed value of the first `key = value` line for `key` in `venv_dir/pyvenv.cfg`. `None`
/// when the file or the key is missing.
pub(crate) fn pyvenv_cfg_value(venv_dir: &Path, key: &str) -> Option<String> {
    let cfg = fs::read_to_string(venv_dir.join("pyvenv.cfg")).ok()?;
    cfg.lines()
        .filter_map(|line| line.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, value)| value.trim().to_owned())
}

#[cfg(test)]
mod tests;
