//! Pre-clap bootstrap: detect missing `tools/.toolr-manifest.json`
//! and run a full rebuild before clap parses the user's command.
//!
//! See `specs/archive/2026/2026-05-19-fill-the-gaps-design.md` (gap 1) for the
//! decision logic.

use std::path::Path;

use anyhow::Context;
use toolr_core::discovery::discover_project_root;
use toolr_core::dynamic::{compute_third_party_hash, empty_third_party_hash};
use toolr_core::freshness::{FreshnessVerdict, compare};
use toolr_core::manifest::{Manifest, Origin, load_manifest, write_manifest};
use toolr_core::parser::{build_static_manifest, build_static_manifest_with_venv};
use toolr_core::venv::resolve_venv_path;

/// Bootstrap step that runs before clap parses the user's command.
///
/// When the manifest is missing AND `tools/pyproject.toml` exists AND
/// argv doesn't look like a built-in / completion call, build the
/// manifest **statically** so the user's command can succeed on a fresh
/// clone. This never resolves or spawns the venv interpreter: first-party
/// commands come from a pure-Rust AST parse of `tools/*.py`, and
/// third-party commands are picked up by an execution-free glob of an
/// already-existing venv's `site-packages/*/toolr-manifest.json`.
///
/// Errors propagate so `main.rs` can print them and exit non-zero — we
/// intentionally do NOT fall through to an empty manifest, since that's
/// the buggy old behaviour this task fixes.
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

    let manifest_path = tools.join(".toolr-manifest.json");
    // First-party is always available (pure AST). Add third-party only when
    // a venv already exists — globbing site-packages JSON executes nothing.
    let venv_dir = resolve_venv_path(&root).ok().map(|r| r.venv_dir);
    let manifest = match venv_dir.as_deref() {
        Some(v) if v.join("pyvenv.cfg").is_file() => {
            build_static_manifest_with_venv(&tools, v).map_err(anyhow::Error::from)?
        }
        _ => build_static_manifest(&tools)?,
    };
    write_manifest(&manifest_path, &manifest)
        .with_context(|| format!("writing {}", manifest_path.display()))?;
    Ok(())
}

/// Decide whether the pre-clap bootstrap should skip building a missing
/// manifest for this argv.
///
/// Building the manifest is now a cheap, **execution-free** static parse
/// (AST of `tools/*.py` + an execution-free third-party glob), so this
/// skip is purely a latency optimisation — never a safety gate. No code
/// path here (or downstream) ever executes repository Python; that only
/// happens when the user explicitly dispatches a command.
///
/// We still skip the built-ins that manage their own state
/// (`__complete`, `project`, `self`, `init`) and `--version` (prints
/// binary metadata only). `--help` and bare `toolr` take the static
/// build so help renders the user's command tree.
pub(crate) fn should_skip_auto_rebuild(argv: &[String]) -> bool {
    const BUILTINS: &[&str] = &["__complete", "project", "self", "init"];
    const VERSION_FLAGS: &[&str] = &["--version", "-V"];

    // `--version` prints binary metadata only — never needs the user
    // manifest, so don't pay the build cost.
    if argv.iter().skip(1).any(|a| VERSION_FLAGS.contains(&a.as_str())) {
        return true;
    }
    // First positional (= first arg after `toolr` that doesn't start with `-`).
    // `--help` / bare `toolr` fall through to the static build so both
    // surfaces render the user's command tree.
    let first_positional = argv.iter().skip(1).find(|a| !a.starts_with('-'));
    match first_positional {
        None => false, // `toolr` alone (with or without `--help`) → static build so help shows user groups
        Some(name) => BUILTINS.contains(&name.as_str()),
    }
}

/// Dispatch-time freshness step: detect a stale manifest, rebuild it
/// in-process (pure Rust, no Python), persist the result, and fall
/// back to the cached manifest with a warning when a rebuild errors.
///
/// Returns `Ok(())` on success OR on any non-fatal soft failure
/// (drift rebuild error, missing venv, etc.). Only I/O errors from
/// writing the freshly rebuilt manifest propagate.
pub(crate) fn ensure_manifest_fresh(
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
    if should_skip_auto_rebuild(argv) {
        return Ok(());
    }

    let manifest_path = tools.join(".toolr-manifest.json");
    let cached = load_manifest(&manifest_path).ok();
    let venv_dir: Option<std::path::PathBuf> =
        resolve_venv_path(&root).ok().map(|r| r.venv_dir);

    let verdict = compare(cached.as_ref(), &tools, venv_dir.as_deref())?;

    if matches!(verdict, FreshnessVerdict::Fresh) {
        return Ok(());
    }

    match try_rebuild(verdict, &tools, venv_dir.as_deref(), cached.as_ref()) {
        Ok(fresh) => write_manifest(&manifest_path, &fresh)
            .with_context(|| format!("writing {}", manifest_path.display())),
        Err(e) => {
            warn_and_keep_cache(&e, cached.is_some());
            Ok(())
        }
    }
}

fn try_rebuild(
    verdict: FreshnessVerdict,
    tools: &Path,
    venv: Option<&Path>,
    cached: Option<&Manifest>,
) -> anyhow::Result<Manifest> {
    // `build_static_manifest_inner` already stamps `static_hash` on the
    // returned manifest, so no re-hashing here.
    let mut fresh = match verdict {
        FreshnessVerdict::StaticDrift => build_static_manifest(tools)?,
        FreshnessVerdict::ThirdPartyDrift => match venv {
            Some(v) => build_static_manifest_with_venv(tools, v).map_err(anyhow::Error::from)?,
            None => build_static_manifest(tools)?,
        },
        FreshnessVerdict::Fresh => unreachable!("Fresh handled by caller"),
    };
    if let Some(cached) = cached {
        carry_forward_cached_entries(&mut fresh, cached, verdict);
    }
    // Stamp third_party_hash according to the drift axis.
    fresh.third_party_hash = match verdict {
        FreshnessVerdict::StaticDrift => cached
            .map(|c| c.third_party_hash.clone())
            .unwrap_or_else(empty_third_party_hash),
        FreshnessVerdict::ThirdPartyDrift => match venv {
            Some(v) => compute_third_party_hash(v)
                .with_context(|| "hashing third-party manifests")?,
            None => empty_third_party_hash(),
        },
        FreshnessVerdict::Fresh => unreachable!(),
    };
    Ok(fresh)
}

/// Copy non-static entries from `cached` into `fresh` when the fresh
/// rebuild has no entry with the same identity. On `StaticDrift` we
/// preserve `ThirdParty` entries (we didn't re-glob the venv). On
/// `ThirdPartyDrift` we carry forward nothing — third-party comes from
/// the fresh glob, and there is no longer any untrusted dynamic origin
/// to carry forward (this is the SEC-03 fix).
///
/// Note: this helper is purpose-built for persistent dispatch paths.
/// It MUST NOT be confused with `complete::freshness::preserve_non_static_entries`,
/// which is for in-memory tab-completion paths that never write to disk.
fn carry_forward_cached_entries(
    fresh: &mut Manifest,
    cached: &Manifest,
    verdict: FreshnessVerdict,
) {
    let keep = |o: &Origin| {
        matches!(
            (verdict, o),
            (FreshnessVerdict::StaticDrift, Origin::ThirdParty)
        )
    };
    for group in &cached.groups {
        if keep(&group.origin) && !fresh.groups.iter().any(|g| g.name == group.name) {
            fresh.groups.push(group.clone());
        }
    }
    for cmd in &cached.commands {
        if keep(&cmd.origin)
            && !fresh
                .commands
                .iter()
                .any(|c| c.group == cmd.group && c.name == cmd.name)
        {
            fresh.commands.push(cmd.clone());
        }
    }
}

fn warn_and_keep_cache(err: &anyhow::Error, had_cache: bool) {
    eprintln!(
        "toolr: warning: tools manifest is stale and a fresh build failed; \
         falling back to cached manifest"
    );
    let s = err.to_string();
    let first = s.lines().next().unwrap_or(&s);
    eprintln!("toolr: warning: cause: {first}");
    if !had_cache {
        eprintln!(
            "toolr: warning: no cached manifest available — `toolr <user-cmd>` \
             will likely fail until you fix the build error"
        );
    }
    eprintln!("toolr: warning: run `toolr project manifest rebuild` to see the full error");
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
        // the manifest. Falling through to the static build beats showing
        // a partial help that hides every user command. The build is a
        // pure-Rust AST parse — it executes no repository Python.
        assert!(!should_skip_auto_rebuild(&args(&["--help"])));
    }

    #[test]
    fn fires_for_short_help_flag() {
        // Same as `--help`: take the execution-free static build.
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
        // which is the same surface as `--help`. The static build it
        // triggers executes no repository Python.
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
