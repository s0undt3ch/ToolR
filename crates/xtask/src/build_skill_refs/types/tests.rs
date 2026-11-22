use super::*;

#[test]
fn escape_cell_escapes_pipes() {
    assert_eq!(escape_cell("T | None"), "T \\| None");
}

#[test]
fn render_annotation_cell_wraps_bare_annotation() {
    let doc = TypeDoc {
        annotation: "int",
        validated_by: "clap",
        wire_format: "JSON number",
        python_receives: "`int`",
        note: "",
    };
    assert_eq!(render_annotation_cell(&doc, TableFlavor::Gfm), "`int`");
}

#[test]
fn render_annotation_cell_appends_note_after_one_space() {
    let doc = TypeDoc {
        annotation: "Enum",
        validated_by: "clap (member values)",
        wire_format: "string",
        python_receives: "enum member",
        note: "subclass",
    };
    assert_eq!(
        render_annotation_cell(&doc, TableFlavor::Gfm),
        "`Enum` subclass"
    );
}

fn pipe_bearing_doc() -> TypeDoc {
    TypeDoc {
        annotation: "T | None",
        validated_by: "clap (`required=false`)",
        wire_format: "typed or absent",
        python_receives: "`T` or `None`",
        note: "",
    }
}

#[test]
fn render_annotation_cell_gfm_escapes_pipe_inside_code_span() {
    let doc = pipe_bearing_doc();
    assert_eq!(
        render_annotation_cell(&doc, TableFlavor::Gfm),
        "`T \\| None`"
    );
}

#[test]
fn render_annotation_cell_python_markdown_table_uses_html_entity() {
    // Python-Markdown's table extension splits a row on every
    // literal `|` before inline processing runs, so a backslash
    // escape survives verbatim; only a non-`|` entity does not.
    let doc = pipe_bearing_doc();
    assert_eq!(
        render_annotation_cell(&doc, TableFlavor::PythonMarkdownTable),
        "<code>T &#124; None</code>"
    );
}

#[test]
fn render_annotation_cell_python_markdown_table_keeps_code_span_without_pipe() {
    let doc = TypeDoc {
        annotation: "int",
        validated_by: "clap",
        wire_format: "JSON number",
        python_receives: "`int`",
        note: "",
    };
    assert_eq!(
        render_annotation_cell(&doc, TableFlavor::PythonMarkdownTable),
        "`int`"
    );
}

#[test]
fn types_table_has_one_row_per_catalogue_entry() {
    let table = render_types_table(TableFlavor::Gfm);
    assert_eq!(table.lines().count(), 2 + SupportedType::catalogue().len());
}

#[test]
fn types_table_python_markdown_flavor_uses_html_for_pipe_row() {
    let table = render_types_table(TableFlavor::PythonMarkdownTable);
    assert!(table.contains("<code>T &#124; None</code>"));
    assert!(!table.contains("T \\| None"));
}
