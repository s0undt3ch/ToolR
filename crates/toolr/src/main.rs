mod cli;
mod dispatch;
mod execute_build;
mod init_scaffold;
mod init_templates;
mod markdown;
mod project;
mod self_cache;
mod self_cache_prune;
mod value_parsers;

use std::process::ExitCode;

use toolr_core::discovery::discover_project_root;
use toolr_core::manifest::{Manifest, SCHEMA_VERSION, load_manifest};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("toolr: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    // Emit the passive cache hint before clap touches argv, so `--version`
    // and `--help` (which would otherwise exit inside clap) still see it.
    maybe_emit_cache_hint_from_argv();
    let manifest = load_or_empty(&cwd);
    let mut command = cli::build_command(&manifest);
    let matches = command.clone().get_matches();
    dispatch::dispatch(&matches, &manifest, &mut command)
}

fn maybe_emit_cache_hint_from_argv() {
    if std::env::var_os("TOOLR_NO_CACHE_HINT").is_some() {
        return;
    }
    // Suppress for tab-completion and `self cache ...` invocations.
    let argv: Vec<String> = std::env::args().collect();
    let positional: Vec<&str> = argv
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(String::as_str)
        .collect();
    if positional.first().copied() == Some("__complete") {
        return;
    }
    if positional.first().copied() == Some("self") && positional.get(1).copied() == Some("cache") {
        return;
    }
    let Ok(cache_root) = self_cache::resolve_cache_root() else {
        return;
    };
    let cfg = toolr_core::cache::HintConfig::default();
    if let Ok(Some(msg)) = toolr_core::cache::compute_hint(&cache_root, &cfg, chrono::Utc::now()) {
        eprintln!("{msg}");
    }
}

fn load_or_empty(cwd: &std::path::Path) -> Manifest {
    let Ok(root) = discover_project_root(cwd) else {
        return empty_manifest();
    };
    let manifest_path = root.join("tools").join(".toolr-manifest.json");
    load_manifest(&manifest_path).unwrap_or_else(|_| empty_manifest())
}

fn empty_manifest() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: String::new(),
        dynamic_hash: String::new(),
        groups: Vec::new(),
        commands: Vec::new(),
    }
}
