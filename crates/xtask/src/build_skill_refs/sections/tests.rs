use super::*;

fn missing_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("xtask-sections-missing-{}", std::process::id()))
}

#[test]
fn arguments_reference_names_the_section_it_failed_to_extract() {
    let err = format!("{:#}", arguments_reference(&missing_root()).err().unwrap());
    assert!(
        err.contains("extracting `arg-shapes` from docs/writing-commands/arguments.md"),
        "{err}"
    );
}

#[test]
fn external_sources_reference_names_the_section_it_failed_to_extract() {
    let err = format!(
        "{:#}",
        external_sources_reference(&missing_root()).err().unwrap()
    );
    assert!(
        err.contains(
            "extracting `external-sources` from docs/writing-commands/external-sources.md"
        ),
        "{err}"
    );
}

#[test]
fn prek_hook_references_name_the_section_they_failed_to_extract() {
    let err = format!("{:#}", prek_hook_references(&missing_root()).err().unwrap());
    assert!(
        err.contains("extracting `prek-hook` from docs/third-party.md"),
        "{err}"
    );
}

#[test]
fn packaging_example_names_the_missing_source() {
    let err = format!("{:#}", packaging_example(&missing_root()).err().unwrap());
    assert!(
        err.contains("examples/plugin-package/pyproject.toml"),
        "{err}"
    );
}
