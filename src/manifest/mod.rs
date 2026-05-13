//! Toolr command manifest data model and IO.

pub mod io;
pub mod model;

pub use io::{ManifestError, load_manifest, write_manifest};
pub use model::{
    ArgMetadata, Argument, ArgumentKind, Command, Group, HelpSection, Manifest, Origin,
    SCHEMA_VERSION,
};

#[cfg(test)]
mod tests;
