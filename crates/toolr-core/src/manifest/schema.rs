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
mod tests {
    use crate::manifest::{
        ArgMetadata, Argument, ArgumentKind, Command, FRAGMENT_SHAPE_SCHEMA, Nargs, Origin,
        SCHEMA_VERSION,
    };
    use crate::parser::types::{SupportedType as T, SupportedTypeKind as K};

    // `argument_kind_index` is exhaustive, so a new variant won't compile until it has a slot here.
    const ARGUMENT_KINDS: [ArgumentKind; 8] = [
        ArgumentKind::Positional,
        ArgumentKind::Optional,
        ArgumentKind::Flag,
        ArgumentKind::Repeated,
        ArgumentKind::VarPositional,
        ArgumentKind::Count,
        ArgumentKind::FixedArity,
        ArgumentKind::OptionalPositional,
    ];

    // `nargs_index` is exhaustive, so a new variant won't compile until it has a slot here;
    // `Fixed(1)` stands for every `Fixed(n)`.
    const NARGS: [Nargs; 3] = [Nargs::Plus, Nargs::Star, Nargs::Fixed(1)];

    // Bump the arm count and add the variant to `ARGUMENT_KINDS` and the golden table.
    const ARGUMENT_KIND_ARMS: usize = 8;
    fn argument_kind_index(k: ArgumentKind) -> usize {
        match k {
            ArgumentKind::Positional => 0,
            ArgumentKind::Optional => 1,
            ArgumentKind::Flag => 2,
            ArgumentKind::Repeated => 3,
            ArgumentKind::VarPositional => 4,
            ArgumentKind::Count => 5,
            ArgumentKind::FixedArity => 6,
            ArgumentKind::OptionalPositional => 7,
        }
    }

    // Bump the arm count and add the variant to `NARGS` and the golden table.
    const NARGS_ARMS: usize = 3;
    fn nargs_index(n: Nargs) -> usize {
        match n {
            Nargs::Plus => 0,
            Nargs::Star => 1,
            Nargs::Fixed(_) => 2,
        }
    }

    fn email_is_newer(k: K) -> u32 {
        if k == K::Email { 3 } else { 2 }
    }

    fn argument(name: &str, resolved_type: Option<T>) -> Argument {
        Argument {
            name: name.to_string(),
            kind: ArgumentKind::Optional,
            help: String::new(),
            default: None,
            type_annotation: None,
            resolved_type,
            allowed_values: vec![],
            metadata: ArgMetadata::default(),
            long_flag: None,
        }
    }

    fn command(arguments: Vec<Argument>) -> Command {
        Command {
            name: "build".to_string(),
            group: "ci".to_string(),
            module: "tools.ci".to_string(),
            function: "build".to_string(),
            summary: String::new(),
            description: String::new(),
            arguments,
            origin: Origin::ThirdParty,
            dispatched_from: None,
            is_dispatcher: false,
        }
    }

    #[test]
    fn variant_lists_cover_every_variant() {
        assert_eq!(ARGUMENT_KINDS.len(), ARGUMENT_KIND_ARMS);
        for k in ARGUMENT_KINDS {
            assert_eq!(ARGUMENT_KINDS[argument_kind_index(k)], k);
        }
        assert_eq!(NARGS.len(), NARGS_ARMS);
        for n in NARGS {
            assert_eq!(NARGS[nargs_index(n)], n);
        }
    }

    #[test]
    fn since_schema_golden_tables() {
        let got: Vec<(K, u32)> = K::ALL.iter().map(|k| (*k, k.since_schema())).collect();
        let want: Vec<(K, u32)> = vec![
            (K::Int, 2),
            (K::Float, 2),
            (K::Bool, 2),
            (K::Str, 2),
            (K::Path, 2),
            (K::AbsolutePath, 2),
            (K::NewPath, 2),
            (K::ResolvedPath, 2),
            (K::FilePath, 2),
            (K::DirectoryPath, 2),
            (K::ExecutablePath, 2),
            (K::WritableDirectoryPath, 2),
            (K::DateTime, 2),
            (K::Date, 2),
            (K::Time, 2),
            (K::Uuid, 2),
            (K::Ipv4, 2),
            (K::Ipv6, 2),
            (K::Email, 2),
            (K::Version, 2),
            (K::Count, 2),
            (K::Literal, 2),
            (K::Enum, 2),
            (K::List, 2),
            (K::Tuple, 2),
            (K::Optional, 2),
        ];
        assert_eq!(
            got, want,
            "a new SupportedTypeKind needs a SCHEMA_VERSION bump and a new golden row"
        );

        let got: Vec<(ArgumentKind, u32)> = ARGUMENT_KINDS
            .iter()
            .map(|k| (*k, k.since_schema()))
            .collect();
        let want: Vec<(ArgumentKind, u32)> = vec![
            (ArgumentKind::Positional, 2),
            (ArgumentKind::Optional, 2),
            (ArgumentKind::Flag, 2),
            (ArgumentKind::Repeated, 2),
            (ArgumentKind::VarPositional, 2),
            (ArgumentKind::Count, 2),
            (ArgumentKind::FixedArity, 2),
            (ArgumentKind::OptionalPositional, 2),
        ];
        assert_eq!(
            got, want,
            "a new ArgumentKind needs a SCHEMA_VERSION bump and a new golden row"
        );

        let got: Vec<(Nargs, u32)> = NARGS.iter().map(|n| (*n, n.since_schema())).collect();
        let want: Vec<(Nargs, u32)> =
            vec![(Nargs::Plus, 2), (Nargs::Star, 2), (Nargs::Fixed(1), 2)];
        assert_eq!(
            got, want,
            "a new Nargs variant needs a SCHEMA_VERSION bump and a new golden row"
        );
    }

    #[test]
    fn every_since_is_at_most_schema_version() {
        for k in K::ALL {
            assert!(k.since_schema() <= SCHEMA_VERSION, "{k:?}");
        }
        for k in ARGUMENT_KINDS {
            assert!(k.since_schema() <= SCHEMA_VERSION, "{k:?}");
        }
        for n in NARGS {
            assert!(n.since_schema() <= SCHEMA_VERSION, "{n:?}");
        }
        const { assert!(FRAGMENT_SHAPE_SCHEMA <= SCHEMA_VERSION) };
    }

    #[test]
    fn nested_new_kind_raises_the_minimum() {
        let ty = T::List(Box::new(T::Optional(Box::new(T::Email))));
        assert_eq!(ty.min_schema_with(&email_is_newer), 3);
        assert_eq!(
            T::List(Box::new(T::Str)).min_schema_with(&email_is_newer),
            2
        );
    }

    #[test]
    fn command_minimum_folds_over_arguments() {
        let with_email = command(vec![
            argument("first", Some(T::Str)),
            argument("second", Some(T::Tuple(vec![T::Str, T::Email]))),
        ]);
        assert_eq!(with_email.min_schema_with(&email_is_newer), 3);

        let untyped = command(vec![
            argument("first", Some(T::Str)),
            argument("second", None),
        ]);
        assert_eq!(untyped.min_schema_with(&email_is_newer), 2);
    }
}
