//! `toolr self cache prune` implementation.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use chrono::Utc;
use clap::ArgMatches;
use humansize::{BINARY, format_size};

use toolr_core::cache::{
    CachedVenv, Candidate, Classification, PruneReason, classify_entries, enumerate_caches,
    sweep_rebuild_locks,
};

/// Output of one prune run. The `-q` rule is stated on the arg in `cli::root_flag_args`.
struct Report<'a> {
    out: &'a mut dyn Write,
    /// Test seam for where warnings go; `None` is the process's stderr.
    err: Option<&'a mut dyn Write>,
    quiet: bool,
    dry_run: bool,
}

impl<'a> Report<'a> {
    fn new(out: &'a mut dyn Write, quiet: bool, dry_run: bool) -> Self {
        Self {
            out,
            err: None,
            quiet,
            dry_run,
        }
    }

    /// Send warnings to `err` rather than the process's stderr.
    #[cfg(test)]
    fn with_err(mut self, err: &'a mut dyn Write) -> Self {
        self.err = Some(err);
        self
    }

    /// Success chatter. A dry run's lines are its answer, so `--quiet` keeps them.
    fn note(&mut self, line: &str) -> io::Result<()> {
        if self.quiet && !self.dry_run {
            return Ok(());
        }
        writeln!(self.out, "{line}")
    }

    /// Warnings go to stderr, and always print.
    fn warn(&mut self, line: &str) -> io::Result<()> {
        match self.err.as_deref_mut() {
            Some(err) => writeln!(err, "toolr: warning: {line}"),
            None => writeln!(io::stderr(), "toolr: warning: {line}"),
        }
    }
}

pub fn run(cache_root: &Path, matches: &ArgMatches, quiet: bool) -> Result<ExitCode> {
    let all = matches.get_flag("all");
    let dry_run = matches.get_flag("dry-run");
    let yes = matches.get_flag("yes");
    let stale_after_days = *matches.get_one::<u32>("stale-after-days").unwrap_or(&30);

    let mut entries = enumerate_caches(cache_root)?;
    entries.sort_by(|a, b| a.meta.repo_path.cmp(&b.meta.repo_path));
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut report = Report::new(&mut out, quiet, dry_run);

    if all {
        return prune_all(cache_root, entries, yes, confirm_destroy_all, &mut report);
    }
    let classification = classify_entries(entries, Utc::now(), stale_after_days);
    prune_classified(cache_root, classification, &mut report)
}

fn prune_all(
    cache_root: &Path,
    entries: Vec<CachedVenv>,
    yes: bool,
    confirm: impl FnOnce(&[CachedVenv]) -> Result<bool>,
    report: &mut Report,
) -> Result<ExitCode> {
    let dry_run = report.dry_run;
    if entries.is_empty() {
        // Leftover locks are empty files holding no data, so they need no confirmation.
        if sweep_locks(cache_root, &[], report)? == 0 {
            let root = cache_root.display();
            let line = format!("toolr: no cache entries to remove under {root}");
            report.note(&line)?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    if !yes && !dry_run && !confirm(&entries)? {
        report.note("toolr: aborted, nothing removed")?;
        return Ok(ExitCode::SUCCESS);
    }

    let mut total_bytes: u64 = 0;
    for e in &entries {
        total_bytes = total_bytes.saturating_add(e.size_bytes);
        if dry_run {
            let line = format!(
                "DRY-RUN would remove {} ({})",
                e.cache_dir.display(),
                format_size(e.size_bytes, BINARY)
            );
            report.note(&line)?;
        } else {
            remove_entry(&e.cache_dir, report)?;
        }
    }
    let action = if dry_run { "would free" } else { "freed" };
    let line = format!(
        "toolr: {action} {} across {} entr{}",
        format_size(total_bytes, BINARY),
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" },
    );
    report.note(&line)?;
    sweep_locks(cache_root, &[], report)?;
    Ok(ExitCode::SUCCESS)
}

fn prune_classified(
    cache_root: &Path,
    classification: Classification,
    report: &mut Report,
) -> Result<ExitCode> {
    let dry_run = report.dry_run;
    let keep_repos: Vec<PathBuf> = classification
        .keep
        .iter()
        .map(|e| e.meta.repo_path.clone())
        .collect();
    let candidates: Vec<Candidate> = classification
        .orphan
        .into_iter()
        .chain(classification.stale)
        .collect();
    if candidates.is_empty() {
        if sweep_locks(cache_root, &keep_repos, report)? == 0 {
            report.note("toolr: nothing to prune")?;
        }
        return Ok(ExitCode::SUCCESS);
    }

    let mut total_bytes: u64 = 0;
    for c in &candidates {
        total_bytes = total_bytes.saturating_add(c.entry.size_bytes);
        let tag = match c.reason {
            PruneReason::Orphan => "ORPHAN",
            PruneReason::Stale => "STALE",
        };
        if dry_run {
            let line = format!(
                "DRY-RUN {tag:<7} {} ({})",
                c.entry.cache_dir.display(),
                format_size(c.entry.size_bytes, BINARY),
            );
            report.note(&line)?;
        } else {
            let line = format!(
                "{tag:<7} removing {} ({})",
                c.entry.cache_dir.display(),
                format_size(c.entry.size_bytes, BINARY),
            );
            report.note(&line)?;
            remove_entry(&c.entry.cache_dir, report)?;
        }
    }
    let action = if dry_run { "would free" } else { "freed" };
    let line = format!(
        "toolr: {action} {} across {} entr{}",
        format_size(total_bytes, BINARY),
        candidates.len(),
        if candidates.len() == 1 { "y" } else { "ies" },
    );
    report.note(&line)?;
    sweep_locks(cache_root, &keep_repos, report)?;
    Ok(ExitCode::SUCCESS)
}

/// Remove rebuild locks none of `keep_repos` needs. A failure only warns, like `remove_entry`.
/// The count is returned whether or not the line prints: the caller's "nothing to prune" depends on it.
fn sweep_locks(cache_root: &Path, keep_repos: &[PathBuf], report: &mut Report) -> Result<usize> {
    match sweep_rebuild_locks(cache_root, keep_repos, report.dry_run) {
        Ok(sweep) if sweep.removed > 0 => {
            let action = if report.dry_run {
                "would remove"
            } else {
                "removed"
            };
            let plural = if sweep.removed == 1 { "" } else { "s" };
            let n = sweep.removed;
            report.note(&format!("toolr: {action} {n} rebuild lock{plural}"))?;
            Ok(sweep.removed)
        }
        Ok(_) => Ok(0),
        Err(e) => {
            report.warn(&format!("failed to sweep rebuild locks: {e}"))?;
            Ok(0)
        }
    }
}

fn remove_entry(cache_dir: &Path, report: &mut Report) -> Result<()> {
    match std::fs::remove_dir_all(cache_dir) {
        Ok(()) => Ok(()),
        Err(e) => {
            let line = format!("failed to remove {}: {e}", cache_dir.display());
            report.warn(&line)?;
            Ok(())
        }
    }
}

fn confirm_destroy_all(entries: &[CachedVenv]) -> Result<bool> {
    if !io::stdin().is_terminal() {
        anyhow::bail!(
            "refusing to wipe {} cache entries without --yes (stdin is not a terminal)",
            entries.len(),
        );
    }
    Ok(toolr_core::prompt::yes_no_stdin(
        &format!(
            "toolr: about to remove {} cache entries. continue?",
            entries.len()
        ),
        false,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::TempDir;
    use toolr_core::cache::CachedVenv;

    fn make_entry(cache_dir: PathBuf, repo_path: PathBuf, size: u64) -> CachedVenv {
        // Inline meta builder — full Meta struct construction lives here
        // so the test stays decoupled from any meta::new convenience.
        let ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
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
}
