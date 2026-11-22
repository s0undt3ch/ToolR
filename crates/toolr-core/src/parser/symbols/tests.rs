use super::*;
use crate::parser::parse_python_file;
use std::io::Write;
use tempfile::NamedTempFile;

fn parse(src: &str) -> ModModule {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(src.as_bytes()).unwrap();
    parse_python_file(f.path()).unwrap()
}

#[test]
fn import_table_resolves_direct_absolute_import() {
    let src = "from tools.metrics._common import Environment\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    let candidates = table.candidates("Environment");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].module, "tools.metrics._common");
    assert_eq!(candidates[0].original_name, "Environment");
    assert!(!candidates[0].via_type_checking);
}

#[test]
fn import_table_resolves_aliased_import() {
    let src = "from tools.metrics._common import Environment as Env\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    let candidates = table.candidates("Env");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].module, "tools.metrics._common");
    assert_eq!(candidates[0].original_name, "Environment");
}

#[test]
fn import_table_ignores_unrelated_names() {
    let src = "from tools.metrics._common import Environment\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert!(table.candidates("SomethingElse").is_empty());
}

#[test]
fn import_table_handles_multiple_names_one_statement() {
    let src = "from tools.metrics._common import Environment, Region as R\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert_eq!(
        table.candidates("Environment")[0].module,
        "tools.metrics._common"
    );
    assert_eq!(table.candidates("R")[0].original_name, "Region");
}

#[test]
fn import_table_resolves_relative_sibling_import() {
    let src = "from ._common import Environment\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert_eq!(
        table.candidates("Environment")[0].module,
        "tools.metrics._common"
    );
}

#[test]
fn import_table_resolves_relative_parent_import() {
    // `from .. import shared` inside `tools.metrics.sub.analyse`
    // binds the *submodule* `shared` living in the parent package
    // `tools.metrics` — same bookkeeping shape as any other import
    // (module = the package, original_name = what's imported from
    // it). Task 6 is responsible for rejecting this as a class
    // source when used in a command signature (it names a module,
    // not a class).
    let src = "from .. import shared\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.sub.analyse", false);
    assert_eq!(table.candidates("shared")[0].module, "tools.metrics");
    assert_eq!(table.candidates("shared")[0].original_name, "shared");
}

#[test]
fn import_table_ignores_relative_import_with_too_many_dots() {
    // More leading dots than the importing module has path segments
    // for: `resolve_relative_module` returns `None`, and the whole
    // import statement is silently dropped rather than panicking.
    let src = "from ... import shared\n";
    let table = ImportTable::from_module(&parse(src), "tools.analyse", false);
    assert!(table.candidates("shared").is_empty());
}

#[test]
fn import_table_ignores_relative_import_at_exact_package_boundary() {
    // Dots exactly consume every path segment (no `?`-triggered
    // underflow, unlike the too-many-dots case above) and there's
    // no module suffix -- `resolve_relative_module` computes an
    // empty target and returns `None` rather than an empty-string
    // module.
    let src = "from .. import shared\n";
    let table = ImportTable::from_module(&parse(src), "tools.sub", false);
    assert!(table.candidates("shared").is_empty());
}

#[test]
fn import_table_resolves_relative_import_from_package_root() {
    let src = "from ._common import Environment\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics", true);
    assert_eq!(
        table.candidates("Environment")[0].module,
        "tools.metrics._common"
    );
}

#[test]
fn import_table_tags_type_checking_import() {
    let src = "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from tools.metrics._common import Environment\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    let c = table.candidates("Environment");
    assert_eq!(c.len(), 1);
    assert!(c[0].via_type_checking);
}

#[test]
fn import_table_recognises_dotted_type_checking() {
    let src = "import typing\nif typing.TYPE_CHECKING:\n    from tools.metrics._common import Environment\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert!(table.candidates("Environment")[0].via_type_checking);
}

#[test]
fn import_table_ignores_type_checking_nested_in_function() {
    let src = r#"
from typing import TYPE_CHECKING

def helper():
    if TYPE_CHECKING:
        from tools.metrics._common import Environment
"#;
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert!(table.candidates("Environment").is_empty());
}

#[test]
fn import_table_ignores_non_type_checking_if_guard() {
    // A top-level `if` whose test isn't `TYPE_CHECKING` at all (here
    // a plain boolean literal) must not be mistaken for one.
    let src = "if True:\n    from tools.metrics._common import Environment\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert!(table.candidates("Environment").is_empty());
}

#[test]
fn import_table_collects_both_try_except_branches() {
    let src = r#"
try:
    from tools.metrics._common import Environment
except ImportError:
    from tools.metrics._legacy import Environment
"#;
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    let c = table.candidates("Environment");
    assert_eq!(c.len(), 2);
    let modules: Vec<&str> = c.iter().map(|i| i.module.as_str()).collect();
    assert!(modules.contains(&"tools.metrics._common"));
    assert!(modules.contains(&"tools.metrics._legacy"));
}

#[test]
fn import_table_records_star_import_presence() {
    let src = "from tools.metrics._common import *\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert!(table.candidates("anything_at_all").is_empty());
    assert!(table.has_star_import());
}

#[test]
fn import_table_no_star_import_by_default() {
    let src = "from tools.metrics._common import Environment\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert!(!table.has_star_import());
}

#[test]
fn import_table_tracks_module_binding_for_bare_import() {
    let src = "import tools.metrics._common\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert_eq!(
        table.resolve_module_binding("tools"),
        Some("tools.metrics._common")
    );
}

#[test]
fn import_table_tracks_aliased_module_binding() {
    let src = "import tools.metrics._common as common\n";
    let table = ImportTable::from_module(&parse(src), "tools.metrics.analyse", false);
    assert_eq!(
        table.resolve_module_binding("common"),
        Some("tools.metrics._common")
    );
}

#[test]
fn collects_string_enum_values() {
    let src = r#"
from enum import StrEnum

class Mode(StrEnum):
    FAST = "fast"
    SLOW = "slow"
"#;
    let table = EnumTable::from_module(&parse(src), "tools.example");
    let (module, vals) = table
        .lookup("Mode", "tools.example", &std::collections::HashMap::new())
        .unwrap();
    assert_eq!(module, "tools.example");
    assert_eq!(vals, vec!["fast".to_string(), "slow".to_string()]);
}

#[test]
fn lookup_member_returns_serialised_value() {
    let src = r#"
from enum import StrEnum

class Operation(StrEnum):
    ADD = "add"
    SUBTRACT = "subtract"
"#;
    let table = EnumTable::from_module(&parse(src), "tools.example");
    assert_eq!(
        table.lookup_member(
            "Operation",
            "ADD",
            "tools.example",
            &std::collections::HashMap::new()
        ),
        Some("add")
    );
    assert_eq!(
        table.lookup_member(
            "Operation",
            "SUBTRACT",
            "tools.example",
            &std::collections::HashMap::new()
        ),
        Some("subtract")
    );
    assert_eq!(
        table.lookup_member(
            "Operation",
            "MISSING",
            "tools.example",
            &std::collections::HashMap::new()
        ),
        None
    );
    assert_eq!(
        table.lookup_member(
            "OtherClass",
            "ADD",
            "tools.example",
            &std::collections::HashMap::new()
        ),
        None
    );
}

#[test]
fn int_enum_member_falls_back_to_name() {
    let src = r#"
from enum import IntEnum

class Code(IntEnum):
    OK = 0
    ERROR = 1
"#;
    let table = EnumTable::from_module(&parse(src), "tools.example");
    // No string value, so we record the member's own name.
    assert_eq!(
        table.lookup_member(
            "Code",
            "OK",
            "tools.example",
            &std::collections::HashMap::new()
        ),
        Some("OK")
    );
    assert_eq!(
        table.lookup_member(
            "Code",
            "ERROR",
            "tools.example",
            &std::collections::HashMap::new()
        ),
        Some("ERROR")
    );
}

#[test]
fn ignores_non_enum_classes() {
    let src = r#"
class Foo:
    X = "x"
"#;
    let table = EnumTable::from_module(&parse(src), "tools.example");
    assert!(
        table
            .lookup("Foo", "tools.example", &std::collections::HashMap::new())
            .is_none()
    );
}

#[test]
fn colliding_bare_names_resolve_per_module_not_last_merged_wins() {
    let a = parse(
        r#"
from enum import StrEnum

class Database(StrEnum):
    PRIMARY = "primary"
"#,
    );
    let b = parse(
        r#"
from enum import StrEnum

class Database(StrEnum):
    REPLICA = "replica"
"#,
    );
    let mut table = EnumTable::from_module(&a, "tools.module_a");
    table.merge(EnumTable::from_module(&b, "tools.module_b"));

    assert_eq!(
        table
            .lookup(
                "Database",
                "tools.module_a",
                &std::collections::HashMap::new()
            )
            .unwrap(),
        ("tools.module_a".to_string(), vec!["primary".to_string()])
    );
    assert_eq!(
        table
            .lookup(
                "Database",
                "tools.module_b",
                &std::collections::HashMap::new()
            )
            .unwrap(),
        ("tools.module_b".to_string(), vec!["replica".to_string()])
    );
    // Neither module's definition wins from an unrelated module —
    // the collision is genuinely ambiguous, so this resolves to
    // nothing rather than silently picking one (GH #449).
    assert!(
        table
            .lookup(
                "Database",
                "tools.module_c",
                &std::collections::HashMap::new()
            )
            .is_none()
    );
}

#[test]
fn single_definition_resolves_from_an_unrelated_module() {
    let src = r#"
from enum import StrEnum

class Mode(StrEnum):
    FAST = "fast"
"#;
    let table = EnumTable::from_module(&parse(src), "tools.module_a");

    assert_eq!(
        table
            .lookup("Mode", "tools.module_b", &std::collections::HashMap::new())
            .unwrap(),
        ("tools.module_a".to_string(), vec!["fast".to_string()])
    );
}

#[test]
fn resolve_def_prefers_explicit_import_over_guessing() {
    // Two modules declare a *different*-shaped `Database` class.
    // Without an import this is genuinely ambiguous; with an
    // explicit import naming one of them, it resolves to exactly
    // that one instead of erroring or guessing.
    let a =
        parse("from enum import StrEnum\nclass Database(StrEnum):\n    PRIMARY = \"primary\"\n");
    let b =
        parse("from enum import StrEnum\nclass Database(StrEnum):\n    REPLICA = \"replica\"\n");
    let mut table = EnumTable::from_module(&a, "tools.module_a");
    table.merge(EnumTable::from_module(&b, "tools.module_b"));

    let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
    all_imports.insert(
        "tools.module_c".to_string(),
        ImportTable::from_module(
            &parse("from tools.module_b import Database\n"),
            "tools.module_c",
            false,
        ),
    );
    assert_eq!(
        table
            .lookup("Database", "tools.module_c", &all_imports)
            .unwrap(),
        ("tools.module_b".to_string(), vec!["replica".to_string()])
    );
}

#[test]
fn resolve_def_import_pointing_nowhere_known_is_none() {
    let a =
        parse("from enum import StrEnum\nclass Database(StrEnum):\n    PRIMARY = \"primary\"\n");
    let table = EnumTable::from_module(&a, "tools.module_a");
    let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
    all_imports.insert(
        "tools.module_c".to_string(),
        ImportTable::from_module(
            &parse("from tools.somewhere_else import Database\n"),
            "tools.module_c",
            false,
        ),
    );
    assert!(
        table
            .lookup("Database", "tools.module_c", &all_imports)
            .is_none()
    );
}

#[test]
fn resolve_def_follows_reexport_chain_through_init() {
    // tools/metrics/_common.py declares Environment.
    // tools/metrics/__init__.py does `from ._common import Environment`.
    // tools/analyse.py does `from tools.metrics import Environment`.
    let common = parse("class Environment(enum.StrEnum):\n    PRODUCTION = \"production\"\n");
    let enums = EnumTable::from_module(&common, "tools.metrics._common");

    let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
    all_imports.insert(
        "tools.metrics".to_string(),
        ImportTable::from_module(
            &parse("from ._common import Environment\n"),
            "tools.metrics",
            true,
        ),
    );
    all_imports.insert(
        "tools.analyse".to_string(),
        ImportTable::from_module(
            &parse("from tools.metrics import Environment\n"),
            "tools.analyse",
            false,
        ),
    );

    assert_eq!(
        enums
            .lookup("Environment", "tools.analyse", &all_imports)
            .unwrap(),
        (
            "tools.metrics._common".to_string(),
            vec!["production".to_string()]
        )
    );
}

#[test]
fn resolve_def_follows_aliasing_chain() {
    // mod1 imports Foo from tools.real and re-aliases it as Bar;
    // mod2 imports Bar from mod1.
    let real = parse("class Foo(enum.StrEnum):\n    A = \"a\"\n");
    let enums = EnumTable::from_module(&real, "tools.real");

    let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
    all_imports.insert(
        "tools.mod1".to_string(),
        ImportTable::from_module(
            &parse("from tools.real import Foo as Bar\n"),
            "tools.mod1",
            false,
        ),
    );
    all_imports.insert(
        "tools.mod2".to_string(),
        ImportTable::from_module(&parse("from tools.mod1 import Bar\n"), "tools.mod2", false),
    );

    assert_eq!(
        enums.lookup("Bar", "tools.mod2", &all_imports).unwrap(),
        ("tools.real".to_string(), vec!["a".to_string()])
    );
}

#[test]
fn resolve_def_chain_cycle_guard_does_not_hang() {
    // mod_a imports X from mod_b; mod_b imports X from mod_a. Neither
    // declares X. Must terminate with None, not loop forever.
    let enums = EnumTable::default();
    let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
    all_imports.insert(
        "tools.mod_a".to_string(),
        ImportTable::from_module(&parse("from tools.mod_b import X\n"), "tools.mod_a", false),
    );
    all_imports.insert(
        "tools.mod_b".to_string(),
        ImportTable::from_module(&parse("from tools.mod_a import X\n"), "tools.mod_b", false),
    );
    assert!(enums.lookup("X", "tools.mod_a", &all_imports).is_none());
}

#[test]
fn resolve_def_try_except_branches_with_identical_members_resolve() {
    // Both try/except branches name a different module, but those
    // modules declare byte-identical Environment shapes -- resolves
    // to either, same as the cross-module dedupe case.
    let a = parse("class Environment(enum.StrEnum):\n    PRODUCTION = \"production\"\n");
    let b = parse("class Environment(enum.StrEnum):\n    PRODUCTION = \"production\"\n");
    let mut enums = EnumTable::from_module(&a, "tools.metrics._common");
    enums.merge(EnumTable::from_module(&b, "tools.metrics._legacy"));

    let mut all_imports: HashMap<String, ImportTable> = HashMap::new();
    all_imports.insert(
        "tools.metrics.analyse".to_string(),
        ImportTable::from_module(
            &parse(
                "try:\n    from ._common import Environment\nexcept ImportError:\n    from ._legacy import Environment\n",
            ),
            "tools.metrics.analyse",
            false,
        ),
    );

    assert_eq!(
        enums
            .lookup("Environment", "tools.metrics.analyse", &all_imports)
            .unwrap(),
        (
            "tools.metrics._common".to_string(),
            vec!["production".to_string()]
        )
    );
}

#[test]
fn arg_section_collects_module_bindings() {
    let src = r#"
LOGGING = arg_section("Logging Options", description="Control verbosity.")
"#;
    let table = ArgSectionTable::from_module(&parse(src));
    let entry = table.lookup("LOGGING").unwrap();
    assert_eq!(entry.title, "Logging Options");
    assert_eq!(entry.description.as_deref(), Some("Control verbosity."));
}

#[test]
fn arg_section_without_description_is_none() {
    let src = r#"NETWORK = arg_section("Network Options")"#;
    let table = ArgSectionTable::from_module(&parse(src));
    let entry = table.lookup("NETWORK").unwrap();
    assert_eq!(entry.title, "Network Options");
    assert!(entry.description.is_none());
}

#[test]
fn arg_section_ignores_non_arg_section_calls() {
    let src = r#"FOO = something_else("title")"#;
    let table = ArgSectionTable::from_module(&parse(src));
    assert!(table.lookup("FOO").is_none());
}
