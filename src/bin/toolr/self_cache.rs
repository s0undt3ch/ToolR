//! `toolr self cache <...>` implementation.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::ArgMatches;
use humansize::{BINARY, format_size};

use _rust_utils::cache::{CachedVenv, enumerate_caches};

/// Resolve `$XDG_CACHE_HOME/toolr/`. Same precedence as
/// `_rust_utils::uv::toolr_cache_dir` so writers and readers agree.
pub fn resolve_cache_root() -> Result<PathBuf> {
    _rust_utils::uv::toolr_cache_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve toolr cache directory"))
}

pub fn dispatch(matches: &ArgMatches) -> Result<ExitCode> {
    let root = resolve_cache_root()?;
    match matches.subcommand() {
        Some(("list", _)) => {
            run_list(&root, &mut io::stdout().lock())?;
            Ok(ExitCode::SUCCESS)
        }
        Some(("prune", prune_m)) => crate::self_cache_prune::run(&root, prune_m),
        _ => Ok(ExitCode::from(2)),
    }
}

pub fn run_list(cache_root: &Path, out: &mut dyn Write) -> Result<()> {
    let mut caches = enumerate_caches(cache_root)?;
    caches.sort_by(|a, b| b.meta.last_used_at.cmp(&a.meta.last_used_at));

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

    let now = Utc::now();
    let rows: Vec<(String, String, String)> = caches
        .iter()
        .map(|c| {
            let repo = c.meta.repo_path.display().to_string();
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
pub fn human_ago(now: DateTime<Utc>, then: DateTime<Utc>) -> String {
    let delta = now.signed_duration_since(then);
    let secs = delta.num_seconds();
    if secs < 60 {
        "just now".into()
    } else if secs < 3600 {
        format!("{}m ago", delta.num_minutes())
    } else if secs < 86_400 {
        format!("{}h ago", delta.num_hours())
    } else if delta.num_days() < 30 {
        format!("{}d ago", delta.num_days())
    } else if delta.num_days() < 365 {
        format!("{}mo ago", delta.num_days() / 30)
    } else {
        format!("{}y ago", delta.num_days() / 365)
    }
}
