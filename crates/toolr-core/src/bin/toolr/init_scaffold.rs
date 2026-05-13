//! Scaffold writer for `toolr project init`. Atomically writes the
//! rendered template files into `<cwd>/tools/`, rolling back any
//! partial state on failure.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow};

use crate::init_templates::{ScaffoldOptions, render_all};

/// Outcome of a successful scaffold.
#[derive(Debug)]
pub struct ScaffoldOutcome {
    pub tools_dir: PathBuf,
    pub files_written: Vec<PathBuf>,
}

/// Scaffold `tools/` under `cwd`, refusing without `force` if `tools/`
/// already exists and is non-empty.
pub fn scaffold(cwd: &Path, opts: &ScaffoldOptions, force: bool) -> Result<ScaffoldOutcome> {
    let tools_dir = cwd.join("tools");
    if tools_dir.exists() && !force {
        let mut iter = fs::read_dir(&tools_dir)
            .with_context(|| format!("reading {}", tools_dir.display()))?;
        if iter.next().is_some() {
            return Err(anyhow!(
                "tools/ already exists at {} (use --force to overwrite)",
                tools_dir.display()
            ));
        }
    }
    fs::create_dir_all(&tools_dir)
        .with_context(|| format!("creating {}", tools_dir.display()))?;

    let rendered = render_all(opts);
    let mut written: Vec<PathBuf> = Vec::with_capacity(rendered.len());
    for file in &rendered {
        let dest = tools_dir.join(file.relative_path);
        if let Err(e) = write_file(&dest, &file.contents) {
            for path in written.iter().rev() {
                let _ = fs::remove_file(path);
            }
            return Err(e).with_context(|| format!("writing {}", dest.display()));
        }
        written.push(dest);
    }
    Ok(ScaffoldOutcome {
        tools_dir,
        files_written: written,
    })
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    let tmp = with_extension(path, "tmp");
    {
        let mut f =
            fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        f.write_all(contents.as_bytes())?;
        f.flush()?;
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

fn with_extension(path: &Path, extra: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".");
    name.push(extra);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_templates::VenvLocation;
    use tempfile::TempDir;

    fn opts() -> ScaffoldOptions {
        ScaffoldOptions {
            requires_python: ">=3.11".into(),
            venv_location: VenvLocation::Cache,
            include_example: true,
        }
    }

    #[test]
    fn scaffold_writes_three_files() {
        let tmp = TempDir::new().unwrap();
        let outcome = scaffold(tmp.path(), &opts(), false).unwrap();
        assert_eq!(outcome.tools_dir, tmp.path().join("tools"));
        assert_eq!(outcome.files_written.len(), 3);
        assert!(tmp.path().join("tools/pyproject.toml").is_file());
        assert!(tmp.path().join("tools/.gitignore").is_file());
        assert!(tmp.path().join("tools/example.py").is_file());
    }

    #[test]
    fn scaffold_without_example_writes_two_files() {
        let tmp = TempDir::new().unwrap();
        let mut o = opts();
        o.include_example = false;
        let outcome = scaffold(tmp.path(), &o, false).unwrap();
        assert_eq!(outcome.files_written.len(), 2);
        assert!(!tmp.path().join("tools/example.py").exists());
    }

    #[test]
    fn scaffold_refuses_when_tools_non_empty_without_force() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("existing.py"), "x = 1").unwrap();

        let err = scaffold(tmp.path(), &opts(), false).expect_err("should refuse");
        assert!(err.to_string().contains("already exists"));
        assert_eq!(fs::read_to_string(tools.join("existing.py")).unwrap(), "x = 1");
    }

    #[test]
    fn scaffold_force_overwrites() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("pyproject.toml"), "# stale").unwrap();

        scaffold(tmp.path(), &opts(), true).unwrap();
        let pyproject = fs::read_to_string(tools.join("pyproject.toml")).unwrap();
        assert!(pyproject.contains(r#"name = "tools""#));
    }

    #[test]
    fn scaffold_accepts_an_empty_tools_dir() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("tools")).unwrap();
        scaffold(tmp.path(), &opts(), false).unwrap();
        assert!(tmp.path().join("tools/pyproject.toml").is_file());
    }
}
