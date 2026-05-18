//! Toolr venv cache: per-venv metadata sidecar, enumeration, pruning,
//! and passive size hints.

pub mod classify;
pub mod enumerate;
pub mod hint;
pub mod init;
pub mod meta;
pub mod touch;

pub use classify::{Candidate, Classification, PruneReason, classify_entries};
pub use enumerate::{CachedVenv, dir_size_bytes, enumerate_caches};
pub use hint::{HintConfig, compute_hint};
pub use init::write_meta_for_new_venv;
pub use meta::{Meta, MetaError, SCHEMA_VERSION};
pub use touch::touch_last_used;

#[cfg(test)]
mod tests;
