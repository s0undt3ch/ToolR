//! Toolr command manifest data model and IO.

pub mod model;

pub use model::{Argument, ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION};

#[cfg(test)]
mod tests;
