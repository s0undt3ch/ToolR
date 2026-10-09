//! Toolr command manifest data model and IO.

pub mod io;
pub mod lock;
pub mod model;
mod schema;

pub use io::{ManifestError, load_manifest, write_manifest};
pub use lock::{
    LOCKS_DIR, ManifestRebuildLock, acquire_rebuild_lock, rebuild_if_stale, rebuild_lock_path,
};
pub use model::{
    ArgMetadata, Argument, ArgumentKind, Command, FRAGMENT_SHAPE_SCHEMA, Group, HelpSection,
    MIN_READABLE_FRAGMENT_SCHEMA, Manifest, Nargs, Origin, PluginWarning, PluginWarningKind,
    SCHEMA_VERSION,
};

#[cfg(test)]
mod tests;
