use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a parsed Google-style docstring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Docstring {
    /// The first line of the docstring (short description)
    pub short_description: String,
    /// The remaining lines that don't fit into specific sections
    pub long_description: Option<String>,
    /// Parameters section
    pub params: HashMap<String, Option<String>>,
    /// Examples section
    pub examples: Vec<Example>,
    /// Notes section
    pub notes: Vec<String>,
    /// Warnings section
    pub warnings: Vec<String>,
    /// See also section
    pub see_also: Vec<String>,
    /// References section
    pub references: Vec<String>,
    /// Todo section
    pub todo: Vec<String>,
    /// Deprecated section
    pub deprecated: Option<String>,
    /// Version added section
    pub version_added: Option<String>,
    /// Version changed section
    pub version_changed: Vec<VersionChanged>,
}


/// Represents an example in the docstring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Example {
    /// The example description
    pub description: String,
    /// The example code snippet
    pub snippet: String,
    /// The syntax/language identifier (e.g., "python", "rust", "javascript")
    pub syntax: Option<String>,
}

/// Represents a version changed entry
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionChanged {
    /// The version number
    pub version: String,
    /// The description of what changed
    pub description: String,
}

/// Represents a parsing error with suggestions for fixing
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParseError {
    /// The error message
    pub message: String,
    /// The line number where the error occurred
    pub line_number: Option<usize>,
    /// The column number where the error occurred
    pub column_number: Option<usize>,
    /// Suggestions for fixing the error
    pub suggestions: Vec<String>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(line) = self.line_number {
            write!(f, " at line {}", line)?;
        }
        if let Some(col) = self.column_number {
            write!(f, ", column {}", col)?;
        }
        if !self.suggestions.is_empty() {
            write!(f, "\nSuggestions:")?;
            for suggestion in &self.suggestions {
                write!(f, "\n  - {}", suggestion)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

/// Simple but robust parser for Google-style docstrings
pub struct SimpleDocstringParser;

impl SimpleDocstringParser {
    /// Create a new parser
    pub fn new() -> Self {
        Self
    }

    /// Create a new parser with strict mode enabled
    pub fn strict() -> Self {
        Self
    }

    /// Parse a docstring string into a Docstring struct
    pub fn parse(&self, docstring: &str) -> Result<Docstring, ParseError> {
        let lines: Vec<&str> = docstring.lines().collect();
        let mut result = Docstring {
            short_description: String::new(),
            long_description: None,
            params: HashMap::new(),
            examples: Vec::new(),
            notes: Vec::new(),
            warnings: Vec::new(),
            see_also: Vec::new(),
            references: Vec::new(),
            todo: Vec::new(),
            deprecated: None,
            version_added: None,
            version_changed: Vec::new(),
        };
        if lines.is_empty() {
            return Ok(result);
        }

        // Parse the docstring
        self.parse_docstring_content(&lines, &mut result)?;

        Ok(result)
    }

    fn parse_docstring_content(&self, lines: &[&str], result: &mut Docstring) -> Result<(), ParseError> {
        let mut current_section: Option<&str> = None;
        let mut current_content = Vec::new();
        let mut in_code_block = false;
        let mut in_description = true;
        let mut description_lines = Vec::new();

        // First pass: collect all description lines (until we hit a section)
        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Skip empty lines at the beginning
            if description_lines.is_empty() && trimmed.is_empty() {
                continue;
            }

            // Handle code blocks
            if trimmed.starts_with("```") || trimmed.starts_with(">>>") {
                in_code_block = !in_code_block;
                if in_description {
                    description_lines.push(*line);
                } else {
                    current_content.push(*line);
                }
                continue;
            }

            // If we're in a code block, just add the line
            if in_code_block {
                if in_description {
                    description_lines.push(*line);
                } else {
                    current_content.push(*line);
                }
                continue;
            }

            // Check for section headers
            if let Some(section) = self.detect_section(trimmed) {
                // Process previous section if we have one
                if let Some(prev_section) = current_section {
                    self.process_section(prev_section, &current_content, result, line_num)?;
                }

                // We found a new section, so we're no longer in description
                in_description = false;
                current_section = Some(section);
                current_content.clear();
                continue;
            }


            // If we're still in description, add to description lines
            if in_description {
                description_lines.push(*line);
            } else {
                // We're in a section, add to current content
                current_content.push(*line);
            }
        }

        // Parse the description lines
        self.parse_description_lines(&description_lines, result)?;

        // Process the last section if we have one
        if let Some(section) = current_section {
            self.process_section(section, &current_content, result, lines.len())?;
        }

        Ok(())
    }

    fn parse_description_lines(&self, lines: &[&str], result: &mut Docstring) -> Result<(), ParseError> {
        if lines.is_empty() {
            return Ok(());
        }

        // Find the first non-empty line for short description
        for line in lines {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                result.short_description = trimmed.to_string();
                break;
            }
        }

        // Find where long description starts (after short description)
        let mut long_desc_started = false;
        let mut long_desc_lines = Vec::new();

        for line in lines {
            let trimmed = line.trim();

            // Skip empty lines at the beginning
            if !long_desc_started && trimmed.is_empty() {
                continue;
            }

            // If this is the short description line, mark that we've seen it
            if !long_desc_started && trimmed == result.short_description {
                long_desc_started = true;
                continue;
            }

            // If we've seen the short description, everything else is long description
            if long_desc_started {
                long_desc_lines.push(trimmed);
            }
        }

        // Join long description lines, preserving structure
        if !long_desc_lines.is_empty() {
            // Remove leading empty lines
            while !long_desc_lines.is_empty() && long_desc_lines[0].is_empty() {
                long_desc_lines.remove(0);
            }

            // Remove trailing empty lines
            while long_desc_lines.last().is_some_and(|s| s.is_empty()) {
                long_desc_lines.pop();
            }

            if !long_desc_lines.is_empty() {
                result.long_description = Some(long_desc_lines.join("\n"));
            }
        }

        Ok(())
    }

    fn detect_section(&self, line: &str) -> Option<&str> {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if lower.starts_with("args:") || lower.starts_with("arguments:") || lower.starts_with("parameters:") {
            Some("args")
        } else if lower.starts_with("returns:") || lower.starts_with("return:") {
            Some("returns")
        } else if lower.starts_with("yields:") || lower.starts_with("yield:") {
            Some("yields")
        } else if lower.starts_with("raises:") || lower.starts_with("raise:") || lower.starts_with("except:") {
            Some("raises")
        } else if lower.starts_with("attributes:") || lower.starts_with("attrs:") {
            Some("attributes")
        } else if lower.starts_with("attr ") || lower.starts_with("attribute ") {
            Some("attr")
        } else if lower.starts_with("examples:") || lower.starts_with("example:") {
            Some("examples")
        } else if lower.starts_with("notes:") || lower.starts_with("note:") {
            Some("notes")
        } else if lower.starts_with("warnings:") || lower.starts_with("warning:") {
            Some("warnings")
        } else if lower.starts_with("see also:") || lower.starts_with("see:") {
            Some("see_also")
        } else if lower.starts_with("references:") || lower.starts_with("refs:") {
            Some("references")
        } else if lower.starts_with("todo:") {
            Some("todo")
        } else if lower.starts_with("deprecated:") {
            Some("deprecated")
        } else if lower.starts_with("version added:") {
            Some("version_added")
        } else if lower.starts_with("version changed:") {
            Some("version_changed")
        } else {
            None
        }
    }

    fn process_section(&self, section: &str, content: &[&str], result: &mut Docstring, line_num: usize) -> Result<(), ParseError> {
        match section {
            "args" => self.parse_args_section(content, result, line_num),
            "examples" => self.parse_examples_section(content, result, line_num),
            "notes" => self.parse_notes_section(content, result, line_num),
            "warnings" => self.parse_warnings_section(content, result, line_num),
            "see_also" => self.parse_see_also_section(content, result, line_num),
            "references" => self.parse_references_section(content, result, line_num),
            "todo" => self.parse_todo_section(content, result, line_num),
            "deprecated" => self.parse_deprecated_section(content, result, line_num),
            "version_added" => self.parse_version_added_section(content, result, line_num),
            "version_changed" => self.parse_version_changed_section(content, result, line_num),
            // Ignore removed sections: returns, yields, raises, attributes, attr
            _ => Ok(()),
        }
    }

    fn parse_args_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse format: name (type): description
            if let Some((name_part, description)) = trimmed.split_once(':') {
                let name_part = name_part.trim();
                let description = description.trim();

                result.params.insert(name_part.to_string(), Some(description.to_string()));
            }
        }
        Ok(())
    }




    /// Dedent a code snippet by removing common leading whitespace
    fn dedent_snippet(&self, snippet: &str) -> String {
        let lines: Vec<&str> = snippet.lines().collect();
        if lines.is_empty() {
            return String::new();
        }

        // Find the minimum indentation (excluding empty lines)
        let mut min_indent = usize::MAX;
        for line in &lines {
            if !line.trim().is_empty() {
                let indent = line.len() - line.trim_start().len();
                min_indent = min_indent.min(indent);
            }
        }

        // If no indentation found, return as is
        if min_indent == usize::MAX {
            return snippet.to_string();
        }

        // Dedent all lines
        let dedented_lines: Vec<String> = lines
            .iter()
            .map(|line| {
                if line.len() >= min_indent {
                    &line[min_indent..]
                } else {
                    line
                }
            })
            .map(|s| s.to_string())
            .collect();

        dedented_lines.join("\n")
    }

    fn parse_examples_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        let mut current_example = Example {
            description: String::new(),
            snippet: String::new(),
            syntax: None,
        };
        let mut in_code_block = false;
        let mut has_code_blocks = false;

        // First pass: check if there are any code blocks
        for line in content {
            if line.trim().starts_with("```") {
                has_code_blocks = true;
                break;
            }
        }

        for line in content {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                if in_code_block {
                    // Check if we're in a Python REPL code block (no closing marker)
                    let is_python_repl = current_example.snippet.contains(">>>");
                    if is_python_repl {
                        // Python REPL code blocks end on empty lines
                        if !current_example.description.is_empty() || !current_example.snippet.is_empty() {
                            // Dedent the snippet before pushing
                            if !current_example.snippet.is_empty() {
                                current_example.snippet = self.dedent_snippet(&current_example.snippet);
                            }
                            result.examples.push(current_example.clone());
                            current_example = Example {
                                description: String::new(),
                                snippet: String::new(),
                                syntax: None,
                            };
                            in_code_block = false;
                        }
                    } else {
                        // Regular code blocks - preserve empty lines in the snippet
                        if !current_example.snippet.is_empty() {
                            current_example.snippet.push('\n');
                        }
                        current_example.snippet.push_str(line);
                    }
                } else {
                    // Outside code block - empty line ends the current example
                    if !current_example.description.is_empty() || !current_example.snippet.is_empty() {
                        // Dedent the snippet before pushing
                        if !current_example.snippet.is_empty() {
                            current_example.snippet = self.dedent_snippet(&current_example.snippet);
                        }
                        result.examples.push(current_example.clone());
                        current_example = Example {
                            description: String::new(),
                            snippet: String::new(),
                            syntax: None,
                        };
                        in_code_block = false;
                    }
                }
                continue;
            }

            // Check for code block markers
            if trimmed.starts_with("```") {
                if !in_code_block {
                    // Starting a code block - check for syntax identifier
                    let syntax_part = trimmed.strip_prefix("```").unwrap_or("").trim();
                    if !syntax_part.is_empty() {
                        current_example.syntax = Some(syntax_part.to_string());
                    }
                }
                in_code_block = !in_code_block;
                continue;
            }

            // Check for Python REPL-style code blocks (>>>)
            if trimmed.starts_with(">>>") {
                if !in_code_block {
                    // Starting a Python REPL code block
                    current_example.syntax = Some("python".to_string());
                    in_code_block = true;
                }
                // Add the line to the snippet (preserve original indentation)
                if !current_example.snippet.is_empty() {
                    current_example.snippet.push('\n');
                }
                current_example.snippet.push_str(line);
                continue;
            }

            if in_code_block {
                // Inside a code block - this is snippet content
                // Preserve original indentation by using the original line
                if !current_example.snippet.is_empty() {
                    current_example.snippet.push('\n');
                }
                current_example.snippet.push_str(line);
            } else {
                // Outside code block
                if has_code_blocks {
                    // If there are code blocks, treat everything as description
                    if !current_example.description.is_empty() {
                        current_example.description.push('\n');
                    }
                    current_example.description.push_str(trimmed);
                } else {
                    // If no code blocks, treat non-empty lines as descriptions
                    if current_example.description.is_empty() {
                        current_example.description = trimmed.to_string();
                    }
                    // Ignore additional content when there are no code blocks
                }
            }
        }

        if !current_example.description.is_empty() || !current_example.snippet.is_empty() {
            // Dedent the snippet before pushing
            if !current_example.snippet.is_empty() {
                current_example.snippet = self.dedent_snippet(&current_example.snippet);
            }
            result.examples.push(current_example);
        }

        Ok(())
    }

    fn parse_notes_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                result.notes.push(trimmed.to_string());
            }
        }
        Ok(())
    }

    fn parse_warnings_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                result.warnings.push(trimmed.to_string());
            }
        }
        Ok(())
    }

    fn parse_see_also_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                result.see_also.push(trimmed.to_string());
            }
        }
        Ok(())
    }

    fn parse_references_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                result.references.push(trimmed.to_string());
            }
        }
        Ok(())
    }

    fn parse_todo_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                result.todo.push(trimmed.to_string());
            }
        }
        Ok(())
    }

    fn parse_deprecated_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        let description = content.join(" ").trim().to_string();
        if !description.is_empty() {
            result.deprecated = Some(description);
        }
        Ok(())
    }

    fn parse_version_added_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        let version = content.join(" ").trim().to_string();
        if !version.is_empty() {
            result.version_added = Some(version);
        }
        Ok(())
    }

    fn parse_version_changed_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse format: version: description
            if let Some((version, description)) = trimmed.split_once(':') {
                let version = version.trim().to_string();
                let description = description.trim().to_string();

                result.version_changed.push(VersionChanged {
                    version,
                    description,
                });
            } else {
                // If no colon, treat the whole line as version with empty description
                result.version_changed.push(VersionChanged {
                    version: trimmed.to_string(),
                    description: String::new(),
                });
            }
        }
        Ok(())
    }
}

impl Default for SimpleDocstringParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
