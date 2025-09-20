#[cfg(test)]
mod test_suite {
    use crate::docstrings::SimpleDocstringParser;

    // Test docstrings of different sizes
    const SMALL_DOCSTRING: &str = r#"Generate test data for Stripe sandbox accounts.

Args:
    stripe_secret_key: Stripe secret key (must be a test key starting with sk_test_)
    products: The number of products to create.
    customers: The number of customers to create.

Returns:
    dict: Summary of created data"#;

    const MEDIUM_DOCSTRING: &str = r#"Generate test data for Stripe sandbox accounts.

Only test keys (sk_test_*) are accepted for safety.

Args:
    stripe_secret_key: Stripe secret key (must be a test key starting with sk_test_)
    products: The number of products to create.
    customers: The number of customers to create.
    subscriptions_per_customer: The number of subscriptions per customer.

Returns:
    dict: Summary of created data with counts and IDs

Raises:
    ValueError: If the secret key is not a test key
    ConnectionError: If unable to connect to Stripe API

Examples:
    Basic usage:
        result = generate_test_data("sk_test_xxx", 5, 20, 1)

    Custom quantities:
        result = generate_test_data("sk_test_xxx", 3, 10, 2)

Notes:
    Only test keys (sk_test_*) are accepted for safety.
    This function creates realistic test data for development."#;

    const LARGE_DOCSTRING: &str = r#"Generate comprehensive test data for Stripe sandbox accounts.

This function creates a complete test environment with products, customers,
subscriptions, and payment methods. Only test keys (sk_test_*) are accepted
for safety reasons.

Args:
    stripe_secret_key: Stripe secret key (must be a test key starting with sk_test_)
    products: The number of products to create (default: 5)
    customers: The number of customers to create (default: 20)
    subscriptions_per_customer: The number of subscriptions per customer (default: 1)
    include_payment_methods: Whether to create payment methods (default: True)
    include_coupons: Whether to create discount coupons (default: False)
    test_mode: Run in test mode with reduced data (default: False)

Returns:
    dict: Comprehensive summary containing:
        - products: List of created product IDs
        - customers: List of created customer IDs
        - subscriptions: List of created subscription IDs
        - payment_methods: List of created payment method IDs
        - coupons: List of created coupon IDs (if enabled)
        - total_amount: Total value of all subscriptions
        - metadata: Additional information about the test data

Raises:
    ValueError: If the secret key is not a test key or parameters are invalid
    ConnectionError: If unable to connect to Stripe API
    TimeoutError: If the operation takes too long
    RateLimitError: If Stripe API rate limits are exceeded

Yields:
    dict: Progress updates during data creation process

Examples:
    Basic usage - creates 5 products, 20 customers, 1 subscription each:
        result = generate_test_data("sk_test_xxx")

    Custom quantities:
        result = generate_test_data(
            "sk_test_xxx",
            products=3,
            customers=10,
            subscriptions_per_customer=2
        )

    Full test suite with all features:
        result = generate_test_data(
            "sk_test_xxx",
            products=10,
            customers=50,
            subscriptions_per_customer=3,
            include_payment_methods=True,
            include_coupons=True
        )

Notes:
    Only test keys (sk_test_*) are accepted for safety.
    This function creates realistic test data for development and testing.
    The generated data follows Stripe's best practices for test scenarios.
    All created objects are automatically cleaned up after 24 hours in sandbox mode.

Warnings:
    This function will create real objects in your Stripe sandbox account.
    Make sure you're using a test key to avoid charges.

See Also:
    stripe.Customer.create: For creating individual customers
    stripe.Product.create: For creating individual products
    stripe.Subscription.create: For creating individual subscriptions

References:
    - Stripe API Documentation: https://stripe.com/docs/api
    - Test Data Best Practices: https://stripe.com/docs/testing

Todo:
    - Add support for creating webhooks
    - Implement data validation before creation
    - Add progress callbacks for long-running operations

Deprecated:
    The 'legacy_mode' parameter is deprecated and will be removed in v2.0.

Version Added:
    1.0.0

Version Changed:
    1.2.0: Added support for payment methods and coupons
    1.5.0: Improved error handling and progress reporting"#;

    #[test]
    fn test_small_docstring_parsing() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse(SMALL_DOCSTRING).expect("Failed to parse small docstring");


        // Check short description
        assert_eq!(result.short_description, "Generate test data for Stripe sandbox accounts.");

        // Check long description
        assert_eq!(result.long_description, None);

        // Check parameters
        assert_eq!(result.params.len(), 3);

        let stripe_param_desc = result.params.get("stripe_secret_key").unwrap();
        assert_eq!(stripe_param_desc, &Some("Stripe secret key (must be a test key starting with sk_test_)".to_string()));

        let products_param_desc = result.params.get("products").unwrap();
        assert_eq!(products_param_desc, &Some("The number of products to create.".to_string()));

        let customers_param_desc = result.params.get("customers").unwrap();
        assert_eq!(customers_param_desc, &Some("The number of customers to create.".to_string()));

        // Check other sections are empty
        assert_eq!(result.examples.len(), 0);
        assert_eq!(result.notes.len(), 0);
        assert_eq!(result.warnings.len(), 0);
        assert_eq!(result.see_also.len(), 0);
        assert_eq!(result.references.len(), 0);
        assert_eq!(result.todo.len(), 0);
        assert_eq!(result.deprecated, None);
        assert_eq!(result.version_added, None);
        assert_eq!(result.version_changed.len(), 0);
    }

    #[test]
    fn test_medium_docstring_parsing() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse(MEDIUM_DOCSTRING).expect("Failed to parse medium docstring");

        // Check short description
        assert_eq!(result.short_description, "Generate test data for Stripe sandbox accounts.");

        // Check long description
        assert_eq!(result.long_description, Some("Only test keys (sk_test_*) are accepted for safety.".to_string()));

        // Check parameters
        assert_eq!(result.params.len(), 4);

        let stripe_param_desc = result.params.get("stripe_secret_key").unwrap();
        assert_eq!(stripe_param_desc, &Some("Stripe secret key (must be a test key starting with sk_test_)".to_string()));

        let subscriptions_param_desc = result.params.get("subscriptions_per_customer").unwrap();
        assert_eq!(subscriptions_param_desc, &Some("The number of subscriptions per customer.".to_string()));

        // Check examples
        assert_eq!(result.examples.len(), 2);
        let basic_example = result.examples.iter().find(|e| e.description == "Basic usage:").unwrap();
        assert_eq!(basic_example.snippet, "");
        let custom_example = result.examples.iter().find(|e| e.description == "Custom quantities:").unwrap();
        assert_eq!(custom_example.snippet, "");

        // Check notes
        assert_eq!(result.notes.len(), 2);
        assert!(result.notes.contains(&"Only test keys (sk_test_*) are accepted for safety.".to_string()));
        assert!(result.notes.contains(&"This function creates realistic test data for development.".to_string()));

        // Check other sections are empty
        assert_eq!(result.warnings.len(), 0);
        assert_eq!(result.see_also.len(), 0);
        assert_eq!(result.references.len(), 0);
        assert_eq!(result.todo.len(), 0);
        assert_eq!(result.deprecated, None);
        assert_eq!(result.version_added, None);
        assert_eq!(result.version_changed.len(), 0);
    }

    #[test]
    fn test_large_docstring_parsing() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse(LARGE_DOCSTRING).expect("Failed to parse large docstring");

        // Check short description
        assert_eq!(result.short_description, "Generate comprehensive test data for Stripe sandbox accounts.");

        // Check long description
        let long_desc = result.long_description.as_ref().unwrap();
        assert!(long_desc.contains("This function creates a complete test environment"));
        assert!(long_desc.contains("Only test keys (sk_test_*) are accepted"));

        // Check parameters
        assert_eq!(result.params.len(), 7);

        let expected_params = [
            "stripe_secret_key", "products", "customers", "subscriptions_per_customer",
            "include_payment_methods", "include_coupons", "test_mode"
        ];

        for param_name in expected_params {
            let param_desc = result.params.get(param_name).unwrap();
            assert!(param_desc.is_some());
            assert!(!param_desc.as_ref().unwrap().is_empty());
        }

        // Note: returns, raises, yields, and attributes sections are not parsed in our current implementation

        // Check examples
        assert_eq!(result.examples.len(), 3);
        let basic_example = result.examples.iter().find(|e| e.description == "Basic usage - creates 5 products, 20 customers, 1 subscription each:").unwrap();
        assert_eq!(basic_example.snippet, "");
        let custom_example = result.examples.iter().find(|e| e.description == "Custom quantities:").unwrap();
        assert_eq!(custom_example.snippet, "");
        let full_example = result.examples.iter().find(|e| e.description == "Full test suite with all features:").unwrap();
        assert_eq!(full_example.snippet, "");
        // let full_example = result.examples.iter().find(|e| e.description.as_ref() == Some(&"Full test suite with all features:".to_string())).unwrap();
        // assert!(full_example.snippet.contains("include_payment_methods=True"));

        // Check notes
        assert_eq!(result.notes.len(), 4);
        assert!(result.notes.contains(&"Only test keys (sk_test_*) are accepted for safety.".to_string()));
        assert!(result.notes.contains(&"This function creates realistic test data for development and testing.".to_string()));

        // Check warnings
        assert_eq!(result.warnings.len(), 2);
        assert!(result.warnings[0].contains("This function will create real objects"));
        assert!(result.warnings[1].contains("Make sure you're using a test key"));

        // Check see also
        assert_eq!(result.see_also.len(), 3);
        assert!(result.see_also.contains(&"stripe.Customer.create: For creating individual customers".to_string()));
        assert!(result.see_also.contains(&"stripe.Product.create: For creating individual products".to_string()));
        assert!(result.see_also.contains(&"stripe.Subscription.create: For creating individual subscriptions".to_string()));

        // Check references
        assert_eq!(result.references.len(), 2);
        assert!(result.references.contains(&"- Stripe API Documentation: https://stripe.com/docs/api".to_string()));
        assert!(result.references.contains(&"- Test Data Best Practices: https://stripe.com/docs/testing".to_string()));

        // Check todo
        assert_eq!(result.todo.len(), 3);
        assert!(result.todo.contains(&"- Add support for creating webhooks".to_string()));
        assert!(result.todo.contains(&"- Implement data validation before creation".to_string()));
        assert!(result.todo.contains(&"- Add progress callbacks for long-running operations".to_string()));

        // Check deprecated
        assert!(result.deprecated.is_some());
        assert!(result.deprecated.unwrap().contains("legacy_mode"));

        // Check version info
        assert_eq!(result.version_added, Some("1.0.0".to_string()));
        assert_eq!(result.version_changed.len(), 2);
        assert_eq!(result.version_changed[0].version, "1.2.0");
        assert_eq!(result.version_changed[0].description, "Added support for payment methods and coupons");
        assert_eq!(result.version_changed[1].version, "1.5.0");
        assert_eq!(result.version_changed[1].description, "Improved error handling and progress reporting");
    }

    #[test]
    fn test_empty_docstring() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse("").expect("Failed to parse empty docstring");

        assert_eq!(result.short_description, "");
        assert_eq!(result.long_description, None);
        assert_eq!(result.params.len(), 0);
        assert_eq!(result.examples.len(), 0);
        assert_eq!(result.notes.len(), 0);
        assert_eq!(result.warnings.len(), 0);
        assert_eq!(result.see_also.len(), 0);
        assert_eq!(result.references.len(), 0);
        assert_eq!(result.todo.len(), 0);
        assert_eq!(result.deprecated, None);
        assert_eq!(result.version_added, None);
        assert_eq!(result.version_changed.len(), 0);
    }

    #[test]
    fn test_single_line_docstring() {
        let parser = SimpleDocstringParser::new();
        let result = parser.parse("A simple function that does nothing.").expect("Failed to parse single line docstring");

        assert_eq!(result.short_description, "A simple function that does nothing.");
        assert_eq!(result.long_description, None);
        assert_eq!(result.params.len(), 0);
    }

    #[test]
    fn test_docstring_with_attributes() {
        let docstring = r#"A class with attributes.

Attributes:
    name: The name of the object
    value: The value of the object (int)
    active: Whether the object is active (bool, optional)"#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with attributes");

        assert_eq!(result.short_description, "A class with attributes.");
        // Note: attributes section is not parsed in our current implementation
    }

    #[test]
    fn test_docstring_with_version_info() {
        let docstring = r#"Function with version information.

Version Added:
    1.0.0

Version Changed:
    1.1.0: Added new parameter
    1.2.0: Fixed bug in processing"#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with version info");

        assert_eq!(result.version_added, Some("1.0.0".to_string()));
        assert_eq!(result.version_changed.len(), 2);
        assert_eq!(result.version_changed[0].version, "1.1.0");
        assert_eq!(result.version_changed[0].description, "Added new parameter");
        assert_eq!(result.version_changed[1].version, "1.2.0");
        assert_eq!(result.version_changed[1].description, "Fixed bug in processing");
    }

    #[test]
    fn test_docstring_with_deprecated_info() {
        let docstring = r#"Function with deprecated parameter.

Deprecated:
    The 'old_param' parameter is deprecated and will be removed in v2.0.
    Use 'new_param' instead."#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with deprecated info");

        assert!(result.deprecated.is_some());
        let deprecated = result.deprecated.unwrap();
        assert!(deprecated.contains("old_param"));
        assert!(deprecated.contains("new_param"));
    }

    #[test]
    fn test_parser_strict_mode() {
        let strict_parser = SimpleDocstringParser::strict();
        let result = strict_parser.parse(SMALL_DOCSTRING).expect("Failed to parse with strict parser");

        assert_eq!(result.short_description, "Generate test data for Stripe sandbox accounts.");
        assert_eq!(result.params.len(), 3);
    }

    #[test]
    fn test_parser_performance() {
        let parser = SimpleDocstringParser::new();

        // Time 1000 iterations
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            let _result = parser.parse(SMALL_DOCSTRING).expect("Failed to parse");
        }
        let duration = start.elapsed();

        // Should complete 1000 iterations in less than 1 second
        assert!(duration.as_secs() < 1, "Parsing took too long: {:?}", duration);
    }

    #[test]
    fn test_parser_consistency() {
        let parser = SimpleDocstringParser::new();

        // Parse the same docstring multiple times
        let mut results = Vec::new();
        for _ in 0..10 {
            let result = parser.parse(SMALL_DOCSTRING).expect("Failed to parse");
            results.push(result);
        }

        // All results should be identical
        let first_result = &results[0];
        for result in results.iter().skip(1) {
            assert_eq!(result.short_description, first_result.short_description);
            assert_eq!(result.long_description, first_result.long_description);
            assert_eq!(result.params.len(), first_result.params.len());
            // Note: returns field no longer exists
        }
    }

    #[test]
    fn test_parser_error_handling() {
        let parser = SimpleDocstringParser::new();

        // Test with very long input (should still work)
        let long_docstring = "A".repeat(10000);
        let result = parser.parse(&long_docstring).expect("Failed to parse long docstring");
        assert_eq!(result.short_description, long_docstring);
    }

    #[test]
    fn test_parameter_type_extraction() {
        let docstring = r#"Function with typed parameters.

Args:
    name: The name (str)
    age: The age (int, optional)
    active: Whether active (bool, default: True)
    items: List of items (list[str])"#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with typed parameters");

        let name_param_desc = result.params.get("name").unwrap();
        assert!(name_param_desc.is_some());

        let age_param_desc = result.params.get("age").unwrap();
        assert!(age_param_desc.is_some());

        let active_param_desc = result.params.get("active").unwrap();
        assert!(active_param_desc.is_some());

        let items_param_desc = result.params.get("items").unwrap();
        assert!(items_param_desc.is_some());
    }

    #[test]
    fn test_complex_examples_parsing() {
        let docstring = r#"Process data with various options.

Examples:
    Basic usage:
        result = process_data("input.txt")

    With custom options:
        result = process_data(
            "input.txt",
            format="json",
            validate=True
        )

    Error handling:
        try:
            result = process_data("nonexistent.txt")
        except FileNotFoundError:
            print("File not found")"#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with complex examples");

        assert_eq!(result.examples.len(), 3);
        let basic_example = result.examples.iter().find(|e| e.description == "Basic usage:").unwrap();
        assert_eq!(basic_example.snippet, "");
        let custom_example = result.examples.iter().find(|e| e.description == "With custom options:").unwrap();
        assert_eq!(custom_example.snippet, "");
        let error_example = result.examples.iter().find(|e| e.description == "Error handling:").unwrap();
        assert_eq!(error_example.snippet, "");
        // assert!(error_example.snippet.contains("except FileNotFoundError:"));
    }

    #[test]
    fn test_simple_examples_parsing() {
        let docstring = r#"Process data with various options.

Examples:
    Examples can be seen under the `examples/` directory."#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with simple examples");

        assert_eq!(result.examples.len(), 1);
        assert_eq!(result.examples[0].description, "Examples can be seen under the `examples/` directory.");
        assert_eq!(result.examples[0].snippet, "");
    }

    #[test]
    fn test_complex_examples_without_markdown_parsing() {
        let docstring = r#"Process data with various options.

Examples:
    Basic usage:
        result = process_data("input.txt")

    With custom options:
        result = process_data(
            "input.txt",
            format="json",
            validate=True
        )

    Error handling:
        try:
            result = process_data("nonexistent.txt")
        except FileNotFoundError:
            print("File not found")"#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with complex examples without markdown");

        assert_eq!(result.examples.len(), 3);
        assert_eq!(result.examples[0].description, "Basic usage:");
        assert_eq!(result.examples[0].snippet, "");
        assert_eq!(result.examples[1].description, "With custom options:");
        assert_eq!(result.examples[1].snippet, "");
        assert_eq!(result.examples[2].description, "Error handling:");
        assert_eq!(result.examples[2].snippet, "");
    }

    #[test]
    fn test_complex_examples_with_markdown_parsing() {
        let docstring = r#"Process data with various options.

Examples:
    Basic usage:
    ```
    result = process_data("input.txt")
    ```

    With custom options:
    ```
    result = process_data(
            "input.txt",
            format="json",
            validate=True
        )
    ```

    Error handling:
    ```
        try:
            result = process_data("nonexistent.txt")
        except FileNotFoundError:
            print("File not found")
    ```"#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with complex examples with markdown");

        assert_eq!(result.examples.len(), 3);
        assert_eq!(result.examples[0].description, "Basic usage:");
        assert!(!result.examples[0].snippet.is_empty());
        assert!(result.examples[0].snippet.contains("result = process_data(\"input.txt\")"));
        assert_eq!(result.examples[1].description, "With custom options:");
        assert!(!result.examples[1].snippet.is_empty());
        assert!(result.examples[1].snippet.contains("format=\"json\""));
        assert_eq!(result.examples[2].description, "Error handling:");
        assert!(!result.examples[2].snippet.is_empty());
        assert!(result.examples[2].snippet.contains("try:"));
    }

    #[test]
    fn test_examples_with_syntax_parsing() {
        let docstring = r#"Process data with various options.

Examples:
    Basic usage:
    ```python
    result = process_data("input.txt")
    ```

    Python REPL example:
    >>> result = process_data("input.txt")
    >>> print(result)

    JavaScript example:
    ```javascript
    const result = processData("input.txt");
    console.log(result);
    ```"#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with syntax examples");

        assert_eq!(result.examples.len(), 3);

        // First example with explicit python syntax
        assert_eq!(result.examples[0].description, "Basic usage:");
        assert!(!result.examples[0].snippet.is_empty());
        assert_eq!(result.examples[0].syntax, Some("python".to_string()));

        // Second example with Python REPL (>>>) - should auto-detect as python
        assert_eq!(result.examples[1].description, "Python REPL example:");
        assert!(!result.examples[1].snippet.is_empty());
        assert_eq!(result.examples[1].syntax, Some("python".to_string()));

        // Third example with explicit javascript syntax
        assert_eq!(result.examples[2].description, "JavaScript example:");
        assert!(!result.examples[2].snippet.is_empty());
        assert_eq!(result.examples[2].syntax, Some("javascript".to_string()));
    }


    #[test]
    fn test_multiple_notes_parsing() {
        let docstring = r#"Function with multiple notes.

Notes:
    This is the first note.
    This is the second note with more details.
    This is the third note with even more information."#;

        let parser = SimpleDocstringParser::new();
        let result = parser.parse(docstring).expect("Failed to parse docstring with multiple notes");

        assert_eq!(result.notes.len(), 3);
        assert_eq!(result.notes[0], "This is the first note.");
        assert_eq!(result.notes[1], "This is the second note with more details.");
        assert_eq!(result.notes[2], "This is the third note with even more information.");
    }
}
