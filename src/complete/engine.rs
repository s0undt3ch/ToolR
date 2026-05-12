//! Pure prefix-matching completion engine. No I/O.

use crate::manifest::Manifest;

/// Compute the list of completion candidates for a fully tokenised
/// command line. `tokens` is everything after `toolr` itself — e.g.
/// `["ci", "hello", "--na"]`. Returns one candidate per line in shell
/// output.
pub fn serve_completions(_manifest: &Manifest, _tokens: &[String]) -> Vec<String> {
    // Filled in by Task 2.
    Vec::new()
}
