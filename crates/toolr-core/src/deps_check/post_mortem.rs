//! Parse Python tracebacks on subprocess stderr looking for
//! `ImportError` / `ModuleNotFoundError`, and produce a rendered
//! report with the original traceback plus the toolr suggestion.

/// One intercepted `ImportError` from a Python subprocess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportErrorReport {
    /// The original, unmodified subprocess stderr captured by the
    /// runner. Always rendered verbatim — toolr only appends.
    pub traceback: String,
    /// `"ImportError"` or `"ModuleNotFoundError"`.
    pub error_class: String,
    /// For `ModuleNotFoundError: No module named 'X'`, the captured
    /// `X`. `None` for the bare `ImportError` form, where the
    /// missing-thing is a name inside an existing module, not a
    /// top-level package.
    pub missing_hint: Option<String>,
}

impl ImportErrorReport {
    /// Render the report exactly as toolr should print it to the
    /// user. The original traceback comes first; the toolr suggestion
    /// is appended on its own line(s) after a blank separator.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(self.traceback.trim_end());
        out.push_str("\n\n");
        match self.missing_hint.as_deref() {
            Some(module) => {
                out.push_str(&format!(
                    "toolr: import `{module}` failed at runtime. \
                     A dependency may be missing - run \
                     `toolr project deps sync` and check \
                     tools/pyproject.toml.\n"
                ));
            }
            None => {
                out.push_str(
                    "toolr: import failed at runtime. \
                     A dependency may be missing - run \
                     `toolr project deps sync` and check \
                     tools/pyproject.toml.\n",
                );
            }
        }
        out
    }
}

/// Inspect captured stderr from a Python subprocess. If it ends in an
/// `ImportError` / `ModuleNotFoundError`, return a structured report.
/// Otherwise return `None` and let normal error handling take over.
///
/// **Heuristic.** Python's `traceback.print_exc()` puts the exception
/// class and message on the **last non-empty line**. We scan from
/// the end backwards for the first non-empty line and pattern-match
/// against `ModuleNotFoundError: No module named '...'` and
/// `ImportError: ...`.
pub fn intercept_import_error(stderr: &str) -> Option<ImportErrorReport> {
    let last = stderr.lines().rev().find(|line| !line.trim().is_empty())?;
    if let Some(rest) = last.strip_prefix("ModuleNotFoundError: ") {
        let hint = extract_quoted_module(rest);
        return Some(ImportErrorReport {
            traceback: stderr.to_string(),
            error_class: "ModuleNotFoundError".to_string(),
            missing_hint: hint,
        });
    }
    if last.starts_with("ImportError: ") {
        return Some(ImportErrorReport {
            traceback: stderr.to_string(),
            error_class: "ImportError".to_string(),
            missing_hint: None,
        });
    }
    None
}

/// Pull the module name out of `No module named 'X'` or
/// `No module named "X"`. Tolerant of additional text after the quoted
/// name (Python sometimes adds `; 'X' is not a package`).
fn extract_quoted_module(message: &str) -> Option<String> {
    let prefix = "No module named ";
    let rest = message.strip_prefix(prefix)?;
    let bytes = rest.as_bytes();
    let (quote, start) = match bytes.first()? {
        b'\'' => ('\'', 1),
        b'"' => ('"', 1),
        _ => return None,
    };
    let after_open = &rest[start..];
    let end = after_open.find(quote)?;
    Some(after_open[..end].to_string())
}
