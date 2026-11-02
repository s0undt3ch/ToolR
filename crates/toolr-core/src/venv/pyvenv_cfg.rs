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
mod tests {
    use super::*;

    #[test]
    fn reads_the_first_value_for_a_key() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(
            tmp.path().join("pyvenv.cfg"),
            "home = /usr/bin\nnot a pair\nversion_info = 3.13.2\nhome = /other\n",
        )
        .unwrap();
        assert_eq!(
            pyvenv_cfg_value(tmp.path(), "home").as_deref(),
            Some("/usr/bin")
        );
        assert_eq!(
            pyvenv_cfg_value(tmp.path(), "version_info").as_deref(),
            Some("3.13.2")
        );
        assert_eq!(pyvenv_cfg_value(tmp.path(), "version"), None);
        assert_eq!(pyvenv_cfg_value(&tmp.path().join("nope"), "home"), None);
    }
}
