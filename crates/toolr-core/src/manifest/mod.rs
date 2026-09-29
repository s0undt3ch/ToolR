//! Toolr command manifest data model and IO.

pub mod io;
pub mod model;
mod schema;

pub use io::{ManifestError, load_manifest, write_manifest};
pub use model::{
    ArgMetadata, Argument, ArgumentKind, Command, Group, HelpSection, Manifest, Nargs, Origin,
    FRAGMENT_SHAPE_SCHEMA, MIN_READABLE_FRAGMENT_SCHEMA, SCHEMA_VERSION,
};

#[cfg(test)]
mod tests;
