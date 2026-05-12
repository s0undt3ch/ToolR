//! Placeholder — fully populated in Task 2.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ThirdPartyError {
    #[error("non-UTF-8 path: {0}")]
    NonUtf8Path(PathBuf),
    #[error("glob pattern error: {0}")]
    Pattern(#[from] glob::PatternError),
    #[error("glob iteration error: {0}")]
    Glob(#[from] glob::GlobError),
}

pub fn parse_fragment(
    _path: &std::path::Path,
) -> Result<super::model::ManifestFragment, ThirdPartyError> {
    unimplemented!("populated in Task 2")
}
