//! `toolr self cache prune` implementation.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use clap::ArgMatches;
use humansize::{BINARY, format_size};
use jiff::Timestamp;

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
    let classification = classify_entries(entries, Timestamp::now(), stale_after_days);
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
mod tests;
