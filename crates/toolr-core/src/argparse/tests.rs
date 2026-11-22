use super::*;
use std::collections::HashMap;

#[test]
fn run_for_project_returns_grafted_children() {
    let project = tempfile::tempdir().unwrap();
    let tools = project.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let cmds = project.path().join("apps/billing/management/commands");
    std::fs::create_dir_all(&cmds).unwrap();
    std::fs::write(
        cmds.join("sync.py"),
        "def add_arguments(self, parser):\n    parser.add_argument('--force', action='store_true')\n",
    )
    .unwrap();
    std::fs::write(
        tools.join("pyproject.toml"),
        r#"
                [tool.toolr.argparse.django]
                scan_paths = ["apps/*/management/commands/*.py"]

                [[tool.toolr.argparse.django.attach]]
                parent = "django"
            "#,
    )
    .unwrap();

    let mut parents = HashMap::new();
    parents.insert(
        "django".to_string(),
        ("tools.dispatcher".to_string(), "django".to_string()),
    );

    let result = run_for_project(project.path(), &parents).unwrap();
    let django_children = result.children_by_parent.get("django").unwrap();
    assert_eq!(django_children.len(), 1);
    assert_eq!(django_children[0].name, "sync");
    assert_eq!(
        django_children[0].dispatched_from.as_deref(),
        Some("argparse:django"),
    );

    // `django` received grafted children, so it's in `dispatchers`.
    assert!(result.dispatchers.contains("django"));
}
