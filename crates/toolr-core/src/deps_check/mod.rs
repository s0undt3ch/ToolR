//! Missing-dependency diagnostics.
//!
//! Two halves:
//!
//! - [`probe`] — filesystem-only check that a top-level import exists in
//!   a venv's `site-packages`. Used by pre-flight (Task 2).
//! - [`post_mortem`] (Task 6) — parse Python `ImportError` tracebacks
//!   off subprocess stderr and append the standard suggestion.

pub mod post_mortem;
pub mod preflight;
pub mod probe;

pub use post_mortem::{ImportErrorReport, intercept_import_error};
pub use preflight::{MissingDeps, check_imports};
pub use probe::{ProbeOutcome, probe_module, site_packages_dir};

#[cfg(test)]
mod tests;
