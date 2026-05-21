//! Pre-clap bootstrap: detect missing `tools/.toolr-manifest.json`
//! and run a full rebuild before clap parses the user's command.
//!
//! See `specs/2026-05-19-fill-the-gaps-design.md` (gap 1) for the
//! decision logic.

use std::path::Path;

use toolr_core::discovery::discover_project_root;
use toolr_core::dynamic::rebuild_manifest_full;
use toolr_core::venv::resolve_venv_path;

/// Bootstrap step that runs before clap parses the user's command.
///
/// When the manifest is missing AND `tools/pyproject.toml` exists AND
/// argv doesn't look like a built-in / help / completion call, run a
/// full `rebuild_manifest_full` so the user's command can succeed on
/// a fresh clone. Errors propagate so `main.rs` can print them and
/// exit non-zero — we intentionally do NOT fall through to an empty
/// manifest, since that's the buggy old behaviour this task fixes.
pub(crate) fn ensure_manifest_present_or_bootstrap(
    cwd: &Path,
    argv: &[String],
) -> anyhow::Result<()> {
    let Ok(root) = discover_project_root(cwd) else {
        return Ok(());
    };
    let tools = root.join("tools");
    if !tools.join("pyproject.toml").is_file() {
        return Ok(());
    }
    if tools.join(".toolr-manifest.json").is_file() {
        return Ok(());
    }
    if should_skip_auto_rebuild(argv) {
        return Ok(());
    }

    let resolved = match resolve_venv_path(&root) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    if !resolved.python.is_file() {
        return Ok(());
    }

    eprintln!("toolr: manifest missing; building (first-time setup)...");
    rebuild_manifest_full(&root, &resolved.python, &resolved.venv_dir)?;
    Ok(())
}

pub(crate) fn should_skip_auto_rebuild(argv: &[String]) -> bool {
    const BUILTINS: &[&str] = &["__complete", "project", "self", "init"];
    const VERSION_FLAGS: &[&str] = &["--version", "-V"];

    // `--version` prints binary metadata only — never needs the user
    // manifest, so don't pay the rebuild cost.
    if argv.iter().skip(1).any(|a| VERSION_FLAGS.contains(&a.as_str())) {
        return true;
    }
    // First positional (= first arg after `toolr` that doesn't start with `-`).
    // Note: `--help` / bare `toolr` deliberately fall through to the
    // rebuild — both surfaces render the command tree, and rendering it
    // without user groups (because the manifest is missing) is the UX
    // bug this skip exists to avoid.
    let first_positional = argv.iter().skip(1).find(|a| !a.starts_with('-'));
    match first_positional {
        None => false, // `toolr` alone (with or without `--help`) → rebuild so help shows user groups
        Some(name) => BUILTINS.contains(&name.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::should_skip_auto_rebuild;

    fn args(parts: &[&str]) -> Vec<String> {
        std::iter::once("toolr")
            .chain(parts.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn fires_for_long_help_flag() {
        // `toolr --help` must render the user's command tree; that needs
        // the manifest. Falling through to rebuild beats showing a
        // partial help that hides every user command.
        assert!(!should_skip_auto_rebuild(&args(&["--help"])));
    }

    #[test]
    fn fires_for_short_help_flag() {
        assert!(!should_skip_auto_rebuild(&args(&["-h"])));
    }

    #[test]
    fn skips_for_long_version_flag() {
        // `--version` prints binary metadata — independent of the
        // manifest, no reason to rebuild.
        assert!(should_skip_auto_rebuild(&args(&["--version"])));
    }

    #[test]
    fn skips_for_short_version_flag() {
        assert!(should_skip_auto_rebuild(&args(&["-V"])));
    }

    #[test]
    fn fires_for_bare_toolr() {
        // Bare `toolr` falls through to clap's auto-generated help,
        // which is the same surface as `--help`.
        assert!(!should_skip_auto_rebuild(&args(&[])));
    }

    #[test]
    fn skips_for_tab_completion() {
        assert!(should_skip_auto_rebuild(&args(&["__complete", "/tmp", "..."])));
    }

    #[test]
    fn skips_for_project_subcommands() {
        assert!(should_skip_auto_rebuild(&args(&["project", "manifest", "rebuild"])));
    }

    #[test]
    fn skips_for_self_subcommands() {
        assert!(should_skip_auto_rebuild(&args(&["self", "cache", "list"])));
    }

    #[test]
    fn skips_for_init() {
        assert!(should_skip_auto_rebuild(&args(&["init"])));
    }

    #[test]
    fn fires_for_user_command() {
        assert!(!should_skip_auto_rebuild(&args(&["jenkins", "job", "migrate"])));
    }

    #[test]
    fn fires_with_leading_global_flag() {
        assert!(!should_skip_auto_rebuild(&args(&["--debug", "django", "migrate"])));
    }
}
