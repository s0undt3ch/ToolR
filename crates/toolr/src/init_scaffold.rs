//! Scaffold writer for `toolr project init`. Classifies each template file
//! as new / identical / conflict before touching disk, then writes atomically
//! with rollback on failure.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::init_templates::{ScaffoldOptions, render_all};

// ---------------------------------------------------------------------------
// Public error
// ---------------------------------------------------------------------------

/// Returned by [`scaffold`] (non-`force` path) when one or more files already
/// exist on disk with different content.
#[derive(Debug)]
pub struct ScaffoldConflictsError {
    pub files: Vec<PathBuf>,
}

impl std::fmt::Display for ScaffoldConflictsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tools/ has file(s) that would be overwritten (use --force):"
        )?;
        for p in &self.files {
            write!(f, "\n  {}", p.display())?;
        }
        Ok(())
    }
}

impl std::error::Error for ScaffoldConflictsError {}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

/// A single scaffold file together with its on-disk classification.
#[derive(Debug)]
pub struct ScaffoldFile {
    /// Absolute destination path.
    pub dest: PathBuf,
    /// Rendered content that would be written.
    pub contents: String,
    /// `true` when the file exists on disk with **different** content.
    pub is_conflict: bool,
    /// `true` when the file exists on disk with **identical** content.
    pub is_identical: bool,
}

/// Result of analysing what a scaffold run would do, without touching disk.
#[derive(Debug)]
pub struct ScaffoldAnalysis {
    pub tools_dir: PathBuf,
    pub files: Vec<ScaffoldFile>,
}

impl ScaffoldAnalysis {
    pub fn conflict_files(&self) -> Vec<&Path> {
        self.files
            .iter()
            .filter(|f| f.is_conflict)
            .map(|f| f.dest.as_path())
            .collect()
    }
}

/// Classify every scaffold file against what is currently on disk.
/// Does **not** create directories or write any files.
pub fn analyze_scaffold(cwd: &Path, opts: &ScaffoldOptions) -> Result<ScaffoldAnalysis> {
    let tools_dir = cwd.join("tools");
    let rendered = render_all(opts);
    let mut files = Vec::with_capacity(rendered.len());

    for f in rendered {
        let dest = tools_dir.join(f.relative_path);
        let (is_conflict, is_identical) = if dest.exists() {
            let existing = fs::read_to_string(&dest)
                .with_context(|| format!("reading {}", dest.display()))?;
            if existing == f.contents {
                (false, true)
            } else {
                (true, false)
            }
        } else {
            (false, false)
        };
        files.push(ScaffoldFile {
            dest,
            contents: f.contents,
            is_conflict,
            is_identical,
        });
    }

    Ok(ScaffoldAnalysis { tools_dir, files })
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// Outcome of a completed scaffold run.
#[derive(Debug)]
pub struct ScaffoldOutcome {
    pub tools_dir: PathBuf,
    /// Files that were actually written to disk.
    pub files_written: Vec<PathBuf>,
}

/// Execute a scaffold run given a pre-computed [`ScaffoldAnalysis`].
///
/// * **New** files are always written.
/// * **Identical** files are always skipped.
/// * **Conflict** files are written only if their path appears in `overwrite`.
pub fn execute_scaffold(
    analysis: &ScaffoldAnalysis,
    overwrite: &HashSet<PathBuf>,
) -> Result<ScaffoldOutcome> {
    fs::create_dir_all(&analysis.tools_dir)
        .with_context(|| format!("creating {}", analysis.tools_dir.display()))?;

    let mut written: Vec<PathBuf> = Vec::new();

    for f in &analysis.files {
        if f.is_identical {
            continue;
        }
        if f.is_conflict && !overwrite.contains(&f.dest) {
            continue;
        }
        if let Err(e) = write_file(&f.dest, &f.contents) {
            for path in written.iter().rev() {
                let _ = fs::remove_file(path);
            }
            return Err(e).with_context(|| format!("writing {}", f.dest.display()));
        }
        written.push(f.dest.clone());
    }

    Ok(ScaffoldOutcome {
        tools_dir: analysis.tools_dir.clone(),
        files_written: written,
    })
}

/// Convenience wrapper used by tests and the non-interactive code path.
///
/// * `force = true` — overwrite all conflict files.
/// * `force = false` — fail with [`ScaffoldConflictsError`] if any conflicts exist.
pub fn scaffold(cwd: &Path, opts: &ScaffoldOptions, force: bool) -> Result<ScaffoldOutcome> {
    let analysis = analyze_scaffold(cwd, opts)?;

    let conflicts: Vec<PathBuf> = analysis
        .conflict_files()
        .into_iter()
        .map(PathBuf::from)
        .collect();

    if !conflicts.is_empty() && !force {
        return Err(anyhow::Error::new(ScaffoldConflictsError { files: conflicts }));
    }

    let overwrite: HashSet<PathBuf> = conflicts.into_iter().collect();
    execute_scaffold(&analysis, &overwrite)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
    fn scaffold_writes_alongside_unrelated_files() {
        // Unrelated files in tools/ are not scaffold files → no conflict,
        // scaffold proceeds and the unrelated file is left untouched.
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("my_tool.py"), "# custom").unwrap();

        let outcome = scaffold(tmp.path(), &opts(), false).unwrap();
        assert_eq!(outcome.files_written.len(), 3);
        assert_eq!(
            fs::read_to_string(tools.join("my_tool.py")).unwrap(),
            "# custom"
        );
    }

    #[test]
    fn scaffold_returns_conflict_error_without_force() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("pyproject.toml"), "# stale").unwrap();

        let err = scaffold(tmp.path(), &opts(), false).expect_err("should conflict");
        let conflicts = err
            .downcast_ref::<ScaffoldConflictsError>()
            .expect("should be ScaffoldConflictsError");
        assert_eq!(conflicts.files.len(), 1);
        assert!(conflicts.files[0].ends_with("pyproject.toml"));
        // Existing content must be preserved.
        assert_eq!(
            fs::read_to_string(tools.join("pyproject.toml")).unwrap(),
            "# stale"
        );
    }

    #[test]
    fn scaffold_skips_identical_files() {
        let tmp = TempDir::new().unwrap();
        // Write the scaffold once.
        scaffold(tmp.path(), &opts(), false).unwrap();
        // Run again — all files identical, nothing written.
        let outcome = scaffold(tmp.path(), &opts(), false).unwrap();
        assert_eq!(outcome.files_written.len(), 0);
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

    #[test]
    fn execute_scaffold_respects_overwrite_set() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("pyproject.toml"), "# stale").unwrap();
        fs::write(tools.join(".gitignore"), "# stale").unwrap();

        let analysis = analyze_scaffold(tmp.path(), &opts()).unwrap();
        // Approve only pyproject.toml.
        let overwrite: HashSet<PathBuf> = [tools.join("pyproject.toml")].into();
        let outcome = execute_scaffold(&analysis, &overwrite).unwrap();

        // pyproject.toml overwritten, .gitignore skipped (conflict but not approved).
        assert!(outcome.files_written.iter().any(|p| p.ends_with("pyproject.toml")));
        assert!(!outcome.files_written.iter().any(|p| p.ends_with(".gitignore")));
        assert_eq!(
            fs::read_to_string(tools.join(".gitignore")).unwrap(),
            "# stale"
        );
    }
}
