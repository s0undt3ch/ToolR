//! High-level orchestration: find repo, ensure uv, sync venv, validate.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cache::meta::Meta;
use crate::discovery::discover_project_root;
use crate::manifest_build::rebuild_manifest_full;
use crate::trust::{TrustMode, Trusted, require_trust};
use crate::uv::{UvBinary, UvError, ensure_uv, install::ConsentMode, toolr_cache_dir};
use crate::venv::{
    ResolvedVenv, UpgradeMode, VenvLocation, cache_slot_dir, check_freshness, resolve_venv_path,
    sync::sync_if_needed, validate::validate_venv,
};

/// Options for [`ensure_venv_ready`]. Constructed via `Default::default()`
/// plus the builder setters; new fields can be added without breaking
/// callers that took an `EnsureOpts::default()`.
#[derive(Debug, Clone, Default)]
pub struct EnsureOpts {
    /// Run `uv sync` even when the freshness stamp says the venv is fresh.
    pub force_sync: bool,
    /// Forward `--quiet` to the uv subprocess. Has no effect when the
    /// stamp short-circuits sync (no uv invocation happens).
    pub quiet: bool,
    /// Whether to pass `-U` / `-P` flags through to uv.
    pub upgrade: UpgradeMode,
    /// How to ask for repository trust when the repo isn't trusted yet.
    pub trust: TrustMode,
    /// Proof that this run already established trust, so the check isn't repeated
    /// for the root it covers.
    pub trusted: Option<Trusted>,
}

impl EnsureOpts {
    pub fn with_force_sync(mut self, v: bool) -> Self {
        self.force_sync = v;
        self
    }
    pub fn with_quiet(mut self, v: bool) -> Self {
        self.quiet = v;
        self
    }
    pub fn with_upgrade(mut self, mode: UpgradeMode) -> Self {
        self.upgrade = mode;
        self
    }
    pub fn with_trust(mut self, mode: TrustMode) -> Self {
        self.trust = mode;
        self
    }
    pub fn with_trusted(mut self, proof: Trusted) -> Self {
        self.trusted = Some(proof);
        self
    }
}

/// One-stop "make the venv ready" entrypoint. Returns the resolved venv
/// + the chosen uv binary on success.
pub fn ensure_venv_ready(
    cwd: &Path,
    consent: ConsentMode,
    opts: EnsureOpts,
) -> Result<(ResolvedVenv, UvBinary)> {
    let repo_root =
        discover_project_root(cwd).context("locating project root for the tools venv")?;
    let resolved = resolve_venv_path(&repo_root).context("resolving the tools venv path")?;
    // After the static pyproject read, so a broken one stays a benign
    // `--quiet` exit; before anything repo-controlled runs.
    if !opts.trusted.as_ref().is_some_and(|t| t.covers(&repo_root)) {
        require_trust(&repo_root, opts.trust)?;
    }
    let uv = ensure_uv(consent).map_err(UvError::into_anyhow)?;
    let tools = repo_root.join("tools");
    sync_if_needed(
        &uv,
        &tools,
        &resolved,
        opts.force_sync,
        opts.quiet,
        &opts.upgrade,
    )
    .with_context(|| format!("uv sync against {}", tools.display()))?;
    validate_venv(&resolved.venv_dir, &resolved.python).context("validating the synced venv")?;
    if let Err(e) = finalize_sync(&repo_root, &resolved) {
        eprintln!("toolr: warning: failed to finalize sync (meta/manifest): {e}");
    }
    Ok((resolved, uv))
}

/// What to tell a user who declined the sync that follows trust.
pub const SYNC_LATER_HINT: &str =
    "the tools venv isn't synced; run `toolr project venv sync` when you're ready.";

/// Result of [`sync_after_trust`].
#[derive(Debug)]
pub enum SyncAfterTrust {
    /// The venv was already fresh.
    NotNeeded,
    Synced(ResolvedVenv),
    /// The sync prompt was declined, or couldn't be asked.
    Declined,
}

/// The answer to the sync prompt.
enum SyncChoice {
    /// Sync, with this uv install consent.
    Run(ConsentMode),
    /// Don't sync; end with this outcome.
    Skip(SyncAfterTrust),
}

/// The sync prompt. It stands in for the uv install prompt, so a `y` is
/// consent to download uv and a Python too. EOF and bare Enter decline.
pub fn confirm_sync(input: &mut dyn BufRead, out: &mut dyn Write) -> bool {
    let _ = write!(
        out,
        "The tools venv needs a sync before toolr can run this repository's commands.\n\
         Syncing may download uv and a Python, and installs the dependencies in tools/pyproject.toml.\n\
         Sync it now? [y/N] "
    );
    let _ = out.flush();
    let mut answer = String::new();
    match input.read_line(&mut answer) {
        Ok(n) if n > 0 => matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
        _ => {
            // EOF echoes no newline, so whatever follows would land on the prompt line.
            let _ = writeln!(out);
            false
        }
    }
}

/// Sync a trusted repository's venv if it needs it, asking first. The sync
/// prompt follows the uv install prompt it replaces: `TOOLR_AUTO_INSTALL_UV=1`
/// answers it, and `never_prompt` or no `prompt_io` (no terminal) declines it.
pub fn sync_after_trust(
    trusted: &Trusted,
    quiet: bool,
    never_prompt: bool,
    prompt_io: Option<(&mut dyn BufRead, &mut dyn Write)>,
) -> Result<SyncAfterTrust> {
    let choice = choose_sync(trusted, never_prompt, prompt_io)?;
    finish_sync(trusted, quiet, choice)
}

/// [`sync_after_trust`] prompting on the terminal, when stdin is one.
/// `quiet` both silences uv and stops the prompt.
pub fn sync_after_trust_on_tty(trusted: &Trusted, quiet: bool) -> Result<SyncAfterTrust> {
    // The stdin lock isn't reentrant, and finding uv may lock stdin again.
    let choice = {
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        let mut stderr = std::io::stderr();
        let prompt_io = stdin.is_terminal().then_some((
            &mut input as &mut dyn BufRead,
            &mut stderr as &mut dyn Write,
        ));
        choose_sync(trusted, quiet, prompt_io)?
    };
    finish_sync(trusted, quiet, choice)
}

fn choose_sync(
    trusted: &Trusted,
    never_prompt: bool,
    prompt_io: Option<(&mut dyn BufRead, &mut dyn Write)>,
) -> Result<SyncChoice> {
    let repo_root = trusted.root();
    let env = ConsentMode::from_env(never_prompt);
    let can_answer = env.auto_install_env || (!never_prompt && prompt_io.is_some());
    let resolved = match resolve_venv_path(repo_root) {
        Ok(resolved) => resolved,
        // Nothing would be synced, so leave a config error for `venv sync` to report.
        Err(_) if !can_answer => return Ok(SyncChoice::Skip(SyncAfterTrust::Declined)),
        Err(e) => return Err(e).context("resolving the tools venv path"),
    };
    if check_freshness(&resolved, &repo_root.join("tools")) == crate::venv::Freshness::Fresh {
        return Ok(SyncChoice::Skip(SyncAfterTrust::NotNeeded));
    }
    let agreed = env.auto_install_env
        || match prompt_io {
            Some((input, out)) if !never_prompt => confirm_sync(input, out),
            _ => false,
        };
    if !agreed {
        return Ok(SyncChoice::Skip(SyncAfterTrust::Declined));
    }
    Ok(SyncChoice::Run(ConsentMode {
        yes_flag: true,
        ..env
    }))
}

fn finish_sync(trusted: &Trusted, quiet: bool, choice: SyncChoice) -> Result<SyncAfterTrust> {
    let consent = match choice {
        SyncChoice::Run(consent) => consent,
        SyncChoice::Skip(outcome) => return Ok(outcome),
    };
    let opts = EnsureOpts::default()
        .with_quiet(quiet)
        .with_trusted(trusted.clone());
    let (resolved, _uv) = ensure_venv_ready(trusted.root(), consent, opts)?;
    Ok(SyncAfterTrust::Synced(resolved))
}

/// Resolve the out-of-repo cache directory that holds this repo's
/// `meta.json` sidecar.
///
/// For cache-located venvs the sidecar lives next to the venv
/// (`<cache>/<key>/meta.json`, the venv being `<cache>/<key>/venv`). For
/// in-tree venvs (`tools/.venv`) the venv stays in the repo but the
/// sidecar must live somewhere the repo cannot write, so it goes to
/// `$XDG_CACHE_HOME/toolr/<repo-key>-in-tree/` (see [`cache_slot_dir`]).
pub fn meta_cache_dir(repo_root: &Path, resolved: &ResolvedVenv) -> Result<PathBuf> {
    if !resolved.is_in_tree() {
        // Cache-located venv: sidecar sits beside the venv.
        return resolved
            .venv_dir
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("cache venv dir has no parent"));
    }
    let base = toolr_cache_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve toolr cache directory"))?;
    cache_slot_dir(
        &base,
        repo_root,
        &resolved.python_version,
        VenvLocation::InTree,
    )
}

/// Final step of a successful sync: write the out-of-repo `meta.json`
/// sidecar (repo path, versions, timestamps) and rebuild the manifest so
/// third-party plugin commands appear immediately (design §2 and §3).
pub fn finalize_sync(repo_root: &Path, resolved: &ResolvedVenv) -> Result<()> {
    let cache_dir = meta_cache_dir(repo_root, resolved)?;

    // Preserve created_at when a sidecar already exists; otherwise mint a
    // fresh one.
    let meta = match Meta::load(&cache_dir) {
        Ok(existing) => existing,
        Err(_) => Meta::new(
            repo_root.to_path_buf(),
            env!("CARGO_PKG_VERSION"),
            &resolved.python_version,
        ),
    };
    meta.write(&cache_dir)
        .with_context(|| format!("writing meta.json to {}", cache_dir.display()))?;

    rebuild_manifest_full(repo_root, &resolved.venv_dir)
        .with_context(|| "rebuilding manifest after sync")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::EnvGuard;
    use crate::venv::config::ToolrConfig;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_resolved_cache(venv_dir: PathBuf, repo_key: &str) -> ResolvedVenv {
        ResolvedVenv {
            python: venv_dir.join("bin").join("python"),
            venv_dir,
            repo_key: repo_key.to_string(),
            python_version: "3.13.1".to_string(),
            config: ToolrConfig::default(),
        }
    }

    #[test]
    fn ensure_opts_default_means_no_force_no_quiet() {
        let opts = EnsureOpts::default();
        assert!(!opts.force_sync);
        assert!(!opts.quiet);
    }

    #[test]
    fn ensure_opts_builder_setters_work() {
        let opts = EnsureOpts::default().with_force_sync(true).with_quiet(true);
        assert!(opts.force_sync);
        assert!(opts.quiet);
    }

    #[test]
    fn ensure_venv_ready_reports_missing_project_root() {
        let _env = EnvGuard::read();
        // No `tools/pyproject.toml` - `ensure_venv_ready` must fail at
        // discovery, before any uv interaction. A bare host `tools/` (the
        // Windows runners' `C:\tools\`) fails there too.
        let tmp = tempfile::tempdir().unwrap();
        let err = ensure_venv_ready(tmp.path(), ConsentMode::default(), EnsureOpts::default())
            .expect_err("expected ensure_venv_ready to fail without a project");
        let chain: Vec<String> = err.chain().map(|e| e.to_string()).collect();
        assert!(
            chain.iter().any(|m| m.contains("locating project root")),
            "expected discovery to fail, got: {chain:?}"
        );
    }

    #[test]
    fn meta_cache_dir_for_cache_venv_is_venv_parent() {
        // Cache-located venv (`repo_key` non-empty): the sidecar lives
        // next to the venv, never derived from the cache dir.
        let resolved = make_resolved_cache(PathBuf::from("/cache/toolr/abc/venv"), "abc");
        let dir = meta_cache_dir(&PathBuf::from("/repo"), &resolved).unwrap();
        assert_eq!(dir, PathBuf::from("/cache/toolr/abc"));
    }

    #[test]
    fn meta_cache_dir_for_in_tree_venv_uses_an_out_of_repo_cache_slot() {
        // In-tree venv (`repo_key` empty): the sidecar lives outside the repo,
        // in the `-in-tree` cache slot rather than the cache venv's slot.
        let (_env, xdg) = EnvGuard::temp_cache_home();

        let repo = TempDir::new().unwrap();
        let resolved = ResolvedVenv {
            python: repo.path().join("tools/.venv/bin/python"),
            venv_dir: repo.path().join("tools/.venv"),
            repo_key: String::new(), // in-tree
            python_version: "3.13".to_string(),
            config: ToolrConfig::default(),
        };
        assert_eq!(
            meta_cache_dir(repo.path(), &resolved).unwrap(),
            cache_slot_dir(
                &xdg.path().join("toolr"),
                repo.path(),
                "3.13",
                VenvLocation::InTree
            )
            .unwrap()
        );
    }

    #[test]
    fn ensure_opts_with_upgrade_sets_mode() {
        use crate::venv::UpgradeMode;
        let opts = EnsureOpts::default().with_upgrade(UpgradeMode::Packages(vec!["foo".into()]));
        match opts.upgrade {
            UpgradeMode::Packages(ref p) => assert_eq!(p, &vec!["foo".to_string()]),
            other => panic!("expected Packages, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn finalize_sync_writes_meta_and_rebuilds_manifest() {
        let (_env, _cache_home) = EnvGuard::temp_cache_home();
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        // A cache-located venv keyed by "abc" so the sidecar lands next to it.
        let cache_entry = tmp.path().join("cache-entry");
        let venv_dir = cache_entry.join("venv");
        let bin = venv_dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let py = bin.join("python");
        std::fs::write(&py, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();

        // A minimal repo with a statically-parseable tools/ dir.
        let repo_root = tmp.path().join("my-project");
        let tools = repo_root.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(
            tools.join("ci.py"),
            "\"\"\"CI.\"\"\"\ngroup = command_group(\"ci\", \"CI\")\n@group.command\ndef hello(ctx):\n    \"\"\"Hi.\"\"\"\n",
        )
        .unwrap();

        let resolved = make_resolved_cache(venv_dir.clone(), "abc");
        finalize_sync(&repo_root, &resolved).unwrap();

        // Sidecar written next to the venv; no interpreter fields recorded.
        let cache_dir = venv_dir.parent().unwrap();
        let meta = Meta::load(cache_dir).unwrap();
        assert_eq!(meta.repo_path, repo_root);
        assert!(
            meta.interpreter_path.is_none(),
            "interpreter_path not recorded"
        );
        assert!(
            meta.interpreter_hash.is_none(),
            "interpreter_hash not recorded"
        );
        // Manifest rebuilt as the final step.
        assert!(tools.join(".toolr-manifest.json").is_file());
    }

    #[test]
    #[cfg(unix)]
    fn finalize_sync_preserves_an_existing_sidecar() {
        let (_env, _cache_home) = EnvGuard::temp_cache_home();
        // When a sidecar already exists, finalize_sync takes the
        // `Meta::load -> Ok(existing)` branch and preserves `created_at`
        // (rather than minting a fresh one).
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let venv_dir = tmp.path().join("cache-entry").join("venv");
        let bin = venv_dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let py = bin.join("python");
        std::fs::write(&py, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();

        let repo_root = tmp.path().join("proj");
        let tools = repo_root.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(
            tools.join("ci.py"),
            "\"\"\"CI.\"\"\"\ngroup = command_group(\"ci\", \"CI\")\n",
        )
        .unwrap();

        // Pre-seed a sidecar so the `Ok(existing)` branch runs.
        let cache_dir = venv_dir.parent().unwrap();
        Meta::new(&repo_root, "old-version", "3.13")
            .write(cache_dir)
            .unwrap();
        let created = Meta::load(cache_dir).unwrap().created_at;

        let resolved = make_resolved_cache(venv_dir.clone(), "abc");
        finalize_sync(&repo_root, &resolved).unwrap();

        let meta = Meta::load(cache_dir).unwrap();
        assert_eq!(
            meta.created_at, created,
            "created_at preserved from the existing sidecar"
        );
        assert!(meta.interpreter_path.is_none());
    }

    fn ask(input: &str) -> (bool, String) {
        let mut out = Vec::new();
        let agreed = confirm_sync(&mut std::io::Cursor::new(input), &mut out);
        (agreed, String::from_utf8(out).unwrap())
    }

    #[test]
    fn confirm_sync_takes_only_yes() {
        for (input, expected) in [
            ("y\n", true),
            ("YES\n", true),
            ("n\n", false),
            ("\n", false),
        ] {
            let (agreed, out) = ask(input);
            assert_eq!(agreed, expected, "input {input:?}");
            assert!(out.ends_with("Sync it now? [y/N] "), "{out:?}");
            assert!(out.contains("may download uv and a Python"));
        }
    }

    #[test]
    fn confirm_sync_eof_declines_and_ends_the_line() {
        let (agreed, out) = ask("");
        assert!(!agreed);
        assert!(out.ends_with("[y/N] \n"), "{out:?}");
    }

    /// A trusted-looking repository with an in-tree venv that was never synced.
    fn unsynced_repo() -> TempDir {
        let repo = TempDir::new().unwrap();
        let tools = repo.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(
            tools.join("pyproject.toml"),
            "[project]\nname = \"t\"\nversion = \"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
        )
        .unwrap();
        repo
    }

    fn after_trust(
        repo: &Path,
        never_prompt: bool,
        input: Option<&str>,
    ) -> (SyncAfterTrust, String) {
        let mut out = Vec::new();
        let trusted = Trusted::for_tests(repo);
        let result = match input {
            Some(text) => {
                let mut cursor = std::io::Cursor::new(text.as_bytes().to_vec());
                sync_after_trust(&trusted, false, never_prompt, Some((&mut cursor, &mut out)))
            }
            None => sync_after_trust(&trusted, false, never_prompt, None),
        };
        (result.unwrap(), String::from_utf8(out).unwrap())
    }

    #[test]
    fn sync_after_trust_declines_without_asking_when_it_cannot_prompt() {
        let mut env = EnvGuard::write();
        env.remove("TOOLR_AUTO_INSTALL_UV");
        env.remove("TOOLR_VENV_LOCATION");
        let repo = unsynced_repo();
        let (r, out) = after_trust(repo.path(), false, None);
        assert!(matches!(r, SyncAfterTrust::Declined));
        assert_eq!(out, "");
        let (r, out) = after_trust(repo.path(), true, Some("y\n"));
        assert!(matches!(r, SyncAfterTrust::Declined));
        assert_eq!(out, "", "never_prompt must not prompt");
    }

    #[test]
    fn sync_after_trust_leaves_a_bad_config_to_venv_sync_unless_asked() {
        let mut env = EnvGuard::write();
        env.remove("TOOLR_AUTO_INSTALL_UV");
        env.remove("TOOLR_VENV_LOCATION");
        let repo = unsynced_repo();
        std::fs::write(
            repo.path().join("tools/pyproject.toml"),
            "[project]\nname = \"t\"\nversion = \"0\"\n\n[tool.toolr]\nvenv-location = \"bogus\"\n",
        )
        .unwrap();
        let (r, _) = after_trust(repo.path(), false, None);
        assert!(matches!(r, SyncAfterTrust::Declined));
        let mut out = Vec::new();
        let mut input = std::io::Cursor::new(b"y\n".to_vec());
        let trusted = Trusted::for_tests(repo.path());
        let err =
            sync_after_trust(&trusted, false, false, Some((&mut input, &mut out))).unwrap_err();
        assert!(
            format!("{err:#}").contains("resolving the tools venv path"),
            "{err:#}"
        );
    }

    #[test]
    fn a_proof_for_another_root_does_not_skip_the_trust_check() {
        let mut env = EnvGuard::write();
        env.remove("TOOLR_TRUST_ALL");
        env.remove("TOOLR_VENV_LOCATION");
        let state = TempDir::new().unwrap();
        env.set("XDG_STATE_HOME", state.path());
        let repo = unsynced_repo();
        let other = TempDir::new().unwrap();
        let opts = EnsureOpts::default()
            .with_trusted(Trusted::for_tests(other.path()))
            .with_trust(TrustMode { never_prompt: true });
        let err = ensure_venv_ready(repo.path(), ConsentMode::default(), opts).unwrap_err();
        assert!(
            format!("{err:#}").contains("untrusted repository"),
            "{err:#}"
        );
    }

    #[test]
    fn sync_after_trust_declined_at_the_prompt() {
        let mut env = EnvGuard::write();
        env.remove("TOOLR_AUTO_INSTALL_UV");
        env.remove("TOOLR_VENV_LOCATION");
        let repo = unsynced_repo();
        let (r, out) = after_trust(repo.path(), false, Some("n\n"));
        assert!(matches!(r, SyncAfterTrust::Declined));
        assert!(out.contains("Sync it now?"));
    }
}
