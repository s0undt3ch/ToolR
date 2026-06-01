//! Missing-dependency diagnostics — pre-flight check that a command's
//! declared `imports` resolve before we spawn the Python runner.
//!
//! Filesystem-only and intentionally narrow: for each import we check
//! whether its top-level segment (`requests` for `requests.adapters`)
//! exists in the toolr venv's `site-packages/` as any shape Python's
//! importer would resolve — a regular package (`name/__init__.py`),
//! a single-file module (`name.py`), a C-extension shared library
//! (`name.so` / `name.pyd` / `name.<abi-tag>.so`), or a PEP 420
//! namespace package (a bare `name/` directory). No subprocess; a
//! missing dep is caught in milliseconds with a styled error and an
//! actionable "run `toolr project venv sync`" hint instead of a raw
//! Python `ModuleNotFoundError` traceback.
//!
//! Caveat — the pre-flight only checks the `imports` list the static
//! parser recorded from the command's source. Transitive imports
//! (e.g. a declared `import yaml` that itself imports a missing
//! package) are caught at runtime by the Python runner, which adds
//! the same styled hint to its own stderr when it sees an
//! `ImportError`.

pub mod preflight;
pub mod probe;

pub use preflight::{MissingDeps, check_imports};
pub use probe::{ProbeOutcome, probe_module, site_packages_dir};

#[cfg(test)]
mod tests;
