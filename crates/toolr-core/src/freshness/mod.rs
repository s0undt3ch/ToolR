//! Shared freshness comparison for both dispatch and tab completion.
//!
//! Both paths must answer the same question: "is the cached
//! `tools/.toolr-manifest.json` still good?" They differ only in what
//! they do with the answer (dispatch rebuilds + persists; tab
//! completion rebuilds in-memory only or, for third-party drift,
//! accepts a slightly stale completion result).
//!
//! Drift is reported on three axes — local-tools (`.py` content),
//! third-party plugin manifests, and the toolr binary's own version —
//! and collapsed into a single `FreshnessVerdict` whose variants are
//! ordered by "stronger rebuild needed."

mod compare;

#[cfg(test)]
mod tests;

pub use compare::{FreshnessVerdict, compare};
