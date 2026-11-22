use super::*;

fn module(src: &str) -> ModModule {
    ruff_python_parser::parse_module(src)
        .expect("valid python")
        .into_syntax()
}

fn is_overload_stmt(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::FunctionDef(def) if is_overload(def))
}

#[test]
fn find_definition_returns_implementation_over_overload_stubs() {
    let m = module(
        "from typing import overload\n\
         @overload\n\
         def f(x: int) -> int: ...\n\
         @overload\n\
         def f(x: str) -> str: ...\n\
         def f(x):\n    return x\n",
    );
    let found = find_definition(&m, "f").expect("f found");
    assert!(
        !is_overload_stmt(found),
        "should return the implementation, not an @overload stub"
    );
}

#[test]
fn find_definition_recognises_attribute_spelled_overload() {
    let m = module(
        "import typing\n\
         @typing.overload\n\
         def f(x: int) -> int: ...\n\
         def f(x):\n    return x\n",
    );
    let found = find_definition(&m, "f").expect("f found");
    assert!(!is_overload_stmt(found));
}

#[test]
fn find_definition_falls_back_to_first_when_all_overloads() {
    let m = module(
        "from typing import overload\n\
         @overload\n\
         def f(x: int) -> int: ...\n\
         @overload\n\
         def f(x: str) -> str: ...\n",
    );
    // No implementation present — fall back to the first stub rather
    // than returning nothing.
    let found = find_definition(&m, "f").expect("falls back to a stub");
    assert!(is_overload_stmt(found));
}

#[test]
fn render_class_members_includes_public_and_dunder_init_skips_private_and_overloads() {
    let src = "class Widget:\n\
         \x20   def __init__(self, name: str) -> None:\n\
         \x20       \"\"\"Set up the widget.\"\"\"\n\
         \x20   def _helper(self) -> None:\n\
         \x20       pass\n\
         \x20   @overload\n\
         \x20   def render(self, *, wide: bool) -> str: ...\n\
         \x20   def render(self) -> str:\n\
         \x20       \"\"\"Render the widget.\"\"\"\n";
    let m = module(src);
    let Stmt::ClassDef(def) = find_definition(&m, "Widget").expect("Widget found") else {
        panic!("expected a class definition");
    };
    let members = render_class_members("mod", src, &def.body).expect("renders members");
    let names: Vec<&str> = members.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["__init__", "render"], "{names:?}");
    assert_eq!(members[1].docstring.as_deref(), Some("Render the widget."));
}

#[test]
fn render_class_members_includes_public_annotated_fields_skips_private_and_unannotated() {
    let src = "class Widget(Struct, frozen=True):\n\
         \x20   name: str\n\
         \x20   size: int | None = None\n\
         \x20   _hidden: int = 0\n\
         \x20   legacy = 1\n\
         \x20   def render(self) -> str:\n\
         \x20       \"\"\"Render the widget.\"\"\"\n";
    let m = module(src);
    let Stmt::ClassDef(def) = find_definition(&m, "Widget").expect("Widget found") else {
        panic!("expected a class definition");
    };
    let members = render_class_members("mod", src, &def.body).expect("renders members");
    let names: Vec<&str> = members.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["name", "size", "render"], "{names:?}");
    assert_eq!(members[0].signature.as_deref(), Some("name: str"));
    assert_eq!(
        members[1].signature.as_deref(),
        Some("size: int | None = None")
    );
}

#[test]
fn render_class_members_skips_attrs_init_false_fields_keeps_real_constructor_field() {
    let src = "@define(slots=True, frozen=True)\n\
         class Widget:\n\
         \x20   search_path: Path\n\
         \x20   sys_path: list[str] = field(init=False, repr=False)\n\
         \x20   cwd: Path = field(init=False, repr=False, factory=Path.cwd)\n";
    let m = module(src);
    let Stmt::ClassDef(def) = find_definition(&m, "Widget").expect("Widget found") else {
        panic!("expected a class definition");
    };
    let members = render_class_members("mod", src, &def.body).expect("renders members");
    let names: Vec<&str> = members.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["search_path"], "{names:?}");
}

#[test]
fn render_class_members_reads_trailing_attribute_docstring() {
    let src = "class Widget(Struct, frozen=True):\n\
         \x20   timeout_secs: float | None = None\n\
         \x20   \"\"\"Fallback used when the caller doesn't pass one.\"\"\"\n\
         \x20   size: int = 0\n";
    let m = module(src);
    let Stmt::ClassDef(def) = find_definition(&m, "Widget").expect("Widget found") else {
        panic!("expected a class definition");
    };
    let members = render_class_members("mod", src, &def.body).expect("renders members");
    let names: Vec<&str> = members.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["timeout_secs", "size"], "{names:?}");
    assert_eq!(
        members[0].docstring.as_deref(),
        Some("Fallback used when the caller doesn't pass one.")
    );
    assert_eq!(members[1].docstring, None);
}

#[test]
fn render_class_members_consumes_trailing_docstring_even_for_a_skipped_field() {
    // A docstring after a `_`-prefixed field must not be misread as the
    // *next* field's docstring (the field itself is filtered, but the
    // string statement following it is still consumed).
    let src = "class Widget(Struct, frozen=True):\n\
         \x20   _hidden: int = 0\n\
         \x20   \"\"\"Internal only.\"\"\"\n\
         \x20   size: int = 0\n";
    let m = module(src);
    let Stmt::ClassDef(def) = find_definition(&m, "Widget").expect("Widget found") else {
        panic!("expected a class definition");
    };
    let members = render_class_members("mod", src, &def.body).expect("renders members");
    let names: Vec<&str> = members.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["size"], "{names:?}");
    assert_eq!(members[0].docstring, None);
}

#[test]
fn render_member_md_omits_no_docstring_disclaimer_for_fields() {
    let member = Entry {
        name: "repo_root".to_string(),
        kind: EntryKind::Constant,
        source_module: "mod".to_string(),
        signature: Some("repo_root: pathlib.Path".to_string()),
        docstring: None,
        members: Vec::new(),
    };
    let mut out = String::new();
    render_member_md(&mut out, &member);
    assert!(!out.contains("No docstring"), "{out}");
}

#[test]
fn render_class_members_skips_private_dunder_init_without_docstring_requirement() {
    // `__init__` is kept even with no docstring — only non-`__init__`
    // names starting with `_` are filtered out.
    let src = "class Empty:\n\
         \x20   def __init__(self) -> None:\n\
         \x20       pass\n";
    let m = module(src);
    let Stmt::ClassDef(def) = find_definition(&m, "Empty").expect("Empty found") else {
        panic!("expected a class definition");
    };
    let members = render_class_members("mod", src, &def.body).expect("renders members");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, "__init__");
    assert_eq!(members[0].docstring, None);
}

#[test]
fn render_member_md_renders_signature_and_docstring() {
    let member = Entry {
        name: "render".to_string(),
        kind: EntryKind::Function,
        source_module: "mod".to_string(),
        signature: Some("def render(self) -> str:".to_string()),
        docstring: Some("Render the widget.".to_string()),
        members: Vec::new(),
    };
    let mut out = String::new();
    render_member_md(&mut out, &member);
    assert!(out.contains("#### `render`"));
    assert!(out.contains("def render(self) -> str:"));
    assert!(out.contains("Render the widget."));
}

#[test]
fn render_member_md_falls_back_when_no_docstring() {
    let member = Entry {
        name: "render".to_string(),
        kind: EntryKind::Function,
        source_module: "mod".to_string(),
        signature: None,
        docstring: None,
        members: Vec::new(),
    };
    let mut out = String::new();
    render_member_md(&mut out, &member);
    assert!(out.contains("_No docstring on the source definition._"));
}

#[test]
fn public_source_hides_private_modules() {
    assert_eq!(public_source("toolr.utils._signature", "toolr"), "toolr");
    assert_eq!(public_source("toolr._context", "toolr"), "toolr");
    assert_eq!(public_source("toolr.sources", "toolr"), "toolr.sources");
}

#[test]
fn render_entry_md_cites_the_public_module() {
    let entry = Entry {
        name: "command".to_string(),
        kind: EntryKind::Function,
        source_module: "toolr._decorators".to_string(),
        signature: None,
        docstring: None,
        members: Vec::new(),
    };
    let mut out = String::new();
    render_entry_md(&mut out, &entry, "toolr");
    assert!(out.contains("**Source:** `toolr`"), "{out}");
    assert!(!out.contains("_decorators"), "{out}");
}

#[test]
fn render_entry_nests_class_members_under_the_class() {
    let src = "class Widget:\n\
         \x20   def render(self) -> str:\n\
         \x20       \"\"\"Render the widget.\"\"\"\n";
    let m = module(src);
    let stmt = find_definition(&m, "Widget").expect("Widget found");
    let entry = render_entry("Widget", "mod", src, stmt).expect("renders entry");
    assert_eq!(entry.members.len(), 1);
    assert_eq!(entry.members[0].name, "render");
}

#[test]
fn render_entry_handles_plain_assign_constants() {
    let src = "VALUE = 42\n";
    let m = module(src);
    let stmt = find_definition(&m, "VALUE").expect("VALUE found");
    let entry = render_entry("VALUE", "mod", src, stmt).expect("renders entry");
    assert!(matches!(entry.kind, EntryKind::Constant));
    assert_eq!(entry.signature.as_deref(), Some("VALUE = 42"));
    assert!(entry.members.is_empty());
}

#[test]
fn render_entry_rejects_unhandled_definition_shapes() {
    let src = "if True:\n    pass\n";
    let m = module(src);
    let stmt = &m.body[0];
    let result = render_entry("noop", "mod", src, stmt);
    let Err(err) = result else {
        panic!("expected an unhandled-shape error");
    };
    assert!(err.to_string().contains("unhandled definition shape"));
}

#[test]
fn extract_example_blocks_returns_source_in_the_requested_order() {
    let src = "def b():\n    pass\n\n\ndef a():\n    pass\n";
    let m = module(src);
    let blocks = extract_example_blocks(&m, src, &["a", "b"]).expect("both names resolve");
    assert_eq!(
        blocks,
        vec![
            "def a():\n    pass".to_string(),
            "def b():\n    pass".to_string()
        ]
    );
}

#[test]
fn extract_example_blocks_errors_on_a_renamed_or_removed_function() {
    let src = "def deploy():\n    pass\n";
    let m = module(src);
    let result = extract_example_blocks(&m, src, &["deploy", "typo_name"]);
    let Err(err) = result else {
        panic!("expected a missing-definition error");
    };
    let message = err.to_string();
    assert!(message.contains("typo_name"), "{message}");
    assert!(message.contains("TESTING_EXAMPLE_FUNCTIONS"), "{message}");
}

#[test]
fn crlf_python_source_slices_byte_identically_to_lf() {
    let src = "import x\n\n\ndef f():\n    \"\"\"Doc.\n\n    More.\n    \"\"\"\n    return 1\n";
    let slice = |tag: &str, eol: &str| {
        let path = std::env::temp_dir().join(format!(
            "xtask-authoring-eol-{tag}-{}.py",
            std::process::id()
        ));
        std::fs::write(&path, src.replace('\n', eol)).unwrap();
        let parsed = parse_with_source(&path);
        std::fs::remove_file(&path).unwrap();
        let (source, module) = parsed.unwrap();
        extract_example_blocks(&module, &source, &["f"]).unwrap()
    };
    let lf = slice("lf", "\n");
    assert_eq!(
        lf,
        ["def f():\n    \"\"\"Doc.\n\n    More.\n    \"\"\"\n    return 1"]
    );
    assert_eq!(slice("crlf", "\r\n"), lf);
}
