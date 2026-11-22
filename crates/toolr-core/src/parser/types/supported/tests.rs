use super::*;

#[test]
fn path_completion_maps_every_path_type() {
    for (ty, want) in [
        (SupportedType::Path, PathCompletion::Any),
        (SupportedType::AbsolutePath, PathCompletion::Any),
        (SupportedType::ResolvedPath, PathCompletion::Any),
        (SupportedType::NewPath, PathCompletion::Directory),
        (SupportedType::FilePath, PathCompletion::File),
        (SupportedType::ExecutablePath, PathCompletion::Executable),
        (SupportedType::DirectoryPath, PathCompletion::Directory),
        (
            SupportedType::WritableDirectoryPath,
            PathCompletion::Directory,
        ),
    ] {
        assert_eq!(ty.path_completion(), Some(want), "{ty:?}");
    }
}

#[test]
fn path_completion_unwraps_optional_and_list() {
    let optional = SupportedType::Optional(Box::new(SupportedType::FilePath));
    assert_eq!(optional.path_completion(), Some(PathCompletion::File));
    let list = SupportedType::List(Box::new(SupportedType::DirectoryPath));
    assert_eq!(list.path_completion(), Some(PathCompletion::Directory));
}

#[test]
fn path_completion_is_none_for_non_path_types() {
    // A tuple's slots complete one by one, which the engine doesn't track.
    for ty in [
        SupportedType::Str,
        SupportedType::Literal(vec!["a".into()]),
        SupportedType::Tuple(vec![SupportedType::Path]),
    ] {
        assert_eq!(ty.path_completion(), None, "{ty:?}");
    }
    let non_paths = SupportedTypeKind::ALL
        .iter()
        .map(|k| k.representative())
        .filter(|t| {
            !t.is_path() && !matches!(t, SupportedType::Optional(_) | SupportedType::List(_))
        });
    for ty in non_paths {
        assert_eq!(ty.path_completion(), None, "{ty:?}");
    }
}

#[test]
fn is_path_is_true_exactly_for_the_path_kinds() {
    let paths: Vec<SupportedTypeKind> = SupportedTypeKind::ALL
        .iter()
        .copied()
        .filter(|k| k.representative().is_path())
        .collect();
    assert_eq!(
        paths,
        [
            SupportedTypeKind::Path,
            SupportedTypeKind::AbsolutePath,
            SupportedTypeKind::NewPath,
            SupportedTypeKind::ResolvedPath,
            SupportedTypeKind::FilePath,
            SupportedTypeKind::DirectoryPath,
            SupportedTypeKind::ExecutablePath,
            SupportedTypeKind::WritableDirectoryPath,
        ]
    );
}

#[test]
fn catalogue_covers_every_toolr_types_name() {
    // Same list as `toolr_types_names_match_python_surface` in mod.rs,
    // which is itself pinned to tests/types/test_types_module.py.
    let names = [
        "AbsolutePath",
        "Count",
        "Date",
        "DateTime",
        "DirectoryPath",
        "Email",
        "ExecutablePath",
        "FilePath",
        "IPv4",
        "IPv6",
        "NewPath",
        "ResolvedPath",
        "Time",
        "UUID",
        "Version",
        "WritableDirectoryPath",
    ];
    let annotations: Vec<&str> = SupportedType::catalogue()
        .iter()
        .map(|d| d.annotation)
        .collect();
    for name in names {
        let want = format!("toolr.types.{name}");
        assert!(
            annotations.iter().any(|a| *a == want),
            "catalogue has no row for `{want}`",
        );
    }
}

#[test]
fn every_kind_has_a_representative_of_that_kind() {
    for kind in SupportedTypeKind::ALL {
        assert_eq!(kind.representative().kind(), *kind);
    }
    assert_eq!(
        SupportedType::catalogue().len(),
        SupportedTypeKind::ALL.len()
    );
}

#[test]
fn catalogue_rows_are_unique_and_complete() {
    let rows = SupportedType::catalogue();
    let mut seen = std::collections::BTreeSet::new();
    for row in &rows {
        assert!(
            seen.insert(row.annotation),
            "duplicate row {}",
            row.annotation
        );
        for field in [row.validated_by, row.wire_format, row.python_receives] {
            assert!(!field.is_empty(), "empty column in {}", row.annotation);
        }
    }
    for primitive in ["int", "float", "bool", "str", "pathlib.Path"] {
        assert!(seen.contains(primitive), "missing {primitive}");
    }
}

#[test]
fn enum_and_list_rows_carry_the_expected_note() {
    let rows = SupportedType::catalogue();
    let enum_row = rows.iter().find(|d| d.annotation == "Enum").unwrap();
    assert_eq!(enum_row.note, "subclass");
    let list_row = rows.iter().find(|d| d.annotation == "list[T]").unwrap();
    assert_eq!(list_row.note, "(T above)");
}
