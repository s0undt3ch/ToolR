use std::io::Cursor;

use tempfile::TempDir;

use super::*;

pub(super) struct Fixture {
    repo: TempDir,
    store_dir: TempDir,
}

impl Fixture {
    pub(super) fn new() -> Self {
        Self {
            repo: TempDir::new().unwrap(),
            store_dir: TempDir::new().unwrap(),
        }
    }
    pub(super) fn root(&self) -> PathBuf {
        fs::canonicalize(self.repo.path()).unwrap()
    }
    pub(super) fn store(&self) -> TrustStore {
        TrustStore::at(self.store_dir.path().join("trust"))
    }
    fn decide(
        &self,
        input: Option<&str>,
        mode: TrustMode,
    ) -> (Result<Trusted, TrustError>, String) {
        let store = self.store();
        let mut err = Vec::new();
        let result = match input {
            Some(text) => {
                let mut cursor = Cursor::new(text.as_bytes().to_vec());
                decide(
                    &self.root(),
                    &store,
                    false,
                    mode,
                    Some((&mut cursor, &mut err)),
                )
            }
            None => decide(&self.root(), &store, false, mode, None),
        };
        (result, String::from_utf8(err).unwrap())
    }
}

const PROMPT: TrustMode = TrustMode {
    never_prompt: false,
};

#[test]
fn untrusted_without_tty_is_refused() {
    let f = Fixture::new();
    let (r, _) = f.decide(None, PROMPT);
    assert!(matches!(r, Err(TrustError::Untrusted { repo, repair: None }) if repo == f.root()));
}

#[test]
fn untrusted_display_is_the_refusal_text() {
    let msg = TrustError::Untrusted {
        repo: PathBuf::from("/r"),
        repair: None,
    }
    .to_string();
    assert_eq!(
        msg,
        "refusing to run code from an untrusted repository: /r\n\
         Run `toolr project trust` there once to allow it, or set TOOLR_TRUST_ALL=1 for automation."
    );
}

#[test]
fn trust_all_env_allows() {
    let f = Fixture::new();
    let store = f.store();
    let outcome = decide(&f.root(), &store, true, PROMPT, None).unwrap();
    assert!(!outcome.granted_at_prompt());
    assert!(outcome.covers(&f.root()));
    assert!(!store.is_trusted(&f.root()).unwrap());
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn recorded_root_is_allowed() {
    let f = Fixture::new();
    f.store().trust(&f.root()).unwrap();
    let (r, _) = f.decide(None, PROMPT);
    assert!(!r.unwrap().granted_at_prompt());
}

#[test]
fn untrust_removes_record() {
    let f = Fixture::new();
    let store = f.store();
    store.trust(&f.root()).unwrap();
    assert_eq!(store.untrust(&f.root()).unwrap(), (f.root(), true));
    assert!(!store.is_trusted(&f.root()).unwrap());
    assert!(!store.untrust(&f.root()).unwrap().1);
}

#[test]
fn prompt_yes_records_trust() {
    let f = Fixture::new();
    let (r, err) = f.decide(Some("y\n"), PROMPT);
    let proof = r.unwrap();
    assert!(proof.granted_at_prompt());
    assert_eq!(proof.root(), f.root());
    assert!(f.store().is_trusted(&f.root()).unwrap());
    assert!(err.contains("Trust it? [y/N]"));
    assert!(err.contains(&f.root().display().to_string()));
}

#[test]
fn grant_proves_only_the_root_it_recorded() {
    let f = Fixture::new();
    let other = TempDir::new().unwrap();
    let (record, proof) = f.store().grant(&f.root()).unwrap();
    assert_eq!(record.repo_path, f.root());
    assert_eq!(record.key, f.root());
    assert!(!proof.granted_at_prompt());
    assert!(proof.covers(f.repo.path()));
    assert!(!proof.covers(other.path()));
}

#[test]
fn prompt_default_and_eof_refuse() {
    for input in ["\n", ""] {
        let f = Fixture::new();
        let (r, _) = f.decide(Some(input), PROMPT);
        assert!(
            matches!(r, Err(TrustError::Untrusted { .. })),
            "input {input:?}"
        );
        assert!(!f.store().is_trusted(&f.root()).unwrap());
    }
}

#[test]
fn shown_escapes_control_characters() {
    assert_eq!(
        shown(Path::new("evil\r~/mine\x1b[2K")),
        "evil\\u{d}~/mine\\u{1b}[2K"
    );
    assert_eq!(shown(Path::new("a\u{202e}b")), "a\\u{202e}b");
}

#[test]
fn never_prompt_refuses_even_on_tty() {
    let f = Fixture::new();
    let (r, err) = f.decide(Some("y\n"), TrustMode { never_prompt: true });
    assert!(matches!(r, Err(TrustError::Untrusted { .. })));
    assert!(err.is_empty());
}

#[cfg(unix)]
#[test]
fn trust_key_is_canonical_through_symlink() {
    let f = Fixture::new();
    let link_parent = TempDir::new().unwrap();
    let link = link_parent.path().join("link");
    std::os::unix::fs::symlink(f.root(), &link).unwrap();
    let store = f.store();
    store.trust(&link).unwrap();
    assert!(store.is_trusted(&f.root()).unwrap());
    assert_eq!(store.list().unwrap()[0].repo_path, f.root());
}

#[test]
fn store_inside_repo_is_rejected() {
    let f = Fixture::new();
    let store = TrustStore::at(f.root().join("state"));
    let r = decide(&f.root(), &store, false, PROMPT, None);
    assert!(matches!(r, Err(TrustError::StoreInsideRepo(_))));
    assert!(!f.root().join("state").exists());
}

#[test]
fn direct_operations_reject_store_inside_repo() {
    let f = Fixture::new();
    let store = TrustStore::at(f.root().join("state"));
    for err in [
        store.trust(&f.root()).unwrap_err(),
        store.untrust(&f.root()).unwrap_err(),
        store.is_trusted(&f.root()).unwrap_err(),
        store
            .settle_venv(&f.root(), &f.root().join("tools/.venv"), None)
            .unwrap_err(),
    ] {
        assert!(matches!(
            err.downcast_ref::<TrustError>(),
            Some(TrustError::StoreInsideRepo(_))
        ));
    }
    assert!(!f.root().join("state").exists());
}

#[test]
fn store_inside_home_root_is_allowed() {
    let f = Fixture::new();
    let store = TrustStore::at(f.root().join("state"));
    let root = canonical(&f.root());
    let key = RepoKey::resolve(&root, None);
    assert!(store.guard_with_home(&key, Some(&root)).is_ok());
    assert!(matches!(
        store.guard_with_home(&key, None),
        Err(TrustError::StoreInsideRepo(_))
    ));
}

#[test]
fn relative_store_inside_repo_is_rejected() {
    let f = Fixture::new();
    let store = TrustStore::at(PathBuf::from("state/trust"));
    assert!(resolves_inside(store.dir(), &f.root(), &f.root()));
    assert!(!resolves_inside(store.dir(), &f.root(), f.store_dir.path()));
}

#[cfg(unix)]
#[test]
fn store_reaching_repo_through_symlink_is_rejected() {
    let f = Fixture::new();
    let link_parent = TempDir::new().unwrap();
    let link = link_parent.path().join("link");
    std::os::unix::fs::symlink(f.root(), &link).unwrap();
    let store = TrustStore::at(link.join("state"));
    let r = decide(&f.root(), &store, false, PROMPT, None);
    assert!(matches!(r, Err(TrustError::StoreInsideRepo(_))));
    assert!(!f.root().join("state").exists());
}

// Windows reports a path under a file as NotFound, which reads as untrusted.
#[cfg(unix)]
#[test]
fn unusable_store_fails_closed() {
    let f = Fixture::new();
    fs::write(f.store().dir(), b"not a directory").unwrap();
    let (r, _) = f.decide(None, PROMPT);
    assert!(matches!(r, Err(TrustError::Untrusted { .. })));
    assert!(f.store().is_trusted(&f.root()).is_err());
    assert!(f.store().list().is_err());
}

#[test]
fn yes_with_unusable_store_runs_and_warns() {
    let f = Fixture::new();
    fs::write(f.store().dir(), b"not a directory").unwrap();
    let (r, err) = f.decide(Some("y\n"), PROMPT);
    assert!(r.is_ok());
    assert!(err.contains("warning: could not record trust"), "{err}");
}

#[test]
fn record_that_is_a_directory_is_an_error_not_untrusted() {
    let f = Fixture::new();
    let store = f.store();
    store.trust(&f.root()).unwrap();
    let record = store.record_path(&f.root(), KeyKind::PerPath);
    fs::remove_file(&record).unwrap();
    fs::create_dir(&record).unwrap();
    assert!(store.is_trusted(&f.root()).is_err());
    assert!(store.untrust(&f.root()).is_err());
}

#[test]
fn list_skips_leftover_temp_files() {
    let f = Fixture::new();
    let store = f.store();
    store.trust(&f.root()).unwrap();
    fs::write(store.dir().join(".trust.abc123.tmp"), b"{}").unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn list_skips_corrupt_records() {
    let f = Fixture::new();
    let store = f.store();
    store.trust(&f.root()).unwrap();
    fs::write(store.dir().join("junk.json"), b"{not json").unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn record_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let store = f.store();
    store.trust(&f.root()).unwrap();
    let dir_mode = fs::metadata(store.dir()).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700);
    let file = fs::read_dir(store.dir())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let file_mode = fs::metadata(file).unwrap().permissions().mode() & 0o777;
    assert_eq!(file_mode, 0o600);
}

/// `TOOLR_TRUST_ALL` answers before the store is consulted, even one inside the
/// repository; otherwise an unusable store or unknown root is untrusted.
/// Nothing creates the store directory.
#[test]
fn is_trusted_now_reads_env_then_store_and_creates_nothing() {
    let f = Fixture::new();
    let mut env = crate::test_env::EnvGuard::write();
    env.set("XDG_STATE_HOME", f.root());
    env.set("TOOLR_TRUST_ALL", "1");
    assert!(is_trusted_now(&f.root()));

    env.set("TOOLR_TRUST_ALL", "true");
    assert!(is_trusted_now(&f.root()));
    env.set("TOOLR_TRUST_ALL", "0");
    assert!(!is_trusted_now(&f.root()));

    env.remove("TOOLR_TRUST_ALL");
    assert!(!is_trusted_now(&f.root()));
    env.set("XDG_STATE_HOME", f.store_dir.path());
    assert!(!is_trusted_now(&f.root()));

    assert!(!f.root().join("toolr").exists());
    assert!(!f.store_dir.path().join("toolr").exists());
}

/// git can't create directories under a verbatim (`\\?\`) path on Windows.
fn git_path(path: &Path) -> &str {
    let s = path.to_str().unwrap();
    s.strip_prefix(r"\\?\").unwrap_or(s)
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_CEILING_DIRECTORIES")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "git {args:?}: {stderr}");
    String::from_utf8(out.stdout).unwrap()
}

fn init_repo(path: &Path) -> PathBuf {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q"]);
    git(path, &["commit", "-q", "--allow-empty", "-m", "init"]);
    fs::canonicalize(path).unwrap()
}

fn add_worktree(repo: &Path, wt: &Path) -> PathBuf {
    git(repo, &["worktree", "add", "-q", "--detach", git_path(wt)]);
    fs::canonicalize(wt).unwrap()
}

fn decide_at(store: &TrustStore, root: &Path) -> Result<Trusted, TrustError> {
    decide(root, store, false, PROMPT, None)
}

#[test]
fn worktree_proof_covers_main_and_keeps_its_own_root() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let wt = add_worktree(&main, &tmp.path().join("wt"));
    let unrelated = TempDir::new().unwrap();
    let (_, proof) = f.store().grant(&wt).unwrap();
    assert_eq!(proof.root(), wt);
    assert!(proof.covers(&main));
    assert!(!proof.covers(unrelated.path()));
}

#[test]
fn trust_main_then_worktree_is_trusted() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let inside = add_worktree(&main, &main.join(".worktrees/in"));
    let outside = add_worktree(&main, &tmp.path().join("out"));
    let store = f.store();
    store.trust(&main).unwrap();
    assert!(store.is_trusted(&inside).unwrap());
    assert!(store.is_trusted(&outside).unwrap());
    let r = decide_at(&store, &outside);
    assert_eq!(r.unwrap().root(), outside);
}

#[test]
fn untrust_from_worktree_untrusts_main() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let wt = add_worktree(&main, &tmp.path().join("wt"));
    let store = f.store();
    store.trust(&wt).unwrap();
    assert!(store.is_trusted(&main).unwrap());
    assert_eq!(store.untrust(&wt).unwrap(), (main.clone(), true));
    assert!(!store.is_trusted(&main).unwrap());
    assert!(!store.is_trusted(&wt).unwrap());
}

#[test]
fn two_projects_in_one_repo_share_trust() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    fs::create_dir_all(main.join("a")).unwrap();
    fs::create_dir_all(main.join("b")).unwrap();
    let store = f.store();
    store.trust(&main.join("a")).unwrap();
    assert!(store.is_trusted(&main.join("b")).unwrap());
}

#[test]
fn record_stores_key_and_display() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let record = f.store().trust(&main).unwrap();
    assert_eq!(record.key, main.join(".git"));
    assert_eq!(record.repo_path, main);
    assert_eq!(record.kind, KeyKind::Repository);
    assert_eq!(record.schema_version, 2);
}

#[test]
fn untrust_key_removes_by_stored_key() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let store = f.store();
    let record = store.trust(&main).unwrap();
    assert!(store.untrust_key(&record.key).unwrap());
    assert!(!store.untrust_key(&record.key).unwrap());
    assert!(!store.is_trusted(&main).unwrap());
}

#[test]
fn list_skips_old_schema_records() {
    let f = Fixture::new();
    let store = f.store();
    store.trust(&f.root()).unwrap();
    fs::write(
        store.dir().join("old.json"),
        r#"{"schema_version":1,"repo_path":"/old","trusted_at":"2026-01-01T00:00:00Z","toolr_version":"0"}"#,
    )
    .unwrap();
    let mut stale = serde_json::to_value(&store.list().unwrap()[0]).unwrap();
    stale["schema_version"] = 1.into();
    fs::write(store.dir().join("stale.json"), stale.to_string()).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    fs::remove_file(store.record_path(&f.root(), KeyKind::PerPath)).unwrap();
    fs::write(
        store.record_path(&f.root(), KeyKind::PerPath),
        stale.to_string(),
    )
    .unwrap();
    assert!(!store.is_trusted(&f.root()).unwrap());
}

fn record_with_trusted_at(trusted_at: &str) -> String {
    let f = Fixture::new();
    let store = f.store();
    let record = store.trust(&f.root()).unwrap();
    let mut json = serde_json::to_value(&record).unwrap();
    json["trusted_at"] = trusted_at.into();
    json.to_string()
}

#[test]
fn trust_record_reads_chrono_written_timestamps() {
    let parse = |s: &str| serde_json::from_str::<TrustRecord>(&record_with_trusted_at(s));
    let z = parse("2026-01-01T00:00:00Z").unwrap();
    let offset = parse("2026-01-01T00:00:00+00:00").unwrap();
    let fractional = parse("2026-01-01T00:00:00.123456789Z").unwrap();
    assert_eq!(fractional.trusted_at.subsec_nanosecond(), 123_456_789);
    assert_eq!(z.trusted_at, offset.trusted_at);
}

#[test]
fn trust_record_serialises_trusted_at_as_rfc3339_z() {
    let record: TrustRecord =
        serde_json::from_str(&record_with_trusted_at("2026-05-11T12:34:56+00:00")).unwrap();
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["trusted_at"], "2026-05-11T12:34:56Z");
}

#[test]
fn missing_project_root_untrusts_cleanly() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let dir = fs::canonicalize(tmp.path()).unwrap().join("gone");
    fs::create_dir(&dir).unwrap();
    let store = f.store();
    store.trust(&dir).unwrap();
    fs::remove_dir(&dir).unwrap();
    assert_eq!(store.untrust(&dir).unwrap(), (dir, true));
}

#[test]
fn store_in_main_checkout_refused_from_external_worktree() {
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let wt = add_worktree(&main, &tmp.path().join("wt"));
    let store = TrustStore::at(main.join("state"));
    let r = decide_at(&store, &wt);
    assert!(matches!(r, Err(TrustError::StoreInsideRepo(_))));
    assert!(!main.join("state").exists());
}

#[test]
fn home_dotgit_project_not_refused_by_guard() {
    let tmp = TempDir::new().unwrap();
    let home = init_repo(&tmp.path().join("home"));
    let store = TrustStore::at(home.join("state/trust"));
    let key = RepoKey::resolve(&home, Some(&home));
    assert_eq!(key.key, home.join(".git"));
    assert_eq!(key.display, home);
    assert!(store.guard_with_home(&key, Some(&home)).is_ok());
    assert!(store.guard_with_home(&key, None).is_err());
}

#[test]
fn store_in_external_worktree_refused() {
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let wt = add_worktree(&main, &tmp.path().join("wt"));
    let store = TrustStore::at(wt.join("state/trust"));
    let r = decide_at(&store, &wt);
    assert!(matches!(r, Err(TrustError::StoreInsideRepo(_))));
    assert!(!wt.join("state").exists());
}

#[test]
fn home_exemption_is_per_path() {
    let tmp = TempDir::new().unwrap();
    let home = init_repo(&tmp.path().join("home"));
    let key = RepoKey::resolve(&home, Some(&home));
    let store = TrustStore::at(home.join(".git/state"));
    assert!(matches!(
        store.guard_with_home(&key, Some(&home)),
        Err(TrustError::StoreInsideRepo(_))
    ));
}

#[test]
fn store_in_worktree_top_above_nested_project_refused() {
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let wt = add_worktree(&main, &tmp.path().join("wt"));
    fs::create_dir(wt.join("sub")).unwrap();
    let store = TrustStore::at(wt.join("state"));
    let r = decide_at(&store, &wt.join("sub"));
    assert!(matches!(r, Err(TrustError::StoreInsideRepo(_))));
    assert!(!wt.join("state").exists());
}

#[test]
fn untrusted_error_carries_repair_hint() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let wt = add_worktree(&main, &tmp.path().join("wt"));
    fs::create_dir(wt.join("sub")).unwrap();
    fs::rename(&wt, tmp.path().join("moved")).unwrap();
    let moved = fs::canonicalize(tmp.path().join("moved")).unwrap();
    let store = f.store();
    let r = decide_at(&store, &moved.join("sub"));
    let err = r.unwrap_err();
    assert!(matches!(
        &err,
        TrustError::Untrusted { repair: Some(h), .. } if h.worktree_top == moved
    ));
    let text = err.to_string();
    let expected = format!(
        "This directory claims to be worktree `wt` of {}. Only if you moved that \
         worktree yourself, run `git worktree repair {}` from the main checkout.",
        main.display(),
        moved.display()
    );
    assert!(text.contains(&expected), "{text}");
    let nested = format!("git worktree repair {}`", moved.join("sub").display());
    assert!(!text.contains(&nested), "{text}");
}

#[test]
fn deleted_unpruned_worktree_gets_cautious_hint() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let main = init_repo(&tmp.path().join("main"));
    let wt = add_worktree(&main, &tmp.path().join("feature"));
    fs::remove_dir_all(&wt).unwrap();
    let evil = tmp.path().join("evil");
    fs::create_dir(&evil).unwrap();
    let gitdir = main.join(".git/worktrees/feature");
    fs::write(evil.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    let evil = fs::canonicalize(evil).unwrap();
    let r = decide_at(&f.store(), &evil);
    let text = r.unwrap_err().to_string();
    assert!(
        text.contains(&format!(
            "claims to be worktree `feature` of {}. Only if you moved that worktree yourself, \
             run `git worktree repair {}`",
            main.display(),
            evil.display()
        )),
        "{text}"
    );
}

#[test]
fn prompt_mentions_worktrees() {
    let f = Fixture::new();
    let (_, err) = f.decide(Some("n\n"), PROMPT);
    assert!(err.contains("worktrees"), "{err}");
}

#[test]
fn forged_worktree_layout_does_not_borrow_a_per_path_record() {
    let f = Fixture::new();
    let tmp = TempDir::new().unwrap();
    let p = fs::canonicalize(tmp.path()).unwrap();
    let store = f.store();
    store.trust(&p).unwrap();
    fs::create_dir_all(p.join("worktrees/proj")).unwrap();
    fs::create_dir_all(p.join("worktrees/g")).unwrap();
    fs::write(p.join("worktrees/proj/.git"), "gitdir: ../g\n").unwrap();
    fs::write(p.join("worktrees/g/gitdir"), "../proj/.git\n").unwrap();
    let forged = p.join("worktrees/proj");
    let key = resolve_key(&forged);
    assert_eq!(
        (key.key.as_path(), key.kind),
        (p.as_path(), KeyKind::Repository)
    );
    assert!(!store.is_trusted(&forged).unwrap());
    assert!(store.is_trusted(&p).unwrap());
}
