use super::*;
use crate::argparse::config::Attachment;
use crate::manifest::Origin;

fn parents_with_django() -> HashMap<String, (String, String)> {
    let mut m = HashMap::new();
    m.insert(
        "django".into(),
        ("tools.dispatcher".into(), "django".into()),
    );
    m
}

fn block(name: &str, parent: &str) -> ArgparseBlock {
    ArgparseBlock {
        name: name.into(),
        scan_paths: vec![],
        common_args: vec![],
        django: false,
        attach: vec![Attachment {
            parent: parent.into(),
        }],
    }
}

#[test]
fn unknown_parent_with_hint() {
    let err =
        validate_attachments(&[block("django", "djnago")], &parents_with_django()).unwrap_err();
    match err {
        AttachError::UnknownParent { hint, .. } => {
            assert!(
                hint.contains("django"),
                "hint did not surface the close match: {hint:?}"
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn collision_is_detected() {
    let cmd = |source: &str| Command {
        name: "migrate".into(),
        group: "django".into(),
        module: "tools.dispatcher".into(),
        function: "django".into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![],
        origin: Origin::Static,
        dispatched_from: Some(source.into()),
        is_dispatcher: false,
    };
    let children: HashMap<String, Vec<Command>> =
        HashMap::from([("django".into(), vec![cmd("a"), cmd("b")])]);
    let err = validate_no_collisions(&children).unwrap_err();
    match err {
        AttachError::Collision { name, .. } => assert_eq!(name, "migrate"),
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn dedup_intra_source_keeps_first_and_warns() {
    // Two files with the same stem (`sync.py` in two Django apps)
    // both grafted under one parent from the same source. The
    // pre-fix behaviour would let clap explode at startup. Now we
    // keep the first and warn loudly.
    let cmd = |module: &str| Command {
        name: "sync".into(),
        group: "django".into(),
        module: module.into(),
        function: "django".into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![],
        origin: Origin::Static,
        dispatched_from: Some("argparse:django".into()),
        is_dispatcher: false,
    };
    let mut children: HashMap<String, Vec<Command>> = HashMap::from([(
        "django".into(),
        vec![cmd("apps.billing"), cmd("apps.invoicing"), cmd("apps.subs")],
    )]);
    let warnings = dedup_intra_source(&mut children);
    assert_eq!(children["django"].len(), 1);
    assert_eq!(children["django"][0].module, "apps.billing", "first wins");
    assert_eq!(warnings.len(), 2, "one warning per discarded duplicate");
    for w in &warnings {
        assert!(
            w.contains("sync"),
            "warning {w:?} should name the duplicate"
        );
        assert!(w.contains("django"), "warning {w:?} should name the parent");
        assert!(
            w.contains("argparse:django"),
            "warning {w:?} should name the source"
        );
    }
}

#[test]
fn dedup_intra_source_is_a_noop_when_no_duplicates() {
    let mk = |name: &str| Command {
        name: name.into(),
        group: "django".into(),
        module: format!("apps.{name}"),
        function: "django".into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![],
        origin: Origin::Static,
        dispatched_from: Some("argparse:django".into()),
        is_dispatcher: false,
    };
    let mut children: HashMap<String, Vec<Command>> =
        HashMap::from([("django".into(), vec![mk("sync"), mk("migrate")])]);
    let warnings = dedup_intra_source(&mut children);
    assert_eq!(children["django"].len(), 2);
    assert!(warnings.is_empty());
}

#[test]
fn graft_emits_one_child_per_scanned_with_dispatched_from() {
    let block = ArgparseBlock {
        name: "django".into(),
        scan_paths: vec![],
        common_args: vec![],
        django: false,
        attach: vec![Attachment {
            parent: "django".into(),
        }],
    };
    let scanned = vec![ScannedCommand {
        name: "migrate".into(),
        summary: "Migrate".into(),
        description: "".into(),
        arguments: vec![],
        warnings: vec![],
        is_django_command_class: false,
    }];
    let children = graft_children(&block, &scanned, &parents_with_django()).unwrap();
    assert_eq!(children.len(), 1);
    let django_children = children.get("django").unwrap();
    assert_eq!(django_children[0].name, "migrate");
    assert_eq!(
        django_children[0].dispatched_from.as_deref(),
        Some("argparse:django")
    );
    assert_eq!(django_children[0].module, "tools.dispatcher");
    assert_eq!(django_children[0].function, "django");
}
