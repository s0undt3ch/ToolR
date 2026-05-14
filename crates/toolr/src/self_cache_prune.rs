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
