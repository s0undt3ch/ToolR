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
mod tests;
