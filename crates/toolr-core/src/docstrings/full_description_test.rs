// Direct unit coverage for ``Docstring::full_description`` — the
// rendered text fed to clap's ``long_about`` slot for both
// ``command_group(docstring=...)`` and ``@command``-decorated function
// docstrings. Each section header (Examples / Notes / Warnings /
// See Also / References / Todo / Deprecated / Version Added /
// Version Changed) gets its own branch in the renderer; this file
// exercises every one so a regression that drops a section can't
// land silently.
#[cfg(test)]
mod full_description_test {
    use crate::docstrings::SimpleDocstringParser;

    fn render(docstring: &str) -> String {
        SimpleDocstringParser::new()
            .parse(docstring)
            .expect("parse failed")
            .full_description()
    }

    #[test]
    fn empty_docstring_renders_empty_string() {
        // No short, no long, no sections → empty output. Guards the
        // "no leading paragraph to anchor on" path.
        assert_eq!(render(""), "");
    }

    #[test]
    fn short_only_renders_just_the_short_paragraph() {
        // Single-line docstring: ``full_description`` returns the
        // short paragraph verbatim with no trailing whitespace.
        assert_eq!(render("Short paragraph."), "Short paragraph.");
    }

    #[test]
    fn short_plus_long_separates_with_blank_line_and_trailing_newline() {
        // The renderer appends ``\n\n<long>\n`` so the resulting
        // ``long_about`` string is markdown-friendly and lets the
        // section blocks that follow start on their own line.
        let out = render("Short paragraph.\n\nLong body paragraph.");
        assert_eq!(out, "Short paragraph.\n\nLong body paragraph.\n");
    }

    #[test]
    fn examples_section_renders_with_bullet_per_entry() {
        // ``Examples:`` is a multi-entry section; each example gets
        // a ``- `` bullet unless the line already starts with one.
        // Entries are blank-line separated in the source docstring.
        let out = render(
            "Short.\n\nExamples:\n    First example.\n\n    Second example.\n",
        );
        assert!(out.contains("## Examples"), "missing Examples header: {out}");
        assert!(out.contains("- First example."), "missing first example bullet: {out}");
        assert!(out.contains("- Second example."), "missing second example bullet: {out}");
    }

    #[test]
    fn examples_section_with_snippet_emits_fenced_code_block() {
        // When the parser pulls a fenced code snippet out of an
        // example, the renderer wraps it in another fenced block so
        // clap's markdown path preserves the formatting.
        let docstring = "\
Short.

Examples:
    Calling the hello command:

    ```
    toolr greet hello --who world
    ```
";
        let out = render(docstring);
        assert!(out.contains("```\n"), "missing fenced code block: {out:?}");
        assert!(
            out.contains("toolr greet hello --who world"),
            "missing snippet body: {out:?}"
        );
    }

    #[test]
    fn examples_section_with_pre_existing_bullet_marker_is_left_alone() {
        // If an Examples entry already begins with ``- `` (or ``* ``),
        // the renderer must not double-prefix it. Guards the
        // bullet-detection branch.
        let out = render("Short.\n\nExamples:\n    - Already a bullet.\n");
        assert!(
            out.contains("- Already a bullet."),
            "bullet was rewritten: {out}"
        );
        assert!(
            !out.contains("- - Already a bullet."),
            "bullet was double-prefixed: {out}"
        );
    }

    #[test]
    fn notes_section_renders_as_bullet_list() {
        let out = render("Short.\n\nNotes:\n    Heads up.\n    Second note.\n");
        assert!(out.contains("## Notes\n"), "missing Notes header: {out}");
        assert!(out.contains("- Heads up."), "missing first note: {out}");
        assert!(out.contains("- Second note."), "missing second note: {out}");
    }

    #[test]
    fn warnings_section_renders_as_bullet_list() {
        let out = render("Short.\n\nWarnings:\n    Be careful here.\n");
        assert!(out.contains("## Warnings\n"), "missing Warnings header: {out}");
        assert!(out.contains("- Be careful here."), "missing warning bullet: {out}");
    }

    #[test]
    fn see_also_section_renders_as_bullet_list() {
        let out = render("Short.\n\nSee Also:\n    other_function\n    related_module\n");
        assert!(out.contains("## See Also\n"), "missing See Also header: {out}");
        assert!(out.contains("- other_function"), "missing first see-also: {out}");
        assert!(out.contains("- related_module"), "missing second see-also: {out}");
    }

    #[test]
    fn references_section_renders_as_bullet_list() {
        let out = render("Short.\n\nReferences:\n    https://example.com/spec\n");
        assert!(out.contains("## References\n"), "missing References header: {out}");
        assert!(
            out.contains("- https://example.com/spec"),
            "missing reference bullet: {out}"
        );
    }

    #[test]
    fn todo_section_renders_as_bullet_list() {
        let out = render("Short.\n\nTodo:\n    Improve error messages.\n");
        assert!(out.contains("## Todo\n"), "missing Todo header: {out}");
        assert!(
            out.contains("- Improve error messages."),
            "missing todo bullet: {out}"
        );
    }

    #[test]
    fn deprecated_section_renders_inline() {
        // ``Deprecated:`` is a single-string field, not a list — the
        // renderer emits ``Deprecated:\n<text>`` (no bullet).
        let out = render("Short.\n\nDeprecated:\n    Removed in 2.0.\n");
        assert!(out.contains("## Deprecated\n"), "missing Deprecated header: {out}");
        assert!(
            out.contains("Removed in 2.0."),
            "missing deprecated body: {out}"
        );
    }

    #[test]
    fn version_added_section_renders_inline() {
        // ``Version Added:`` is a single string formatted on one line.
        let out = render("Short.\n\nVersion Added:\n    1.5.0\n");
        assert!(
            out.contains("## Version Added\n\n1.5.0"),
            "missing Version Added line: {out}"
        );
    }

    #[test]
    fn version_changed_section_renders_each_entry_on_its_own_line() {
        // ``Version Changed:`` is a list of (version, description)
        // pairs. The renderer emits ``- <version>: <description>``
        // for each one.
        let out = render(
            "Short.\n\nVersion Changed:\n    1.2.0: tightened input validation.\n    1.4.0: added X.\n",
        );
        assert!(
            out.contains("## Version Changed\n"),
            "missing Version Changed header: {out}"
        );
        assert!(
            out.contains("- 1.2.0: tightened input validation."),
            "missing first version line: {out}"
        );
        assert!(
            out.contains("- 1.4.0: added X."),
            "missing second version line: {out}"
        );
    }

    #[test]
    fn long_paragraph_is_skipped_when_empty_string() {
        // If the parser returns ``Some("")`` for long_description (an
        // edge case where a section header appears immediately after
        // the short paragraph with no body in between), the renderer
        // must not insert a phantom ``\n\n\n`` block.
        let docstring = "Short paragraph.\n\nNotes:\n    A note.\n";
        let out = render(docstring);
        // The short paragraph is followed by either ``\n\n## Notes`` or
        // ``\n\n<long>\n## Notes`` — never ``\n\n\n## Notes``.
        assert!(
            !out.contains("\n\n\n"),
            "empty long_description produced a triple newline: {out:?}"
        );
    }
}
