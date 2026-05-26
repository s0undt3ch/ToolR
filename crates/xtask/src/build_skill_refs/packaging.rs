//! Placeholder for the packaging-skill reference generator.
//!
//! Filled in by a subsequent commit. Left empty here so the
//! generator-registry pattern in `mod.rs` stays uniform.

use std::path::Path;

use anyhow::Result;

use super::Generated;

#[allow(dead_code)]
pub fn packaging(_repo_root: &Path) -> Result<Generated> {
    unreachable!(
        "the packaging generator is not registered yet; see the upcoming \
         build_skill_refs/packaging.rs commit"
    )
}
