//! The lowest reader schema each manifest shape needs. Structs are destructured without `..`, so
//! a new field won't compile until someone decides whether an old reader may drop it.

use crate::manifest::{ArgMetadata, Argument, Command, FRAGMENT_SHAPE_SCHEMA, Group, HelpSection};
use crate::parser::types::{SupportedType, SupportedTypeKind};

impl SupportedType {
    /// This type's kind's `since` schema, maxed with every inner type's, so `list[NewType]` counts.
    pub fn min_schema_with(&self, since: &impl Fn(SupportedTypeKind) -> u32) -> u32 {
        let own = since(self.kind());
        // Exhaustive, so a new container variant can't silently skip its inner types.
        let inner = match self {
            SupportedType::List(inner) | SupportedType::Optional(inner) => {
                inner.min_schema_with(since)
            }
            SupportedType::Tuple(items) => items
                .iter()
                .map(|t| t.min_schema_with(since))
                .max()
                .unwrap_or(FRAGMENT_SHAPE_SCHEMA),
            SupportedType::Str
            | SupportedType::Int
            | SupportedType::Float
            | SupportedType::Bool
            | SupportedType::Path
            | SupportedType::AbsolutePath
            | SupportedType::NewPath
            | SupportedType::ResolvedPath
            | SupportedType::FilePath
            | SupportedType::DirectoryPath
            | SupportedType::ExecutablePath
            | SupportedType::WritableDirectoryPath
            | SupportedType::DateTime
            | SupportedType::Date
            | SupportedType::Time
            | SupportedType::Uuid
            | SupportedType::Ipv4
            | SupportedType::Ipv6
            | SupportedType::Email
            | SupportedType::Version
            | SupportedType::Count
            | SupportedType::Literal(_)
            | SupportedType::Enum {
                name: _,
                module: _,
                values: _,
            } => FRAGMENT_SHAPE_SCHEMA,
        };
        FRAGMENT_SHAPE_SCHEMA.max(own).max(inner)
    }
}

impl Argument {
    /// The highest of its kind's, resolved type's and metadata's minimum schema.
    pub fn min_schema_with(&self, since: &impl Fn(SupportedTypeKind) -> u32) -> u32 {
        let Argument {
            name: _,
            kind,
            help: _,
            default: _,
            type_annotation: _,
            resolved_type,
            allowed_values: _,
            metadata,
            long_flag: _,
        } = self;
        let resolved = resolved_type
            .as_ref()
            .map_or(FRAGMENT_SHAPE_SCHEMA, |t| t.min_schema_with(since));
        FRAGMENT_SHAPE_SCHEMA
            .max(kind.since_schema())
            .max(resolved)
            .max(metadata.min_schema())
    }
}

impl ArgMetadata {
    /// The highest of its `nargs` and help section's minimum schema.
    pub fn min_schema(&self) -> u32 {
        let ArgMetadata {
            aliases: _,
            metavar: _,
            env: _,
            hide: _,
            display_order: _,
            help_section,
            conflicts_with: _,
            requires: _,
            nargs,
        } = self;
        let nargs = nargs.map_or(FRAGMENT_SHAPE_SCHEMA, |n| n.since_schema());
        let help_section = help_section
            .as_ref()
            .map_or(FRAGMENT_SHAPE_SCHEMA, HelpSection::min_schema);
        FRAGMENT_SHAPE_SCHEMA.max(nargs).max(help_section)
    }
}

impl HelpSection {
    /// Every field is ignorable by an old reader, so only the shape floor.
    pub fn min_schema(&self) -> u32 {
        let HelpSection {
            title: _,
            description: _,
        } = self;
        FRAGMENT_SHAPE_SCHEMA
    }
}

impl Group {
    /// Every field is ignorable by an old reader, so only the shape floor.
    pub fn min_schema(&self) -> u32 {
        let Group {
            name: _,
            title: _,
            description: _,
            parent: _,
            origin: _,
        } = self;
        FRAGMENT_SHAPE_SCHEMA
    }
}

impl Command {
    /// The highest minimum schema across its arguments.
    pub fn min_schema_with(&self, since: &impl Fn(SupportedTypeKind) -> u32) -> u32 {
        let Command {
            name: _,
            group: _,
            module: _,
            function: _,
            summary: _,
            description: _,
            arguments,
            origin: _,
            dispatched_from: _,
            is_dispatcher: _,
        } = self;
        arguments
            .iter()
            .map(|a| a.min_schema_with(since))
            .fold(FRAGMENT_SHAPE_SCHEMA, u32::max)
    }
}

#[cfg(test)]
mod tests;
