mod cli;
mod dispatch;
mod project;
mod self_cache;
mod self_cache_prune;

use std::process::ExitCode;

use _rust_utils::discovery::discover_project_root;
use _rust_utils::manifest::{Manifest, SCHEMA_VERSION, load_manifest};

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
    let manifest = load_or_empty(&cwd);
    let mut command = cli::build_command(&manifest);
    let matches = command.clone().get_matches();
    dispatch::dispatch(&matches, &manifest, &mut command)
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
