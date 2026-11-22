use super::*;
use crate::parser::parse_python_file;
use crate::parser::symbols::ImportTable;
use std::io::Write;
use tempfile::NamedTempFile;

fn parse_src(src: &str) -> ModModule {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(src.as_bytes()).unwrap();
    parse_python_file(f.path()).unwrap()
}

/// Import tables for `(module path, source)` pairs, all in `tools/`.
fn imports(modules: &[(&str, &str)]) -> HashMap<String, ImportTable> {
    modules
        .iter()
        .map(|(path, src)| {
            let m = parse_src(src);
            (path.to_string(), ImportTable::from_module(&m, path, false))
        })
        .collect()
}

/// `extract_groups` on `src` parsed as `tools.m`, alongside `others`.
fn groups_in(src: &str, others: &[(&str, &str)]) -> Vec<GroupBinding> {
    let mut modules = vec![("tools.m", src)];
    modules.extend_from_slice(others);
    let all = imports(&modules);
    extract_groups(
        &parse_src(src),
        None,
        &HashMap::new(),
        &ToolrNames::new(&all),
        "tools.m",
    )
}

/// `extract_groups` on a module with no imports beyond its own.
fn groups_in_module(m: &ModModule, doc: Option<&str>) -> Vec<GroupBinding> {
    let all = HashMap::from([(
        "tools.m".to_string(),
        ImportTable::from_module(m, "tools.m", false),
    )]);
    extract_groups(m, doc, &HashMap::new(), &ToolrNames::new(&all), "tools.m")
}

fn paths(groups: &[GroupBinding]) -> Vec<(String, String)> {
    groups
        .iter()
        .map(|g| (g.var.clone(), g.full_path()))
        .collect()
}

fn pair(var: &str, path: &str) -> (String, String) {
    (var.to_string(), path.to_string())
}

#[test]
fn aliased_import_binds_a_group() {
    let groups = groups_in(
        "from toolr import command_group as cg\ng = cg(\"alias\", \"Alias\", \"Aliased.\")\nkid = cg(\"alias.kid\", \"Kid\", \"Kid.\")\n",
        &[],
    );
    assert_eq!(
        paths(&groups),
        [pair("g", "alias"), pair("kid", "alias.kid")]
    );
    assert_eq!(groups[0].fields.description.text(), "Aliased.");
}

#[test]
fn toolr_module_alias_attribute_binds_a_group() {
    let groups = groups_in(
        "import toolr as t\ng = t.command_group(\"t\", \"T\", \"T.\")\n",
        &[],
    );
    assert_eq!(paths(&groups), [pair("g", "t")]);
}

#[test]
fn assigned_alias_binds_a_group() {
    let groups = groups_in(
        "import toolr\ncg = toolr.command_group\ng = cg(\"assigned\", \"A\", \"A.\")\n",
        &[],
    );
    assert_eq!(paths(&groups), [pair("g", "assigned")]);
}

#[test]
fn reexported_alias_binds_a_group() {
    let groups = groups_in(
        "from tools._common import cg\ncg(\"re\", \"Re\", \"Re.\")\n",
        &[("tools._common", "from toolr import command_group as cg\n")],
    );
    assert_eq!(paths(&groups), [pair("re", "re")]);
}

#[test]
fn annotated_binding_binds_a_group() {
    let groups = groups_in(
        "h: CommandGroup = command_group(\"ann\", \"Ann\", \"Ann.\")\nkid = command_group(\"ann.kid\", \"Kid\", \"Kid.\")\n",
        &[],
    );
    assert_eq!(paths(&groups), [pair("h", "ann"), pair("kid", "ann.kid")]);
}

#[test]
fn annotated_alias_binds_a_group() {
    let groups = groups_in(
        "import toolr\nfrom toolr import command_group\nfrom toolr import command_group as other\ncg: object = command_group\ntcg: object = toolr.command_group\nlater: object = cg\na = cg(\"a\", \"A\", \"A.\")\nb = tcg(\"b\", \"B\", \"B.\")\nc = later(\"c\", \"C\", \"C.\")\nd = other(\"d\", \"D\", \"D.\")\n",
        &[],
    );
    assert_eq!(
        paths(&groups),
        [
            pair("a", "a"),
            pair("b", "b"),
            pair("c", "c"),
            pair("d", "d")
        ]
    );
}

#[test]
fn annotated_alias_of_something_else_is_not_a_group() {
    let groups = groups_in(
        "from toolr import command_group as cg\ncg: object = print\nsub: object = print.x()\ng = cg(\"x\", \"X\", \"X.\")\n",
        &[],
    );
    assert!(groups.is_empty());
}

#[test]
fn try_except_fallback_binding_wins() {
    let groups = groups_in(
        "try:\n    from toolr import command_group as cg\nexcept ImportError:\n    cg = None\ng = cg(\"d\", \"D\", \"D.\")\n",
        &[],
    );
    assert!(groups.is_empty());
}

#[test]
fn del_does_not_unbind_an_alias() {
    let groups = groups_in(
        "from toolr import command_group as cg\ng = cg(\"c\", \"C\", \"C.\")\ndel cg\n",
        &[],
    );
    assert_eq!(paths(&groups), [pair("g", "c")]);
}

#[test]
fn annotation_without_a_value_binds_nothing() {
    let groups = groups_in(
        "g: CommandGroup\nh: int = 1\nk.x: int = command_group(\"k\", \"K\", \"K.\")\n",
        &[],
    );
    assert!(groups.is_empty());
}

#[test]
fn bare_annotation_does_not_hide_an_imported_alias() {
    let groups = groups_in(
        "from toolr import command_group as cg\ncg: object\ng = cg(\"x\", \"X\", \"Group x.\")\n",
        &[],
    );
    assert_eq!(paths(&groups), [pair("g", "x")]);
}

#[test]
fn self_copy_resolves_through_the_import() {
    let groups = groups_in(
        "from toolr import command_group as cg\ncg = cg\ng = cg(\"y\", \"Y\", \"Y.\")\n",
        &[],
    );
    assert_eq!(paths(&groups), [pair("g", "y")]);
    let groups = groups_in(
        "from toolr import command_group as cg\ncg: object = cg\ng = cg(\"y\", \"Y\", \"Y.\")\n",
        &[],
    );
    assert_eq!(paths(&groups), [pair("g", "y")]);
}

#[test]
fn self_copy_resolves_through_a_star_import() {
    let groups = groups_in(
        "from tools.lib import *\ncg = cg\ng = cg(\"s\", \"S\", \"S.\")\n",
        &[("tools.lib", "from toolr import command_group as cg\n")],
    );
    assert_eq!(paths(&groups), [pair("g", "s")]);
}

#[test]
fn self_copy_of_nothing_is_not_a_group() {
    let groups = groups_in("cg = cg\ng = cg(\"n\", \"N\", \"N.\")\n", &[]);
    assert!(groups.is_empty());
}

#[test]
fn local_def_alias_is_not_a_group() {
    let groups = groups_in(
        "def cg(*args):\n    pass\ng = cg(\"local\", \"L\", \"L.\")\n",
        &[],
    );
    assert!(groups.is_empty());
}

#[test]
fn alias_from_an_unreadable_module_is_not_a_group() {
    let groups = groups_in(
        "from elsewhere import cg\ng = cg(\"unknown\", \"U\", \"U.\")\n",
        &[],
    );
    assert!(groups.is_empty());
}

#[test]
fn literal_spelling_counts_even_when_local() {
    let groups = groups_in(
        "def command_group(*args):\n    pass\ng = command_group(\"lit\", \"L\", \"L.\")\n",
        &[],
    );
    assert_eq!(paths(&groups), [pair("g", "lit")]);
}

#[test]
fn extracts_command_group_with_literal_args() {
    let src = r#"group = command_group("ci", "CI utilities")"#;
    let m = parse_src(src);
    let groups = groups_in_module(&m, None);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].var, "group");
    assert_eq!(groups[0].name, "ci");
    assert_eq!(groups[0].fields.title.text(), "CI utilities");
}

#[test]
fn explicit_title_keeps_docstring_full_description() {
    // When the caller passes both a positional title and a docstring,
    // the title wins for the short slot but the docstring's rendered
    // ``full_description`` (short + long + sections) still populates
    // `description` (clap's `long_about`).
    let src = r#"group = command_group("ci", "CI utilities", docstring=__doc__)"#;
    let m = parse_src(src);
    let groups = groups_in_module(&m, Some("First paragraph short.\n\nLong paragraph body."));
    assert_eq!(groups[0].fields.title.text(), "CI utilities");
    assert_eq!(
        groups[0].fields.description.text(),
        "First paragraph short.\n\nLong paragraph body."
    );
}

#[test]
fn docstring_first_paragraph_becomes_title() {
    // Single-line docstring: short_description fills `title`,
    // `description` ends up the same single paragraph (no long
    // body, no sections to append).
    let src = r#"group = command_group("widget", docstring=__doc__)"#;
    let m = parse_src(src);
    let groups = groups_in_module(&m, Some("Example single-line docstring for a group."));
    assert_eq!(
        groups[0].fields.title.text(),
        "Example single-line docstring for a group."
    );
    assert_eq!(
        groups[0].fields.description.text(),
        "Example single-line docstring for a group."
    );
}

#[test]
fn docstring_long_paragraph_becomes_description() {
    // Two-paragraph docstring: first paragraph → title.
    // `description` is the full render (short + long), so
    // `--help` re-states the blurb plus the long body.
    let src = r#"group = command_group("ci", docstring=__doc__)"#;
    let m = parse_src(src);
    let groups = groups_in_module(
        &m,
        Some("Short blurb for parent listing.\n\nLonger prose shown by --help only."),
    );
    assert_eq!(
        groups[0].fields.title.text(),
        "Short blurb for parent listing."
    );
    assert_eq!(
        groups[0].fields.description.text(),
        "Short blurb for parent listing.\n\nLonger prose shown by --help only."
    );
}

#[test]
fn description_kwarg_with_docstring_is_contradictory() {
    // Python's `command_group` refuses both; the build reports it.
    let src = r#"group = command_group("ci", description="kw-desc", docstring=__doc__)"#;
    let m = parse_src(src);
    let groups = groups_in_module(&m, Some("Short title.\n\nLong paragraph."));
    assert!(groups[0].contradictory);
    assert!(groups[0].describes);
}

#[test]
fn title_only_call_does_not_describe() {
    let m = parse_src(r#"group = command_group("ci", "CI")"#);
    let groups = groups_in_module(&m, None);
    assert!(!groups[0].describes);
    assert!(!groups[0].contradictory);
}

#[test]
fn parses_description_from_third_positional() {
    let src = r#"group = command_group("n", "t", "desc body")"#;
    let m = parse_src(src);
    let groups = groups_in_module(&m, None);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].fields.description.text(), "desc body");
}

#[test]
fn description_kwarg_wins_over_positional() {
    let src = r#"group = command_group("n", "t", "pos", description="kw")"#;
    let m = parse_src(src);
    let groups = groups_in_module(&m, None);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].fields.description.text(), "kw");
}

#[test]
fn ignores_non_command_group_assignments() {
    let src = "x = 1\ny = some_other_func(\"ci\")\n";
    let m = parse_src(src);
    assert!(groups_in_module(&m, None).is_empty());
}

#[test]
fn nested_group_records_parent_full_path() {
    let src = r#"docker = command_group("docker", "Docker")
image = command_group("docker.image", "Image")
"#;
    let m = parse_src(src);
    let groups = groups_in_module(&m, None);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].name, "docker");
    assert_eq!(groups[0].parent, None);
    assert_eq!(groups[0].full_path(), "docker");
    assert_eq!(groups[1].name, "image");
    assert_eq!(groups[1].parent.as_deref(), Some("docker"));
    assert_eq!(groups[1].full_path(), "docker.image");
}

#[test]
fn explicit_parent_kwarg_nests_under_named_group() {
    // Matches the `command_group("...", parent="ci")` signature
    // exposed by `toolr._registry.command_group`. The parent
    // doesn't have to be declared in the same file — we trust the
    // string as the dotted full_path.
    let src = r#"
helm_diff = command_group("helm-diff-pr-comment", "Helm diff", parent="ci")
"#;
    let m = parse_src(src);
    let groups = groups_in_module(&m, None);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "helm-diff-pr-comment");
    assert_eq!(groups[0].parent.as_deref(), Some("ci"));
    assert_eq!(groups[0].full_path(), "ci.helm-diff-pr-comment");
}

#[test]
fn two_level_nesting_concatenates_full_path() {
    let src = r#"a = command_group("a", "A")
b = command_group("a.b", "B")
c = command_group("a.b.c", "C")
"#;
    let m = parse_src(src);
    let groups = groups_in_module(&m, None);
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[2].full_path(), "a.b.c");
    assert_eq!(groups[2].parent.as_deref(), Some("a.b"));
}

const IMPORT: &str = "from toolr import command_group\n";

/// The names `detect_unsupported_group_bindings` reports for `src` parsed as `tools.m`.
fn unbound_raw(src: &str) -> Vec<String> {
    let m = parse_src(src);
    let all = imports(&[("tools.m", src)]);
    detect_unsupported_group_bindings(&m, "tools.m", &ToolrNames::new(&all))
        .into_iter()
        .map(|u| {
            assert_eq!(u.module, "tools.m");
            assert_eq!(u.kind, UnsupportedCommandKind::GroupBinding);
            u.function
        })
        .collect()
}

/// As `unbound_raw`, after `from toolr import command_group`.
fn unbound(src: &str) -> Vec<String> {
    unbound_raw(&format!("{IMPORT}{src}"))
}

fn named(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|n| format!("command_group(\"{n}\")"))
        .collect()
}

#[test]
fn plain_top_level_bindings_are_not_reported() {
    let src = "a = command_group(\"a\", \"A\", \"A.\")\nb: T = command_group(\"b\", \"B\", \"B.\")\ncommand_group(\"c\", \"C\", \"C.\")\nh: Annotated[object, command_group(\"h\", \"H\", \"H.\")] = command_group(\"h\", \"H\", \"H.\")\n";
    assert_eq!(unbound(src), Vec::<String>::new());
}

#[test]
fn method_form_is_reported_with_its_own_kind() {
    for call in [
        "a.command_group(\"k\", \"K\", \"K.\")",
        "make().command_group(\"k\", \"K\", \"K.\", parent=\"ci\")",
        "nope.command_group(\"k\", \"K\", \"K.\")",
    ] {
        let src = format!("{IMPORT}a = command_group(\"a\", \"A\", \"A.\")\nk = {call}\n");
        let m = parse_src(&src);
        let all = imports(&[("tools.m", &src)]);
        let found = detect_unsupported_group_bindings(&m, "tools.m", &ToolrNames::new(&all));
        assert_eq!(found.len(), 1, "{call}");
        assert_eq!(found[0].kind, UnsupportedCommandKind::MethodGroup);
        assert_eq!(found[0].function, "command_group(\"k\")");
        assert!(
            found[0]
                .kind
                .to_string()
                .contains("command_group(\"parent.child\", ...)")
        );
        // The statement binds no group either.
        assert_eq!(
            groups_in(&format!("{IMPORT}k = {call}\n"), &[]).len(),
            0,
            "{call}"
        );
    }
}

#[test]
fn tuple_target_reports_each_call() {
    assert_eq!(
        unbound(
            "a, b = command_group(\"a\", \"A\", \"A.\"), command_group(\"b\", \"B\", \"B.\")\n"
        ),
        named(&["a", "b"])
    );
}

#[test]
fn chained_target_and_walrus_are_reported() {
    assert_eq!(
        unbound(
            "a = b = command_group(\"a\", \"A\", \"A.\")\n(g := command_group(\"g\", \"G\", \"G.\"))\n"
        ),
        named(&["a", "g"])
    );
}

#[test]
fn compound_statement_bodies_are_reported() {
    for head in [
        "if x:\n    ",
        "try:\n    pass\nfinally:\n    ",
        "with x:\n    ",
        "for _ in x:\n    ",
        "while x:\n    ",
        "match x:\n    case 1:\n        ",
    ] {
        let src = format!("{head}g = command_group(\"g\", \"G\", \"G.\")\n");
        assert_eq!(unbound(&src), named(&["g"]), "{src}");
    }
    assert_eq!(
        unbound("if x:\n    command_group(\"g\", \"G\", \"G.\")\n"),
        named(&["g"])
    );
}

#[test]
fn function_class_lambda_and_comprehension_bodies_are_reported() {
    assert_eq!(
        unbound(
            "def make():\n    return command_group(\"g\", \"G\", \"G.\")\n\n\nclass C:\n    g = command_group(\"h\", \"H\", \"H.\")\n\n\nl = lambda: command_group(\"l\", \"L\", \"L.\")\ngs = [command_group(n, \"T\", \"D.\") for n in \"ab\"]\n"
        ),
        [
            "command_group(\"g\")",
            "command_group(\"h\")",
            "command_group(\"l\")",
            "command_group"
        ]
    );
}

#[test]
fn call_nested_in_an_expression_or_decorator_is_reported() {
    assert_eq!(
        unbound(
            "command_group(\"g\", \"G\", \"G.\").command(f)\n@command_group(\"d\", \"D\", \"D.\")\ndef f(): ...\n"
        ),
        named(&["g", "d"])
    );
}

#[test]
fn call_among_a_bound_statements_arguments_is_not_reported() {
    let src =
        "g = command_group(\"g\", \"G\", \"G.\", parent=command_group(\"p\", \"P\", \"P.\"))\n";
    assert_eq!(unbound(src), Vec::<String>::new());
}

#[test]
fn aliased_and_attribute_calls_under_an_if_are_reported() {
    for (head, call) in [
        ("from toolr import command_group as cg\n", "cg"),
        ("import toolr as t\n", "t.command_group"),
        ("import toolr.types\n", "toolr.command_group"),
    ] {
        let src = format!("{head}if x:\n    g = {call}(\"g\", \"G\", \"G.\")\n");
        assert_eq!(unbound_raw(&src), named(&["g"]), "{src}");
    }
}

#[test]
fn calls_that_are_not_toolrs_are_not_reported() {
    for src in [
        "def command_group(*a):\n    return a\nif x:\n    h = command_group('a', 'b', 'c')\n",
        "def helper(command_group):\n    return command_group('a', 'b', 'c')\n",
        "f = lambda command_group: command_group('a', 'b', 'c')\n",
        "from pathlib import Path as command_group\nif x:\n    h = command_group('a', 'b', 'c')\n",
        "command_group = print\nif x:\n    command_group('a', 'b', 'c')\n",
        "class R:\n    def go(self, o):\n        self.command_group('a', 'b', 'c')\n        o.command_group('a', 'b', 'c')\n",
        "import toolr\ndef f(toolr):\n    return toolr.command_group('a', 'b', 'c')\n",
        "import other as toolr\nif x:\n    toolr.command_group('a', 'b', 'c')\n",
    ] {
        assert_eq!(unbound_raw(src), Vec::<String>::new(), "{src}");
    }
}

#[test]
fn parameter_shadows_only_the_function_body() {
    let src = "def f(command_group, other=command_group(\"d\", \"D\", \"D.\")):\n    return command_group(\"x\", \"X\", \"X.\")\n";
    assert_eq!(unbound(src), named(&["d"]));
}

#[test]
fn keyword_name_binds_and_names_a_group() {
    let groups = groups_in(
        "from toolr import command_group\ng = command_group(name=\"kw\", title=\"Kw\", description=\"D.\")\n",
        &[],
    );
    assert_eq!(paths(&groups), [pair("g", "kw")]);
    assert_eq!(
        unbound("def f():\n    command_group(name=\"g\", description=\"D.\")\n"),
        named(&["g"])
    );
}

#[test]
fn non_literal_name_is_reported_generically() {
    let src = "N = \"n\"\na = command_group(N, \"T\", \"D.\")\ncommand_group(name=N)\nb: T = command_group(*args)\nok = command_group(name=\"ok\", title=\"Ok\", description=\"D.\")\n";
    assert_eq!(unbound(src), ["command_group"; 3]);
    assert_eq!(
        unbound("def f(n):\n    return command_group(n, \"N\", \"N.\")\n"),
        ["command_group"]
    );
}

#[test]
fn name_only_calls_are_reported_too() {
    let src = "g = command_group(\"g\", \"G\", \"G.\")\ndef f(extra):\n    command_group(\"g\")\n    command_group(\"h\", \"H\")\n    command_group(\"i\", title=\"I\")\n    command_group(\"j\", \"J\", None)\n    command_group(\"k\", \"K\", **extra)\n";
    assert_eq!(unbound(src), named(&["g", "h", "i", "j", "k"]));
}

#[test]
fn problems_come_in_source_order() {
    let src = "if x:\n    d[command_group(\"a\", \"A\", \"A.\")] = command_group(\"b\", \"B\", \"B.\")\n    y = [command_group(\"c\", \"C\", \"C.\") if command_group(\"d\", \"D\", \"D.\") else 0]\n    f(x=command_group(\"e\", \"E\", \"E.\"), *[command_group(\"f\", \"F\", \"F.\")])\n";
    assert_eq!(unbound(src), named(&["a", "b", "c", "d", "e", "f"]));
}

#[test]
fn supported_group_call_matches_group_binding() {
    let src = "from toolr import command_group\na = command_group(\"a\", \"A\", \"A.\")\nb: T = command_group(\"b\", \"B\", \"B.\")\ncommand_group(\"c\", \"C\", \"C.\")\nx, y = command_group(\"d\", \"D\", \"D.\"), 1\n(w := command_group(\"e\", \"E\", \"E.\"))\nz = other(1)\n";
    let m = parse_src(src);
    let all = imports(&[("tools.m", src)]);
    let names = ToolrNames::new(&all);
    let source = Source::new(&m, None);
    let bound: Vec<bool> = m
        .body
        .iter()
        .skip(1)
        .map(|s| group_binding(s, &source, &[], &HashMap::new(), &names, "tools.m").is_some())
        .collect();
    assert_eq!(bound, [true, true, true, false, false, false]);
    assert_eq!(unbound_raw(src).len(), 2);
}
