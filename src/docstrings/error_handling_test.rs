#[cfg(test)]
mod error_handling_tests {
    use crate::docstrings::{ParseError, SimpleDocstringParser};

    #[test]
    fn test_parse_error_display_with_all_fields() {
        let error = ParseError {
            message: "Test error".to_string(),
            line_number: Some(5),
            column_number: Some(10),
            suggestions: vec!["Fix this".to_string(), "Try that".to_string()],
        };

        let display = format!("{}", error);
        assert!(display.contains("Test error"));
        assert!(display.contains("at line 5"));
        assert!(display.contains("column 10"));
        assert!(display.contains("Suggestions:"));
        assert!(display.contains("- Fix this"));
        assert!(display.contains("- Try that"));
    }

    #[test]
    fn test_parse_error_display_without_optional_fields() {
        let error = ParseError {
            message: "Simple error".to_string(),
            line_number: None,
            column_number: None,
            suggestions: vec![],
        };

        let display = format!("{}", error);
        assert_eq!(display, "Simple error");
    }

    #[test]
    fn test_parse_error_display_with_line_only() {
        let error = ParseError {
            message: "Line error".to_string(),
            line_number: Some(3),
            column_number: None,
            suggestions: vec![],
        };

        let display = format!("{}", error);
        assert!(display.contains("Line error"));
        assert!(display.contains("at line 3"));
        assert!(!display.contains("column"));
    }

    #[test]
    fn test_parse_error_display_with_column_only() {
        let error = ParseError {
            message: "Column error".to_string(),
            line_number: None,
            column_number: Some(7),
            suggestions: vec![],
        };

        let display = format!("{}", error);
        assert!(display.contains("Column error"));
        assert!(!display.contains("at line"));
        assert!(display.contains("column 7"));
    }

    #[test]
    fn test_parse_error_display_with_suggestions_only() {
        let error = ParseError {
            message: "Suggestion error".to_string(),
            line_number: None,
            column_number: None,
            suggestions: vec!["First suggestion".to_string()],
        };

        let display = format!("{}", error);
        assert!(display.contains("Suggestion error"));
        assert!(display.contains("Suggestions:"));
        assert!(display.contains("- First suggestion"));
    }

    #[test]
    fn test_parse_error_error_trait() {
        let error = ParseError {
            message: "Error trait test".to_string(),
            line_number: None,
            column_number: None,
            suggestions: vec![],
        };

        // Test that it implements std::error::Error
        let error_ref: &dyn std::error::Error = &error;
        assert!(error_ref.source().is_none());
    }

    #[test]
    fn test_parser_new() {
        let _parser = SimpleDocstringParser::new();
        // Test that we can create a parser instance
        // The parser creation itself is the test
    }

    #[test]
    fn test_parse_empty_string() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse("");

        assert!(result.is_ok());
        let docstring = result.unwrap();
        assert_eq!(docstring.short_description, "");
        assert_eq!(docstring.long_description, None);
        assert!(docstring.params.is_empty());
        assert!(docstring.examples.is_empty());
        assert!(docstring.notes.is_empty());
        assert!(docstring.warnings.is_empty());
        assert!(docstring.see_also.is_empty());
        assert!(docstring.references.is_empty());
        assert!(docstring.todo.is_empty());
        assert!(docstring.deprecated.is_none());
        assert!(docstring.version_added.is_none());
        assert!(docstring.version_changed.is_empty());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse("   \n  \t  \n   ");

        assert!(result.is_ok());
        let docstring = result.unwrap();
        assert_eq!(docstring.short_description, "");
        assert_eq!(docstring.long_description, None);
    }

    #[test]
    fn test_parse_single_whitespace() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse(" ");

        assert!(result.is_ok());
        let docstring = result.unwrap();
        assert_eq!(docstring.short_description, "");
        assert_eq!(docstring.long_description, None);
    }

    #[test]
    fn test_parse_with_only_newlines() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse("\n\n\n");

        assert!(result.is_ok());
        let docstring = result.unwrap();
        assert_eq!(docstring.short_description, "");
        assert_eq!(docstring.long_description, None);
    }

    #[test]
    fn test_parse_with_mixed_whitespace_and_content() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse("  \n  Hello world  \n  \t  ");

        assert!(result.is_ok());
        let docstring = result.unwrap();
        assert_eq!(docstring.short_description, "Hello world");
        assert_eq!(docstring.long_description, None);
    }

    #[test]
    fn test_parse_with_unknown_section() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Unknown Section:
    This is an unknown section that should be ignored.
";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        assert_eq!(parsed.long_description, Some("Unknown Section:\nThis is an unknown section that should be ignored.".to_string()));
    }

    #[test]
    fn test_parse_with_empty_sections() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Args:

Returns:

Raises:

Notes:

Warnings:
";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        assert_eq!(parsed.long_description, None);
        assert!(parsed.params.is_empty());
        // Note: returns and raises sections are not parsed in our current implementation
        assert!(parsed.notes.is_empty());
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn test_parse_with_whitespace_only_sections() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Args:


Returns:


Raises:
  \t
";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        assert_eq!(parsed.long_description, None);
        assert!(parsed.params.is_empty());
        // Note: returns and raises sections are not parsed in our current implementation
    }

    #[test]
    fn test_parse_with_malformed_parameter() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Args:
    malformed parameter without colon
    another malformed one
";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        // Malformed parameters should be ignored
        assert!(parsed.params.is_empty());
    }

    #[test]
    fn test_parse_with_malformed_raises() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Raises:
    malformed exception without colon
    another malformed one
";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        // Malformed raises should be ignored
        // Note: raises section is not parsed in our current implementation
    }

    #[test]
    fn test_parse_with_malformed_attributes() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Attributes:
    malformed attribute without colon
    another malformed one
";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        // Malformed attributes should be ignored
        // Note: attributes section is not parsed in our current implementation
    }

    #[test]
    fn test_parse_with_version_changed_no_colon() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Version Changed:
    1.0.0
    1.1.0
";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        assert_eq!(parsed.version_changed.len(), 2);
        assert_eq!(parsed.version_changed[0].version, "1.0.0");
        assert_eq!(parsed.version_changed[0].description, "");
        assert_eq!(parsed.version_changed[1].version, "1.1.0");
        assert_eq!(parsed.version_changed[1].description, "");
    }

    #[test]
    fn test_parse_with_version_changed_mixed_format() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Version Changed:
    1.0.0: Added feature
    1.1.0
    1.2.0: Fixed bug
";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        assert_eq!(parsed.version_changed.len(), 3);
        assert_eq!(parsed.version_changed[0].version, "1.0.0");
        assert_eq!(parsed.version_changed[0].description, "Added feature");
        assert_eq!(parsed.version_changed[1].version, "1.1.0");
        assert_eq!(parsed.version_changed[1].description, "");
        assert_eq!(parsed.version_changed[2].version, "1.2.0");
        assert_eq!(parsed.version_changed[2].description, "Fixed bug");
    }

    #[test]
    fn test_parse_with_empty_version_changed() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Version Changed:
";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        assert!(parsed.version_changed.is_empty());
    }

    #[test]
    fn test_parse_with_whitespace_version_changed() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Test function.

Version Changed:


";
        let result = parser.parse(docstring);

        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.short_description, "Test function.");
        assert!(parsed.version_changed.is_empty());
    }
}
