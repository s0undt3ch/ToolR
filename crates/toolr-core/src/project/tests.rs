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
fn nothing_to_sync_is_recognised_by_type_not_by_text() {
    use crate::trust::TrustError;
    let no_project = anyhow::Error::new(NothingToSync::NoProject("boom".into()));
    let no_venv_path = anyhow::anyhow!("boom").context(NothingToSync::NoVenvPath);
    let refused = UvError::UserRefusedInstall.into_anyhow();
    let wrapped = UvError::UserRefusedInstall
        .into_anyhow()
        .context("uv sync against tools");
    for err in [&no_project, &no_venv_path, &refused, &wrapped] {
        assert!(is_nothing_to_sync(err), "{err:#}");
    }

    // The same words from another source, and a trust refusal, are not it.
    let lookalike = anyhow::anyhow!("no toolr project found above /x");
    let path = PathBuf::from("/x/locating project root/user declined uv install");
    let untrusted = anyhow::Error::new(TrustError::StoreInsideRepo(path))
        .context(NothingToSync::NoProject("no toolr project found above /x".into()).to_string());
    let failed_sync = anyhow::anyhow!("uv sync failed");
    for err in [&lookalike, &untrusted, &failed_sync] {
        assert!(!is_nothing_to_sync(err), "{err:#}");
    }
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
        "\"\"\"CI.\"\"\"\ngroup = command_group(\"ci\", \"CI\", \"CI.\")\n@group.command\ndef hello(ctx):\n    \"\"\"Hi.\"\"\"\n",
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
        "\"\"\"CI.\"\"\"\ngroup = command_group(\"ci\", \"CI\", \"CI.\")\n",
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

fn after_trust(repo: &Path, never_prompt: bool, input: Option<&str>) -> (SyncAfterTrust, String) {
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
    env.remove("TOOLR_AUTO_SYNC");
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
    env.remove("TOOLR_AUTO_SYNC");
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
    let err = sync_after_trust(&trusted, false, false, Some((&mut input, &mut out))).unwrap_err();
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
    env.remove("TOOLR_AUTO_SYNC");
    env.remove("TOOLR_VENV_LOCATION");
    let repo = unsynced_repo();
    let (r, out) = after_trust(repo.path(), false, Some("n\n"));
    assert!(matches!(r, SyncAfterTrust::Declined));
    assert!(out.contains("Sync it now?"));
}
