use serde::{Deserialize, Serialize};

/// Represents a parsed Google-style docstring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Docstring {
    /// The first line of the docstring (short description)
    pub short_description: String,
    /// The remaining lines that don't fit into specific sections
    pub long_description: String,
    /// Parameters section
    pub params: Vec<Parameter>,
    /// Returns section
    pub returns: Option<Return>,
    /// Yields section (for generators)
    pub yields: Option<Yield>,
    /// Examples section
    pub examples: Vec<Example>,
    /// Notes section
    pub notes: Vec<String>,
    /// Raises section (exceptions that may be raised)
    pub raises: Vec<Raise>,
    /// Attributes section (for classes)
    pub attributes: Vec<Attribute>,
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
    pub version_changed: Vec<std::collections::HashMap<String, String>>,
}

/// Represents a parameter in the docstring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    /// The parameter name
    pub name: String,
    /// The parameter type (if specified)
    pub param_type: Option<String>,
    /// The parameter description
    pub description: String,
    /// Whether the parameter is optional
    pub is_optional: bool,
    /// Default value (if any)
    pub default_value: Option<String>,
}

/// Represents a return value in the docstring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Return {
    /// The return type (if specified)
    pub return_type: Option<String>,
    /// The return description
    pub description: String,
}

/// Represents a yielded value in the docstring (for generators)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Yield {
    /// The yield type (if specified)
    pub yield_type: Option<String>,
    /// The yield description
    pub description: String,
}

/// Represents an example in the docstring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Example {
    /// The example description
    pub description: Option<String>,
    /// The example code snippet
    pub snippet: String,
}

/// Represents an exception that may be raised
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Raise {
    /// The exception type
    pub exception_type: String,
    /// The description of when this exception is raised
    pub description: String,
}

/// Represents an attribute in the docstring (for classes)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attribute {
    /// The attribute name
    pub name: String,
    /// The attribute type (if specified)
    pub attr_type: Option<String>,
    /// The attribute description
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
            long_description: String::new(),
            params: Vec::new(),
            returns: None,
            yields: None,
            examples: Vec::new(),
            notes: Vec::new(),
            raises: Vec::new(),
            attributes: Vec::new(),
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

        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Skip empty lines at the beginning
            if result.short_description.is_empty() && trimmed.is_empty() {
                continue;
            }

            // Handle code blocks
            if trimmed.starts_with("```") || trimmed.starts_with(">>>") {
                in_code_block = !in_code_block;
                current_content.push(*line);
                continue;
            }

            // If we're in a code block, just add the line
            if in_code_block {
                current_content.push(*line);
                continue;
            }

            // Check for section headers
            if let Some(section) = self.detect_section(trimmed) {
                // Process previous section
                if let Some(prev_section) = current_section {
                    self.process_section(prev_section, &current_content, result, line_num)?;
                }

                current_section = Some(section);
                current_content.clear();
                continue;
            }

            // If we have a section, add to current content
            if current_section.is_some() {
                current_content.push(*line);
            } else {
                // This is part of the description
                if result.short_description.is_empty() {
                    result.short_description = trimmed.to_string();
                } else {
                    if !result.long_description.is_empty() {
                        result.long_description.push('\n');
                    }
                    result.long_description.push_str(trimmed);
                }
            }
        }

        // Process the last section
        if let Some(section) = current_section {
            self.process_section(section, &current_content, result, lines.len())?;
        }

        Ok(())
    }

    fn detect_section(&self, line: &str) -> Option<&str> {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if lower.starts_with("args:") || lower.starts_with("arguments:") || lower.starts_with("parameters:") {
            Some("args")
        } else if lower.starts_with("param ") || lower.starts_with("parameter ") {
            Some("param")
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
            "param" => self.parse_param_section(content, result, line_num),
            "returns" => self.parse_returns_section(content, result, line_num),
            "yields" => self.parse_yields_section(content, result, line_num),
            "raises" => self.parse_raises_section(content, result, line_num),
            "attributes" => self.parse_attributes_section(content, result, line_num),
            "attr" => self.parse_attr_section(content, result, line_num),
            "examples" => self.parse_examples_section(content, result, line_num),
            "notes" => self.parse_notes_section(content, result, line_num),
            "warnings" => self.parse_warnings_section(content, result, line_num),
            "see_also" => self.parse_see_also_section(content, result, line_num),
            "references" => self.parse_references_section(content, result, line_num),
            "todo" => self.parse_todo_section(content, result, line_num),
            "deprecated" => self.parse_deprecated_section(content, result, line_num),
            "version_added" => self.parse_version_added_section(content, result, line_num),
            "version_changed" => self.parse_version_changed_section(content, result, line_num),
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

                if let Some((name, param_type)) = name_part.split_once('(') {
                    let name = name.trim();
                    let param_type = param_type.trim_end_matches(')').trim();

                    result.params.push(Parameter {
                        name: name.to_string(),
                        param_type: Some(param_type.to_string()),
                        description: description.to_string(),
                        is_optional: false,
                        default_value: None,
                    });
                } else {
                    result.params.push(Parameter {
                        name: name_part.to_string(),
                        param_type: None,
                        description: description.to_string(),
                        is_optional: false,
                        default_value: None,
                    });
                }
            }
        }
        Ok(())
    }

    fn parse_param_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse format: param name: description (no type extraction)
            let parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
            if parts.len() >= 3 {
                let name = parts[1];
                let rest = parts[2..].join(" ");

                if let Some((_, description)) = rest.split_once(':') {
                    let description = description.trim();
                        result.params.push(Parameter {
                            name: name.to_string(),
                        param_type: None,
                            description: description.to_string(),
                            is_optional: false,
                            default_value: None,
                        });
                    } else {
                        result.params.push(Parameter {
                            name: name.to_string(),
                            param_type: None,
                        description: rest.to_string(),
                            is_optional: false,
                            default_value: None,
                        });
                }
            }
        }
        Ok(())
    }

    fn parse_returns_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        let description = content.join(" ").trim().to_string();

        // Split on colon but don't extract type - just use the description part
        let description = if let Some((_, desc_part)) = description.split_once(':') {
            desc_part.trim().to_string()
        } else {
            description
        };

        result.returns = Some(Return {
            return_type: None,
            description,
        });

        Ok(())
    }

    fn parse_yields_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        let description = content.join(" ").trim().to_string();

        // Split on colon but don't extract type - just use the description part
        let description = if let Some((_, desc_part)) = description.split_once(':') {
            desc_part.trim().to_string()
        } else {
            description
        };

        result.yields = Some(Yield {
            yield_type: None,
            description,
        });

        Ok(())
    }

    fn parse_raises_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse format: ExceptionType: description
            if let Some((exception_type, description)) = trimmed.split_once(':') {
                result.raises.push(Raise {
                    exception_type: exception_type.trim().to_string(),
                    description: description.trim().to_string(),
                });
            }
        }
        Ok(())
    }

    fn parse_attributes_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse format: name (type): description
            if let Some((name_part, description)) = trimmed.split_once(':') {
                let name_part = name_part.trim();
                let description = description.trim();

                if let Some((name, attr_type)) = name_part.split_once('(') {
                    let name = name.trim();
                    let attr_type = attr_type.trim_end_matches(')').trim();

                    result.attributes.push(Attribute {
                        name: name.to_string(),
                        attr_type: Some(attr_type.to_string()),
                        description: description.to_string(),
                    });
                } else {
                    result.attributes.push(Attribute {
                        name: name_part.to_string(),
                        attr_type: None,
                        description: description.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    fn parse_attr_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        for line in content {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse format: attr name: description (no type extraction)
            let parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
            if parts.len() >= 3 {
                let name = parts[1];
                let rest = parts[2..].join(" ");

                if let Some((_, description)) = rest.split_once(':') {
                    let description = description.trim();
                    result.attributes.push(Attribute {
                        name: name.to_string(),
                        attr_type: None,
                        description: description.to_string(),
                    });
                } else {
                    result.attributes.push(Attribute {
                        name: name.to_string(),
                        attr_type: None,
                        description: rest.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    fn parse_examples_section(&self, content: &[&str], result: &mut Docstring, _line_num: usize) -> Result<(), ParseError> {
        let mut current_example = Example {
            description: None,
            snippet: String::new(),
        };

        for line in content {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                if !current_example.snippet.is_empty() {
                    result.examples.push(current_example.clone());
                    current_example = Example {
                        description: None,
                        snippet: String::new(),
                    };
                }
                continue;
            }

            // Check if this looks like a description (not code)
            if !trimmed.starts_with(">>>") && !trimmed.starts_with("...") && !trimmed.starts_with("```") &&
               !trimmed.starts_with("    ") && !trimmed.starts_with("\t") {
                if current_example.snippet.is_empty() {
                    current_example.description = Some(trimmed.to_string());
                } else {
                    current_example.snippet.push('\n');
                    current_example.snippet.push_str(trimmed);
                }
            } else {
                if !current_example.snippet.is_empty() {
                    current_example.snippet.push('\n');
                }
                current_example.snippet.push_str(trimmed);
            }
        }

        if !current_example.snippet.is_empty() {
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

                let mut version_dict = std::collections::HashMap::new();
                version_dict.insert(version, description);
                result.version_changed.push(version_dict);
            } else {
                // If no colon, treat the whole line as version with empty description
                let mut version_dict = std::collections::HashMap::new();
                version_dict.insert(trimmed.to_string(), String::new());
                result.version_changed.push(version_dict);
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
