use super::*;

#[test]
fn parse_one_block_two_attachments() {
    let toml_text = r#"
            [tool.toolr.argparse.django]
            scan_paths = ["apps/*/management/commands/*.py"]
            common_args = [
              { name = "verbosity", kind = "optional", default = "1" },
            ]

            [[tool.toolr.argparse.django.attach]]
            parent = "django"

            [[tool.toolr.argparse.django.attach]]
            parent = "jenkins.job"
        "#;
    let blocks = parse_blocks(toml_text).unwrap();
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert_eq!(block.name, "django");
    assert_eq!(block.scan_paths, vec!["apps/*/management/commands/*.py"]);
    assert_eq!(block.common_args.len(), 1);
    assert_eq!(
        block
            .attachments()
            .iter()
            .map(|a| a.parent.as_str())
            .collect::<Vec<_>>(),
        vec!["django", "jenkins.job"],
    );
}

#[test]
fn empty_table_returns_empty() {
    assert!(parse_blocks("[project]\nname = 'x'\n").unwrap().is_empty());
}
