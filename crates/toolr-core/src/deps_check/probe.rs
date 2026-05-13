//! Filesystem-only module probe.

use std::path::{Path, PathBuf};

/// Result of probing a single top-level module name against a venv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// `<site-packages>/<module>/__init__.py` exists.
    Package(PathBuf),
    /// `<site-packages>/<module>.py` exists.
    SingleFile(PathBuf),
    /// Neither was found.
    Missing,
}

/// Locate the `site-packages` directory under a venv. Returns the first
/// match for `<venv>/lib/python*/site-packages/`. On Windows this is
/// `<venv>/Lib/site-packages/` (no `python*` segment).
pub fn site_packages_dir(venv: &Path) -> Option<PathBuf> {
    // Windows layout first — short-circuit if it matches.
    let win = venv.join("Lib").join("site-packages");
    if win.is_dir() {
        return Some(win);
    }
    // Unix layout: <venv>/lib/python<X.Y>/site-packages
    let lib = venv.join("lib");
    let entries = std::fs::read_dir(&lib).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("python") {
            continue;
        }
        let candidate = entry.path().join("site-packages");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Probe a single top-level import name against a `site-packages` dir.
///
/// **Scope.** Only checks for `<module>/__init__.py` or `<module>.py`.
/// This is the same shape Python's `importlib` finds first. It misses:
///
/// - Namespace packages (`PEP 420`): no `__init__.py`, just a bare
///   directory. These will pass at runtime but the probe returns
///   `Missing`. Falls through to post-mortem.
/// - C-extension modules shipped as `.so` / `.pyd` without a `.py`
///   sibling. Rare for the modules toolr commands import directly at
///   the top level (these are usually re-exported from a Python
///   shim package).
///
/// Both gaps are accepted: pre-flight is a fast-path, not a guarantee.
/// Post-mortem catches whatever pre-flight misses.
pub fn probe_module(site_packages: &Path, module: &str) -> ProbeOutcome {
    // Defensive: a dotted import name like `a.b.c` always has its
    // top-level segment as `a`. Static parser already records only
    // top-level names, but be safe here too.
    let top = module.split('.').next().unwrap_or(module);
    if top.is_empty() {
        return ProbeOutcome::Missing;
    }

    let pkg = site_packages.join(top).join("__init__.py");
    if pkg.is_file() {
        return ProbeOutcome::Package(pkg);
    }
    let single = site_packages.join(format!("{top}.py"));
    if single.is_file() {
        return ProbeOutcome::SingleFile(single);
    }
    ProbeOutcome::Missing
}
