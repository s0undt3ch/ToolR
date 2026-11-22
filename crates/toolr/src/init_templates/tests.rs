use super::*;

#[test]
fn render_pyproject_substitutes_placeholders() {
    let opts = ScaffoldOptions {
        requires_python: ">=3.13".into(),
        venv_location: VenvLocation::InTree,
        include_example: true,
    };
    let rendered = render_pyproject(&opts);
    assert!(rendered.contains(r#"requires-python = ">=3.13""#));
    assert!(rendered.contains(r#"venv-location = "in-tree""#));
    assert!(!rendered.contains("{REQUIRES_PYTHON}"));
    assert!(!rendered.contains("{VENV_LOCATION}"));
}

#[test]
fn render_all_with_example_returns_three_files() {
    let opts = ScaffoldOptions {
        requires_python: ">=3.11".into(),
        venv_location: VenvLocation::Cache,
        include_example: true,
    };
    let files = render_all(&opts);
    let names: Vec<_> = files.iter().map(|f| f.relative_path).collect();
    assert_eq!(names, vec!["pyproject.toml", ".gitignore", "example.py"]);
}

#[test]
fn render_all_without_example_returns_two_files() {
    let opts = ScaffoldOptions {
        requires_python: ">=3.11".into(),
        venv_location: VenvLocation::Cache,
        include_example: false,
    };
    let files = render_all(&opts);
    let names: Vec<_> = files.iter().map(|f| f.relative_path).collect();
    assert_eq!(names, vec!["pyproject.toml", ".gitignore"]);
}

#[test]
fn parse_venv_location_accepts_both_known_values() {
    assert_eq!(parse_venv_location("cache").unwrap(), VenvLocation::Cache);
    assert_eq!(
        parse_venv_location("in-tree").unwrap(),
        VenvLocation::InTree
    );
}

#[test]
fn parse_venv_location_rejects_unknown_values() {
    assert!(parse_venv_location("system").is_err());
}

#[test]
fn example_template_is_non_empty_and_mentions_each_command() {
    assert!(EXAMPLE_PY.contains("def hello("));
    assert!(EXAMPLE_PY.contains("def commit("));
    assert!(EXAMPLE_PY.contains("def confirm("));
    assert!(EXAMPLE_PY.contains("def setlog("));
    assert!(EXAMPLE_PY.contains("Literal["));
}
