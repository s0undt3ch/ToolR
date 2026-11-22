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
            let existing =
                fs::read_to_string(&dest).with_context(|| format!("reading {}", dest.display()))?;
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
        return Err(anyhow::Error::new(ScaffoldConflictsError {
            files: conflicts,
        }));
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
        return Err(e).with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()));
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
mod tests;
