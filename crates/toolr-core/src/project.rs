//! High-level orchestration: find repo, ensure uv, sync venv, validate.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cache::meta::Meta;
use crate::discovery::DiscoveryError;
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

/// Failures that mean there is no tools venv to ready, so `venv sync --if-project` recognises
/// them by type ([`is_nothing_to_sync`]). `NoProject` is the one "no project" error every
/// built-in reports; it carries the discovery message as its own text.
#[derive(Debug, thiserror::Error)]
pub enum NothingToSync {
    #[error("{0}")]
    NoProject(String),
    /// Context of the underlying error.
    #[error("resolving the tools venv path")]
    NoVenvPath,
}

impl NothingToSync {
    /// `NoProject` for a failed project-root lookup.
    pub fn no_project(why: &DiscoveryError) -> Self {
        Self::NoProject(why.to_string())
    }
}

/// Whether `err` says there was nothing to sync: no project, no readable venv location, or
/// uv absent and not installable without asking. A trust refusal, a failed sync and the
/// rest are not.
pub fn is_nothing_to_sync(err: &anyhow::Error) -> bool {
    err.downcast_ref::<NothingToSync>().is_some()
        || matches!(
            err.downcast_ref::<UvError>(),
            Some(UvError::UserRefusedInstall)
        )
}

/// One-stop "make the venv ready" entrypoint for the project at `repo_root`. Returns the resolved venv
/// + the chosen uv binary on success.
pub fn ensure_venv_ready(
    repo_root: &Path,
    consent: ConsentMode,
    opts: EnsureOpts,
) -> Result<(ResolvedVenv, UvBinary)> {
    // Before `tools/pyproject.toml` is read: toolr reads nothing of an untrusted repository.
    if !opts.trusted.as_ref().is_some_and(|t| t.covers(repo_root)) {
        require_trust(repo_root, opts.trust)?;
    }
    let resolved = resolve_venv_path(repo_root).context(NothingToSync::NoVenvPath)?;
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
    .map_err(|e| match e {
        // Says itself what happened and what to do.
        UvError::UntrustedVenv(_) => anyhow::Error::new(e),
        e => anyhow::Error::new(e).context(format!("uv sync against {}", tools.display())),
    })?;
    validate_venv(&resolved.venv_dir, &resolved.python).context("validating the synced venv")?;
    if let Err(e) = finalize_sync(repo_root, &resolved) {
        eprintln!("toolr: warning: failed to finalize sync (meta/manifest): {e}");
    }
    Ok((resolved, uv))
}

/// What to tell a user who declined the sync that follows trust.
pub const SYNC_LATER_HINT: &str =
    "the tools venv isn't synced; run `toolr project venv sync` when you're ready.";

/// Result of syncing after trust ([`sync_after_trust_on_tty`]).
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
pub(crate) fn confirm_sync(input: &mut dyn BufRead, out: &mut dyn Write) -> bool {
    let _ = write!(
        out,
        "The tools venv needs a sync before toolr can run this repository's commands.\n\
         Syncing may download uv and a Python, and installs the dependencies in tools/pyproject.toml.\n"
    );
    crate::prompt::yes_no(input, out, "Sync it now?", false)
}

/// Sync a trusted repository's venv if it needs it, asking first. The sync
/// prompt follows the uv install prompt it replaces: `TOOLR_AUTO_SYNC=1`
/// (or the older `TOOLR_AUTO_INSTALL_UV=1`) answers it, and `never_prompt` or no
/// `prompt_io` (no terminal) declines it.
/// Test-only: exists so tests can inject the prompt IO that the tty variant reads from stdin.
#[cfg(test)]
fn sync_after_trust(
    trusted: &Trusted,
    quiet: bool,
    never_prompt: bool,
    prompt_io: Option<(&mut dyn BufRead, &mut dyn Write)>,
) -> Result<SyncAfterTrust> {
    let choice = choose_sync(trusted, never_prompt, prompt_io)?;
    finish_sync(trusted, quiet, choice)
}

/// Sync after trust, prompting on the terminal when stdin is one.
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
    let can_answer = env.auto_sync_env || (!never_prompt && prompt_io.is_some());
    let resolved = match resolve_venv_path(repo_root) {
        Ok(resolved) => resolved,
        // Nothing would be synced, so leave a config error for `venv sync` to report.
        Err(_) if !can_answer => return Ok(SyncChoice::Skip(SyncAfterTrust::Declined)),
        Err(e) => return Err(e).context("resolving the tools venv path"),
    };
    if check_freshness(&resolved, &repo_root.join("tools")) == crate::venv::Freshness::Fresh {
        return Ok(SyncChoice::Skip(SyncAfterTrust::NotNeeded));
    }
    let agreed = env.auto_sync_env
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

    rebuild_manifest_full(repo_root, Some(&resolved.venv_dir))
        .with_context(|| "rebuilding manifest after sync")?;
    Ok(())
}

#[cfg(test)]
mod tests;
