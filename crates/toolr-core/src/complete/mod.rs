//! Shell-completion engine.
//!
//! Backs the hidden `toolr __complete <cwd> <args...>` endpoint that
//! shell completion scripts shell out to on every Tab press. The engine
//! is split into three concerns:
//!
//! 1. [`serve_completions`] — pure prefix-matching against a loaded
//!    `Manifest`. No I/O.
//! 2. [`resolve_manifest_at_tab`] — Tab-time freshness check that loads
//!    the cached manifest, compares its `static_hash` against the live
//!    `tools/**/*.py` hash, and either returns the cached manifest or a
//!    fresh one built by [`crate::parser::build_static_manifest`].
//! 3. [`scripts`] — embedded shell-completion scripts (bash, zsh, fish).

pub mod engine;
pub mod freshness;
pub mod install;
pub mod scripts;

pub use engine::serve_completions;
pub use freshness::{ResolvedManifest, resolve_manifest_at_tab};
pub use install::{InstallOptions, InstallOutcome, PriorState, install_path_for, install_script};
pub use scripts::{Shell, completion_script};

#[cfg(test)]
mod tests;
