//! Manifest build helpers: static third-party glob-merge + hashing.
//! (Historically also hosted a Python introspection layer, now removed —
//! toolr never executes repository code to build the manifest.)

pub mod hash;
pub mod rebuild;

pub use hash::{compute_third_party_hash, empty_third_party_hash};
pub use rebuild::{RebuildOutcome, rebuild_manifest_full};
