#[cfg(test)]
mod known_headers_test {
    use crate::docstrings::{KNOWN_SECTION_HEADERS, SimpleDocstringParser};

    /// Every header in [`KNOWN_SECTION_HEADERS`] must round-trip through
    /// [`SimpleDocstringParser::detect_section`] to the category the
    /// table pairs it with. This is the load-bearing contract: the
    /// `xtask build-skill-refs` generator publishes the table verbatim
    /// in `skills/toolr-command-authoring/references/docstrings.md`,
    /// so a header that "documents but does not parse" would mislead
    /// downstream agents.
    #[test]
    fn every_known_header_round_trips_through_detect_section() {
        let parser = SimpleDocstringParser::new();
        for (header, category) in KNOWN_SECTION_HEADERS {
            // Headers ending in a space are inline forms — `attr <name>`,
            // `attribute <name>` — and only match when followed by
            // content. Append a stub so the probe matches the real
            // parser path. Colon-suffixed headers match standalone.
            let probe = if header.ends_with(' ') {
                format!("{header}name")
            } else {
                (*header).to_string()
            };
            assert_eq!(
                parser.detect_section(&probe),
                Some(*category),
                "probe `{probe}` (from header `{header}`) should map to category `{category}`",
            );
        }
    }

    /// `KNOWN_SECTION_HEADERS` must be sorted ASCII by header spelling.
    /// The generator iterates it as-is, and a stable sort is part of
    /// `--check`'s byte-identity guarantee — `BTreeMap` won't save us
    /// if the source table itself drifts.
    #[test]
    fn known_section_headers_table_is_ascii_sorted() {
        let mut prev: Option<&str> = None;
        for (header, _) in KNOWN_SECTION_HEADERS {
            if let Some(p) = prev {
                assert!(
                    p < *header,
                    "KNOWN_SECTION_HEADERS not sorted: `{p}` >= `{header}`",
                );
            }
            prev = Some(header);
        }
    }

    /// Detection is case-insensitive in the parser; the table stores
    /// lowercase prefixes. Spot-check the most common mixed-case forms.
    #[test]
    fn detect_section_is_case_insensitive() {
        let parser = SimpleDocstringParser::new();
        assert_eq!(parser.detect_section("Args:"), Some("args"));
        assert_eq!(parser.detect_section("ARGS:"), Some("args"));
        assert_eq!(parser.detect_section("Returns:"), Some("returns"));
        assert_eq!(parser.detect_section("See Also:"), Some("see_also"));
    }
}
