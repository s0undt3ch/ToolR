//! Toolr command manifest data model and IO.

pub mod io;
pub mod lock;
pub mod model;
mod schema;

pub use io::{ManifestError, load_manifest, write_manifest};
pub use lock::{
    LOCKS_DIR, ManifestRebuildLock, acquire_rebuild_lock, lock_path_for_repo, rebuild_if_stale,
    rebuild_lock_path,
};
pub use model::{
    ArgMetadata, Argument, ArgumentKind, Command, Content, FRAGMENT_SHAPE_SCHEMA, Group,
    HelpSection, InputRecord, InputRecords, MIN_READABLE_FRAGMENT_SCHEMA, Manifest, Nargs, Origin,
    PluginWarning, PluginWarningKind, SCHEMA_VERSION,
};

/// Where `project_root`'s manifest lives.
pub fn manifest_path(project_root: &std::path::Path) -> std::path::PathBuf {
    project_root.join("tools").join(".toolr-manifest.json")
}

#[cfg(test)]
mod tests;
