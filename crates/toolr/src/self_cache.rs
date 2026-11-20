//! `toolr self cache <...>` implementation.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::ArgMatches;
use humansize::{BINARY, format_size};
use jiff::Timestamp;

use toolr_core::cache::{CachedVenv, enumerate_caches};

/// Resolve `$XDG_CACHE_HOME/toolr/`. Same precedence as
/// `toolr_core::uv::toolr_cache_dir` so writers and readers agree.
pub fn resolve_cache_root() -> Result<PathBuf> {
    toolr_core::uv::toolr_cache_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve toolr cache directory"))
}

pub fn dispatch(matches: &ArgMatches, quiet: bool) -> Result<ExitCode> {
    let root = resolve_cache_root()?;
    match matches.subcommand() {
        Some(("list", _)) => {
            run_list(&root, &mut io::stdout().lock())?;
            Ok(ExitCode::SUCCESS)
        }
        Some(("prune", prune_m)) => crate::self_cache_prune::run(&root, prune_m, quiet),
        _ => Ok(ExitCode::from(2)),
    }
}

pub fn run_list(cache_root: &Path, out: &mut dyn Write) -> Result<()> {
    let mut caches = enumerate_caches(cache_root)?;
    caches.sort_by_key(|c| std::cmp::Reverse(c.meta.last_used_at));

    if caches.is_empty() {
        writeln!(
            out,
            "toolr: no cached virtualenvs found under {}",
            cache_root.display()
        )?;
        return Ok(());
    }
    write_table(out, &caches)?;
    Ok(())
}

fn write_table(out: &mut dyn Write, caches: &[CachedVenv]) -> Result<()> {
    let mut w_repo = "REPO".len();
    let mut w_size = "SIZE".len();
    let mut w_last = "LAST USED".len();

    let now = Timestamp::now();
    let rows: Vec<(String, String, String)> = caches
        .iter()
        .map(|c| {
            let repo = toolr_core::trust::shown(&c.meta.repo_path);
            let size = format_size(c.size_bytes, BINARY);
            let last = human_ago(now, c.meta.last_used_at);
            w_repo = w_repo.max(repo.len());
            w_size = w_size.max(size.len());
            w_last = w_last.max(last.len());
            (repo, size, last)
        })
        .collect();

    writeln!(
        out,
        "{:<w_repo$}  {:>w_size$}  {:<w_last$}",
        "REPO",
        "SIZE",
        "LAST USED",
        w_repo = w_repo,
        w_size = w_size,
        w_last = w_last,
    )
    .context("writing cache table header")?;
    for (repo, size, last) in rows {
        writeln!(
            out,
            "{:<w_repo$}  {:>w_size$}  {:<w_last$}",
            repo,
            size,
            last,
            w_repo = w_repo,
            w_size = w_size,
            w_last = w_last,
        )?;
    }
    Ok(())
}

/// Human-friendly relative time. Coarse buckets are fine — the column
/// exists to let users spot ancient entries at a glance.
pub fn human_ago(now: Timestamp, then: Timestamp) -> String {
    let delta = now.duration_since(then);
    let secs = delta.as_secs();
    let days = secs / 86_400;
    if secs < 60 {
        "just now".into()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else if days < 30 {
        format!("{days}d ago")
    } else if days < 365 {
        format!("{}mo ago", days / 30)
    } else {
        format!("{}y ago", days / 365)
    }
}

#[cfg(test)]
mod tests {
    use jiff::{SignedDuration, civil::date, tz::Offset};

    use super::human_ago;

    #[test]
    fn human_ago_picks_the_coarsest_bucket() {
        let now = Offset::UTC
            .to_timestamp(date(2026, 11, 4).at(12, 0, 0, 0))
            .unwrap();
        let cases = [
            (SignedDuration::from_secs(30), "just now"),
            (SignedDuration::from_mins(5), "5m ago"),
            (SignedDuration::from_hours(3), "3h ago"),
            (SignedDuration::from_hours(4 * 24), "4d ago"),
            (SignedDuration::from_hours(90 * 24), "3mo ago"),
            (SignedDuration::from_hours(800 * 24), "2y ago"),
        ];
        for (age, expected) in cases {
            assert_eq!(human_ago(now, now - age), expected, "{age:?}");
        }
    }
}
