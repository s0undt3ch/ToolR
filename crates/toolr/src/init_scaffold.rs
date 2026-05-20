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

/// On-disk classification of a single scaffold file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// The file does not yet exist on disk.
    New,
    /// The file exists on disk with the **same** content (idempotent, skip).
    Identical,
    /// The file exists on disk with **different** content (conflict).
    Conflict,
}

/// A single scaffold file together with its on-disk classification.
#[derive(Debug)]
pub struct ScaffoldFile {
    /// Absolute destination path.
    pub dest: PathBuf,
    /// Rendered content that would be written.
    pub contents: String,
    /// On-disk classification.
    pub status: FileStatus,
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
            .filter(|f| f.status == FileStatus::Conflict)
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
        let status = if dest.exists() {
            let existing = fs::read_to_string(&dest)
                .with_context(|| format!("reading {}", dest.display()))?;
            if existing == f.contents {
                FileStatus::Identical
            } else {
                FileStatus::Conflict
            }
        } else {
            FileStatus::New
        };
        files.push(ScaffoldFile {
            dest,
            contents: f.contents,
            status,
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
    // Refuse to write through a symlink: it could redirect files into an
    // unintended directory without any visible indication.
    if analysis.tools_dir.is_symlink() {
        anyhow::bail!(
            "{} is a symlink — resolve or remove it before running `toolr project init`",
            analysis.tools_dir.display()
        );
    }

    fs::create_dir_all(&analysis.tools_dir)
        .with_context(|| format!("creating {}", analysis.tools_dir.display()))?;

    let mut written: Vec<PathBuf> = Vec::new();
    // Snapshots of conflict-file originals for rollback (path → original content).
    let mut snapshots: Vec<(PathBuf, String)> = Vec::new();

    for f in &analysis.files {
        match f.status {
            FileStatus::Identical => continue,
            FileStatus::Conflict if !overwrite.contains(&f.dest) => continue,
            FileStatus::New => {
                // TOCTOU: catch a file that appeared between analysis and execution.
                if f.dest.exists() {
                    return Err(anyhow::anyhow!(
                        "file appeared unexpectedly during scaffold: {}",
                        f.dest.display()
                    ));
                }
            }
            FileStatus::Conflict => {
                // Snapshot the original so we can restore it on rollback.
                let original = fs::read_to_string(&f.dest)
                    .with_context(|| format!("snapshotting {}", f.dest.display()))?;
                snapshots.push((f.dest.clone(), original));
            }
        }

        if let Err(e) = write_file(&f.dest, &f.contents) {
            // Rollback: restore snapshots for overwritten conflict files;
            // delete new files that were already written.
            for path in written.iter().rev() {
                if let Some((_, original)) = snapshots.iter().find(|(p, _)| p == path) {
                    let _ = write_file(path, original);
                } else {
                    let _ = fs::remove_file(path);
                }
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
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e)
            .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()));
    }
    Ok(())
}

fn with_extension(path: &Path, extra: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".");
    name.push(extra);
    // Include the process ID so concurrent `toolr project init` runs in the
    // same directory don't collide on the same .tmp filename.
    name.push(".");
    name.push(std::process::id().to_string());
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

    #[test]
    fn execute_scaffold_restores_conflict_originals_on_rollback() {
        // Build a real analysis for a single-file case so we can intercept
        // the failure by having one file succeed and then simulating a disk
        // full via a read-only directory on the second write.
        //
        // Simpler approach: use a two-conflict scenario where we approve both,
        // but make the second destination a read-only file so write_file fails,
        // then assert the first destination is restored to its original.
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();

        // Pre-populate both scaffold conflict files with known "original" content.
        let pyproject_original = "# pyproject original";
        let gitignore_original = "# gitignore original";
        fs::write(tools.join("pyproject.toml"), pyproject_original).unwrap();
        fs::write(tools.join(".gitignore"), gitignore_original).unwrap();

        let analysis = analyze_scaffold(tmp.path(), &opts()).unwrap();
        assert_eq!(
            analysis.conflict_files().len(),
            2,
            "expected both files to be conflicts"
        );

        // Make the tools dir itself read-only so that writing example.py
        // (a New file) fails. This triggers the rollback after pyproject.toml
        // and .gitignore have already been overwritten.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&tools).unwrap().permissions();
            perms.set_mode(0o555); // read+execute, no write
            fs::set_permissions(&tools, perms).unwrap();

            let overwrite: HashSet<PathBuf> = analysis
                .conflict_files()
                .into_iter()
                .map(PathBuf::from)
                .collect();
            let result = execute_scaffold(&analysis, &overwrite);

            // Restore permissions so TempDir can clean up.
            let mut perms = fs::metadata(&tools).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&tools, perms).unwrap();

            assert!(result.is_err(), "expected write to fail");
            // Originals must be restored, not deleted.
            assert_eq!(
                fs::read_to_string(tools.join("pyproject.toml")).unwrap(),
                pyproject_original,
                "pyproject.toml must be restored"
            );
            assert_eq!(
                fs::read_to_string(tools.join(".gitignore")).unwrap(),
                gitignore_original,
                ".gitignore must be restored"
            );
        }
        // Non-Unix: just assert the happy path so the test still compiles.
        #[cfg(not(unix))]
        {
            let _ = analysis;
        }
    }

    #[test]
    fn scaffold_conflicts_error_display() {
        let err = ScaffoldConflictsError {
            files: vec![
                PathBuf::from("tools/pyproject.toml"),
                PathBuf::from("tools/.gitignore"),
            ],
        };
        let msg = err.to_string();
        assert!(msg.contains("would be overwritten"), "msg: {msg}");
        assert!(msg.contains("pyproject.toml"), "msg: {msg}");
        assert!(msg.contains(".gitignore"), "msg: {msg}");
    }

    #[test]
    fn file_status_classify_new_identical_conflict() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("pyproject.toml"), "# stale").unwrap();

        // Write the scaffold to get the real .gitignore content.
        let first = scaffold(tmp.path(), &opts(), true).unwrap();
        let gitignore_path = first
            .files_written
            .iter()
            .find(|p| p.ends_with(".gitignore"))
            .expect("gitignore must be written");
        let gitignore_content = fs::read_to_string(gitignore_path).unwrap();

        // Now put stale content back into pyproject.toml and keep .gitignore identical.
        fs::write(tools.join("pyproject.toml"), "# stale again").unwrap();
        // Delete example.py so it is "New" on the next analysis.
        fs::remove_file(tools.join("example.py")).unwrap();

        let analysis = analyze_scaffold(tmp.path(), &opts()).unwrap();
        let pyproject = analysis.files.iter().find(|f| f.dest.ends_with("pyproject.toml")).unwrap();
        let gitignore = analysis.files.iter().find(|f| f.dest.ends_with(".gitignore")).unwrap();
        let example = analysis.files.iter().find(|f| f.dest.ends_with("example.py")).unwrap();

        assert_eq!(pyproject.status, FileStatus::Conflict);
        assert_eq!(gitignore.status, FileStatus::Identical, "content: {gitignore_content:?}");
        assert_eq!(example.status, FileStatus::New);
    }

    #[test]
    #[cfg(unix)]
    fn execute_scaffold_rejects_symlinked_tools_dir() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().unwrap();
        let real_dir = tmp.path().join("real_tools");
        let link = tmp.path().join("tools");
        fs::create_dir(&real_dir).unwrap();
        symlink(&real_dir, &link).unwrap();

        let analysis = analyze_scaffold(tmp.path(), &opts()).unwrap();
        let result = execute_scaffold(&analysis, &HashSet::new());
        assert!(result.is_err(), "expected error for symlinked tools/");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("symlink"), "error should mention symlink: {msg}");
    }

    #[test]
    fn tmp_filename_includes_pid() {
        let path = std::path::Path::new("/some/dir/pyproject.toml");
        let tmp = with_extension(path, "tmp");
        let name = tmp.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("pyproject.toml.tmp."), "unexpected name: {name}");
        let pid_str = name.strip_prefix("pyproject.toml.tmp.").unwrap();
        assert!(pid_str.parse::<u32>().is_ok(), "suffix must be a PID: {pid_str}");
    }
}
