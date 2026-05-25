//! Embedded scaffold templates for `toolr project init`.

/// Where the tools venv should live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VenvLocation {
    Cache,
    InTree,
}

impl VenvLocation {
    pub fn as_str(self) -> &'static str {
        match self {
            VenvLocation::Cache => "cache",
            VenvLocation::InTree => "in-tree",
        }
    }
}

/// Render-time options for the scaffold templates.
#[derive(Debug)]
pub struct ScaffoldOptions {
    pub requires_python: String,
    pub venv_location: VenvLocation,
    pub include_example: bool,
}

const PYPROJECT_TMPL: &str = include_str!("init_templates/pyproject.toml.tmpl");
const GITIGNORE: &str = include_str!("init_templates/gitignore.tmpl");
const EXAMPLE_PY: &str = include_str!("init_templates/example.py.tmpl");

/// One rendered file ready to be written to disk.
#[derive(Debug)]
pub struct RenderedFile {
    pub relative_path: &'static str,
    pub contents: String,
}

/// Render every file the scaffold should write, in deterministic order.
pub fn render_all(opts: &ScaffoldOptions) -> Vec<RenderedFile> {
    let mut out = Vec::with_capacity(3);
    out.push(RenderedFile {
        relative_path: "pyproject.toml",
        contents: render_pyproject(opts),
    });
    out.push(RenderedFile {
        relative_path: ".gitignore",
        contents: GITIGNORE.to_string(),
    });
    if opts.include_example {
        out.push(RenderedFile {
            relative_path: "example.py",
            contents: EXAMPLE_PY.to_string(),
        });
    }
    out
}

fn render_pyproject(opts: &ScaffoldOptions) -> String {
    PYPROJECT_TMPL
        .replace("{REQUIRES_PYTHON}", &opts.requires_python)
        .replace("{VENV_LOCATION}", opts.venv_location.as_str())
}

/// Parse the venv-location CLI value.
pub fn parse_venv_location(value: &str) -> anyhow::Result<VenvLocation> {
    match value {
        "cache" => Ok(VenvLocation::Cache),
        "in-tree" => Ok(VenvLocation::InTree),
        other => anyhow::bail!("invalid --venv-location value: {other} (use cache or in-tree)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_pyproject_substitutes_placeholders() {
        let opts = ScaffoldOptions {
            requires_python: ">=3.13".into(),
            venv_location: VenvLocation::InTree,
            include_example: true,
        };
        let rendered = render_pyproject(&opts);
        assert!(rendered.contains(r#"requires-python = ">=3.13""#));
        assert!(rendered.contains(r#"venv-location = "in-tree""#));
        assert!(!rendered.contains("{REQUIRES_PYTHON}"));
        assert!(!rendered.contains("{VENV_LOCATION}"));
    }

    #[test]
    fn render_all_with_example_returns_three_files() {
        let opts = ScaffoldOptions {
            requires_python: ">=3.11".into(),
            venv_location: VenvLocation::Cache,
            include_example: true,
        };
        let files = render_all(&opts);
        let names: Vec<_> = files.iter().map(|f| f.relative_path).collect();
        assert_eq!(names, vec!["pyproject.toml", ".gitignore", "example.py"]);
    }

    #[test]
    fn render_all_without_example_returns_two_files() {
        let opts = ScaffoldOptions {
            requires_python: ">=3.11".into(),
            venv_location: VenvLocation::Cache,
            include_example: false,
        };
        let files = render_all(&opts);
        let names: Vec<_> = files.iter().map(|f| f.relative_path).collect();
        assert_eq!(names, vec!["pyproject.toml", ".gitignore"]);
    }

    #[test]
    fn parse_venv_location_accepts_both_known_values() {
        assert_eq!(parse_venv_location("cache").unwrap(), VenvLocation::Cache);
        assert_eq!(parse_venv_location("in-tree").unwrap(), VenvLocation::InTree);
    }

    #[test]
    fn parse_venv_location_rejects_unknown_values() {
        assert!(parse_venv_location("system").is_err());
    }

    #[test]
    fn example_template_is_non_empty_and_mentions_each_command() {
        assert!(EXAMPLE_PY.contains("def hello("));
        assert!(EXAMPLE_PY.contains("def commit("));
        assert!(EXAMPLE_PY.contains("def confirm("));
        assert!(EXAMPLE_PY.contains("def setlog("));
        assert!(EXAMPLE_PY.contains("Literal["));
    }
}
