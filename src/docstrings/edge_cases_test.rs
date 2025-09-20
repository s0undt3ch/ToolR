#[cfg(test)]
mod edge_cases_test {
    use crate::docstrings::SimpleDocstringParser;

    #[test]
    fn test_case_1_single_line_short_description() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Short Description.";
        let result = parser.parse(docstring).expect("Failed to parse case 1");

        assert_eq!(result.short_description, "Short Description.");
        assert_eq!(result.long_description, None);
    }

    #[test]
    fn test_case_2_multiline_short_description() {
        let parser = SimpleDocstringParser::new();
        let docstring = r#"
Short Description.
"#;
        let result = parser.parse(docstring).expect("Failed to parse case 2");

        assert_eq!(result.short_description, "Short Description.");
        assert_eq!(result.long_description, None);
    }

    #[test]
    fn test_case_3_short_and_long_same_line() {
        let parser = SimpleDocstringParser::new();
        let docstring = r#"Short Description.
Long description."#;
        let result = parser.parse(docstring).expect("Failed to parse case 3");

        assert_eq!(result.short_description, "Short Description.");
        assert_eq!(result.long_description, Some("Long description.".to_string()));
    }

    #[test]
    fn test_case_4_short_and_long_with_empty_line() {
        let parser = SimpleDocstringParser::new();
        let docstring = r#"Short Description.

Long description."#;
        let result = parser.parse(docstring).expect("Failed to parse case 4");

        assert_eq!(result.short_description, "Short Description.");
        assert_eq!(result.long_description, Some("Long description.".to_string()));
    }

    #[test]
    fn test_case_5_multiline_with_empty_line() {
        let parser = SimpleDocstringParser::new();
        let docstring = r#"
Short Description.

Long description.
"#;
        let result = parser.parse(docstring).expect("Failed to parse case 5");

        assert_eq!(result.short_description, "Short Description.");
        assert_eq!(result.long_description, Some("Long description.".to_string()));
    }

    #[test]
    fn test_case_6_long_description_until_section() {
        let parser = SimpleDocstringParser::new();
        let docstring = r#"
Short Description.
Long description.

More long description.

Examples:
   ....etc...
"#;
        let result = parser.parse(docstring).expect("Failed to parse case 6");

        assert_eq!(result.short_description, "Short Description.");
        assert_eq!(result.long_description, Some("Long description.\n\nMore long description.".to_string()));
        // Examples section should be parsed separately
        assert!(!result.examples.is_empty());
    }

    #[test]
    fn test_case_6_with_args_section() {
        let parser = SimpleDocstringParser::new();
        let docstring = r#"
Short Description.
Long description.

More long description.

Args:
    param1: Description of param1
"#;
        let result = parser.parse(docstring).expect("Failed to parse case 6 with args");

        assert_eq!(result.short_description, "Short Description.");
        assert_eq!(result.long_description, Some("Long description.\n\nMore long description.".to_string()));
        // Args section should be parsed separately
        assert!(result.params.contains_key("param1"));
    }

    #[test]
    fn test_empty_docstring() {
        let parser = SimpleDocstringParser::new();
        let docstring = "";
        let result = parser.parse(docstring).expect("Failed to parse empty docstring");

        assert_eq!(result.short_description, "");
        assert_eq!(result.long_description, None);
    }

    #[test]
    fn test_whitespace_only_docstring() {
        let parser = SimpleDocstringParser::new();
        let docstring = "   \n  \n  ";
        let result = parser.parse(docstring).expect("Failed to parse whitespace docstring");

        assert_eq!(result.short_description, "");
        assert_eq!(result.long_description, None);
    }

    #[test]
    fn test_single_word_short_description() {
        let parser = SimpleDocstringParser::new();
        let docstring = "Hello";
        let result = parser.parse(docstring).expect("Failed to parse single word");

        assert_eq!(result.short_description, "Hello");
        assert_eq!(result.long_description, None);
    }

    #[test]
    fn test_multiline_long_description() {
        let parser = SimpleDocstringParser::new();
        let docstring = r#"
Short description.

This is a longer description
that spans multiple lines
and provides more detail.

Args:
    param: A parameter
"#;
        let result = parser.parse(docstring).expect("Failed to parse multiline long description");

        assert_eq!(result.short_description, "Short description.");
        assert_eq!(result.long_description, Some("This is a longer description\nthat spans multiple lines\nand provides more detail.".to_string()));
        assert!(result.params.contains_key("param"));
    }
}
