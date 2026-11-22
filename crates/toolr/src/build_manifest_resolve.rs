//! Resolve `(source_dir, package_name)` for `toolr self build-manifest`.
//!
//! Two entry modes (mutually exclusive at the CLI level):
//! 1. `<package>` positional → look in the tools venv's site-packages
//!    directories for the installed package directory.
//! 2. `--source-dir PATH` → use the path verbatim; package name comes
//!    from `--package PKG` or the leaf directory name.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::invocation::Invocation;
use toolr_core::venv::{resolve_venv_path, site_packages_dirs};

pub struct ResolvedSource {
    pub source_dir: PathBuf,
    pub package_name: String,
}

pub fn resolve_source_and_package(
    matches: &clap::ArgMatches,
    run: &Invocation,
    quiet: bool,
) -> Result<ResolvedSource> {
    let source_dir = matches.get_one::<String>("source-dir").map(PathBuf::from);
    let package_arg = matches.get_one::<String>("package").cloned();
    let positional_pkg = matches.get_one::<String>("package_positional").cloned();

    match (source_dir, positional_pkg) {
        (Some(_), Some(_)) => anyhow::bail!(
            "`<package>` and `--source-dir` are mutually exclusive; pass one or the other"
        ),
        (Some(dir), None) => {
            let package_name = package_arg.or_else(|| leaf_dir_name(&dir)).ok_or_else(|| {
                anyhow::anyhow!(
                    "--source-dir {} has no inferable package name; pass --package PKG",
                    dir.display()
                )
            })?;
            if !dir.is_dir() {
                anyhow::bail!("--source-dir `{}` is not a directory", dir.display());
            }
            Ok(ResolvedSource {
                source_dir: dir,
                package_name,
            })
        }
        (None, Some(pkg)) => {
            let repo_root = run.project_root()?;
            // The lookup reads the repository's tools venv.
            toolr_core::trust::require_trust_quietly(&repo_root, quiet)?;
            let resolved = resolve_venv_path(&repo_root).context("resolving tools-venv path")?;
            let Some(venv_dir) = resolved.usable_venv_dir(&repo_root) else {
                anyhow::bail!("{}", foreign_venv_hint(&resolved.venv_dir));
            };
            let dir = find_in_venv(venv_dir, &pkg).with_context(|| {
                format!(
                    "package `{pkg}` not found under {}; run `uv sync` or pass --source-dir",
                    venv_dir.display()
                )
            })?;
            Ok(ResolvedSource {
                source_dir: dir,
                package_name: pkg,
            })
        }
        (None, None) => {
            anyhow::bail!("missing required argument: either `<package>` or `--source-dir PATH`")
        }
    }
}

/// Why toolr won't read or run an in-tree venv it didn't build, and the fix.
pub(crate) fn foreign_venv_hint(venv_dir: &Path) -> String {
    format!(
        "{} — run `toolr project venv sync` to rebuild it",
        toolr_core::venv::untrusted_venv_reason(venv_dir)
    )
}

fn leaf_dir_name(dir: &Path) -> Option<String> {
    dir.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

/// Find an installed plugin's source directory, `<site-packages>/<package>/`, in any of the
/// venv's site-packages directories (see [`site_packages_dirs`]). Picks the first in sorted
/// order when more than one has it.
fn find_in_venv(venv_dir: &Path, package: &str) -> Result<PathBuf> {
    let entries: Vec<PathBuf> = site_packages_dirs(venv_dir)?
        .into_iter()
        .map(|sp| sp.join(package))
        .filter(|p| p.is_dir())
        .collect();
    let Some(first) = entries.first() else {
        anyhow::bail!(
            "no site-packages directory in {} contains `{package}`",
            venv_dir.display()
        );
    };
    if entries.len() > 1 {
        eprintln!(
            "toolr: warning: multiple matches for `{package}` in venv, using {}",
            first.display()
        );
    }
    Ok(first.clone())
}

#[cfg(test)]
mod tests;
