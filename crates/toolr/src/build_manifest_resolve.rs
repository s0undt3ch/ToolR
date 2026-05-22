//! Resolve `(source_dir, package_name)` for `toolr self build-manifest`.
//!
//! Two entry modes (mutually exclusive at the CLI level):
//! 1. `<package>` positional → glob the tools venv for the installed
//!    package directory.
//! 2. `--source-dir PATH` → use the path verbatim; package name comes
//!    from `--package PKG` or the leaf directory name.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toolr_core::discovery::discover_project_root;
use toolr_core::venv::resolve_venv_path;

pub struct ResolvedSource {
    pub source_dir: PathBuf,
    pub package_name: String,
}

pub fn resolve_source_and_package(matches: &clap::ArgMatches) -> Result<ResolvedSource> {
    let source_dir = matches.get_one::<String>("source-dir").map(PathBuf::from);
    let package_arg = matches.get_one::<String>("package").cloned();
    let positional_pkg = matches.get_one::<String>("package_positional").cloned();

    match (source_dir, positional_pkg) {
        (Some(_), Some(_)) => anyhow::bail!(
            "`<package>` and `--source-dir` are mutually exclusive; pass one or the other"
        ),
        (Some(dir), None) => {
            let package_name = package_arg
                .or_else(|| leaf_dir_name(&dir))
                .ok_or_else(|| {
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
            let cwd = std::env::current_dir().context("getting current directory")?;
            let repo_root = discover_project_root(&cwd)
                .context("resolving repo root for tools-venv lookup")?;
            let resolved = resolve_venv_path(&repo_root)
                .context("resolving tools-venv path")?;
            let dir = find_in_venv(&resolved.venv_dir, &pkg).with_context(|| {
                format!(
                    "package `{pkg}` not found under {}; run `uv sync` or pass --source-dir",
                    resolved.venv_dir.display()
                )
            })?;
            Ok(ResolvedSource {
                source_dir: dir,
                package_name: pkg,
            })
        }
        (None, None) => anyhow::bail!(
            "missing required argument: either `<package>` or `--source-dir PATH`"
        ),
    }
}

fn leaf_dir_name(dir: &Path) -> Option<String> {
    dir.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

/// Glob `<venv>/lib/python*/site-packages/<package>/` for an installed
/// plugin's source directory. Picks the first match lexicographically
/// when more than one Python version is present.
fn find_in_venv(venv_dir: &Path, package: &str) -> Result<PathBuf> {
    let pattern = format!(
        "{}/lib/python*/site-packages/{}",
        venv_dir.display(),
        package
    );
    let entries = glob::glob(&pattern)
        .with_context(|| format!("globbing `{pattern}`"))?
        .filter_map(Result::ok)
        .filter(|p| p.is_dir())
        .collect::<Vec<_>>();
    let mut entries = entries;
    entries.sort();
    if entries.is_empty() {
        anyhow::bail!("no site-packages directory matches `{pattern}`");
    }
    if entries.len() > 1 {
        eprintln!(
            "toolr: warning: multiple matches for `{package}` in venv, using {}",
            entries[0].display()
        );
    }
    Ok(entries.into_iter().next().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn find_in_venv_picks_first_lexicographic_match() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path();
        std::fs::create_dir_all(venv.join("lib/python3.12/site-packages/pkg")).unwrap();
        std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/pkg")).unwrap();
        let found = find_in_venv(venv, "pkg").unwrap();
        assert!(
            found.ends_with("lib/python3.12/site-packages/pkg"),
            "got: {}",
            found.display()
        );
    }

    #[test]
    fn find_in_venv_errors_when_missing() {
        let tmp = TempDir::new().unwrap();
        let err = find_in_venv(tmp.path(), "nope").unwrap_err();
        assert!(err.to_string().contains("no site-packages"));
    }

    #[test]
    fn leaf_dir_name_extracts_basename() {
        let dir = PathBuf::from("/a/b/mypkg");
        assert_eq!(leaf_dir_name(&dir).as_deref(), Some("mypkg"));
    }
}
