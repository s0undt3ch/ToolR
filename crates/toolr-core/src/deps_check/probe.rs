//! Filesystem-only module probe.

use std::path::{Path, PathBuf};

/// Result of probing a single top-level module name against a venv.
///
/// Every non-`Missing` variant means Python's importer would resolve
/// the name at runtime. The variants record *how* the probe found it
/// so callers can surface that in diagnostics if useful; the preflight
/// itself only cares about `Missing` vs. not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// Regular package: `<site-packages>/<module>/__init__.py`.
    Package(PathBuf),
    /// Single-file Python module: `<site-packages>/<module>.py`.
    SingleFile(PathBuf),
    /// C-extension module: `<site-packages>/<module>.so`,
    /// `<site-packages>/<module>.<abi-tag>.so`, or the Windows `.pyd`
    /// equivalent. The path points at the resolved shared library.
    Extension(PathBuf),
    /// PEP 420 namespace package: bare directory at
    /// `<site-packages>/<module>/` with no `__init__.py` but with
    /// importable contents. Python builds the package object lazily
    /// from path entries.
    NamespacePackage(PathBuf),
    /// None of the above matched.
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
/// Recognises every shape Python's importer resolves from a
/// `site-packages` entry:
///
/// - Regular package: `<module>/__init__.py`
/// - Single-file Python module: `<module>.py`
/// - C-extension module: `<module>.so` / `<module>.pyd`, including
///   ABI-tagged forms like `<module>.cpython-313-darwin.so` or
///   `<module>.abi3.so`
/// - PEP 420 namespace package: bare directory `<module>/` with no
///   `__init__.py`
///
/// Returns `Missing` only when none of the above is found. Compiled
/// `.pyc`-only distributions (no source, no extension) are not
/// recognised, but these don't occur in normal `pip`/`uv` installs.
pub fn probe_module(site_packages: &Path, module: &str) -> ProbeOutcome {
    // Defensive: a dotted import name like `a.b.c` always has its
    // top-level segment as `a`. Static parser already records only
    // top-level names, but be safe here too.
    let top = module.split('.').next().unwrap_or(module);
    if top.is_empty() {
        return ProbeOutcome::Missing;
    }

    let dir = site_packages.join(top);
    let pkg_init = dir.join("__init__.py");
    if pkg_init.is_file() {
        return ProbeOutcome::Package(pkg_init);
    }
    let single = site_packages.join(format!("{top}.py"));
    if single.is_file() {
        return ProbeOutcome::SingleFile(single);
    }
    if let Some(ext) = find_extension_module(site_packages, top) {
        return ProbeOutcome::Extension(ext);
    }
    // Namespace package falls last: a bare directory is only an
    // import target if no `__init__.py`/`.py`/extension shadowed it.
    if dir.is_dir() {
        return ProbeOutcome::NamespacePackage(dir);
    }
    ProbeOutcome::Missing
}

/// Look for a top-level C-extension module under `site_packages`.
///
/// Python's importer accepts both the bare `<top>.so`/`<top>.pyd`
/// form and ABI-tagged variants like `<top>.cpython-313-darwin.so`,
/// `<top>.cpython-313-x86_64-linux-gnu.so`, or `<top>.abi3.so`. We
/// check the bare form first (one stat, the common case) and only
/// fall back to a directory scan when needed.
fn find_extension_module(site_packages: &Path, top: &str) -> Option<PathBuf> {
    for suffix in [".so", ".pyd"] {
        let candidate = site_packages.join(format!("{top}{suffix}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let prefix = format!("{top}.");
    let entries = std::fs::read_dir(site_packages).ok()?;
    for entry in entries.flatten() {
        let raw = entry.file_name();
        let name = raw.to_string_lossy();
        if !name.starts_with(&prefix) {
            continue;
        }
        if name.ends_with(".so") || name.ends_with(".pyd") {
            return Some(entry.path());
        }
    }
    None
}
