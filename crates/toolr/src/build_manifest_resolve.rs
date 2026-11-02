//! Resolve `(source_dir, package_name)` for `toolr self build-manifest`.
//!
//! Two entry modes (mutually exclusive at the CLI level):
//! 1. `<package>` positional → look in the tools venv's site-packages
//!    directories for the installed package directory.
//! 2. `--source-dir PATH` → use the path verbatim; package name comes
//!    from `--package PKG` or the leaf directory name.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toolr_core::discovery::discover_project_root;
use toolr_core::venv::{resolve_venv_path, site_packages_dirs};

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
            let cwd = std::env::current_dir().context("getting current directory")?;
            let repo_root =
                discover_project_root(&cwd).context("resolving repo root for tools-venv lookup")?;
            let resolved = resolve_venv_path(&repo_root).context("resolving tools-venv path")?;
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
        (None, None) => {
            anyhow::bail!("missing required argument: either `<package>` or `--source-dir PATH`")
        }
    }
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
    fn find_in_venv_under_bracketed_venv() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path().join("[wip]").join("venv");
        std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/pkg")).unwrap();
        let found = find_in_venv(&venv, "pkg").unwrap();
        assert_eq!(found, venv.join("lib/python3.13/site-packages/pkg"));
    }

    #[test]
    fn find_in_venv_errors_when_missing() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("lib/python3.13/site-packages/other")).unwrap();
        let err = find_in_venv(tmp.path(), "nope").unwrap_err().to_string();
        assert!(err.contains("no site-packages directory in"), "{err}");
        assert!(err.contains("contains `nope`"), "{err}");
    }

    /// The reported shape: on Windows the package lives under `Lib\site-packages`.
    #[test]
    fn find_in_venv_finds_windows_layout() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path();
        std::fs::create_dir_all(venv.join("Lib/site-packages/pkg")).unwrap();
        let found = find_in_venv(venv, "pkg").unwrap();
        assert_eq!(found, venv.join("Lib").join("site-packages").join("pkg"));
    }

    #[test]
    fn find_in_venv_finds_free_threaded_layout() {
        let tmp = TempDir::new().unwrap();
        let venv = tmp.path();
        std::fs::create_dir_all(venv.join("lib/python3.14t/site-packages/pkg")).unwrap();
        let found = find_in_venv(venv, "pkg").unwrap();
        assert_eq!(found, venv.join("lib/python3.14t/site-packages/pkg"));
    }

    #[test]
    fn find_in_venv_ignores_a_file_named_like_the_package() {
        let tmp = TempDir::new().unwrap();
        let sp = tmp.path().join("lib/python3.13/site-packages");
        std::fs::create_dir_all(&sp).unwrap();
        std::fs::write(sp.join("pkg"), "").unwrap();
        assert!(find_in_venv(tmp.path(), "pkg").is_err());
    }

    #[test]
    fn leaf_dir_name_extracts_basename() {
        let dir = PathBuf::from("/a/b/mypkg");
        assert_eq!(leaf_dir_name(&dir).as_deref(), Some("mypkg"));
    }

    #[cfg(unix)]
    #[test]
    fn find_in_venv_reports_unreadable_lib() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("lib/python3.13/site-packages/pkg")).unwrap();
        let lib = tmp.path().join("lib");
        // SAFETY: geteuid has no preconditions. Root ignores chmod 000.
        let result = (unsafe { libc::geteuid() } != 0).then(|| {
            std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o000)).unwrap();
            let result = find_in_venv(tmp.path(), "pkg");
            std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o755)).unwrap();
            result
        });
        // `None` as root.
        assert!(
            result.as_ref().is_none_or(|r| r
                .as_ref()
                .is_err_and(|err| err.to_string().contains("I/O error reading"))),
            "{result:?}"
        );
    }
}
