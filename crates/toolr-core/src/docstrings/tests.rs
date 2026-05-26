// Include the edge case tests
include!("edge_cases_test.rs");

// Include the original tests (will be fixed)
include!("docstring_test.rs");

// Include the error handling tests (will be fixed)
include!("error_handling_test.rs");

// Round-trip guard between KNOWN_SECTION_HEADERS and detect_section.
include!("known_headers_test.rs");
