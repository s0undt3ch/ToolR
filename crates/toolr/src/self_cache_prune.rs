//! `toolr self cache prune` implementation.

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use chrono::Utc;
use clap::ArgMatches;
use humansize::{BINARY, format_size};

use toolr_core::cache::{
    CachedVenv, Candidate, Classification, PruneReason, classify_entries, enumerate_caches,
};

pub fn run(cache_root: &Path, matches: &ArgMatches) -> Result<ExitCode> {
    let all = matches.get_flag("all");
    let dry_run = matches.get_flag("dry-run");
    let yes = matches.get_flag("yes");
    let stale_after_days = *matches.get_one::<u32>("stale-after-days").unwrap_or(&30);

    let mut entries = enumerate_caches(cache_root)?;
    entries.sort_by(|a, b| a.meta.repo_path.cmp(&b.meta.repo_path));
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if all {
        return prune_all(cache_root, entries, dry_run, yes, &mut out);
    }
    let classification = classify_entries(entries, Utc::now(), stale_after_days);
    prune_classified(classification, dry_run, &mut out)
}

fn prune_all(
    cache_root: &Path,
    entries: Vec<CachedVenv>,
    dry_run: bool,
    yes: bool,
    out: &mut dyn Write,
) -> Result<ExitCode> {
    if entries.is_empty() {
        writeln!(
            out,
            "toolr: no cache entries to remove under {}",
            cache_root.display()
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    if !yes && !dry_run && !confirm_destroy_all(&entries)? {
        writeln!(out, "toolr: aborted, nothing removed")?;
        return Ok(ExitCode::SUCCESS);
    }

    let mut total_bytes: u64 = 0;
    for e in &entries {
        total_bytes = total_bytes.saturating_add(e.size_bytes);
        if dry_run {
            writeln!(
                out,
                "DRY-RUN would remove {} ({})",
                e.cache_dir.display(),
                format_size(e.size_bytes, BINARY)
            )?;
        } else {
            remove_entry(&e.cache_dir, out)?;
        }
    }
    let action = if dry_run { "would free" } else { "freed" };
    writeln!(
        out,
        "toolr: {action} {} across {} entr{}",
        format_size(total_bytes, BINARY),
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" },
    )?;
    Ok(ExitCode::SUCCESS)
}

fn prune_classified(
    classification: Classification,
    dry_run: bool,
    out: &mut dyn Write,
) -> Result<ExitCode> {
    let candidates: Vec<Candidate> = classification
        .orphan
        .into_iter()
        .chain(classification.stale)
        .collect();
    if candidates.is_empty() {
        writeln!(out, "toolr: nothing to prune")?;
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
            writeln!(
                out,
                "DRY-RUN {tag:<7} {} ({})",
                c.entry.cache_dir.display(),
                format_size(c.entry.size_bytes, BINARY),
            )?;
        } else {
            writeln!(
                out,
                "{tag:<7} removing {} ({})",
                c.entry.cache_dir.display(),
                format_size(c.entry.size_bytes, BINARY),
            )?;
            remove_entry(&c.entry.cache_dir, out)?;
        }
    }
    let action = if dry_run { "would free" } else { "freed" };
    writeln!(
        out,
        "toolr: {action} {} across {} entr{}",
        format_size(total_bytes, BINARY),
        candidates.len(),
        if candidates.len() == 1 { "y" } else { "ies" },
    )?;
    Ok(ExitCode::SUCCESS)
}

fn remove_entry(cache_dir: &Path, out: &mut dyn Write) -> Result<()> {
    match std::fs::remove_dir_all(cache_dir) {
        Ok(()) => Ok(()),
        Err(e) => {
            writeln!(
                out,
                "toolr: warning: failed to remove {}: {e}",
                cache_dir.display()
            )?;
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
    eprint!(
        "toolr: about to remove {} cache entries. continue? [y/N] ",
        entries.len(),
    );
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::path::PathBuf;
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
            },
            size_bytes: size,
            is_orphan: false,
        }
    }

    #[test]
    fn prune_all_with_empty_input_reports_no_entries() {
        let tmp = TempDir::new().unwrap();
        let mut out: Vec<u8> = Vec::new();
        let rc = prune_all(tmp.path(), vec![], false, true, &mut out).unwrap();
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
        let rc = prune_all(tmp.path(), entries, true, true, &mut out).unwrap();
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
        let rc = prune_all(tmp.path(), entries, false, true, &mut out).unwrap();
        assert_eq!(rc, ExitCode::SUCCESS);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("freed"));
        assert!(s.contains("entries"), "expected plural, got {s}");
        assert!(!tmp.path().join("repo-a").exists());
        assert!(!tmp.path().join("repo-b").exists());
    }

    #[test]
    fn prune_classified_empty_reports_nothing_to_prune() {
        let mut out: Vec<u8> = Vec::new();
        let classification = Classification::default();
        let rc = prune_classified(classification, false, &mut out).unwrap();
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
        let rc = prune_classified(classification, true, &mut out).unwrap();
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
        let rc = prune_classified(classification, false, &mut out).unwrap();
        assert_eq!(rc, ExitCode::SUCCESS);
        let s = String::from_utf8(out).unwrap();
        // Singular "entry" — the entries.len() == 1 branch.
        assert!(s.contains(" 1 entry"), "expected singular, got {s}");
        assert!(!entry_dir.exists());
    }

    #[test]
    fn remove_entry_swallows_already_missing_directory_and_warns() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("never-was");
        let mut out: Vec<u8> = Vec::new();
        // `remove_dir_all` on a non-existent path returns `NotFound`; the
        // helper writes a warning and returns Ok(()) regardless so the
        // surrounding loop doesn't abort mid-prune.
        remove_entry(&missing, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("warning: failed to remove"));
    }
}
