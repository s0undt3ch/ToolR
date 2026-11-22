use super::*;
use jiff::{civil::date, tz::Offset};
use tempfile::TempDir;
use toolr_core::cache::CachedVenv;

fn make_entry(cache_dir: PathBuf, repo_path: PathBuf, size: u64) -> CachedVenv {
    // Inline meta builder — full Meta struct construction lives here
    // so the test stays decoupled from any meta::new convenience.
    let ts = Offset::UTC
        .to_timestamp(date(2026, 1, 1).at(0, 0, 0, 0))
        .unwrap();
    CachedVenv {
        repo_key: cache_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        cache_dir,
        meta: toolr_core::cache::Meta {
            schema_version: 1,
            repo_path,
            toolr_version: "test".into(),
            python_version: "3.13.1".into(),
            created_at: ts,
            last_used_at: ts,
            interpreter_path: None,
            interpreter_hash: None,
        },
        size_bytes: size,
        is_orphan: false,
    }
}

fn orphan_candidate(dir: &Path, bytes: u64) -> Candidate {
    std::fs::create_dir_all(dir.join("venv")).unwrap();
    std::fs::write(dir.join("venv/blob"), vec![0u8; bytes as usize]).unwrap();
    Candidate {
        entry: make_entry(dir.to_path_buf(), "/missing".into(), bytes),
        reason: PruneReason::Orphan,
    }
}

fn two_entries(root: &Path) -> Vec<CachedVenv> {
    ["repo-a", "repo-b"]
        .iter()
        .map(|key| {
            let dir = root.join(key);
            std::fs::create_dir_all(dir.join("venv")).unwrap();
            std::fs::write(dir.join("venv/blob"), vec![0u8; 64]).unwrap();
            make_entry(dir, root.to_path_buf(), 64)
        })
        .collect()
}

/// A stray rebuild lock: an empty `*.lock` file no kept repo owns.
fn stray_lock(root: &Path) -> PathBuf {
    let lock = toolr_core::manifest::lock_path_for_repo(root, Path::new("/stray/repo"));
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, b"").unwrap();
    lock
}

#[test]
fn prune_all_with_empty_input_reports_no_entries() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let rc = prune_all(
        tmp.path(),
        vec![],
        true,
        confirm_destroy_all,
        &mut Report::new(&mut out, false, false),
    )
    .unwrap();
    assert_eq!(rc, ExitCode::SUCCESS);
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("no cache entries to remove"));
}

#[test]
fn prune_all_dry_run_keeps_files_on_disk_and_says_would_free() {
    let tmp = TempDir::new().unwrap();
    let entry_dir = tmp.path().join("repo-key-1");
    std::fs::create_dir_all(entry_dir.join("venv")).unwrap();
    std::fs::write(entry_dir.join("venv/blob"), vec![0u8; 256]).unwrap();
    let entries = vec![make_entry(entry_dir.clone(), tmp.path().to_path_buf(), 256)];

    let mut out: Vec<u8> = Vec::new();
    let rc = prune_all(
        tmp.path(),
        entries,
        true,
        confirm_destroy_all,
        &mut Report::new(&mut out, false, true),
    )
    .unwrap();
    assert_eq!(rc, ExitCode::SUCCESS);
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("DRY-RUN would remove"));
    assert!(s.contains("would free"));
    assert!(entry_dir.exists(), "dry-run must not remove anything");
}

#[test]
fn prune_all_with_yes_removes_directories_and_reports_freed() {
    let tmp = TempDir::new().unwrap();
    // Two entries, mixed sizes, so the plural form fires too.
    let mut entries = Vec::new();
    for (key, bytes) in [("repo-a", 128u64), ("repo-b", 256u64)] {
        let entry_dir = tmp.path().join(key);
        std::fs::create_dir_all(entry_dir.join("venv")).unwrap();
        std::fs::write(entry_dir.join("venv/blob"), vec![0u8; bytes as usize]).unwrap();
        entries.push(make_entry(entry_dir, tmp.path().to_path_buf(), bytes));
    }

    let mut out: Vec<u8> = Vec::new();
    let rc = prune_all(
        tmp.path(),
        entries,
        true,
        confirm_destroy_all,
        &mut Report::new(&mut out, false, false),
    )
    .unwrap();
    assert_eq!(rc, ExitCode::SUCCESS);
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("freed"));
    assert!(s.contains("entries"), "expected plural, got {s}");
    assert!(!tmp.path().join("repo-a").exists());
    assert!(!tmp.path().join("repo-b").exists());
}

#[test]
fn prune_classified_empty_reports_nothing_to_prune() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let classification = Classification::default();
    let rc = prune_classified(
        tmp.path(),
        classification,
        &mut Report::new(&mut out, false, false),
    )
    .unwrap();
    assert_eq!(rc, ExitCode::SUCCESS);
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("nothing to prune"));
}

#[test]
fn prune_classified_dry_run_emits_orphan_and_stale_tags() {
    let tmp = TempDir::new().unwrap();
    let orphan_dir = tmp.path().join("orphan");
    let stale_dir = tmp.path().join("stale");
    std::fs::create_dir_all(&orphan_dir).unwrap();
    std::fs::create_dir_all(&stale_dir).unwrap();
    let classification = Classification {
        keep: vec![],
        orphan: vec![Candidate {
            entry: make_entry(orphan_dir.clone(), "/missing".into(), 32),
            reason: PruneReason::Orphan,
        }],
        stale: vec![Candidate {
            entry: make_entry(stale_dir.clone(), tmp.path().to_path_buf(), 64),
            reason: PruneReason::Stale,
        }],
    };

    let mut out: Vec<u8> = Vec::new();
    let rc = prune_classified(
        tmp.path(),
        classification,
        &mut Report::new(&mut out, false, true),
    )
    .unwrap();
    assert_eq!(rc, ExitCode::SUCCESS);
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("DRY-RUN ORPHAN"));
    assert!(s.contains("DRY-RUN STALE"));
    assert!(s.contains("would free"));
    // Nothing on disk got touched.
    assert!(orphan_dir.exists());
    assert!(stale_dir.exists());
}

#[test]
fn prune_classified_real_run_removes_one_entry_singular_form() {
    let tmp = TempDir::new().unwrap();
    let entry_dir = tmp.path().join("only-one");
    std::fs::create_dir_all(entry_dir.join("venv")).unwrap();
    std::fs::write(entry_dir.join("venv/blob"), vec![0u8; 16]).unwrap();
    let classification = Classification {
        keep: vec![],
        orphan: vec![],
        stale: vec![Candidate {
            entry: make_entry(entry_dir.clone(), tmp.path().to_path_buf(), 16),
            reason: PruneReason::Stale,
        }],
    };
    let mut out: Vec<u8> = Vec::new();
    let rc = prune_classified(
        tmp.path(),
        classification,
        &mut Report::new(&mut out, false, false),
    )
    .unwrap();
    assert_eq!(rc, ExitCode::SUCCESS);
    let s = String::from_utf8(out).unwrap();
    // Singular "entry" — the entries.len() == 1 branch.
    assert!(s.contains(" 1 entry"), "expected singular, got {s}");
    assert!(!entry_dir.exists());
}

#[test]
fn prune_all_quiet_real_run_removes_silently() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, false);
    prune_all(
        tmp.path(),
        two_entries(tmp.path()),
        true,
        confirm_destroy_all,
        rep,
    )
    .unwrap();
    assert!(out.is_empty(), "got {}", String::from_utf8_lossy(&out));
    assert!(!tmp.path().join("repo-a").exists());
    assert!(!tmp.path().join("repo-b").exists());
}

#[test]
fn prune_all_quiet_dry_run_keeps_result_lines() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, true);
    prune_all(
        tmp.path(),
        two_entries(tmp.path()),
        true,
        confirm_destroy_all,
        rep,
    )
    .unwrap();
    let s = String::from_utf8(out).unwrap();
    assert_eq!(s.matches("DRY-RUN would remove").count(), 2, "got {s}");
    assert!(s.contains("would free"));
    assert!(tmp.path().join("repo-a").exists());
}

#[test]
fn prune_all_quiet_empty_is_silent() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, false);
    prune_all(tmp.path(), vec![], true, confirm_destroy_all, rep).unwrap();
    assert!(out.is_empty(), "got {}", String::from_utf8_lossy(&out));
}

#[test]
fn prune_all_quiet_dry_run_empty_still_reports() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, true);
    prune_all(tmp.path(), vec![], true, confirm_destroy_all, rep).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("no cache entries to remove"), "got {s}");
}

#[test]
fn prune_all_quiet_declined_prompt_is_silent_and_keeps_entries() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, false);
    let rc = prune_all(
        tmp.path(),
        two_entries(tmp.path()),
        false,
        |_| Ok(false),
        rep,
    )
    .unwrap();
    assert_eq!(rc, ExitCode::SUCCESS);
    assert!(out.is_empty(), "got {}", String::from_utf8_lossy(&out));
    assert!(tmp.path().join("repo-a").exists());
    assert!(tmp.path().join("repo-b").exists());
}

#[test]
fn prune_all_declined_prompt_reports_abort() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, false, false);
    prune_all(
        tmp.path(),
        two_entries(tmp.path()),
        false,
        |_| Ok(false),
        rep,
    )
    .unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("aborted, nothing removed"), "got {s}");
    assert!(tmp.path().join("repo-a").exists());
}

#[test]
fn prune_classified_quiet_real_run_removes_silently() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("orphan");
    let classification = Classification {
        keep: vec![],
        orphan: vec![orphan_candidate(&dir, 16)],
        stale: vec![],
    };
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, false);
    prune_classified(tmp.path(), classification, rep).unwrap();
    assert!(out.is_empty(), "got {}", String::from_utf8_lossy(&out));
    assert!(!dir.exists());
}

#[test]
fn prune_classified_quiet_dry_run_keeps_tagged_lines() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("orphan");
    let classification = Classification {
        keep: vec![],
        orphan: vec![orphan_candidate(&dir, 16)],
        stale: vec![],
    };
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, true);
    prune_classified(tmp.path(), classification, rep).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("DRY-RUN ORPHAN"), "got {s}");
    assert!(s.contains("would free"));
    assert!(dir.exists());
}

#[test]
fn prune_classified_quiet_nothing_to_prune_is_silent() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, false);
    prune_classified(tmp.path(), Classification::default(), rep).unwrap();
    assert!(out.is_empty(), "got {}", String::from_utf8_lossy(&out));
}

#[test]
fn prune_classified_quiet_dry_run_nothing_to_prune_still_reports() {
    let tmp = TempDir::new().unwrap();
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, true);
    prune_classified(tmp.path(), Classification::default(), rep).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("nothing to prune"), "got {s}");
}

#[test]
fn sweep_locks_quiet_returns_count_without_printing() {
    let tmp = TempDir::new().unwrap();
    let lock = stray_lock(tmp.path());
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, false);
    assert_eq!(sweep_locks(tmp.path(), &[], rep).unwrap(), 1);
    assert!(out.is_empty(), "got {}", String::from_utf8_lossy(&out));
    assert!(!lock.exists());
}

#[test]
fn sweep_locks_quiet_dry_run_still_reports() {
    let tmp = TempDir::new().unwrap();
    let lock = stray_lock(tmp.path());
    let mut out: Vec<u8> = Vec::new();
    let rep = &mut Report::new(&mut out, true, true);
    assert_eq!(sweep_locks(tmp.path(), &[], rep).unwrap(), 1);
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("would remove 1 rebuild lock"), "got {s}");
    assert!(lock.exists());
}

#[test]
fn remove_entry_swallows_already_missing_directory_and_warns_on_stderr() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("never-was");
    for quiet in [false, true] {
        let (mut out, mut err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
        // `remove_dir_all` on a non-existent path returns `NotFound`; the helper warns and
        // returns Ok(()) regardless so the surrounding loop doesn't abort mid-prune.
        let rep = &mut Report::new(&mut out, quiet, false).with_err(&mut err);
        remove_entry(&missing, rep).unwrap();
        let s = String::from_utf8(err).unwrap();
        assert!(
            s.contains("toolr: warning: failed to remove"),
            "quiet={quiet}, got {s}"
        );
        assert!(out.is_empty(), "quiet={quiet}");
    }
}
